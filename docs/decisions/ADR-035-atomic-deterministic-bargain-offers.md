# ADR-035 - Atomic deterministic Bargain offers

Status: Accepted

Implementation package: `GB-M03-05D`

## Context

The canonical GDD grants up to three life-persistent Veil Bargain slots and requires a qualifying shrine to present exactly three legal boon/curse choices, with refusal allowed and no premium rerolls. The Content Production Specification defines a byte-exact BLAKE3 offer algorithm, milestone idempotency, immutable candidate persistence, terminal select/refuse behavior, emergency short-offer handling, and an exact 10-Ash fallback. The Development Roadmap requires this state to survive retry, reconnect, transfer, and restart within the first complete private life.

The repository already anticipates one Oath/Bargain restore component, but no durable aggregate owns it yet. Progression currently owns the qualifying reward-event authority, while Ash has an independent replay-first wallet. Creating the offer or fallback in a later transaction would allow a committed milestone with no slot, offer, or fallback.

## Decision

1. One `character_oath_bargain_state` row is the character-life aggregate root for Oath/Bargain restore state. It owns a positive version and earned slots `0..=3`; active Bargains are normalized children ordered by immutable acquisition ordinal.
2. The Core trigger is the first qualifying Sepulcher Knight clear in the exact Core layout at level 5 or above. The trigger is content/stage gated and absent from Slice and later bundles.
3. Milestone resolution, slot grant, offer/fallback disposition, progression award, and zero-candidate or no-slot 10-Ash fallback share one serializable transaction and account/character lock order. Connection-bound domain helpers may participate; nested repository transactions may not.
4. Offer identity is bound to the raw 16-byte source reward event. Exact replay returns the stored disposition before current-state validation; conflicting canonical material fails idempotently.
5. Candidate planning is a pure function over immutable catalog revision, raw reward/character IDs, active Bargains, capability tags, challenge mode, and resolved-stat inputs. It implements the exact `bargain-offer-v1\0` preimage and unsigned score ordering from `CONT-014`.
6. Persisted offers retain immutable content revision, candidate IDs, 32-byte scores, and ordinals. One/two-candidate emergency offers retain only legal candidates and project disabled `UNAVAILABLE` cells. A zero-candidate disposition is terminal unavailable and grants the exact fallback once.
7. An offer may transition only `Open -> Selected(candidate)` or `Open -> Refused`. Escape is a local panel close, not refusal. Multiple milestone-keyed open offers are not silently auto-refused; selection revalidates the chosen candidate against current active state.
8. Selection uses a bounded mutation identity, canonical payload hash, expected life version, exact content revision, explicit confirmation, and authoritative ownership/living/location/security checks. The active child, terminal offer, aggregate version, receipt, and outbox event commit together before success returns.
9. Refusal is an explicit terminal mutation with no gameplay penalty. It does not consume the earned slot or fabricate an active Bargain. Identical retry returns the stored result.
10. The server supplies comparison values. The client renders exact copy and before/after health, damage, cooldown, movement, healing, and Bargain-specific attack-rate/belt axes; it does not independently resolve gameplay stats.
11. `GB-M03-05D` does not activate 05E mechanics, Hall purge, death/retirement cleanup, live normal-route milestones, or Core promotion.

## Consequences

- A qualifying reward cannot commit without exactly one durable slot/offer/fallback disposition.
- Offer order is reproducible across platforms and restarts without exposing or consuming random state.
- The existing danger restore snapshot gains a real Oath/Bargain provider instead of a placeholder version.
- Emergency content disable remains playable: legal cells stay selectable, unavailable cells are explicit, and refusal remains safe.
- Cross-domain progression/Ash participation requires connection-bound transaction helpers and strict lock-order tests.
- PostgreSQL concurrency/restart evidence and a real-QUIC shrine journey remain mandatory closure gates.
