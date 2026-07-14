# SPEC-CONFLICT-019 — Sepulcher Knight charge and gap resolution

**Status:** Owner-approved by standing authorization on 2026-07-13

**Blocks:** Sepulcher Knight charge/contact execution, stop-ring materialization, and the complete B3 combat trace in `GB-M03-03D`

## Authorities consulted

1. `Gravebound_Production_GDD_v1_Canonical.md`: `COM-001`–`006`, `DNG-005`, `WRLD-006`, `ENC-014`, and `ENC-020`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-001`–`002`, `CONT-ENEMY-003`, `CONT-PATTERN-002`, and the exact Sepulcher Knight pattern records.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` deterministic combat, replay, duration, and presentation gates.

## Conflict

The authorities fix the Knight's five-tile/550 ms charge, one-contact limit, parent-only ten-index stop ring, target-opposite adjacent two-shot gap, collision radii, and northwest-origin coordinates. They do not define the 550 ms tick ownership, the exact adjacent-pair tie break, or how a solid-truncated charge affects its endpoint and later ring. Leaving these choices to frame order, floating-point angle rounding, or renderer collision would make replay and reset evidence unstable.

## Approved resolution

1. A charge locks origin and target position when its 900 ms telegraph begins. Its fixed direction and nominal five-tile endpoint cannot retarget after that lock.
2. The 550 ms duration converts once to 17 authoritative ticks. Movement owns 17 equal deterministic segments on the release tick through release `+16`; the parent stop ring releases at release `+17`. The Knight cannot pursue or begin another movement during those segments.
3. Charge contact uses the swept Knight collision circle (`0.55` tiles) against each player hurtbox, ordered by contact time then entity ID. A cast may damage each player at most once and M03 capacity one therefore produces at most one contact hit. The authored charge is the Knight's only contact damage.
4. A solid collision truncates the remaining movement at the shared swept-collision contact point. The Knight holds that actual endpoint until the fixed parent end tick; that endpoint becomes its new home and the stop-ring origin. The ring cadence and payload do not accelerate or disappear because of truncation.
5. Stop-ring index `0` points east and indices advance clockwise in the northwest-origin simulation plane. Consider the ten adjacent pairs `(i, (i+1) mod 10)` and select the pair whose angular midpoint is nearest the direction from the locked target position through the actual charge endpoint (target-opposite). An exact angular tie selects the lower clockwise starting index. The selected pair is omitted and the remaining eight indices emit in ascending index order.
6. The stop-ring gap continues to use the parent charge's immutable target position even if that target moves, becomes immune, dies, or leaves after the telegraph. Reset cancels the charge, ring, fan, and every owned projectile; retry restores the authored spawn/home without reusing cast, actor, or projectile identities.

This decision supplies only missing deterministic ordering, collision, and gap selection. It does not change authored health scaling, armor, warning, cadence, length, speed, damage, projectile geometry, target selection, introduction, room composition, quiet time, reward, or XP.

## Approval record

The owner authorized all future recommended GB-M03 resolutions without an approval pause on 2026-07-13. This resolution was adopted under that standing authorization.
