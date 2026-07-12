# ADR-020 — Client prediction and network presentation

Status: Accepted

Implementation package: `GB-M02-03`

## Context

The server now owns the complete combat session. A responsive client still needs immediate local movement and attack feedback, but replaying or interpolating incomplete facts can silently create a second gameplay authority.

## Decision

1. The client predicts only local movement with the exact simulation-owned movement primitive. Authoritative position and velocity reset that primitive before unacknowledged inputs replay.
2. Simulation prediction commits immediately; reconciliation smoothing is a presentation offset only. Future input never starts from a visually blended or otherwise false position.
3. Correction thresholds and durations are the exact `TECH-014` values. Boundary comparisons are pinned by tests.
4. Remote transforms use a three-tick delayed interpolation buffer with integer fixed-point interpolation. Outside two known samples, presentation holds the nearest known state and does not extrapolate gameplay.
5. Local projectile feedback is a presentation track keyed by input sequence plus within-attack ordinal so multishot weapons remain unambiguous. An authoritative projectile snapshot may confirm and replace its identity/anchor. Presentation tracks have no collision, damage, health, reward, or mutation API.
6. Complete snapshot assembly precedes all reconciliation. Datagram chunk arrival order cannot expose partial world state.
7. Protocol `1.2` adds authoritative velocity and projectile source-input sequence. Exact-minor negotiation remains in force.

## Consequences

- Rendering remains smooth without weakening server finality.
- Prediction can be tested without Bevy, sockets, or wall-clock nondeterminism.
- M02-05 can drive the same runtime with controlled impairment rather than adding a second test-only client model.
