# SPEC-CONFLICT-003 — M02 “together” scope and First Playable Recall

## Status

Resolved by owner direction on 2026-07-12. ADR-027 is the binding implementation decision.

## Conflict A — four-human meaning

- Development Roadmap `GB-M02` exit gate says: “Four humans complete the combat test together.”
- The M02 work packages and implemented authority bind one authenticated owner's complete combat-test state to one `ManagedSession`. Each session owns an independent `AuthoritativeSession`; M04 later introduces shared encounter/party scaling.
- At conflict discovery, `GB-M02-08` grouped isolated sessions under one host instance. `GB-M02-09` subsequently replaced that limitation with one shared maximum-four-player aggregate and explicit controlled-player identity.

## Conflict B — manual Recall

- Content Specification `CONT-FP-010` says Recall input in nonpersistent `fp.1.0.0` returns `recall_unavailable_combat_laboratory` and the HUD displays `RECALL UNAVAILABLE — LOCAL TEST`.
- Roadmap `GB-M02-07` says the headless bot “Recalls,” while GDD `DTH-010` and `TECH-015` require Emergency Recall and automatic disconnect resolution. Existing M02-07 interpreted this as a successful manual Recall journey.
- Protocol `1.4` has no exact `recall_unavailable_combat_laboratory` result code.

## Implemented resolution

- M02 requires genuine shared-world combat for the four-human exit row. Isolated concurrent sessions remain regression evidence only.
- The native M02 playtest client does not offer or transmit manual Recall and displays the Content Specification copy.
- Protocol 1.5 returns `recall_unavailable_combat_laboratory` for manual Recall actions without mutating state.
- Automatic `LinkLost` Recall remains required because it is explicitly network lifecycle behavior.
- Superseded manual-Recall fixtures were removed. Typed manual rejection and automatic LinkLost Recall/death ordering provide the active coverage.
- Four concurrent isolated human completions may be recorded as runtime/authority evidence. They must not be labeled shared party combat or used alone to assert the ambiguous “together” row.

## Follow-through

`GB-M02-09` implements the shared aggregate, deterministic ordering, protocol identity, player-local lifecycle, and acceptance tests. The four-human row is closed separately as an explicitly labeled owner assumption under ADR-025; no individual telemetry is fabricated.
