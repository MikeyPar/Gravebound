# GB-M02 exit-gate audit

## Status

PENDING - automated gate PASS; four-human concurrent combat evidence missing.

| Exit requirement | Evidence | Status |
|---|---|---|
| Four humans complete the combat test together | No M02-specific owner attestation, tester record, or runnable network-client session evidence exists. The M01 assumption says no later milestone is waived. | PENDING |
| Sixteen bots run for two hours without crash, memory growth, invalid state, or simulation stall | `docs/evidence/GB-M02-08-soak.json`: 16 bots, 216,000 frames, zero invalid/stall/divergence/residue, growth below the approved leak floor. | PASS |
| 100 ms RTT / 20 ms jitter / 1% loss remains playable and accepted death matches authority | `GB-M02-05` deterministic codec-backed exit-profile trace. | PASS |
| Server tick p95 <=20 ms and p99 <=30 ms | Release soak p95 179 microseconds, p99 218 microseconds. | PASS |
| Every malicious/mutation test passes | `GB-M02-06` encoded abuse matrix remains green in networking CI. | PASS |

## Promotion rule

M02 remains active and M03 remains blocked until the four-human row becomes PASS. A future explicit owner decision may accept an assumption, but it must name M02 and retain the same no-fabricated-evidence boundary used for M01.
