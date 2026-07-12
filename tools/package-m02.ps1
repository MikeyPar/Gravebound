[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Destination = Join-Path $RepoRoot 'dist\Gravebound-M02-Playtest'

Push-Location $RepoRoot
try {
    & cargo build --locked --release -p server_app -p client_bevy
    if ($LASTEXITCODE -ne 0) {
        throw "M02 release build failed with exit code $LASTEXITCODE"
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $ContentDestination = Join-Path $Destination 'content'
    New-Item -ItemType Directory -Force -Path $ContentDestination | Out-Null
    $StaleCertificate = Join-Path $Destination 'server-cert.der'
    if (Test-Path -LiteralPath $StaleCertificate) {
        Remove-Item -LiteralPath $StaleCertificate -Force
    }
    Copy-Item -Force 'target\release\server_app.exe' $Destination
    Copy-Item -Force 'target\release\client_bevy.exe' $Destination
    Copy-Item -Recurse -Force 'content\*' $ContentDestination
    Copy-Item -Force 'tools\m02\Start Server.cmd' $Destination
    Copy-Item -Force 'tools\m02\Start Client 1.cmd' $Destination
    Copy-Item -Force 'tools\m02\Start Client 2.cmd' $Destination
    Copy-Item -Force 'tools\m02\Start Client 3.cmd' $Destination
    Copy-Item -Force 'tools\m02\Start Client 4.cmd' $Destination
    Copy-Item -Force 'docs\playtests\GB-M02-network-gate-runbook.md' (Join-Path $Destination 'PLAYTEST.md')
    Write-Host "M02 playtest package ready: $Destination"
}
finally {
    Pop-Location
}
