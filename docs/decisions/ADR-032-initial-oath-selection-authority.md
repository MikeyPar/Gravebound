# ADR-032 — Initial Oath selection authority

Status: Accepted

Implementation package: `GB-M03-05B`

## Context

The canonical GDD makes an Oath a level-10 character-life specialization, requires irreversible-action confirmation, permits changes only in Lantern Halls, and rejects Oath changes while character or inventory mutations are unresolved. `CONT-HUB-002` requires the exact two Grave Arbalist choices, a permanent-life warning, and a free first choice; later changes cost 40 Ash and require additional safeguards. The Development Roadmap assigns durable retry-safe Oath selection to the first complete private life and requires committed state to survive restart.

The first choice therefore crosses identity, location, content, inventory, UI, and event-delivery boundaries. Treating inventory safety as a preflight query or storing an independent Oath version would permit races with item movement or other character-life mutations.

## Decision

1. The server is the only selection authority. Clients send bounded reliable frames containing the selected character, exact Core Oath ID, immutable records/assets/localization revision, explicit confirmation, expected character-life version, issuance time, mutation identity, and canonical payload hash.
2. Eligibility is derived from locked authoritative state. The character must be owned, selected, living, exactly level 10, in the exact Lantern Halls location, version-aligned with that location, and free of unresolved security state.
3. Inventory safety is joined into the same serializable transaction. The repository locks the inventory root and live item rows in stable UID order; only equipped/Belt or durably destroyed units are safe. A missing aggregate or any RunBackpack/personal-ground/pending unit fails closed.
4. Oath selection uses the shared character-life version. Acceptance advances character and location versions together; no independent Oath version can drift from transfer, death, inventory, or later life mutations.
5. Mutation handling is replay-first after the account authority lock. The immutable receipt retains the canonical hash and exact result. Identical retries return that result even after later state changes; mutation-ID reuse with different canonical material is an idempotency conflict.
6. An accepted first choice atomically writes one durable `oath_selected` outbox event keyed by the mutation identity. Selection state, receipt, version advance, and event either commit together or not at all.
7. Only the initial free choice is active in this stage. A different choice after selection returns typed `stage_disabled`; no partial 40-Ash flow or provisional later-change contract is exposed before `GB-M03-12` and the assigned shrine package.
8. The native safe-Hall UI has a distinct review state containing exact localized Oath copy and the permanent-life warning. Choosing an Oath does not mutate state; only the separately labeled confirmation action emits the reliable mutation, and cancellation emits nothing.
9. Persistent Core runtime routes the messages to PostgreSQL authority. Process-local Core recognizes the message kinds but fails closed with a typed unavailable result, preventing an ephemeral selection from masquerading as durable state.

## Consequences

- Inventory movement cannot race the safety decision, and no client can forge readiness.
- Every accepted first choice is attributable, replayable, restart-safe, and available to downstream combat construction through one character-life version.
- Content drift fails before selection rather than silently applying different mechanics or warning copy.
- Accessibility does not depend on color: names, descriptions, warning, action labels, keyboard bindings, and rejection reasons are textual.
- The compiled disposable-PostgreSQL repository and real-QUIC restart fixtures remain mandatory closure evidence; compilation and in-memory tests do not substitute for authorized live execution.
- Oath mechanics, paid changes, Bargains, death/crash integration, Core promotion, and the normal player route remain gated by their assigned M03 packages.
