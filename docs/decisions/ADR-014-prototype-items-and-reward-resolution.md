# ADR-014: Typed prototype items and deterministic reward resolution

- **Status:** Accepted
- **Date:** 2026-07-11
- **Milestone:** `GB-M01-07B`
- **GDD authority:** `LOOT-001`, constrained `LOOT-002`, `PRD-123`
- **Content authority:** `CONT-FP-006` through `CONT-FP-010`
- **Roadmap authority:** `GB-M01-07B`

## Context

The 12 item and five reward records were present and schema-valid, but record presence did not prove that every exact effect was executable. In particular, Scatterbow required multi-projectile primary support and Wave 3's global selection had to exclude the Charm chosen by its preceding group.

## Decision

1. `sim_content::prototype` compiles every item to a typed behavior and rejects any ID, slot, rarity, effect count, stat, operation, or value drift.
2. Primary weapon definitions carry one deterministic local-space direction per bolt and a per-release target cap. Scatterbow uses fixed millionth vectors for `-8/0/+8` degrees and caps one target at two bolts.
3. Projectile allocation and multi-bolt emission remain inside the existing clone-before-commit combat tick. Allocation failure exposes no partial release.
4. Per-release hit accounting is keyed by source, release tick, and target. A capped target is ignored by remaining sibling bolts without changing their lifetime or collisions with other targets.
5. Reward resolution uses a named deterministic stream derived from content version, root seed, table ID, and resolution ID. Guaranteed groups consume no presence draw.
6. Selected equipment is tracked across groups. A `without_replacement` group excludes every equipment item already selected by earlier groups as well as its own prior selections. This gives Wave 3 its required cross-group nonduplication and the boss three distinct equipment grants.
7. Repeated identical consumable selections consolidate into one grant quantity, so the boss yields exactly two Tonics.
8. Runtime evidence uses one explicit debug loadout to demonstrate four real behaviors without changing ordinary starter inventory: Scatterbow, Still Eye, Parish Leather, and Undertaker Knot.

## Consequences

- Future projectile patterns must provide deterministic local vectors and a reviewed per-target cap.
- Adding an item or reward table requires extending the explicit compiled contract; unknown prototype IDs fail closed.
- Reward replay remains isolated from unrelated RNG consumers.
- Ordinary play still begins with the documented Pine/Dented/Reedcloth loadout; the four-item matrix is screenshot-evidence-only.
