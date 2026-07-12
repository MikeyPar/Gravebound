# ADR-007: Stillness, Focused, and avatar-tick ordering

- **Status:** Accepted
- **Date:** 2026-07-10
- **Milestone:** `GB-M01-02E`
- **Owner:** Simulation/gameplay
- **GDD authority:** `SIM-005`, `SIM-010`, `COM-002`, `CLS-020`
- **Content authority:** `CONT-010`, `CONT-011`, `CONT-FP-001`

## Context

The approved design fixes Stillness activation at `600 ms`, the movement threshold at `20%`, and Focused bonuses at `+10%` projectile speed and `+8%` primary damage. It also requires Focused to end immediately on faster movement, Slipstep, or received damage. The documents do not independently define threshold equality, whether movement is sampled before or after the avatar moves, or whether a same-tick gain/break affects primary fire. These choices must be stable across replay, future server authority, and eventual incoming-damage integration.

## Decision

1. `sim_content` compiles exactly one `ability.arbalist.stillness` referenced as the First Playable Grave Arbalist passive. Ordinary nearest-tick conversion resolves `600 ms` to exactly 18 ticks. The immutable definition also contains threshold `2,000`, projectile-speed bonus `1,000`, primary-damage bonus `800`, and both required break flags.
2. Stillness evaluates authoritative velocity after ordinary movement or the current Slipstep segment has resolved. Input magnitude is not authoritative evidence of motion. The threshold is `resolved_final_speed * 0.20`; only `velocity.length() < threshold` is eligible. Equality is moving and resets/breaks Focused.
3. One eligible post-movement sample contributes one tick. Focused is gained on the eighteenth consecutive eligible sample. Evaluation precedes primary emission, so a primary emitted on that same tick is Focused. Once active, Focused has no duration and the progress counter remains bounded at the activation value.
4. A movement-ineligible sample breaks Focused and resets progress before primary emission. Slipstep acceptance breaks Focused immediately, before its first forced segment and before same-tick primary emission. Slipstep travel cannot accumulate Stillness progress.
5. Received damage is represented by a simulation-owned `break_focused_from_damage` seam. The future `GB-M01-05A` resolver must call it after validating that damage was received and before any later same-tick primary resolution. Calling the seam while not Focused is idempotent and emits no transition. This milestone does not mutate health or synthesize a debug damage event.
6. A Focused primary captures both provenance and resolved primary parameters at emission. With Pine Crossbow, projectile speed is `12 * 1.10 = 13.2 tiles/s`; integer half-up damage is `20 * 1.08 = 21.6 -> 22`.
7. Conditional damage modifiers compose sequentially at their owning stages. Focused resolves projectile base raw damage at emission. Grave Mark resolves marked-target bonus at collision. Therefore the golden composition is `20 -> 22 -> 25`, because `22 * 1.15 = 25.3 -> 25`. Percentages are not flattened into a single combined multiplier.
8. Slipstep empowerment owns projectile-speed precedence when its armed window overlaps Focused; `+30%` and `+10%` speed do not stack. Focused damage remains independently applicable while Focused remains active. A newly begun Slipstep has already broken Focused, so its same-tick empowered primary cannot also be Focused.
9. Focused transitions carry authoritative tick, kind, and resulting Stillness progress. The client may render buildup, aura, bolt treatment, and HUD diagnostics from this state but cannot infer or mutate the passive.

## Rejected options

- **Treat equality as still:** contradicts the strict reading of "below 20%" and creates an ambiguous boundary.
- **Sample movement input:** input does not prove realized motion after acceleration, collision, forced movement, or future status modifiers.
- **Sample before movement:** allows a player beginning movement to retain Focused for one extra shot despite already moving on the authoritative tick.
- **Gain or break after fire:** delays the design's immediate state change and makes displayed state disagree with the emitted projectile.
- **Let Slipstep accumulate Stillness:** forced travel is movement and the design explicitly names Slipstep as a break.
- **Stack Focused and Slipstep speed bonuses:** silently creates `+40%` projectile speed and changes established Slipstep timing/readability.
- **Combine Focused and Grave Mark percentages before rounding:** produces a different modifier pipeline and bypasses the authored emission-versus-collision ownership boundary.
- **Add debug health damage now:** bypasses `COM-002` and the ordered `GB-M01-05A` health/damage ticket.
- **Track Focused in Bevy:** presentation frame rate and query order cannot own authoritative passive state.

## Consequences and migration cost

- Avatar stepping requires a movement state to evaluate Stillness and preserves transactional movement/combat commit behavior.
- Network snapshots and deterministic traces must eventually include Stillness progress, Focused state, transition identity, and projectile Focused provenance.
- `GB-M01-05A` must integrate the damage-break seam into its ordered damage transaction before same-tick fire; changing that precedence requires this ADR and its goldens to change together.
- `item.prototype.charm.still_eye`, Long Vigil, and future status/movement modifiers must replace the immutable compiled definition or resolved final speed before run construction; they cannot patch passive math ad hoc.
- Changes to threshold closure, movement sampling, activation boundary, break/fire order, speed precedence, or sequential modifier rounding require explicit migration review and regenerated deterministic fixtures.

## Validation fixtures

- Exact definition compilation: `18/2000/1000/800/true/true`, with authored-millisecond drift rejected before tick equivalence can hide it.
- Seventeen eligible samples remain unfocused; sample 18 gains Focused and modifies a same-tick primary.
- Strict below/equal/above threshold fixtures use post-movement velocity and prove reset/break-before-fire.
- Slipstep acceptance and the damage seam emit distinct deterministic break reasons and reset progress.
- Pine Crossbow Focused emission pins raw damage `22`, speed `13.2`, and Focused provenance.
- Same-tick Grave Mark composition pins primary `base_raw_damage=22` and `resolved_raw_damage=25` in stable contact order.
- Warning-free optimized LocalLab capture shows buildup/Focused HUD, shape-distinct Focused bolt, exact damage/speed, and the deferred health boundary.
