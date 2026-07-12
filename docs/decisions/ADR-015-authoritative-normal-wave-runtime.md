# ADR-015 — Authoritative normal-wave runtime

- **Status:** Accepted for `GB-M01` implementation
- **Date:** 2026-07-11
- **Owner:** Gameplay / simulation
- **Scope:** `GB-M01-03`, `GB-M01-07`, `GB-M01-08A`, `CONT-FP-003`

## Authorities reviewed together

- `Gravebound_Production_GDD_v1_Canonical.md`: `TECH-001`, `SIM-010`, `SIM-011`, `COM-001`, `COM-009`.
- `Gravebound_Content_Production_Spec_v1.md`: `CONT-FP-002` through `CONT-FP-004`, `CONT-FP-008`, `CONT-FP-009`.
- `Gravebound_Development_Roadmap_v1.md`: M01 objective, work packages `03`, `05`, `07`, `08`, and deterministic exit gate.

## Context

The first semantic enemy slice used one Drowned Pilgrim, one Bell Reed, and one Chain Sentry in a fixed showcase coordinator. That harness correctly proved each ordinary kit, but it cannot represent the exact authored `4/6/6` Wave 1–3 rosters or duplicate instances of one enemy kind. The encounter director already owns the exact stage schedule, spawn identities, delays, and reward transitions. The client must connect that director to real duplicate-safe simulation rather than display a wave label over an unrelated showcase.

## Decision

1. `sim_core` owns a generalized normal-wave simulation keyed by sorted, run-qualified `SpawnInstanceId`. Duplicate content kinds are legal; duplicate instance or entity IDs fail transactionally.
2. One instance owns one actor, one ordinary timeline, one health record, and one stable presentation identity. Hostile projectiles are shared by the wave; active lanes are keyed by source instance and cast rather than stored as a singleton.
3. The existing fixed-three `EnemyLab` remains a semantic/evidence harness only. Ordinary LocalLab play and the encounter debug scenario use the generalized wave path, never both paths concurrently.
4. The encounter director remains the sole stage/schedule authority. `SpawnTelegraphStarted` creates the exact pending roster at authored anchors/points. The wave timeline advances across that same 27-tick interval, so there is exactly one 900 ms telegraph. It exposes no hurtbox and authorizes no attack before activation.
5. Fixed client order for the normal path is input/movement and player combat, then normal-wave resolution, then encounter event acceptance, consumables/inventory, and death. This lets real friendly damage produce sorted defeat IDs for the encounter in the same engine tick. Health zero and hostile cleanup commit before later presentation.
6. Clock mapping is explicit: the first player combat step is tick `0`; the first encounter step advances to tick `1`. Therefore `encounter_tick = combat_tick + 1`. A telegraph beginning at encounter tick `46` begins on combat tick `45`; activation at encounter tick `73` is combat tick `72`, exactly 27 fixed ticks later. Wave timelines accept a global start tick and emit globally comparable ticks without resetting persistent player cooldowns or consumables.
7. When the last real enemy dies, the wave clears every hostile projectile and every active lane in the same transaction. The encounter then owns the exact 45-tick reward delay. No hostile simulation runs during reward delay/open.
8. The client presentation is event-driven and keyed by stable simulation identity: telegraph on scheduled spawn, actor on activation, death/removal once, and complete cleanup on death/restart. Bevy entity IDs never enter deterministic state.
9. Wave reward resolution is immutable once opened and uses ADR-014's versioned deterministic stream. The panel cannot close until the player accepts a legal grant or explicitly leaves it. Normal drops remain actual 60-second field pickups and become due exactly eight ticks after their owning enemy death.

## Rejected options

- **Reuse one semantic trio and change labels/positions per wave.** Rejected because it cannot represent duplicate actors, exact budgets, or maximum-load behavior.
- **Run one `EnemyLab` per spawned enemy.** Rejected because each lab owns three actors, a player, projectiles, and a lane, producing duplicate authority and irreconcilable damage order.
- **Keep encounter state only in Bevy.** Rejected by `TECH-001`/`SIM-010`; replay and future server execution must use identical rules.
- **Reset combat/consumable clocks for each wave.** Rejected because it silently refreshes cooldowns, cancels projectiles/restores, and breaks deterministic run time.
- **Inject defeat IDs directly for client progression.** Rejected except in isolated director unit tests; playable progression must bridge real health death events.

## Migration cost

- Add the duplicate-safe simulation owner and global clock seam in `sim_core`.
- Add a normal/showcase runtime mode at the client boundary while retaining existing focused evidence scenarios.
- Replace fixed startup enemy presentation in ordinary play with spawn/activation/death event consumers.
- Connect exact reward offers, field pickups, restart cleanup, debug state, and evidence captures.
- Preserve existing semantic harness tests; add a full headless Wave 1–3 journey and client mirror tests.

## Validation fixture

`fixture.fp_normal_wave_journey` must prove on seed `B311A501`:

- exact Wave 1/2/3 rosters, authored positions, counts `4/6/6`, and budgets `4/10/15`;
- unique stable IDs for duplicate kinds and distinct run ordinals;
- zero hurtboxes/attacks for the 27-tick telegraph and activation on the exact boundary;
- real combat deaths map once to sorted encounter defeat IDs;
- last death clears all projectiles/lanes, followed by exactly 45 hostile-free reward ticks;
- deterministic reward offers and eight-tick normal drops do not reroll or duplicate;
- death/restart leaves zero old simulation or presentation entities; and
- two identical fixed-input journeys emit identical events and gameplay hashes.
