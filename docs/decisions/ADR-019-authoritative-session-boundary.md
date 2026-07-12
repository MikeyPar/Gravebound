# ADR-019 — Authoritative session boundary

Status: Accepted

Implementation package: `GB-M02-02`

## Context

The M01 client currently orchestrates several deterministic `sim_core` primitives. M02 must make the remote server final without copying those rules into transport handlers or coupling the simulation to QUIC/protocol types.

## Decision

1. `sim_core` owns a transactionally stepped combat-test aggregate. It consumes simulation-native intents and returns immutable step/snapshot facts. It has no Tokio, QUIC, Serde, account, or renderer dependency.
2. `server_app` owns authenticated session identity, latest-state input coalescing, ordered action queues, 30 Hz scheduling, 15 Hz snapshot cadence, mutation idempotency, and translation between bounded protocol values and simulation-native intents.
3. `protocol` exposes only client intent and server facts. There is no client message for position, hit confirmation, damage, health, death, eligibility, reward resolution, or item grant.
4. `sim_content` compiles `fp.1.0.0` records into the definitions passed to `sim_core`. `server_app` never restates content-authored numeric values.
5. A gameplay tick is clone-then-commit across the aggregate. If movement, combat, collision, health, death, or inventory processing fails, no partial tick is observable.
6. Pickup eligibility and reach/capacity are simulation rules. The server boundary owns mutation identity and caches the first typed response for idempotent replay.

## Consequences

- LocalLab, headless tests, and the remote server can consume one gameplay-rule implementation.
- Network abuse tests can attack a narrow intent boundary without granting the server transport ownership of gameplay math.
- Durable death/item commits remain a later persistence adapter; M02 proves authority and finality inside the live instance only.
