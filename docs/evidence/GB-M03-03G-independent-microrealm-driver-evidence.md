# GB-M03-03G independent microrealm driver evidence

## Authority

This slice was reviewed against all three governing documents together:

1. `Gravebound_Production_GDD_v1_Canonical.md` `LOOP-001`-`003`, `COM-001`-`006`, and `TECH-010`-`023` require a fixed 30 Hz server simulation, action-only ingress, reliable ability presses, generation-safe reconnect, and no client-authored outcomes.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-WORLD-001`, `CONT-WORLD-004`, `CONT-ENEMY-001`-`002`, and `CONT-ROOM-007` fix the capacity-one Core microrealm, movement/combat inputs, Bell pack, and later B0-B6 handoff.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires the ordinary private loop to continue authoritatively through disconnect, reconnect, terminal competition, and cleanup without developer-command or packet-cadence authority.

## Delivered contract

Commits `c3bc57e`, `995151c`, and `4516b1a` replace the retained shared runtime mutex with one exclusive driver and keep normal admission disabled.

- Runtime input sequence is now an acknowledgement value. Session ingress validates and coalesces transport sequences; one accepted state may drive many server-owned frames.
- `CorePrivateMicrorealmDriver` is the only mutable runtime owner. A Tokio interval runs at 30 Hz with `MissedTickBehavior::Skip`; it never catches up with simulation bursts and never owns the reliable network writer.
- Continuous movement, aim, held primary, and primary sequence use a one-slot latest-state reducer. Released legacy frames retain the maximum accepted primary sequence. Ability 1 and Ability 2 presses use a separate reliable monotonic action sequence and advance their server sequences exactly once.
- `LinkLost` neutralizes movement and held primary while retaining aim and already accepted presses. The same driver and committed tick stream continue until reconnect, a lethal frame, a fatal authority fault, or planned shutdown.
- Each completed frame publishes one bounded read-only observation. Lethal and fatal-fault frames freeze exactly; no later frame runs. Shutdown is checked between frames and joins rather than aborting an in-flight route compare-and-swap.
- The private-life session exposes observer-only bindings. Every input/action enqueue is linearized with the current transport generation; stale transports cannot retain an ingress handle or race detach neutralization.
- Reconnect receives the same account/character/actor/binding lease and observer. Terminal retirement uses that transport-independent lease after `LinkLost`; it does not require a live network transport and does not retire the shared reliable writer.
- Planned shutdown joins every driver, records join failures, and includes remaining bindings in the existing zero-residue result.

## Verification

Local Windows verification at `4516b1a`:

- Paused-time driver tests: `5 passed`, covering exactly 30 committed frames per simulated second, newest-state coalescing, legacy release normalization, reliable presses, `LinkLost` danger ticks, lethal freeze, fatal route fault, and frame-complete shutdown.
- Real-QUIC session lifecycle: pass, covering transport handoff, stale input rejection, detach neutralization, same binding on reconnect, retirement without an active transport, joined task, and zero residue.
- Live runtime tests: `6 passed`, including repeated retained input, same-frame Slipstep/collision, and route rollback.
- `cargo test -p server_app --lib`: `315 passed`, `0 failed`.
- `cargo clippy -p server_app --all-targets --all-features -- -D warnings`: pass.
- `cargo fmt --all -- --check` and `git diff --check`: pass.

Hosted CI [`29641751122`](https://github.com/MikeyPar/Gravebound/actions/runs/29641751122) was queued when this record was written and is not claimed green before completion. Earlier cumulative Slipstep, landmark, and documentation sources are green under runs [`29640683239`](https://github.com/MikeyPar/Gravebound/actions/runs/29640683239), [`29640707913`](https://github.com/MikeyPar/Gravebound/actions/runs/29640707913), and [`29640811604`](https://github.com/MikeyPar/Gravebound/actions/runs/29640811604).

## Explicit boundary

This driver publishes `TerminalPending` or a fault; it does not claim durable death, automatic Recall commitment, extraction, reward persistence, or pending-inventory authority. Commits `42d9f85`, `9694790`, and `6749141` now supply the lifecycle-free fixed-room traversal owner and atomic persistent route binding, but the driver still must transform that owner inside the same task. The shared five-producer terminal coordinator and one authoritative tick domain remain required before normal admission.

## Current Next Step

Transform the exact cleared-microrealm allocation into `CorePrivateFixedDungeonRuntime` inside this exclusive task after the durable Bell result, keeping one observer and retained-input owner through cancellation/reconnect. Then derive live B1-B5 movement/combat frames, persist B4, and add Sir Caldus, committed rewards, pending inventory, stable B6 exit, and all five terminal producers. Keep Character Select `Play`, production Realm Gate interaction, and ordinary extraction/Recall admission disabled until cumulative real-QUIC restart, 25-journey, performance, cleanup, and visual evidence pass.
