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
.\tools\dev.ps1 bootstrap
.\tools\dev.ps1 ci
```

`Cargo.lock` is committed. Use `--locked` for reproducible automation and do not update dependencies incidentally.

## Commands

| Purpose | Command |
|---|---|
| Format check | `.\tools\dev.ps1 format` |
| Clippy with warnings denied | `.\tools\dev.ps1 lint` |
| Unit and integration tests | `.\tools\dev.ps1 test` |
| Content schema and semantic validation | `.\tools\dev.ps1 validate` |
| Deterministic headless trace | `.\tools\dev.ps1 headless` |
| LocalLab client | `.\tools\dev.ps1 local-lab` |
| Windows release build | `.\tools\dev.ps1 release` |
| Full local CI equivalent | `.\tools\dev.ps1 ci` |

Direct Cargo aliases are also available: `cargo gb-format`, `cargo gb-lint`, `cargo gb-test`, and `cargo gb-release`.

## Runtime modes

- **LocalLab:** available in M00; `client_bevy` and `sim_core` share one process with ephemeral state.
- **Headless/Replay:** `cargo run -p tools_content -- trace tests/deterministic/m00_smoke.json`.
- **LocalStack:** intentionally unavailable until `server_app` arrives in M02 and PostgreSQL persistence arrives in M03. `.\tools\dev.ps1 local-stack` fails explicitly rather than running a behaviorally false substitute.

Gameplay rules must live in `sim_core` or validated content. `client_bevy` owns presentation only.

## Logging

Copy `.env.example` only when a future launcher requires a local environment file. For current commands, set `RUST_LOG` in the shell when needed:

```powershell
$env:RUST_LOG = 'info,client_bevy=debug,sim_core=debug,sim_content=debug'
```

Never log credentials, authentication tickets, account tokens, or future commerce secrets.
