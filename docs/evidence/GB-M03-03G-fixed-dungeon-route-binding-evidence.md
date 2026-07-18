# GB-M03-03G fixed-dungeon route binding evidence

**Status:** Local implementation evidence accepted; hosted CI for the exact commits remains in progress. Normal route admission remains disabled.

## Three-authority basis

1. `Gravebound_Production_GDD_v1_Canonical.md` `DNG-003`-`006`, `COM-001`-`006`, and `BRG-001`-`002` require server-owned room transitions, the complete quiet/reset contract, one continuing character life, and an explicit rest/Bargain result.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-007` fixes the exact `B0 -> B1 -> B2 -> B3 -> B4 -> B5 -> B6` route. `CONT-BOSS-001`-`002` keep Caldus and the post-reward exit outside ordinary-room authority.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires the private route to preserve one authoritative character through explicit instance transfers without developer commands.

## Implemented contracts

- Commit `5deee6d` corrects every fixed combat-room entry point to a player-radius-safe inset from the authored door. Each scene transition changes only the scene-local target position; health, Belt state, cooldowns, player entity ID, and the hostile-projectile allocator remain in the same moved allocation.
- Commit `f044a61` carries the microrealm owner's next hostile spawn ordinal into the fixed-room compiler. B1 and every later room/reset now begin after every ordinal consumed by `pack.bell.01`, preventing run-local hostile identity reuse.
- Commit `6749141` adds one atomic fixed-dungeon route compare-and-swap command. It validates the expected route version and exact canonical B0-B6 position, commits all legal same-frame phase changes under one actor lock, supports DNG-005 reset to Dormant, and rejects stale, Bell-reserved, terminal-reserved, foreign, or impossible targets without partial actor progress.
- `CorePrivateMicrorealmRuntime` now exposes a consuming committed-Bell handoff only when its owned simulation remains cleared and in range and the exact route generation has converged on B0. It carries the combat envelope, player, hostile-projectile allocator, next hostile ordinal, final tick, route directory, and generation-pinned lease.
- The Bell handoff rebases the envelope's character state version exactly once from the transition's source to its stored `+1` destination. Skipped, stale, repeated, or foreign version material fails closed.
- `CorePrivateFixedDungeonRuntime` stages B0-B6 simulation changes, maps their resulting node/phase to the persistent route actor, and replaces local combat/tick state only after the route CAS commits. Its first room frame is exactly the final microrealm tick plus one.

## Verification

- `130/130` complete `sim_content` tests passed after safe room-entry relocation.
- Focused fixed-dungeon route tests prove B0-to-B1 entry, one-frame participant-lock plus door-close projection, carried tick continuity, exact Bell version rebase, player/projectile identity continuity, hostile ordinal continuity, stale CAS rejection, invalid-target rejection, and local rollback.
- The complete server suite passed `323/323` library tests plus every enabled server binary/integration/doc target. PostgreSQL tests that require the explicitly authorized disposable database remain ignored by their existing gate.
- Strict `cargo clippy -p sim_content --all-targets --all-features -- -D warnings` and `cargo clippy -p server_app --all-targets --all-features -- -D warnings` passed.
- Formatting and `git diff --check` passed.
- Hosted CI runs for `5deee6d`, `f044a61`, and `6749141` were still in progress at the latest poll and are not claimed green here.

## Explicit boundary

This slice does not yet transform the existing session-owned 30 Hz driver in place, generate fixed-room movement/combat frames from retained input, persist the B4 Bargain result, construct Sir Caldus, commit room/boss rewards or pending inventory, create the stable B6 exit, or compose all five terminal producers. Returning the old driver runtime through a caller-owned join remains unsuitable for production Bell transition because caller cancellation could lose the allocation after commit. Normal Character Select `Play`, Realm Gate interaction, dungeon admission, extraction, and Recall remain disabled.

## Current Next Step

Convert the existing session-owned 30 Hz danger task in place from microrealm to `CorePrivateFixedDungeonRuntime` only after the durable Bell result commits or replays. Keep unknown outcomes frozen, preserve one observer/writer across reconnect, and prove cancellation/shutdown cannot detach the allocation. Then bind server-generated movement/combat frames and the durable B4 Bargain result before constructing Caldus and reward/terminal authority.
