# Development environment

This document implements `GB-M00-02` and `GB-M00-03`. The canonical architecture is GDD `TECH-001` through `TECH-006`; exact content rules remain in the Content Production Specification.

## Opt-in local playtest telemetry

Ordinary LocalLab runs collect nothing. To enable the `GB-M01-10B` live adapter, the operator must set every value below before launch:

```text
GRAVEBOUND_TELEMETRY_CONSENT=1
GRAVEBOUND_TELEMETRY_TESTER_ID=tester-<16 lowercase hex>
GRAVEBOUND_TELEMETRY_SESSION_ID=session-<16 lowercase hex>
GRAVEBOUND_TELEMETRY_COHORT=eligible_blind|excluded_feature_contributor|excluded_incomplete_consent
GRAVEBOUND_TELEMETRY_GENRE_FAMILIARITY=new_to_both|action_rpg_only|bullet_hell_only|action_rpg_and_bullet_hell
GRAVEBOUND_TELEMETRY_OUTPUT=<local .jsonl path>
```

The adapter writes only on app exit, publishes atomically, uses account sentinel `local_lab_no_account`, and fails launch on incomplete or malformed consent metadata. Developer-tool sessions are marked metric-ineligible. No name, email, account, IP, unrestricted transcript, or remote endpoint exists.

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
| M02 server foundation diagnostics | `.\tools\dev.cmd server-doctor` |
| M02 bot foundation diagnostics | `.\tools\dev.cmd bot-doctor` |
| M02 protocol/server/bot focused gate | `.\tools\dev.cmd network-ci` |
| Windows release build | `.\tools\dev.cmd release` |
| Full local CI equivalent | `.\tools\dev.cmd ci` |

Direct Cargo aliases are also available: `cargo gb-format`, `cargo gb-lint`, `cargo gb-test`, and `cargo gb-release`.

## Runtime modes

- **LocalLab:** available in M00; `client_bevy` and `sim_core` share one process with ephemeral state.
- **Headless/Replay:** `cargo run -p tools_content -- trace tests/deterministic/m00_smoke.json`.
- **LocalStack:** intentionally unavailable until M02 completes real transport/session/authority and M03 supplies PostgreSQL persistence. `.\tools\dev.cmd local-stack` fails explicitly rather than running a behaviorally false substitute.
- **Server foundation:** `.\tools\dev.cmd server-doctor` validates the M02 crate boundary, shared 30 Hz simulation contract, and canonical protocol rates. It reports transport and database as disabled until their owning packages are complete.
- **Bot foundation:** `.\tools\dev.cmd bot-doctor` validates the headless client boundary and shared protocol/simulation cadence. It reports transport and journey execution as disabled until `GB-M02-01` and `GB-M02-07` respectively.

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

`GB-M01-02B` adds a deterministic collision showcase. It fires first toward the west shell and then toward the nearest nondamageable debug enemy. Capture is not requested until authoritative diagnostics have recorded at least one solid block and one enemy hit; the renderer then settles for 60 complete presentation frames before atomic publication:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\collision-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'collision_showcase'
.\tools\dev.cmd local-lab
```

The accepted frame must show `ENEMY HITS 1` or greater, `SOLID BLOCKS 1` or greater, the stable last target, circular hurtbox geometry, distinct enemy/solid contact shapes, and `DAMAGE DEFERRED`. This scenario does not apply damage or create a production enemy.

`GB-M01-02C` adds a Grave Mark showcase. It fires Grave Mark and a same-tick later-ID primary eastward so the stable target receives the mark before the primary intent resolves. Capture waits for both the Mark hit and marked-primary intent, then settles 60 presentation frames:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\grave-mark-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'grave_mark_showcase'
.\tools\dev.cmd local-lab
```

The accepted frame must show a dual-ring active mark, `MARK HITS 1` or greater, `+15% PRIMARY INTENTS 1` or greater, stable target/ticks, raw intent `23`, cooldown/GCD state, and `HEALTH UNCHANGED`. The showcase never mutates target health.

`GB-M01-02D` adds a Slipstep showcase. It activates Ability 2 with eastward movement on the first fixed tick and emits a same-tick primary. The player stops after the authored two-tile travel while the empowered bolt contacts two aligned nondamageable debug targets. Capture waits for one cast, one empowered shot, and two piercing enemy contacts, then settles 12 presentation frames so the trail and active Exhaustion remain visible:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\slipstep-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'slipstep_showcase'
.\tools\dev.cmd local-lab
```

The accepted frame must show position `(6.00,12.00)` from the `(4.00,12.00)` spawn, five cyan/luminance-distinct travel samples (the terminal sample overlaps the player), two distinct enemy contact glyphs, `CASTS 1`, `SHOTS 1`, `PIERCE HITS 2`, active cooldown/Exhaustion state, and `HEALTH UNCHANGED`. The scenario's reduced 12-frame settle applies only after semantic readiness and does not alter simulation timing.

`GB-M01-02E` adds a Stillness showcase. It holds primary fire without movement until the exact eighteenth post-movement sample grants Focused, then waits for a Focused projectile emission before capture:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\stillness-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'stillness_showcase'
.\tools\dev.cmd local-lab
```

The accepted frame must show `FOCUSED +10% SPD / +8% DMG`, at least one gain and Focused shot, the gold/teal shape-distinct Focused bolt treatment, stable collision diagnostics, and `HEALTH UNCHANGED`.

