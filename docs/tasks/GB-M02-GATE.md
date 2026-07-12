# GB-M02-GATE — runnable shared native network playtest

## Authority and objective

This task is subordinate to the Canonical Production GDD, Content Production Specification, and Development Roadmap. It closes the executable and human-evidence boundary after `GB-M02-08`; it does not authorize M03 content or persistence.

| Field | Contract |
|---|---|
| Feature | `GB-M02-GATE` |
| Objective | Four humans complete the M02 combat test together through native clients against one local modular-monolith server. |
| Runtime owners | `server_app` owns QUIC, sessions, instances, shared simulation, routing, and outcomes. `client_bevy` owns input, local movement prediction, presentation, and HUD. |
| Simulation | One `SharedAuthoritativeArena` per hosted instance at fixed 30 Hz; no client gameplay authority. |
| Content | Immutable `fp.1.0.0`; no M03 hub, persistence, dungeon, or production-content activation. |
| Population | One to four controlled players per arena; the packaged all-client launcher fills one four-player roster before activation. |

## Required implementation

- `server_app serve` validates content before binding, requires exact build/protocol/content identity, hashes opaque local credentials, and never logs ticket bytes.
- One scheduler advances each shared arena exactly once per 30 Hz frame and sends complete recipient-specific snapshots at 15 Hz.
- Every accepted Join/Reattach binds one stable controlled entity. Snapshots contain all shared players, enemies, and projectiles; acknowledgements and personal pickups remain recipient-specific.
- Shared action, mutation, reconnect, leave, snapshot, death, and automatic LinkLost Recall results use the shared authority clock.
- Friendly projectile provenance includes its owner, preventing remote fire from confirming local prediction.
- Manual Recall is unavailable in `fp.1.0.0`; LinkLost remains vulnerable for exactly 90 shared simulation ticks before player-local automatic Recall. Death on the deadline wins.
- The native client installs no local combat authority in network mode. It predicts only its controlled player's movement and renders remote state from snapshots.
- Transport queues, reliable boundaries, mutation caches, anomaly history, and snapshot assembly remain bounded and fail closed.
- Shutdown stops admission, closes transports, drains tasks, and proves zero instance/session/route residue.

## Acceptance tests

1. Real QUIC performs handshake, Join, input, shared snapshot, typed manual-Recall rejection, and zero-residue shutdown.
2. Four credentials join one arena, receive all four players and identical enemy facts, and retain independent directional input acknowledgement.
3. Shared enemy health/death facts are identical across recipients; owner-qualified projectiles cannot cross-confirm prediction.
4. Player-local death or automatic Recall does not clear shared threats or stop survivors.
5. Prediction, lifecycle, protocol, impairment, abuse, bot-journey, retirement, and soak tests pass with ordinary tests unquarantined.
6. Strict format/Clippy, content validation, deterministic traces, workspace tests, and Windows release builds pass.
7. The package contains both executables, content, server launcher, all-client launcher, four individual relaunchers, and this gate's runbook.
8. Four-human evidence records versions, opaque tester labels, simultaneous timing, outcome per client, defects, observations, and owner result. Automation must not fabricate this row.

## Explicit limitations

- This is a local nonpersistent M02 combat laboratory, not the production nexus/realm/dungeon loop.
- Enemy scaling and party bonuses remain out of scope; all clients fight the immutable `fp.1.0.0` authored encounter.
- PostgreSQL, durable accounts/characters, Lantern Halls presentation, transfers, parties, chat, Steam auth, and production certificates are M03+.
- Human evidence remains required even when all automated gates pass.
