[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$priorUrl = $env:GRAVEBOUND_DATABASE_URL
$priorPassword = $env:GRAVEBOUND_POSTGRES_PASSWORD
$ownsContainer = $false
$projectName = "gravebound-local-stack-$PID-$([Guid]::NewGuid().ToString('N').Substring(0, 8))"
$primaryFailure = $null
$cleanupFailure = $null

try {
    if (-not $env:GRAVEBOUND_DATABASE_URL) {
        if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
            throw 'LocalStack requires Docker Desktop or GRAVEBOUND_DATABASE_URL pointing to PostgreSQL. SQLite is prohibited.'
        }
        $env:GRAVEBOUND_POSTGRES_PASSWORD = [Guid]::NewGuid().ToString('N')
        $ownsContainer = $true
        & docker compose --project-name $projectName --file (Join-Path $workspace 'docker-compose.persistence.yml') up --detach --wait
        if ($LASTEXITCODE -ne 0) {
            throw "LocalStack PostgreSQL startup failed with exit code $LASTEXITCODE"
        }
        $published = (& docker compose --project-name $projectName --file (Join-Path $workspace 'docker-compose.persistence.yml') port postgres 5432).Trim()
        if ($LASTEXITCODE -ne 0 -or $published -notmatch ':(\d+)$') {
            throw 'Docker Compose did not report the LocalStack PostgreSQL port'
        }
        $env:GRAVEBOUND_DATABASE_URL = "postgres://gravebound_test:$($env:GRAVEBOUND_POSTGRES_PASSWORD)@127.0.0.1:$($Matches[1])/gravebound_test"
    }

    Write-Host 'Starting durable Core identity LocalStack. Run .\tools\dev.cmd m03-identity-client in another terminal.'
    & cargo run --locked -p server_app -- serve-core-identity
    if ($LASTEXITCODE -ne 0) {
        throw "Durable Core identity server failed with exit code $LASTEXITCODE"
    }
}
catch {
    $primaryFailure = $_
}

if ($ownsContainer) {
    & docker compose --project-name $projectName --file (Join-Path $workspace 'docker-compose.persistence.yml') down --volumes
    if ($LASTEXITCODE -ne 0) {
        $cleanupFailure = "LocalStack cleanup failed with exit code $LASTEXITCODE for project $projectName"
    }
}
$env:GRAVEBOUND_DATABASE_URL = $priorUrl
$env:GRAVEBOUND_POSTGRES_PASSWORD = $priorPassword

if ($primaryFailure -and $cleanupFailure) {
    throw "$($primaryFailure.Exception.Message) Cleanup also failed: $cleanupFailure"
}
if ($primaryFailure) {
    throw $primaryFailure
}
if ($cleanupFailure) {
    throw $cleanupFailure
}
