# ADR-005: Grave Mark timing, collision, and raw-damage intents

- **Status:** Accepted
- **Date:** 2026-07-10
- **Milestone:** GB-M01-02C
- **Owner:** Simulation/gameplay
- **GDD authority:** SIM-003, SIM-004, SIM-010, CLS-002, CLS-020
- **Content authority:** CONT-010, CONT-011, CONT-013, CONT-FP-001

## Context

The design fixes Grave Mark's binding, cooldown, speed, range, coefficient, duration, bonus, and one-target limit. The content record omits projectile radius and disposition because shared defaults and later oath behavior own them: `CONT-013` supplies radius `0.12` unless overridden, while Nailkeeper requires the base bolt to identify the first enemy or wall impact. The roadmap defers health/damage order to `GB-M01-05A`, so this ticket also needs a stable way to prove `1.8W` and marked-primary `+15%` without prematurely mutating health.

## Decision

1. `sim_content` resolves one immutable Grave Mark definition from `ability.arbalist.grave_mark`, the owning class reference, exact FP package, shared duration rules, and `CONT-013` projectile radius `0.12`. Resolved disposition is consume on first enemy or solid.
2. Right mouse is presentation input only. Simulation receives normalized aim plus a checked monotonic ability-1 press sequence. Each new sequence is consumed once even when the render sampler retains it across fixed ticks.
3. Ability presses buffer only when all readiness timers can reach zero within three future ticks. Readiness on the current tick fires immediately. Earlier presses are consumed and discarded; a newer legal press replaces an older pending sequence.
4. Fixed-tick order is: validate sequences transactionally; increment tick; expire/decrement the existing mark; advance projectiles in ID order and apply collision-derived mark/intents; decrement primary/ability/global timers; resolve a pending/new Grave Mark fire; resolve primary fire. A newly fired projectile never moves on its spawn tick.
5. Grave Mark cooldown and the five-tick global cooldown begin on fire. The 150-tick cooldown means the next unmodified fire is legal exactly 150 tick indices later. The three-tick input buffer may hold the newest press until that tick.
6. Grave Mark uses speed 12, range 11, radius 0.12, no pierce, and exact range clamp. Enemy contact emits `1.8W` raw intent and applies the mark. Solid contact consumes with block feedback only. Range expiry does neither.
7. A mark applied on tick `T` is observable during collision processing on `T` through `T+119` and expires before collision processing on `T+120`. Same-target application refreshes; different-target application atomically replaces.
8. Direct raw intents use integer basis-point half-up math and retain base value, multiplier, and resolved value. With Pine Crossbow `W=20`, Grave Mark is `36`; a marked primary is `23`. These are facts for future `COM-002` consumption, not health mutation.
9. Same-tick Mark/primary behavior follows ascending projectile ID. If the Mark projectile applies before a later-ID primary contacts, the bonus exists; an earlier-ID primary does not retroactively gain it.

## Rejected options

- **Treat radius as unspecified:** contradicts the unoverridden `CONT-013` common default.
- **Pass through walls before Nailkeeper exists:** makes Nailkeeper's authored “wall impact” impossible to implement without changing base projectile behavior.
- **Use the Pine Crossbow's `0.10` radius:** silently copies an item override instead of applying the ability's shared `0.12` default.
- **Start cooldown on impact:** permits multiple unresolved Mark bolts and makes cooldown depend on range/obstruction.
- **Buffer every cooldown press:** exceeds the explicit 100 ms contract and rewards spam.
- **Mutate debug-target health:** bypasses `COM-002` and the ordered `GB-M01-05A` ticket.
- **Apply bonus retroactively within a tick:** violates stable projectile ordering and complicates server replay.
- **Float damage multiplication:** integer basis points already define exact, portable half-up behavior.

## Consequences and migration cost

- The general projectile path gains a source kind, allowing one collision system to support primary and ability bolts without duplicating geometry math.
- Raw-damage intents become the stable seam consumed later by health/damage resolution.
- Future Dented Scope, Mark Lens, Long Vigil, and Nailkeeper compilation must override the immutable resolved definition before run construction and regenerate goldens.
- Future target death/removal must clear marks through an explicit entity-lifecycle event; static M01 debug targets cannot die.
- Any change to tick order, buffer boundary, duration boundary, disposition, or intent rounding invalidates replay fixtures and requires an ADR revision.

## Validation fixtures

- Exact FP ability compilation including common-default radius and derived ticks.
- Immediate, buffered, too-early, blocked, stale, overflow, and replacement input traces.
- Enemy/solid/range projectile terminals and exact 28-step motion.
- Apply/refresh/replace/expire mark boundaries.
- Unmarked/marked/Mark raw-intent values and same-tick projectile-ID ordering.
- Engine-native deterministic showcase and warning-free optimized runtime.
