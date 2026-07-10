[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('bootstrap', 'format', 'lint', 'test', 'validate', 'headless', 'local-lab', 'local-stack', 'ci', 'release')]
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
            Invoke-Cargo fetch --locked
            Invoke-Cargo run --locked -p tools_content -- doctor
        }
        'format' { Invoke-Cargo fmt --all -- --check }
        'lint' { Invoke-Cargo clippy --workspace --all-targets -- -D warnings }
        'test' { Invoke-Cargo test --workspace }
        'validate' { Invoke-Cargo run --locked -p tools_content -- validate }
        'headless' { Invoke-Cargo run --locked -p tools_content -- trace --fixture tests/deterministic/m00_smoke.json }
        'local-lab' { Invoke-Cargo run --locked -p client_bevy }
        'local-stack' {
            throw 'LocalStack becomes runnable with GB-M02-00 (server_app) and GB-M03-02 (PostgreSQL). M00 intentionally provides no substitute server.'
        }
        'release' { Invoke-Cargo build --locked --release -p client_bevy }
        'ci' {
            Invoke-Cargo fmt --all -- --check
            Invoke-Cargo clippy --workspace --all-targets -- -D warnings
            Invoke-Cargo test --workspace
        }
    }
}
finally {
    Pop-Location
}

