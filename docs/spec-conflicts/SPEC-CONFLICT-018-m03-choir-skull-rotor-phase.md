# SPEC-CONFLICT-018 — Choir Skull rotor phase

**Status:** Owner-approved by standing authorization on 2026-07-13

**Blocks:** Choir Skull hostile-projectile materialization and the complete B2 combat trace in `GB-M03-03D`

## Authorities consulted

1. `Gravebound_Production_GDD_v1_Canonical.md`: `COM-001`–`006`, `DNG-005`, and the Core enemy-role/Choir Skull roster rows.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ENEMY-001`–`002`, Choir Skull's two opposite arms, clockwise `35°/s` rotation, `400 ms` emission interval, four-second active duration, and exact B2 roster.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` deterministic full-combat, replay, and presentation gates.

## Conflict

The authorities fix rotor angular velocity, arm count, cadence, duration, projectile payload, and clockwise direction, but do not define the initial angular phase or whether that phase carries between casts. Deriving either from renderer time, locomotion phase, or the current target would make warnings and projectile traces unstable.

## Approved resolution

1. Every Choir Skull rotor cast previews and begins at phase `0°`: one arm points east and the opposite arm points west.
2. The phase advances clockwise by the exact authored `35°/s × 400 ms = 14°` between consecutive volleys. Volleys therefore use first-arm phases `0°,14°,28°,42°,56°,70°,84°,98°,112°,126°`; the second arm is always exactly opposite.
3. Rotor attack phase is local to the immutable cast lock and resets to `0°` for each new cast. It is independent of locomotion orbit phase, target position, renderer time, and unordered entity iteration.
4. Reset cancels the active rotor and its hostile projectiles. A later cast restarts at `0°` without reusing cast or projectile identities.

This decision supplies only missing deterministic attack phase. It does not change authored warning, angular speed, cadence, duration, damage, geometry, movement, target selection, or room composition.

## Approval record

The owner authorized all future recommended GB-M03 resolutions without an approval pause on 2026-07-13. This resolution was adopted under that standing authorization.
