# GB-M02 exit-gate audit

## Status

PENDING — automated shared-authority gates pass; four-human evidence remains missing.

| Exit requirement | Evidence | Status |
|---|---|---|
| Four humans complete the combat test together | One maximum-four-player shared arena, four-client real-QUIC integration, native package, and runbook exist. No four-human session record exists. | PENDING |
| Sixteen bots run for two hours without crash, memory growth, invalid state, or simulation stall | `docs/evidence/GB-M02-08-soak.json`: 16 bots, four shared arenas, 216,000 frames, zero invalid/stall/divergence/residue, and no monotonic post-warmup leak. | PASS |
| 100 ms RTT / 20 ms jitter / 1% loss remains playable and accepted death matches authority | `GB-M02-05` deterministic codec-backed exit-profile trace passes. | PASS |
| Server tick p95 <= 20 ms and p99 <= 30 ms | Release soak: p95 79 microseconds, p99 105 microseconds. | PASS |
| Every malicious/mutation test passes | `GB-M02-06` encoded abuse matrix passes in networking CI. | PASS |

## Promotion rule

M02 remains active and M03 remains blocked until the four-human row is recorded and accepted. Automation may validate the executable but may not fabricate human playtest evidence.
