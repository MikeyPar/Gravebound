# GB-M03-03G route-bound Sir Caldus runtime evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md` `SIM-004`, `DNG-006`, `ENC-005`, `ENC-010`, `TECH-012`, and `TECH-015` require continuous server-owned 30 Hz danger time, the exact boss lock, a five-second countdown, no late scaling, exact introduction timing, deterministic combat, and fail-closed ownership.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-002`, `CONT-ROOM-007`, and `CONT-BOSS-001`-`002` fix the B6 arena, stage/loading contract, 150-tick countdown, 75-tick Caldus introduction, participant scaling, distinct `0.70/0.62` collision/hurtbox radii, and exact scheduler.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires the fixed dungeon and Sir Caldus to compose into the ordinary private loop without developer authority, while reconnect, terminal, cleanup, and promotion gates remain fail closed until evidence passes.

## Delivered contract

Commits `8efcec0`, `b61cbce`, `4eba061`, `c433cbd`, `f7f7fbb`, `a9a17e0`, and `c0af19d` add the route-bound B6 owner, its complete physical-body collision path, and a deterministic full-fight trace.

- The route actor accepts atomic, versioned Caldus countdown, introduction, combat, break, defeat, exit-ready, and pre-defeat reset projections. Stale versions fail before local state commits; exact same-position replay is read-only.
- Sir Caldus uses stable run-qualified entity ID offset `40_002`, disjoint from player, projectile, normal-enemy, and Bell Proctor namespaces. A consuming reset API preserves the monotonic hostile-projectile allocator without retaining abandoned encounter authority.
- A reset interrupted during introduction resumes every remaining introduction tick instead of skipping to combat. Legal lethal damage may commit from every active phase and break, including a first-active-tick kill.
- The immutable fixed-route plan now carries the exact compiled B6 arena. `CorePrivateCaldusRuntime` relocates the moved player only after consuming B5 authority and owns B6 movement from that point onward.
- Every loading, countdown, and introduction frame advances the carried player combat/movement tick. On the combat-start tick, the runtime creates and steps the Caldus encounter at that same inherited tick, then commits its projected route phase by compare-and-swap.
- A typed body-collision world keeps Caldus's authored `0.70` physical radius separate from the `0.62` projectile hurtbox. Walking and forced Slipstep stop at the combined player/body radius, exact boundary departure remains legal, and the route runtime consumes the live encounter body snapshot.
- Every charge segment resolves living locked participants by immutable party slot and entity ID to the shortest legal combined `1.00` body radius. Blocked radial placement uses the approved reverse-axis/clockwise cardinal fallback; absence of a legal placement rolls the staged frame back. The server synchronizes the route-bound movement owner to the committed separation and clears stale inward velocity.
- A test-only authoritative damage driver exercises the production-private owner without adding protocol, ingress, showcase, or developer-command authority. Two complete runs produce the same BLAKE3 trace through Phase 1, both 120-tick breaks, the Phase 2 charge and separation, Phase 3, lethal defeat, atomic `BossDefeated` route projection, and terminal hostile cleanup.
- Reset evidence now emits a real hostile projectile set before consuming the encounter, proves the recovered allocator has advanced, creates a second attempt, and proves no prior hostile survives or reuses identity.
- The normal route, reward, pending-inventory, stable exit, and presentation registrations remain disabled.

## Verification

Local Windows verification through exact source `c0af19d`:

- Exact inherited lifecycle: first B6 frame enters the visible countdown; tick `start + 150` commits the lock/introduction; tick `start + 225` creates and steps Phase 1 without tick rewind.
- Stale route mutation rejects the next runtime frame without advancing local tick or player combat.
- Introduction-reset cancellation retains the exact remaining ticks; early-phase and break defeat transitions are route-legal.
- Full server library: `338 passed`, `0 failed`.
- Focused route-bound Caldus runtime: `2 passed`, `0 failed`.
- Focused boss-lock simulation: `9 passed`, `0 failed`.
- Full simulation library after body-collision and separation integration: `399 passed`, `0 failed`.
- Full server library after full-fight integration: `339 passed`, `0 failed`.
- Strict `sim_core`/`server_app` all-target, all-feature Clippy: pass.
- `cargo fmt --all` and `git diff --check`: pass.

## Explicit boundary

This slice now uses the compiled arena shell/pillars plus the exact `0.70` Caldus body for player walking, forced Slipstep, and deterministic moving-charge separation while retaining the exact `0.62` hurtbox for friendly projectile damage. It closes the local route-bound fight through defeat, but does not yet claim durable victory/reward, pending inventory, stable exit, driver/session composition, ordinary no-test-driver play, or normal admission.

## Current Next Step

Commit `78b7ff0` closes the frozen defeat-to-stable-exit owner seam, and commits `f4ad323`, `e5f7dc8`, `1bd230a`, and `47ad6c3` add automatic durable execution plus the coherent custody, protocol, and reconnect-binding prerequisites; see [extraction-prerequisite evidence](GB-M03-03G-caldus-extraction-prerequisites-evidence.md). Next publish/retain the snapshot and construct/register the exact `BossExitReady` production extraction actor. Keep normal admission disabled until that integration and hosted adverse proof pass.
