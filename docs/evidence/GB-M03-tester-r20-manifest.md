# GB-M03 Optimized Tester r20 Manifest

**Result:** PASS for the user-authorized production-blocking packaging gate.

## Design authorities

The package contains the planned testable GB-M03 route defined by:

1. `Gravebound_Production_GDD_v1_Canonical.md`
2. `Gravebound_Content_Production_Spec_v1.md`
3. `Gravebound_Development_Roadmap_v1.md`

It does not substitute packaging for the Roadmap's formal full-loop, restart, nonduplication, timing, cohort, backup/restore, or external platform gates.

## Identity and hashes

| Artifact | Value |
| --- | --- |
| Build date / refresh | `2026-07-21 / r20` |
| Source revision | `115e7e53c7dadc134b7cd7bb2e0b8bade525ea6a` |
| Source archive SHA-256 | `10045085E64214F2F21A70937196EBE940FEBF0078C92D22111FE269076543AB` |
| `Gravebound.exe` SHA-256 | `B7A1DBE716253C20754735B9BCA639D466E69205F72696D94269CE56DF031F39` |
| `GraveboundServer.exe` SHA-256 | `CD20D0E1B73689E3E18632AE80EAF0AA6337D0D70EF01AD4E24473C4BBA45309` |
| Tester ZIP SHA-256 | `722ACF65C05AD84098B4119D3611C6D1FD15F2CD58BFCB76C23EC23EF97F52CB` |

Local delivery paths:

- `dist/Gravebound-GB-M03-Tester-2026-07-21-r20/`
- `dist/Gravebound-GB-M03-Tester-2026-07-21-r20.zip`

## Production-blocking package verification

- Locked optimized Windows release build for `client_bevy` and `server_app`: PASS.
- Strict compiled-content validation: PASS.
- Packaged client and server CLI checks: PASS.
- Dependency-free Local Lab 12-second launch/responding check: PASS.
- Hall, dungeon, boss, item/Vault, death/Memorial, and successor preview launch/responding checks: PASS.
- Startup stderr across all seven standalone launch modes: EMPTY.
- `TESTING.txt` records the normal route, controls, server-authored combat-presentation expectations, exact health thresholds, design authorities, source revision, and executable hashes: PASS.
- Atomic publication completed before cleanup; final `dist` contains only the r20 directory and r20 ZIP. The previous r19 directory and archive are absent: PASS.
- Temporary staging directory was removed: PASS.

`PLAY GAME.cmd` requires Docker Desktop because the normal authenticated route uses the package-local PostgreSQL service. Docker/PostgreSQL were unavailable in the packaging environment, so this manifest does not claim a fresh persistent-route journey; that journey remains part of the owner's deferred full audit. The dependency-free Local Lab and six isolated M03 review modes were smoke-verified from the packaged optimized executable.

The user-owned untracked Grave Arbalist seed under `assets/core/player/grave_arbalist/` was preserved and is not represented as committed source in this package.

## Current Next Step

Run the owner-deferred formal M03 acceptance sweep from r20: exhaustive adverse/restart and 25 complete journeys, nonduplication and timing checks, optimized live visual/performance capture, backup/restore rehearsal, private-cohort comprehension, and external Steam/platform evidence. This step remains governed by `Gravebound_Production_GDD_v1_Canonical.md`, `Gravebound_Content_Production_Spec_v1.md`, and `Gravebound_Development_Roadmap_v1.md`. Implementation reopens only for production blockers found by that sweep.
