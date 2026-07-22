# GB-M03-09 Disabled Production Worker Evidence

## Authorities

1. `Gravebound_Production_GDD_v1_Canonical.md`: `TECH-005` places telemetry export inside the modular monolith, `TECH-123` requires pseudonymous privacy boundaries, and `TEL-001`-`005` define the typed event and measurement contract.
2. `Gravebound_Content_Production_Spec_v1.md`: Core stable IDs, content revision, and committed item/death/terminal outcomes remain the only content and lifecycle authority; the worker cannot invent or rewrite them.
3. `Gravebound_Development_Roadmap_v1.md`: roadmap `ADR-005` and `GB-M03-09` require bounded batching, privacy filtering, crash handling, and correct disabled/offline operation.

`ADR-039-telemetry-outbox-privacy-and-retention.md` further requires disabled mode to accept, retain, and export nothing; telemetry may never block gameplay; and no remote destination may be enabled before its processor and privacy controls are approved.

## Implemented production boundary

`BoundCorePrivateLifeServer` now owns one `CorePrivateTelemetryWorkerRuntime` for its complete bind/serve/shutdown lifecycle. The owner constructs the existing `telemetry::TelemetryWorker` in disabled mode with the ADR-039 maximum future queue bound, but deliberately has no committed source, exporter, exporter destination, pseudonymization secret, or background task.

The root exposes a typed bind-time status and includes a typed shutdown report in `CoreIdentityServerReport`. Native server logs report the same state at readiness and shutdown. The shutdown sequence marks telemetry stopping before gameplay/session retirement and destroys the disabled worker after connection and private-life process drain. The worker's zero-residue result is part of the server's aggregate `zero_residue` result.

The disabled report is exact:

- committed source attached: `false`
- exporter attached: `false`
- pseudonymization secret loaded: `false`
- source polls: `0`
- source acknowledgements: `0`
- exporter attempts: `0`
- queued events at shutdown: `0`
- spawned/remaining tasks: `0`
- shutdown complete and zero residue: `true`

Because source and exporter handles are structurally absent from the production disabled owner, database outage, collector outage, missing exporter configuration, or missing pseudonymization configuration cannot enter authentication, bootstrap, inventory, combat, or terminal gameplay paths.

## Focused production-blocking verification

The focused server module fixture passes trap source and exporter implementations directly to the owned disabled `TelemetryWorker`. `run_once` returns `Disabled`, and the trap records zero polls, zero acknowledgements, and zero exports. A second fixture verifies the exact lifecycle status and clean shutdown report.

Commands run from the repository root:

```text
cargo check --offline -p server_app
cargo test --locked -p server_app core_private_telemetry_worker --lib
cargo clippy --locked -p server_app --lib --bin server_app --no-deps -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## Deliberate limitation and next gate

This slice does not invent or enable a remote exporter, does not attach even a PostgreSQL adapter while disabled, and does not claim destination/offline/restart operational evidence. That is intentional: terminal-family rows still require immutable origin-session binding, and ADR-039's destination/privacy review is still open. After both gates pass, the approved committed adapters and exporter can be attached behind this single lifecycle owner without modifying gameplay handlers or making telemetry a gameplay writer.

## Current Next Step

Following `Gravebound_Production_GDD_v1_Canonical.md`, `Gravebound_Content_Production_Spec_v1.md`, and `Gravebound_Development_Roadmap_v1.md`, use hosted run `29906346469` to diagnose and additively repair the schema-71 loot projection, complete immutable origin-session binding for death/extraction/Recall/successor rows, and prove bounded lag plus restart re-poll. Keep the production worker disabled and source/exporter-free until the processor, region, access, encryption, retention, deletion, backup-expiry, and privacy-notice review passes.
