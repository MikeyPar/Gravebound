# ADR-024 - Protocol-visible journey bot and manual Recall

Status: Accepted

Implementation package: `GB-M02-07`

## Context

The existing bot executable can exchange individual QUIC messages but has no journey policy. The server also implements disconnect-timeout Recall while rejecting the explicit `RecallStart`/`RecallCancel` actions that `DTH-010` and the M02-07 handoff require. A useful bot must not gain privileged access merely because it is an automated client.

## Decision

1. `bot_client` owns protocol-visible observation, bounded snapshot assembly, intent policy, sequence generation, and journey evidence. It does not depend on `server_app`, `sim_content`, client presentation, or server authority types.
2. Combat and pickup choices use only decoded entity snapshots and typed reliable results. Exact gameplay range, cooldown, damage, eligibility, inventory, and terminal outcomes remain server-owned.
3. A bot keeps its logical session ID and command sequences across transport replacement. It performs a real handshake and reliable Reconnect on the new QUIC connection.
4. Manual Emergency Recall is authoritative simulation state, not bot timing. `sim_core` owns the 12-tick channel, 75% movement scaling, combat locks, damage behavior, cleanup, and death precedence. `server_app` translates reliable Recall actions and lifecycle destinations only.
5. Manual Recall and authoritative death use separate terminal journey fixtures because a single life cannot legally reach both terminal states.
6. CI runs deterministic bounded journey coverage. The exact sixteen-bot/two-simulated-hour population soak remains a conjunctive M02 exit artifact and is executed after `GB-M02-08` supplies multi-instance scheduling, tick percentiles, and teardown diagnostics.
7. Bot diagnostics are bounded and redact auth material. A bot failure is evidence; it never repairs or mutates server state out of band.

## Consequences

- The same headless policy can drive loopback integration, future local-lab processes, and population soak without a privileged test seam.
- Recall rules become available to human and bot clients through the same protocol action.
- `GB-M02-08` can schedule many independent bot sessions without redesigning bot authority boundaries.
- Durable reconnection, production identity, and persistence remain later milestone responsibilities.
