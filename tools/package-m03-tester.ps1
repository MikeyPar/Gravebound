[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$DistRoot = Join-Path $RepoRoot 'dist'
$StageRoot = Join-Path $RepoRoot "tmp\m03-tester-package-$PID"
$PackagePrefix = 'Gravebound-GB-M03-Tester'
$PackageRefreshFloor = 15
$PackageInputs = @(
    'Cargo.lock',
    'Cargo.toml',
    'rust-toolchain.toml',
    'rustfmt.toml',
    'assets',
    'content',
    'crates\client_bevy',
    'crates\content_schema',
    'crates\persistence',
    'crates\protocol',
    'crates\server_app',
    'crates\sim_content',
    'crates\sim_core',
    'migrations',
    'tools\m03-tester-postgres.yml',
    'tools\run-m03-tester.ps1'
)

function Assert-ChildPath {
    param(
        [Parameter(Mandatory = $true)][string]$Parent,
        [Parameter(Mandatory = $true)][string]$Child
    )

    $ParentPath = [IO.Path]::GetFullPath($Parent).TrimEnd(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    )
    $ChildPath = [IO.Path]::GetFullPath($Child)
    $ExpectedPrefix = $ParentPath + [IO.Path]::DirectorySeparatorChar
    if (-not $ChildPath.StartsWith($ExpectedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to modify a path outside '$ParentPath': $ChildPath"
    }
}

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][string]$Command,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Write-Launcher {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Arguments
    )

    @"
@echo off
setlocal
pushd "%~dp0"
"%~dp0Gravebound.exe" $Arguments
set "exit_code=%ERRORLEVEL%"
popd
if not "%exit_code%"=="0" pause
exit /b %exit_code%
"@ | Set-Content -LiteralPath $Path -Encoding ascii
}

function Write-PrivateLifeLauncher {
    param([Parameter(Mandatory = $true)][string]$Path)

    @"
@echo off
setlocal
pushd "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-m03-tester.ps1"
set "exit_code=%ERRORLEVEL%"
popd
if not "%exit_code%"=="0" pause
exit /b %exit_code%
"@ | Set-Content -LiteralPath $Path -Encoding ascii
}

