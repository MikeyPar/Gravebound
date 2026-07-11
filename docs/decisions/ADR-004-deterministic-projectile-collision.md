# ADR-004: Deterministic swept projectile collision

- **Status:** Accepted
- **Date:** 2026-07-10
- **Milestone:** GB-M01-02B
- **Owner:** Simulation/gameplay
- **GDD authority:** SIM-001, SIM-010, SIM-011, COM-001, COM-009, CLS-020
- **Content authority:** CONT-FP-002, CONT-FP-006, CONT-FP-009

## Context

The design fixes projectile size, speed, range, first-enemy termination, arena solids, and simulation authority. It does not prescribe the continuous-collision algorithm, equal-time target precedence, stable identifiers for authored solids, or how range expiry and collision share a tick. These choices affect fairness, replay hashes, later server reconciliation, and whether a fast narrow bolt can tunnel or leak through a pillar corner.

## Decision

1. Collision is renderer-independent state in `sim_core`. Bevy mirrors authoritative hurtboxes and events but never queries sprite bounds to decide a hit.
2. Every tick first clamps a projectile's candidate displacement to remaining range, then sweeps the projectile circle over that segment. Collision therefore precedes range terminalization at the same endpoint; a contact exactly at maximum range is a collision, not an expiry.
3. Shell contact is solved against the projectile-center bounds inset by its radius. Pillars use the exact rectangle Minkowski sum: face candidates plus rounded corner-circle candidates. Enemy collision uses a segment-versus-circle quadratic with the sum of both radii.
4. Contact is closed: tangency and an initial touch/overlap are hits. The projectile terminates at the earliest valid fraction in `[0,1]`, accumulates only realized distance, emits one event, and is removed.
5. Candidate fractions use `f32::total_cmp`; no nondeterministic collection iteration or arbitrary epsilon grouping participates in target selection. Exact equal fractions resolve solids before enemies, then by stable collider/entity ID. This conservative policy prevents a bolt from damaging an enemy through a coincident wall boundary.
6. Solid IDs are semantic shell sides and canonical pillar indices. `sim_content` already sorts pillars by `(y,x,width,height)`, so content source ordering cannot change their IDs.
7. Collision events use `(tick, projectile_id)` as their unique run-local event identity and carry the target stable ID, terminal center, and cumulative travel distance. Projectile iteration and emitted event order are ascending projectile ID.
8. `GB-M01-02B` terminates the current zero-pierce Pine Crossbow on either solid or enemy contact but performs no damage mutation. `GB-M01-05A` consumes future enemy collision events when it adds validated damage ordering.

## Rejected options

- **Discrete endpoint overlap:** a `0.10`-tile bolt moving `0.4` tiles per tick can tunnel through narrow targets and violates the ticket gate.
- **Expanded AABB only for pillars:** it falsely collides inside the square regions outside rounded Minkowski corners.
- **Bevy sprite/AABB collision:** presentation scale and frame scheduling would become authoritative and nondeterministic.
- **Enemy-first ties:** it can award a hit through a wall at a shared boundary.
- **Unordered collider iteration:** source JSON order or hash-map state could change replay outcomes.
- **Epsilon-based target tie grouping:** nearby but genuinely earlier contacts could be reordered; exact numeric order is the clearer deterministic contract.
- **Damage application in this ticket:** it bypasses the authored `COM-002` order and the roadmap's `GB-M01-05A` dependency.

## Consequences and migration cost

- Collision cost is linear in active projectile count times solid/enemy hurtbox count in the small M01 laboratory. A later spatial index may prune candidates only if it preserves the same candidate ordering and golden results.
- Static debug enemies can prove the contract before AI exists, and their presentation must clearly identify them as nondamageable targets.
- Any future moving-target continuous collision must define a tick-relative target trajectory in a new ADR; this decision consumes a tick-start hurtbox snapshot.
- Changing numeric precision, contact closure, range-versus-collision precedence, stable solid IDs, or tie ordering invalidates collision goldens and requires an explicit migration review.

## Validation fixtures

- Exact shell, pillar face/corner, and enemy-circle analytic fixtures.
- Adversarial tangent, overlap, corner near-miss, high-speed tunneling, range-end contact, and equal-time target fixtures.
- Stable target ordering under reordered enemy input and canonically reordered content geometry.
- Fixed multi-projectile trace with exact float-bit terminal snapshots.
- Engine-native hitbox/contact evidence and clean Windows/Linux CI.
