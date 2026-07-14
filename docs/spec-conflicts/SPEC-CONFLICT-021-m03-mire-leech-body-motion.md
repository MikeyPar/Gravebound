# SPEC-CONFLICT-021 — Mire Leech charge and retreat body motion

**Status:** Owner-approved by standing authorization on 2026-07-13

**Blocks:** Complete Mire Leech full-combat fixture in `GB-M03-03D`

## Authorities consulted

1. `Gravebound_Production_GDD_v1_Canonical.md`: `COM-001`–`006`, the Core normal-enemy roster, and encounter acceptance criteria.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ENEMY-001`–`002`, the exact Mire Leech record, and Core reward/XP bindings.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` deterministic combat, replay, and presentation gates.

## Conflict

The authorities fix approach speed, trigger distance, charge distance/duration, one-contact disposition, retreat speed/duration, and cycle cadence. They do not define charge/retreat tick ownership, the retreat vector after a truncated or crossing charge, or swept contact/solid ordering. Frame-dependent movement would make the required full-combat trace unstable.

## Approved resolution

1. Approach at 3.0 tiles/s clamps without overshoot at the inclusive 2.5-tile trigger boundary. The actor holds during telegraph and attack.
2. The telegraph locks origin, target entity, and target position. The two-tile/500 ms charge owns 15 equal cumulative segments on the release tick through release `+14`; it never retargets.
3. Charge movement sweeps the Leech's 0.35-tile collision circle against solids and player hurtboxes. A cast may damage each player once, ordered by contact time then entity ID; a player does not block movement. A solid truncates the remaining charge without sliding.
4. Retreat begins at release `+15` and owns 45 movement ticks at 3.5 tiles/s. Its direction is locked away from the charge's immutable target position through the realized charge endpoint. If those points coincide, use the direction opposite the locked charge. Solids truncate each retreat segment without sliding; retreat has no contact damage.
5. Reset cancels approach, telegraph, charge, retreat, and owned contact groups, restores the authored spawn and first-use warning, and does not emit reward or reuse cast/projectile identities.

This decision supplies only missing deterministic motion and collision ownership. It does not change authored health, armor, warning, cadence, distance, speed, damage, threat, reward, XP, or disabled-route status.

## Approval record

The owner authorized all future recommended GB-M03 resolutions without an approval pause on 2026-07-13. This resolution was adopted under that standing authorization.
