# GB-M03-03G same-task Caldus driver binding evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md` `SIM-004`, `DNG-006`, `SOC-010`, `TECH-012`, `TECH-015`, and `TECH-021` require one continuous server-owned danger clock, immutable boss eligibility, durable idempotency, and fail-closed handoffs.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-007`, `CONT-BOSS-001`/`002`, and `CONT-REWARD-003` fix B5 -> B6, Caldus staging/combat, the personal reward, and reward-gated exit order.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires the fixed Bell route and Sir Caldus to compose inside the ordinary persistent private loop without a developer-owned outcome.

Approved `SPEC-CONFLICT-023` defines the boss identity/reward/exit order. Approved `SPEC-CONFLICT-029` keeps later extraction inside the shared terminal barrier.

## Delivered contract

Commit `0cb44f0` consumes the completed B5 owner into `CorePrivateCaldusRuntime` inside the existing exclusive driver task.

- B6 conversion preserves the route lease, player and projectile allocations, inherited tick, ingress reducer, observer, task identity, and reconnect owner. Held movement/fire is neutralized at relocation while accepted sequence watermarks survive.
- The same 30 Hz task publishes bounded `CaldusRunning`, `CaldusRewardPending`, `CaldusTerminalPending`, and `CaldusExitReady` observations.
- `CaldusRewardPending` stops gameplay frames. Only the opaque durable resolution command can advance the owner; stale, changed, unavailable, or mismatched acknowledgements reject without faulting or advancing the route.
- The validated Caldus content object is pinned at the original Bell conversion. Reward acknowledgement cannot substitute caller-selected presentation content.
- A reusable PostgreSQL Caldus coordinator loader and authenticated reward-authority seam build owner commands from the frozen handoff, process progression revision, and wipeable authenticated account.
- Accepted B3 progression now reconciles its exact one-step version into the carried combat envelope before B4/B5. Exact acknowledgement replay is read-only, and the later Caldus handoff cannot submit a pre-B3 progression version.
- Normal route admission, automatic Caldus persistence execution, and production extraction binding remain disabled.

## Verification

Local Windows verification at exact source `0cb44f0`:

- Complete server library: `345 passed`, `0 failed`.
- Focused paused-time same-task Caldus driver: pass at exact first tick/route phase.
- Focused B3 -> B6 progression reconciliation and consuming handoff: pass.
- Strict `server_app` all-target, all-feature Clippy with warnings denied: pass.
- `cargo fmt --all` and `git diff --check`: pass.

## Explicit boundary

The PostgreSQL reward coordinator still performs personal item, progression, and exit writes as separately replayable subterminals. Before the automatic session executor or production extraction actor can be enabled, their finalization must verify that the exact danger lineage/restore root remains active and no death, Recall, extraction, disconnect-recovery, or server-fault terminal has won.

## Current Next Step

Commit `3f749dd` now constructs/registers production extraction only from the committed `BossExitReady` exit and coherent storage versions, binds it through the exact danger lease, and closes replay-owned rollback and terminal-completion teardown hazards; see [activation evidence](GB-M03-03G-caldus-extraction-activation-evidence.md). Next prove the complete automatic-worker active/reconnect/transport-free-`LinkLost`/competing-terminal lifecycle, then execute hosted adverse/restart evidence. Keep normal admission disabled.
