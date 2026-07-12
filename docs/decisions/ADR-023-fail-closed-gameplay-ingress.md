# ADR-023 — Fail-closed gameplay ingress

Status: Accepted

Implementation package: `GB-M02-06`

## Context

Server authority prevents a client from directly setting position or health, but valid-looking sequence and mutation traffic can still exploit ambiguity: an old primary sequence can fail a later simulation tick, repeated reliable presses can overwrite each other before a tick, and one mutation ID can currently be replayed with a different payload while receiving the first cached result.

## Decision

1. Preserve the intent-only wire model. Teleport and forged-hit defense is structural first, then proven with codec adversarial tests.
2. Input datagrams remain latest-state/coalesced. Old input sequence is a benign superseded disposition and count. A regressing held-primary press sequence is a suspicious typed rejection because it can attack combat sequencing; equal remains legal held state.
3. Reliable Action replay returns an ordered typed `StaleSequence` result rather than tearing down transport. Only one pending press per ability may be accepted before the next authoritative tick consumes it.
4. Input datagrams cannot carry ability presses. Their legacy ability sequence fields are required to be zero until a future protocol removal; reliable Action is the only accepted seam.
5. Mutation cache entries store the original request and result. Exact retry is idempotent. Same ID/different payload is `IdempotencyConflict`, increments diagnostics, and performs no mutation.
6. New mutation attempts are bounded to eight per tick and 1024 cached identities per session. Limits are infrastructure safety bounds, not gameplay balance values.
7. Ingress diagnostics are bounded, pseudonymous-session-local evidence. Ordinary reorder/loss counts are separate from anomaly records; score alone never bans or changes simulation.
8. Protocol advances to exact-match minor `1.4` for new typed mutation results and stricter Input semantics. No compatibility adapter is claimed.

## Consequences

- Bad-network replay remains playable and observable without being mislabeled as cheating.
- Semantically malicious traffic fails without simulation stalls or ambiguous cached responses.
- M02-07 bots and M02-08 instances can consume the same bounded ingress owner.
- Durable security review, sanctions, and cross-session evidence remain M06 responsibilities.
