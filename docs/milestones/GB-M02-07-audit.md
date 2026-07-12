# GB-M02-07 completion audit

## Result

PASS. Gravebound now has a reusable headless policy that observes decoded protocol state and sends bounded intent through real QUIC. Separate legal terminal journeys prove movement, aim, combat, pickup, transport replacement, manual Emergency Recall, Lantern Halls routing, authoritative death, and Death Final routing without a client-side authority shortcut.

## Authority review

| Authority | Evidence |
|---|---|
| Canonical GDD | `SIM-003`-`SIM-012` and `COM-001`-`COM-009` keep movement, cadence, collision, damage, and death server-owned. `DTH-010` supplies the exact Recall channel, movement, input-lock, damage, cleanup, and death-precedence rules. `TECH-010`-`TECH-015` require bounded reliable lifecycle/action traffic, latest-state snapshots, and state-preserving reconnect. |
| Content Production Specification | Both journeys load immutable `fp.1.0.0` through the ordinary managed session. Bot policy contains no arena, class, enemy, attack, drop, reach, health, or inventory definitions. |
| Development Roadmap | `GB-M02-07` names a headless bot that moves, aims, fights, picks up, dies, Recalls, and reconnects. Every verb is observed through protocol-visible evidence. The sixteen-bot/two-hour run remains the full M02 gate after `GB-M02-08` provides scheduling and diagnostics. |

## Bot boundary evidence

| Requirement | Evidence |
|---|---|
| Dependency boundary | `bot_client` has no dependency on `server_app`, `sim_content`, inventory, encounter, or client presentation crates. It consumes protocol messages and uses `sim_core` only to assert the shared 30 Hz foundation constant. |
| Snapshot assembly | Up to 64 protocol-valid chunks are assembled by sequence and index. Older completed sequences and identical duplicates are harmless; inconsistent metadata, conflicting duplicates, cross-chunk entity duplication, invalid chunks, and missing/multiple players fail closed. |
| Intent policy | The bot selects the nearest visible living enemy, produces bounded fixed-point aim, holds primary through server cadence, approaches only visible eligible/uncollected personal pickups, and waits for a typed accepted mutation result before recording collection. |
| Sequence safety | Input, Action, Control, and mutation identities are monotonic and checked. Exhaustion fails before message construction; mutation identities are deterministic and never zero. |
| Reconnect | The bot retains the logical session ID and its command/evidence state across a replacement QUIC connection. It sends reliable Reconnect, accepts only the typed server result, and never reconstructs gameplay state locally. |
| Finality | Player `alive`/health snapshot facts and lifecycle destination select Active, Recalled, or Dead. The bot does not predict or author Recall/death finality. |
| Diagnostics | Evidence is bounded to counts and journey facts; authentication bytes never enter reports or debug output. |

## Manual Emergency Recall

Manual Recall was the one deferred journey feature missing from the pre-M02-07 server.

- Reliable `RecallStart` begins a server-owned `400 ms = 12` tick channel; redundant start is `InvalidState` and reliable `RecallCancel` restores ordinary play.
- Movement intent is scaled to exactly 7,500 basis points while channeling. Primary held state and unconsumed press identity are both suppressed, preventing a queued shot from leaking through the channel.
- Ability presses and pickup interaction are typed nonmutating rejections while channeling. Existing damage does not cancel Recall.
- Each tick resolves ordinary hostile simulation first. Health zero on the completion tick commits Dead and clears the channel; only a still-living player commits Recall.
- Manual completion emits a critical snapshot even on an odd tick, clears combat/projectiles/unsecured pickups and pending inventory through the existing Recall transaction, preserves equipped/belt state, removes the active transport binding, and routes a later reconnect to Lantern Halls.

## Real-QUIC journey evidence

The first journey performs a real handshake and Join, consumes snapshot datagrams, aims and fires until it observes friendly projectiles and enemy damage, moves to a visible personal pickup, receives an accepted reliable mutation, replaces its QUIC transport through Reconnect without state/version loss, starts manual Recall, receives its terminal snapshot, and reconnects to `LanternHalls`.

The second journey uses the same policy in passive-death mode. It continues sending bounded input while hostile simulation owns damage, receives the critical zero-health death snapshot at the exact server committed tick, and reconnects to `DeathFinal`. Bot-side code never calls server authority or lifecycle mutation methods; server-side assertions inspect final state only after protocol travel.

## Verification

| Gate | Result |
|---|---|
| Focused simulation/bot/server suite | PASS - 220 simulation, 5 bot, 23 server-library, and 2 real-QUIC journey tests plus existing ingress/impairment integrations |
| Networking CI | PASS - 58 protocol/harness/server/bot tests, strict warnings-denied Clippy, real QUIC tests, and both doctor commands |
| Full workspace CI | PASS - 367 tests, format, strict all-target warnings-denied Clippy, content validation, and two byte-identical deterministic traces |
| Worktree diff check | PASS before commit |

## Deferred without waiver

- Realm/arena instance ownership, scheduler, bounded admission, tick percentile diagnostics, and clean multi-instance teardown were completed by `GB-M02-08`; this audit retains their original deferral as historical scope evidence.
- The sixteen-bot/two-simulated-hour automated gate was completed by `GB-M02-08`; four-human concurrent combat remains the only open M02 exit item.
- Four-human impaired playability approval remains a conjunctive M02 gate and is not replaced by bot automation.
- Accounts, durable reconnect/character/item/death state, PostgreSQL, and production identity/certificates remain later milestones.

## Handoff

`GB-M02-08` must host independent managed sessions under explicit instance ownership, drive this existing bot policy without privileged state, record tick p95/p99 and memory/stall/invalid-state evidence, and tear down every transport, session, and instance cleanly. It must then run the full sixteen-bot/two-hour M02 population gate before claiming M02 completion.
