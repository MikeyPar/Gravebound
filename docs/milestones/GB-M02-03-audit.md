# GB-M02-03 completion audit

## Result

PASS. The native client now predicts only local movement and immediate local projectile presentation, then reconciles both to authoritative protocol `1.2` snapshots. The server remains final for position, collision, hits, health, death, eligibility, rewards, inventory, and item grants.

## Authority review

| Authority | Evidence |
|---|---|
| Canonical GDD | `SIM-004` input is replayed through the shared fixed-step movement model. `SIM-010`–`SIM-012` authority remains server-side. `TECH-011`–`TECH-014` drive snapshot cadence, interpolation delay, prediction scope, and exact correction thresholds. |
| Content Production Specification | Presentation consumes the validated immutable `fp.1.0.0` simulation values already compiled by `sim_content`; the client prediction layer does not copy or invent content values. |
| Development Roadmap | The `GB-M02-03` package supplies local movement prediction, remote interpolation, reconciliation, and deterministic projectile presentation while leaving lifecycle and impairment work to their named packages. |

## Implementation evidence

| Requirement | Evidence |
|---|---|
| Snapshot assembly | `SnapshotAssembler` accepts out-of-order chunks only after complete, consistent metadata is present; duplicate chunks, duplicate entity IDs, stale sequences, invalid counts, and more than four pending sequences fail closed or are bounded. |
| Local prediction | `LocalMovementPrediction` stores at most 256 strictly increasing inputs and steps the existing `sim_core::PlayerMovementState` at 30 Hz. |
| Reconciliation | Each snapshot rebuilds from authoritative position and velocity, removes acknowledged input, and deterministically replays the remaining history before committing. Presentation correction never mutates the resulting simulation state. |
| Exact thresholds | Errors below `0.10` tile blend for `100 ms`; errors from `0.10` through `0.35` tile blend for `60 ms` with a debug metric; errors above `0.35` tile snap with warning and anomaly signal. Boundary tests pin `0`, `0.099`, `0.100`, `0.350`, and `0.351` tile. |
| Death finality | Authoritative death is applied immediately, clears pending prediction, and prevents later local prediction until a new authoritative runtime is established. |
| Remote presentation | Remote samples interpolate at the exact three-tick/`100 ms` delay with bounded integer arithmetic and hold at known endpoints instead of extrapolating gameplay state. |
| Projectiles | Local projectiles appear immediately under `(input_sequence, projectile_ordinal)`, expire unconfirmed after `250 ms`, converge to server entity/position/velocity facts, and retire only from authoritative absence or acknowledgement. No presentation API can report a hit or mutate gameplay. |
| Bevy integration | Update systems consume completed snapshots, synchronize remote entity sprites, apply local correction offsets, interpolate remote visuals, reconcile projectile visuals, and expose bounded correction diagnostics. They remain dormant in LocalLab unless a network presentation resource is installed. |

## Protocol decision

Protocol minor `1.2` adds authoritative entity velocity plus friendly-projectile source input sequence and within-attack ordinal. Exact minor matching remains mandatory; no compatibility adapter is claimed. The canonical codec fixture is repinned for the intentional version change.

## Verification

| Gate | Result |
|---|---|
| Focused prediction suite | PASS — nine pure/runtime/Bevy tests, including exact correction boundaries, replay, death finality, interpolation, projectile convergence, and ECS integration |
| Networking CI | PASS — protocol/server/bot tests, warnings-denied Clippy, QUIC integration, and doctor commands |
| Full workspace CI | PASS — 331 tests, format, strict all-target warnings-denied Clippy, content validation, and two byte-identical deterministic traces |
| Worktree diff check | PASS before commit |

## Deferred without waiver

- Join, leave, timeout, three-second `LinkLost`, reconnect, duplicate-session handoff, and shutdown: `GB-M02-04`.
- Deterministic latency, jitter, loss, duplication, reordering, and outage injection: `GB-M02-05`.
- Comprehensive malicious input and mutation rejection: `GB-M02-06`.
- Full journey bot and instance lifecycle: `GB-M02-07`/`GB-M02-08`.
- Four-human, sixteen-bot, impairment-playability, and server tick percentile gates: full M02 exit gate.

## Handoff

`GB-M02-04` must build connection lifecycle around protocol `1.2` without converting presentation state into authority. The exact three-second `LinkLost` behavior, reconnect identity rules, duplicate-session handoff, and clean teardown require explicit deterministic tests before this plan advances again.
