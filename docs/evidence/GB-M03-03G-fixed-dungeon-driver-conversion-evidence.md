# GB-M03-03G fixed-dungeon driver conversion evidence

**Status:** Commit `a2a5b09` is green under hosted CI [`29645411895`](https://github.com/MikeyPar/Gravebound/actions/runs/29645411895). Follow-on live fixed-room evidence is recorded separately. Normal route admission remains disabled.

## Three-authority basis

1. `Gravebound_Production_GDD_v1_Canonical.md` `SIM-004`, `DNG-003`, `DNG-005`, `TECH-012`, and `TECH-015` require one server-owned 30 Hz simulation domain, explicit instance transfer, fail-closed room authority, and reconnect to the same live state.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-007` fixes the M03 Bell route to `B0 -> B1 -> B2 -> B3 -> B4 -> B5 -> B6`; no seeded or alternate layout may be admitted.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires Character Select -> Hall -> microrealm -> six-room dungeon -> boss -> Hall as one private, authoritative route without developer commands.

## Implemented contract

- The existing session-owned driver pauses only between committed microrealm frames and publishes `BellResolutionPending` before durable Bell work begins.
- Known rejection explicitly aborts and restores the exact prior runtime and observer state. A dropped decision is treated as an unknown durable outcome: ingress and simulation remain frozen until restart/receipt reconciliation rather than risking progress in the wrong world.
- A committed or replayed Bell transition is consumed inside the existing driver task. The task converts its sole `CorePrivateMicrorealmRuntime` into `CorePrivateFixedDungeonRuntime`; it never returns the mutable allocation through a caller-owned join and never creates a second driver.
- Dropping the conversion acknowledgement cannot cancel or detach the conversion. The original observer publishes the immutable transfer ID, final microrealm tick, route lease, and `BellVestibuleB0` ownership.
- The session retains the same binding lease, driver task, observer channel, and shared reliable writer across transport replacement. Reconnect neither reconstructs combat state nor creates a second authority.
- This conversion commit stops at B0. Follow-on commits `b50a8c0` and `09c9c9e` add server-generated live room frames inside the same task under [`GB-M03-03G-live-fixed-room-driver-evidence.md`](GB-M03-03G-live-fixed-room-driver-evidence.md); neither change opens normal admission.

## Verification

- `16/16` focused microrealm/driver tests pass. They cover exact 30 Hz ownership, retained input, reliable-action sequencing, LinkLost neutralization, frame-boundary abort, unknown-outcome freeze, dropped conversion acknowledgement, terminal/fault freeze, and zero-residue joined shutdown.
- A focused real-QUIC session test replaces the transport while Bell resolution is paused, commits the route transition, converts in place, and proves both pre- and post-reconnect observers see the same binding and fixed-dungeon readiness with one retained session driver.
- The complete server matrix passes `324/324` library tests plus every enabled binary, integration, and doc target. Tests requiring the explicitly gated disposable PostgreSQL stack or long soak profile remain ignored by their existing gates.
- `cargo clippy -p server_app --all-targets --all-features -- -D warnings`, `cargo fmt --check`, and `git diff --check` pass.
- Hosted CI [`29645411895`](https://github.com/MikeyPar/Gravebound/actions/runs/29645411895) is green for exact commit `a2a5b09`.

## Explicit boundary

The conversion itself stops at B0; the follow-on live-driver slice now synthesizes fixed-room frames. Durable B4 Bargain resolution, Sir Caldus combat, room/boss reward commits, pending inventory, stable B6 exit, all five terminal producers, ordinary native admission, restart journeys, and optimized native visual evidence remain open.

## Current Next Step

Bind the committed/replayed durable B4 Bargain result into the same task before constructing Sir Caldus, committed reward/pending-inventory authority, the stable B6 exit, and terminal composition.
