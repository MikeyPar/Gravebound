# GB-M03-03G live microrealm combat evidence

## Authority

This slice was reviewed against all three governing documents together:

1. `Gravebound_Production_GDD_v1_Canonical.md` `LOOP-001`-`003`, `COM-001`-`006`, and `TECH-010`-`023`: the server owns simulation, collision, damage, enemy defeat, and route outcomes; input communicates actions rather than results.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-WORLD-001`, `CONT-WORLD-004`, `CONT-ENEMY-001`-`002`, and `CONT-ROOM-007`: the Core microrealm uses the exact compiled geometry and `pack.bell.01` composition, warning, activation, reset, and clear contracts.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`: the normal private route must compose live server authority without developer commands and remain disabled until its cumulative evidence gates pass.

## Delivered contract

Commits `302ccb3`, `f07a282`, and `587924e` establish the live combat ownership seam without enabling normal admission.

- `CoreMicrorealmPackCombat` is a lifecycle-free capacity-one owner of exactly one participant handoff or one instantiated Bell wave. Lifecycle events may prepare, reset, or clear it, but client input cannot create those events.
- Pack construction preserves the exact compiled eight-enemy order, activation boundary, run-local entity identities, projectile allocator, reset cleanup, and non-reused spawn ordinals.
- `CoreCharacterCombat::into_live_player` moves the only mutable health, Belt, Bell Debt, cooldown, and projectile state into the scene participant. An immutable/versioned envelope rejoins only the same entity with unchanged armor, resistance, immunity, and maximum-health axes.
- `CorePrivateMicrorealmInput` contains only monotonic input sequence and bounded movement, aim, primary, Grave Mark, and Slipstep action state. It contains no tick, displacement, collision, damage, clear, route phase, or Bell-range field.
- Each invoked frame allocates the next run-local server tick, derives equipment-speed displacement, advances player combat, constructs collision from the compiled arena and live hostile hurtboxes, advances the hostile wave, and derives lifecycle clear only from the exact authoritative wave-clear tick.
- The real primary `ShotEvent`, not an ingress boolean, supplies the lifecycle's first-release trigger. The clear proof remains crate-private and is minted only when the lifecycle consumes an exact wave clear.
- Movement, combat, lifecycle, and pack state are cloned into one staged frame. The shared route actor then performs one expected-version phase/range compare-and-swap; local state is replaced only after that command commits.
- An active wave cannot hand off its mutable participant. Quiet and cleared handoffs rejoin the original character combat allocation.
- Slipstep movement is deliberately fail closed until its movement result is composed with scene collision. The independent 30 Hz session scheduler is also still open; this slice owns tick values per invoked frame but does not claim that network input cadence is the production clock.

## Verification

Local Windows verification at `587924e`:

- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p sim_content -p server_app --all-targets --all-features -- -D warnings`: pass.
- `cargo test -p sim_content --lib`: `125 passed`, `0 failed`.
- `cargo test -p server_app --lib`: `309 passed`, `0 failed`.
- Focused pack tests: `10 passed`, covering exact construction, authoritative clear, atomic reset, spawn-ordinal advance, and quiet/active handoff rules.
- Focused live runtime tests: `5 passed`, covering server-generated tick/displacement, real-shot lifecycle entry, exact warning/wave construction, stale-route rollback, unsupported Slipstep rollback, and combat rejoin.
- The existing real-QUIC session test remains green and proves the retained live owner survives authoritative handoff, `LinkLost`, and reconnect without a second allocation.

Hosted CI runs for the three implementation commits were not yet complete as [`29639877086`](https://github.com/MikeyPar/Gravebound/actions/runs/29639877086), [`29639885732`](https://github.com/MikeyPar/Gravebound/actions/runs/29639885732), and [`29639908490`](https://github.com/MikeyPar/Gravebound/actions/runs/29639908490) when this record was written. This evidence does not call them green before completion.

## Current Next Step

Drive the retained action-only microrealm owner from the session's independent 30 Hz scheduler using bounded latest-state input so ticks, hostile simulation, and danger continue without packet cadence authority. Compose Slipstep displacement through the same collision transaction, then extend the single mutable combat handoff through fixed B0-B6 rooms, rewards, pending inventory, and all five terminal producers before enabling normal admission.
