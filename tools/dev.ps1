[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('bootstrap', 'format', 'lint', 'test', 'validate', 'headless', 'local-lab', 'server-doctor', 'bot-doctor', 'network-ci', 'm02-network-smoke', 'm02-soak', 'm02-server', 'm02-client', 'm02-package', 'local-stack', 'ci', 'release')]
    [string]$Command = 'ci'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $RepoRoot

function Invoke-Cargo {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

try {
    switch ($Command) {
        'bootstrap' {
            rustup show active-toolchain
            Invoke-Cargo -Arguments @('fetch', '--locked')
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'tools_content', '--', 'doctor')
        }
        'format' { Invoke-Cargo -Arguments @('fmt', '--all', '--', '--check') }
        'lint' { Invoke-Cargo -Arguments @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings') }
        'test' { Invoke-Cargo -Arguments @('test', '--workspace', '--locked') }
        'validate' { Invoke-Cargo -Arguments @('run', '--locked', '-p', 'tools_content', '--', 'validate') }
        'headless' { Invoke-Cargo -Arguments @('run', '--locked', '-p', 'tools_content', '--', 'trace', 'tests/deterministic/m00_smoke.json') }
        'local-lab' { Invoke-Cargo -Arguments @('run', '--locked', '-p', 'client_bevy') }
        'server-doctor' { Invoke-Cargo -Arguments @('run', '--locked', '-p', 'server_app', '--', 'doctor') }
        'bot-doctor' { Invoke-Cargo -Arguments @('run', '--locked', '-p', 'bot_client', '--', 'doctor') }
        'network-ci' {
            Invoke-Cargo -Arguments @('test', '--locked', '-p', 'protocol', '-p', 'network_harness', '-p', 'server_app', '-p', 'bot_client')
            Invoke-Cargo -Arguments @('clippy', '--locked', '-p', 'protocol', '-p', 'network_harness', '-p', 'server_app', '-p', 'bot_client', '--all-targets', '--', '-D', 'warnings')
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'server_app', '--', 'doctor')
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'bot_client', '--', 'doctor')
        }
        'm02-soak' {
            Invoke-Cargo -Arguments @('test', '--locked', '--release', '-p', 'server_app', '--test', 'm02_soak', 'm02_sixteen_bot_two_hour_soak', '--', '--ignored', '--nocapture', '--test-threads=1')
        }
        'm02-network-smoke' {
            Invoke-Cargo -Arguments @('test', '--locked', '-p', 'server_app', '--lib', 'runtime::tests', '--', '--nocapture')
            Invoke-Cargo -Arguments @('test', '--locked', '-p', 'client_bevy', '--lib', 'network_', '--', '--nocapture')
        }
        'm02-server' {
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'server_app', '--', 'serve')
        }
        'm02-client' {
            $player = if ($env:GRAVEBOUND_LOCAL_PLAYER) { $env:GRAVEBOUND_LOCAL_PLAYER } else { 'local-player-1' }
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'client_bevy', '--', 'network', '--player', $player)
        }
        'm02-package' {
            & (Join-Path $PSScriptRoot 'package-m02.ps1')
            if ($LASTEXITCODE -ne 0) {
                throw "M02 packaging failed with exit code $LASTEXITCODE"
            }
        }
        'local-stack' {
            throw 'The M02 authoritative loop is verified by network-ci and m02-soak. Runnable LocalStack remains blocked on GB-M03-02 PostgreSQL and its approved persistence boundary.'
        }
        'release' { Invoke-Cargo -Arguments @('build', '--locked', '--release', '-p', 'client_bevy', '-p', 'server_app') }
        'ci' {
            Invoke-Cargo -Arguments @('fmt', '--all', '--', '--check')
            Invoke-Cargo -Arguments @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
            Invoke-Cargo -Arguments @('test', '--workspace', '--locked')
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'tools_content', '--', 'validate')
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'tools_content', '--', 'trace', 'tests/deterministic/m00_smoke.json')
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'tools_content', '--', 'trace', 'tests/deterministic/m00_smoke.json')
        }
    }
}
finally {
    Pop-Location
}
