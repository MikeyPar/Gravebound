# GB-M03-03G live microrealm authority evidence

**Result:** Local implementation pass for the live capacity-one microrealm owner at source `37cf99d`. This advances `GB-M03-03G`; it does not enable normal route admission or close the package.

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md`: `LOOP-001`-`003` and `TECH-010`-`023` require server-owned movement, collision, encounter state, and terminal decisions.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001` and `CONT-WORLD-004` own the exact capacity-one Core microrealm, spawn, route geometry, lifecycle, and Bell portal.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` requires the ordinary private loop to run without developer commands while retaining deterministic, fail-closed authority.

## Implemented contract

- `CorePrivateMicrorealmRuntime` owns the compiled `world.core_microrealm_01` scene, the exact 170 milli-tile Grave Arbalist movement step at 30 Hz, radius-aware collision, one authoritative player position, and the `Dormant -> Waiting -> Active -> Cleared` lifecycle.
- Client-shaped input may supply only monotonic sequence/tick, bounded displacement, and primary-release intent. Pack completion requires an exact server-created clear proof bound to character, actor generation, danger lineage, and tick.
- Movement and lifecycle changes stage on clones. The route actor then applies the exact expected state version, target phase, and Bell range in one mailbox command under one actor lock. Local state swaps only after that command succeeds.
- Bell eligibility is derived from the compiled Bell Sepulcher circle after authoritative clear. The client does not author portal range, lifecycle phase, clear state, or a route projection.
- Actor retirement, transfer reservation, terminal reservation, stale versions, foreign clear proofs, invalid scene/content/generation authority, oversized movement, nonmonotonic ticks, and stale/zero input sequences fail closed.

## Verification

- Focused live-runtime tests: `4/4` pass.
- Complete server library suite: `305/305` pass.
- Every enabled server integration target passes; explicitly PostgreSQL-gated tests remain ignored locally and are owned by hosted CI.
- Strict `cargo clippy -p server_app --all-targets -- -D warnings`: pass.
- `cargo fmt --all -- --check` and `git diff --check`: pass.

The focused matrix proves exact movement/lifecycle-to-route synchronization, server-clear binding, the complete spawn-to-Bell movement path, Bell-range derivation, stale-input rejection, foreign-proof rejection, stale actor-version rejection, and rollback of staged local movement/lifecycle when route authority has changed.

## Remaining integration gate

The owner is not yet attached to `CorePrivateLifeSessionDirectory`, does not yet receive a clear proof from live `CoreEnemySimulation`, and does not yet publish the route snapshot over the ordinary QUIC connection. Fixed B1-B6 combat, rewards, pending inventory, damage/death, Recall/extraction competition, reconnect/restart, and native control remain open. Therefore `core_world_flow_integration`, Character Select `Play`, production Realm Gate interaction, and normal departure stay disabled.
