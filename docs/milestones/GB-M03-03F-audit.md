# GB-M03-03F completion audit

**Status:** PASS on main commit `915d075`; hosted CI [`29330704486`](https://github.com/MikeyPar/Gravebound/actions/runs/29330704486) is green.

## Three-authority closure

| Authority | Closure |
|---|---|
| `Gravebound_Production_GDD_v1_Canonical.md` | `UI-002` transition states, `UI-003` protected aiming corridor, `TECH-010`-`015` reliable control/reconnect authority, `TECH-070` frame/memory budgets, and `QA-001` layered evidence are implemented without client-predicted death, extraction, or rewards. |
| `Gravebound_Content_Production_Spec_v1.md` | The strict unpromoted Core records, assets, localization, fixed route, authored arrivals, and disabled branches remain unchanged and independently hashed. |
| `Gravebound_Development_Roadmap_v1.md` | `GB-M03-03` now has the disposable Character Select -> Hall -> private life -> Caldus -> Hall transport proof, 25 scripted loops, restart/idempotency coverage, and login-to-control enforcement while the parent remains gated by its later dependencies. |

Approved [`SPEC-CONFLICT-025`](../spec-conflicts/SPEC-CONFLICT-025-m03-native-transition-and-reconnect-ux.md) supplied only the missing `03F` projection, copy, disposable-transport, adverse-matrix, and evidence contract. It did not promote Core content or broaden gameplay ownership.

## Acceptance evidence

| Criterion | Evidence | Result |
|---|---|---|
| Closed deterministic projection | Seven `core_world_transition` tests cover exact readiness, all transfer/handshake rejections, identical retry identity, stale sequencing, LinkLost nonterminality, and irreversible committed resolutions. | PASS |
| Native loading/error/reconnect UX | Three adapter contract tests plus the optimized [33-frame visual matrix](../evidence/GB-M03-03F-visual-manifest.md) cover eight states, both effects modes, 1280x720, 1920x1080, and 2560x1080 ultrawide. | PASS |
| Strict localized copy | The Core world-flow compiler owns 29 typed phase/action/status/handshake keys and all 20 transfer-result messages; strict ordering and independent localization hash reject substitutions. | PASS |
| Complete real-QUIC route | The guarded `core_route_quic` baseline uses production framing, atomic dormant entry, production Caldus reward/progression victory, derived extraction binding, committed `HallDefault` return, and exact replays. | PASS |
| Adverse recovery | The [adverse matrix](../evidence/GB-M03-03F-adverse-matrix.md) maps response loss, stale/duplicate requests, disconnect/reconnect, 89/90 timing, death precedence, duplicate session, mismatch, allocation rollback, restart, corruption, and normal-route negatives to their closest authority. | PASS |
| 25 scripted full loops | Hosted PostgreSQL runs 25 serial real-QUIC loops in the wipeable namespace. Login-to-control median is 25.620 ms, p95 29.874 ms, and maximum 37.903 ms; all 25 are below 30 seconds. | PASS |
| Optimized performance and soak | [30-minute report](../performance/GB-M03-03F-transition-30m.json): 650,563 rendered frames, 361.482 FPS, p95 8.423 ms, p99 11.420 ms, 338,255,872-byte peak RSS, 359 state rebuilds, memory `pass`, report `accepted: true`. | PASS |
| Immutable evidence identity | Build `release-6d3968b5797c37ea11cec754cd16d13eb1c658ee487438652ff0eef1b4f623a4`; records `97b7188e…b773158`; assets `32ce9fce…2a3759`; localization `895c3872…763a26`; report BLAKE3 `63772d0a…4962da`; file SHA-256 `0F20D45E…9317E`. | PASS |
| Scope remains closed | Normal runtime does not advertise the disposable feature and returns `StageDisabled`; normal Character Select `Play`, Realm Gate admission, inventory conversion, seeded branches, Hall stations, production namespaces, and Core promotion remain unavailable. | PASS |
| Cumulative construction | Local format, focused/workspace tests, warnings-denied client/server Clippy, optimized Windows construction, strict content validation, deterministic traces, generated schemas, mandatory PostgreSQL, and hosted CI pass. | PASS |

## Performance host

- Windows 10 Home, 10.0.19045.
- Intel Core i7-10700K CPU @ 3.80 GHz.
- AMD Radeon RX 6700 XT, driver 32.0.21043.19003.
- 64 GiB installed RAM.
- 1920x1080, standard effects, optimized executable, no-vsync measurement.
- Five-second warmup, five-second state interval, ten-second RSS cadence, 30:00.003 measurement duration.

The recorded machine materially exceeds the minimum `TECH-070` reference target; that fact is stated rather than presenting these values as minimum-hardware certification.

## Cumulative verification

- Local: `cargo fmt --all -- --check`.
- Local: `cargo test --locked -p client_bevy core_transition_showcase -- --nocapture` (4 passed).
- Local: `cargo clippy --locked -p client_bevy --all-targets -- -D warnings`.
- Local: `cargo test --locked -p server_app --test core_route_quic --no-run`.
- Local: `cargo clippy --locked -p server_app --test core_route_quic -- -D warnings`.
- Local: `cargo build --locked --release -p client_bevy`.
- Hosted: main CI `29330704486` passed all quality, mandatory PostgreSQL, 25-journey, and Windows release jobs on commit `915d075`.

## Deferred ownership

`GB-M03-03F` does not close parent `GB-M03-03` or GB-M03. Production inventory/vault admission, pending-inventory stabilization, Overflow, ResolutionHold, Recall loss, full death/memorial/successor UX, parties/public allocation, seeded branches, telemetry export, Steam packaging, production namespace cutover, and Core promotion remain with their roadmap packages.

## Handoff

After the final hosted run passes, continue the parent roadmap audit and the next dependency-ready packages: `GB-M03-04E`-`04G` inventory/vault integration and the approved `SPEC-CONFLICT-009` death foundation. Keep the normal route and all later-owned surfaces fail closed.
