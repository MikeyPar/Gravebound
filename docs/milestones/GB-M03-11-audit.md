# GB-M03-11 completion audit

## Result

PASS. The reusable M03 PostgreSQL foundation, migrations, transaction boundary, readiness checks, and disposable real-database gate are complete. This does not close the parent `GB-M03-02` aggregate package or the M03 milestone.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | One PostgreSQL database backs the modular monolith; `persistence` owns connections, migrations, transactions, snapshots, and storage rather than combat rules; durable configuration is environment-only and pre-Early-Access data remains explicitly wipeable. |
| Content Production Specification | Stable identity/content references remain typed; exact replay uses bounded stored results; no prototype item or incomplete Core promotion enters durable state. |
| Development Roadmap | `GB-M03-11` supplies the crate, migrations, transactional repository primitive, and ephemeral PostgreSQL stack needed by incremental `GB-M03-02`, under all three approved `SPEC-CONFLICT-005` resolutions. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Reusable crate boundary | `crates/persistence` exposes redacted configuration, asynchronous pool ownership, embedded migration/readiness checks, typed errors, and an owned serializable transaction wrapper. | PASS |
| Pinned supported stack | SQLx is pinned to `0.9.0`; PostgreSQL is pinned to official `postgres:17.10-alpine3.23`; accepted `ADR-029` records compatibility and rejected alternatives. | PASS |
| Migration integrity | Readiness reruns the embedded migrator, validates history/checksums, requires schema version 1, and requires the exact wipeable `test.core` namespace. | PASS |
| Safe destructive testing | Tests require `GRAVEBOUND_ALLOW_DESTRUCTIVE_DATABASE_TESTS=1` and database `gravebound_test` or `gravebound_test_*`; cleanup failures are fatal. Local Docker projects and host ports are isolated per invocation. Hosted PostgreSQL uses credential-free trust only on the disposable runner-local loopback binding, so no test password can enter service bootstrap logs. | PASS |
| Real PostgreSQL evidence | Hosted CI runs all three ignored-by-default foundation tests against PostgreSQL 17.10: migration replay/readiness, schema ownership/bounds/rollback, and typed store round-trip. | PASS |
| Fail-closed local route | `tools/dev.cmd persistence-ci` exits nonzero without Docker or `TEST_DATABASE_URL`; LocalStack does the same without Docker or `GRAVEBOUND_DATABASE_URL`. Both messages explicitly prohibit SQLite. | PASS |

## Verification

- Latest hosted gate: [CI run 29206825598](https://github.com/MikeyPar/Gravebound/actions/runs/29206825598) — format/lint/workspace tests, Windows release build, and PostgreSQL jobs pass.
- Local named gates on the same code: `format`, `lint`, `test`, `validate`, two `headless` runs, and `release` pass.
- Persistence unit tests: 2 passed. Hosted PostgreSQL foundation tests: 3 passed with one test thread.
- Both deterministic runs produced the same hashes at ticks 1, 30, 60, 90, and 120.
- `git diff --check` and LF migration checkout policy pass.

## Outcome

`GB-M03-11` is complete. Later item/vault, death/memorial, Echo, and currency-ledger migrations must reuse this boundary and arrive only with their reviewed authoritative packages.
