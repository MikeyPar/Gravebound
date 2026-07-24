[CmdletBinding()]
param(
    [string]$Identity = 'local-m03-tester',
    [switch]$ServerSmokeOnly
)

$ErrorActionPreference = 'Stop'
$PackageRoot = (Resolve-Path $PSScriptRoot).Path
$RuntimeRoot = Join-Path $PackageRoot '.runtime'
$PortablePostgresRoot = Join-Path $PackageRoot 'runtime\postgresql'
$PortablePostgresBin = Join-Path $PortablePostgresRoot 'bin'
$PortablePostgresData = Join-Path $RuntimeRoot 'postgres-data'
$PortablePostgresLog = Join-Path $RuntimeRoot 'postgres.log'
$ComposeFile = Join-Path $PackageRoot 'm03-tester-postgres.yml'
$Client = Join-Path $PackageRoot 'Gravebound.exe'
$Server = Join-Path $PackageRoot 'GraveboundServer.exe'
$ContentRoot = Join-Path $PackageRoot 'content'
$ProjectName = 'gravebound-m03-tester'
$SecretFile = Join-Path $RuntimeRoot 'private-life.secrets.json'
$Certificate = Join-Path $RuntimeRoot 'server-cert.der'
$Readiness = Join-Path $RuntimeRoot 'server-ready.txt'
$ServerStdout = Join-Path $RuntimeRoot 'server.stdout.log'
$ServerStderr = Join-Path $RuntimeRoot 'server.stderr.log'
$ServerProcess = $null
$PostgresMode = $null
$PrimaryFailure = $null
$CleanupFailure = $null

function Read-OrCreate-LocalSecrets {
    New-Item -ItemType Directory -Force -Path $RuntimeRoot | Out-Null
    if (Test-Path -LiteralPath $SecretFile) {
        $Stored = Get-Content -LiteralPath $SecretFile -Raw | ConvertFrom-Json
        if (
            $Stored.postgres_password -notmatch '^[a-f0-9]{32}$' -or
            $Stored.reward_secret_hex -notmatch '^[a-f0-9]{64}$'
        ) {
            throw 'The local tester secret file is malformed. Delete .runtime and retry.'
        }
        return $Stored
    }

    $Stored = [pscustomobject]@{
        postgres_password = [Guid]::NewGuid().ToString('N')
        reward_secret_hex = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    }
    $Stored | ConvertTo-Json | Set-Content -LiteralPath $SecretFile -Encoding ascii
    try {
        $Acl = Get-Acl -LiteralPath $SecretFile
        $Acl.SetAccessRuleProtection($true, $false)
        $Rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.WindowsIdentity]::GetCurrent().Name,
            'FullControl',
            'Allow'
        )
        $Acl.SetAccessRule($Rule)
        Set-Acl -LiteralPath $SecretFile -AclObject $Acl
    }
    catch {
        Write-Warning 'Could not restrict the local tester secret file ACL. Keep this package private.'
    }
    return $Stored
}

function Read-ServerFailure {
    if (Test-Path -LiteralPath $ServerStderr) {
        return (Get-Content -LiteralPath $ServerStderr -Raw).Trim()
    }
    return ''
}

function Get-AvailableLoopbackPort {
    $Listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    try {
        $Listener.Start()
        return ([Net.IPEndPoint]$Listener.LocalEndpoint).Port
    }
    finally {
        $Listener.Stop()
    }
}

