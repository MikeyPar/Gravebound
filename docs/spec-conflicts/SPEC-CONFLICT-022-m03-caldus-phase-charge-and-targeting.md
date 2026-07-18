# SPEC-CONFLICT-022 — Sir Caldus phase, charge, and targeting resolution

**Status:** Owner-approved by standing authorization on 2026-07-13

**Blocks:** Production Sir Caldus scheduler, hostile materialization, participant-scaled health, and deterministic full-combat evidence in `GB-M03-03E`

## Authorities consulted

1. `Gravebound_Production_GDD_v1_Canonical.md`: `COM-001`–`006`, `WRLD-006`, `DNG-006`, `ENC-005`, `ENC-010`, and `ENC-020`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-010`, `CONT-ROOM-001`–`002`, `CONT-PATTERN-001`, and `CONT-BOSS-001`–`002`.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, the deterministic replay/duration/presentation gates, and approved `SPEC-CONFLICT-006` ownership.

## Conflict

The authorities fix Caldus's health scaling, phase thresholds, breaks, timelines, target order, charge warning/travel, and ring gaps. They do not define integer threshold boundaries, overkill across multiple thresholds, charge segment ownership, solid truncation, the exact opposite-gap angular tie break, or rotating-target cursor behavior when a selected participant becomes ineligible after selection. Leaving those choices to floating-point percentages, frame order, or presentation state would make replay and reward eligibility unstable.

## Approved resolution

1. Scale maximum health once at participant lock using `round_half_up(7200 × (1 + 0.72 × (N_locked - 1)))`. Phase 2 begins when health after an ordered damage intent is at or below `floor(maximum_health × 0.70)`; Phase 3 begins at or below `floor(maximum_health × 0.35)`; low-health Phase 3 begins at or below `floor(maximum_health × 0.20)`. Integer comparison uses `health × 100 <= maximum_health × threshold_percent` in widened arithmetic, so no floating point participates.
2. A single legal hit may cross only the next phase boundary. Preserve its full damage, then cancel all scheduled actions, movement, lanes, and projectiles and enter the authored four-second break. If the resulting health is already below a later boundary, the next transition occurs on the first post-break authoritative tick before any combat action. Lethal damage defeats Caldus immediately and never starts a redundant break.
3. A Charge Lane locks origin, nearest living locked target, target position, and direction at warning `+700 ms`. Its fixed nominal endpoint is 6.5 tiles from that origin, limited to the authored cardinal arena endpoint reached by the locked direction. Movement owns 17 deterministic segments from `+1000 ms` through the tick before `+1550 ms`; each segment uses the shared swept-solid path. Solid or arena collision truncates the realized endpoint without changing the fixed end tick.
4. Charge contact uses Caldus's swept `0.70` collision circle against participant hurtboxes, ordered by contact time then immutable party slot then entity ID. Each locked participant can take the authored 48 raw damage at most once per charge. Caldus holds the realized endpoint until the fixed charge end, emits the Stop Ring there, then returns toward `(9,9)` at two tiles per second and stops within 0.25 tiles when not charging.
5. Stop-ring index `0` points east and 14 indices advance clockwise in the northwest-origin plane. Consider every adjacent pair `(i, (i+1) mod 14)` and omit the pair whose angular midpoint is nearest the direction opposite the locked charge direction. An exact angular tie selects the lower clockwise starting index. Emit remaining indices in ascending order.
6. Bell Ring uses index `0` east and advances its ordinary three-adjacent gap start `+5 mod 18` after every consumed cast. Phase 3 previews reserve the next three ordinary starts in order; their later child emissions consume those reservations and add no implicit 800 ms telegraph.
7. Each Shield start snapshots its required distinct living targets in rotating immutable-slot order. Later death, Recall, disconnect, or immunity does not retarget or collapse already scheduled fans. Ineligible slots are skipped only when the Shield start is created. Advance the cursor exactly one eligible slot after the final selected target; reset restores it to the lowest living locked slot.
8. Soft enrage and the below-20% loop reduction affect only future loop starts. Equal-tick ordering remains phase transition, movement, personal Shield, radial Ring. Reset cancels every owned action and restores authored arena state without reusing encounter, cast, actor, or projectile identities.
9. Player walking and forced Slipstep use the player `0.30` physical radius against Caldus's separate `0.70` body radius. The earliest swept contact wins; an exact solid/body tie uses the stable collision-target order, and an exact boundary contact blocks only inward movement so tangent or outward departure remains legal. Friendly projectile damage continues to target only the distinct `0.62` hurtbox.
10. After each Caldus charge segment, resolve any body/player overlap before the frame commits. Process living locked participants by immutable party slot then entity ID and move each by the shortest legal separation from the final body center to the combined `1.00` radius. A coincident-center tie uses the reverse charge axis. If that radial result is blocked by the authored shell or pillar geometry, test the remaining cardinal separation points in clockwise order beginning with the reverse charge axis and select the first legal point; absence of a legal point fails the staged frame without committing body, player, damage, projectile, or route state.

This decision supplies only missing deterministic arithmetic, ordering, collision, and targeting. It does not change authored health, armor, cadence, warnings, damage, projectile geometry, participant scaling, reward, extraction, or route ownership.

## Approval record

The owner authorized all future recommended GB-M03 resolutions without an approval pause on 2026-07-13. This resolution was adopted under that standing authorization.