`GB-M01-11` adds a Red Tonic showcase. It starts the local player at 70/128 health, emits one semantic Q press, and captures only after an accepted use and at least four authoritative restore ticks:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\red-tonic-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'red_tonic_showcase'
.\tools\dev.cmd local-lab
```

The accepted frame must show `Q RED TONIC x1`, health above 70/128, active restore and cooldown tick counts, a nonzero heal delta, `DRINK CONFIRMED`, and the confirmation-cue counter. The HUD must stay outside the central aiming corridor.

`GB-M01-03A-C` adds a three-role enemy showcase using the strict compiled Drowned Pilgrim, Bell Reed, and Chain Sentry definitions. The capture waits until the real fan, gap ring, and lane have each dealt canonical damage to the shared player health state:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\enemy-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'enemy_showcase'
.\tools\dev.cmd local-lab
```

The accepted frame must show three distinct enemy silhouettes, coral Physical fan bolts, purple diamond Veil ring bolts with a navigable gap, the Sentry's two-axis lane shape, `FAN/RING/LANES` event and hit counters, reduced player health, and `HOSTILE VITALS ACTIVE`. Legacy kit evidence scenarios suppress this coordinator so later hostile ticks cannot contaminate their fixed captures.

`GB-M01-06A` adds the local death/restart transaction scenario. It starts at 8/128 health, accepts a real lethal hostile hit, holds the old run frozen for three fixed ticks, and invokes the same explicit Run Again action exposed on `R`:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\death-restart-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'death_restart_showcase'
.\tools\dev.cmd local-lab
```

The accepted frame must show run 2 at full health with active control, the default seed, exact three starter equipment items/two Tonics, a nonzero death ID, frozen-tick count, cleanup census, retained lethal trace, retired old-run IDs, and measured reconstruction below 3000 ms. An incomplete GPU composite is rejected even when semantic readiness passed.

The `GB-M01-06B` death-recap scenario stops at the frozen boundary instead of automatically restarting:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\death-recap-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'death_recap_showcase'
.\tools\dev.cmd local-lab
```

The frame must bind killer, attack, damage/type/source, recent timeline, local network mode, exact Lost cleanup, `Preserved NONE`, `Created NONE`, and primary `[R] RUN AGAIN` from the committed death snapshot. Boss-victory summary evidence remains unavailable until the real Bell Proctor clear state is promoted.

`GB-M01-07A` provides an inventory integration scenario that applies a canonical Still Eye field Take through the live run inventory and displays the result in the `[I]` overlay:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\inventory-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'inventory_showcase'
.\tools\dev.cmd local-lab
```

The field frame must show four equipment slots, eight backpack indices, two-Tonic belt state, explicit/no-silent-destruction policy, lowest-index behavior, and Still Eye in backpack index 1. Reward-panel evidence is tracked separately and is required before 07A promotion.

`GB-M01-07B` provides the four-item executable catalog/reward matrix:

```powershell
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\item-catalog-showcase.png')
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'item_catalog_showcase'
.\tools\dev.cmd local-lab
```

Capture waits for a real raw-13 Focused Scatterbow contact and accepted Undertaker Tonic healing. The frame must also show catalog `12/12`, reward tables `5/5`, Parish Leather health/armor/movement, and the pinned default-seed boss grants.

## GB-M01 rendered performance evidence

The `stress_full` and `stress_reduced` scenarios are evidence-only release modes. They render exactly 800 moving hostile projectiles and 40 moving enemies, retain all 40 hostile telegraphs, sample every rendered frame after a five-second warmup, and sample this process's resident memory every ten seconds. Stress windows are fixed at the configured size for the whole run. Reports identify the exact executable by its BLAKE3 digest and publish JSON atomically before the final screenshot.

Build once, then run both effect modes from the same executable/content pair. Set `GRAVEBOUND_TARGET_CLASS_VERIFIED=1` only after recording and checking the machine against GDD `TECH-070`; otherwise leave it at `0`. `GRAVEBOUND_TARGET_GPU` is evidence text and must contain the actual adapter name.

```powershell
cargo build --workspace --release --locked

$env:GRAVEBOUND_WINDOW_SIZE = '1920x1080'
$env:GRAVEBOUND_TARGET_CLASS_VERIFIED = '0'
$env:GRAVEBOUND_TARGET_GPU = '<actual GPU adapter>'
$env:GRAVEBOUND_STRESS_DURATION_SECONDS = '1800'
$env:GRAVEBOUND_EVIDENCE_SCENARIO = 'stress_full'
$env:GRAVEBOUND_PERFORMANCE_REPORT_PATH = (Join-Path $PWD 'tmp\stress-full-30m.json')
$env:GRAVEBOUND_SCREENSHOT_PATH = (Join-Path $PWD 'tmp\stress-full-30m.png')
.\target\release\client_bevy.exe
```

For the documented fallback capture, change the scenario to `stress_reduced`, use new output paths, and run the same immutable executable. Reduced mode may cull priority 5 decorative ambience and then priority 4 remote-friendly effects; it never culls hostile projectiles or telegraphs. A passing report requires 1920×1080 rendered samples, at least 60 FPS, p95 at most 16.7 ms, p99 at most 33.3 ms, at least 30 minutes of memory samples, peak resident memory at most 1.5 GB, no detected monotonic leak, exact hostile counts, retained telegraphs, and truthful target-hardware attestation. Short runs are useful smoke tests but deliberately receive `memory_failed` acceptance.

## Logging

Copy `.env.example` only when a future launcher requires a local environment file. For current commands, set `RUST_LOG` in the shell when needed:

```powershell
$env:RUST_LOG = 'info,client_bevy=debug,sim_core=debug,sim_content=debug'
```

Never log credentials, authentication tickets, account tokens, or future commerce secrets.
