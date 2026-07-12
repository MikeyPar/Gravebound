# GB-M02 exit-gate audit

## Status

PASS BY EXPLICIT OWNER ASSUMPTION. Every automated shared-authority gate passes locally. The owner directed the project to assume successful playtests and continue building; ADR-025 permits that direction only when clearly labeled, so the human row is an owner-assumed pass rather than fabricated measured telemetry.

| Exit requirement | Evidence | Status |
|---|---|---|
| Four humans complete the combat test together | [`GB-M02-owner-assumed-session-record.md`](../playtests/GB-M02-owner-assumed-session-record.md) records the owner's successful-playtest assumption without inventing individual telemetry. Shared-world meaning is resolved by ADR-027. | OWNER-ASSUMED PASS |
| Sixteen bots run for two hours without crash, memory growth, invalid state, or simulation stall | `docs/evidence/GB-M02-08-soak.json`: 16 bots, four shared arenas, 216,000 frames, zero invalid/stall/divergence/residue, and no monotonic post-warmup leak. | PASS |
| 100 ms RTT / 20 ms jitter / 1% loss remains playable and accepted death matches authority | `GB-M02-05` deterministic codec-backed exit-profile trace passes. | PASS |
| Server tick p95 <= 20 ms and p99 <= 30 ms | Release soak: p95 75 microseconds, p99 103 microseconds. | PASS |
| Every malicious/mutation test passes | `GB-M02-06` encoded abuse matrix passes in networking CI. | PASS |

## Promotion rule

All conjunctive M02 rows are closed. M03 may begin at `GB-M03-01`. The owner-assumed human result must remain labeled and cannot substitute for measured tester telemetry at later milestones.
