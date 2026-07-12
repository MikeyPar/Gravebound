# SPEC-CONFLICT-003 — M02 “together” scope and First Playable Recall

## Status

Open. Only the conflicting promotion claims and manual-Recall client work are stopped; runnable network authority work may continue.

## Conflict A — four-human meaning

- Development Roadmap `GB-M02` exit gate says: “Four humans complete the combat test together.”
- The M02 work packages and implemented authority bind one authenticated owner's complete combat-test state to one `ManagedSession`. Each session owns an independent `AuthoritativeSession`; M04 later introduces shared encounter/party scaling.
- Current `GB-M02-08` capacity groups sessions under one host instance but does not create a shared simulation. All isolated sessions use protocol player ID `10_000` and receive owner-routed snapshots.

## Conflict B — manual Recall

- Content Specification `CONT-FP-010` says Recall input in nonpersistent `fp.1.0.0` returns `recall_unavailable_combat_laboratory` and the HUD displays `RECALL UNAVAILABLE — LOCAL TEST`.
- Roadmap `GB-M02-07` says the headless bot “Recalls,” while GDD `DTH-010` and `TECH-015` require Emergency Recall and automatic disconnect resolution. Existing M02-07 interpreted this as a successful manual Recall journey.
- Protocol `1.4` has no exact `recall_unavailable_combat_laboratory` result code.

## Conservative implementation pending owner/spec decision

- The native M02 playtest client does not offer or transmit manual Recall and displays the Content Specification copy.
- Automatic `LinkLost` Recall remains required because it is explicitly network lifecycle behavior.
- Existing server/manual-bot coverage is retained as historical implementation evidence, but it does not override `CONT-FP-010` or close this conflict.
- Four concurrent isolated human completions may be recorded as runtime/authority evidence. They must not be labeled shared party combat or used alone to assert the ambiguous “together” row.

## Required resolution

The owner/spec revision must choose both:

1. Define M02 “together” as either concurrent isolated completions on one server or shared-world combat, with the latter adding an explicit multiplayer-authority package.
2. Define whether M02's network build inherits the `fp.1.0.0` manual Recall prohibition or creates a versioned network-test exception, including the exact typed result and tests.
