# ADR-011: Pattern fairness fixtures and deterministic diagnostics

- **Status:** Accepted
- **Date:** 2026-07-11
- **Milestone:** `GB-M01-04A`
- **Owner:** Simulation/content tooling
- **GDD authority:** `ENC-003`, `ENC-004`, `COM-005`, `COM-006`, `SIM-010`, `SIM-011`
- **Content authority:** `CONT-010`, `CONT-011`, `CONT-012`, `CONT-013`, `CONT-FP-004`, `CONT-FP-005`

## Context

First Playable enemies and Bell Proctor already use concrete scheduler-owned attack structures. The milestone also requires a common authoring/validation contract for telegraphs, fairness, compatibility, threat, caps, and fixed timelines. Replacing those schedulers would create unnecessary drift; trusting loosely populated fixture flags would fail to prove the actual arena geometry.

## Decision

1. `sim_core::pattern` is a validation and normalized-definition boundary, not a second attack scheduler. Existing `ProjectileAttackDefinition` and `LaneAttackDefinition` values adapt into common pattern definitions without mutation.
2. The supported grammar is fan, ring-with-gap, telegraphed lane, and fixed timeline. Kind-specific fields remain closed and validators reject mismatched geometry, shape, counterplay, or disposition.
3. Stable cue IDs derive mechanically from pattern ID. Major, Severe, and Execution add the `.warning.major` cue. Missing or unexpected cue variants are typed errors.
4. Hostile warnings retain authored milliseconds and compile with ceiling-to-tick only after exact minimum validation. The normalized definition stores both authored data and compiled warning ticks.
5. Fairness arithmetic is integer-only. Projectile arrival uses ceiling distance/speed time. Corridor clearance uses millitiles and the exact `0.25 + 0.15 = 0.40 tile` boundary distance.
6. A fairness fixture must declare the exact baseline `4.5 speed / 0.25 radius / 120 ms RTT / no ability`. A Frostbind-compatible mandatory overlap must additionally carry an exact 4.0-speed solver result.
7. The pure validator checks fixture semantics and exact numeric boundaries. A separate arena/path solver must produce those fixture results from actual geometry. Until that solver is connected, the presence of a fixture is necessary but not sufficient completion evidence.
8. Maximum active instances are authored scheduler-trace facts and must be positive and within context cap. The validator does not estimate them from average cadence.
9. Compatibility uses sorted tags and forbidden tags. Any forbidden intersection fails. Frostbind plus a mandatory pattern without the exact slow-speed fixture also fails even when a tag rule already rejects it; both diagnostics remain observable.
10. Phase projectile policy is explicit and must be `cancel_on_phase_change=true` for current First Playable definitions. A later exception requires a distinct record and review.
11. Validation accumulates, sorts, and deduplicates typed diagnostics. It does not stop at the first malformed field or rely on map iteration.
12. LocalLab consumes the same validated definition and scheduler events for timeline, threat, cap, compatibility, and corridor overlays. Bevy cannot infer validity or timing from rendered effects.

## Rejected options

- **Replace enemy/boss schedulers with a new generic runtime now:** increases regression risk and duplicates already-tested exact schedules.
- **Validate only JSON shape:** schema-valid unsafe warnings, corridors, caps, and compatibility would reach runtime.
- **Use floating-point geometry thresholds:** introduces tolerance ambiguity at the exact fairness boundary.
- **Round hostile warnings to nearest tick:** can shorten the authored minimum.
- **Trust `passes=true` without a geometry solver:** records intent, not proof.
- **Calculate active cap from average cadence:** misses overlap peaks; owning scheduler goldens are authoritative.
- **Permit empty compatibility tags:** prevents deterministic combination rejection.
- **Emit only the first diagnostic:** slows content iteration and hides independent defects.
- **Let the debug overlay recompute timelines:** creates a second, presentation-owned schedule.
- **Mutate the promoted bundle while version ownership is unresolved:** violates the recorded immutability rule.

## Consequences and migration cost

- Strict content compilers must supply every normalized field and preserve authored milliseconds before tick conversion.
- Existing enemy/boss definitions remain usable and gain a lossless validation adapter rather than a rewrite.
- Arena geometry or player footprint changes invalidate safe-path fixtures and evidence even when pattern records are unchanged.
- Scheduler changes invalidate maximum-active-instance fixtures and timeline/debug goldens.
- New pattern kinds, phase-cancel exceptions, player-piercing attacks, acceleration, or compatibility semantics require schema, this ADR, validators, and evidence to change together.
- Bell Proctor content compilation remains blocked until the bundle version decision authorizes exact records/manifests; simulation definitions do not bypass that gate.

## Validation fixtures

- All five exact first/repeat warning minima and compiled ceiling ticks.
- Lossless adapters for Pilgrim fan, Reed ring, Sentry lanes, and Bell Proctor attacks.
- Exact `0.40` boundary clearance and `0.80/0.65` corridor helpers.
- Exact `350 ms` projectile-arrival and `1.25 tile / 750 ms` close-spawn boundaries.
- Normal 300 and boss 500 caps, positive threat, stable cue derivation, and cancellation policy.
- Strict fixed timeline ordering/reference bounds.
- Forbidden tag and unsafe Frostbind overlap diagnostics.
- Multi-error sorted/deduplicated adversarial golden.
- Completed by the 04A audit: arena solver golden, strict record compilation, cap/threat presentation, LocalLab overlay, and optimized inspected evidence.
