# SPEC-CONFLICT-024 — Sir Caldus cardinal charge-axis tie resolution

**Status:** Owner-approved by standing authorization on 2026-07-13

**Blocks:** Deterministic Caldus Charge Lane movement and endpoint evidence in `GB-M03-03E`

## Authorities consulted

1. `Gravebound_Production_GDD_v1_Canonical.md`: `COM-001`–`006`, `ENC-005`, `ENC-010`, and `TECH-004`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-010`, `CONT-ROOM-001`–`002`, `CONT-PATTERN-001`, and `CONT-BOSS-001`–`002`.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, deterministic replay/duration gates, and approved `SPEC-CONFLICT-006`/`022` ownership.

## Conflict

The content record provides the four authored cardinal endpoints west/east/north/south, while the GDD describes locking a direction toward the target. The authorities do not select an axis when the locked target direction has both horizontal and vertical components or define an exact diagonal tie. Choosing from presentation angle or floating-point normalization would make the realized path and Stop Ring gap dependent on implementation detail.

## Approved resolution

1. At the `+700 ms` direction-lock boundary, compare the absolute fixed-point horizontal and vertical deltas from the locked body origin to the selected living participant position.
2. Select the signed horizontal cardinal axis when `abs(dx) >= abs(dy)` and the signed vertical cardinal axis otherwise. This is equivalent to selecting the authored cardinal endpoint with maximum directional dot product and resolving an exact diagonal tie by the checked-in endpoint order west, east, north, south.
3. Advance up to `6.5 tiles` on that axis, capped at the matching authored endpoint `(1,9)`, `(17,9)`, `(9,1)`, or `(9,17)`. Shared swept-solid collision may truncate that nominal segment further as already approved by `SPEC-CONFLICT-022`.
4. The selected axis and nominal endpoint remain immutable for the cast. Player movement after the lock, body presentation, collision truncation, and center return cannot rotate or lengthen it.

This resolution supplies only a missing deterministic cardinal-axis and tie rule. It does not change warning/travel duration, lane width, contact damage, ring geometry, target selection, collision radius, or phase cadence.

## Approval record

The owner authorized all future recommended GB-M03 resolutions without an approval pause on 2026-07-13. This resolution was adopted under that standing authorization.
