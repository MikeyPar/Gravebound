# SPEC-CONFLICT-015 — M03 shared-AI leash and introduction timing

**Status:** Owner-approved on 2026-07-13

**Raised:** 2026-07-13

**Blocks:** `GB-M03-03D` Core-authored enemy runtime state machines

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

Content Specification `CONT-ENEMY-001` fixes the shared enemy state contract, nearest-target acquisition, `12/16`-tile aggro/leash defaults, telegraph locks, and five-second no-target reset. It does not state whether the leash radius is measured from the actor to its current target or from the actor to its spawn position. Existing immutable First Playable behavior measures actor-to-target distance.

The same specification gives every enemy a `900 ms` spawn telegraph and each miniboss a `3 s` introduction, but does not state whether those nonattacking gates overlap or run serially. The canonical GDD requires the introduction to last three seconds and does not require a 3.9-second combined delay. The Development Roadmap requires responsive private-loop encounters without changing authored attack timings.

## Approved resolution

1. Measure the `16`-tile leash from the actor to its current target, matching the existing First Playable contract. Aggro and leash boundaries are inclusive. A target beyond leash is immediately ineligible; the ordinary five-second no-target reset remains a separate rule and returns the actor to its authored spawn.
2. Start the `900 ms` spawn warning and the `3 s` miniboss introduction together. A miniboss may enter Acquire only after both have completed, which is tick `90` for the exact Core minibosses. The introduction therefore remains exactly three seconds and is not extended to 3.9 seconds.

Neither decision changes health, attack cadence, warnings, damage, movement speed, reward/XP ownership, or the normal-route gate.

## Approval record

The owner approved both recommendations without amendment on 2026-07-13.
