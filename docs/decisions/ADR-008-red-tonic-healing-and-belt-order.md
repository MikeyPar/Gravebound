# ADR-008: Red Tonic healing schedule and belt order

- **Status:** Accepted
- **Date:** 2026-07-10
- **Milestone:** `GB-M01-11`
- **Owner:** Simulation/gameplay
- **GDD authority:** `SIM-003`, `SIM-010`, `COM-004`
- **Content authority:** `CONT-010`, `CONT-FP-002`, `CONT-FP-007`, `CONT-FP-010`

## Context

The approved design fixes Red Tonic at 30% max health over 0.4 seconds, a 2-second shared potion cooldown, stack cap 6, and no interruption from damage. The First Playable begins with two Tonics in belt slot 1 and later uses a two-slot belt plus backpack. The documents do not separately define tick distribution, when the first heal occurs, overheal carry, full-health use, cooldown boundary order, or how much belt behavior may exist before the inventory ticket. These rules must be deterministic and must not consume or destroy player resources ambiguously.

## Decision

1. `sim_content` compiles exactly one `consumable.red_tonic` definition. Ordinary nearest-tick conversion resolves `400 ms` to 12 ticks and `2000 ms` to 60 ticks. Cap `6`, restore `3,000` basis points, `damage_interrupts_restore=false`, and `consumed_on_use=true` are exact First Playable invariants.
2. `consumable_1` defaults to Q and gamepad West (`X` / Square) through a replaceable presentation action map. Simulation receives a checked monotonic press sequence. Each new sequence is consumed once. Consumables do not use the ability input buffer; rejected input is never queued.
3. A press is rejected in deterministic precondition order: invalid state fails closed; then `NoTonic`, `FullHealth`, then `Cooldown`. A gameplay rejection changes no belt, restore, or cooldown state. Full health is evaluated after any restore tick already due on the same authoritative tick.
4. An accepted use atomically decrements belt slot 1, captures current resolved max health, creates one restore schedule, and sets shared cooldown to 60. The acceptance tick heals zero.
5. Restore tick `i` for `i=1..12` occurs on `T+i`. Its cumulative scheduled amount is `C_i = round_half_up(captured_max_health * 3000 * i / 120000)`. The tick's scheduled delta is `C_i - C_(i-1)`, with `C_0=0`. Direct cumulative calculation prevents iterative rounding drift and guarantees `C_12 = round_half_up(captured_max_health * 3000 / 10000)`.
6. Each delta clamps independently to current max health. Overheal discarded on one tick is not banked or redistributed. The schedule continues after a clamp; therefore damage between later ticks may still be healed by only those later deltas. Damage does not cancel, pause, or restart the schedule.
7. Authoritative tick order for this subsystem is: advance tick; apply the existing due restore tick; decrement existing cooldown; process the new consumable sequence. A use accepted at `T` is ready again at `T+60`; the new use still must pass belt and post-heal full-health checks. Since restore ends at `T+12`, the cooldown prohibits concurrent use.
8. Fresh and restarted First Playable run state is current health at max, two Red Tonics in belt slot 1, empty restore, and ready cooldown. Restart destroys all previous tonic state before constructing this baseline.
9. Pre-inventory belt merging is intentionally narrow. Additions merge into slot 1 first, then slot 2 only if it already contains Red Tonic. Neither an empty slot 2 nor the backpack is silently populated by this ticket. Any remainder is returned to the caller for `GB-M01-07A` capacity/ground handling and is never destroyed.
10. Simulation owns health changes, belt count, cooldown, restore progress, and typed use/heal events. Bevy samples input and renders feedback only. The minimal health interface introduced here must be reusable by `GB-M01-05A`; it cannot implement damage mitigation out of order.

## Rejected options

- **Heal on the acceptance tick:** shortens the authored 0.4-second restore window and makes use/fire frame ordering less legible.
- **Divide total by 12 and put the remainder on one tick:** makes distribution depend on an arbitrary remainder rule and is harder to extend to modifier-resolved max health.
- **Round each tick independently:** can accumulate to a total different from 30%.
- **Carry overheal forward:** turns damage timing after a full-health clamp into hidden stored healing not described by the design.
- **Cancel on damage:** directly contradicts `COM-004` and `damage_interrupts_restore=false`.
- **Consume at full health:** creates a resource-loss trap with no gameplay benefit and weakens input trust.
- **Queue cooldown presses:** changes a rejected consumable press into an unexpected later resource mutation.
- **Reset cooldown on rejected input:** enables griefy or accidental self-lockout and violates transactional rejection.
- **Create slot 2 or backpack behavior now:** invents `GB-M01-07A` inventory policy before item instances, capacity, and ground remainder exist.
- **Discard merge overflow:** silently destroys a local personal resource.
- **Let Bevy own healing:** makes render scheduling authoritative and breaks deterministic replay.

## Consequences and migration cost

- Healing schedules must retain captured max health, next restore ordinal, and cumulative scheduled amount or an equivalent replay-stable representation.
- Future max-health changes during the 12-tick window do not rewrite the captured schedule; application still clamps to the current legal maximum.
- `GB-M01-05A` reuses the health owner and inserts ordered incoming damage without changing the restore schedule or damage-noncancel rule.
- `GB-M01-07A` must connect returned merge remainder to slot-2 creation/rearrangement, backpack insertion, and personal ground lifetime while preserving the same slot1/existing-slot2 priority.
- Undertaker Knot later replaces restore and cooldown values before immutable run construction and regenerates schedule/cooldown fixtures.
- Any change to first-heal tick, cumulative rounding, overheal disposal, precondition order, cooldown boundary, merge order, or restart baseline requires this ADR and deterministic fixtures to change together.

## Validation fixtures

- Exact content compilation pins `6/3000/12/60/false/true` and rejects authored millisecond drift before equivalent tick rounding can hide it.
- Full-health, no-tonic, and cooldown presses produce typed rejection with no item/cooldown/restore mutation.
- Multiple max-health fixtures pin all 12 cumulative deltas and exact final scheduled 30% half-up result.
- Overheal is discarded per tick; intervening damage receives only future scheduled deltas and does not cancel the restore.
- Use at `T`, healing on `T+1..T+12`, and next-ready boundary `T+60` are exact.
- Fresh/restarted run pins slot 1 count 2. Merge fixtures pin slot1, existing-Tonic slot2, cap 6, and returned remainder.
- Warning-free optimized LocalLab capture shows accepted use, health movement, belt decrement, restore progress, cooldown, and typed rejection without obscuring the arena.
