# ADR-006: Slipstep, Exhaustion, and piercing-primary order

- **Status:** Accepted
- **Date:** 2026-07-10
- **Milestone:** `GB-M01-02D`
- **Owner:** Simulation/gameplay
- **GDD authority:** `SIM-003`, `SIM-004`, `SIM-005`, `SIM-010`, `CLS-002`, `CLS-020`, `CLS-040`
- **Content authority:** `CONT-010`, `CONT-011`, `CONT-FP-001`, `CONT-FP-002`

## Context

The approved design fixes Slipstep's distance, authored duration, reduction, empowerment, cooldown, and Exhaustion values, but several fixed-tick edge rules require one replay-safe interpretation. In particular, `180 ms` does not divide evenly into the 30 Hz simulation; movement direction and primary fire may arrive on the same input frame; collision can shorten travel; and one pierce requires stable multi-contact identity and a per-projectile ignore set.

## Decision

1. `sim_content` compiles exactly one `ability.arbalist.slipstep` definition referenced once by the First Playable Grave Arbalist. `CONT-010` nearest-tick conversion resolves authored `180 ms` to five fixed ticks. The simulation travels `0.4 tiles` on each unobstructed travel tick for exactly `2.0 tiles`.
2. Space and gamepad left bumper are replaceable presentation bindings. Simulation receives a checked monotonic ability-2 press sequence plus normalized movement action and aim. Blocked physical presses remain suppressed until release. The common 100 ms action buffer is three ticks; readiness four is consumed too early and readiness three/two/one buffers the newest sequence.
3. Per-tick order is: validate every sequence transactionally; advance tick; expire existing mark/status windows; advance existing projectiles in stable ID/contact order; decrement cooldowns and global cooldown; resolve Ability 1; resolve Ability 2; apply Slipstep or ordinary movement; emit primary fire. Ability 1 therefore wins a same-tick global-cooldown tie. A Slipstep accepted on the same tick as primary fire moves its first `0.4 tiles` before the shot origin is captured and empowers that shot.
4. Nonneutral movement input selects its normalized direction. Neutral input selects the exact inverse of current aim. Direction locks at accepted activation. Ordinary acceleration and walking are suppressed during the five travel ticks, and movement velocity is zero after every forced segment.
5. Each forced segment uses the authoritative swept-circle solid query with the `0.30 tile` player radius. Enemies do not body block. First shell, pillar, sealed-gate, or future void contact clamps the center to the exact contact point and terminates the whole cast; there is no slide or leftover-distance carry.
6. The 25% direct-damage reduction is emitted as a `2,500` basis-point step intent on every tick where forced travel occurs, including the terminal collision/completion tick. It grants no invulnerability and performs no health mutation before `GB-M01-05A`.
7. Cooldown, Exhaustion, and empowerment begin on accepted activation. Cooldown is 240 ticks. Exhaustion and empowerment are each active on activation tick `T` through `T+44` and expire before Ability 2 or primary resolution on `T+45`. Exhaustion rejects movement-ability activation rather than buffering it.
8. The first primary emitted while empowerment is active consumes empowerment, even if the projectile later misses, expires, or hits a solid. Pine Crossbow speed becomes exactly `12 × 1.30 = 15.6 tiles/s`. One bonus pierce permits two enemy contacts total.
9. A piercing projectile keeps a sorted unique set of enemy IDs already contacted. Each target may be damaged once by that projectile. Every remaining subsegment selects the earliest swept contact, using the existing solid-first then stable-ID tie break. It continues after a legal enemy contact while pierce remains, stops on the next enemy or any solid, and may produce multiple contacts in one fixed tick.
10. Collision and raw-intent identity is `(tick, projectile_id, contact_ordinal)`. Contact ordinals begin at zero and increase in traversal order. This replaces the earlier nonpiercing assumption that `(tick, projectile_id)` alone was unique.

## Rejected options

- **Six ticks for 180 ms:** contradicts the ordinary nearest-tick rule in `CONT-010` and would travel for 200 ms.
- **Teleport to the endpoint:** removes reduction travel ticks, mid-path collision semantics, and readable interpolation.
- **Move after firing:** makes same-frame use feel inconsistent and captures an origin behind the player.
- **Slide along solids or spend leftover distance:** silently changes the authored “stops at solid collision” rule.
- **Let enemies body block Slipstep:** violates shared-player dignity and changes an enemy hurtbox into environment collision.
- **Consume empowerment on hit:** permits indefinite retained power after misses and makes replay state depend on projectile lifetime.
- **Allow a pierced projectile to re-hit a target:** creates frame-rate/geometry-dependent damage and contradicts once-per-projectile contact semantics.
- **Use unordered target sets:** makes stable replay and server reconciliation dependent on container iteration.

## Consequences and migration cost

- Movement and combat now commit one avatar tick transactionally in the client integration, so an invalid sequence or collision result cannot partially move the player.
- Friendly collision events carry contact ordinals, continuation state, and Slipstep empowerment provenance. Future network snapshots must preserve these fields.
- `item.prototype.relic.slip_clasp` remains a later equipment-resolution override; it will replace cooldown/window values before constructing the immutable definition and migrate deterministic fixtures.
- `GB-M01-02E` may consume Slipstep-began transitions to break Focused without changing this order.
- Any change to tick conversion, same-frame order, solid endpoint, expiry boundary, consumption point, or pierce ignore order requires this ADR and deterministic fixtures to change together.

## Validation fixtures

- Exact content compilation: 240/5/3/5/45/45 ticks and 2,000/2,500/3,000/1 numeric values.
- Five unobstructed `0.4 tile` segments, neutral backward direction, shell/pillar shortening, and no enemy body blocking.
- Same-tick cast/primary origin, exact `15.6` speed, one-pierce two-target traversal, stable contact ordinals, and no repeat target.
- Exact 45-tick empowerment/Exhaustion expiry-before-input boundary, consumption-on-emission, rejection, buffering, sequence rollback, and repeated deterministic trace.
- Warning-free optimized LocalLab capture showing trail, empowered bolt, two contacts, HUD cooldown/Exhaustion/empowerment, and health unchanged.
