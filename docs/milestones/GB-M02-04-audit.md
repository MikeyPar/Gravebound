# GB-M02-04 completion audit

## Result

PASS. One authenticated owner now has exactly one server-owned logical combat session independent of replaceable QUIC transports. Join, leave, vulnerable LinkLost timeout, reconnect, duplicate-session handoff, Emergency Recall resolution, and graceful shutdown are deterministic and covered through the real protocol.

## Authority review

| Authority | Evidence |
|---|---|
| Canonical GDD | `TECH-010`–`TECH-015` define the bounded handshake/Control channel, 30 Hz timebase, exact three-second LinkLost window, state-preserving reconnect, death/Recall precedence, and post-handoff invalidation. `DTH-010` defines Emergency Recall inventory disposition. |
| Content Production Specification | The managed session continues to construct gameplay exclusively from immutable `fp.1.0.0`. Reconnect retains the server record. Recall removes unsecured backpack/ground state, preserves equipped items and belt consumables, and disables reward eligibility. |
| Development Roadmap | `GB-M02-04` names join, leave, timeout, LinkLost, reconnect, duplicate-session handling, and clean shutdown; every named behavior has a focused test and the complete journey crosses real QUIC. |

## Implementation evidence

| Requirement | Evidence |
|---|---|
| Identity ownership | `SessionDirectory` is keyed by opaque nonzero `SessionOwnerId`; each record owns one `AuthoritativeSession`. Nonzero logical-session and transport IDs are separate types, and auth-ticket bytes never enter lifecycle state. |
| Reliable Control contract | Protocol `1.3` adds bounded `SessionControlFrame` Join/Reconnect/Leave requests and typed accepted/rejected `SessionControlResult` values on the reliable Control channel. The codec kind map and canonical frame hash are repinned. |
| Join and typed rejection | First Join constructs one authority aggregate. Missing, unauthorized, stale-sequence, resolved, and shutdown requests return closed typed codes without mutating gameplay or transport ownership. |
| LinkLost | Transport loss and accepted Leave neutralize continuous movement/held-fire input, remove ingress authority, and set `recall_tick = lost_tick + 90`. `ManagedSession::tick` continues the ordinary authoritative combat aggregate while disconnected. |
| Boundary precedence | Each LinkLost tick commits simulation and observes death before checking the deadline. Tests pin reconnect through tick 89, Recall exactly at tick 90, and final death winning on the same boundary tick. |
| Emergency Recall | `AuthoritativeArena::commit_emergency_recall` clone-then-commits a terminal Recalled phase, clears friendly/hostile projectiles and unsecured ground pickups, destroys backpack stacks, preserves equipped items and belt Tonics, disables rewards, and increments state version. |
| Reconnect | A valid owner/session pair rebinds the same aggregate. Tests pin retained entity identity, monotonically advanced state version during LinkLost, immediate server tick/monotonic time-sync facts, combat reattachment before resolution, Lantern Halls after Recall, and Death Final after death. |
| Duplicate handling | A replacement binding is fully validated and its response constructed before atomic commit. Only then does the result identify the previous transport for closure. Later ingress from that transport fails `StaleTransport`. |
| Client finality | `ClientConnectionLifecycle` shows the local three-second countdown but changes only to `AwaitingAuthoritativeResolution` at expiry. Only a typed server result selects Combat Instance, Lantern Halls, or Death Final. |
| Real transport | A Rustls-authenticated two-connection QUIC test performs handshake, Join, duplicate Join/reattach, explicit old-transport application close, reliable Leave acknowledgement, and clean leaving-transport close. |
| Shutdown | `begin_shutdown` stops admission, emits one reliable `ServerShuttingDown` event per connected session, closes sessions without death, and is idempotent. `finish_shutdown` deterministically drains the directory to zero entries. |

## Protocol decision

Protocol minor `1.3` added reliable lifecycle Control request/result variants. `GB-M02-06` subsequently advances the exact-match protocol to `1.4` for fail-closed ingress semantics and typed mutation results. Exact minor matching remains mandatory and no adapter is claimed. Session owner and transport identities remain server-internal; the wire carries only the bounded logical session identifier required for reconnect.

## Verification

| Gate | Result |
|---|---|
| Focused authority/network/client suite | PASS — 308 tests across `sim_core`, `protocol`, `server_app`, `bot_client`, and `client_bevy` |
| Networking CI | PASS — 34 protocol/server/bot tests, real QUIC lifecycle, warnings-denied Clippy, and both doctor commands |
| Full workspace CI | PASS — 342 tests, format, strict all-target warnings-denied Clippy, content validation, and two byte-identical deterministic traces |
| Worktree diff check | PASS before commit |

## Deferred without waiver

- Deterministic latency, jitter, loss, duplication, reordering, and outage injection were completed by `GB-M02-05`; this audit retains their original deferral as historical scope evidence.
- The comprehensive teleport, speed, fire-rate, forged-hit, replay, pickup, and mutation abuse matrix was completed by `GB-M02-06`; this audit retains its original deferral as historical scope evidence.
- Full journey bot including manual Recall and reconnect: `GB-M02-07`.
- Realm/arena scheduler ownership, lifecycle diagnostics, teardown, and tick percentiles: `GB-M02-08`.
- Durable Lantern Halls records, danger-entry crash restoration, item ledgers, and extraction transactions: M03 and later persistence packages.

## Handoff

`GB-M02-05` subsequently drove the existing protocol, managed session, prediction, reconciliation, and client lifecycle through a deterministic codec-backed impairment layer without granting the harness authority over timeout, Recall, or death.
