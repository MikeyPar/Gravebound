# SPEC-CONFLICT-012 — M03 production reward RNG and audit contract

**Status:** Approved by owner on 2026-07-12

**Raised:** 2026-07-12

**Blocks:** Production reward planning, durable item grants, retry fixtures, and reward audit evidence in `GB-M03-04D`

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1, and ADR-001

## Context

The canonical GDD fixes reward-table order, immutable UID allocation timing, retry behavior, and deterministic placement, but does not define the production entropy boundary. ADR-001 defines deterministic `ChaCha8Rng` streams for simulation but deliberately does not supply a server-secret reward seed. A public or client-predictable seed would make reward outcomes exploitable; an unstored random draw would make retries and support reconstruction unreliable.

## Approved contract

1. Production reward planning uses `ChaCha8Rng` with a 32-byte seed derived by BLAKE3 under the exact context string `gravebound.reward-plan.v1`.
2. Seed material is the active server epoch secret followed, in order, by reward request ID, recipient character ID, source instance ID, reward table ID, and exact content revision. Every field is encoded as its canonical bytes preceded by an unsigned little-endian 32-bit byte length. IDs are never concatenated ambiguously or formatted through locale-dependent text.
3. The request row stores the nonsecret epoch identifier and canonical request fields before planning. The epoch secret remains in secret management and is never stored in gameplay tables, logs, traces, telemetry, or client payloads.
4. One transaction persists the request, complete resolved reward plan/result, reserved item UIDs, placement decisions, and immutable ledger events. A retry by reward request ID returns the stored result and consumes no RNG. A reused request ID with different canonical material fails as an idempotency conflict.
5. Support/audit correlation uses a distinct BLAKE3 context string, `gravebound.reward-audit.v1`, over length-delimited epoch secret, canonical request material, and canonical persisted result. Only this audit digest and epoch identifier may be logged. The seed and raw epoch secret are never logged.
6. Test fixtures use an explicit fixed test epoch secret in the wipeable namespace. Production startup fails closed when no active epoch secret is available; it never substitutes a build ID, timestamp, process RNG, zero key, or client-controlled value.

## Rationale

This contract makes outcomes unpredictable before the authoritative request, deterministic inside one request, reconstructable with controlled secret access, and exact under retry. Domain separation prevents an audit value from being reused as a planning seed.

## Decision

The owner approved this complete contract on 2026-07-12 without amendment.
