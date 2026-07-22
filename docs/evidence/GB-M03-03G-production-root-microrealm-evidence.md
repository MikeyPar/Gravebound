# GB-M03-03G Production-Root Bell Route Evidence

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
9. clears the exact Bell wave with bounded ordinary movement, aim, and primary-fire input;
10. walks the authored Lantern Fork/east-bend road to the live Bell portal radius;
11. commits public `UsePortal portal.dungeon.bell_sepulcher` without changing danger lineage or restore authority;
12. proves the safe B0 vestibule contains one player and no hostile, then enters B1 with public `ActionFrame::Interact`;
13. matches the active B1 authority and exact eight-enemy/no-boss snapshot;
14. records the exact schema-70 session/onboarding/first-combat/CleanExit sequence under explicit test attribution; and
15. shuts down with exactly one combat admission, no worker/connection residue, no open telemetry session, and no retained disposable roots after cleanup.

Every transport and authority transition is bounded. Hall traversal allows 15 seconds per authored waypoint; handshake, reliable mutations, route publication, and matching snapshot waits allow 10 seconds each.

## Local production-blocking checks

- Focused test-target compilation: PASS.
- Rust formatting and scoped diff validation: PASS.
- Strict target lint: PASS for the production-root journey target with warnings denied.
- Hosted run [`29900131501`](https://github.com/MikeyPar/Gravebound/actions/runs/29900131501) at source `fbb0c01` passed the schema-70 PostgreSQL source journey and Windows release construction. Its integrated production-root journey then failed before transport admission because dormant-composition validation expected two `runtime_bootstrap` owners while the required production graph correctly retained three: the foundation, terminal reconciler, and world-flow coordinator.
- The composition invariant now requires those exact three owners. Focused target compilation, formatting, diff validation, and strict target lint pass with the route extended through B1. Disposable PostgreSQL plus real-QUIC execution of that correction remains pending hosted CI because no local `TEST_DATABASE_URL` is configured.

## Claim boundary

This slice proves the source path through active B1, not a completed dungeon. It does not yet prove B1 clear through B6, Caldus, extraction, death/successor, reconnect/restart repetition, the 25-journey matrix, current login timing, or optimized Realm Gate capture. The Current Next Step is the hosted corrected B1 journey, followed by extending the same public-protocol driver through B1-B6/Caldus and both terminal branches. No `GB-M03-03` or M03 closure credit is awarded here.
