# GB-M03 Formal Gate Audit

**Status:** IN PROGRESS — `14 / 23` fixed closure units are proven (**61%**).

## Design authorities

This audit reads all three authorities together:

1. `Gravebound_Production_GDD_v1_Canonical.md`
2. `Gravebound_Content_Production_Spec_v1.md`
3. `Gravebound_Development_Roadmap_v1.md`

The percentage uses a stable denominator: the Roadmap's 14 named `GB-M03` work packages plus nine explicit M03 exit-gate outcomes. A unit counts only when its implementation, required automated evidence, and applicable human or operational evidence are complete. Partial credit is never rounded into a pass.

## Work-package closure (`10 / 14`)

| Package | Verdict | Evidence boundary |
| --- | --- | --- |
| `01`, `02`, `04`, `05`, `06`, `07`, `08`, `11`, `12`, `13` | Proven | Published task evidence and three-authority package audits close their owned contracts. |
| `03` | Open | The assembled ordinary route is packaged, but final no-command PostgreSQL/real-QUIC full-loop and 25-journey evidence are not yet published. |
| `09` | Open | Terminal telemetry is committed-source backed; onboarding, session, loot, and crash still need committed producer paths and hosted accuracy/redaction proof. |
| `10` | Open | The bounded read-only implementation and PostgreSQL least-privilege gate exist; hosted execution and the package audit remain open. |
| `14` | Open | Provider-pinned hosting, backup/restore, rollback, and owner-supplied Steamworks evidence remain external gates. |

## Exit-gate closure (`4 / 9`)

| Exit outcome | Verdict | Evidence boundary |
| --- | --- | --- |
| Restart preservation | Proven | Hosted component evidence covers inventory, Vault, death, Memorial, Echo, extraction, Recall, and successor state. Final route-level repetition remains useful corroboration, not a reason to erase the component pass. |
| No duplicate durable mutation | Proven | Transactional replay/restart evidence covers item, currency, character, death, Echo, and terminal results. |
| Atomic qualifying death | Proven | Death, destruction, Memorial, and Echo commit together or not at all. |
| Median death-to-successor control `<15 s` | Proven | The authenticated PostgreSQL/QUIC 25-journey successor report records a median far below the limit. |
| Ordinary no-command route plus 25 full loops | Open | Earlier 25-journey reports are valuable subsystem evidence, not the final production-server route sweep. |
| Median login-to-control `<30 s` | Open | Existing timings predate the assembled normal route and must be remeasured there. |
| `>=70%` eligible deaths reach successor combat within two minutes | Open | Requires the real private cohort; automated reachability is not player behavior. |
| `>=80%` correctly explain the latest death | Open | Requires the real private-cohort comprehension check against stored death facts. |
| 10–20-person private cohort and operational release proof | Open | Hosting, backup/restore, support/telemetry operational review, and Steamworks evidence are not yet complete. |

## Release-candidate boundary

The optimized tester assembles the implemented player route and its server into one package. Packaging proves construction, content validation, CLI behavior, isolated launch health, and stale-artifact cleanup. It does not by itself prove the open normal-route, cohort, hosting, or platform gates.

## Current Next Step

First, obtain a green hosted PostgreSQL run for migration `0069` and the `GB-M03-10` least-privilege test. Then implement committed onboarding/session/loot/crash telemetry sources and run the final ordinary-route PostgreSQL/real-QUIC journey harness, including 25 loops and current login timing. After those engineering gates pass, publish the `03`, `03G`, `09`, and `10` audits. The owner/operations gates for the 10–20-person cohort, comprehension metrics, provider backup/restore, deployment rollback, and Steamworks evidence remain required for `14` and final M03 closure. Every action continues to be governed by `Gravebound_Production_GDD_v1_Canonical.md`, `Gravebound_Content_Production_Spec_v1.md`, and `Gravebound_Development_Roadmap_v1.md`.
