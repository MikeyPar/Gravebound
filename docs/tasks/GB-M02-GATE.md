# GB-M02-GATE — runnable native network playtest and human exit evidence

## Authority and objective

This task is subordinate to the canonical GDD, Content Production Specification, and Development Roadmap. It closes only the executable and human-evidence boundary after `GB-M02-08`; it does not authorize M03 content or persistence.

| Field | Contract |
|---|---|
| Feature | `GB-M02-GATE` |
| Objective | Let four humans run the M02 authoritative combat test through native clients against one local modular-monolith server, then record honest gate evidence. |
| Runtime owners | `server_app` owns QUIC transports, logical sessions, instances, scheduling, routing, and final outcomes. `client_bevy` owns input, local movement prediction, presentation, and HUD. |
| Simulation | Existing shared `sim_core` authority at fixed 30 Hz; no client gameplay authority. |
| Content | Immutable `fp.1.0.0` only. No M03 hub, persistence, dungeon, or production-content activation. |
| Transport | Exact protocol version; QUIC datagrams for input/snapshots and bounded reliable streams for control/actions/mutations. |
| Population | Four distinct opaque local credentials connected concurrently. |

## Required implementation

### Server executable

- `server_app serve` binds a configured loopback QUIC address and generates a per-launch self-signed certificate for explicit client trust.
- Validate `fp.1.0.0` before listening. Handshake build, manifest, protocol, rates, region, and feature flag must match exactly.
- Hash opaque local tickets inside the authentication boundary and map them to stable ephemeral owner IDs without logging ticket bytes.
- Allocate unique transport IDs, preserve owner identity across reconnect, route every request through `InstanceScheduler`, and close replaced transports only after the replacement response commits.
- Advance the scheduler at 30 Hz with skipped—not burst-replayed—missed wall-clock ticks.
- Queue 15 Hz owner-specific snapshot datagrams. A resolved session becomes retirement-eligible after its final snapshot, but retains its authoritative reconnect tombstone while a transport route exists; release both without cross-owner leakage after disconnect or shutdown.
- On connection loss, neutralize input and enter the existing three-second vulnerable `LinkLost` lifecycle.
- On shutdown, stop admission, emit typed shutdown events where possible, close transports, drain tasks, finish the scheduler, and prove zero instance/session/route residue.

### Native client executable

- Preserve `client_bevy local-lab`; add `client_bevy network --server --certificate --player --content-root`.
- A dedicated Tokio thread owns Quinn. Bevy communicates through a latest-state input watch, a fixed 16-chunk rolling snapshot queue, and a fixed 64-command/event reliable boundary.
- Trust only the explicitly supplied local certificate. No blanket certificate-verification bypass is allowed.
- Install no LocalLab combat, enemy, encounter, inventory, consumable, or death authority systems in network mode.
- Sample WASD/mouse, transmit coalesced input at 30 Hz, predict only local movement, reconcile from server snapshots, and render remote entities/projectiles from protocol state.
- Send right-click/Space ability edges reliably and use `E` for the nearest eligible personal pickup within the authored interact reach.
- Derive health, authoritative death, enemy presence, and combat-test completion only from complete snapshots or typed reliable results.
- Display `M02 NETWORK PLAYTEST — NONPERSISTENT`, connection state, authoritative health, correction diagnostics, control help, and `RECALL UNAVAILABLE — LOCAL TEST`. Never claim party/shared-world combat.
- Reconnect automatically within the existing three-second window using the same opaque credential and logical session ID. Local time may show status but may not decide Recall or death.

## Acceptance tests

1. Real QUIC server integration performs handshake, Join, input, owner-routed snapshot, shutdown, and zero-residue teardown.
2. Four distinct concurrent clients join one server; each receives a snapshot acknowledging only its own directional input and no other owner's authority stream.
3. Snapshot transport storage remains bounded and retains the newest 16 chunks under pressure.
4. Existing prediction, lifecycle, protocol, impairment, abuse, bot-journey, instance, and soak tests remain green.
5. Strict format/Clippy, content validation, deterministic traces, full workspace tests, and Windows release builds for both executables pass.
6. A packaged directory contains both executables, `fp.1.0.0` content, launchers for one server and four unique clients, and the playtest runbook.
7. Four-human evidence records build/content versions, four opaque tester IDs, simultaneous session timing, outcome per client, defects, observations, and an owner result. Automation may not fabricate this row.

## Explicit limitations and out of scope

- Current M02 authority is one isolated combat simulation per owner. Concurrent clients do not see one another or share enemies. This proves concurrent native authority routing, not shared party combat.
- The roadmap phrase “complete the combat test together” is not silently redefined. `SPEC-CONFLICT-003` records the ambiguity against the single-owner M02 work-package architecture.
- PostgreSQL, durable accounts/characters, Lantern Halls presentation, realm/dungeon transfers, party systems, shared encounter scaling, chat, Steam auth, and production certificates are M03+.
- Manual Recall behavior for `fp.1.0.0` remains inside `SPEC-CONFLICT-003`; the network HUD exposes the Content Specification rule and sends no manual Recall action.
