# GB-M03-03G terminal-frame feed evidence

## Authority

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-004`, `SIM-010`, `COM-002`, `DTH-001`, `DTH-010`, and `TECH-015` require server-authored combat, an exact ordered death history, lethal-first terminal resolution, and disconnect-safe authority.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-010`, `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001` define the Core micro-realm, fixed-room, and Sir Caldus simulation authority whose frames cross this boundary.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-06`, `GB-M03-08`, and `GB-M03-13` require the ordinary private route, atomic death, extraction/Recall arbitration, and durable Echo projection without duplicate terminal state.

## Implemented boundary

Commit `68830ad` adds one capacity-one, single-writer terminal feed across the micro-realm, fixed dungeon, and Sir Caldus scene loops. Every bound simulation frame, including an empty-damage frame, carries an immutable delivery view with its sequence, tick, exact route projection, player position, ordered damage facts, and lethal state. The driver waits for an exact acknowledgement before publishing the tick, advancing its public committed-frame count, updating presentation, or simulating another frame.

Commit `c9f8705` extends that same channel into one total-ordered private-route event stream. It delivers the exact opaque Bell commit, every B0-to-B6 room advance, fresh B3 durable reward, fresh B4 Bargain/no-offer resolution, and fresh Caldus reward/exit authority before any corresponding response, presentation, ingress resume, or next scene frame. Replays do not emit a second control event. Bell, room, and exit transitions require the exact next route version; B3 and B4 intentionally accept one exact byte-identical equal-version authority because those durable results do not move the route.

Control events inherit `simulation_tick` and never advance it. This value is not the lifetime or permadeath-combat clock: B0, B4, staging, reward waits, and `LinkLost` can continue authoritative life time without producing a simulation frame. The production terminal owner must run those two approved 30 Hz clocks independently and use the ordered controls only to change their eligibility and deed state.

The feed is constructed from one opaque terminal/route/content/restore-point binding. It rejects foreign character, lineage, route generation, content, scene, tick, route-version, damage-order, health-continuity, or lethal authority. Repeated route versions are accepted only when the entire projection is unchanged. A lethal acknowledgement requires a validated stored `LethalDeath` receipt for the exact terminal binding and observed tick; a bare terminal-kind claim cannot release the driver.

Lethal ingress closes immediately after the committed lethal frame and before terminal resolution. Feed loss, altered acknowledgement, or owner loss faults terminal authority without publishing the committed tick. The task report still identifies that committed-but-unacknowledged frame so restart logic can distinguish it from a frame that never committed. Shutdown while a delivery is queued or received but unacknowledged returns an explicit unresolved-frame fault and cannot deadlock.

This evidence closes the reusable frame-feed contract only. The persistent life session still uses the explicitly named terminal-ownerless component path while normal admission is disabled. That path permits empty nonlethal frames and fails closed on the first damage-bearing or lethal frame. No production-route or durable-death integration claim is made until the real terminal owner and the total-ordered control-event stream are installed.

## Verification

- `cargo test -p server_app --lib`: `374 passed; 0 failed`.
- `cargo clippy -p server_app --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- Focused coverage proves acknowledgement-before-publication, foreign-generation rejection before delivery, feed-loss faulting without public tick advance, lethal receipt ownership, undrained-delivery shutdown, received-but-unacknowledged shutdown, `frame N -> Bell/B0 control N -> B0/B1 control N -> B1 frame N+1`, and exactly one accepted equal-version B3 durable authority.

Hosted proof for commits `e9a7e16`, `68830ad`, and `c9f8705` remains pending and is not claimed by this local record.

## Current Next Step

Commit `2e02b94` now constructs the event stream binding from the exact [opaque danger-entry authority](GB-M03-03G-danger-entry-authority-evidence.md). Next replace the persistent session's terminal-ownerless spawn with a mandatory real owner. That owner must join the live damage trace, independent lifetime/permadeath clocks, deeds, custody, per-tick network/Recall/status providers, immutable simulation-to-journal entity identities, and all five terminal producers. Keep normal admission disabled until three-scene/restart/adverse coverage proves that complete owner graph and zero-residue shutdown.
