# GB-M03-01 completion audit

## Result

PASS. The wipeable Core identity, authoritative character aggregate, and native Grave Arbalist creation/select package close together. This is not the M03 milestone exit gate.

## Design authority and approved decisions

- Canonical GDD: Core Prototype identity/UI requirements, two initial slots, authoritative safe-state mutation rules, and no client-fabricated account or character state.
- Content Production Specification: immutable `fp.1.0.0`, stable IDs, strict validation, localization discipline, and no incomplete Core promotion.
- Development Roadmap: `GB-M03-01` precedes PostgreSQL `GB-M03-02`, with reusable persistence infrastructure in `GB-M03-11`.
- `SPEC-CONFLICT-004`: all seven recommendations were approved on 2026-07-12 and implemented without amendment.

## Slice closure

| Slice | Result | Evidence |
|---|---|---|
| `GB-M03-01A` | PASS | Strict unpromoted `core-dev` compiler resolves exactly the existing Grave Arbalist identity subset; see [`GB-M03-01A-audit`](GB-M03-01A-audit.md). |
| `GB-M03-01B` | PASS | Protocol 1.6 and process-local authority provide bounded, versioned, retry-safe, account-bound create/select and restart wipe; see [`GB-M03-01B-audit`](GB-M03-01B-audit.md). |
| `GB-M03-01C` | PASS | Native release UI completes and presents the authoritative journey at both target resolutions; see [`GB-M03-01C-audit`](GB-M03-01C-audit.md). |

## Combined gates

- Full `tools/dev.cmd ci` passed: workspace formatting and warnings-denied Clippy, every workspace unit/integration/doc test, strict content validation, and two identical deterministic trace runs. Relevant package totals include client 68, protocol 19, server 43, content schema 6, network harness 7, bot client 5, and simulation core 231.
- `tools/dev.cmd network-ci` passed, including M02 compatibility, real QUIC, four-client authority, abuse, bot, outage, impairment, and Core identity restart-wipe coverage.
- Optimized client/server build and two real release journeys passed with the hashes recorded in the 01C audit.
- `git diff c321599..HEAD -- content/fp` is empty. The existing M02 canonical compatibility frame remains pinned to SHA-256 `643b0c2d1746c2e697e2c5cb3b4fc0e352019903a951004326e808e00b5cd7ec`.
- The Core endpoint admits zero combat sessions and intentionally loses all rosters on restart.

## Granular delivery commits

- `0df0df4` — unpromoted Core identity content boundary.
- `a289bab` — wipeable server identity authority and append-only protocol.
- `de5b328` — validated Core identity UI copy.
- `564011c` — native Core creation/select presentation and runtime tooling.
- `27b1115`, `09b73d8` — maintainability refactor and typed-path correction identified by the full workspace gate.

## Deferred boundaries and current next step

No unresolved `GB-M03-01` specification conflict remains. Editable names, production appearance entitlement, preview clips, item power, Play/control transfer, and formal `core.1.0.0` promotion remain deferred exactly as approved.

The Current Next Step is `GB-M03-02`: accept the PostgreSQL ADR, introduce the `GB-M03-11` persistence/migration/ephemeral-test-stack foundation, and replace the in-memory identity repository adapter without importing later item, vault, memorial, or ledger domain scope prematurely.
