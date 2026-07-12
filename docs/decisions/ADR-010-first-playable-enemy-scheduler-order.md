# ADR-010 — First Playable enemy scheduler and boundary order

- **Status:** Accepted for `fp.1.0.0`
- **Date:** 2026-07-10
- **Owners:** Gameplay simulation and content validation
- **Features:** `GB-M01-03A`, `GB-M01-03B`, `GB-M01-03C`, `SIM-010`, `SIM-011`, `CONT-FP-004`

## Context

The authorities specify exact authored state order and durations but do not state whether a boundary transition waits an extra tick, which timestamp anchors repeating cycles, or how target-distance equality behaves. Those choices change cadence and deterministic hashes.

## Decision

- Simulations begin at `Tick(0)` and emit the spawn-telegraph event at tick 0. A state whose `ends_at` equals the current tick transitions and may emit its next legal warning/action on that same tick; no hidden one-tick delay is added.
- Hostile telegraphs compile with ceiling-to-tick. Dormant, recovery, cycle, active, and projectile lifetimes compile round-to-nearest as ordinary durations.
- Pilgrim aggro and five-tile stop are inclusive (`<=`); leash is exceeded only by `>` or target absence. Aim locks when windup begins and remains immutable for that cast.
- Reed and Sentry repeat cycles are anchored at telegraph start. The next telegraph begins at `cycle_started + cycle_ticks`; it is not delayed by fire, active, or recovery presentation.
- Reed omitted-gap progression and Sentry orientation toggle occur once per completed cast in stable integer order.
- Sentry contact identity is `(cast_id, player_id)`; the first contact during that active cast is accepted and repeats are ignored. Wrong/inactive cast IDs are typed failures.
- Each `advance` computes checked events/state and increments the authoritative tick exactly once. Overflow fails without a partial commit.

## Rejected options

- Transitioning one tick after equality: silently lengthens spawn/warning/cycle timing.
- Anchoring cycles at projectile fire or active expiry: drifts from the authored whole-cycle duration.
- Floating-angle/random gap progression: adds unnecessary nondeterminism.
- Letting presentation collision own Sentry hit suppression: breaks simulation authority and replay.

## Consequences and validation

Golden event ticks are Pilgrim definition trace hash `12b4a96d...1910`, Reed warnings/fire `42/56`, `132/141` with trace `86956284...4e0`, and Sentry warnings/impact `48/72`, `183/203` with trace `54b45fbe...638a4`. Changing this order requires a content/determinism version decision and new golden fixtures; it cannot be a presentation-only edit.
