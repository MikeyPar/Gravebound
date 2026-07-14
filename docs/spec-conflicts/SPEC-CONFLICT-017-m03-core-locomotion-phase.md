# SPEC-CONFLICT-017 — Core authored locomotion phase

**Status:** Owner-approved on 2026-07-13

**Blocks:** Final B2 Bell Acolyte/Choir Skull movement traces in `GB-M03-03D`

## Authorities consulted

1. `Gravebound_Production_GDD_v1_Canonical.md`: `COM-001`–`006`, `DNG-005`, and the Core enemy-role/Choir Skull roster rows.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ENEMY-001`–`002`, Bell Acolyte `maintain6 at3.0`, Choir Skull `orbit anchor radius3 at2.8`, shallow-water exception, and B2's exact roster/rotation.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` deterministic full-combat and presentation gates.

## Conflict

The authorities fix speed and desired geometry but do not define two state needed for a bit-stable runtime:

- Bell Acolyte has no maintain-distance deadband or overshoot rule around exactly six tiles.
- Choir Skull has no initial angular phase or rule for reaching its three-tile orbit from the authored anchor, which is currently also its spawn coordinate.

Choosing these values in renderer or movement code would make replays and future content changes depend on an undocumented implementation accident.

## Approved resolution

1. Bell Acolyte uses no deadband. Each ordinary movement tick corrects radially toward exactly `6000` milli-tiles from its locked current target at `3000` milli-tiles/second, preserves fixed-point remainder, and clamps the final step so it never crosses the six-tile boundary. At exactly six tiles it holds position. Telegraph aim/position remains locked by `CONT-ENEMY-001`; movement resumes only when the authored state permits it.
2. Choir Skull treats its authored room anchor as orbit center and starts at phase `0°` (east). It first moves radially east at `2800` milli-tiles/second until reaching radius `3000`, clamping without overshoot, then advances clockwise at the same ordinary ground speed with deterministic radial correction. The orbit phase is simulation state, never derived from renderer time, target position, or unordered entity iteration.
3. Choir Skull retains the written shallow-water immunity. Bell Acolyte receives the ordinary strongest terrain multiplier after base movement resolution. Solids truncate movement through the shared swept-hurtbox collision path without changing the stored orbit phase; a blocked Skull continues attempting its authored clockwise route.
4. Reset restores both actors to their authored spawn coordinate, clears movement remainder/target/cast/hostile output, and restores the Skull phase to `0°`. No reset grants reward or reuses a spawn identity.

This decision supplies only missing deterministic locomotion state. It does not change authored speed, radius, preferred distance, attack cadence, warning, damage, target selection, or room composition.

## Approval record

The owner approved all four recommendations without amendment on 2026-07-13.
