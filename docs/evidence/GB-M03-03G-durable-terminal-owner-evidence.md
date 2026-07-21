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

Commit `fa44c81` keeps a neutral 30 Hz terminal heartbeat alive after Caldus reward commitment. Combat stays frozen, but authoritative ticks, Recall state, terminal precedence, and shutdown ownership continue. Extraction reserves the first tick that cannot already be awaiting acknowledgement, eliminating the previously possible stale sealed-tick race.

Commit `8c4c907` injects the dynamic, route-bound extraction directory into the owner before the later Caldus actor exists. Once an authenticated intent is accepted, repository planning is retried server-side against the same durable frame and target tick. The owner submits its sealed candidate beside death and Recall, executes only a prepared extraction winner, publishes only the executor's opaque proof, and retires the actor/route after commit. A death or Recall winner retires any registered extraction actor as a loser.

Commit `5d5fad6` corrects terminal-feed acknowledgement so a matching nonlethal extraction, Recall, disconnect, or verified-fault receipt can close a nonlethal frame, while route controls and lethal frames retain their stricter disposition contracts.

Commit `b252a3b` adds the fifth live producer. A pure microrealm, fixed-room, Caldus, or exit-heartbeat runtime fault emits a typed next-unsealed-boundary delivery; planned shutdown, terminal-feed failure, and indeterminate durable-control failure do not. The owner persists that boundary, loads the coherent danger snapshot, evaluates extraction and both Recall producers alongside the fault candidate, then executes the stable per-root TECH-023 transaction only if fault restoration wins. Fresh and exact-replay receipts must report `Restored`; superseding terminal or conflicting results fail closed. Before acknowledging, the runtime bootstrap retires the exact danger lease, verifies durable Hall state, and installs a fresh Hall actor.

## Production-blocking verification

Per the owner's reduced-test instruction, full workspace, hosted PostgreSQL/QUIC, journey, performance, and visual suites remain deferred until implementation completion. The changed production blockers passed:

- `cargo clippy -p server_app --lib -- -D warnings` after each slice;
- `cargo test -p server_app --lib recall_projection_constrains_the_same_frame_and_trace_context`;
- `cargo test -p server_app --lib intent_actor_binds_transport_to_one_server_owned_character_snapshot`;
- `cargo test -p persistence --lib terminal_snapshot_rejects_any_mixed_tick_or_aggregate_view`;
- `cargo test -p server_app --lib lethal_death_wins_same_tick_without_touching_recall_writer`;
- `cargo test -p server_app --lib link_lost_tick_ninety_commits_after_tick_eighty_nine_absence`;
- `cargo test -p server_app --lib missing_terminal_owner_fails_before_microrealm_driver_spawn`;
- `cargo test -p server_app --lib defeat_freezes_evidence_and_only_exact_durable_result_unlocks_exit`;
- `cargo test -p server_app --lib terminal_owner_retries_the_exact_accepted_tick_without_client_replanning`;
- `cargo test -p server_app --lib dynamic_runtime_reuses_session_writer_replays_commit_and_retires_route`;
- `cargo test -p server_app --lib nonlethal_frame_accepts_matching_durable_terminal_owner`;
- `cargo test -p server_app --lib preparation_is_deterministic_and_binds_fault_boundary`;
- `cargo test -p server_app --lib staged_runtime_fault_reaches_next_terminal_boundary_and_requires_durable_owner`;
- formatting and whitespace validation.

## Current Next Step

Compose the completed five-producer owner into ordinary authenticated dispatch and capability admission for the full private loop. Keep normal admission disabled until that composition is production-ready; run the deferred hosted PostgreSQL/QUIC, restart, response-loss, shutdown, journey, performance, and visual audit after implementation completion.
