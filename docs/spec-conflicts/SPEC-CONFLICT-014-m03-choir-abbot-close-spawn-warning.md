# SPEC-CONFLICT-014 — M03 Choir Abbot close-spawn warning

**Status:** Owner-approved on 2026-07-13

**Raised:** 2026-07-13

**Blocks:** `GB-M03-03D` COM-006 proof for `miniboss.choir_abbot.recovery_ring`

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The Content Production Specification gives the Choir Abbot a stationary `0.55`-tile collision radius and a `2.5 s` recovery, ending in `miniboss.choir_abbot.recovery_ring`. The ring's target-facing four-shot gap is directionally previewed only during the final `650 ms` of recovery, with Major bell audio.

Canonical GDD `COM-006` separately prohibits a hostile projectile from spawning within `1.25 tiles` of a player unless a ground telegraph has existed for at least `750 ms`. A player with the canonical `0.25`-tile collision radius may legally stand `0.80 tiles` from the stationary Abbot's center. The directional preview therefore cannot also be the complete close-spawn warning without changing an exact Content Specification timing.

The Development Roadmap requires the complete Core encounter roster and COM-006 validation before `GB-M03-03` can admit the normal player route.

## Approved resolution

- Preserve the exact final `650 ms` target-facing directional gap preview and its Major bell audio.
- Add a low-intensity, non-directional ground-origin warning at the Abbot when recovery starts. It remains visible for the complete `2.5 s` recovery and therefore precedes the ring spawn by `2.5 s`.
- The origin warning communicates that a hostile release will occur from the Abbot, but it does not reveal or lock the target-facing gap before the final `650 ms` directional preview.
- The Abbot emits no hostile output during recovery before the ring release.
- This clarification does not change recovery duration, ring cadence, damage, projectile count, omitted gap, projectile speed, range, radius, counterplay, quiet time, or scheduler capacity.
- Reduced-effects presentation must preserve both warning geometries, their timings, and the Major audio cue; it may only reduce decorative animation intensity.

## Typed implementation contract

`RecoveryPreview` carries two independently named durations:

- `ground_origin_warning_milliseconds = 2500`
- `directional_gap_preview_milliseconds = 650`

The simulation compiles them once to `75` and `20` ticks respectively at 30 Hz. Validation rejects a recovery preview when the origin warning is below the `750 ms` close-spawn minimum, the directional preview is below its damage-band minimum, or the directional preview exceeds the origin warning.

## Approval record

The owner approved this resolution without amendment on 2026-07-13.
