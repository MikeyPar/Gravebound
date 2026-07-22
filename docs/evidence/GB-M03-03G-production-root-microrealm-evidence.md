# GB-M03-03G Production-Root Microrealm Evidence

**Status:** SOURCE COMPLETE; HOSTED EXECUTION PENDING

## Authorities

This evidence reads the three design authorities together:

1. `Gravebound_Production_GDD_v1_Canonical.md` — authoritative QUIC transport, explicit loading transitions, measured transfer behavior, and the M03 complete private-life quality gate.
2. `Gravebound_Content_Production_Spec_v1.md` — the exact Core Hall, Realm Gate, microrealm, stable IDs, and content revision.
3. `Gravebound_Development_Roadmap_v1.md` — `GB-M03-03` Character Select → Hall → microrealm → dungeon → boss → Hall and the no-developer-command exit gate.

## Production boundary exercised

`crates/server_app/tests/core_private_life_journey.rs` constructs `BoundCorePrivateLifeServer`, connects with real QUIC, and uses only public protocol messages and ordinary input datagrams. The journey:

1. negotiates the production handshake and normal-route capabilities;
2. bootstraps a wipeable account, creates and selects a character, and transfers to Lantern Hall;
3. proves the native client model accepts control from the durable Hall snapshot;
4. walks the authoritative Hall route around the authored central obstruction;
5. opens the in-range Realm Gate through `HallInteractionFrameV1`;
6. submits the public `UsePortal` world-flow mutation;
7. verifies the committed danger location, nonzero lineage and restore-point authority;
8. matches the live Core-microrealm route and gameplay snapshot by tick and state version;
9. proves the native client model accepts microrealm control; and
10. records the exact schema-70 session/onboarding/first-combat/CleanExit sequence under explicit test attribution; and
11. shuts down with exactly one combat admission, no worker/connection residue, no open telemetry session, and no retained disposable roots after cleanup.

Every transport and authority transition is bounded. Hall traversal allows 15 seconds per authored waypoint; handshake, reliable mutations, route publication, and matching snapshot waits allow 10 seconds each.

## Local production-blocking checks

- Focused test-target compilation: PASS.
- Rust formatting and scoped diff validation: PASS.
- Strict target lint: PASS for the production-root journey target with warnings denied.
- Disposable PostgreSQL plus real-QUIC execution: pending hosted CI because no local `TEST_DATABASE_URL` is configured. The hosted command alone opts into `environment=test` and `region=local-playtest`; all other commands retain disabled-by-default telemetry.

## Claim boundary

This slice proves the source path through controllable microrealm entry. It does not yet prove B0–B6 combat, Caldus, extraction, death/successor, reconnect/restart repetition, the 25-journey matrix, current login timing, or optimized Realm Gate capture. Those remain the Current Next Step and no `GB-M03-03` or M03 closure credit is awarded here.
