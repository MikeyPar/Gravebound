# GB-M03 Optimized Tester r36 Manifest

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
| Build date / refresh | `2026-07-23 / r36` |
| Source revision | `eb0c6b45ee7c185fcc4e23d07ad598384dcb1d79` |
| Source archive SHA-256 | `8276321F3CD0D0401F9CB9D2519687AACA3B2AC0853A09365417CD2D1DA11975` |
| `Gravebound.exe` SHA-256 | `1543F64BC15421D7D16F84D50D33712BCDAD214C2AEFB05E2FEB56301C05F3D3` |
| `GraveboundServer.exe` SHA-256 | `E52AD6D4D88522AC66CE1A49660C1381B40919F14B3F5646796B2F6D8260C63E` |
| Tester ZIP SHA-256 | `19E31A54FFD149FE704AA910FAAA391D73AFB437711CCC99CDB5E7094FDA731F` |
| Bundled PostgreSQL | `17.10-2 windows-x86_64` |
| PostgreSQL archive SHA-256 | `7AC798A66DF007B614D7B885E49646BF3633B13DE345FF319697C70E00F5E99B` |

Local delivery paths:

- `dist/Gravebound-GB-M03-Tester-2026-07-23-r36/`
- `dist/Gravebound-GB-M03-Tester-2026-07-23-r36.zip`

## Wired route

`PLAY GAME.cmd` initializes and starts package-local PostgreSQL 17.10 on loopback, starts
`GraveboundServer.exe serve-core-private-life`, waits for schema-79 readiness, and then starts
`Gravebound.exe core-private-life`. No Docker, Steam, or external database installation is
required. Generated local secrets and wipeable tester data remain under the package-local
`.runtime` directory.

The accessible route includes Character Select, Lantern Hall, Oath/Bargain stations,
Vault/Overflow, the fixed Bell Sepulcher B0-B6 route, Sir Caldus, extraction, Emergency Recall,
durable death/Memorial/Echo state, Resolution Hold, successor recovery, Belt consumables, and the
F6 accessibility panel.

Commit `eb0c6b4` repairs the durable Bell scene transition. A danger-to-danger transfer updates the
open lineage's current content ID only when the lineage and restore-point identities are unchanged
and the prior content ID matches exactly. Location, lineage, and immutable transfer receipt remain
one transaction; mismatch rolls back and fails closed.

## Production-blocking package verification

- Locked optimized Windows client/server release build: PASS.
- Strict compiled-content validation: PASS.
- Packaged client/server CLI checks: PASS.
- Bundled PostgreSQL first initialization and loopback startup: PASS.
- Schema-79 persistent-server readiness from a package path containing spaces: PASS.
- Local Lab 12-second launch/responding check: PASS.
- Hall, dungeon, boss, item/Vault, death/Memorial, and successor preview launch/responding checks:
  PASS.
- Startup stderr across all seven standalone launch modes: EMPTY.
- ZIP inspection found 2,017 entries: PASS.
- Atomic publication completed before cleanup; final `dist` contains only the r36 directory and r36
  ZIP. r35 and every older tester artifact are absent: PASS.
- Focused disposable-PostgreSQL Bell transfer, bootstrap reload, pool restart, exact replay, and
  changed-binding conflict journey: PASS (`1/1`).

The r36 runtime introduces no art change from r33, so the existing optimized README captures remain
current. The untracked Grave Arbalist asset seed under `assets/core/player/grave_arbalist/` was
preserved and was not included in the Git source archive or modified during release construction.

## Historical publication boundary

Under all three design authorities, the Docker packaging defect and the named B1 persistence defect
are corrected. Focused hosted run
[`30069213403`](https://github.com/MikeyPar/Gravebound/actions/runs/30069213403) passed the
schema-79/pre-checkpoint gate at exact source `eb0c6b4` and later stopped at route tick 1300 when
the evidence driver reused primary-press identity `1`. Tester r38 supersedes this package and adds
the matching native-client startup gate. At this publication, the next step was green completion followed by the
Roadmap's 25 complete ordinary-loop journeys, current aggregate timing, optimized Realm Gate
capture, bounded operational telemetry proof, private cohort, hosting/backup rehearsal,
destination/privacy approval, and Steamworks evidence. Formal progress remains `15/23 (65%)`
until an entire package or exit outcome closes.
