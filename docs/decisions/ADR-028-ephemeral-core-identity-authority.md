# ADR-028 — ephemeral Core identity authority and protocol 1.6

## Status

Accepted and implemented, 2026-07-12, under the owner's approval of all seven `SPEC-CONFLICT-004` resolutions.

## Authorities

- The canonical GDD requires two initial active slots (`LOOT-050`), authoritative character select/creation (`UI-007`/`UI-008`), and mutation identity, account binding, optimistic version, payload hash, and issue time (`TECH-021`).
- The Content Production Specification permits only `class.grave_arbalist` in Core and prohibits treating the unpromoted `core-dev` target as `core.1.0.0` (`CONT-002`).
- The Development Roadmap scopes `GB-M03-01` to a wipeable test identity, creation/select, and one class before PostgreSQL in `GB-M03-02`.
- Approved `SPEC-CONFLICT-004` removes editable name and production appearance fields from the Core aggregate. Cards derive localized `Hero {roster_ordinal}` labels and use the base sprite as a non-entitlement preview.

## Decision

Protocol advances from exact minor 1.5 to 1.6. Existing message-kind bytes 1–8 and enum discriminants remain unchanged; account bootstrap and character mutation append bytes 9 and 10. A dedicated compatibility encoder pins the final canonical M02 frame bytes for immutable regression evidence while live peers continue to require exact minor 1.6.

`AccountBootstrapFrame` uses reliable ordered Control. `CharacterMutationFrame` uses reliable ordered Mutation and binds `mutation_id`, expected account version, canonical BLAKE3 payload hash, issue time, and a bounded create/select payload. Responses append typed account results to `ReliableEvent`, preserving the established reliable response path. Account and character state never use datagrams.

The server resolves an opaque `AccountId` from the authenticated test ticket before entering the domain. No client field establishes ownership. `IdentityService` owns validation; `AccountRepository::transact` supplies a single-writer aggregate seam. The first adapter is an in-memory map, so reconnecting to the same process restores state and restarting intentionally wipes it. A separate `BoundCoreIdentityServer` advertises build `m03-core-dev-identity-1`, content target `core-dev`, and feature `core_test_identity_character_select`; it constructs no combat scheduler and admits zero combat sessions.

Each aggregate has version 1 when bootstrapped, capacity 2, and a bounded 128-result mutation ledger. The ledger never evicts accepted identities: once full, new mutations fail `rate_limited`, preserving process-lifetime retry safety. Accepted creation increments the version and produces exactly level 1, no oath, living, safe-at-character-select state. Accepted selection increments the version without world transfer. Identical retries return the stored result; changed payload under one mutation ID fails `idempotency_conflict`; stale commands return the current safe snapshot.

Internal identity events contain only event kind, typed error, and roster ordinal. Raw credentials, account IDs, platform IDs, display labels, character labels, and auth tokens are absent.

## Rejected options

- Reusing M02 `SessionControl::Join`: rejected because it grants combat authority and violates the Core boot/select boundary.
- Sending account IDs, editable names, or appearance IDs: rejected because authentication owns account binding and `SPEC-CONFLICT-004` explicitly defers those player-facing rules.
- Promoting `core.1.0.0`: rejected because the complete M03 manifest and `CONT-VALID-003` gate do not exist yet.
- Adding PostgreSQL now: rejected because it belongs to `GB-M03-02`; persistence would make the wipe claim false.
- Evicting old idempotency records: rejected because an evicted create retry could create a second character.

## Migration cost

`GB-M03-02` implements the same repository transaction boundary with PostgreSQL, migrates only explicitly supported test aggregates, and replaces process-local IDs according to its reviewed namespace policy. Adding authored name or appearance policy requires an append-only protocol minor and aggregate migration. Formal Core promotion replaces the development target string only after the full content gate; it cannot relabel current bytes.

## Owner and validation fixtures

Owner: backend/networking.

Fixtures: pinned M02 byte hash, protocol 1.6 round trips and bounds, deterministic clock/ID unit tests, slot/version/idempotency/cross-account adversarial tests, privacy-safe `Debug`, real-QUIC bootstrap/create/reconnect, explicit zero combat admissions, and restart-wipe integration.
