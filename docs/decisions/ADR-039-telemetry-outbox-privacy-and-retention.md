# ADR-039 - Telemetry outbox, privacy, and retention boundary

**Status:** Accepted on 2026-07-21

**Owners:** Telemetry, persistence, server, privacy, operations

**Applies to:** `GB-M03-09` and the logical telemetry/crash/privacy decision identified as `ADR-005` in the development roadmap

## Authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: `TECH-005` keeps telemetry inside the modular-monolith boundary; `TECH-123` defines privacy and pseudonymization requirements; `TEL-001` defines the common envelope; `TEL-002`-`005` define event families, death detail, KPI uses, and validation gates.
- `Gravebound_Content_Production_Spec_v1.md`: the Core stable IDs, exact content revision, and item/death lifecycle boundaries are the only content authority represented by M03 telemetry. Telemetry cannot invent content identity or gameplay outcomes.
- `Gravebound_Development_Roadmap_v1.md`: roadmap `ADR-005` and `GB-M03-09` require versioned telemetry, crash handling, bounded batching, privacy filtering, and operation when telemetry is disabled or offline. Repository ADR numbers are monotonic, so this implementation record is `ADR-039`.

## Context

Telemetry is useful only if it describes committed gameplay truth without becoming another writer, a source of duplicate outcomes, or a route for credentials and personal data to leave the service. A live gameplay callback is not a safe ingestion boundary because its transaction may still roll back. Likewise, a generic JSON or logging API makes it too easy to serialize an account identifier, network address, authentication material, email, platform identity, free-form crash message, or stack trace.

M03 also has to remain playable when analytics is disabled, disconnected, or backpressured. Remote collector selection and its legal retention/deletion policy are hosting decisions that have not yet been authorized. The package therefore establishes a strict local contract without silently creating an external data processor.

## Decision

### Versioned event contract

- The `telemetry` workspace crate owns schema version 1 envelopes for onboarding, session, loot, extraction, Emergency Recall, death, successor, and crash events.
- Every exported envelope contains the `TEL-001` fields: event ID, derived event name, schema version, UTC occurrence time, pseudonymous account ID, optional character ID, session ID, build ID, content bundle version, platform, region, environment, and bounded canonical cohort tags.
- Event names are derived from typed variants. Callers cannot provide a name that disagrees with the payload.
- Stable labels are length- and character-bounded. Death collections are bounded and canonically ordered. A `server_fault` cannot be recorded as a final player death.
- Protocol evolution is append-only: a changed shape receives a new envelope/event version. Version 1 fields and meanings are not repurposed.

### Committed-outbox-only ingestion

- The pipeline accepts only `CommittedOutboxEventV1`, constructed by a persistence adapter after the transaction containing the source outbox row has committed.
- There is no ingestion method for a bare event envelope or live gameplay state. Telemetry reads committed records and cannot mutate gameplay, author outcomes, or acknowledge a domain outbox row before exporter acceptance.
- Outbox ID is the queue idempotency key. Duplicate polls do not create duplicate queued documents. Delivery acknowledgement removes only the exact accepted IDs; absent or failed IDs remain pending and their durable source rows remain unpublished.
- Additive schema 0070 owns durable logical telemetry sessions and typed onboarding, session, and crash source rows. Account creation, character creation, and first combat are projected by triggers in the owning gameplay transaction; session transitions and redacted crash observations are accepted only through typed persistence commands. These are domain-source facts, not analytics decisions.
- Each schema-0070 source row binds the immutable origin session and joins its build, content bundle, platform, region, environment, and cohort context when polled. Delayed export, reconnect, or process restart therefore cannot relabel old facts with current live-session metadata.
- Additive schema 0071 projects typed loot facts only from a newly committed `item_ledger_events` row. Its immutable sidecar captures the ledger identity, the one durable session interval covering that ledger transaction, item UID, template, reward/starter source, item version, action, and occurrence time. A bounded nonlocking lookup prevents a later session from relabelling an older in-flight mutation. It never writes item/reward history and never backfills unknown historical context. Missing, ambiguous, or unavailable telemetry context skips projection without rejecting the gameplay write or waiting on session shutdown.
- Additive schema 0072 repairs only PostgreSQL's integer-literal resolution for the schema-71 event-ID call by delegating a strict immutable `INTEGER` overload to the original `SMALLINT` function. It changes no table, gameplay transaction, source identity, historical row, or publication rule.
- Session start and transition identities are durable and replay-safe; durations and link-loss intervals are server-derived. Crash rows accept only typed source/kind/reporter values and a fixed nonzero signature. Raw message, stack, path, network, credential, and arbitrary-property columns are structurally absent.
- The production-root server owns one optional logical-session coordinator. It starts after handshake acceptance but before account/bootstrap writes, consumes only the derived account ID and accepted client platform, inspects the durable session head on restart, and uses generation leases so stale transports cannot report false disconnects or exits. `Disconnected` roots reconnect in place; orphaned active roots end as `TransportClosed` before replacement; native clean exit and graceful server shutdown have explicit terminal reasons.
- The M03 PostgreSQL adapter polls the existing `death_outbox_events`, `extraction_terminal_outbox_events_v1`, `recall_terminal_outbox_events_v1`, and `successor_mutation_outbox_events_v1` families in committed order. It decodes their canonical stored payloads and, for death only, reads the immutable death/summary/trace snapshot graph needed by the typed event. It never reads mutable world, inventory, actor, or session state.
- Source publication is a separate PostgreSQL transaction after exporter acceptance. Every accepted event ID must belong to the adapter's exact in-flight set; every corresponding `published_at` transition must succeed or the acknowledgement transaction rolls back. Export failure, response loss, adapter loss, or process restart leaves the source row eligible for deterministic re-poll.
- The existing terminal-family projection context temporarily requires the runtime to supply its already-authoritative session, build, content bundle, platform, region, environment, and cohort attribution. Schema-0070 and schema-0071 sources instead carry immutable per-row origin context. Terminal sources must adopt equivalent durable binding before runtime export is enabled; no adapter may reconstruct missing session facts from live state. Raw account IDs are consumed only inside the adapter to produce keyed pseudonyms and never cross into an event envelope.

