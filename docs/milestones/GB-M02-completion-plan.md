# GB-M02 authoritative network loop completion plan

This plan is subordinate to the canonical GDD, Content Production Specification, and Development Roadmap. It records executable order and conservative status; it does not waive an upstream requirement.

## Milestone contract

| Field | Requirement |
|---|---|
| Objective | Establish final authority boundaries before persistence or content expansion |
| Runtime | Native client plus authoritative modular-monolith `server_app`; headless `bot_client`; no PostgreSQL yet |
| Simulation | Shared `sim_core`, fixed 30 Hz, server authoritative |
| Transport | QUIC with datagrams where available and reliable streams for critical events, after ADR-003 |
| Content | Continue immutable `fp.1.0.0`; M02 adds network behavior, not M03 content |
| Exit population | Four humans and sixteen bots |

## Dependency order

```text
M01 PASS
  -> 00 workspace/boundaries
  -> 01 protocol/handshake/channels
  -> 02 server authority
  -> 03 prediction/interpolation/reconciliation
  -> 04 lifecycle/reconnect
  -> 05 impairment harness
  -> 06 malicious input/mutation rejection
  -> 07 headless journey bot
  -> 08 instance lifecycle/diagnostics
  -> M02 gate
```

## Work packages

| ID | Exact outcome | Status |
|---|---|---|
| `GB-M02-00` | Add `protocol`, `server_app`, and `bot_client` with pinned runtime dependencies, strict ownership boundaries, tests, doctor commands, and CI targets. | PASS |
| `GB-M02-01` | Versioned hello/rejection/session, input frames, snapshots, reliable events, channel envelopes, error codes, schema/codec limits, and the roadmap ADR-003 transport decision (recorded as repository `ADR-018`). | PASS |
| `GB-M02-02` | Server authority for movement, attacks, cooldowns, projectiles, collision, health, death, eligibility, and pickup using shared `sim_core`. | PASS |
| `GB-M02-03` | Local movement prediction, remote interpolation, reconciliation thresholds, and deterministic projectile presentation. | PASS |
| `GB-M02-04` | Join/leave/timeout, three-second `LinkLost`, reconnect, duplicate-session handoff, and clean shutdown. | PENDING |
| `GB-M02-05` | Deterministic latency, jitter, loss, duplication, reordering, and outage harness. | PENDING |
| `GB-M02-06` | Reject teleport, speed, fire-rate, forged hit, duplicate pickup, stale/replayed input, and mutation misuse. | PENDING |
| `GB-M02-07` | Headless bot moves, aims, fights, picks up, dies, Recalls, and reconnects only through the real protocol. | PENDING |
| `GB-M02-08` | Realm/arena instance lifecycle, ownership, scheduler, tick diagnostics, and clean teardown. | PENDING |

## Exit gate

All are conjunctive:

- Four humans complete the combat test together.
- Sixteen bots run for two hours without crash, memory growth, invalid state, or simulation stall.
- At 100 ms RTT, 20 ms jitter, and 1% loss, controls remain playable and accepted deaths match authoritative traces.
- Server tick p95 is at most 20 ms and p99 at most 30 ms.
- Every malicious/mutation test passes.

Failure holds the project in M02 authority/network work. It does not authorize persistence or content expansion.
