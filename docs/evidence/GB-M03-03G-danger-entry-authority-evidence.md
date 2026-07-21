# GB-M03-03G opaque danger-entry authority evidence

## Authority

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `DTH-010`, `DTH-011`, `TECH-015`, and `TECH-021`-`023` require one authenticated danger root, lethal-first terminal resolution, response-loss replay, and crash-safe restoration.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001` fix the Core micro-realm, Bell Sepulcher, and Sir Caldus route that must share that root.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-06`, `GB-M03-08`, and `GB-M03-13` require the ordinary private-life route, atomic death, terminal arbitration, and same-transaction Echo projection without mixed or duplicate authority.

## Implemented boundary

Commit `2e02b94` closes the missing provenance seam between the committed Realm Gate transfer and live danger simulation. `CorePrivateRouteEnterMicrorealmTransition` now retains the exact committed entry restore point beside transfer, lineage, character-version, and world-content authority. The route actor records that material only after accepted world-flow reconciliation and treats exact replay as a no-op; changed transfer, lineage, restore point, version, or content fails closed.

`CorePrivateRouteActorDirectory::danger_entry_authority` now returns one opaque, generation-pinned `CorePrivateDangerEntryAuthority`. It is constructed under the actor lock only when account, character, actor generation, committed transition, Core micro-realm projection, route content, and world-flow content still agree. Hall, uncommitted, stale, foreign, retired, or drifted state cannot mint the proof.

`CorePrivateMicrorealmRuntime::new` requires and retains that proof before simulation construction. `CorePrivateTerminalFeedBinding::from_danger_entry` consumes it directly, so later terminal owners no longer need to assemble account, character, lineage, restore-point, lease, and content tuples independently. The raw feed constructor is restricted to crate tests.

Normal admission remains disabled. The persistent session still uses its explicitly named component-only terminal-ownerless spawn until a real owner is installed; this commit does not auto-acknowledge or discard terminal history.

## Production-blocking verification

Per the owner's instruction, broad audit and workspace suites are deferred until GB-M03 implementation is complete. The production-blocking checks for this contract passed:

- `cargo test -p server_app --lib committed_microrealm_entry_reconciles_once_and_exact_replay_never_rewinds`: passed.
- `cargo test -p server_app --lib retained_input_advances_many_ticks_and_commits_the_exact_route_projection`: passed.
- `cargo test -p server_app --lib committed_microrealm_transition_is_exactly_replayable_and_changed_material_fails`: passed.
- `cargo clippy -p server_app --lib -- -D warnings`: passed.
- `cargo fmt --all` and `git diff --check`: passed.

## Current Next Step

Replace `CorePrivateLifeSessionDirectory::bind_microrealm`'s component-only ownerless spawn with a mandatory process-owned terminal-owner factory. Start the receiver before the driver, retain and join it across `LinkLost`, reconnect, unbind, and shutdown, and fail before the first tick when no owner exists. The first production implementation must consume the opaque danger authority and join the live damage trace, independent lifetime/permadeath clocks, deeds, custody, per-tick network/Recall/status providers, immutable simulation-to-journal entity identities, and all five terminal producers. Keep normal admission disabled until that owner graph is complete.
