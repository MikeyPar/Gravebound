# ADR-022 — Deterministic transport impairment

Status: Accepted

Implementation package: `GB-M02-05`

## Context

The network loop has real QUIC integration, but wall-clock proxy tests are slow, flaky, and poor at reproducing a single unfair correction or death. Testing simplified input objects would miss codec limits, channel reliability, version drift, and stale datagram behavior.

## Decision

1. Add a small socket-free `network_harness` crate depending only on `protocol`, deterministic RNG, and error types. Production gameplay crates do not depend on it.
2. Submit actual `WireMessage` values through the canonical encoder and store encoded bytes. Decode only when the explicit clock releases a delivery.
3. Split RTT into one-way base delay and draw signed symmetric jitter from one named seeded stream in stable submission order.
4. Datagram frames may be lost, duplicated, and delayed for reordering. Reliable application frames are never probabilistically lost, duplicated, or reordered; QUIC packet retransmission is below this seam.
5. Outages emit deterministic link transitions and suppress datagram delivery. Lifecycle orchestration consumes those facts and remains the only owner of LinkLost, reconnect, Recall, and death.
6. Queue count/bytes, probability bounds, outage count/duration/overlap, clock monotonicity, and time arithmetic fail closed.
7. The integration journey uses `ManagedSession` and `RemoteClientRuntime` directly on either side of the harness. It does not duplicate movement, collision, health, or reconciliation rules.
8. Authoritative death is local-critical state and emits a snapshot on its commit tick even if that tick is outside the ordinary 15 Hz cadence.

## Consequences

- Any failed adverse-network trace can be replayed exactly from profile, seed, submissions, and clock steps.
- M02-06 can inject stale and duplicate application messages intentionally without conflating them with QUIC reliability.
- M02-07 can reuse the harness for long-running bot journeys without sleeps.
- Real QUIC tests remain responsible for socket/TLS behavior; the deterministic harness is responsible for delivery semantics and repeatable gameplay evidence.
