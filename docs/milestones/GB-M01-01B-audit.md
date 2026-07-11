# GB-M01-01B completion audit

- **Status:** Passed
- **Audited:** 2026-07-10
- **Authorities reviewed together:** GDD `SIM-001` through `SIM-005`, `CLS-020`, and Section 29; content specification `CONT-010`, `CONT-011`, and `CONT-FP-001`/`CONT-FP-002`; roadmap M01 day-one target, work package `GB-M01-01`, and implementation order 10
- **Feature registry:** `GB-M01-01B`, depending on `GB-M01-01A`
- **Implementation commits:** `e00d026`, `90ad582`
- **Verification commits:** `933bad8`, `9ed45b6`
- **Decision:** `ADR-002`
- **Next feature:** `GB-M01-02A`

## Acceptance evidence

| Criterion | Evidence | Result |
|---|---|---|
| Rebind-ready fixed-step movement | `MovementBindings` owns replaceable keys; Bevy samples `ButtonInput` per rendered frame into one compact action; `PlayerMovementState` alone advances position at 30 Hz. Direct input tests prove WASD, rebound arrows, diagonal composition, and opposing-key cancellation. | Passed |
| Exact speed and response | `CLS-020` speed is `5.1 tiles/second`. CONT-010 rounds 60 ms to two ticks, so vector velocity approaches target by `2.55 tiles/second` each tick. Tests prove half speed on tick one, exact settled speed on tick two, exact stop after two neutral ticks, and equal cardinal/diagonal path lengths within `0.0005 tiles` after 100 ticks. | Passed |
| Solid arena collision | The simulation uses the compiled `32 × 24` arena and `0.30 tile` physical radius. Movement substeps are at most `0.15 tiles`; exact circle/AABB depenetration, stable pillar order, and inward-velocity removal preserve tangential sliding. Sustained shell, pillar, and diagonal-corner pressure tests remain finite and nonpenetrating. | Passed |
| Camera is presentation-only | The `80 ms` analytic critically damped spring is a Bevy component/system with no `PlayerSimulation` write access. Tests prove convergence without overshoot at 60/120 Hz, safe invalid-state snapping, and unchanged authoritative movement state. | Passed |
| Integrated readable runtime | LocalLab renders the controllable Grave Arbalist at `(4,12)`, live position/speed/radius diagnostics, exact arena geometry, and the player-centered orthographic view. The engine-native capture completed after ten rendered frames. | Passed |

## Verification

- `tools\dev.cmd ci`: passed.
- Workspace results: 33 tests passed, 0 failed.
  - `client_bevy`: 8 render/input/camera-boundary tests.
  - `content_schema`: 3 strict ID/schema tests.
  - `sim_content`: 6 package/reference/exact-arena tests.
  - `sim_core`: 16 geometry/movement/determinism tests.
- Format and full pedantic Clippy: passed with warnings denied.
- Strict `fp.1.0.0` validation: passed, 30 records.
- Generated schemas: current with no diff.
- M00 golden trace: passed twice in separate processes with identical hashes at ticks 1, 30, 60, 90, and 120. The movement replay additionally matches a checked-in exact four-field `f32::to_bits` snapshot.
- Local debug runtime: launched, remained responsive, and emitted engine-native evidence.
- Local optimized Windows build and runtime: passed, remained responsive, and produced byte-identical evidence to debug.
- Evidence SHA-256: `0E797527612F73790D9F4D73056AB6806DAF245DD57B0734217B13DD98665A69`.
- GitHub clean CI: recorded in the final audit commit/check run.

## Visual review

![Accepted Grave Arbalist movement frame](../evidence/GB-M01-01B.png)

The frame confirms the high-luminance player silhouette, exact `(4.00,12.00)` spawn, `5.1 tiles/s` class cap, `0.30` collision radius, rebind-ready control label, centered camera, and unchanged geometry/debug hierarchy. Windows desktop capture could not attach to the Bevy swapchain (`0x80004002`), so no UI input was attempted after that observation failure. Runtime evidence uses the existing engine-native capture path; movement, collision, binding, and camera dynamics are proven by direct boundary/integration tests rather than inferred from a still image.

## Changed ownership boundaries

- `sim_core::movement`: movement action normalization, fixed-tick response, authoritative position/velocity, typed construction failures, and shell/pillar collision; no Bevy dependency.
- `client_bevy::player`: replaceable keyboard bindings, per-frame sampling, player presentation, movement diagnostics, and presentation camera spring.
- `client_bevy::arena_view`: simulation-to-render coordinate conversion and camera/HUD composition.
- The camera system can read only rendered player/camera transforms; simulation state is neither a parameter nor an output.

## Adversarial audit

- Excess axis values clamp to `[-1,1]`; opposing bindings cancel without event-order dependence.
- Non-finite or solid-overlapping initial states fail closed with typed `MovementError` variants.
- Cardinal and diagonal target vectors have equal magnitude; acceleration is vector-based rather than per-axis.
- Movement cannot tunnel through current solids because the maximum ordinary substep is half the player radius and materially thinner than the one-tile shell/pillars.
- Collision loops have fixed pass bounds and stable authored pillar order.
- Invalid camera values snap safely; render `delta_seconds` is capped at `0.25` and never enters simulation.

## Deferred scope and conflicts

- Controller/analog axes, a settings UI, and persisted bindings remain future accessibility/settings work; the resource boundary is ready for them.
- Mouse aim, held primary fire, and projectile presentation begin in `GB-M01-02A`; hitbox/solid/enemy collision remains `GB-M01-02B` per the roadmap.
- A complete `+5.1` to `-5.1` reversal takes four ticks because the velocity delta is double a rest-to-full change. ADR-002 records this consequence for M01 feel testing.
- No unresolved conflict was found among the three design documents. The discrete response and collision math absent from the design package are now explicit in ADR-002 and the task contract.
