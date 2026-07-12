# GB-M01-10B completion audit

- **Status:** PASS — implementation verified; human gate accepted by owner assumption
- **Date:** 2026-07-11
- **Features:** `TEL-001` through `TEL-004`, `TECH-123`, `QA-008`, `QA-100`, `CONT-FP-010`
- **Task contract:** [GB-M01-10B-task.md](GB-M01-10B-task.md)

## Acceptance evidence

| Criterion | Evidence | Result |
|---|---|---|
| Common envelope | Each record carries event ID/name/schema/time, fixed local account sentinel, opaque tester/session IDs, build, `fp.1.0.0`-capable bundle field, platform, local region/environment, cohort tags, and monotonic sequence. | PASS |
| Local identity policy | Tester/session IDs have strict opaque hexadecimal formats; account is always `local_lab_no_account`; eligibility distinguishes blind, contributor, incomplete-consent, and developer-tool exclusions. | PASS |
| Required event coverage | Typed events cover session/run, boss lifecycle, damage, death/killer, item pickup/equip/destroy, restart, crash, observations, trace protocol, and survey. | PASS |
| Event ordering | Append-time state machine requires session/run/boss prerequisites, unique run/death/boss events, nonregressing timestamps, active-run ownership, and no event after session end. | PASS |
| Restart correlation | Death restarts require a known death; victory restarts require a boss defeat; previous/new run IDs cannot alias or be reused. | PASS |
| Cause-before-trace | A detailed trace cannot be revealed until the tester selects a killer and pattern; a response after reveal is rejected. | PASS |
| Observation vs opinion | First confusion/damage/item/death/restart observations have a dedicated event and uniqueness rule; survey ratings/open answers use a separate event. | PASS |
| Survey/cohort | Four individual `1..=5` ratings, three mandatory open prompts, restart desire, genre familiarity, and blind-cohort exclusion state are explicit. | PASS |
| No raw PII | No name/email/platform/IP field exists. The sole human-text type is a bounded redacted researcher summary and rejects identifier markers, URLs, handles, phone-like digit runs, and control characters. | PASS |
| Deterministic export | Identical typed input produces byte-identical JSON Lines; fixture BLAKE3 is `9688cab59dd8cf880473932d34af352dd9bc70e52f55858f3422646a42c0f961`. | PASS |
| Live event-source adapter | Explicit-consent-only client adapter records session/run, exact damage, boss lifecycle, death, item pickup/equip/destroy, and correlated death/victory restart events; ordinary play writes nothing. | PASS |
| Local export | App exit atomically publishes JSON Lines to the operator-selected local path with the fixed `local_lab_no_account` sentinel and cohort/metric exclusions. | PASS |
| Immutable build correlation | The live adapter, diagnostics, and performance evidence use the same full executable-derived BLAKE3 build ID rather than relabeling a content hash as a build. | PASS |
| Research package | `docs/playtests/GB-M01-blind-test-runbook.md` and the session-record template specify consent, eligibility, uncoached observation, cause-before-trace order, privacy boundaries, exact questions, evidence hashing, and gate math without researcher invention. | PASS (tooling ready) |

## Automated verification

- `cargo fmt --all -- --check`: PASS.
- `cargo test -p sim_core telemetry`: PASS, 7 tests.
- `cargo clippy -p sim_core --all-targets -- -D warnings`: PASS.
- Integrated live dry runs passed for six exact hostile damage events, a Still Eye pickup, Bell start/defeat, and ordered death cleanup/restart. The final restart export contains `session_started -> run_started -> damage_received -> character_died -> item_destroyed x3 -> run_restarted -> item_equipped x3 -> session_ended`, SHA-256 `9D98D48AE13E49860226BC59C2593F7074C39D22E763D4D585B3A59CB4D0375D`.
- The dry-run cohort is explicitly `excluded_feature_contributor`; it cannot contaminate the blind gate.
- Final cumulative verification passes 294 workspace tests, warnings-denied all-target Clippy, strict content validation, repeated deterministic traces, the optimized workspace build, release smoke, and the target-performance gate.

The tests cover local sentinel isolation, schema/ordering failures, boss prerequisites, cause-before-trace, death restart correlation, cross-run activity, observation/opinion separation, complete survey prompts, rating and identifier boundaries, obvious PII rejection, required event names, and pinned deterministic export bytes.

## Owner-accepted human and external evidence

The product owner explicitly instructed the implementation agent to assume successful playtests and continue. The following checks are therefore accepted for milestone progression without inventing raw records:

1. The consented survey/evidence procedure completed end to end, including killer response before trace and separate observation/opinion entry.
2. The gate roster contained at least 10 complete eligible `GB-TP02` testers and excluded feature contributors.

Exact raw identities, rows, recordings, and denominators were not supplied and are not fabricated. The decision provenance and every assumed threshold are recorded in [`GB-M01-owner-assumed-gate.md`](../playtests/GB-M01-owner-assumed-gate.md). Future real evidence may replace the assumption report without rewriting this decision history.
