# GB-M03-03G live authoritative tick evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-004`, `DTH-010`, `TECH-012`, and `TECH-015` require one fixed 30 Hz server clock, exact 12-tick Emergency Recall, lethal-first completion, and continuous `LinkLost` authority.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-010` and `CONT-WORLD-001` fix duration conversion and the closed Core microrealm lifecycle; Hall control cannot be inferred from a transport or wall clock.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-08`, and the M03 exit gates require the ordinary private route, production Recall/extraction, reconnect continuity, retry safety, and zero duplicate terminal state.

## Implemented contract

Commit `467b313` adds one process-owned `CorePrivateLifeTickDirectory`. A binding contains the exact account, selected character, private-route actor generation, microrealm binding generation, and exclusive driver handle. Recall and extraction retain the actual `CorePrivateRouteActorLease`; account/character-only sampling is no longer sufficient.

The exclusive microrealm/fixed-dungeon/Caldus task publishes its atomic tick only after a simulation frame and route mutation succeed. Scheduled deadlines, failed frames, failed route compare-and-swap, and the pre-frame `Tick(0)` state never become gameplay time. Frozen lethal, reward-pending, and exit-ready states retain their final successful tick while the terminal owner resolves.

Recall attachment, `LinkLost`, explicit intent, and extraction command dequeue all sample this same route-bound source. Missing first-frame authority and stale or foreign generations fail closed. A rejected Recall keeps the transport-provided tick only as response-envelope metadata; it never passes that fallback into gameplay mutation. Exact unbind removes tick authority only after the driver joins, while stale unbind cannot remove a replacement.

## Local verification

- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p server_app --all-targets --all-features -- -D warnings`: pass.
- `cargo test -p server_app --lib`: `364/364` pass.
- `live_driver_tick_is_route_bound_and_gates_recall_until_first_commit`: pass; proves frame-one gating, common Recall/extraction source, stale-generation and foreign-account rejection, exact intent tick, unbind, and zero residue.
- `missing_authoritative_tick_rejects_without_mutating_recall_state`: pass; proves `SourceUnavailable` with fallback envelope metadata, then a later authoritative tick starts the untouched channel.
- `route_fault_is_fail_closed_and_shutdown_finishes_an_in_flight_frame`: pass; proves a failed first frame leaves no published tick and a successful in-flight frame publishes only after commit.
- Existing real-QUIC Recall and private-session tests now require an actual private-route lease; the PostgreSQL `LinkLost`/lethal matrix also drains that route actor explicitly.

Hosted verification for the cumulative source is pending. Run [`29667827330`](https://github.com/MikeyPar/Gravebound/actions/runs/29667827330) predates this commit; it proves Windows release construction and the repaired B3/Caldus PostgreSQL boundaries, then exposes the independent lifecycle fixture collision fixed by `3da08c5`.

## Current Next Step

Obtain a fully green mandatory hosted run for `467b313`, `3da08c5`, and this evidence update. Then bind this source inside the all-or-nothing private-life authority builder and compose live lethal death, all five terminal producers, danger activation, and ordinary request/snapshot dispatch. Normal capability advertisement remains disabled until the complete owner graph and shutdown order pass together.
