[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$priorUrl = $env:TEST_DATABASE_URL
$priorPassword = $env:GRAVEBOUND_POSTGRES_PASSWORD
$priorOptIn = $env:GRAVEBOUND_ALLOW_DESTRUCTIVE_DATABASE_TESTS
$ownsContainer = $false
$projectName = "gravebound-persistence-$PID-$([Guid]::NewGuid().ToString('N').Substring(0, 8))"
$primaryFailure = $null
$cleanupFailure = $null

function Invoke-PersistenceTests {
    & cargo test --locked -p persistence --test postgres_foundation -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) {
        throw "PostgreSQL persistence tests failed with exit code $LASTEXITCODE"
    }
    & cargo test --locked -p server_app --test postgres_progression_restore -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) {
        throw "PostgreSQL progression restore tests failed with exit code $LASTEXITCODE"
    }
    & cargo test --locked -p server_app --test postgres_ash_wallet -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) {
        throw "PostgreSQL Ash wallet tests failed with exit code $LASTEXITCODE"
    }
    & cargo test --locked -p server_app --test postgres_identity -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) {
        throw "PostgreSQL identity tests failed with exit code $LASTEXITCODE"
    }
}

try {
    if ($env:TEST_DATABASE_URL) {
        Invoke-PersistenceTests
    }
    elseif (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
        throw 'Real PostgreSQL is required. Install Docker Desktop or set TEST_DATABASE_URL to a disposable PostgreSQL database; SQLite and skipped tests are prohibited.'
    }
    else {
        $env:GRAVEBOUND_POSTGRES_PASSWORD = [Guid]::NewGuid().ToString('N')
        $env:GRAVEBOUND_ALLOW_DESTRUCTIVE_DATABASE_TESTS = '1'
        $ownsContainer = $true
        & docker compose --project-name $projectName --file (Join-Path $workspace 'docker-compose.persistence.yml') up --detach --wait
        if ($LASTEXITCODE -ne 0) {
            throw "Docker Compose PostgreSQL startup failed with exit code $LASTEXITCODE"
        }
        $published = (& docker compose --project-name $projectName --file (Join-Path $workspace 'docker-compose.persistence.yml') port postgres 5432).Trim()
        if ($LASTEXITCODE -ne 0 -or $published -notmatch ':(\d+)$') {
            throw 'Docker Compose did not report the ephemeral PostgreSQL port'
        }
        $env:TEST_DATABASE_URL = "postgres://gravebound_test:$($env:GRAVEBOUND_POSTGRES_PASSWORD)@127.0.0.1:$($Matches[1])/gravebound_test"
        Invoke-PersistenceTests
    }
}
catch {
    $primaryFailure = $_
}

if ($ownsContainer) {
    & docker compose --project-name $projectName --file (Join-Path $workspace 'docker-compose.persistence.yml') down --volumes
    if ($LASTEXITCODE -ne 0) {
        $cleanupFailure = "Docker Compose cleanup failed with exit code $LASTEXITCODE for project $projectName"
    }
}

try {
    $env:TEST_DATABASE_URL = $priorUrl
    $env:GRAVEBOUND_POSTGRES_PASSWORD = $priorPassword
    $env:GRAVEBOUND_ALLOW_DESTRUCTIVE_DATABASE_TESTS = $priorOptIn
}
catch {
    if (-not $cleanupFailure) {
        $cleanupFailure = 'Failed to restore PostgreSQL test environment variables'
    }
}

if ($primaryFailure -and $cleanupFailure) {
    throw "$($primaryFailure.Exception.Message) Cleanup also failed: $cleanupFailure"
}
if ($primaryFailure) {
    throw $primaryFailure
}
if ($cleanupFailure) {
    throw $cleanupFailure
}
