# GB-M03-03G fixed-dungeon combat owner evidence

## Authority

This slice was reviewed against all three governing documents together:

1. `Gravebound_Production_GDD_v1_Canonical.md` `DNG-003`-`006`, `COM-001`-`006`, `BRG-001`-`002`, and `LOOT-002` require authoritative room activation/completion, participant locking, reset cleanup, one server-owned combat state, an explicit Bargain outcome, and no reward or exit inferred by the client.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-007` fixes `layout.core_private_life_01` as `B0 -> B1 -> B2 -> B3 -> B4 -> B5 -> B6`, with combat only in B1/B2/B3/B5, one B4 rest/Bargain room, and Caldus in B6. `CONT-BOSS-001` requires the stable B6 exit only after committed boss reward.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires one ordinary Character Select -> Hall -> micro-realm -> fixed six-room dungeon -> boss -> terminal route without developer commands.

## Delivered contract

Commits `42d9f85` and `9694790` add the lifecycle-free fixed-dungeon ownership foundation while normal admission remains disabled.

- `CoreImmutableFixedRoomSimulation`, `CoreB2FixedRoomSimulation`, and `CoreB3FixedRoomSimulation` now consume into a `NormalWaveHandoff` only after the authoritative room reaches `Cleared`. Dormant, warning, active, reset, and quiet states fail closed.
- Completed handoffs preserve the exact player entity and one monotonic hostile-projectile allocator. No room boundary clones or reconstructs mutable combat state.
- `CoreFixedDungeonCombat` compiles the exact B1/B2/B3/B5 plans from the immutable Core content and owns one capacity-one state across B0 vestibule, B1, B2, B3, B4 rest, B5, and B6 staging.
- Each combat-room step is typed by node. Hostile identities are derived from the active simulation, sorted for deterministic server collision/render consumers, and unavailable in safe/staging states.
- Route advance is staged on a clone and commits only after the current room's complete quiet period opens its doors. Early advance leaves the original owner unchanged.
- B3's reward handoff occurs before the participant can enter B4. B4 stores one explicit `BargainSelected`, `BargainRefused`, or authoritative `NoOffer` result; exact replay succeeds and changed resolution conflicts.
- B6 exposes the participant handoff only after B5 reaches committed `Cleared`. The component does not construct Caldus, commit rewards, expose the stable exit, or write persistence.

## Verification

Local Windows verification at `9694790`:

- Focused fixed-room handoff tests: pass for dormant/active/quiet rejection, B1/B2/B3 completed transfer, player identity continuity, and monotonic hostile-projectile allocation.
- Full-chain trace: pass for B0 -> B1 -> B2 -> B3 -> B4 -> B5 -> B6 staging, exact authored warning/invulnerability boundaries, B3 reward handoff, early-advance rejection, B4 exact replay/conflict, and final identity continuity.
- `cargo test -p sim_content --lib`: `129 passed`, `0 failed`.
- `cargo clippy -p sim_content --all-targets --all-features -- -D warnings`: pass.
- `cargo fmt --all` and `git diff --check`: pass.

Hosted runs [`29642037538`](https://github.com/MikeyPar/Gravebound/actions/runs/29642037538) for `42d9f85` and [`29642381573`](https://github.com/MikeyPar/Gravebound/actions/runs/29642381573) for `9694790` were in progress when this record was written and are not claimed green before completion.

## Explicit boundary

This component is not yet bound to the persistent `CorePrivateRouteActor`, the session-owned independent 30 Hz driver, real QUIC, PostgreSQL reward/pending-inventory transactions, Sir Caldus, the stable exit, or the shared five-producer terminal coordinator. It cannot enable Character Select `Play`, production Realm Gate interaction, normal extraction/Recall, or any other ordinary route capability.

## Current Next Step

Bind `CoreFixedDungeonCombat` to the persistent route actor and session-owned 30 Hz driver so a cleared microrealm transfers the exact mutable allocation into B0 and server-derived room events advance B1-B5. Then add authoritative Sir Caldus combat, committed reward/pending-inventory placement, stable B6 exit, and all five terminal producers before opening the ordinary route.
