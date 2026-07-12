# ADR-029 — PostgreSQL persistence boundary

**Status:** Accepted on 2026-07-12 under the owner's approval of all three `SPEC-CONFLICT-005` resolutions

**Owner:** Backend/tools

**Roadmap decision:** `ADR-004`, `GB-M03-02`, and `GB-M03-11`

## Context

The canonical GDD requires one authoritative modular-monolith server backed by PostgreSQL beginning with the Complete Private Loop. `persistence` owns transactions, snapshots, ledgers, and migrations but cannot own combat rules. Durable commands must be idempotent, auditable, versioned, and atomic. The roadmap separately schedules the durable aggregate outcome and the infrastructure it requires, while several named aggregates are not defined until later M03 packages.

## Proposed decision

- Add one workspace `persistence` crate with PostgreSQL connection management, embedded forward migrations, transaction helpers, readiness diagnostics, and typed infrastructure errors.
- Pin SQLx `0.9.0` with only its Tokio, rustls-ring/web-PKI, PostgreSQL, migration, and macro features. Server request tasks never block on synchronous database I/O.
- Keep aggregate reducers and validation in their authoritative server/domain modules. The database adapter loads and locks approved records, invokes the domain transition, persists its result and idempotency record, and commits as one transaction.
- Acquire multi-aggregate locks in stable binary account/character/item order. Use database constraints as invariant backstops, not as the primary product-rule implementation.
- Store explicit relational columns and bounded serialized result payloads only where exact replay requires them. Do not add a generic mutable account JSON document or speculative later-domain schema.
- Use checked-in migrations and a schema-version table. Production migration rollback is an explicit reviewed procedure or compensating forward migration; application rollback must reject an incompatible schema rather than reinterpret it.
- Use actual PostgreSQL 17.10 through the official `postgres:17.10-alpine3.23` image or `TEST_DATABASE_URL`, with the same pinned service in CI. Never substitute SQLite or silently skip the suite.
- Retain an explicit wipeable test namespace through the pre-Early-Access rehearsals required by `TECH-030`.

SQLx 0.9.0 requires Rust 1.94 and is compatible with the workspace's pinned Rust 1.95. PostgreSQL 17 remains supported through November 2029; pinning its current 17.10 minor follows PostgreSQL's recommendation to run the current minor while retaining a mature major.

## Rejected options

- **SQLite for tests:** rejected by `TECH-002`; its locking and transaction behavior is not PostgreSQL evidence.
- **Synchronous PostgreSQL calls inside Tokio request tasks:** rejected because blocking database I/O would compromise the authoritative runtime.
- **Business rules in SQL procedures/triggers:** rejected by `TECH-004`; domain behavior must remain testable without a database connection.
- **One opaque JSON account aggregate:** rejected because item provenance, support lookup, locking, migrations, and ledgers require explicit durable identities.
- **Create every M03 table up front:** rejected because later state machines are not yet approved and persistence cannot invent them.
- **Distributed database/cache/message bus:** rejected by the modular-monolith and scope controls; one PostgreSQL database is sufficient.

## Migration cost

`GB-M03-01` state is explicitly wipeable and process-local, so the first adapter migrates no player data. Later schema changes require checked-in migrations, compatibility fixtures, and rollback rehearsal. The final wipe-to-live namespace remains owned by the later roadmap cutover and cannot be implied here.

## Validation fixture

A real-PostgreSQL journey creates an empty account, commits one Grave Arbalist, replays the mutation, rejects a conflicting replay and concurrent stale writer, selects the character, restarts the server, and observes the exact roster. Injected failure before commit leaves no account/character/result residue. A fresh namespace remains empty, and log scans prove credentials are absent.
