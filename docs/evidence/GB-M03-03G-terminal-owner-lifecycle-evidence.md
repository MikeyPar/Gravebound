# GB-M03-03G mandatory terminal-owner lifecycle evidence

## Authority

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `DTH-010`, `DTH-011`, `TECH-015`, and `TECH-021`-`023` require one continuously authoritative life owner, lethal-first terminal resolution, disconnect vulnerability, and durable retry/recovery.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001` define the exact Core micro-realm through Caldus event stream that the owner consumes.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-06`, `GB-M03-08`, and `GB-M03-13` require the private-life loop, death, terminal arbitration, and Echo projection to share one fail-closed authority graph.

## Implemented boundary

Commit `0991cd6` removes ownerless danger-driver construction from every production build. `CorePrivateLifeSessionDirectory::bind_microrealm` now requires a `CorePrivateTerminalOwnerFactory`; without one it returns `TerminalOwnerUnavailable` before spawning the driver or binding the authoritative tick.

When configured, the session constructs `CorePrivateTerminalFeedBinding` from the exact opaque committed danger authority, opens the capacity-one receiver, starts the owner, and only then starts the driver. The owner is transport-independent and remains stored beside the driver across `LinkLost` and reconnect.

Unbind and process shutdown preserve the required order: B3/Caldus producers stop, the driver shuts down while the terminal receiver remains available to acknowledge its last committed delivery, the terminal owner joins, and the authoritative tick binding retires last. Start, runtime, and join failures are typed and contribute to zero-residue failure.

There is deliberately no production discard or automatic-acknowledgement owner. The small acknowledgement consumer exists only inside the session unit-test module. Normal route admission therefore remains disabled and fails closed until the real live trace/clock/five-producer implementation installs the factory.

## Production-blocking verification

Per the owner's instruction, broad workspace/audit suites remain deferred until GB-M03 implementation is complete. These changed-contract gates passed:

- `cargo test -p server_app --lib missing_terminal_owner_fails_before_microrealm_driver_spawn`: passed.
- `cargo test -p server_app --lib live_driver_tick_is_route_bound_and_gates_recall_until_first_commit`: passed.
- `cargo test -p server_app --lib live_microrealm_survives_handoff_and_link_lost_until_exact_unbind`: passed.
- `cargo clippy -p server_app --lib -- -D warnings`: passed.
- `cargo fmt --all` and `git diff --check`: passed.

## Current Next Step

Commit `8107752` adds [transaction-validated live-trace activation and immutable simulation-to-journal identity](GB-M03-03G-live-trace-activation-evidence.md), and commit `e869f01` adds the [same-boundary terminal tick context](GB-M03-03G-terminal-tick-context-evidence.md). Commits `234fa2c`, `ff22530`, `3238a0a`, and `8d71a04` now add [commit-time Echo revalidation and the sealed PostgreSQL death-context planner](GB-M03-03G-death-context-planner-evidence.md). Next install that planner in this mandatory production owner, persist clocks/deeds continuously, wire the server-only Recall phase, and hold lethal acknowledgement for the exact stored receipt. Keep normal admission disabled until all five terminal producers and three-scene shutdown/restart behavior pass together.
