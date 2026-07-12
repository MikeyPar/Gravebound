[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('bootstrap', 'format', 'lint', 'test', 'validate', 'headless', 'local-lab', 'server-doctor', 'bot-doctor', 'network-ci', 'local-stack', 'ci', 'release')]
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
            Invoke-Cargo -Arguments @('test', '--locked', '-p', 'protocol', '-p', 'server_app', '-p', 'bot_client')
            Invoke-Cargo -Arguments @('clippy', '--locked', '-p', 'protocol', '-p', 'server_app', '-p', 'bot_client', '--all-targets', '--', '-D', 'warnings')
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'server_app', '--', 'doctor')
            Invoke-Cargo -Arguments @('run', '--locked', '-p', 'bot_client', '--', 'doctor')
        }
        'local-stack' {
            throw 'LocalStack becomes runnable with GB-M02-00 (server_app) and GB-M03-02 (PostgreSQL). M00 intentionally provides no substitute server.'
        }
        'release' { Invoke-Cargo -Arguments @('build', '--locked', '--release', '-p', 'client_bevy') }
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
