# GB-M02 exit-gate audit

## Status

PENDING - automated gate and runnable implementation PASS; four-human evidence and `SPEC-CONFLICT-003` resolution missing.

| Exit requirement | Evidence | Status |
|---|---|---|
| Four humans complete the combat test together | Runnable native package and four-client automated routing smoke exist. No four-human session record exists. Current authority is four concurrent isolated simulations, not a shared combat world; `SPEC-CONFLICT-003` prevents silently treating those as equivalent. | PENDING |
| Sixteen bots run for two hours without crash, memory growth, invalid state, or simulation stall | `docs/evidence/GB-M02-08-soak.json`: 16 bots, 216,000 frames, zero invalid/stall/divergence/residue, growth below the approved leak floor. | PASS |
| 100 ms RTT / 20 ms jitter / 1% loss remains playable and accepted death matches authority | `GB-M02-05` deterministic codec-backed exit-profile trace. | PASS |
| Server tick p95 <=20 ms and p99 <=30 ms | Final release soak p95 191 microseconds, p99 228 microseconds. | PASS |
| Every malicious/mutation test passes | `GB-M02-06` encoded abuse matrix remains green in networking CI. | PASS |

## Promotion rule

M02 remains active and M03 remains blocked until the four-human row and `SPEC-CONFLICT-003` become resolved. A future explicit owner decision may accept an assumption, but it must name M02, define “together,” resolve the First Playable Recall rule, and retain the no-fabricated-evidence boundary used for M01.
