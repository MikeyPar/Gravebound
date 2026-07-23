# GB-M03 Optimized Tester r32 Manifest

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
| Build date / refresh | `2026-07-23 / r32` |
| Source revision | `525b42385bbb49fb4d36151068795ccf90451f27` |
| Source archive SHA-256 | `D305721EEB86BCCE539B979823179FABFF1BD4515C7A5F7EAE5BD290EE45AAA3` |
| `Gravebound.exe` SHA-256 | `054D5F0783271972BB66788356CA585DAE5E7519E149C7CF71663B9A0E554946` |
| `GraveboundServer.exe` SHA-256 | `B28779B2D8760D8A4127B8E8FC8185B09674FC1E62C09C1214ACB9A61CEC4C9A` |
| Tester ZIP SHA-256 | `7ED2032686E139E82D8171C424AB817E9AEEE0DF85222C78847503E97C700767` |

Local delivery paths:

- `dist/Gravebound-GB-M03-Tester-2026-07-23-r32/`
- `dist/Gravebound-GB-M03-Tester-2026-07-23-r32.zip`

## Wired route

`PLAY GAME.cmd` starts the package-local PostgreSQL service, `GraveboundServer.exe
serve-core-private-life`, and `Gravebound.exe core-private-life`. The accessible route includes
Character Select, Lantern Hall, Oath/Bargain stations, Vault/Overflow, the fixed Bell Sepulcher
B0-B6 route, Sir Caldus, extraction, Emergency Recall, durable death/Memorial/Echo state,
Resolution Hold, successor recovery, Belt consumables, and the F6 accessibility panel.

The release includes migrations through schema 78 and protocol 1.25. Schema 78 atomically stores
server-owned death network context, correction authority, Caldus phase/party/contribution facts,
and their redacted schema-2 telemetry projection without changing the player-visible route.
Historical pre-78 deaths remain explicitly unavailable rather than reconstructed.

Commits `2e8d2f4` and `1ad117e` correct production-journey targeting and initial-input liveness
without adding a privileged gameplay route. Commit `1f1d0fd` adds the schema-78 authority and
protocol contract. Hosted run
[`30048509525`](https://github.com/MikeyPar/Gravebound/actions/runs/30048509525) proves legacy
upgrade/restart/replay plus fresh PostgreSQL atomicity and redaction.

## Production-blocking package verification

- Locked optimized Windows release build for `client_bevy` and `server_app`: PASS.
- Strict compiled-content validation: PASS.
- Packaged client and server CLI checks: PASS.
- Dependency-free Local Lab 12-second launch/responding check: PASS.
- Hall, dungeon, boss, item/Vault, death/Memorial, and successor preview launch/responding checks:
  PASS.
- Startup stderr across all seven standalone launch modes: EMPTY.
- ZIP inspection found 422 entries under the single expected r32 root: PASS.
- Atomic publication completed before cleanup; final `dist` contains only the r32 directory and r32
  ZIP. r31 and every older tester artifact are absent: PASS.

`PLAY GAME.cmd` requires Docker Desktop because the authenticated route uses its private PostgreSQL
service. Docker is not installed in the packaging environment, so this manifest does not claim a
fresh local persistent-route playthrough. Hosted PostgreSQL and real-QUIC evidence remain the
authority for that route.

The r32 runtime introduces no player-visible layout or art change from r31, so the existing
optimized README captures remain current. The untracked Grave Arbalist asset seed under
`assets/core/player/grave_arbalist/` was preserved and was not included in the Git source archive
or modified during release construction.

## Current Next Step

Under all three design authorities, hosted run
[`30050712798`](https://github.com/MikeyPar/Gravebound/actions/runs/30050712798) passed the focused
schema-79/pre-checkpoint PostgreSQL proof after this package, then exposed the independently
corrected Bell B0 safe-entry publication boundary. Commit `79668fe` carries that source fix. The
Current Next Step is active hosted complete-route run
[`30052832985`](https://github.com/MikeyPar/Gravebound/actions/runs/30052832985), followed by a
replacement schema-79 package built from the exact proven source. The package script will remove
r32 only after the replacement passes every production-blocking build, launch, mode, CLI, root,
and hash check. The Roadmap's 25 complete private-loop journeys, aggregate timing, optimized Realm
Gate capture, cohort, hosting, and Steamworks evidence remain separate final gates. Formal progress
remains `15/23 (65%)` until an entire package or exit outcome closes.
