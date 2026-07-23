# GB-M03-09 terminal-origin telemetry evidence

## Authority and outcome

This evidence is governed by:

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `DTH-010`, `DTH-011`, `TECH-021`-`023`, `TECH-123`, and `TEL-001`-`005` require terminal truth to be committed, auditable, privacy-safe, and independent of telemetry availability.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-CATALOG-003`, `CONT-ROOM-007`, and `CONT-BOSS-001` define the exact Core successor and terminal content boundaries; telemetry cannot invent alternate outcomes or content identity.
3. `Gravebound_Development_Roadmap_v1.md`: roadmap `ADR-005` and `GB-M03-06`-`09` require the complete private-life terminal loop plus versioned, optional telemetry.

Additive schema 0073 binds only newly committed `death_committed`, `extraction_committed`, Emergency Recall/disconnect-recovery, and `successor_created` outbox facts to the exact durable schema-70 telemetry-session interval covering their PostgreSQL transaction. It does not rewrite terminal payloads, author an outcome, or backfill unknown historical context.

## Contract proof

- Every owning outbox receives an additive nullable `origin_session_id`; death also stores the derived immutable account authority needed for an exact composite session foreign key.
- A bounded two-row MVCC lookup accepts exactly one covering interval. Session start/end boundaries and terminal occurrence timestamps all use PostgreSQL transaction time, so an application-authored clock cannot relabel a terminal. No interval or overlapping interval produces `NULL` rather than a guessed session; interval ends are half-open for an exact handoff.
- Every binding trigger catches telemetry-side failure and returns the original gameplay row with no origin. Telemetry cannot reject death, extraction, Recall, or successor creation.
- Dedicated update guards make origin fields immutable while the owning outbox's existing one-way `published_at` rule remains authoritative.
- The terminal adapter no longer accepts build/session/platform/region/environment/cohort context. Its only process input is the separated pseudonymization key; all TEL-001 attribution is joined from the captured durable session.
- Death session duration is computed from the captured PostgreSQL session-start boundary, and item-power band uses the same immutable summary/destruction/item calculation as the durable death/Echo graph.
- Terminal polling remains bounded to 256 unpublished committed rows in deterministic commit/event/family order. Rows without a known origin are intentionally ineligible rather than relabeled from current runtime state.

## Local verification

- `rustfmt --edition 2024` over the schema-73 adapter and five focused integration-test sources: passed.
- `cargo test --locked -p persistence --test postgres_terminal_telemetry_origins --test postgres_durable_death --test postgres_extraction_terminal --test postgres_recall_terminal --no-run`: passed.
- `cargo test --locked -p server_app --test postgres_successor --no-run`: passed.
- `cargo test --locked -p persistence --lib terminal_telemetry_origins_are_additive_immutable_exact_and_optional`: passed (`1` passed).
- `cargo test --locked -p persistence telemetry_outbox::tests`: passed (`4` focused adapter/privacy tests).
- `cargo clippy --locked -p persistence --lib --test postgres_terminal_telemetry_origins --test postgres_durable_death --test postgres_extraction_terminal --test postgres_recall_terminal -- -D warnings`: passed.
- `cargo clippy --locked -p server_app --test postgres_successor -- -D warnings`: passed.
- The ignored `schema_73_uses_one_database_clock_and_fails_open_on_ambiguous_origin` test requires dedicated disposable PostgreSQL through `TEST_DATABASE_URL`. It uses typed session repositories plus an intentionally overlapping closed-session fixture to prove database-clock selection, application-clock skew exclusion, ambiguous fail-open, foreign-account absence, and half-open handoff.
- The four owning hosted journeys use the real terminal repositories. Each has an isolated/reset database, and together they prove fresh terminal commits with captured origins, gameplay commits with no session and `NULL` origin, guard-authoritative origin immutability, adapter loss before acknowledgement, deterministic restart re-poll, exact acknowledgement, and rejection of a second publication. These journeys are compile-checked locally but not claimed as executed without the dedicated database.

## Remaining limitations

- No hosted PostgreSQL execution for schema 73 exists yet. Local static/compile proof does not substitute for applying schemas 72/73 and executing the ignored test plus ordinary death/extraction/Recall/successor transactions.
- Pre-0073 terminal rows remain without telemetry attribution by design; assigning a later session would fabricate TEL-001 context.
- This evidence predates schema 78. Commit `1f1d0fd` now supplies the formerly missing `TEL-003`
  boss, contribution, and network-health facts; the current boundary is recorded in
  [`GB-M03-09-death-context-telemetry-evidence.md`](GB-M03-09-death-context-telemetry-evidence.md).
- The worker remains disabled and has no attached source/exporter until hosted origin, queue-lag/restart, destination, retention, deletion, and privacy-review evidence passes.
