# ADR-026 — runnable M02 local network runtime

## Status

Accepted for `GB-M02-GATE`.

## Context

`GB-M02-01` through `GB-M02-08` proved the protocol, QUIC primitives, authority, prediction, lifecycle, impairment, abuse resistance, journey bot, scheduler, and soak in tests, but neither native executable owned a complete runnable network loop. The M02 human gate cannot be executed against doctor commands or test-only endpoints.

## Decision

1. `server_app serve` is the nonpersistent gate runtime. It generates an ephemeral loopback certificate, validates `fp.1.0.0`, accepts exact-version QUIC clients, maps hashed opaque tickets to ephemeral owners, drives `InstanceScheduler` at 30 Hz, and owns routing/teardown.
2. `client_bevy network` is a distinct presentation mode. A bounded Tokio/Quinn worker communicates with Bevy through latest-state input, a 16-chunk rolling snapshot queue, and 64-entry reliable channels. LocalLab authority plugins are not installed.
3. The supplied certificate is added to a dedicated root store. An insecure verifier is prohibited.
4. The current isolated M02 player entity ID is one shared protocol constant. This is explicitly temporary: a shared-world protocol must carry the controlled entity binding in handshake/Join state.
5. Existing workspace `blake3`, Quinn, rustls, rcgen, Tokio, and Clap dependencies are reused. No external service or database is introduced.
6. `LocalStack` retains its GDD meaning—client plus `server_app` plus PostgreSQL—and therefore remains M03. The M02 executable is named “network playtest,” not LocalStack.

## Consequences

- Four humans can now run the actual native protocol/authority seam on one Windows machine.
- Input and snapshots remain droppable latest-state traffic; reliable queue saturation fails visibly instead of silently losing a command.
- Credentials, content, characters, and certificates are ephemeral and unsuitable for production.
- Concurrent isolated sessions do not establish shared-world multiplayer. The gate audit must keep that distinction visible.
