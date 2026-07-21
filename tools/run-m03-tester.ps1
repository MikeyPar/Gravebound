[CmdletBinding()]
param(
    [string]$Identity = 'local-m03-tester'
)

$ErrorActionPreference = 'Stop'
$PackageRoot = (Resolve-Path $PSScriptRoot).Path
$RuntimeRoot = Join-Path $PackageRoot '.runtime'
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

try {
    if ($Identity -notmatch '^[A-Za-z0-9._-]{1,64}$') {
        throw 'Tester identity must contain 1-64 letters, numbers, dots, underscores, or dashes.'
    }
    foreach ($RequiredPath in @($ComposeFile, $Client, $Server, $ContentRoot)) {
        if (-not (Test-Path -LiteralPath $RequiredPath)) {
            throw "Tester package is incomplete: missing $RequiredPath"
        }
    }
    if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
        throw 'PLAY GAME requires Docker Desktop for the local PostgreSQL service. Install/start Docker Desktop, then retry. PLAY LOCAL LAB.cmd remains available without Docker.'
    }

    & docker info *> $null
    if ($LASTEXITCODE -ne 0) {
        throw 'Docker Desktop is installed but is not running.'
    }

    $Secrets = Read-OrCreate-LocalSecrets
    $env:GRAVEBOUND_POSTGRES_PASSWORD = $Secrets.postgres_password
    $env:GRAVEBOUND_REWARD_EPOCH_ID = 'm03-tester-v1'
    $env:GRAVEBOUND_REWARD_EPOCH_SECRET_HEX = $Secrets.reward_secret_hex

    Write-Host 'Starting the private M03 PostgreSQL service...'
    & docker compose --project-name $ProjectName --file $ComposeFile up --detach --wait
    if ($LASTEXITCODE -ne 0) {
        throw "PostgreSQL startup failed with exit code $LASTEXITCODE"
    }
    $Published = (& docker compose --project-name $ProjectName --file $ComposeFile port postgres 5432).Trim()
    if ($LASTEXITCODE -ne 0 -or $Published -notmatch ':(\d+)$') {
        throw 'Docker Compose did not report the local PostgreSQL port.'
    }
    $env:GRAVEBOUND_DATABASE_URL = "postgres://gravebound_test:$($Secrets.postgres_password)@127.0.0.1:$($Matches[1])/gravebound_test"

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

    Write-Host 'Launching the current persistent GB-M03 private-life route...'
    & $Client core-private-life --server $Address --certificate $Certificate --identity $Identity --content-root $ContentRoot
    if ($LASTEXITCODE -ne 0) {
        throw "Gravebound exited with code $LASTEXITCODE"
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
    if (Get-Command docker -ErrorAction SilentlyContinue) {
        & docker compose --project-name $ProjectName --file $ComposeFile stop *> $null
        if ($LASTEXITCODE -ne 0) {
            $CleanupFailure = "Could not stop the local PostgreSQL service (exit $LASTEXITCODE)."
        }
    }
    Remove-Item Env:GRAVEBOUND_DATABASE_URL -ErrorAction SilentlyContinue
    Remove-Item Env:GRAVEBOUND_POSTGRES_PASSWORD -ErrorAction SilentlyContinue
    Remove-Item Env:GRAVEBOUND_REWARD_EPOCH_ID -ErrorAction SilentlyContinue
    Remove-Item Env:GRAVEBOUND_REWARD_EPOCH_SECRET_HEX -ErrorAction SilentlyContinue
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
