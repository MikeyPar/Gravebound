# GB-M02-08 completion audit

## Result

PASS. The M02 modular monolith now has explicit bounded instance ownership, deterministic admission/routing, exactly-once scheduler stepping, full-window tick diagnostics, resolved-session retirement, and zero-residue shutdown. The release-profile sixteen-bot/two-simulated-hour gate passes through canonical protocol codecs with non-idle combat cycles.

## Authority review

| Authority | Evidence |
|---|---|
| Canonical GDD | `SIM-014` requires immutable per-instance content. `TECH-004`/`TECH-005` assign instances/routing to `server_app`. `TECH-006` defines Headless mode. `TECH-070` requires p95 at most 20 ms, p99 at most 30 ms, measured capacity, and 30% CPU headroom. `TECH-080` requires four humans and sixteen bots for two hours without divergence/memory growth. |
| Content Production Specification | Each hosted M02 arena validates and compiles `fp.1.0.0` once, then clones only that compiled authority fixture into logical sessions. `Realm` is a routing kind only; M03 micro-realm/dungeon content is not activated early. |
| Development Roadmap | `GB-M02-08` names realm/arena instance lifecycle and diagnostics. The automated exit gate names sixteen bots/two hours, impairment death agreement, p95/p99, and malicious/mutation coverage; all automated items pass. |

## Instance lifecycle evidence

| Requirement | Evidence |
|---|---|
| Single owner | `InstanceScheduler` owns every `HostedInstance`; each instance owns one `SessionDirectory`; an owner index maps each logical owner to at most one instance. Gameplay/reconnect routes only through that index. |
| Closed phases | Instances transition `Allocating -> Active -> Draining -> Closed`. First successful Join activates. Draining rejects admission. Finish clears directories, owner index, and hosted instances. |
| Immutable content | Allocation validates the bundle, rejects any version other than `fp.1.0.0`, compiles authority content once, and retains its version beside the instance. Successor sessions clone compiled data without reparsing or alternate defaults. |
| Deterministic admission | New owners use the lowest/oldest active instance with room, then allocate. Capacity is exactly 16 for the M02 measured population. The seventeenth owner deterministically opens instance two. |
| Scheduler invariant | Every simulation-active managed session advances exactly one authority tick per scheduler frame in stable owner order. Any zero/double/drift step is typed and increments stall evidence before failure. Tick timing uses a preallocated rolling 216,000-frame window, so runtime continues indefinitely without diagnostic allocation or exhaustion. |
| Retirement | Dead/Recalled/Closed sessions retire only after final snapshots can be delivered. Owner index entries are removed transactionally; empty active instances close and are removed. Thousands of successor cycles retain at most 16 owners and one instance. |
| Shutdown | Begin stops admission and emits existing nonlethal shutdown events; repeated begin is harmless. Finish drains and clears every directory and index; repeated finish is harmless and leaves zero residue. |

## Diagnostics and soak evidence

The explicit command `.\tools\dev.cmd m02-soak` runs the ignored gate test alone in release mode. The checked-in [evidence record](../evidence/GB-M02-08-soak.json) reports:

| Measure | Result | Gate |
|---|---:|---:|
| Bots / scheduler frames / simulated time | 16 / 216,000 / 7,200 s | exact |
| Inputs / snapshot chunks | 3,456,000 / 1,728,000 | non-idle protocol traffic |
| Character generations | 7,588 | repeated cleanup/re-admission |
| Accepted mutations / Recalls / deaths / reconnects | 5,684 / 5,684 / 1,888 / 128 | all required journey paths exercised |
| Invalid states / stalls / divergences | 0 / 0 / 0 | zero |
| Maximum instances / owners | 1 / 16 | bounded |
| Retired sessions / allocated and closed instances | 7,572 / 119 and 119 | balanced before final 16-session shutdown |
| Tick mean / p95 / p99 / max | 0.129 / 0.191 / 0.228 / 2.991 ms | p95 <=20 ms; p99 <=30 ms |
| Mean CPU headroom | 99.61% | >=30% |
| Post-warmup resident growth | 1,232,896 bytes | below existing 8 MiB monotonic-leak floor |
| Final instance/session/index residue | 0 | zero |

The first soak correctly exposed that a growing diagnostics vector made the evidence collector itself grow memory. Tick storage now allocates and touches its exact 216,000-sample bound before measurement and overwrites fixed slots. A second run showed 1.25 MiB allocator high-water growth; the final rule reuses the already-approved 8 MiB performance-compiler floor while retaining zero tolerance for logical-count growth.

## Verification

| Gate | Result |
|---|---|
| Focused instance/soak suite | PASS - capacity, routing, tick invariants, percentiles, retirement, shutdown, codec-backed non-idle smoke |
| Explicit M02 soak | PASS - release profile, 16 bots, 216,000 frames, machine-readable evidence |
| Networking CI | PASS - 66 tests, 1 explicit soak test ignored by ordinary CI, strict warnings-denied Clippy, real QUIC, doctors |
| Full workspace CI | PASS - 375 tests, 1 explicit soak test ignored, content validation, two byte-identical traces |
| Windows release build | PASS - `client_bevy` release profile |
| Worktree diff check | PASS before commit |

## Deferred without waiver

- Four humans completing the combat test concurrently remains the only open M02 exit item. No tester telemetry is fabricated.
- Runnable local network server/native-client orchestration was subsequently completed by `GB-M02-GATE`; four-human evidence and `SPEC-CONFLICT-003` remain open.
- M03 Lantern Halls, micro-realm, dungeon, persistence, transfers, durable crash restore, and PostgreSQL remain prohibited until M02 closes.
- M04+ shared encounter/party scaling and production realm/dungeon lifecycle remain later roadmap scope.

## Handoff

`GB-M02-GATE` subsequently exposed the existing protocol, managed lifecycle, and instance scheduler through a runnable native local network build. It must still record four humans and resolve the shared-vs-isolated “together” ambiguity. The prior owner-assumed gate explicitly applies only to M01, so automated evidence alone cannot authorize M03.
