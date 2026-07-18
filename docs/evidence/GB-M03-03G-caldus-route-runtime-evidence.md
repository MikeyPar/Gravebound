# GB-M03-03G route-bound Sir Caldus runtime evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md` `SIM-004`, `DNG-006`, `ENC-005`, `ENC-010`, `TECH-012`, and `TECH-015` require continuous server-owned 30 Hz danger time, the exact boss lock, a five-second countdown, no late scaling, exact introduction timing, deterministic combat, and fail-closed ownership.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-002`, `CONT-ROOM-007`, and `CONT-BOSS-001`-`002` fix the B6 arena, stage/loading contract, 150-tick countdown, 75-tick Caldus introduction, participant scaling, distinct `0.70/0.62` collision/hurtbox radii, and exact scheduler.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires the fixed dungeon and Sir Caldus to compose into the ordinary private loop without developer authority, while reconnect, terminal, cleanup, and promotion gates remain fail closed until evidence passes.

## Delivered contract

Commits `8efcec0`, `b61cbce`, `4eba061`, `c433cbd`, and `f7f7fbb` add the first route-bound B6 owner and its distinct physical-body collision path.

- The route actor accepts atomic, versioned Caldus countdown, introduction, combat, break, defeat, exit-ready, and pre-defeat reset projections. Stale versions fail before local state commits; exact same-position replay is read-only.
- Sir Caldus uses stable run-qualified entity ID offset `40_002`, disjoint from player, projectile, normal-enemy, and Bell Proctor namespaces. A consuming reset API preserves the monotonic hostile-projectile allocator without retaining abandoned encounter authority.
- A reset interrupted during introduction resumes every remaining introduction tick instead of skipping to combat. Legal lethal damage may commit from every active phase and break, including a first-active-tick kill.
- The immutable fixed-route plan now carries the exact compiled B6 arena. `CorePrivateCaldusRuntime` relocates the moved player only after consuming B5 authority and owns B6 movement from that point onward.
- Every loading, countdown, and introduction frame advances the carried player combat/movement tick. On the combat-start tick, the runtime creates and steps the Caldus encounter at that same inherited tick, then commits its projected route phase by compare-and-swap.
- A typed body-collision world keeps Caldus's authored `0.70` physical radius separate from the `0.62` projectile hurtbox. Walking and forced Slipstep stop at the combined player/body radius, exact boundary departure remains legal, and the route runtime consumes the live encounter body snapshot.
- The normal route, reward, pending-inventory, stable exit, and presentation registrations remain disabled.

## Verification

Local Windows verification through exact source `f7f7fbb`:

- Exact inherited lifecycle: first B6 frame enters the visible countdown; tick `start + 150` commits the lock/introduction; tick `start + 225` creates and steps Phase 1 without tick rewind.
- Stale route mutation rejects the next runtime frame without advancing local tick or player combat.
- Introduction-reset cancellation retains the exact remaining ticks; early-phase and break defeat transitions are route-legal.
- Full server library: `338 passed`, `0 failed`.
- Focused route-bound Caldus runtime: `2 passed`, `0 failed`.
- Focused boss-lock simulation: `9 passed`, `0 failed`.
- Full simulation library after body-collision integration: `396 passed`, `0 failed`.
- Full server library after body-collision integration: `338 passed`, `0 failed`.
- Strict `sim_core`/`server_app` all-target, all-feature Clippy: pass.
- `cargo fmt --all` and `git diff --check`: pass.

## Explicit boundary

This slice now uses the compiled arena shell/pillars plus the exact `0.70` Caldus body for player walking and forced Slipstep while retaining the exact `0.62` hurtbox for friendly projectile damage. It does not yet claim deterministic player separation after a moving charge, the complete full-fight route trace, durable victory/reward, pending inventory, stable exit, driver/session composition, or normal admission.

## Current Next Step

Implement the approved deterministic post-charge player de-overlap rule, including shell/pillar legality and rollback when no placement exists. Then execute deterministic full-fight and adverse route-CAS traces through `BossDefeated` before composing the already-existing durable victory, pending-inventory, and stable-exit authorities.
