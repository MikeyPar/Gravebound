# ADR-027 — literal shared M02 authority and First Playable Recall

## Status

Accepted, 2026-07-12.

## Decision

The M02 roadmap phrase “four humans complete the combat test together” means four humans in one authoritative combat world. Concurrent isolated sessions do not satisfy that exit row.

The network build continues to use immutable `fp.1.0.0`, so `CONT-FP-010` controls manual Recall: `RecallStart` and `RecallCancel` return the typed `RecallUnavailableCombatLaboratory` result and do not mutate state. Automatic LinkLost Recall remains required by the GDD network lifecycle and is not a manual input.

One shared combat aggregate will be owned by each hosted instance. Logical sessions become authenticated endpoint/lifecycle owners bound to one explicit player entity. Existing one-player authority remains a compatibility facade for deterministic regression coverage.

## Consequences

- Protocol advances from exact-match 1.4 to 1.5 for controlled-player identity and the typed Recall result.
- M02 remains open until the shared-authority implementation and four-human evidence pass.
- M03 remains blocked.
- Existing manual-Recall session fixtures are superseded; automatic Recall and death-before-Recall ordering remain mandatory coverage.
- No M04 party scaling or group rewards are introduced. The authored `fp.1.0.0` encounter is unchanged for one to four M02 participants.

## Outcome

Implemented and verified on 2026-07-12. `GB-M02-09` supplies the shared aggregate and protocol 1.5 contract. The final human row is recorded as an explicitly labeled owner-assumed pass under ADR-025, so M02 is closed without inventing individual tester telemetry.
