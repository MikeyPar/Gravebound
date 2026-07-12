# GB-M02-05 completion audit

## Result

PASS. Gravebound now has a deterministic, socket-free, codec-backed impairment harness and an end-to-end M02 combat trace through the documented 100 ms RTT / 20 ms jitter / 1% loss exit profile. The harness changes delivery only; the real managed session, simulation, prediction, reconciliation, and lifecycle owners remain final.

## Authority review

| Authority | Evidence |
|---|---|
| Canonical GDD | `SIM-012` supplies 80/120/250 ms behavior targets and the 0.35-tile ordinary-correction allowance. `TECH-011`–`TECH-015` constrain channel reliability, timing, prediction, and outages. `QA-006` supplies the complete adverse matrix. |
| Content Production Specification | The integration journey loads and validates immutable `fp.1.0.0`, constructs the existing authority fixture, and never substitutes synthetic health, arena, enemy, movement, or attack values. |
| Development Roadmap | `GB-M02-05` names latency, jitter, loss, duplication, reordering, and outage. The M02 exit profile explicitly requires 100 ms RTT, 20 ms jitter, 1% loss, playable control, and authoritative death agreement. |

## Harness evidence

| Requirement | Evidence |
|---|---|
| Isolation | New `network_harness` depends only on `protocol`, deterministic ChaCha RNG, and error types. Production client/server/simulation crates do not depend on it. |
| Real codec | Every submission passes `protocol::encode_frame`; queued payloads are bytes; every delivery passes `decode_frame`. Header kind, protocol version, channel, transport flag, semantic validation, and size limits therefore remain active. |
| Exact matrix | Closed profiles pin Baseline `20/0/0/0`, Fully Supported `80/10/0.5%/0.1%`, Fair Play `120/20/1%/0.5%`, Degraded `180/40/2%/1%`, Severe `250/80/5%/2%`, and M02 Exit `100/20/1%/0`. Values are RTT/jitter/loss/reordering. |
| Determinism | One named seed and stable draw/submission order produce identical encoded deliveries, link transitions, and diagnostics on replay. The harness uses an explicit monotonic microsecond clock and no sleep or wall clock. |
| Reliability | Datagram messages may drop, duplicate, or receive reordering holdback. Reliable messages are scheduled once and retain per-direction/per-channel application order; explicit outage retransmission delay does not convert them into datagrams. |
| Outages | Up to eight sorted nonoverlapping `500 ms..=5 s` windows emit exact Down/Up transitions. Datagram submission/delivery during Down fails closed. Integration drives a 500 ms outage to pre-deadline reattach and the 90-tick boundary to authoritative Recall/Lantern Halls. |
| Bounds | Basis points reject values above 10000; clock/time/counters are checked; queue limits are 65,536 frames and 64 MiB; exact frame-limit and outage duration/overlap/count tests fail closed. |
| Diagnostics | Stats record submitted, delivered, probability loss, outage drop, duplicate copies, reordering, current/maximum queue count and bytes, and min/max/total delivery latency without account or auth data. |

## Gameplay and death evidence

The integration test sends 30 Hz input frames through the M02 Exit profile into `ManagedSession`, sends only its snapshot output back through the harness, and applies delivered chunks only through `RemoteClientRuntime`.

- Analog movement remains accepted and produces more than the minimum correction/input sample census.
- Snap corrections remain at or below one percent of ordinary samples.
- After network drain, client/server position differs by no more than the one-millitile wire envelope.
- The deterministic hostile simulation commits death; the client receives the same authoritative death tick and state version and clears prediction through its existing death-finality path.
- The authority emits a snapshot on the death commit tick even when that tick is outside ordinary 15 Hz cadence.
- No queued frames or bytes remain after drain.

The first end-to-end run exposed a real fixed-point boundary defect: rounding a legal server contact to millitiles could place the client snapshot fractionally inside a solid. `PlayerMovementState::from_authoritative_snapshot` now resolves only contact displacement within `0.001` tile, rejects deeper illegal positions, removes inward velocity through the shared collision solver, and then replays inputs. This preserves server authority without accepting teleport-through-solid state.

## Verification

| Gate | Result |
|---|---|
| Focused impairment/authority/client suite | PASS — harness goldens, M02 exit combat/death, outage lifecycle, prediction, protocol, and authority tests |
| Networking CI | PASS — 43 protocol/harness/server/bot tests, strict warnings-denied Clippy, real QUIC tests, and both doctor commands |
| Full workspace CI | PASS — 352 tests, format, strict all-target warnings-denied Clippy, content validation, and two byte-identical deterministic traces |
| Worktree diff check | PASS before commit |

## Deferred without waiver

- Teleport, speed, fire-rate, forged-hit, stale/replayed application input, duplicate pickup, and mutation-misuse coverage was completed by `GB-M02-06`; this audit retains its original deferral as historical scope evidence.
- The full journey bot was completed by `GB-M02-07`; the two-hour sixteen-bot soak remains a conjunctive M02 exit gate after `GB-M02-08` supplies instance scheduling and diagnostics.
- Realm/arena scheduler lifecycle, server tick percentiles, and clean multi-instance teardown: `GB-M02-08`.
- Four-human impaired playability approval remains a conjunctive full M02 exit gate and is not replaced by deterministic automation.

## Handoff

`GB-M02-06` subsequently attacked the actual encoded protocol and managed-session ingress exposed here. Encoded abuse now remains bounded by intent-only messages, existing simulation rules, sequenced inputs/actions, and payload-bound idempotent mutations; `GB-M02-07` can build its real-protocol journey bot on that closed boundary.
