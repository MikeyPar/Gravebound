# ADR-009: Direct-damage fixed-point order and damage bands

- **Status:** Accepted for the pure resolver; integration clauses remain GB-M01-05A work
- **Date:** 2026-07-10
- **Milestone:** `GB-M01-05A`
- **Owner:** Simulation/gameplay
- **GDD authority:** `COM-001`, `COM-002`, `COM-003`, `SIM-010`
- **Content authority:** `CONT-011`, `CONT-FP-004`, `CONT-FP-005`, `CONT-FP-009`

## Context

The GDD fixes direct-hit order, ordinary resistance bounds, strongest-only reduction, armor's 35% limit, one whole-damage rounding point, barrier-before-cap order, and player damage bands. The implementation also needs exact integer representations, observable intermediates, low-health/overkill behavior, and a safe boundary between the pure resolver and the future authoritative hostile-hit transaction.

## Decision

1. `DamageType` is a closed M01 enum with `Physical` and `Veil`. `DamageBand` is a closed ordered enum from Chip through Execution. Critical-hit, random-evasion, cheat-death, and resurrection state is intentionally absent.
2. A `DirectHitRequest` can be constructed only for distinct typed source/target IDs, confirmed collision, nonimmune living target, positive raw damage/multiplier, valid reductions/cap, and legal health. Structural rejection precedes arithmetic and mutation.
3. Fractional damage is represented in integer units of `1/10000` damage using checked `u64`. Each basis-point stage truncates only precision below `1/10000`; health damage is converted to a whole integer exactly once after armor using half-up. No floating-point value participates.
4. Resolution order is attacker multiplier, resistance factor, strongest direct reduction, armor, whole-damage rounding, barrier, optional health cap, then health. The event records both the supplied and applied resistance; ordinary resistance clamps to `-2500..=2500`.
5. Direct reductions do not stack. The maximum validated basis-point value wins. A 100% reduction may produce zero; otherwise positive fixed damage rounds to at least one.
6. Armor is converted to the same fixed scale. Its reduction is `min(armor_fixed, damage_after_reduction_fixed * 3500 / 10000)`. The result remains fixed until the one half-up conversion.
7. Barrier absorbs the rounded integer before a cap. Optional max-health-share caps use conservative integer flooring so health damage never exceeds the declared share. The event separately records cap input, integer cap, cap reduction, potential post-cap health damage, actual health lost, and new health.
8. Actual health loss saturates at current health, but potential post-cap damage is retained for trace/overkill analysis. `lethal` is exactly `health_after == 0`. The pure resolver does not mutate its input snapshot.
9. Runtime band classification uses potential final health damage after barrier and cap, before current-health saturation. Zero has no band. Positive damage at or below 8% is Chip, then `>8..=18` Pressure, `>18..=35` Major, `>35..=60` Severe, and `>60` Execution. Runtime sub-1% positive damage is represented as Chip; strict authored validation rejects it because `COM-003` begins at 1%.
10. Strict band validation compares an authored band with the resolved reference result and separately enforces whether Execution is allowed. First Playable standard content passes `execution_allowed=false`.
11. The complete `DamageEvent` is the trace, presentation, telemetry, and death-cause seam. It includes all source/type, multiplier, clamp, fixed damage, armor, rounded damage, barrier, cap, health, band, and lethal fields required to reconstruct the result.
12. Integration must stage the pure result and atomically commit shared vitals/barrier, hostile projectile disposition/grace, Focused damage break, event, and lethal action rejection. The `GB-M01-06A` death transaction consumes the lethal handoff; it does not rerun damage math.

## Rejected options

- **Floating-point damage:** platform/compiler differences and repeated rounding make replay and trace comparison weaker.
- **Round at every modifier:** violates `CONT-011` and changes sequential modifier outcomes.
- **Stack direct reductions:** contradicts strongest-only `COM-002` behavior.
- **Apply armor before direct reduction:** changes the authored order and armor cap basis.
- **Allow armor to reduce a hit to zero:** violates the 35% armor limit and positive-hit minimum.
- **Apply the health cap before barrier:** makes a barrier reduce a value already capped and contradicts the explicit order.
- **Round a percentage cap upward:** may exceed the declared maximum-health share.
- **Classify using actual low-health loss:** a nearly dead target would make a Severe attack appear Chip and corrupt tuning/death telemetry.
- **Treat zero barrier damage as Chip:** fully absorbed damage has no final health-damage share.
- **Permit Execution by enum alone:** standard Early Access content explicitly prohibits it.
- **Mutate health inside the pure function:** prevents transactional composition with collision, grace, Focused, event emission, and lethal handoff.
- **Add crit/evasion/resurrection flags for future use:** those mechanics are explicitly prohibited and would create unsupported states.

## Consequences and migration cost

- Consumers must retain and commit the returned new barrier/health values; reading only `health_damage_applied` is insufficient for atomic replay.
- The existing Red Tonic `PlayerVitals` owner must become the one shared authoritative health state rather than maintaining a second client or combat health value.
- Content validation needs reference resolved stats, not raw damage alone, to validate authored bands. Changes to starting gear, health, armor, resistance, barrier, or modifiers may require band fixtures to change.
- Network snapshots and traces must eventually carry vitals, barrier, grace, and stable damage event identity/order.
- Health-cap rounding, band classification stage, fixed precision, or any pipeline reordering requires this ADR, content fixtures, deterministic goldens, and death telemetry consumers to migrate together.

## Validation fixtures

- Baseline physical hit records every exact fixed intermediate and health result.
- Attacker multiplier, `-25%/+25%` resistance clamp, strongest reduction, and armor cap order are independently pinned.
- Half-up `0.5 -> 1`, tiny positive minimum 1, and full direct reduction to zero are pinned.
- Partial/full barrier, barrier-before-cap, conservative 35% cap, overkill saturation, and lethal flag are pinned.
- Exact band boundaries and sub-1%/mismatch/forbidden-Execution errors are pinned without floating point.
- Invalid collision/immunity/stats/health and arithmetic overflow return typed errors.
- End-to-end fixtures remain required for projectile grace, shared vitals, Focused break, Red Tonic continuation, M01 reference bands, deterministic trace, client HUD/debug feedback, and lethal handoff.
