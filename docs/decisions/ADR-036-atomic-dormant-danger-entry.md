# ADR-036 - Atomic dormant danger entry

Status: Accepted

Implementation package: `GB-M03-03B`

## Context

The canonical GDD requires a complete entry restore point before a durable character enters danger, with progression, inventory/Belt, and Oath/Bargain state captured under one authority boundary. The Content Production Specification fixes the Core Hall Realm Gate destination to `world.core_microrealm_01` and `layout.core_private_life_01` and binds the route to the exact independently hashed world-flow records, assets, and localization. The Development Roadmap requires reliable explicit transfers, retry safety, restart durability, and a fail-closed normal route until the later item, death, extraction, and Recall packages are complete.

The persistence layer already owned durable locations, capacity-one lineages, complete restore roots, and immutable receipts. Its callback-shaped transaction helper could not safely lend a serializable SQL transaction across three asynchronous restore providers. Inserting a provisional root first was also invalid: it could expose a restore point whose children had not all been captured.

Owner-approved `SPEC-CONFLICT-006` and `SPEC-CONFLICT-010` authorize a dormant disposable coordinator and exact revision/arrival contracts without enabling the normal player route.

## Decision

1. World-flow persistence exposes an owned `WorldFlowWrite` session. It retains the serializable PostgreSQL transaction and locked aggregate state until the caller either consumes it through `commit` or drops it to roll back.
2. Session acquisition locks account, character/status, location, and receipt authority in the established order. Exact replay is resolved before current-state validation and returns without allocating IDs or invoking restore providers.
3. A fresh danger entry allocates distinct nonzero transfer, lineage, and restore-point IDs. The lineage is capacity one and binds the exact character, source, destination, and three-part world-flow content revision.
4. Progression, inventory/Belt, and Oath/Bargain providers capture directly into the same SQL transaction. Each provider returns a typed positive component version and digest; missing, malformed, foreign, stale, or incomplete captures reject before publication.
5. The complete restore root is inserted only after all three component rows exist. Its digest is derived from the exact typed snapshot envelope, not from caller-selected or placeholder bytes.
6. The root-to-progression foreign key is `DEFERRABLE INITIALLY DEFERRED`. This permits child-first staging while still requiring a complete, referentially valid graph at commit. No partial root is visible outside the transaction.
7. The danger location, accepted receipt, capacity-one lineage, three restore components, and complete root commit together. Any provider, validation, SQL, serialization, or commit failure drops the session and publishes none of them.
8. Reusing the request with identical canonical material returns the immutable receipt after process/pool restart. Reusing its mutation identity with changed material returns an idempotency conflict.
9. Persisted projections validate exact destination IDs, layout, revision triple, positive versions, nonzero digests, ownership, and location coherence before use. Corrupt or foreign state fails closed.
10. The production endpoint remains gated before transaction acquisition and ID generation, continues to return typed `StageDisabled`, and does not advertise `core_world_flow_integration`. The dormant coordinator is available only to explicitly disposable integration fixtures.

## Rejected alternatives

- A boxed asynchronous callback over a borrowed transaction was rejected because its lifetime surface was difficult to compose across independent providers and encouraged hidden nested transactions.
- A placeholder or provisional restore root was rejected because a crash could make incomplete recovery state appear authoritative.
- Independent provider transactions were rejected because they could commit mutually inconsistent versions and leave danger admission without a recoverable snapshot.
- Enabling Character Select `Play` or the Hall Realm Gate was rejected because `GB-M03-04`, `06`, and `08` still own required inventory, death, extraction, and Recall semantics.

## Consequences

- A committed danger location always has one complete, content-bound entry restore graph and immutable accepted receipt.
- Provider implementations remain replaceable behind typed transaction-bound seams as the production aggregates mature.
- Deferred referential validation preserves strict database integrity without exposing provisional state.
- Replay, conflict, concurrency, rollback, restart, and corruption behavior can be proven against disposable PostgreSQL.
- Hall/microrealm simulation, presentation, normal-route admission, death, extraction, Recall, and Core promotion remain outside this decision.
