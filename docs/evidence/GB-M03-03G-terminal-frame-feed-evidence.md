# GB-M03-03G terminal-frame feed evidence

## Authority

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-004`, `SIM-010`, `COM-002`, `DTH-001`, `DTH-010`, and `TECH-015` require server-authored combat, an exact ordered death history, lethal-first terminal resolution, and disconnect-safe authority.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-010`, `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001` define the Core micro-realm, fixed-room, and Sir Caldus simulation authority whose frames cross this boundary.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-06`, `GB-M03-08`, and `GB-M03-13` require the ordinary private route, atomic death, extraction/Recall arbitration, and durable Echo projection without duplicate terminal state.

## Implemented boundary

Commit `68830ad` adds one capacity-one, single-writer terminal feed across the micro-realm, fixed dungeon, and Sir Caldus scene loops. Every bound simulation frame, including an empty-damage frame, carries an immutable delivery view with its sequence, tick, exact route projection, player position, ordered damage facts, and lethal state. The driver waits for an exact acknowledgement before publishing the tick, advancing its public committed-frame count, updating presentation, or simulating another frame.

The feed is constructed from one opaque terminal/route/content/restore-point binding. It rejects foreign character, lineage, route generation, content, scene, tick, route-version, damage-order, health-continuity, or lethal authority. Repeated route versions are accepted only when the entire projection is unchanged. A lethal acknowledgement requires a validated stored `LethalDeath` receipt for the exact terminal binding and observed tick; a bare terminal-kind claim cannot release the driver.

Lethal ingress closes immediately after the committed lethal frame and before terminal resolution. Feed loss, altered acknowledgement, or owner loss faults terminal authority without publishing the committed tick. The task report still identifies that committed-but-unacknowledged frame so restart logic can distinguish it from a frame that never committed. Shutdown while a delivery is queued or received but unacknowledged returns an explicit unresolved-frame fault and cannot deadlock.

This evidence closes the reusable frame-feed contract only. The persistent life session still uses the explicitly named terminal-ownerless component path while normal admission is disabled. That path permits empty nonlethal frames and fails closed on the first damage-bearing or lethal frame. No production-route or durable-death integration claim is made until the real terminal owner and the total-ordered control-event stream are installed.

## Verification

- `cargo test -p server_app --lib`: `372 passed; 0 failed`.
- `cargo clippy -p server_app --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- Focused paused-time coverage proves acknowledgement-before-publication, foreign-generation rejection before delivery, feed-loss faulting without public tick advance, lethal receipt ownership, undrained-delivery shutdown, and received-but-unacknowledged shutdown.

Hosted proof for commits `e9a7e16` and `68830ad` remains pending and is not claimed by this local record.

## Current Next Step

Add one total-ordered acknowledged authority stream for the route/control transitions that occur between simulation frames: Bell handoff, B0 entry, B3 result, B4 resolution, B6 activation, Caldus reward, and exit readiness. Then construct the feed from the exact danger-entry restore authority and replace the persistent session's terminal-ownerless spawn with the real owner that joins the live damage trace, clocks, deeds, custody, and all five terminal producers. Keep normal admission disabled until three-scene/restart/adverse coverage proves that complete owner graph and zero-residue shutdown.
