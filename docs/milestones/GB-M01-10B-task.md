# GB-M01-10B — Local playtest telemetry and survey contract

## Scope

Implement the privacy-safe, local-only instrumentation required to evaluate the First Playable without introducing accounts, durable player persistence, remote analytics, or unrestricted personal data.

**Current state:** PASS. Typed contract, live explicit-consent client adapter, executable-derived build correlation, local atomic export, deterministic fixtures, contributor-excluded technical dry runs, and the researcher runbook/session template pass. The product owner explicitly accepted the operated survey and external-cohort gates as successful assumptions for milestone progression; provenance is recorded without fabricated raw results.

Authoritative sources:

1. `Gravebound_Production_GDD_v1_Canonical.md`: `TEL-001` through `TEL-004`, `TECH-123`, `QA-008`, and `QA-100`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-FP-010` restart behavior and `fp.1.0.0` content identity.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M01-10`, M01 exit gate, and `GB-TP02` cohort.
4. `docs/milestones/GB-M01-completion-plan.md`: expanded `GB-M01-10B` contract and local identity/survey defaults.

## State ownership and files

- `sim_core::telemetry` owns the versioned local event schema, validation, ordering, restart correlation, survey value types, and deterministic JSON Lines export.
- The gameplay systems that know an event occurred remain authoritative for their gameplay state; they submit typed event payloads to the log rather than reimplementing telemetry rules.
- No gameplay outcome depends on telemetry success.
- Allowed implementation files: `crates/sim_core/src/telemetry.rs`, the `sim_core` public export boundary, and this ticket's task/audit documents.
- Accounts, network transport, remote collection, dashboards, crash upload, retention storage, and production `ADR-005` pipeline work are out of scope until their roadmap milestones.

## Privacy and identity contract

- `pseudonymous_account_id` MUST equal `local_lab_no_account`. M01 has no account identity.
- `local_tester_id` MUST be an opaque `tester-` plus 16 lowercase hexadecimal characters.
- `session_id` MUST be an opaque `session-` plus 16 lowercase hexadecimal characters.
- Build, bundle, platform, local region, local environment, cohort eligibility, genre familiarity, and metric eligibility MUST be present in every envelope.
- An eligible blind tester is separate from an excluded feature contributor or tester lacking complete consent.
- Nonstandard time scale, developer tools, or debug invulnerability MUST make gate metrics ineligible.
- Raw name, email, platform ID, IP address, phone number, Discord/Steam handle, URL, or unrestricted transcript has no schema field.
- Open-ended answers MUST be researcher-authored redacted summaries, bounded to 280 bytes. The boundary rejects common direct-identifier markers and long digit runs. The original raw answer stays outside telemetry and follows the human consent/evidence protocol.

## Required M01 event schema

Every record uses the `TEL-001` envelope and monotonic per-session sequence. Supported M01 events are:

- `session_started`, `session_ended`, `run_started`, `run_restarted`;
- `boss_started`, `boss_phase_changed`, `boss_defeated`;
- `damage_received`, `character_died` with the applicable `TEL-003` fields;
- `item_picked_up`, `item_equipped`, `item_destroyed`;
- `client_crash`;
- `observation_recorded`, `killer_response_recorded`, `death_trace_revealed`, `survey_completed`.

`run_restarted` MUST identify the previous and new run, reason, elapsed ticks, voluntary activation, and the death ID for a death restart. A victory restart requires a recorded boss defeat and no death ID.

## Ordering and survey rules

- `session_started` is first and unique; no record follows `session_ended`.
- Runs use unique IDs. Boss phase/defeat events require a boss start in the active run.
- Damage, death, and item lifecycle events require the active run.
- Final `server_fault` deaths are rejected under `TEL-003`.
- Record each first observation category at most once: confusion, damage, item, death, and restart.
- Record the tester's killer/pattern selection before the detailed death trace is revealed.
- Keep observed behavior in `observation_recorded`; keep opinion in `survey_completed`.
- Survey ratings are four separate `1..=5` values: movement, shooting, dodging, and overall combat feel.
- Survey records require all three open prompts: what felt distinctive, what would make the tester stop, and what they want to do next.
- Genre familiarity is one of: new to both, action-RPG only, bullet-hell only, or familiar with both.

## Acceptance criteria

- Append-time schema and state-machine validation reject missing prerequisites, duplicate identities, timestamp regression, invalid payload arithmetic, and illegal ordering.
- Restart records correlate a known previous run to one unique fresh run and the applicable death/boss result.
- Export is deterministic JSON Lines with a pinned fixture hash.
- Every record contains the exact local sentinel and common build/bundle/cohort fields.
- Tests prove no raw personal-identifier field exists and obvious direct identifiers are rejected at the only human-text boundary.
- Tests prove observation and opinion are separate and the open survey is complete.
- Human completion requires a consented dry run of the survey/evidence form and a `GB-TP02` cohort-exclusion check.

## Required verification

```powershell
cargo fmt --all -- --check
cargo test -p sim_core telemetry
cargo clippy -p sim_core --all-targets -- -D warnings
```

The cumulative repository gate remains the root milestone owner's responsibility after concurrent M01 work settles.
