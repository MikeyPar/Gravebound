# SPEC-CONFLICT-005 — M03 persistence dependency and verification order

**Status:** Open — owner decision required

**Raised:** 2026-07-12

**Blocks:** PostgreSQL implementation in `GB-M03-02` and `GB-M03-11`

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The three authorities agree that the Complete Private Loop uses PostgreSQL, the `persistence` crate owns transactions/migrations/ledgers rather than combat logic, durable mutations are idempotent and auditable, pre-Early-Access state remains wipeable, and SQLite cannot substitute for PostgreSQL transaction tests.

Their delivery order is not executable as written:

- Roadmap `GB-M03-02` names PostgreSQL persistence for accounts, characters, items, vaults, memorials, and ledgers.
- Roadmap `GB-M03-11` separately adds the `persistence` crate, migrations, transactional repositories, and ephemeral PostgreSQL stack needed by `GB-M03-02`.
- The roadmap dependency model says backend persistence begins after the character/item state machine is approved.
- Character identity is approved in `GB-M03-01`, but item/vault behavior belongs to `GB-M03-04`, memorial behavior to `GB-M03-06`, and the first currency ledger to `GB-M03-12`.
- GDD `TECH-004` prohibits the persistence module from owning combat rules, so placeholder SQL cannot invent those missing aggregates.

The current environment also has neither Docker nor a local PostgreSQL client/server. That is a verification constraint, not permission to weaken the database contract.

## Decisions requested

### 1. Package split and dependency order

**Conflict:** Implementing all of `GB-M03-02` before `GB-M03-04`/`06`/`12` would require inventing unapproved item, vault, memorial, and ledger state machines. Waiting for those packages would leave the roadmap-required `persistence` foundation unavailable to them.

**Recommended resolution:** Treat `GB-M03-02` as a parent package that closes incrementally:

- `GB-M03-02A` / infrastructure portion of `GB-M03-11`: accept the PostgreSQL ADR; add the `persistence` crate, migrations, transaction primitives, wipeable namespace, health/readiness checks, and real-PostgreSQL test harness.
- `GB-M03-02B`: adapt only the already-approved `GB-M03-01` account/character aggregate to PostgreSQL and prove restart durability, concurrency, versioning, and idempotency.
- `GB-M03-02C`: add item/vault persistence with `GB-M03-04`, after that state machine is approved.
- `GB-M03-02D`: add death/memorial persistence with `GB-M03-06`, after its atomic domain transition is approved.
- Ledger-specific schemas arrive with their owning package, beginning with `GB-M03-12` Ash earn/spend.

The parent `GB-M03-02` remains open until every named durable aggregate passes; `GB-M03-11` closes when the reusable infrastructure and ephemeral stack pass. This changes no product behavior or milestone scope; it makes the roadmap dependency graph executable.

### 2. Migration contents before domain approval

**Conflict:** Empty or speculative item/memorial/ledger tables would appear to satisfy the roadmap while encoding rules the GDD assigns to later authoritative state machines.

**Recommended resolution:** Initial migrations contain only migration bookkeeping, the explicit wipeable namespace, account/character identity state, and bounded mutation results required by the approved aggregate. Every later migration is forward-only, narrowly owned by its domain package, and paired with a tested rollback procedure. No generic JSON catch-all, premature item ledger, or promoted live namespace is allowed.

### 3. PostgreSQL integration-test policy

**Conflict:** GDD `TECH-002` requires PostgreSQL behavior and forbids SQLite substitution. The local workstation currently exposes neither Docker nor PostgreSQL, while the roadmap requires an ephemeral test stack and clean-machine commands.

**Recommended resolution:** Support both an explicit disposable `TEST_DATABASE_URL` and a documented Docker Compose PostgreSQL service. CI runs every persistence integration test against a pinned PostgreSQL service container. Local commands fail with an actionable prerequisite message when neither route is available; they never silently skip, fall back to SQLite, or count an unexecuted integration test as passing. Database credentials remain environment-only and must never enter logs or committed files.

## Approval requested

Approve all three recommended resolutions, or provide an amended ordering/test policy. Implementation remains blocked until the package dependency is explicit. A local Docker installation is recommended for fast iteration, but a disposable PostgreSQL URL is an equivalent authorized verification route.
