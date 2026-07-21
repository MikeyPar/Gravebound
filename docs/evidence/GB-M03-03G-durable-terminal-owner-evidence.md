# GB-M03-03G durable terminal-owner evidence

## Authority

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `DTH-010`, `DTH-020`, and `TECH-021`-`023` require authoritative clocks, lethal-first terminal ownership, durable evidence, and exact retry behavior.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001`, `CONT-BOSS-001`, and `CONT-ECHO-001` bind the Core private route, reward-qualified deeds, memorial presentation, and Echo projection.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-06`, `GB-M03-08`, and `GB-M03-13` require the playable route, atomic death, shared terminal arbitration, and in-transaction Echo persistence.

## Implemented boundary

Commit `8cd626e` adds `PostgresCorePrivateTerminalOwnerFactory` and constructs it inside the dormant all-or-nothing persistent foundation. It resumes the exact PostgreSQL life-clock and live-trace heads from the opaque danger binding. A nonlethal frame acknowledges only after its clock interval and any damage evidence commit. B3 and Caldus controls acknowledge only after eligible terminal reward evidence projects into the durable deed aggregate.

Lethal damage is never written through the standalone trace path. The owner stages the exact trace with its immutable entity map, invokes `PostgresPrivateDeathContextPlanner`, submits the sealed candidate through the complete five-producer barrier, executes the single death transaction, and acknowledges only with the stored matching lethal receipt. Ambiguous clock, deed, trace, and death outcomes retry the retained request rather than generating new identities or plans. Normal admission remains disabled because the other four live producer inputs are not yet composed into this owner.

Commit `50d858a` adds a read-only, generation-bound Recall projection. The Recall actor publishes `Inactive`, `Channeling`, or `CompletionPending`; the driver samples it through the same retained-input boundary used by simulation and terminal context. `Channeling` applies 7,500 movement basis points and suppresses primary/ability actions before simulation. No client or transport path can author the projection.

Commit `5d9dc33` adds one serializable current-danger terminal snapshot. The same read owns the active lineage/restore root, content revision, all five extraction and all eight Recall aggregate versions, current acknowledged lifetime/combat clocks, bounded pending counts, and exact pending custody. Mixed clock, aggregate, or pending projections fail closed before producer evaluation.

Commit `1ec0285` injects the exact route-generation-bound Recall actor into the terminal owner. Every acknowledged frame refreshes the actor's future pending projection from the coherent snapshot, then evaluates lethal health, explicit Recall, and LinkLost disconnect recovery in canonical producer order. A stored Recall publishes only after the Recall transaction commits; a same-tick death remains the prepared winner and continues through the atomic death/Memorial/Echo transaction. Extraction and verified-fault restoration remain explicit absences until their process-owned sources are composed.

## Production-blocking verification

Per the owner's reduced-test instruction, full workspace, hosted PostgreSQL/QUIC, journey, performance, and visual suites remain deferred until implementation completion. The changed production blockers passed:

- `cargo clippy -p server_app --lib -- -D warnings` after each slice;
- `cargo test -p server_app --lib recall_projection_constrains_the_same_frame_and_trace_context`;
- `cargo test -p server_app --lib intent_actor_binds_transport_to_one_server_owned_character_snapshot`;
- `cargo test -p persistence --lib terminal_snapshot_rejects_any_mixed_tick_or_aggregate_view`;
- `cargo test -p server_app --lib lethal_death_wins_same_tick_without_touching_recall_writer`;
- `cargo test -p server_app --lib link_lost_tick_ninety_commits_after_tick_eighty_nine_absence`;
- `cargo test -p server_app --lib missing_terminal_owner_fails_before_microrealm_driver_spawn`;
- formatting and whitespace validation.

## Current Next Step

Route the prepared Caldus extraction actor into this owner, keep a 30 Hz terminal heartbeat alive after Caldus reaches exit-ready, and add a typed verified-fault restoration producer. Keep normal admission disabled until all five producers share this coordinator and focused restart, response-loss, and shutdown cases pass.
