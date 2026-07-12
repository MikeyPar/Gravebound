# ADR-013: Local death freeze and fresh-run reconstruction

- **Status:** Accepted
- **Date:** 2026-07-11
- **Milestone:** `GB-M01-06A`
- **Owner:** Deterministic simulation and LocalLab integration
- **GDD authority:** `COM-002`; First Playable scope; `DTH-001` intent only where explicitly narrowed by M01
- **Content authority:** `CONT-FP-001`, `CONT-FP-009`, `CONT-FP-010`
- **Roadmap authority:** M01 day 6; `GB-M01-06`

## Context

The First Playable must prove that lethal damage is final for the current local run and that retry is immediate. It has no account, server, durable item ledger, memorial, or Echo system. Reusing the dead runtime or merely restoring health would leave hostile state, item instances, projectile IDs, or delayed rewards alive across the boundary and would invalidate the later permanent-death architecture.

## Decision

1. Health zero wins in the fixed tick that produced it. Hostile damage records one pending lethal observation; later combat and consumable systems see zero health and reject their actions before the death set commits.
2. `LocalRunLifecycle` is the local transaction owner. It validates and appends the damage trace, allocates a run-qualified immutable death ID, enters `DeathFrozen`, clears the prototype inventory and belt, and records checked entity cleanup counts as one clone-before-commit operation.
3. Death and restart are separate actions. Death freezes the old run; only the primary `Run Again` action (`R` in LocalLab, scripted only for screenshot evidence) may construct a successor.
4. Frozen runtime state is never revived. The run factory rebuilds movement, full health, combat, consumables, enemy schedules, projectile allocation, and presentation bindings from validated definitions. The successor uses the documented default seed, run-qualified IDs, Pine Crossbow, Dented Scope, Reedcloth Wraps, empty Charm, and two Red Tonics.
5. Cleanup covers enemies, hostile projectiles, hostile lane hazards, friendly projectiles, field pickups, reward entities, transient effects, equipped/backpack stacks, and belt stacks. Presentation entities are counted before the authoritative commit and despawned only after that commit succeeds.
6. The local trace retains the exact preceding ten simulation seconds in stable tick order. The detailed recap belongs to `GB-M01-06B`; 06A retains and exposes the authoritative source, pattern, damage, type, position, health transition, and lethal flag.
7. Deterministic restart duration is bounded to 90 fixed ticks at the simulation seam. The client separately measures actual Run Again reconstruction wall time and rejects evidence at three seconds or more.
8. Duplicate death, invalid provenance, regressed trace ticks, cleanup-count overflow, mismatched restart ticks, and restart while alive are typed, nonmutating failures.

## Rejected options

- Set health back to maximum on the existing enemy/combat runtime.
- Restart automatically in the lethal transaction with no observable frozen boundary.
- Let presentation infer the killer from the last visible projectile.
- Clear only currently visible entities and leave inventory, delayed rewards, or allocators alive.
- Implement durable death, memorial, Echo, account progression, or item-ledger behavior before their roadmap milestones.

## Consequences

- Every future run-owned entity category must join the cleanup census and receive a run-qualified identity.
- `GB-M01-06B` can build its cause-of-death and completion views from immutable lifecycle data rather than renderer state.
- `GB-M01-07A/07B` can extend inventory and rewards without changing the death transaction boundary.
- The same factory seam can later move behind a server-authoritative run service without changing client presentation semantics.