function Test-LaunchMode {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$LogRoot,
        [ValidateRange(1, 30)][int]$HoldSeconds = 3
    )

    $SafeName = $Name -replace '[^A-Za-z0-9_-]', '-'
    $Stdout = Join-Path $LogRoot "$SafeName.stdout.log"
    $Stderr = Join-Path $LogRoot "$SafeName.stderr.log"
    $Process = Start-Process `
        -FilePath $Executable `
        -ArgumentList $Arguments `
        -WorkingDirectory $WorkingDirectory `
        -WindowStyle Hidden `
        -RedirectStandardOutput $Stdout `
        -RedirectStandardError $Stderr `
        -PassThru
    try {
        Start-Sleep -Seconds $HoldSeconds
        $Process.Refresh()
        if ($Process.HasExited) {
            $ErrorText = if (Test-Path -LiteralPath $Stderr) {
                (Get-Content -LiteralPath $Stderr -Raw).Trim()
            }
            else {
                ''
            }
            throw "Launch mode '$Name' exited during smoke verification. $ErrorText"
        }
        if (-not $Process.Responding) {
            throw "Launch mode '$Name' stopped responding during smoke verification."
        }
    }
    finally {
        if (-not $Process.HasExited) {
            Stop-Process -Id $Process.Id -Force
            $Process.WaitForExit()
        }
    }

    if ((Test-Path -LiteralPath $Stderr) -and (Get-Item -LiteralPath $Stderr).Length -ne 0) {
        $ErrorText = (Get-Content -LiteralPath $Stderr -Raw).Trim()
        throw "Launch mode '$Name' wrote to stderr during smoke verification. $ErrorText"
    }
}

Push-Location $RepoRoot
try {
    New-Item -ItemType Directory -Force -Path $DistRoot | Out-Null
    Assert-ChildPath -Parent $RepoRoot -Child $DistRoot
    Assert-ChildPath -Parent (Join-Path $RepoRoot 'tmp') -Child $StageRoot

    & git diff --quiet -- @PackageInputs
    $TrackedWorktreeStatus = $LASTEXITCODE
    & git diff --cached --quiet -- @PackageInputs
    $TrackedIndexStatus = $LASTEXITCODE
    if ($TrackedWorktreeStatus -gt 1 -or $TrackedIndexStatus -gt 1) {
        throw 'Unable to inspect tester-package source state.'
    }
    if ($TrackedWorktreeStatus -eq 1 -or $TrackedIndexStatus -eq 1) {
        throw 'Commit or revert tracked tester-package input changes before packaging.'
    }

    $Revision = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or -not $Revision) {
        throw 'Unable to resolve the repository source revision.'
    }

    $BuildDate = Get-Date -Format 'yyyy-MM-dd'
    $ExistingRefreshes = Get-ChildItem -LiteralPath $DistRoot -Force |
        ForEach-Object {
            if ($_.Name -match "^$([regex]::Escape($PackagePrefix))-\d{4}-\d{2}-\d{2}-r(?<refresh>\d+)(?:\.zip)?$") {
                [int]$Matches.refresh
            }
        }
    $Refresh = if ($ExistingRefreshes) {
        [Math]::Max(
            $PackageRefreshFloor,
            1 + ($ExistingRefreshes | Measure-Object -Maximum).Maximum
        )
    }
    else {
        $PackageRefreshFloor
    }
    $PackageName = "$PackagePrefix-$BuildDate-r$Refresh"
    $StagePackage = Join-Path $StageRoot $PackageName
    $StageZip = Join-Path $StageRoot "$PackageName.zip"
    $SourceArchive = Join-Path $StageRoot 'source.zip'
    $PayloadArchive = Join-Path $StageRoot 'payload.zip'
    $SmokeLogs = Join-Path $StageRoot 'smoke'

    if (Test-Path -LiteralPath $StageRoot) {
        Remove-Item -LiteralPath $StageRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $StagePackage, $SmokeLogs | Out-Null

    Invoke-Checked -Command cargo -Arguments @('build', '--locked', '--release', '-p', 'client_bevy', '-p', 'server_app')
    Invoke-Checked -Command cargo -Arguments @('run', '--locked', '-p', 'tools_content', '--', 'validate')

    Copy-Item -LiteralPath 'target\release\client_bevy.exe' -Destination (Join-Path $StagePackage 'Gravebound.exe')
    Copy-Item -LiteralPath 'target\release\server_app.exe' -Destination (Join-Path $StagePackage 'GraveboundServer.exe')
    Invoke-Checked -Command git -Arguments @('archive', '--format=zip', "--output=$PayloadArchive", 'HEAD', 'assets', 'content')
    Expand-Archive -LiteralPath $PayloadArchive -DestinationPath $StagePackage
    Copy-Item -LiteralPath 'tools\m03-tester-postgres.yml' -Destination $StagePackage
    Copy-Item -LiteralPath 'tools\run-m03-tester.ps1' -Destination $StagePackage

    Write-PrivateLifeLauncher -Path (Join-Path $StagePackage 'PLAY GAME.cmd')
    Write-Launcher -Path (Join-Path $StagePackage 'PLAY LOCAL LAB.cmd') -Arguments 'local-lab'
    Write-Launcher -Path (Join-Path $StagePackage 'M03 HALL PREVIEW.cmd') -Arguments 'core-world-showcase --scene hall --content-root content'
    Write-Launcher -Path (Join-Path $StagePackage 'M03 DUNGEON PREVIEW.cmd') -Arguments 'core-encounter-showcase --content-root content'
    Write-Launcher -Path (Join-Path $StagePackage 'M03 BOSS PREVIEW.cmd') -Arguments 'core-caldus-showcase --content-root content --state phase-one'
    Write-Launcher -Path (Join-Path $StagePackage 'M03 ITEMS AND VAULT PREVIEW.cmd') -Arguments 'core-item-lifecycle-showcase --content-root content'
    Write-Launcher -Path (Join-Path $StagePackage 'M03 DEATH AND MEMORIAL PREVIEW.cmd') -Arguments 'core-death-view-showcase --content-root content --state summary'
    Write-Launcher -Path (Join-Path $StagePackage 'M03 SUCCESSOR RECOVERY PREVIEW.cmd') -Arguments 'core-successor-recovery-showcase --content-root content --state death-summary'

    Invoke-Checked -Command git -Arguments @('archive', '--format=zip', "--output=$SourceArchive", 'HEAD')
    $SourceArchiveHash = (Get-FileHash -LiteralPath $SourceArchive -Algorithm SHA256).Hash
    $Executable = Join-Path $StagePackage 'Gravebound.exe'
    $ServerExecutable = Join-Path $StagePackage 'GraveboundServer.exe'
    $ExecutableHash = (Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash
    $ServerExecutableHash = (Get-FileHash -LiteralPath $ServerExecutable -Algorithm SHA256).Hash

    @"
GRAVEBOUND - GB-M03 TESTER BUILD
Build date: $BuildDate (refresh $Refresh)
Repository source revision: $Revision
Source archive SHA-256: $SourceArchiveHash
Build profile: optimized Windows release
Executable SHA-256: $ExecutableHash
Server executable SHA-256: $ServerExecutableHash

START HERE
Double-click PLAY GAME.cmd for the current persistent Character Select -> Lantern Hall ->
private danger -> Bell Sepulcher -> extraction/death -> Hall/successor route. This requires
Docker Desktop for the package's private local PostgreSQL service. The wipeable tester data
survives between launches in a local Docker volume. No Steam runtime is required.

PLAY LOCAL LAB.cmd remains a dependency-free combat sandbox. Opening Gravebound.exe
directly also launches that Local Lab.

LOCAL LAB CONTROLS
- Move: WASD
- Aim: mouse
- Primary attack: hold left mouse button
- Slipstep: Space
- Tonics: Q and E
- Inventory: I
- Accessibility panel: F6

CURRENT M03 ROUTE AND PREVIEWS
PLAY GAME.cmd runs the normal authenticated QUIC route implemented so far, including
durable identity, Hall/danger traversal, fixed Bell rooms and Sir Caldus, extraction,
Emergency Recall, lethal death, Memorial, and successor recovery.

The other launchers expose implemented M03 surfaces for direct review: Lantern Halls,
the Core dungeon encounters, Sir Caldus, item/Vault state, durable death/Memorial, and
the two-confirmation successor recovery handoff. Those launchers are isolated previews.

PACKAGE RULE
Keep both executables, the PowerShell runner, Compose file, content, and assets together.
Moving only the EXE will make strict content or asset validation fail at startup.

USEFUL TEST NOTES
- Please record which launcher you used when reporting a problem.
- Include a screenshot and the exact action immediately before the issue when possible.
- PLAY GAME uses the opaque local identity `local-m03-tester`. Delete the package-local
  .runtime folder only if its generated local secrets become corrupt.
- The external private-cohort, Steamworks, hosting rehearsal, and user-deferred full audit
  are not represented as complete by this tester build.

BUILD VERIFICATION
- Optimized Windows client/server release compilation: PASS
- Client/server executable CLI smoke check: PASS
- Local Lab twelve-second launch/responding check: PASS
- All seven standalone packaged launch modes remained alive and responsive: PASS
- Strict compiled content validation: PASS
- Startup stderr for all seven launch modes: EMPTY

DESIGN AUTHORITIES
- Gravebound_Production_GDD_v1_Canonical.md
- Gravebound_Content_Production_Spec_v1.md
- Gravebound_Development_Roadmap_v1.md
"@ | Set-Content -LiteralPath (Join-Path $StagePackage 'TESTING.txt') -Encoding ascii

    & $Executable --help *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "Packaged executable CLI smoke check failed with exit code $LASTEXITCODE"
    }
    & $ServerExecutable --help *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "Packaged server executable CLI smoke check failed with exit code $LASTEXITCODE"
    }

    $LaunchModes = @(
        @{ Name = 'Local Lab'; Arguments = @('local-lab'); HoldSeconds = 12 },
        @{ Name = 'Hall'; Arguments = @('core-world-showcase', '--scene', 'hall', '--content-root', 'content'); HoldSeconds = 3 },
        @{ Name = 'Dungeon'; Arguments = @('core-encounter-showcase', '--content-root', 'content'); HoldSeconds = 3 },
        @{ Name = 'Boss'; Arguments = @('core-caldus-showcase', '--content-root', 'content', '--state', 'phase-one'); HoldSeconds = 3 },
        @{ Name = 'Items and Vault'; Arguments = @('core-item-lifecycle-showcase', '--content-root', 'content'); HoldSeconds = 3 },
        @{ Name = 'Death and Memorial'; Arguments = @('core-death-view-showcase', '--content-root', 'content', '--state', 'summary'); HoldSeconds = 3 },
        @{ Name = 'Successor Recovery'; Arguments = @('core-successor-recovery-showcase', '--content-root', 'content', '--state', 'death-summary'); HoldSeconds = 3 }
    )
    foreach ($LaunchMode in $LaunchModes) {
        Test-LaunchMode `
            -Executable $Executable `
            -WorkingDirectory $StagePackage `
            -Arguments $LaunchMode.Arguments `
            -Name $LaunchMode.Name `
            -LogRoot $SmokeLogs `
            -HoldSeconds $LaunchMode.HoldSeconds
    }

    Compress-Archive -LiteralPath $StagePackage -DestinationPath $StageZip -CompressionLevel Optimal
    $ZipHash = (Get-FileHash -LiteralPath $StageZip -Algorithm SHA256).Hash

    $DestinationPackage = Join-Path $DistRoot $PackageName
    $DestinationZip = Join-Path $DistRoot "$PackageName.zip"
    Assert-ChildPath -Parent $DistRoot -Child $DestinationPackage
    Assert-ChildPath -Parent $DistRoot -Child $DestinationZip
    Move-Item -LiteralPath $StagePackage -Destination $DestinationPackage
    Move-Item -LiteralPath $StageZip -Destination $DestinationZip

    Get-ChildItem -LiteralPath $DistRoot -Force |
        Where-Object {
            $_.Name -like "$PackagePrefix-*" -and
            $_.Name -ne $PackageName -and
            $_.Name -ne "$PackageName.zip"
        } |
        ForEach-Object {
            Assert-ChildPath -Parent $DistRoot -Child $_.FullName
            Remove-Item -LiteralPath $_.FullName -Recurse -Force
        }

    Write-Host "M03 tester package ready: $DestinationPackage"
    Write-Host "M03 tester archive ready: $DestinationZip"
    Write-Host "Archive SHA-256: $ZipHash"
}
finally {
    Pop-Location
    if (Test-Path -LiteralPath $StageRoot) {
        Assert-ChildPath -Parent (Join-Path $RepoRoot 'tmp') -Child $StageRoot
        Remove-Item -LiteralPath $StageRoot -Recurse -Force
    }
}
