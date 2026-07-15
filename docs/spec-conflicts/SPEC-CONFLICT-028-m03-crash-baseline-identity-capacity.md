# SPEC-CONFLICT-028 - M03 crash-baseline identity capacity correction

**Status:** Accepted on 2026-07-14 under the owner's standing instruction to implement the recommended resolution without further approval prompts.

## Authorities reviewed

1. Canonical GDD `LOOT-002`, `LOOT-004`, and `TECH-023`: the `RunBackpack` has eight pending stack slots, exact entry identities must be restorable, and post-entry consumable use rolls back without losing provenance.
2. Content Production Specification `CONT-FP-010`: the backpack capacity is eight stacks and the Tonic stack cap is six.
3. Development Roadmap `GB-M03-02`, `GB-M03-04`, `GB-M03-06`, and the M03 nonduplication/restart gates: durable item identity and crash recovery must agree for every legal Core inventory.
4. Accepted [`SPEC-CONFLICT-007`](SPEC-CONFLICT-007-m03-progression-items.md) and [`SPEC-CONFLICT-013`](SPEC-CONFLICT-013-m03-item-uid-and-consumable-placement.md): every consumable unit retains a distinct UID and provenance record; Belt and backpack stacks are projections over those units.

## Correction

Accepted [`SPEC-CONFLICT-027`](SPEC-CONFLICT-027-m03-crash-restore-completeness.md) counted eight `RunBackpack` slots as eight identities and therefore stated a 24-identity V3 baseline maximum. That bound is too small for a legal backpack containing projected consumable stacks. Applying it would reject danger entry for valid player property or omit identities required for exact recovery.

## Accepted resolution

1. The V3 entry baseline remains bounded by the existing slot contract: four Equipment slots, two Belt stacks of at most six units each, and eight `RunBackpack` stacks of at most six units each.
2. Because every consumable unit has a durable UID, the exact maximum baseline is `4 + (2 * 6) + (8 * 6) = 64` item identities. Equipment and other unstackable items still occupy one identity in their slot.
3. Snapshot ordinals and restored-item result counts therefore permit `0..63`/`0..64`. Location/slot checks, homogeneous projected-stack checks, unsigned UID ordering, and per-stack cap six remain mandatory; widening the identity count does not add slots or increase any gameplay stack cap.
4. This record supersedes only the 24-identity count in `SPEC-CONFLICT-027`. Every other V3 completeness, replay, compensation, provenance, and terminal-precedence rule in that record remains accepted unchanged.

## Scope

This is a persistence-capacity correction, not an inventory expansion. Player-visible Equipment, Belt, and `RunBackpack` capacities do not change.
