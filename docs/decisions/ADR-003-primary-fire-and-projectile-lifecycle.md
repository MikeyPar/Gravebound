# ADR-003: Primary-fire timing and projectile lifecycle

- **Status:** Accepted
- **Date:** 2026-07-10
- **Milestone:** GB-M01-02A
- **Owner:** Gameplay/client
- **GDD authority:** SIM-003, SIM-004, SIM-010, SIM-011, COM-001, CLS-020, TECH-004, TECH-011
- **Content authority:** CONT-010, CONT-011, CONT-013, CONT-FP-006, CONT-ITEM-002

## Context

The design fixes the action model and Pine Crossbow values but leaves tick ordering, first-shot timing, zero-length mouse aim, and final-range motion implicit. Those details affect feel, replay hashes, later server reconciliation, and collision, so they must be stable before `GB-M01-02B` depends on them.

## Decision

1. `sim_content` compiles the weapon item and class-primary ability into one immutable `sim_core::WeaponDefinition`. Required item effects are unique `set` operations; the ability must agree on range, radius, rate, and stop behavior.
2. CONT-010 round-to-nearest compiles 455 ms to 14 ticks. A ready held weapon fires on the current fixed tick, then becomes legal again exactly 14 tick indices later. Release never shortens or resets the timer.
3. Each physical press has a checked monotonic `u32` sequence. Render-frame resampling retains the same value while held. Rising edges with a reused/regressed sequence fail closed; future network code may discard a stale datagram before calling simulation.
4. Aim is a finite nonzero normalized `f32` direction in northwest simulation coordinates. Cursor conversion is client-owned; simulation rejects invalid directions. The client retains the last valid aim and starts east when cursor data is unavailable or coincident with the player.
5. Fire uses the player's authoritative center and locks aim for the projectile lifetime. Muzzle offset and flash are presentation-only and cannot alter range/collision geometry.
6. Existing projectiles advance before firing. New projectiles do not move until the next tick. Stable IDs define iteration and event order.
7. A 12-tile/s bolt advances 0.4 tiles per tick. Travel is clamped to remaining range; 23 full steps reach 9.2 and the 24th reaches exactly 9.5, then expires. It never travels the rounded-lifetime overshoot to 9.6.
8. `GB-M01-02A` performs no solid or entity collision and no damage. `GB-M01-02B` will insert deterministic swept collision before range terminalization without changing fire cadence, IDs, or aim locking.

## Rejected options

- **Fire after waiting one interval on initial press:** makes input feel latent and is not required by the authored timer rule.
- **Reset cooldown on release:** permits click timing to exceed the item interval.
- **Render-frame fire timers:** cadence changes with frame rate and violates server ownership.
- **Aim point stored on each projectile:** a moving target point would curve shots or change direction after release.
- **Authoritative muzzle offset:** no such geometry is authored and it would silently change effective range.
- **Move a newly spawned projectile on its fire tick:** obscures origin/event ordering and shortens visible lifetime by one update.
- **Expire at 9.6 tiles after 24 full steps:** exceeds the exact item range.
- **Partial wall-only collision in 02A:** would create two projectile collision contracts and pre-empt the ordered `GB-M01-02B` ticket.

## Consequences and migration cost

- Held Pine Crossbow shots occur on tick indices `T, T+14, T+28...` while continuously held.
- With a 24-travel-tick lifetime and 14-tick interval, at most two Pine Crossbow bolts are active from one unmodified player in empty space.
- A projectile can visually overlap a pillar in this ticket; debug presentation labels collision as pending rather than implying the behavior is final.
- Future prediction/server code must preserve press sequence, shot tick, ID, and tick ordering. Any change invalidates combat traces and requires an ADR revision plus fixture migration.

## Validation fixtures

- `sim_core::combat` exact shot-tick and projectile-position snapshots.
- `sim_content` Pine Crossbow success and malformed/mismatched record failures.
- `client_bevy::combat` input binding, cursor conversion, gating, and movement-independence tests.
- `GB-M01-02A` engine-native visual evidence and clean Windows/Linux CI.
