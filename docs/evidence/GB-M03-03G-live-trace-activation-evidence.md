# GB-M03-03G live-trace activation authority evidence

## Authority

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-004`, `DTH-001`, `DTH-010`, and `TECH-021`-`023` require server-owned entity identity, exact ordered damage history, and durable danger/restart binding.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001` fix the promoted Core sources whose trace identities must remain stable from micro-realm through Caldus.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-06`, and `GB-M03-13` require the playable private route, durable combat trace, atomic death, and Echo projection.

## Implemented boundary

Commit `8107752` lets the production owner open `LiveDamageTraceService` directly from the current transaction-validated PostgreSQL danger snapshot. Callers provide only the opaque terminal binding and the minimum committed entry character version; the repository supplies and validates the exact current character version, lineage checkpoint tick, restore point, and promoted Core content. Foreign roots and version regression fail before an aggregate opens.

The same commit adds immutable private-route simulation-to-journal entity identity derivation. Account, character, lineage, restore point, durable actor generation, and simulation entity ID are sealed into one reconnect-stable server identity. A complete damage frame registers source and player target mappings together, reuses exact mappings, and fails closed on collision. Clients cannot author either axis.

This closes activation and entity-provenance prerequisites. It does not yet claim the production owner loop, per-tick network/Recall/status projection, independent clocks, lethal durable commit, or terminal acknowledgement.

## Production-blocking verification

Per the owner's instruction, broad workspace/audit suites remain deferred until implementation is complete. The focused gates passed:

- `cargo test -p server_app --lib current_danger_open_uses_repository_checkpoint_and_rejects_foreign_root`: passed.
- `cargo test -p server_app --lib danger_generation_derives_stable_distinct_entity_journal_ids`: passed.
- `cargo clippy -p server_app --lib -- -D warnings`: passed.
- `cargo fmt --all` and `git diff --check`: passed.

## Current Next Step

Commit `e869f01` closes the first receiver-loop prerequisite through the [same-boundary terminal tick context](GB-M03-03G-terminal-tick-context-evidence.md): network transitions are lossless, status input is bounded and explicitly empty for current Core, and Recall has a non-client-authored field awaiting its actor proxy. Next build the PostgreSQL death-context planner, register immutable frame identities, persist/replay trace and independent clocks, connect Recall phase, and hold lethal acknowledgement until the durable death transaction returns the exact stored receipt. Then add extraction, Recall, disconnect-recovery, and server-fault producers to the same arbiter.
