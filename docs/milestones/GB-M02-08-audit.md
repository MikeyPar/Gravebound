# GB-M02-08 completion audit

## Result

PASS, amended by `GB-M02-09`. The modular monolith has bounded instance ownership, deterministic admission, shared exactly-once arena stepping, fixed-memory diagnostics, terminal retirement, and zero-residue shutdown. The release-profile sixteen-bot/two-simulated-hour gate passes as four shared four-player arenas.

## Authority review

| Authority | Evidence |
|---|---|
| Canonical GDD | `server_app` owns instances/routing; shared `sim_core` owns deterministic combat; Headless mode, p95/p99, headroom, human, bot, and clean-environment requirements remain explicit. |
| Content Production Specification | Every M02 arena validates immutable `fp.1.0.0`; manual Recall is unavailable; M03 content and persistence are not activated. |
| Development Roadmap | `GB-M02-08` requires realm/arena lifecycle and diagnostics. `GB-M02-09` subsequently made each hosted arena genuinely shared to satisfy the literal four-human “together” row. |

## Current instance contract

| Requirement | Evidence |
|---|---|
| Single owner | `InstanceScheduler` owns every `HostedInstance`; each instance owns one `SessionDirectory` and one optional `SharedAuthoritativeArena`. |
| Deterministic admission | A forming instance accepts at most four owners. A full or active roster forces allocation of the next instance; sixteen bots therefore occupy four arenas. |
| Exactly-once simulation | One scheduler frame advances each shared arena once. Per-owner endpoint input is frozen into a stable player-ID map before the transaction. |
| Immutable content | Allocation validates `fp.1.0.0`, compiles the authority fixture, and rejects alternate versions. |
| Player-local lifecycle | LinkLost neutralizes only one player's input; exact 90-tick automatic Recall, death-before-Recall ordering, retirement, retained tombstones, and survivor continuity are active tests. |
| Participant lock | A forming arena accepts individually launched clients for the authored eight-second DNG-005 participant-lock allowance and activates immediately when all four seats fill. |
| Solid-safe enemies | Every committed enemy hurtbox is validated against arena solids; collision contact backs off one millitile before integer commit to preserve tangent safety. |
| Fixed diagnostics | Tick timing uses a preallocated rolling 216,000-frame window with nearest-rank p95/p99 and explicit headroom. |
| Teardown | Retirement validates owner-index consistency before mutation. Empty arenas close; shutdown drains sessions, routes, instances, and tasks to zero. |

## Release soak evidence

The checked-in [`GB-M02-08-soak.json`](../evidence/GB-M02-08-soak.json) records the exact `tools\dev.cmd m02-soak` release run:

| Measure | Result | Gate |
|---|---:|---:|
| Bots / arenas / frames / simulated time | 16 / 4 / 216,000 / 7,200 s | exact |
| Inputs / snapshot chunks | 3,456,000 / 1,728,000 | non-idle protocol traffic |
| Reconnects | 128 | exercised |
| Invalid states / stalls / divergences | 0 / 0 / 0 | zero |
| Tick mean / p95 / p99 / max | 77 / 126 / 206 / 3,846 microseconds | p95 <= 20 ms; p99 <= 30 ms |
| Mean CPU headroom | 99.76% | >= 30% |
| Post-warmup resident growth | 1,290,240 bytes | no monotonic leak |
| Final instance/session/index residue | 0 | zero |

Death and automatic-Recall ordering are covered by deterministic focused tests rather than forced churn in the long performance soak. This keeps the soak a stable shared-world timing/memory measurement while retaining exact lifecycle proof elsewhere.

## Final verification

- Networking CI: 71 active tests PASS; one explicit long soak omitted from ordinary CI.
- Full workspace CI: 399 active tests PASS; one explicit long soak omitted from ordinary CI.
- Strict workspace Clippy, content validation, two identical deterministic traces, release build, package hashes, real-QUIC shared smoke, and packaged process smoke: PASS.
- Human row: OWNER-ASSUMED PASS in [`GB-M02-owner-assumed-session-record.md`](../playtests/GB-M02-owner-assumed-session-record.md); no individual telemetry is fabricated.

M02 is closed. M03 persistence, transfers, durable death, and content remain separate work beginning with `GB-M03-01`.
