# GB-M02-01 completion audit

## Result

PASS. Gravebound has bounded versioned contracts, deterministic postcard framing, typed admission, and a real QUIC handshake. Replicated gameplay remains correctly deferred to `GB-M02-02`.

## Authority review

| Authority | Evidence |
|---|---|
| Canonical GDD | `protocol` pins `TECH-010` hello/rejections, `TECH-011` channels, `TECH-012` rates, `TECH-013` interest constants, `SIM-004` inputs, and `SIM-011` pattern descriptors. |
| Content specification | Server hello identifies the immutable bounded bundle version; M02 does not mutate `fp.1.0.0` content. |
| Roadmap | Repository `ADR-018` records the roadmap `ADR-003` transport/codec/version decision and documents the pre-existing identifier collision. |

## Gate evidence

| Gate | Result |
|---|---|
| Strict bounded values and exact rejection set | PASS — protocol and policy unit tests |
| Canonical binary fixture and malformed-frame rejection | PASS — codec round-trip/hash and fail-closed tests |
| Real transport | PASS — ephemeral-certificate Rustls/QUIC client-server loopback |
| Focused warnings-denied Clippy | PASS — `protocol`, `bot_client`, and `server_app`, all targets |
| Focused tests | PASS — 19 library tests plus binary/doc targets |
| Full repository CI | PASS — `tools/dev.cmd ci` after integration |

## Handoff

`GB-M02-02` must attach authenticated session state to the accepted connection and route authoritative gameplay through `sim_core`; it must not duplicate movement, combat, item, or death rules in `server_app`.
