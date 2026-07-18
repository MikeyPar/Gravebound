# GB-M03-03G live microrealm session evidence

## Authority

This slice is governed together by all three design documents:

1. `Gravebound_Production_GDD_v1_Canonical.md` `TECH-015`: a dropped connection enters `LinkLost` while the character remains vulnerable, reconnect resolves against committed terminal authority, and duplicate transport invalidation occurs only after authoritative handoff.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-WORLD-001`: `world.core_microrealm_01` owns the exact capacity-one Core microrealm lifecycle, clear state, reset behavior, and Bell eligibility.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`: the M03 route must compose Character Select, Hall, microrealm, fixed dungeon, boss, and Hall return with explicit transfers and reconnect-safe ownership.

## Delivered contract

Commit `e5e1a9b` makes the live microrealm owner part of the persistent private-life session rather than a transport-local allocation.

- One account session may bind one `CorePrivateMicrorealmRuntime` and its exact generation-pinned route lease.
- A winning transport handoff receives the same `Arc` before session visibility; no second microrealm owner is allocated.
- The replaced transport cannot obtain or unbind authority.
- `LinkLost` transport detach preserves the runtime so the server can continue danger simulation while the player is disconnected.
- Reconnect receives the same runtime allocation and authoritative state.
- Terminal or transfer retirement requires the exact route lease before unbinding; unbind does not retire the session's shared reliable writer.
- Shutdown clears retained runtime ownership and reports the remaining binding count as part of the zero-residue contract.
- The session never awaits the microrealm mutex while holding its own directory lock. It stores the immutable route lease beside the runtime, preserving a single lock order.

Normal route admission remains disabled. Follow-on commits `302ccb3`, `f07a282`, and `587924e` now provide the lifecycle-free Bell pack owner, exact mutable combat handoff, and action-only server-generated combat frames recorded in [`GB-M03-03G-live-microrealm-combat-evidence.md`](GB-M03-03G-live-microrealm-combat-evidence.md). The independent 30 Hz session driver, Slipstep collision, fixed-room combat, rewards, pending inventory, and complete terminal-producer composition remain open.

## Verification

Local verification on Windows at source `e5e1a9b`:

- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p server_app --all-targets -- -D warnings`: pass.
- `cargo test -p server_app --all-targets`: pass.
- Server library: `306 passed`, `0 failed`.
- Enabled server integration targets: pass; tests requiring explicitly authorized disposable PostgreSQL or long-form soak execution remain ignored by their existing gates.
- Focused real-QUIC handoff test: pass.

The focused proof binds a live compiled microrealm, replaces its transport, rejects the stale generation, detaches into `LinkLost`, reconnects to the same allocation by pointer identity, performs exact route-lease unbind, and closes both session and route directories with zero residue.

## Current Next Step

Drive the retained action-only owner from the session's independent 30 Hz scheduler using bounded latest-state input, compose Slipstep through the same collision transaction, then extend the single combat handoff through fixed B0-B6 rooms, rewards, pending inventory, and all five terminal producers before enabling normal admission.
