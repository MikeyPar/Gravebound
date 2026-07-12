# GB-M03-02B completion audit

## Result

PASS. Core account and character identity now use PostgreSQL by default, preserve the exact `GB-M03-01` domain/protocol contract across process restart, and retain an explicitly named ephemeral regression mode.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | Server-side identity, optimistic versioning, idempotency, safe-state ownership, asynchronous database access, restart durability, credential redaction, and build gates follow `UI-007`, `UI-008`, `TECH-004`, `TECH-006`, `TECH-020`, `TECH-021`, `TECH-030`, and `TECH-060`. |
| Content Production Specification | Durable rows contain only the stable Grave Arbalist identity subset and exact stored mutation results. No item, oath, appearance entitlement, or promoted Core record is introduced. |
| Development Roadmap | This closes durable account/character scope under incremental `GB-M03-02B` and the reusable `GB-M03-11` boundary; item/vault, memorial/death, and ledgers remain open with their owning packages. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Reusable repository contract | `AccountRepository` is asynchronous and has in-memory and PostgreSQL adapters. The private domain aggregate maps to explicit typed storage rows; SQL does not own product rules. | PASS |
| Atomic exact replay | One serializable account transaction locks, loads, reduces, persists, and stores exact bounded mutation results. Identical retry returns the original result; altered/stale operations remain nonmutating. | PASS |
| Restart durability | Service-level and real-QUIC fixtures create/select, stop the service, reconnect through a newly bound endpoint, and recover the exact roster and selection. | PASS |
| Isolation and concurrency | A separate authenticated account remains empty. Concurrent expected-version-1 creates commit exactly one character; the loser returns a safe stale/service result and the aggregate remains version 2. | PASS |
| Corrupt-state refusal | An injected invalid serialized mutation result causes bootstrap to return `service_unavailable` without exposing or rewriting the aggregate. | PASS |
| Honest runtime modes | Default `serve-core-identity` requires `GRAVEBOUND_DATABASE_URL`, migrates and checks readiness, reports `persistence_enabled=true`, and admits zero combat sessions. `serve-core-identity-ephemeral` alone retains restart-wipe behavior. | PASS |
| Runnable local boundary | LocalStack uses an isolated disposable Docker project or explicit environment URL. Missing PostgreSQL fails with an actionable nonzero result; credentials never enter command arguments or logs. | PASS |

## Verification

- [CI run 29206825598](https://github.com/MikeyPar/Gravebound/actions/runs/29206825598): PostgreSQL foundation and both durable server integration tests pass; format/lint/workspace and Windows release jobs pass.
- Local `format`, warnings-denied workspace `lint`, all non-ignored workspace `test`, strict `validate`, two identical `headless` traces, and optimized client/server `release` pass.
- Focused server Clippy and PostgreSQL integration-target compilation pass with warnings denied.
- Existing M02 and `GB-M03-01` real-QUIC, protocol-byte, restart-wipe, client, content, simulation, bot, abuse, and impairment regressions remain green.

## Granular delivery commits

- `1bbe013` — PostgreSQL foundation, migration, Docker/test route, and mandatory CI job.
- `ccabafe` — typed transactional identity store.
- `e9fd896` — durable identity repository adapter and service integration.
- `c75c4e8` — durable default Core server, explicit ephemeral mode, LocalStack, and real-QUIC restart journey.
- `a1443ae`, `7c90289`, `d187215`, `e805ca2` — Linux dependency, hosted integration-budget, and credential-log corrections discovered by real CI.
- `60302ae`, `9f85856` — explicit concurrent-writer and corrupt-row PostgreSQL evidence with isolated deterministic fixtures.

## Deferred parent scope

The parent `GB-M03-02` remains open for item/vault persistence with `GB-M03-04`, death/memorial persistence with `GB-M03-06`, and domain-owned ledgers beginning with `GB-M03-12`. No unresolved `GB-M03-02A` or `02B` conflict remains.
