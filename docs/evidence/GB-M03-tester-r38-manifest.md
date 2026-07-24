# GB-M03 Optimized Tester r38 Manifest

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
| Build date / refresh | `2026-07-23 / r38` |
| Source revision | `572318f7cf515af813aaa5365bee90a8f5513678` |
| Source archive SHA-256 | `8D41550BA89E825F068124851401267232377BD110F338442F18EA1F8636F9A9` |
| `Gravebound.exe` SHA-256 | `0BAD8B6E853C8F57FB3D1ADA8F7D03287AE62D0512672CF04D845DB2ABCF289C` |
| `GraveboundServer.exe` SHA-256 | `E52AD6D4D88522AC66CE1A49660C1381B40919F14B3F5646796B2F6D8260C63E` |
| Tester ZIP SHA-256 | `C80B3AAE31278F17FC33AB3E436D87C573EC1427F4020CD7E50A1DD92AED8496` |
| Bundled PostgreSQL | `17.10-2 windows-x86_64` |
| PostgreSQL archive SHA-256 | `7AC798A66DF007B614D7B885E49646BF3633B13DE345FF319697C70E00F5E99B` |

Local delivery paths:

- `dist/Gravebound-GB-M03-Tester-2026-07-23-r38/`
- `dist/Gravebound-GB-M03-Tester-2026-07-23-r38.zip`

## Wired route and panic corrections

`PLAY GAME.cmd` initializes and starts package-local PostgreSQL 17.10 on loopback, starts
`GraveboundServer.exe serve-core-private-life`, waits for schema-79 readiness, and then starts
`Gravebound.exe core-private-life`. No Docker, Steam, or external database installation is
required. Generated local secrets and wipeable tester data remain under the package-local
`.runtime` directory.

The accessible route includes Character Select, Lantern Hall, Oath/Bargain stations,
Vault/Overflow, the fixed Bell Sepulcher B0-B6 route, Sir Caldus, extraction, Emergency Recall,
durable death/Memorial/Echo state, Resolution Hold, successor recovery, Belt consumables, and the
F6 accessibility panel.

Commits `b2d31ce` and `ebef5e8` make the terminal-view and gameplay-render visibility/transform
queries explicitly disjoint. This removes both Bevy `B0001` startup panics found by the real
persistent-client launch path. Commit `abdd408` adds that exact path to publication: a candidate
package cannot be released unless bundled PostgreSQL, the schema-ready persistent server, and the
native client remain alive together. Commit `572318f` adds bounded obsolete-only cleanup and uses
the same guarded cleanup after atomic publication.

## Production-blocking package verification

- Locked optimized Windows client/server release build: PASS.
- Strict compiled-content validation: PASS.
- Packaged client/server CLI checks: PASS.
- Bundled PostgreSQL first initialization and loopback startup: PASS.
- Schema-79 persistent server plus native `core-private-life` client remained ready together for
  five seconds: PASS.
- Local Lab 12-second launch/responding check: PASS.
- Hall, dungeon, boss, item/Vault, death/Memorial, and successor preview launch/responding checks:
  PASS.
- Startup stderr across all seven standalone launch modes: EMPTY.
- ZIP inspection found 2,017 entries: PASS.
- Atomic publication completed before cleanup; final `dist` contains only the r38 directory and r38
  ZIP: PASS.
- Focused client and production-route target compilation after the query and harness corrections:
  PASS.

The r38 runtime introduces no art change from r33, so the existing optimized README captures remain
current. The untracked Grave Arbalist asset seed under `assets/core/player/grave_arbalist/` was
preserved and was not included in the Git source archive or modified during release construction.

## Current Next Step

Under all three design authorities, the Docker dependency and both reported Bevy startup panics are
corrected. Hosted run
[`30069213403`](https://github.com/MikeyPar/Gravebound/actions/runs/30069213403) passed the
schema-79/pre-checkpoint gate and reached route tick 1300 before the evidence driver reused primary
press identity `1`; commit `87a7967` corrects that harness-only defect. Corrected hosted run
[`30071229521`](https://github.com/MikeyPar/Gravebound/actions/runs/30071229521) is the active
single-route proof. The Current Next Step is its green completion, followed by the Roadmap's 25
complete ordinary-loop journeys, current aggregate timing, optimized Realm Gate capture, bounded
operational telemetry proof, private cohort, hosting/backup rehearsal, destination/privacy
approval, and Steamworks evidence. Formal progress remains `15/23 (65%)` until an entire package or
exit outcome closes.