function Invoke-HiddenProcessAndWait {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    $ArgumentLine = (
        $Arguments |
            ForEach-Object {
                $Value = [string]$_
                if ($Value -match '[\s"]') {
                    '"' + $Value.Replace('"', '\"') + '"'
                }
                else {
                    $Value
                }
            }
    ) -join ' '
    $Process = Start-Process `
        -FilePath $FilePath `
        -ArgumentList $ArgumentLine `
        -WorkingDirectory $PackageRoot `
        -WindowStyle Hidden `
        -PassThru
    $Process.WaitForExit()
    return $Process.ExitCode
}

function Invoke-PortablePostgres {
    param([Parameter(Mandatory = $true)]$Secrets)

    $InitDb = Join-Path $PortablePostgresBin 'initdb.exe'
    $PgCtl = Join-Path $PortablePostgresBin 'pg_ctl.exe'
    $Psql = Join-Path $PortablePostgresBin 'psql.exe'
    $CreateDb = Join-Path $PortablePostgresBin 'createdb.exe'
    foreach ($RequiredTool in @($InitDb, $PgCtl, $Psql, $CreateDb)) {
        if (-not (Test-Path -LiteralPath $RequiredTool)) {
            throw "The bundled PostgreSQL runtime is incomplete: missing $RequiredTool"
        }
    }

    if (-not (Test-Path -LiteralPath (Join-Path $PortablePostgresData 'PG_VERSION'))) {
        Write-Host 'Preparing the private PostgreSQL data directory (first launch only)...'
        New-Item -ItemType Directory -Force -Path $PortablePostgresData | Out-Null
        $PasswordFile = Join-Path $RuntimeRoot 'postgres-password.txt'
        try {
            Set-Content -LiteralPath $PasswordFile -Value $Secrets.postgres_password -Encoding ascii
            & $InitDb `
                --pgdata $PortablePostgresData `
                --username gravebound_test `
                --pwfile $PasswordFile `
                --encoding UTF8 `
                --auth-host scram-sha-256 `
                --auth-local trust `
                --no-locale |
                ForEach-Object { Write-Host $_ }
            if ($LASTEXITCODE -ne 0) {
                throw "Bundled PostgreSQL initialization failed with exit code $LASTEXITCODE"
            }
        }
        finally {
            Remove-Item -LiteralPath $PasswordFile -Force -ErrorAction SilentlyContinue
        }
    }

    $Port = Get-AvailableLoopbackPort
    Write-Host "Starting the bundled private PostgreSQL service on loopback port $Port..."
    $PgCtlExitCode = Invoke-HiddenProcessAndWait `
        -FilePath $PgCtl `
        -Arguments @(
            '--pgdata', $PortablePostgresData,
            '--log', $PortablePostgresLog,
            '--options', "-h 127.0.0.1 -p $Port",
            '--wait',
            'start'
        )
    if ($PgCtlExitCode -ne 0) {
        throw "Bundled PostgreSQL startup failed with exit code $PgCtlExitCode. See $PortablePostgresLog"
    }
    Write-Host 'Bundled PostgreSQL is ready.'
    $script:PostgresMode = 'portable'
    $env:PGPASSWORD = $Secrets.postgres_password

    $DatabaseExistsOutput = & $Psql `
        --host 127.0.0.1 `
        --port $Port `
        --username gravebound_test `
        --dbname postgres `
        --tuples-only `
        --no-align `
        --command "SELECT 1 FROM pg_database WHERE datname = 'gravebound_test'"
    $DatabaseExists = ($DatabaseExistsOutput | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "Bundled PostgreSQL readiness check failed with exit code $LASTEXITCODE"
    }
    if ($DatabaseExists -ne '1') {
        & $CreateDb `
            --host 127.0.0.1 `
            --port $Port `
            --username gravebound_test `
            gravebound_test |
            ForEach-Object { Write-Host $_ }
        if ($LASTEXITCODE -ne 0) {
            throw "Bundled PostgreSQL database creation failed with exit code $LASTEXITCODE"
        }
    }

    return "postgres://gravebound_test:$($Secrets.postgres_password)@127.0.0.1:$Port/gravebound_test"
}

function Invoke-DockerPostgres {
    param([Parameter(Mandatory = $true)]$Secrets)

    if (-not (Test-Path -LiteralPath $ComposeFile)) {
        throw "The bundled PostgreSQL runtime and Docker Compose definition are both missing."
    }
    if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
        throw 'The bundled PostgreSQL runtime is missing and Docker Desktop is not installed. Re-extract the complete tester ZIP and retry.'
    }
    & docker info *> $null
    if ($LASTEXITCODE -ne 0) {
        throw 'The bundled PostgreSQL runtime is missing and Docker Desktop is not running. Re-extract the complete tester ZIP or start Docker Desktop.'
    }

    Write-Host 'Starting the Docker private PostgreSQL service...'
    & docker compose --project-name $ProjectName --file $ComposeFile up --detach --wait |
        ForEach-Object { Write-Host $_ }
    if ($LASTEXITCODE -ne 0) {
        throw "PostgreSQL startup failed with exit code $LASTEXITCODE"
    }
    $script:PostgresMode = 'docker'
    $Published = (& docker compose --project-name $ProjectName --file $ComposeFile port postgres 5432).Trim()
    if ($LASTEXITCODE -ne 0 -or $Published -notmatch ':(\d+)$') {
        throw 'Docker Compose did not report the local PostgreSQL port.'
    }
    return "postgres://gravebound_test:$($Secrets.postgres_password)@127.0.0.1:$($Matches[1])/gravebound_test"
}

try {
    if ($Identity -notmatch '^[A-Za-z0-9._-]{1,64}$') {
        throw 'Tester identity must contain 1-64 letters, numbers, dots, underscores, or dashes.'
    }
    foreach ($RequiredPath in @($Client, $Server, $ContentRoot)) {
        if (-not (Test-Path -LiteralPath $RequiredPath)) {
            throw "Tester package is incomplete: missing $RequiredPath"
        }
    }

    $Secrets = Read-OrCreate-LocalSecrets
    $env:GRAVEBOUND_POSTGRES_PASSWORD = $Secrets.postgres_password
    $env:GRAVEBOUND_REWARD_EPOCH_ID = 'm03-tester-v1'
    $env:GRAVEBOUND_REWARD_EPOCH_SECRET_HEX = $Secrets.reward_secret_hex

    if (Test-Path -LiteralPath (Join-Path $PortablePostgresBin 'postgres.exe')) {
        $env:GRAVEBOUND_DATABASE_URL = Invoke-PortablePostgres -Secrets $Secrets
    }
    else {
        $env:GRAVEBOUND_DATABASE_URL = Invoke-DockerPostgres -Secrets $Secrets
    }

    Remove-Item -LiteralPath $Readiness, $Certificate, $ServerStdout, $ServerStderr -Force -ErrorAction SilentlyContinue
    $ServerArguments = @(
        'serve-core-private-life',
        '--bind', '127.0.0.1:0',
        '--content-root', $ContentRoot,
        '--certificate-out', $Certificate,
        '--readiness-out', $Readiness
    )
    $ServerProcess = Start-Process `
        -FilePath $Server `
        -ArgumentList $ServerArguments `
        -WorkingDirectory $PackageRoot `
        -WindowStyle Hidden `
        -RedirectStandardOutput $ServerStdout `
        -RedirectStandardError $ServerStderr `
        -PassThru

    $Deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $Readiness)) {
        $ServerProcess.Refresh()
        if ($ServerProcess.HasExited) {
            throw "The M03 server failed during startup. $(Read-ServerFailure)"
        }
        if ([DateTime]::UtcNow -ge $Deadline) {
            throw "The M03 server did not become ready within 30 seconds. $(Read-ServerFailure)"
        }
        Start-Sleep -Milliseconds 200
    }

    $Address = (Get-Content -LiteralPath $Readiness -Raw).Trim()
    if ($Address -notmatch '^127\.0\.0\.1:\d+$') {
        throw 'The M03 server published an invalid local address.'
    }

    if ($ServerSmokeOnly) {
        Write-Host 'Bundled PostgreSQL and the persistent GB-M03 server are ready.'
    }
    else {
        Write-Host 'Launching the current persistent GB-M03 private-life route...'
        & $Client core-private-life --server $Address --certificate $Certificate --identity $Identity --content-root $ContentRoot
        if ($LASTEXITCODE -ne 0) {
            throw "Gravebound exited with code $LASTEXITCODE"
        }
    }
}
catch {
    $PrimaryFailure = $_
}
finally {
    if ($ServerProcess) {
        Start-Sleep -Seconds 2
        $ServerProcess.Refresh()
        if (-not $ServerProcess.HasExited) {
            Stop-Process -Id $ServerProcess.Id -Force
            $ServerProcess.WaitForExit()
        }
    }
    if ($PostgresMode -eq 'portable') {
        $PgCtl = Join-Path $PortablePostgresBin 'pg_ctl.exe'
        $PgCtlExitCode = Invoke-HiddenProcessAndWait `
            -FilePath $PgCtl `
            -Arguments @(
                '--pgdata', $PortablePostgresData,
                '--mode', 'fast',
                '--wait',
                'stop'
            )
        if ($PgCtlExitCode -ne 0) {
            $CleanupFailure = "Could not stop the bundled PostgreSQL service (exit $PgCtlExitCode)."
        }
    }
    elseif ($PostgresMode -eq 'docker' -and (Get-Command docker -ErrorAction SilentlyContinue)) {
        & docker compose --project-name $ProjectName --file $ComposeFile stop *> $null
        if ($LASTEXITCODE -ne 0) {
            $CleanupFailure = "Could not stop the local PostgreSQL service (exit $LASTEXITCODE)."
        }
    }
    Remove-Item Env:GRAVEBOUND_DATABASE_URL -ErrorAction SilentlyContinue
    Remove-Item Env:GRAVEBOUND_POSTGRES_PASSWORD -ErrorAction SilentlyContinue
    Remove-Item Env:GRAVEBOUND_REWARD_EPOCH_ID -ErrorAction SilentlyContinue
    Remove-Item Env:GRAVEBOUND_REWARD_EPOCH_SECRET_HEX -ErrorAction SilentlyContinue
    Remove-Item Env:PGPASSWORD -ErrorAction SilentlyContinue
}

if ($PrimaryFailure -and $CleanupFailure) {
    throw "$($PrimaryFailure.Exception.Message) Cleanup also failed: $CleanupFailure"
}
if ($PrimaryFailure) {
    throw $PrimaryFailure
}
if ($CleanupFailure) {
    throw $CleanupFailure
}
