# GB-M03-03G terminal tick-context evidence

## Authority

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-004`, `SIM-010`, `DTH-001`, `DTH-010`, and `TECH-015` require one fixed-tick server authority to retain health, status, Recall, reconnect, and lethal ordering.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001` define the exact Core micro-realm, Bell dungeon, and Sir Caldus scenes whose frames feed terminal history.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-06`, and `GB-M03-08` require the normal private route, atomic death, extraction, and Emergency Recall to resolve from one authoritative event order.

## Implemented boundary

Commit `e869f01` extends every lossless micro-realm, fixed-dungeon, and Caldus frame with `CorePrivateTerminalTickContextV1`. Network, Recall, and bounded status context are sampled from the same retained pre-simulation input as the committed frame; terminal consumers cannot reconstruct them later from transport or presentation state.

`LinkLost` is recorded before subsequent danger simulation. A winning authenticated reattach produces exactly one acknowledged `Reattached` frame, then returns to `Connected` only if no newer `LinkLost` transition won while acknowledgement was pending. The transition therefore cannot disappear behind coalescing or race a replacement transport.

Status entries use the death-authority stable-ID, count, duration, capacity, ordering, and uniqueness bounds before delivery. The promoted Core runtime currently has no player-status authority, so production frames deliberately carry an empty status set. Recall remains `Inactive` until the existing Recall actor is connected through a server-only terminal-owner proxy; no transport-facing setter or guessed phase was added.

The frame variant is boxed after adding context so the mixed frame/control delivery enum remains compact. The old context-free delivery helper is test-only; production callers must provide the exact tick context.

## Production-blocking verification

Per the owner's instruction, broad workspace/audit suites remain deferred until GB-M03 implementation is complete. These changed-contract gates passed:

- `cargo test -p server_app terminal_context_records_link_loss_and_one_committed_reattach_frame --lib`: passed.
- `cargo clippy -p server_app --lib -- -D warnings`: passed.
- `cargo fmt --all` and `git diff --check`: passed.

## Current Next Step

Build `PostgresPrivateDeathContextPlanner` and its server adapter. Under one account/character authority, load the exact life-clock head, destructive custody, immutable character/deed/Oath/Bargain/content facts, Echo availability, and current trace into one server-authored durable-death context. Then connect the Recall actor's ordered phase through the terminal owner, feed nonlethal ticks into trace and clock persistence, and acknowledge a lethal frame only with the exact stored death receipt. Keep normal admission disabled.
