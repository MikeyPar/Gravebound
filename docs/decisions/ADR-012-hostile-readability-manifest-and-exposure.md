# ADR-012: Hostile readability manifest and mechanic exposure gate

- **Status:** Accepted
- **Date:** 2026-07-11
- **Milestone:** `GB-M01-05B`
- **Owner:** Simulation/readability/client presentation
- **GDD authority:** `COM-005`, `COM-006`, `COM-009`, `ART-005`, `ART-006`
- **Content authority:** `CONT-010` through `CONT-013`, `CONT-FP-004`, `CONT-FP-009`

## Context

Individual pattern validation already proved warning minima, geometry, counterplay, compatibility, and safe paths. It did not prove aggregate encounter caps, actual hostile-over-friendly presentation priority, grayscale-distinct visual grammar, or the rule that repeated-use timing is unavailable until a mechanic has been shown completely once.

## Decision

1. `sim_core::readability` compiles only previously validated patterns. It does not duplicate pattern geometry or scheduler authority.
2. The canonical visual stack is `telegraph(50) > hostile projectile(40) > friendly projectile(30) > loot(20) > decorative(10)`. Client Z ordering must preserve the same strict order.
3. Fan, ring-gap, and lane patterns use distinct grayscale signatures: tapered dart, hollow ring, and bounded lane band. Color remains supplemental damage-family information.
4. Major-or-higher attacks use thick/white-core treatment and audio priority 100; ordinary warning audio uses priority 80. Actual audio playback/assets remain the M01 audio/accessibility pass, but invalid priority/cue metadata cannot compile.
5. Encounter manifests sort by stable pattern ID, reject duplicates/mixed contexts, and checked-sum threat plus maximum active instances. The aggregate active count must remain within the applicable 300/500 cap.
6. `TelegraphExposureTracker` is the reusable state owner for first/repeated warning eligibility. Repeated timing is illegal until first telegraph, resolve, and completion occur in order. Wrong tick counts and illegal transitions are typed and transactional.
7. LocalLab consumes the compiled manifest and authoritative enemy events. It displays warning ticks, counterplay, grayscale grammar, priority, cue priority, aggregate budgets, observed exposure counts, and repeat legality without recomputing gameplay schedules.
8. Evidence captures only after each of the three normal mechanics has produced at least two warnings, both projectile grammars have fired repeatedly, and the Sentry lane is no longer active.

## Rejected options

- Color-only differentiation.
- One diamond sprite for every hostile projectile family.
- Presentation-owned warning timers or inferred exposure state.
- Per-pattern cap validation without aggregate encounter accounting.
- Allowing repeated warning timing immediately because the record contains both values.
- Treating an incomplete GPU screenshot as evidence.

## Consequences

- New pattern kinds require a reviewed grayscale signature and cannot silently reuse another mechanic's shape grammar.
- Client layer constants are tested against the canonical manifest ordering.
- Audio assets/playback can be added later without changing warning priority semantics.
- Bell Proctor now compiles into its separate boss-context manifest at threat `41` and maximum active `36/500`; ADR-017 records the bundle correction and canonical Chip fan result.
