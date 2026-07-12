# ADR-025 - Bounded instance host and soak evidence

Status: Accepted, amended by ADR-027 and `GB-M02-09`

Implementation package: `GB-M02-08`

## Context

`SessionDirectory` correctly owns logical-session lifecycle but currently exists as a free-standing aggregate. There is no explicit instance owner, capacity/admission policy, scheduler frame, percentile evidence, resolved-session retirement, or population teardown. The M02 gate also requires sixteen non-idle bots for two hours, which cannot be honestly inferred from short single-session tests.

## Decision

1. `InstanceScheduler` is the M02 modular-monolith owner. It owns hosted instances and the owner-to-instance routing index. A hosted instance owns its `SessionDirectory`; directories never migrate between instances.
2. M02 executes only the validated First Playable `CombatArena`. `Realm` remains a closed typed kind for future routing; no M03 world content or persistence is invented.
3. New admission chooses the oldest active instance with room, tie instance ID, then allocates. M02 capacity is sixteen because that is the measured exit population, not an Early Access realm-cap promise.
4. One scheduler frame advances each simulation-active managed session once. Tick movement other than exactly +1 is a typed stall/invariant failure.
5. Resolved sessions are retired explicitly and empty instances close. Successor cycles therefore prove that owner indexes, mutation caches, snapshots, and authority aggregates do not accumulate.
6. Diagnostics retain a fixed-memory rolling 216,000-frame window and use nearest-rank p95/p99. The explicit soak fills it exactly once; longer-running servers overwrite the oldest sample without allocation or exhaustion. Mean work measures 30% CPU headroom against 33,333 microseconds; p95/p99 independently enforce 20/30 ms.
7. The soak uses canonical codec traversal with sixteen existing `JourneyBot` policies and actual process-memory samples. It reuses the performance compiler's 8 MiB monotonic-growth floor to distinguish a leak from allocator page noise, while logical instance/session/index counts have zero growth tolerance. Real QUIC journey behavior was proven in GB-M02-07; repeating millions of loopback packets adds transport overhead noise without improving authority coverage.
8. M02 automated closure and human playability are recorded separately. Automation cannot fabricate a four-human session; an explicit owner assumption remains labeled as such.

## Consequences

- M03 can add transfer/persistence adapters around a stable instance owner rather than moving lifecycle rules into transport code.
- Population performance and cleanup become reproducible artifacts instead of assertions based on unit-test speed.
- The M02 arena host intentionally does not claim shared co-op encounter simulation or production realm capacity.
- Later operations work must replace the full-window test collector with production rolling metrics and external observability.

## 2026-07-12 amendment

ADR-027 supersedes the original isolated-session capacity/stepping model. Current M02 capacity is four players per `HostedInstance`; the instance owns one shared authority aggregate and sixteen-bot evidence runs as four arenas. The owner-assumption labeling rule in decision 8 remains binding.
