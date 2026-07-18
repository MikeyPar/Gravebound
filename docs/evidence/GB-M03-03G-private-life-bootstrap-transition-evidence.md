# GB-M03-03G private-life bootstrap and transition evidence

**Evidence status:** Local implementation gate PASS; hosted exact-source PostgreSQL/Windows gate pending the `a30e644` push.

## Authority

1. `Gravebound_Production_GDD_v1_Canonical.md`: `LOOP-001`-`003`, `DTH-010`-`011`, and `TECH-010`-`023` require server-owned route state, terminal precedence, fixed simulation authority, reconnect reuse, and danger crash restoration to Hall rather than danger resurrection.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001`, `CONT-ROOM-007`, `CONT-BOSS-001`-`002`, and `CONT-HUB-001`-`002` fix the capacity-one Hall -> Core microrealm -> Bell route and keep later stations/content closed.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-08`, and the M03 exit gate require the ordinary private route, durable restart/replay behavior, nonduplication, cleanup, and 25 no-developer-command journeys.

## Implemented contract

Commit `a30e644` adds one dormant persistent adapter between the terminal-first PostgreSQL bootstrap projection, the shared private-life session writer, the durable route-generation allocator, and the private-route actor directory.

- Process restart and within-process reconnect are distinct methods. Only process restart can invoke crash restoration; reconnect obtains the current writer from `CorePrivateLifeSessionDirectory` and reuses retained authority.
- Missing first-time accounts return `AwaitIdentityBootstrap`. Character Select creates no route actor. `HallStorageResolutionRequired` exposes only storage recovery and allocates no generation.
- `HallReady` allocates one durable monotonic route generation and seeds the exact compiled Hall authority. Exact refresh/reconnect reuses it. Stored extraction and Recall terminals remain ordered before their Hall projection; committed death creates no living route actor.
- A restart-observed danger root is atomically restored to Hall or loses to a committed terminal. Refresh cannot reconstruct danger from a checkpoint.
- Accepted and replayed non-Bell PostgreSQL transfers reconcile only after commit. Character Select -> Hall registers the durable Hall actor, Hall -> microrealm converges on the exact lineage/version, and Hall -> Character Select retires the exact generation.
- Reconciliation retains exact transition material. Exact response-loss replay is a no-op even after later microrealm progress; changed transfer, source version, lineage, content, or generation fails closed. A committed database transition is never rolled back because an in-memory callback failed.
- Bell admission remains on its existing two-phase actor permit/reconciliation path. Normal route and client capability admission remain disabled.

## Verification

- `cargo test -p server_app core_private_life_runtime_bootstrap --lib`: 5 passed.
- `cargo test -p server_app core_private_route_actor --lib`: 11 passed.
- `cargo test -p server_app --all-targets`: 293 library tests passed plus every server integration target; PostgreSQL tests remained explicitly ignored without the disposable database opt-in.
- `cargo clippy -p server_app --all-targets -- -D warnings`: passed.
- `cargo fmt --all` and `git diff --check`: passed.
- Prior exact source `e069987` is green under hosted CI [`29633952947`](https://github.com/MikeyPar/Gravebound/actions/runs/29633952947), including mandatory PostgreSQL, strict Linux gates, optimized Windows construction, and optimized native evidence.

## Remaining gate

Bind dynamic extraction and Recall terminal publication through the private-life session owner and its single `CoreReliableWriter`. Then compose live movement, combat, rewards, pending inventory, all terminal owners, ordinary QUIC admission, restart/response-loss coverage, 25 full journeys, and optimized native visual evidence before enabling Character Select `Play`, the production Realm Gate, or normal extraction/Recall capability flags.
