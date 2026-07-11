# ADR-002: Player movement, collision, and camera response

- **Status:** Accepted
- **Date:** 2026-07-10
- **Milestone:** GB-M01-01B
- **Owner:** Gameplay/client
- **GDD authority:** SIM-001 through SIM-005, CLS-020
- **Content authority:** CONT-010, CONT-011, CONT-FP-002

## Context

The design fixes movement speed, response duration, collision radius, tick rate, and camera response, but does not prescribe the discrete response or collision algorithms. Those details must be stable before gameplay, replay, and networking systems depend on movement.

## Decision

1. Authoritative player position and velocity use `f32` simulation tiles as required by SIM-001, live only in `sim_core`, and advance only on fixed 30 Hz ticks.
2. The renderer submits a compact signed digital movement action. `(-1,0,1)` axes cancel opposing inputs and diagonals use the fixed `FRAC_1_SQRT_2` components; no platform square root is needed for keyboard normalization.
3. CONT-010 round-to-nearest compiles 60 ms to two ticks. Velocity moves toward the target velocity by at most half the class speed each tick. This produces a two-tick rest/full response, preserves exact settled speed, and makes a complete reversal take four ticks because the requested velocity delta doubles.
4. Movement uses the compiled arena, a `0.30 tile` physical circle, and substeps no longer than `0.15 tiles`. Each substep clamps the shell bounds, resolves exact circle/AABB overlap against pillars in stored content order, and removes only velocity directed into the contact normal. Tangential velocity remains, producing wall sliding.
5. Any non-finite state or invalid initial overlap is a typed construction error. Collision iteration is bounded; no loop terminates based on unstable equality.
6. Camera follow is client-only. For displacement `y = camera - target`, spring velocity `v`, step `dt`, and `omega = 2 / 0.080`, the client applies the exact stationary-target critically damped solution:

   ```text
   j = v + omega*y
   decay = exp(-omega*dt)
   y_next = (y + j*dt) * decay
   v_next = (v - omega*j*dt) * decay
   camera_next = target + y_next
   ```

   Render-frame `dt` is capped at `0.25 s`; invalid or first-frame state snaps to target. The spring never writes simulation state.

## Rejected options

- **Immediate direction changes:** contradict the authored 60 ms response.
- **A first-order exponential velocity filter:** never reaches exact final speed and makes acceptance timing threshold-dependent.
- **Per-axis acceleration:** diagonals accelerate faster than cardinal movement.
- **Expanded-AABB collision:** replaces circular pillar corners with square invisible corners.
- **One large discrete collision step:** becomes unsafe as future speed modifiers or movement abilities increase displacement.
- **Camera integration in `sim_core`:** presentation frame rate and camera preferences must not affect authoritative outcomes.
- **Frame-rate-dependent interpolation factors:** do not preserve the same response across machines.

## Consequences and migration cost

- A full input reversal intentionally takes approximately 133 ms; this follows from a two-tick acceleration limit across a `10.2 tiles/second` velocity delta and should be evaluated in the M01 feel playtest.
- Future analog input can add quantized axes while retaining the normalized action boundary, but must add deterministic fixtures before use.
- Slipstep or movement above the ordinary speed band may require swept collision. The bounded-substep invariant remains mandatory.
- Changing movement response, normalization constants, collision ordering, or spring interpretation invalidates movement traces and requires this ADR, the task contract, and fixtures to change together.

## Validation fixtures

- `sim_core::movement` unit tests cover action normalization, response, equal-distance travel, shell/pillar collision, corner pressure, non-finite rejection, and deterministic replay.
- `client_bevy::player` tests cover default WASD bindings, authored/render coordinate conversion, and critically damped convergence.
- `GB-M01-01B` visual evidence demonstrates the integrated player, HUD, collision arena, and camera.
