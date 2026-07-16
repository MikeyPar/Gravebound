# GB-M03-06B lifetime, deed, cause, and combat-trace completion audit

**Status:** PASS on main implementation commit `18dcbad`; hosted CI [`29506273492`](https://github.com/MikeyPar/Gravebound/actions/runs/29506273492) is green.

## Three-authority closure

| Authority | Closure |
|---|---|
| `Gravebound_Production_GDD_v1_Canonical.md` | `DTH-001`, `DTH-020`, `ECH-001`, `TECH-015`, and `TECH-023` are represented by checked 30 Hz lifetime/combat clocks, exact LinkLost/crash semantics, deterministic lethal cause, final deed, and the retained final ten seconds. |
| `Gravebound_Content_Production_Spec_v1.md` | Exact Core boss/deed/content identities and `CONT-ECHO-009` eligibility inputs remain stable across replay/restart and reject unqualified or incompatible evidence. |
| `Gravebound_Development_Roadmap_v1.md` | `GB-M03-06`/`13` receive deterministic server inputs for atomic death and tester-readable cause without moving successor or extraction behavior into this slice. |

## Deterministic acceptance

| Criterion | Evidence | Result |
|---|---|---|
| Lifetime clock | Advances only for controllable living Hall/danger state, includes the vulnerable LinkLost window, excludes loading/select/offline, and converts ticks to milliseconds with checked arithmetic. | PASS |
| Combat clock | Advances after committed danger entry, stops at death/extraction/Recall, restores to the entry value after an uncommitted crash, and uses exact 17,999/18,000 Echo eligibility. | PASS |
| Deed authority | Reward-qualified boss/major-event records are idempotent; duplicate, practice, dead, Recalled, rejected, or ineligible rewards do not qualify. Latest `(tick,deed_id)` and `deed.none` fallback are exact. | PASS |
| Cause and trace | Ordered `(tick,event_ordinal)` damage evidence retains the final 300-tick window, stable same-tick order, one lethal entry, source/pattern/attack/damage/health/position/status/network/Recall state, and last five oldest-to-newest. | PASS |
| Boundary rejection | Tick regression, nonfinite/invalid positions, invalid IDs/statuses, health disagreement, inconsistent lethality, overflow, and content mismatch fail before terminal commit. | PASS |
| Restart/replay | Serialized live trace, receipt window, promoted provenance, cause, deed, last-five projection, and hashes reconstruct identically after restart and exact retry. | PASS |

## Cumulative verification

- Focused simulation/service/repository tests cover Hall/danger/loading/offline/LinkLost, exact clock edges, same-tick order, 300/301 eviction, fewer/more than five entries, crash rollback, deed ties, and fallback.
- Hosted PostgreSQL/real-QUIC composition and six reachable eligibility branches pass under [`GB-M03-06E-integrated-evidence.md`](../evidence/GB-M03-06E-integrated-evidence.md).
- The explicit 30-minute soak preserves the stored trace/signature through 425 reconnect/replay cycles.

## Deferred ownership

SQL destruction/finalization, Echo insertion, presentation, successor, extraction/Recall inventory behavior, telemetry, and route admission remain outside `06B`.

## Handoff

`GB-M03-06B` is closed and supplies the sealed deterministic inputs consumed by `06C`.
