# SPEC-CONFLICT-002 - Bell Proctor fan damage band

- **Status:** Resolved — retain `Chip`
- **Owner resolution:** 2026-07-11, after corrected full-reference calculation
- **Discovered:** 2026-07-11
- **Blocked scope:** Bell Proctor content promotion in `GB-M01-04B/04C` and final `GB-M01-05A` reference-band acceptance
- **Unblocked scope:** All nonboss damage resolution, projectile grace, presentation, lethal ordering, and subsequent independent M01 work

## Conflicting authorities

- Canonical GDD `COM-003` classifies final player damage above `8%` and through `18%` of target maximum health as **Pressure**. Chip ends at `8%`.
- Content specification `CONT-FP-005` authors `pattern.prototype.bell_proctor.aimed_fan` as raw `12`, Veil **Chip** damage.
- Content specification `CONT-FP-010` gives the starting Grave Arbalist Reedcloth Wraps (`+8` max health); the class baseline is `120`, producing the required FP reference maximum health of `128`. The loadout has no Veil resistance.
- Roadmap `GB-M01-05A` requires damage-band validation before promotion.

The original analysis incorrectly divided raw damage by maximum health and omitted the reference Arbalist's `2` starting armor. `COM-002` applies armor before the `COM-003` final-damage category: `armor_reduction=min(2,12×0.35)=2`, final damage is `10`, and `10/128=7.8125%`. The original authored **Chip** label is therefore consistent with the current canonical rules.

## Options

1. **Recommended:** retain raw damage `12` and the original `Chip` declaration. The exact reference resolver produces final damage `10` after armor.
2. Intentionally classify bands before armor or exempt Veil damage from armor. This changes `COM-002/003` and the 05A contract, so it is not recommended.

## Resolution

The owner selected the recommended corrected result: retain raw damage `12` and `Chip`. Strict reference validation must use the complete `COM-002` pipeline and exact 128-health/2-armor starting loadout. The independently approved in-place `fp.1.0.0` correction adds the missing Bell Proctor records; it does not change this payload.
