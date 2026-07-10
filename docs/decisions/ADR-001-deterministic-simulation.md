# ADR-001: Deterministic simulation and RNG boundary

- **Status:** Accepted
- **Date:** 2026-07-10
- **Milestone:** GB-M00
- **Owner:** Gameplay/backend
- **GDD authority:** TECH-001, TECH-004, TECH-006, TECH-060, TECH-070
- **Content authority:** CONT-010, CONT-011, CONT-012

## Context

LocalLab, future authoritative servers, headless tests, and replay playback must execute identical gameplay rules. Bevy presentation timing, operating-system entropy, hash-map order, and platform floating-point edge cases cannot own authoritative outcomes.

## Decision

1. `sim_core` is a renderer-independent Rust library. It cannot depend on Bevy, platform APIs, databases, or wall-clock time.
2. The authoritative clock is an integer `Tick(u64)` at exactly 30 Hz. Presentation accumulation stores `elapsed_nanoseconds × 30` and consumes each complete `1,000,000,000` unit, avoiding drift from a rounded nanosecond step. Authored durations compile to ticks under CONT-010 before gameplay execution.
3. Simulation inputs are associated with an explicit target tick. A step consumes one immutable input frame per controlled entity in stable entity-ID order.
4. Runtime entities use monotonic nonzero `EntityId(u64)` values allocated by simulation state. Content uses validated lowercase dot-separated `ContentId` strings. Neither depends on Bevy ECS entity values.
5. Canonical state hashes use BLAKE3 1.8.3 over an explicitly ordered, little-endian byte encoding. Serialization formats and debug text are not hash inputs.
6. Deterministic random streams use `ChaCha8Rng` from `rand_chacha` 0.10.0 and `rand_core` 0.10.1. Call sites consume raw `next_u32`/`next_u64` values only; bounded selection uses committed rejection sampling, never convenience distribution APIs.
7. A 32-byte stream seed is:

   ```text
   BLAKE3("gravebound-rng-v1\0"
          || little_endian_u32(len(content_version)) || UTF8(content_version)
          || little_endian_u64(root_seed)
          || little_endian_u32(len(stream_label)) || UTF8(stream_label))
   ```

8. Gameplay systems receive named, independently derived streams. Adding a draw to one stream cannot perturb another stream. Initial M00 labels are `simulation`, `spawn`, `pattern`, `loot`, and `fixture`.
9. Unordered collections are never iterated to choose or hash gameplay state. Values are stored in ordered collections or copied to a stable sorted view first.

## Rejected options

- **Bevy `Time` as authority:** render scheduling differs from headless/server execution.
- **Thread-local or operating-system RNG:** cannot replay and cannot isolate draw-order changes.
- **`StdRng` or unspecified random distribution helpers:** algorithm/output mapping is not the product contract.
- **One global random stream:** unrelated feature changes would invalidate every downstream outcome.
- **JSON/debug serialization for state hashes:** field ordering and formatting are not a stable canonical encoding.
- **Fixed-point conversion of every value in M00:** unnecessary before consuming systems exist. Authoritative values begin integer/fixed-point and any future float boundary requires its own fixture and ADR amendment.

## Consequences and migration cost

- Replays store root seed, content version, trace schema version, and input frames—not random outputs.
- Changing RNG algorithm, crate version, seed construction, canonical state encoding, tick rate, or rounding is a replay/content compatibility break. It requires a new ADR revision, content-version major increment, trace migration or explicit invalidation, and regenerated golden fixtures.
- New gameplay domains must declare their stream labels before merge.

## Validation fixtures

- `tests/deterministic/m00_smoke.json`: known seed and fixed inputs.
- `tests/deterministic/m00_smoke.golden.json`: selected-tick state hashes.
- Unit fixtures cover tick conversion, seed construction, bounded rejection, stream separation, stable entity allocation, and canonical hash ordering.
- CI runs the trace twice in separate processes and compares exact output bytes.
