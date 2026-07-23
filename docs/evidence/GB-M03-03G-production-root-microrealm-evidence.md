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
14. clears B1, B2, B3, and B5 with bounded movement, aim, primary fire, Grave Mark, and Slipstep inputs while retaining the same character and danger lineage;
15. waits for the committed B3 reward before entering B4 and proves the ordinary low-level life receives the authoritative `NoOffer` Bargain projection;
16. advances through the authored B5 bridge into B6 without a direct route-state write;
17. survives the ordinary Sir Caldus encounter, waits for the durable personal reward and pending-inventory handoff, and reaches the stable `BossExitReady` projection;
18. proves both boss and extraction readiness are available only after that committed handoff;
19. retains the server-initiated extraction-ready and pending-inventory authorities and verifies the ordinary Caldus reward exercises real pending-item placement;
20. submits `ExtractionCommitFrameV1` using only the server-issued extraction identity, content revision, and aggregate versions, accepting either the typed immediate `Stored` result or the typed `Pending` result followed by its durable completion;
21. validates the stored extraction receipt, exact before-versions, Hall destination, and server-planned placement coverage without allowing the client to author a destination;
22. invokes `UseCommittedExtraction` with that exact receipt and proves the durable Hall snapshot plus a controllable Hall route with no retained danger lineage;
23. walks the extracted character through the authored Hall geometry and Realm Gate a second time, committing a fresh danger lineage and restore point through public world flow;
24. uses only ordinary movement input and server snapshots to approach the nearest live hostile with every attack and ability released, then waits for the server-owned encounter to deal lethal damage and publish `TerminalPending`;
25. polls the authenticated read-only death-view capability until the matching immutable committed death is durably acknowledged, then validates its summary, newest-first Memorial entry, and bounded combat-trace page;
26. proves the summary exposes the destroyed starter equipment and remaining Belt units, the fixed preserved/created projections, the lethal final damage event, and the expected ineligible Echo result for this sub-level-10 life;
27. creates a successor by echoing only the server-issued death ID and the Core item-content revision, validating the server-planned class, selection, roster authority, versions, and four fresh distinct starter identities;
28. enters Lantern Halls through public world flow and proves the negotiated native route model reaches controllable Hall state within the `DTH-021` 15-second target from terminal publication;
29. walks the successor through the real Hall and Realm Gate, commits a third independent danger root, and proves matching native route/snapshot readiness for controllable permadeath-enabled combat;
30. records the exact committed session/onboarding/first-combat/CleanExit telemetry sources under explicit test attribution while accepting independently versioned loot-sidecar additions; and
31. shuts down with exactly one combat admission, no worker/connection residue, no open telemetry session, and no retained disposable roots after cleanup.

Every transport and authority transition is bounded. Hall traversal allows 15 seconds per authored waypoint; handshake, reliable mutations, route publication, and matching snapshot waits allow 10 seconds each.

## Local production-blocking checks

- Focused test-target compilation: PASS.
- Rust formatting and scoped diff validation: PASS.
- Strict target lint: PASS for the production-root journey target with warnings denied.
- Hosted run [`29900131501`](https://github.com/MikeyPar/Gravebound/actions/runs/29900131501) at source `fbb0c01` passed the schema-70 PostgreSQL source journey and Windows release construction. Its integrated production-root journey then failed before transport admission because dormant-composition validation expected two `runtime_bootstrap` owners while the required production graph correctly retained three: the foundation, terminal reconciler, and world-flow coordinator.
- The composition invariant now requires those exact three owners. Focused target compilation, formatting, diff validation, and strict target lint pass with the route extended through the stable post-Caldus exit, durable extraction, receipt-bound controllable Hall return, ordinary lethal damage, committed death views, successor creation, and checked Hall/danger control. Disposable PostgreSQL plus real-QUIC execution of that correction and complete source path remains pending hosted CI because no local `TEST_DATABASE_URL` is configured.

## Claim boundary

This slice proves the source path through the complete fixed dungeon, durable ordinary extraction, ordinary lethal death, durable summary/Memorial/trace reads, successor creation, and checked Hall/danger control. Hosted run [`30050712798`](https://github.com/MikeyPar/Gravebound/actions/runs/30050712798) passed the focused schema-79 PostgreSQL test, including damage before TECH-023's first optional checkpoint, and then received committed Bell B0 route authority without a matching snapshot. Inspection proved the runtime retained the micro-realm portal position, B0 had no combat scheduler frame, and `FixedDungeonReady` was excluded from snapshot publication. Commit `79668fe` relocates the same participant to the sole compiled B0 `SafeEntry` anchor `(3,5.5)`, publishes a forced route-version-bound snapshot on the already committed transfer tick, and preserves the next combat tick. The journey now accepts only the exact B0 tick, route state version, spawn, and zero-hostile projection. Optimized tester r32 remains the latest packaged runtime at source `525b423`, protocol 1.25, and schema 78 until the active source is repackaged. Under `Gravebound_Production_GDD_v1_Canonical.md`, `Gravebound_Content_Production_Spec_v1.md`, and `Gravebound_Development_Roadmap_v1.md`, the Current Next Step is active hosted complete-route run [`30052832985`](https://github.com/MikeyPar/Gravebound/actions/runs/30052832985), followed by a clean schema-79 tester release, the 25-complete-loop matrix, current aggregate timing, and optimized Realm Gate capture. No `GB-M03-03` or M03 closure credit is awarded until those whole gates pass.
