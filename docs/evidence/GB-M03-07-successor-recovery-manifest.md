# GB-M03-07 successor recovery journey evidence

## Authority

This evidence is accepted only against all three design authorities:

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-020`, `DTH-021`, `UI-007`-`009`, `TECH-021`-`023`, and `QA-101` require the primary Create Successor path, no more than two confirmations, authoritative Hall readiness, measured death-to-control, rapid combat return, durable retry, and no duplicate state.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-CATALOG-003` fixes the exact four fresh Grave Arbalist starter identities and placements.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-07` and the M03 exit gate require 25 integrated recovery journeys, median death-to-control below 15 seconds, p95 below 30 seconds, at least 70% return to combat within 120 seconds, and zero duplicate character/grant state.

## Evidence identity

- Source commit: `a85c10c4556cba413beafdbf45bbaa54dab95313`
- Hosted CI: [`29608793943`](https://github.com/MikeyPar/Gravebound/actions/runs/29608793943)
- Hosted job: `PostgreSQL migrations and transactions`
- Test: `successor_recovery_completes_25_real_quic_journeys_without_duplicates_or_residue`
- Report: [`GB-M03-07-successor-recovery.json`](GB-M03-07-successor-recovery.json)
- Report schema: `gravebound.performance.gb-m03-07.successor-recovery.v1`
- Canonical report BLAKE3: `ecd75f19ce4e2fbceff5d74faf5bdde93c5a67206ae74b5ff51befe1cd3757da`
- File SHA-256: `8113092de7664da7e0df0ab36043570b53aa47a3cc772e98be46d077f7aead2b`
- Build ID: `m03-core-dev-identity-1`
- Successor content revision: `core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb`

## Measured result

| Measurement | Median | p95 | Maximum |
|---|---:|---:|---:|
| Terminal commit | 135.101 ms | 145.109 ms | 183.330 ms |
| Successor create real-QUIC round trip | 37.185 ms | 44.435 ms | 45.699 ms |
| Durable death acknowledgement to controllable Hall | 118.676 ms | 129.126 ms | 135.342 ms |
| Terminal summary ready to controllable Hall | 41.630 ms | 49.406 ms | 52.149 ms |
| Controllable Hall to danger-scene control | 37.246 ms | 40.608 ms | 46.053 ms |
| Terminal summary ready to danger-scene control | 78.842 ms | 90.945 ms | 94.384 ms |

All 25 journeys passed:

- exactly two semantic confirmations: Create Successor, then Play;
- 25 fresh successor results, 25 unique death IDs, 25 unique successor IDs, and 25 unique receipt IDs;
- 100 unique starter item UIDs across the 25 exact `CONT-CATALOG-003` four-item grants;
- preselected Character Select, checked Hall result, matching Hall scene readiness, checked Realm Gate transfer, and matching danger-scene readiness;
- exact death and successor durable graphs after each journey;
- 25/25 technical routes reached permadeath-enabled danger control within 120 seconds;
- zero remaining QUIC connections, server tasks, non-idle database sessions, active/idle/aborted transactions, or waiting/granted gameplay locks.

The report's `accepted` field is `true`. The measured median and p95 are below the `DTH-021`/roadmap limits by more than two orders of magnitude.

## Scope boundary

This is automated disposable technical-route evidence. It proves the route can perform the required action sequence and timing without duplication or operational residue; it does not claim the later human private-cohort behavior metric. The report records that exclusion in `behavioral_cohort_scope`.

Only the integration test constructs the combined death-view, successor, and world-flow capability policy. Normal Character Select Play, production Realm Gate admission, Core promotion, and M04+ features remain fail closed.
