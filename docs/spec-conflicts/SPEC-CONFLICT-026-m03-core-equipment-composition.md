# SPEC-CONFLICT-026 - M03 Core equipment composition

**Status:** Accepted on 2026-07-14 under the owner's standing instruction to implement the recommended resolution without further approval prompts.

## Authorities reviewed

1. Canonical GDD `COM-002`, `LOOT-001`, `LOOT-004`, and the resolved-stat caps after class, oath, equipment, Bargain, and status calculations.
2. Content Production Specification `CONT-ITEM-001` through `004`, `CONT-AFFIX-005`, and the exact Core catalog rows.
3. Development Roadmap `GB-M03-04` and the M03 deterministic/restart exit gates.

## Gap

The authorities define every Core item value and the broad resolution order, but do not explicitly state (a) whether Armor's flat maximum health enters before or after oath/Bargain health multipliers, (b) how multiple named negative-status reductions combine, or (c) how Charm potion output combines with Lantern Ash and a separate healing-received tradeoff.

## Accepted resolution

1. Resolve class base, level growth, and the equipment template/rarity exactly in `CONT-AFFIX-005` order. Add Armor's flat maximum health to the level-adjusted class health before oath/Bargain multiplicative health changes and the global `0.70` floor.
2. Preserve absolute current health across the rebuild. An increase never heals; a decrease clamps to the new maximum with a living floor of one.
3. Same-family percentage-point modifiers add in basis points. Rootweave and Salt Knot therefore combine their matching named-status reductions additively for a matching status; exclusions remain absolute and cannot be bypassed.
4. Potion-output bonuses in the same family add in basis points before application. Bell Locket and Lantern Ash therefore produce `1.50x` potion output. Healing-received modifiers are a separate family and multiply that resolved output at application; Saltglass's `-8%` produces `1.50 * 0.92` before one field-boundary round-half-up.
5. Relic replacements apply to the class baseline before oath changes. Long Vigil therefore extends the relic-resolved Mark range and adds its primary-bonus change; omitted relic fields retain the class baseline. Relic resonance scales only authored ability/passive damage as specified and never duration, movement, reduction, healing, or fixed signature damage.
6. Core generated equipment remains Forged with no affixes. The combat factory must accept exact legal Forged Core items while continuing to reject illegal slot/class/content/level/rarity combinations.

## Scope

This resolution closes only deterministic composition for the unpromoted Core catalog. It does not enable affixes, later rarities, other classes, normal-route admission, or Core promotion.
