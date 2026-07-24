# GB-M03 Optimized Tester r33 Manifest

## Authority

This release is reviewed against:

1. `Gravebound_Production_GDD_v1_Canonical.md`
2. `Gravebound_Content_Production_Spec_v1.md`
3. `Gravebound_Development_Roadmap_v1.md`

It packages the currently implemented wipeable Core route. It does not promote Core content, open
M04+ capabilities, or substitute packaging for the remaining M03 acceptance gates.

## Build identity

| Field | Value |
| --- | --- |
| Build date / refresh | `2026-07-23 / r33` |
| Source revision | `0afc90e0216ad75366b796a7bc0b5115ec812e0f` |
| Source archive SHA-256 | `0C05742FC88AF26747F2E6D34D59E4DB09FEE9F1CC2E1FE1CA9E10C9E2CBF96A` |
| `Gravebound.exe` SHA-256 | `1543F64BC15421D7D16F84D50D33712BCDAD214C2AEFB05E2FEB56301C05F3D3` |
| `GraveboundServer.exe` SHA-256 | `95C3F12985053E0D16B72BB194E505798CE96553F64F59D1D174A76F03C3B277` |
| Tester ZIP SHA-256 | `E80C08D7D7E96642F3F4C9ECBD820A0D5D90079B2E0CEB8B89114F3D46114F30` |

Local delivery paths:

- `dist/Gravebound-GB-M03-Tester-2026-07-23-r33/`
- `dist/Gravebound-GB-M03-Tester-2026-07-23-r33.zip`

## Wired route

`PLAY GAME.cmd` starts the package-local PostgreSQL service, `GraveboundServer.exe
serve-core-private-life`, and `Gravebound.exe core-private-life`. The accessible route includes
Character Select, Lantern Hall, Oath/Bargain stations, Vault/Overflow, the fixed Bell Sepulcher
B0-B6 route, Sir Caldus, extraction, Emergency Recall, durable death/Memorial/Echo state,
Resolution Hold, successor recovery, Belt consumables, and the F6 accessibility panel.

The release includes migrations through schema 79 and protocol 1.25. Schema 79 permits legitimate
live damage before TECH-023's first optional 30-second danger checkpoint while preserving the
immutable danger-entry restore, retained receipt, content, graph, and terminal-promotion
authorities.

Commit `79668fe` corrects the Bell-transfer presentation boundary. The same retained participant
is relocated from the micro-realm portal to the sole compiled B0 `SafeEntry` anchor `(3,5.5)`;
the server publishes an exact route-version-bound noncombat snapshot on the committed transfer
tick without advancing the combat clock.

## Production-blocking package verification

- Locked optimized Windows release build for `client_bevy` and `server_app`: PASS.
- Strict compiled-content validation: PASS.
- Packaged client and server CLI checks: PASS.
- Dependency-free Local Lab 12-second launch/responding check: PASS.
- Hall, dungeon, boss, item/Vault, death/Memorial, and successor preview launch/responding checks:
  PASS.
- Startup stderr across all seven standalone launch modes: EMPTY.
- ZIP inspection found 422 entries under the single expected r33 root: PASS.
- Atomic publication completed before cleanup; final `dist` contains only the r33 directory and r33
  ZIP. r32 and every older tester artifact are absent: PASS.

`PLAY GAME.cmd` requires Docker Desktop because the authenticated route uses its private PostgreSQL
service. Docker is not installed in the packaging environment, so this manifest does not claim a
fresh local persistent-route playthrough. Hosted PostgreSQL and real-QUIC evidence remain the
authority for that route.

The r33 runtime introduces no art change from r32, so the existing optimized README captures remain
current. The untracked Grave Arbalist asset seed under `assets/core/player/grave_arbalist/` was
preserved and was not included in the Git source archive or modified during release construction.

## Current Next Step

Under all three design authorities, hosted run
[`30053995319`](https://github.com/MikeyPar/Gravebound/actions/runs/30053995319) passed schema-79
pre-checkpoint proof and the corrected exact B0 snapshot, then proved the production terminal
owner drops B1's first frame at route version 9/tick 1243. The public route correctly remains
`RoomSpawnWarning`; this is not a timeout to weaken. Commit `0698737` surfaces the complete
underlying owner error immediately, and focused run
[`30066885083`](https://github.com/MikeyPar/Gravebound/actions/runs/30066885083) is resolving the
exact substep. r33's Docker-only primary launcher is also being replaced by a self-contained
PostgreSQL 17.10 release. The Current Next Step is the smallest evidence-backed terminal-owner
fix, hosted green single-route proof, and optimized r34 publication without Docker. The Roadmap's
25 complete private-loop journeys, aggregate timing, optimized Realm Gate capture, cohort,
hosting, and Steamworks evidence remain separate final gates. Formal progress remains
`15/23 (65%)` until an entire package or exit outcome closes.
