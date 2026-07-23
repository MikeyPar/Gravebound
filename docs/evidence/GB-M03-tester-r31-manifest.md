# GB-M03 Optimized Tester r31 Manifest

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
| Build date / refresh | `2026-07-23 / r31` |
| Source revision | `c4f44e70ac2c770148d12d7bae53b008313699bd` |
| Source archive SHA-256 | `7B46884FC0AECC24202CE777DA8B7DECF892F6F0D36B336107637870E1F9D94F` |
| `Gravebound.exe` SHA-256 | `881C73CBC4BFD5A4F938A919E73F2AFFC0AADA1209CEF24244D8926B7C7E7EA0` |
| `GraveboundServer.exe` SHA-256 | `158199725377998B08EAC355D9EB4B853F8B2AB66691DCECBAEA78EC55259162` |
| Tester ZIP SHA-256 | `B92C98A5A7E97FFB943BF5B4CD2CEE41F16AA76EAA3569B1EBF1AF8D1CA16E53` |

Local delivery paths:

- `dist/Gravebound-GB-M03-Tester-2026-07-23-r31/`
- `dist/Gravebound-GB-M03-Tester-2026-07-23-r31.zip`

## Wired route

`PLAY GAME.cmd` starts the package-local PostgreSQL service, `GraveboundServer.exe
serve-core-private-life`, and `Gravebound.exe core-private-life`. The accessible route includes
Character Select, Lantern Hall, Oath/Bargain stations, Vault/Overflow, the fixed Bell Sepulcher
B0-B6 route, Sir Caldus, extraction, Emergency Recall, durable death/Memorial/Echo state,
Resolution Hold, successor recovery, Belt consumables, and the F6 accessibility panel.

The release includes migrations through schema 77. Schema 77 preserves strict delete immutability
for committed loot telemetry while the guarded disposable-database reset remains the valid wipe
mechanism for the pre-Early-Access Core graph.

Commits `1e6e06f`, `1a53359`, `e003696`, and `c4f44e7` close the latest Hall-to-danger admission
boundaries. The route admits the canonical interval before the first periodic danger checkpoint,
activates only the exact committed Realm Gate lineage before simulation, and publishes danger
route authority against the first committed simulation tick. Exact activation replay is
idempotent, changed authority remains fail-closed, and the shared reliable writer can no longer
suppress the simulation-backed route projection after an earlier tick-zero publication.

## Production-blocking package verification

- Locked optimized Windows release build for `client_bevy` and `server_app`: PASS.
- Strict compiled-content validation: PASS.
- Packaged client and server CLI checks: PASS.
- Dependency-free Local Lab 12-second launch/responding check: PASS.
- Hall, dungeon, boss, item/Vault, death/Memorial, and successor preview launch/responding checks:
  PASS.
- Startup stderr across all seven standalone launch modes: EMPTY.
- ZIP inspection found 422 entries under the single expected r31 root: PASS.
- Atomic publication completed before cleanup; final `dist` contains only the r31 directory and r31
  ZIP. r30 and every older tester artifact are absent: PASS.

`PLAY GAME.cmd` requires Docker Desktop because the authenticated route uses its private PostgreSQL
service. Docker is not installed in the packaging environment, so this manifest does not claim a
fresh local persistent-route playthrough. Hosted PostgreSQL and real-QUIC evidence remain the
authority for that route.

Hosted run
[`30022764927`](https://github.com/MikeyPar/Gravebound/actions/runs/30022764927) proved exact
lineage activation and returned an accepted Realm Gate transfer after every preceding mandatory
PostgreSQL suite passed. It then exposed a reliable-publication mismatch: the server emitted the
new danger route at tick zero, while the production journey correctly required authority joined to
a committed simulation tick; route deduplication suppressed the later identical projection.
Commit `c4f44e7` publishes a bound danger route at its already committed driver tick without
changing Hall control-plane publication. Hosted run
[`30028182217`](https://github.com/MikeyPar/Gravebound/actions/runs/30028182217) proved live
microrealm activation and isolated a test-driver target-selection defect. Commit `2e8d2f4`
corrects that test-only policy; r31 remains the latest runtime package.

The untracked Grave Arbalist asset seed under `assets/core/player/grave_arbalist/` was preserved and
was not included in the Git source archive or modified during release construction.

## Current Next Step

Under all three design authorities, an exact-source hosted rerun from commit `2e8d2f4` must verify
the complete extraction/death/successor route. After one full route is green, run 25 complete
ordinary loops with current aggregate timing and optimized Realm Gate captures. Independently complete the
remaining `TEL-003` boss-phase, party/contribution, and network-health facts. The private cohort,
comprehension metrics, backup/restore, hosting, and Steamworks evidence remain separate final gates.
