# Development environment

This document implements `GB-M00-02` and `GB-M00-03`. The canonical architecture is GDD `TECH-001` through `TECH-006`; exact content rules remain in the Content Production Specification.

## Prerequisites

- Windows 10 or 11.
- Git.
- Rustup. The repository automatically selects Rust 1.95.0 with rustfmt and Clippy from `rust-toolchain.toml`.
- Visual Studio 2022 Build Tools with the Desktop development with C++ workload and a Windows SDK.

No database, container runtime, Steam SDK, or external service is required for GB-M00 or M01 LocalLab.

## Clean setup

```powershell
git clone https://github.com/MikeyPar/Gravebound.git
Set-Location Gravebound
.\tools\dev.cmd bootstrap
.\tools\dev.cmd ci
```

`Cargo.lock` is committed. Use `--locked` for reproducible automation and do not update dependencies incidentally.

## Commands

| Purpose | Command |
|---|---|
| Format check | `.\tools\dev.cmd format` |
| Clippy with warnings denied | `.\tools\dev.cmd lint` |
| Unit and integration tests | `.\tools\dev.cmd test` |
| Content schema and semantic validation | `.\tools\dev.cmd validate` |
| Regenerate checked-in JSON schemas | `cargo run --locked -p tools_content -- generate-schemas` |
| Deterministic headless trace | `.\tools\dev.cmd headless` |
| LocalLab client | `.\tools\dev.cmd local-lab` |
| Windows release build | `.\tools\dev.cmd release` |
| Full local CI equivalent | `.\tools\dev.cmd ci` |

Direct Cargo aliases are also available: `cargo gb-format`, `cargo gb-lint`, `cargo gb-test`, and `cargo gb-release`.

## Runtime modes

- **LocalLab:** available in M00; `client_bevy` and `sim_core` share one process with ephemeral state.
- **Headless/Replay:** `cargo run -p tools_content -- trace tests/deterministic/m00_smoke.json`.
- **LocalStack:** intentionally unavailable until `server_app` arrives in M02 and PostgreSQL persistence arrives in M03. `.\tools\dev.cmd local-stack` fails explicitly rather than running a behaviorally false substitute.

Gameplay rules must live in `sim_core` or validated content. `client_bevy` owns presentation only.
Generated files in `schemas/` are committed contracts; regenerate and review them whenever a Rust content type changes.

LocalLab resolves `GRAVEBOUND_CONTENT_ROOT` first, then `content/` under the working directory, then `content/` beside an ancestor of the executable. Invalid or missing content prevents the window from starting.

For a deterministic visual-review frame, set an unused output path before launching and close the window after the PNG appears:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\local-lab.png')
.\tools\dev.cmd local-lab
```

The capture is scheduled after sixty rendered frames and includes world and UI layers. It is encoded to a same-directory `*.partial.<format>` file, flushed to disk, and renamed only after the synchronous write finishes, so appearance of the requested path is the completion signal and the client can then be closed safely. Use an unused output path; an existing destination is not overwritten by the atomic publish. Committed representative evidence lives under `docs/evidence/`; generated files under `tmp/` remain ignored.

`GB-M01-02A` also provides one strict visual-evidence scenario. It is accepted only when a screenshot path is configured, injects held eastward primary fire without changing movement, and is not a gameplay mode:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\primary-fire.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'primary_fire_east'
.\tools\dev.cmd local-lab
```

Unknown scenario names and scenario use without `GRAVEBOUND_SCREENSHOT_PATH` fail at startup.

## Logging

Copy `.env.example` only when a future launcher requires a local environment file. For current commands, set `RUST_LOG` in the shell when needed:

```powershell
$env:RUST_LOG = 'info,client_bevy=debug,sim_core=debug,sim_content=debug'
```

Never log credentials, authentication tickets, account tokens, or future commerce secrets.