### Structural privacy boundary

- The common account field is a fixed-width, nonzero pseudonymous value produced upstream in a separated identity domain. It is not a reversible encoding of an account, email, or platform identifier.
- Export serialization is private to the redacted pipeline path. Public event structures do not implement a general envelope serializer.
- No public field exists for raw account ID, IP/socket address, device fingerprint, authentication ticket/token, email, platform identity, free-form player text, crash message, process argument, file path, or stack trace.
- Crash events contain a typed source/kind and a fixed-width nonzero, non-reversible signature created by an approved collector. Raw diagnostic text requires a later privacy-reviewed, explicit opt-in design.
- Staff/test cohort tags remain explicit so KPI consumers can exclude them as required by `TECH-123`; the telemetry package does not make product decisions from those tags.

### Disabled, offline, and bounded operation

- Disabled mode accepts nothing, retains nothing, exports nothing, and never blocks gameplay.
- Logical-session collection is disabled unless `GRAVEBOUND_TELEMETRY_ENVIRONMENT` is explicitly valid. Local/test may use the canonical local-playtest region; staging/production also requires a validated `GRAVEBOUND_TELEMETRY_REGION_ID`. Invalid or missing attribution disables collection instead of guessing. Runtime persistence calls are bounded and failures cannot reject authentication, bootstrap, or gameplay.
- Offline enabled mode holds at most 4,096 committed projections in memory and emits no batch until connectivity is restored.
- A full queue applies explicit backpressure without evicting an older committed event. The durable source outbox remains the recovery authority.
- Export batches contain at most 256 documents. Invalid zero or oversized queue/batch bounds fail closed.
- The in-memory queue is not a durable store. Process loss is recovered by polling unpublished committed outbox rows.

### Retention and deletion

- In process, an event is retained only until its exact exporter acknowledgement or process termination. Unacknowledged truth remains in the owning durable outbox under that domain's retention policy.
- M03 remote export is disabled by default. No remote destination may be enabled until the owner records the processor, purpose, region, access roles, encryption, retention period, deletion workflow, backup expiry, and privacy-notice text.
- Collector retention must be the shortest period that supports the approved M03 measurement and incident purpose. Extending retention or adding optional diagnostics requires a reviewed amendment; telemetry availability can never be a gameplay dependency.
- Deletion requests operate on the upstream identity-to-pseudonym mapping and approved collector process. This crate deliberately cannot reverse a pseudonym or query by raw identity.

## Rejected alternatives

- **Emit directly from gameplay handlers:** rejected because rolled-back or replayed attempts would become analytics facts.
- **Accept arbitrary JSON or key/value properties:** rejected because field allowlisting and secret exclusion would be advisory rather than structural.
- **Drop oldest on overflow:** rejected because invisible sampling would corrupt ordered funnels and death/extraction counts.
- **Persist a second local telemetry database:** rejected because it creates another recovery and retention authority when the committed outbox already provides one.
- **Collect raw stack traces by default:** rejected because they can contain paths, arguments, secrets, and personal data.

## Validation and consequences

Production-blocking verification covers structural redaction, rejection of sensitive-looking labels and zero crash signatures, disabled operation, offline bounded capacity, duplicate polling, backpressure, exact post-delivery acknowledgement, exporter-response failure, and restart-style re-poll. Schema-0070 adds compile-checked disposable-PostgreSQL coverage for atomic onboarding projection, logical-session replay and canonical transitions, typed crash observations, immutable per-row context, restart recovery, and one-way publication. Schemas 0071/0072 add focused coverage for canonical item-ledger projection, exact reward/starter origin, replay/conflict, immutable payloads, no-session gameplay availability, restart re-poll, bounded reads, exact one-way acknowledgement, and the hosted integer-literal compatibility defect. Focused runtime tests additionally cover generation-safe handoff/reconnect, orphan replacement, clean exit, truthful transition failure, accepted-client platform attribution, cohort isolation, explicit region/environment configuration, and failure containment. Hosted schema-71/72 execution and full statistical/load/collector/retention audits remain intentionally open until their M03 evidence exists; disabled production-worker ownership is implemented without enabling export.

This boundary gives M03 a typed and recoverable telemetry seam without making analytics part of gameplay availability. Export remains disabled by default. Worker integration, durable terminal-family origin binding, an approved destination, destination-specific retention/deletion review, and hosted operational evidence remain necessary before `GB-M03-09` can close.
