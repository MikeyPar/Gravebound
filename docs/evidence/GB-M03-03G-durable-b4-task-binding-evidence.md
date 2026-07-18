# GB-M03-03G durable B4 task-binding evidence

**Status:** Local implementation evidence accepted for commit `e1e4d7c`. Hosted CI run [`29648007340`](https://github.com/MikeyPar/Gravebound/actions/runs/29648007340) is in progress and is not claimed green. Normal route admission remains disabled.

## Three-authority basis

1. `Gravebound_Production_GDD_v1_Canonical.md` `BRG-001`-`005`, `DNG-003`, `DNG-005`, `TECH-012`, and `TECH-015` require optional select/refuse Bargains, life-persistent accepted state, exact server-owned room authority, replay safety, and reconnect to the same live allocation.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-014` requires one immutable offer and one transactional `Open -> Selected(candidate_id) | Refused` transition; `CONT-ROOM-007` fixes B4 before the B5 bridge and Sir Caldus at B6.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` and `GB-M03-05` require the three-choice shrine to compose inside the closed private route without exposing client-authored state or skipping the route graph.

## Implemented contract

- Persistence derives `StoredBargainRestBinding` by reading the committed/replayed decision receipt first, then validating the immutable source offer's selected/refused state, resolved life version, character, offer, lineage, and restore root.
- `CoreDurableBargainRestResolution` keeps account, character, dangerous-instance lineage, restore root, source receipt, offer, Oath/Bargain version, and selected/refused/no-offer outcome private. The normal transport cannot author a room result, candidate, destination, aggregate version, or lineage.
- Accepted selections map only the three exact Core Bargains into their compiled combat kinds. Refusal remains penalty-free. An authoritative committed Core milestone result can project only the two legal no-offer dispositions.
- The persistent Bargain service can return its exact reliable response plus the opaque B4 proof. Binding failure never fabricates authority; the caller receives no proof and the route stays frozen for replay/reconciliation.
- The private-life session validates the current transport generation and account before forwarding the proof. The original session-owned driver validates character, B4 node, and exact instance lineage again.
- First application stages the B4 combat owner and validates the current route state under the route actor. Exact retry returns `Replayed` without route-version churn. Foreign lineage, changed local outcome, wrong node, stale transport, or unresolved authority cannot advance B4.
- The ordinary destination-free advance remains separate. Only after the durable result is installed can the task select canonical `B4 -> B5`, carrying the exact stored resolution in the transition.

## Verification

- Five focused Bargain authority tests cover exact comparisons, select/refuse persistence, stored selected-result projection, altered/error-result rejection, and the exact Core no-offer milestone contract.
- The fixed-dungeon lifecycle test traverses B1/B2/B3 to B4, rejects a foreign-lineage proof without route or local mutation, commits the exact no-offer proof, replays it without version churn, and enters only B5 with the stored resolution.
- The driver test proves both advance and B4 resolution are bounded typed rejections before Bell conversion; neither request blocks or creates a second owner.
- `242/242` persistence library tests pass.
- `330/330` server library tests pass, including the existing session, real-QUIC, route actor, terminal, reward, and shutdown matrix.
- `cargo fmt --all -- --check`, `git diff --check`, and strict all-target/all-feature Clippy for `persistence` and `server_app` pass.

## B6 presentation candidate

Commit `ac31961` adds the unregistered [`DefeatedRewardUnresolved` / `RewardCommittedAtRisk` Caldus status pack](../../assets/core/ui/caldus_resolution_states/v1/README.md). Its deterministic builder reproduces eleven manifest hashes; 48px/96px readability and 1280x720/1920x1080 standard/reduced review mocks pass independent inspection. `CONT-ROOM-007` is explicit: B5 remains `room.bell.bridge_01`; this pack is B6-only and remains outside runtime/content hashes.

## Explicit boundary

Commit `83ccbb1` now supplies the durable B3 reward/progression/milestone coordinator and the task-owned pending/acknowledgement seam. Automatic normal-server coordinator execution and reliable response publication are still disabled, so response-loss/reconnect/process-restart convergence remains open. Sir Caldus combat, personal rewards, pending inventory, stable exit, native presentation registration, all terminal producers, and normal admission also remain open.

## Current Next Step

Build the transport-independent B3 executor, apply its opaque durable `Granted | Ineligible` proof to the same task, and retain progression/route publication across writer generations only after acknowledgement. Prove response-loss/reconnect/process-restart convergence and integrated inactivity zero-row behavior, then implement B5 and Sir Caldus at B6.
