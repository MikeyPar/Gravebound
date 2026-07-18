# GB-M03-03G live Slipstep evidence

## Authority

This slice was reviewed against the three governing documents together:

1. `Gravebound_Production_GDD_v1_Canonical.md` `COM-001`-`006`, `MOV-001`-`003`, `TECH-010`-`023`, and the approved `slip_clasp` exception require server-owned movement, exact Slipstep timing/distance/damage reduction, and collision-safe action results.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-WORLD-001`, `CONT-WORLD-004`, and `CONT-ROOM-007` require the Core microrealm to use the compiled shell, spawn, movement class, equipment authority, and Bell encounter geometry.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires live action/combat composition on the ordinary private route while keeping admission disabled until the independent driver and cumulative evidence pass.

## Delivered contract

Commits `f75c662`, `8d74ca7`, and `3c6af83` compose Slipstep into the live microrealm frame without enabling normal admission.

- `simulation_to_tile_point` is the single checked projection from finite simulation coordinates to shared fixed-point route coordinates. Non-finite and out-of-range states fail closed.
- Forced movement now repairs only a reported floating-point contact back to the legal solid boundary, reports distance from the actual final position, and revalidates the authoritative arena before commit.
- `microrealm_combat_arena` materializes the exact compiled one-tile world shell as four non-overlapping solids. The test fixture proves the avatar center stops at `x=1300` millitiles for the compiled `0.3`-tile radius and cannot enter `x=1299`.
- `CorePrivateMicrorealmRuntime` owns `PlayerMovementState` and calls the existing `PlayerCombatState::step_with_movement_outcome` in the staged frame. Movement, Slipstep, damage reduction, player target position, hostile combat, lifecycle, and route compare-and-swap therefore succeed or roll back together.
- Runtime construction rejects any drift between the compiled scene and combat arena width, height, shell, spawn, or unexpected interior scene solids.
- The exact equipment-derived movement speed and compiled ability values, including `slip_clasp`, remain content/simulation authority; ingress cannot provide displacement, collision, damage reduction, or route position.

## Verification

Local Windows verification at `3c6af83`:

- `cargo test -p sim_core --lib`: `388 passed`, `0 failed`.
- `cargo test -p sim_content --lib`: `126 passed`, `0 failed`.
- `cargo test -p server_app --lib`: `310 passed`, `0 failed`.
- Focused movement tests: `14 passed`; focused Core pack tests: `11 passed`; focused live runtime tests: `6 passed`.
- `cargo clippy -p sim_core -p sim_content -p server_app --all-targets --all-features -- -D warnings`: pass.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- The shell-contact regression proves the same combat frame reports movement collision, a collided Slipstep outcome, the exact legal boundary, and one matching hostile target position.

Hosted CI [`29640683239`](https://github.com/MikeyPar/Gravebound/actions/runs/29640683239) was still running when this record was written and is not claimed green before completion.

## Current Next Step

Move input-sequence validation to session ingress and place the runtime behind one exclusive driver with a one-slot latest-state input channel, separately reliable ability presses, a `MissedTickBehavior::Skip` 30 Hz interval, LinkLost neutralization with continued danger ticks, generation-safe reconnect, frame-complete freeze/shutdown, and zero residue. Then extend the same mutable handoff through fixed B0-B6 rooms, rewards, pending inventory, and all terminal producers.
