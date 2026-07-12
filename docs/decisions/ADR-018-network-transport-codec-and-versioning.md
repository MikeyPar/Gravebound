# ADR-018 — Network transport, codec, fallback, and versioning

Status: Accepted

Roadmap decision key: `ADR-003`

Implementation package: `GB-M02-01`

## Numbering note

The roadmap reserves `ADR-003` for this M02 decision, but repository `ADR-003-primary-fire-and-projectile-lifecycle.md` was already accepted during M01. Overwriting or silently renumbering an accepted decision would destroy traceability. This record therefore fulfills the roadmap's `ADR-003` requirement under the next repository-safe identifier, `ADR-018`.

## Authorities

- Canonical GDD `TECH-010` through `TECH-015` and `SIM-004`/`SIM-011`.
- Content Production Specification `CONT-002`; network work continues to identify immutable bundle `fp.1.0.0`.
- Development Roadmap `ADR-003` and `GB-M02-01`.

## Decision

1. Use native QUIC through pinned `quinn 0.11.11`, Tokio, and Rustls. Production endpoints require a certificate anchored in the shipped trust policy; tests use an ephemeral locally trusted certificate.
2. Send `Input` and `Snapshot` as QUIC datagrams when both peers support datagrams. Send `Action`, `Pattern`, `Mutation`, `Control`, and `Social` on reliable ordered streams. Handshake always uses one bounded bidirectional reliable stream.
3. If datagrams are unavailable, use dedicated reliable streams for input and snapshots. The receiver still applies sequence semantics: it discards input/snapshot frames older than the newest accepted sequence. Fallback protects correctness but may degrade responsiveness under loss; it must be exposed in diagnostics and tested under the M02 impairment gate.
4. Serialize with pinned `postcard 1.1.3` over Serde using externally tagged enums. Each frame has `GBN1` magic, protocol major/minor, message kind, transport flag, little-endian payload length, and payload. Semantic channel is a closed mapping from message type; header kind and transport must agree with the decoded payload.
5. Cap complete datagram frames at 1,200 bytes and reliable frames at 64 KiB. Snapshot chunks contain at most 32 entities and at most 64 chunks. Auth tickets contain 1–2,048 bytes and are redacted from `Debug` output.
6. Major-version mismatch is incompatible. Minor versions are exact until a tested per-version adapter exists; the server must not claim compatibility merely because a message happens to decode. Required build mismatch is `UpdateRequired`. Protocol evolution must advance the minor or major field and update golden fixtures before changing pinned canonical bytes.
7. Admission rejection precedence is maintenance, protocol, build, content, suspension, authentication, rate limit, capacity, internal retry. This order prevents avoidable authentication work during global denial states and makes client behavior deterministic.

## Consequences

- The protocol crate remains socket-free and independently fuzzable/testable.
- QUIC provides encryption, stream isolation, and optional unreliable delivery without creating a custom security protocol.
- Reliable fallback is correctness-preserving but not performance-equivalent; M02-05 must prove the degraded mode and surface it to operators.
- Any wire-layout change is an explicit compatibility event with updated version fields and fixtures.
