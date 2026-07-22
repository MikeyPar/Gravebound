# GB-M03 Optimized Tester r22 Manifest

**Result:** PASS for the user-authorized production-blocking packaging gate; exact-source hosted CI is still running.

## Design authorities

The package assembles the currently implemented GB-M03 route under:

1. `Gravebound_Production_GDD_v1_Canonical.md`
2. `Gravebound_Content_Production_Spec_v1.md`
3. `Gravebound_Development_Roadmap_v1.md`

Packaging is not a substitute for the Roadmap's final ordinary-route, 25-journey, cohort, hosting, backup/restore, or external platform gates.

## Identity and hashes

| Artifact | Value |
| --- | --- |
| Build date / refresh | `2026-07-21 / r22` |
| Source revision | `503bb1a4205829190613412b3d8f9e20e4e86aef` |
| Source archive SHA-256 | `28A44CA0A89AC1DD7846CFEC9570308454D9221BF1C0C37B5DB42B9F0F6932D7` |
| `Gravebound.exe` SHA-256 | `B7999A4A10220D276629F47D67DA0ECC7B65A7A6D24DBB99734A0D9D90DBD455` |
| `GraveboundServer.exe` SHA-256 | `25B847D80AFD4565F24CCD00E5D77C705A64C10495892422A0E1DC440B180705` |
| Tester ZIP SHA-256 | `E91E77E1AE24889C221CCAC04FDBC5D6A5062664D6D477ACCD69C214D73B8675` |

Local delivery paths:

- `dist/Gravebound-GB-M03-Tester-2026-07-21-r22/`
- `dist/Gravebound-GB-M03-Tester-2026-07-21-r22.zip`

## Wired route

`PLAY GAME.cmd` starts the package-local PostgreSQL service, `GraveboundServer.exe serve-core-private-life`, and `Gravebound.exe core-private-life`. The route exposes Character Select, Lantern Halls, Oath/Bargain stations, Vault/Overflow, the fixed private dungeon and Sir Caldus, extraction, Emergency Recall, death/Memorial/Echo state, Resolution Hold, successor recovery, Belt consumables, and the F6 accessibility panel. `TESTING.txt` records every current control.

## Production-blocking package verification

- Locked optimized Windows release build for `client_bevy` and `server_app`: PASS.
- Strict compiled-content validation: PASS.
- Packaged client and server CLI checks: PASS.
- Dependency-free Local Lab 12-second launch/responding check: PASS.
- Hall, dungeon, boss, item/Vault, death/Memorial, and successor preview launch/responding checks: PASS.
- Startup stderr across all seven standalone launch modes: EMPTY.
- Forward schema `69` includes the canonical item-level `1..20` and rarity `0..5` shape required by GDD `ECH-002`, Content Spec `CONT-ECHO-001`, and Roadmap `GB-M03-06/13`.
- Atomic publication completed before cleanup; final `dist` contains only the r22 directory and r22 ZIP. r21 and all older tester artifacts are absent: PASS.
- Temporary staging directory was removed: PASS.

`PLAY GAME.cmd` requires Docker Desktop because the authenticated route uses its private PostgreSQL service. Docker/PostgreSQL are unavailable in the packaging environment, so this manifest does not claim a fresh persistent-route playthrough. The dependency-free Local Lab and six isolated M03 review modes were smoke-verified from the packaged optimized executable.

The preserved untracked Grave Arbalist asset seed under `assets/core/player/grave_arbalist/` is present in the workspace/package input but is not represented by the source revision. It was not modified or committed during release construction.

## Current Next Step

Obtain a green exact-source hosted PostgreSQL run for migration `0069` and the support least-privilege gate. Then run the production-server/PostgreSQL/real-QUIC ordinary-route harness for extraction and death/successor, followed by 25 complete journeys, current timing, and optimized live capture. The private cohort, backup/restore, hosting, and Steamworks evidence remain separate final gates. All work continues under `Gravebound_Production_GDD_v1_Canonical.md`, `Gravebound_Content_Production_Spec_v1.md`, and `Gravebound_Development_Roadmap_v1.md`.
