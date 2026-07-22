# GB-M03-09 immutable loot-origin telemetry evidence

## Authority

This slice is governed by all three design authorities:

1. `Gravebound_Production_GDD_v1_Canonical.md`: `LOOT-010`, `ECO-002`, `TECH-123`, and `TEL-001`-`005` require authoritative item lifecycle facts, privacy-safe context, typed lifecycle events, and a product gate based on committed item history.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-REWARD-001`-`004` own the exact reward-table and stable-content identities. Telemetry may report those committed IDs but cannot invent or localize them.
3. `Gravebound_Development_Roadmap_v1.md`: `ADR-005` and `GB-M03-09` require versioned loot telemetry, privacy filtering, bounded batching, and disabled/offline gameplay operation.

`ADR-039-telemetry-outbox-privacy-and-retention.md` remains the accepted ingestion, redaction, delivery, and retention boundary.

## Implemented boundary

Additive schema `0071_m03_loot_telemetry_origin_v1.sql` treats `item_ledger_events` as the only item-lifecycle authority. An `AFTER INSERT` projector conditionally emits immutable sidecar rows in the owning gameplay transaction:

- `item_created` comes from ledger creation;
- `item_picked_up` comes from a personal-ground transition;
- `item_equipped` comes from a transition into the equipment location;
- `item_extracted` comes from the canonical extraction source kind; and
- `item_destroyed` comes from destruction, consumption, or crash-revocation tombstones.

A direct ground-to-equipment transition produces both picked-up and equipped facts with separately domain-derived event IDs. Each row stores the ledger ID, item UID, template ID, exact reward-table or starter source, item version, occurrence time, account/character binding, and exact eligible durable session. Build, content bundle, platform, region, environment, and cohort context is joined only through that stored session ID.

The projector performs a nonlocking MVCC lookup capped at two candidates and emits only when exactly one durable session interval covers the ledger transaction timestamp. This binds an in-flight mutation to its original session even if that session closes and a replacement starts concurrently. No eligible session, corrupt overlap, or any telemetry-side projection error cleanly produces no sidecar row; the item/reward write remains authoritative and successful. Existing pre-schema-71 item history is not backfilled because its origin session cannot be proven.

Hosted run [`29906346469`](https://github.com/MikeyPar/Gravebound/actions/runs/29906346469) proved that the reward, ledger, eligible session, and immutable source identities were present, then exposed one exact schema-71 projection defect: PostgreSQL resolves the trigger's numeric literals as `INTEGER`, while the immutable event-ID function originally accepted only `SMALLINT`. The trigger's fail-open handler correctly preserved gameplay but emitted no sidecar. Additive schema `0072_m03_loot_telemetry_literal_compatibility_v1.sql` adds only an immutable, strict `INTEGER` overload that casts and delegates to the original `SMALLINT` authority. It does not replace schema 71, mutate a table, rewrite history, or weaken gameplay availability.

The persistence poll is statically bounded to 256 rows, reads only the immutable sidecar plus its bound session context, and never consults mutable `item_instances`, live world/session state, or raw authentication/network data. The export adapter converts account IDs to keyed pseudonyms and maps only typed `LootEventV1` fields. Acknowledgement accepts only exact IDs returned by the adapter's in-flight ledger and atomically advances only `published_at`; failed, absent, or lost responses leave rows restart-eligible.

## Focused verification

Local production-blocking checks for this slice:

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --locked -p persistence --lib --tests --no-deps -- -D warnings`: pass.
- `cargo test --locked -p persistence --test postgres_foundation --test postgres_telemetry_sources --no-run`: pass, including the exact schema-71 table manifest.
- Focused schema contract, committed-source polling boundary, and event-mapping unit tests: `3 / 3` pass.
- Schema-72 additive compatibility contract plus the retained schema-71 contract: `2 / 2` pass.
- `git diff --check`: pass before commit.

The ignored disposable-PostgreSQL test target now includes:

- a typed reward commit that produces one exact session-bound `item_created` source;
- exact reward retry without a second ledger/telemetry fact;
- changed-payload replay conflict;
- rejection of sidecar payload mutation;
- process-close/reconnect re-poll from the same stored session/build/content/platform/region/environment/cohort context;
- redacted adapter serialization and exact one-way acknowledgement; and
- a separate no-session fixture proving that the reward and ledger commit while no loot sidecar is emitted.

The PostgreSQL journey remains ignored by default; its target compilation passes. Run `29906346469` supplied the exact failing PostgreSQL signature, while execution of the additive schema-72 repair is pending the next hosted run before operational credit. Remote export remains disabled; no destination, processor, or retention claim is made here.

## Current Next Step

Following `Gravebound_Production_GDD_v1_Canonical.md`, `Gravebound_Content_Production_Spec_v1.md`, and `Gravebound_Development_Roadmap_v1.md`, execute both schema-71/72 disposable-PostgreSQL telemetry journeys in hosted CI and record the exact source commit/run. Then commit terminal-family durable origin binding and prove bounded lag/restart behavior through the already-owned disabled production worker. Do not enable remote export until the `ADR-039` destination, region, access, encryption, retention, deletion, backup-expiry, and privacy-notice review passes.
