# GB-M03-13 atomic Fallen Hero Echo persistence completion audit

**Status:** PASS on main implementation commit `18dcbad`; hosted CI [`29506273492`](https://github.com/MikeyPar/Gravebound/actions/runs/29506273492) is green.

## Three-authority closure

| Authority | Closure |
|---|---|
| `Gravebound_Production_GDD_v1_Canonical.md` | `DTH-001` and `ECH-001`/`002` are represented by authoritative eligibility, an immutable approved snapshot without reusable item identity/roll, and atomic creation with the qualifying death. |
| `Gravebound_Content_Production_Spec_v1.md` | `CONT-ECHO-009` is enforced as `none -> Dormant`, at most one Available/account, and oldest-first `(created_at,echo_id)` promotion with immutable transition/outbox authority. |
| `Gravebound_Development_Roadmap_v1.md` | `GB-M03-13` now commits qualifying death, destruction, memorial, and Echo together or not at all; encounter assembly remains explicitly deferred. |

## Echo acceptance

| Criterion | Evidence | Result |
|---|---|---|
| Eligibility | Exact level 9/10, 17,999/18,000 combat ticks, boss/major-event deeds, incident/admin provenance, missing content, and duplicate-death boundaries are covered. | PASS |
| Snapshot safety | Stored Echo identity, character/class/Oath/appearance/theme/signature/deed/cause/region/power band/content fields are bounded and hashed; item UID/roll, commercial entitlement, localized prose, and mutable pointers are absent. | PASS |
| Mandatory transaction participant | Eligible death cannot commit without the projector; projector, transition, promotion, uniqueness, outbox, or deferred-graph failure rolls back death/destruction/summary/Memorial state. | PASS |
| Availability outcomes | Self-promotion creates Dormant then Available; an existing Available leaves the new Echo Dormant; ineligible deaths store exact `NotCreated` outcomes without Echo rows. | PASS |
| Oldest-first selection | Locked-state selection computes the minimum `(created_at,echo_id)` directly. Reversed multiple-Dormant fixtures prove older timestamp precedence and equal-timestamp ID tie-breaking. | PASS |
| Account invariant | PostgreSQL permits at most one Available/account and rejects a committed account with Dormant history but no Available. Later Archive/Defeat-triggered promotion remains outside M03 death ownership. | PASS |
| Concurrency/replay | Two distinct accounts commit/replay concurrently with exact isolated graphs and zero residue; same-account duplicate final-death writers serialize to one stored result. | PASS |
| Restart/corruption | Echo snapshot/transition/outbox/signature state remains byte-stable across reconnect/restart and rejects altered payload, wrong account, corrupt history, and injected rollback. | PASS |

## Cumulative verification

- Archived [concurrency report](../evidence/GB-M03-13-concurrent-eligible-deaths.json): two fresh commits, two exact replays, zero cross-account rows, four stable signature checks, and zero final residue.
- Archived [six-branch matrix](../evidence/GB-M03-06E-death-branch-matrix.json): four exact noneligible outcomes, one Available self-promotion, and one Dormant queue result.
- Full integrated evidence: [`GB-M03-06E-integrated-evidence.md`](../evidence/GB-M03-06E-integrated-evidence.md).

## Deferred ownership

Requiem allocation, Normal/Practice encounter assembly, Defeat/Archive UI and later lifecycle transactions, rewards, party entry, telemetry delivery, support lookup, and M04+ Echo content remain open.

## Handoff

`GB-M03-13` is closed as the atomic death projector. It enables no Requiem encounter or normal route.
