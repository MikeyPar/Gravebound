# GB-M03 Optimized Tester r27 Manifest

## Authority

This release is reviewed against:

1. `Gravebound_Production_GDD_v1_Canonical.md`
2. `Gravebound_Content_Production_Spec_v1.md`
3. `Gravebound_Development_Roadmap_v1.md`

It packages the currently implemented wipeable Core route. It does not promote Core content, open M04+ capabilities, or substitute packaging for the remaining M03 acceptance gates.

## Build identity

| Field | Value |
| --- | --- |
| Build date / refresh | `2026-07-23 / r27` |
| Source revision | `e8982be9f96334d0f08c76c4ffdfda0d9a871faf` |
| Source archive SHA-256 | `C557BC7546377DA41739819FA3581585909C46A516720041F02C8907E411F7BE` |
| `Gravebound.exe` SHA-256 | `881C73CBC4BFD5A4F938A919E73F2AFFC0AADA1209CEF24244D8926B7C7E7EA0` |
| `GraveboundServer.exe` SHA-256 | `E0448C9C1B965E2D9BDE5A241FA427E377B2B62306CF0B462D6728C4AC6F` |
| Tester ZIP SHA-256 | `1D57917CD948EE50389F0CB0C5E66871F4EAAED8778241F47845A9F4150D3EB6` |

Local delivery paths:

- `dist/Gravebound-GB-M03-Tester-2026-07-23-r27/`
- `dist/Gravebound-GB-M03-Tester-2026-07-23-r27.zip`

## Wired route

`PLAY GAME.cmd` starts the package-local PostgreSQL service, `GraveboundServer.exe serve-core-private-life`, and `Gravebound.exe core-private-life`. The accessible route includes Character Select, Lantern Hall, Oath/Bargain stations, Vault/Overflow, the fixed Bell Sepulcher B0–B6 route, Sir Caldus, extraction, Emergency Recall, durable death/Memorial/Echo state, Resolution Hold, successor recovery, Belt consumables, and the F6 accessibility panel.

The release includes migrations through schema 77. Schema 77 preserves strict delete immutability for committed loot telemetry while the guarded disposable-database reset remains the valid wipe mechanism for the pre-Early-Access Core graph.

Commits `ce9c0ef` and `e8982be` repair the ordinary Hall-to-danger admission boundary. The connection now carries its exact generation-pinned Hall route lease into danger construction, binds the microrealm driver, waits for its first committed authoritative frame, and only then binds Recall to that tick. Failed admission retires both partial owners instead of exposing control.

## Production-blocking package verification

- Locked optimized Windows release build for `client_bevy` and `server_app`: PASS.
- Strict compiled-content validation: PASS.
- Packaged client and server CLI checks: PASS.
- Dependency-free Local Lab 12-second launch/responding check: PASS.
- Hall, dungeon, boss, item/Vault, death/Memorial, and successor preview launch/responding checks: PASS.
- Startup stderr across all seven standalone launch modes: EMPTY.
- Atomic publication completed before cleanup; final `dist` contains only the r27 directory and r27 ZIP. r26 and all older tester artifacts are absent: PASS.

`PLAY GAME.cmd` requires Docker Desktop because the authenticated route uses its private PostgreSQL service. Docker is not installed in the packaging environment, so this manifest does not claim a fresh local persistent-route playthrough. Hosted PostgreSQL and real-QUIC evidence remain the authority for that route.

Hosted run [`30013285123`](https://github.com/MikeyPar/Gravebound/actions/runs/30013285123) applied schema 77 and passed the telemetry-source, terminal-origin, death, extraction, Recall, successor, progression, world-flow, recovery, Bargain, Ash, Caldus, equipment, and safe-inventory suites. Its 25 item-lifecycle journeys recorded login median `10.887 ms` / p95 `20.218 ms` and mutation median `7.791 ms` / p95 `17.479 ms`. Those are item-lifecycle journeys, not the Roadmap's 25 complete private loops. The assembled journey then exposed the corrected Hall-to-microrealm admission defect before either terminal branch.

The untracked Grave Arbalist asset seed under `assets/core/player/grave_arbalist/` was preserved and was not included in the Git source archive or modified during release construction.

## Current Next Step

Under all three design authorities, retain r27 while exact-source hosted run [`30016103442`](https://github.com/MikeyPar/Gravebound/actions/runs/30016103442) verifies the corrected complete extraction/death/successor route. After one full route is green, run 25 complete ordinary loops with current aggregate timing and optimized Realm Gate captures. Independently complete the remaining `TEL-003` boss-phase, party/contribution, and network-health facts. The private cohort, comprehension metrics, backup/restore, hosting, and Steamworks evidence remain separate final gates.
