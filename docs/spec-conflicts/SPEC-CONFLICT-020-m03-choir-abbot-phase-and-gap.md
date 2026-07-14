# SPEC-CONFLICT-020 — Choir Abbot rotor phase and recovery gap

**Status:** Owner-approved by standing authorization on 2026-07-13

**Blocks:** Choir Abbot hostile-projectile materialization and the complete Core miniboss fixture in `GB-M03-03D`

## Authorities consulted

1. `Gravebound_Production_GDD_v1_Canonical.md`: `COM-001`–`006`, `DNG-005`, `WRLD-006`, `ENC-014`, and `ENC-020`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-002`, `CONT-ENEMY-003`, `CONT-PATTERN-002`, and the exact Choir Abbot pattern records.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` deterministic combat, replay, duration, and presentation gates.

## Conflict

The authorities fix the Abbot's two opposite rotor arms, 35°/s angular speed, independently rounded 350 ms volley cadence, 3.5-second active duration, 2.5-second recovery, and target-facing adjacent four-shot gap. They do not define the first rotor phase, whether phase carries across recovery, the exact four-index angular tie break, or when the recovery target becomes immutable. Renderer time, accumulated floating rotation, or current-target sampling on the ring tick would make replay and warning evidence unstable.

## Approved resolution

1. Every rotor cast previews and begins with phase `0°`: the first arm points east and the second west. The ten volley phases are `0°, 12.25°, 24.5°, 36.75°, 49°, 61.25°, 73.5°, 85.75°, 98°, 110.25°`, derived from the authored 35°/s × 350 ms cadence independently of rounded release ticks.
2. Rotor phase is local to its cast and resets to `0°` after every recovery and room reset. It never derives from presentation facing, renderer time, the previous cast, or target position.
3. The 2.5-second recovery warning is origin-only and does not lock a target. At the start of the final 20-tick directional preview, lock the nearest living room participant by squared distance then entity ID. That immutable position owns the ring gap even if the participant moves, becomes immune, dies, or leaves before release.
4. Recovery-ring index `0` points east and 16 indices advance clockwise in the northwest-origin simulation plane. Consider each four-index consecutive group `(i, i+1, i+2, i+3)` modulo 16 and select the group whose angular midpoint is nearest the locked target-facing direction. An exact angular tie selects the lower clockwise starting index. Emit the remaining 12 indices in ascending index order.
5. At the shared six-second boundary, materialize the parent recovery ring before starting the next rotor, as already fixed by `SPEC-CONFLICT-016`. Reset cancels the rotor, recovery previews, ring, and every owned projectile without reusing cast or projectile identities.

This decision supplies only missing deterministic phase, lock timing, and gap selection. It does not change authored health scaling, armor, introduction, warning durations, cadence, damage, projectile geometry, recovery, quiet time, reward, XP, or disabled branch status.

## Approval record

The owner authorized all future recommended GB-M03 resolutions without an approval pause on 2026-07-13. This resolution was adopted under that standing authorization.
