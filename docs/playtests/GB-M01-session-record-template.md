# GB-M01 session record template

Copy this file once per session and replace every `<required>` value. Store completed records with restricted local playtest evidence, not in the public repository.

## Identity and eligibility

| Field | Value |
|---|---|
| Tester ID | `tester-<required>` |
| Session ID | `session-<required>` |
| Build ID | `release-<required full BLAKE3>` |
| Executable SHA-256 | `<required>` |
| Content bundle | `fp.1.0.0` |
| Content package BLAKE3 | `<required>` |
| Consent complete | `<true/false>` |
| Feature contributor/prior tester | `<true/false>` |
| Genre familiarity | `<new_to_both/action_rpg_only/bullet_hell_only/action_rpg_and_bullet_hell>` |
| Developer tools used | `<true/false>` |
| Coaching affected result | `<true/false>` |
| Session complete | `<true/false>` |
| Final eligibility | `<eligible_blind/exclusion reason>` |

## Observed behavior

Observed facts only; do not put opinions in this table.

| Observation | Tick/time | Researcher-authored de-identified summary |
|---|---:|---|
| First confusion | `<required/not_observed>` | `<max 280 bytes>` |
| First damage | `<required/not_observed>` | `<max 280 bytes>` |
| First item | `<required/not_observed>` | `<max 280 bytes>` |
| First death | `<required/not_observed>` | `<max 280 bytes>` |
| Restart action | `<required/not_observed>` | `<max 280 bytes>` |

## Death-cause response before trace

| Field | Value |
|---|---|
| Open killer response summary | `<required/not_observed; max 280 bytes>` |
| Open attack response summary | `<required/not_observed; max 280 bytes>` |
| Selected killer ID | `<required/not_observed>` |
| Selected pattern ID | `<required/not_observed>` |
| Response recorded before trace | `<true/false/not_observed>` |
| Authoritative killer ID | `<required/not_observed>` |
| Authoritative pattern ID | `<required/not_observed>` |
| Killer correct | `<true/false/not_observed>` |
| Pattern correct | `<true/false/not_observed>` |

## Tester opinion after run

| Field | Value |
|---|---|
| Movement | `<1..5>` |
| Shooting | `<1..5>` |
| Dodging | `<1..5>` |
| Overall combat feel | `<1..5>` |
| Distinctive summary | `<required; max 280 bytes>` |
| Stop reason summary | `<required; max 280 bytes>` |
| Desired next action summary | `<required; max 280 bytes>` |
| Wants another attempt | `<yes/no>` |
| Voluntarily restarted | `<true/false>` |
| Death-summary-to-restart milliseconds | `<integer/not_observed>` |

## Evidence and issues

| Field | Value |
|---|---|
| Telemetry path | `<required>` |
| Telemetry SHA-256 | `<required>` |
| Record SHA-256 after completion | `<required>` |
| Screenshot/video paths | `<optional local paths>` |
| Issue IDs | `<none or IDs>` |
| Operator pseudonym | `<required>` |
| Session UTC start/end | `<required>` |

Operator attestation: `<I followed the GB-M01 blind-test runbook, did not reveal the detailed death trace before recording the cause response, and kept observed behavior separate from opinion.>`
