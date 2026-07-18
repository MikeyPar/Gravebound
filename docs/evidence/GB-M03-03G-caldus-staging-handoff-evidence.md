# GB-M03-03G Caldus staging handoff evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md` `DNG-006`, `ENC-005`, `ENC-010`, `SIM-004`, `TECH-012`, and `TECH-015` require one server-owned participant lock, exact countdown/introduction timing, authoritative 30 Hz state, and no client-authored boss transition.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-002`, `CONT-ROOM-007`, and `CONT-BOSS-001` fix the `B5 → B6` route, `arena.boss.caldus_01` geometry, loading/countdown/lock order, and Sir Caldus introduction contract.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires the fixed dungeon and one Core major boss to compose into the ordinary private loop without developer commands while preserving reconnect, cleanup, and terminal authority.

## Delivered contract

Commit `cdeceda` adds the consuming `CorePrivateCaldusStagingHandoff` boundary.

- `CorePrivateFixedDungeonRuntime::into_caldus_staging_handoff` succeeds only when compiled combat is at `CaldusArenaB6`, persistent route authority is exactly `CaldusArenaB6/BossStaging`, and no fixed-room movement owner remains.
- The handoff moves one route directory/lease, content revision, immutable character-combat envelope, mutable player allocation, monotonic hostile-projectile allocator, and inherited danger tick together. Callers cannot clone or reconstruct a partial boss participant.
- `CoreBossLockSimulation::new_at_tick` preserves global time across its exact 150-tick ready countdown and 75-tick Caldus introduction.
- `CoreDevelopmentEncounterRooms::compile_caldus_arena` derives the exact rotated B6 simulation geometry from the compiled fixed layout. Presentation data cannot choose arena dimensions, anchors, spawn, or collision.
- Caldus exposes read-only arena, scheduler state, and validated hurtbox projections needed by the future route owner without opening mutation seams.

## Verification

Local Windows verification on the implementation worktree:

- Inherited-tick boss-lock proof: `1 passed`, with countdown close at `start + 150` and combat activation at `start + 225`.
- Checked-in content proof: exact `arena.boss.caldus_01.combat`, `18_000 × 18_000`, center boss spawn, and eight compiled anchors.
- Full B1 → B6 consuming trace: B3 reward acknowledgement, B4 durable resolution/replay, B5 live clear, exact B6 `BossStaging`, and route/content/character/player/tick continuity all pass.
- `cargo test -p sim_core --lib --all-features`: `390 passed`, `0 failed`.
- `cargo test -p sim_content --lib --all-features`: `132 passed`, `0 failed`.
- `cargo test -p server_app --lib --all-features`: `335 passed`, `0 failed`.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass.
- `cargo fmt --all` and `git diff --check`: pass before commit.

## Presentation candidate

Commit `7669e41` adds the unregistered [`staging_transition/v1`](../../assets/core/bosses/sir_caldus/staging_transition/v1/README.md) pack. Its deterministic verifier passes all 21 hashed files, runtime dimensions, transparent corners, standard/reduced alpha parity, and four certified review dimensions. The 1920×1080 standard/reduced and actual-scale sheets passed visual inspection. The pack remains outside the registry and content hashes until optimized native playback proves authoritative ordering, alignment, accessibility, and timing.

## Explicit boundary

This increment does not construct `CorePrivateCaldusRuntime`, accept boss inputs, advance the boss-lock state, start combat, commit reward/pending inventory, create the stable exit, publish a normal route, or enable admission. Normal gameplay remains fail closed.

## Current Next Step

Construct `CorePrivateCaldusRuntime` from the consuming staging handoff. It must own the exact B6 arena, one inherited-tick boss-lock simulation, one Caldus encounter aggregate, authoritative player input/movement/collision, route compare-and-swap projection, and fail-closed staging/countdown/introduction/combat transitions before any reward or exit authority is composed.
