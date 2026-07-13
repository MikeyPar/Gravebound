# ADR-030 — Durable item, reward, and personal-ground lifecycle

Status: Accepted

Implementation package: `GB-M03-04D` / `GB-M03-02C`

## Context

The canonical GDD requires immutable item identity and provenance, deterministic reward retries, eight pending slots, distinct consumable units, recipient-only 60-second ground fallback, and atomic destruction ledgers. The Content Production Specification supplies the exact Core item catalog, reward tables, starter kit, Red Tonic stack cap, and immutable development revision. The Development Roadmap assigns durable items, transactions, and restart evidence to M03 while keeping the normal route gated. Approved `SPEC-CONFLICT-007`, `012`, and `013` close the remaining item-location, entropy, UID, and placement ambiguities.

## Decision

1. One normalized `item_instances` row represents every equipment instance and every consumable unit. Stack membership is a projection over location, slot, template, and ascending unsigned UID; merging never rewrites identity or provenance.
2. Inventory, location, security, provenance, salvage band/value, and item version are independent typed axes with SQL shape constraints. Wipeable Core instances accept only the immutable `core-dev.blake3.<manifest>` revision.
3. Starter reconciliation runs after durable bootstrap and every identity mutation snapshot. It uses domain-separated deterministic UIDs and one replay-first receipt, grants the exact Pine Crossbow, Cracked Mark Lens, and two Grant-provenance Tonics in Belt index 0, and fails the response closed until every returned character is reconciled.
4. A reward transaction locks the character inventory before item rows, checks replay before invoking the planner, persists a planning-state request before consuming RNG, and finalizes request, normalized result, UIDs, placements, items, inventory version, and ledger in one serializable commit.
5. Fresh reward entropy uses the approved secret-epoch BLAKE3 preimage and ChaCha8 stream. Retry returns stored rows and performs no draw. Stored epoch metadata wins across epoch rotation; canonical request mismatch remains an idempotency conflict.
6. Reward Tonics merge matching nonfull `RunBackpack` stacks by ascending slot and then use empty slots. Rewards never auto-fill Belt. Equipment uses the lowest empty pending slot. Remaining units share a deterministic recipient-only personal-ground pickup for exactly 1,800 authoritative 30 Hz ticks.
7. An empty committed reward stores its result without advancing inventory version. A nonempty reward advances it once regardless of unit count.
8. Ground expiry probes bounded due work, locks affected inventories in stable owner order, rechecks each unit under row lock, transitions every still-due unit to `Destroyed(ground_expired)`, appends its deterministic ledger event, and advances each affected inventory once. Pickup/expiry races therefore have one transactional winner.
9. Secrets and seeds never enter stored rows, logs, traces, telemetry, client messages, or debug formatting. Only the nonsecret epoch ID and audit digest are observable.
10. The reward service and expiry worker remain unbound from the normal gameplay route until 04E/04F and the wider M03 death, extraction, Recall, and Oath gates close.

## Consequences

- Support can reconstruct every durable unit from its creation and transition ledger without treating a stack as identity.
- Retry, restart, epoch rotation, full-capacity placement, and expiry are deterministic and duplication-resistant.
- Character creation can recover safely from a crash between identity and starter commits through idempotent reconciliation.
- Live PostgreSQL execution remains a required closure artifact; compiled or pure tests do not substitute for it.
- Field equipment, pickup presentation/interaction, CharacterSafe/Vault, extraction, death, and crash restoration remain in their assigned later packages.
