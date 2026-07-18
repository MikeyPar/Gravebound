# GB-M03-03G live fixed-room driver evidence

**Status:** Local implementation evidence accepted for commits `b50a8c0` and `09c9c9e`. Commit `b50a8c0` is green under hosted CI [`29646020543`](https://github.com/MikeyPar/Gravebound/actions/runs/29646020543); run [`29646920076`](https://github.com/MikeyPar/Gravebound/actions/runs/29646920076) for `09c9c9e` is in progress and is not claimed green. Normal route admission remains disabled.

## Three-authority basis

1. `Gravebound_Production_GDD_v1_Canonical.md` `SIM-004`, `DNG-003`, `DNG-005`, `TECH-012`, and `TECH-015` require server-owned 30 Hz action processing, exact room lifecycle/cleanup, one terminal result, and reconnect to the same live state.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-007` fixes the route to `B0 -> B1 -> B2 -> B3 -> B4 -> B5 -> B6`; clients cannot name a destination or skip a node.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires the private microrealm, fixed dungeon, boss, and Hall return to compose without developer commands while preserving authoritative reconnect and cleanup.

## Implemented contract

- `CorePrivateFixedDungeonRuntime::step_live_room` stages movement, player combat, room combat/lifecycle, persistent route compare-and-swap, and tick advancement as one transaction. Local state changes only after route authority commits.
- B1, B2, B3, and B5 expose the one carried player and only legally active hostile hurtboxes. B2/B3 spawn warnings no longer expose early damage targets.
- The Bell conversion tail-calls the fixed-dungeon loop inside the original session-owned task. No second task, mutable runtime clone, observer, route writer, or reliable writer is created.
- The fixed control request contains no node, room, phase, position, or destination. The runtime selects the next canonical transition and rejects early, unresolved-B4, B6, stale-route, or impossible requests without client-authored authority.
- Authoritative room relocation clears held movement and primary fire while retaining aim and all input/action sequence watermarks. A Bell-held action cannot bleed into B1, and an older reconnect frame cannot replay.
- The same observer publishes B0/non-frame readiness, live fixed-room frames, exact lethal freeze, and fail-closed faults. The same transport-independent binding survives real-QUIC replacement; stale transports cannot advance the route.
- A dropped conversion acknowledgement cannot undo ownership. Premature control before conversion or while Bell resolution is pending receives a bounded typed rejection rather than hanging or advancing after the fact.

## Verification

- `9/9` paused-time driver tests pass, including independent 30 Hz ownership, retained/reliable sequencing, `LinkLost`, pre-conversion control rejection, Bell-pending rejection, exact abort, unknown-outcome freeze, lethal/fault freeze, frame-complete shutdown, and zero residue.
- The conversion trace carries a real combat allocation through microrealm tick `32`, neutralizes held movement/fire at B0 -> B1, publishes the first fixed frame at tick `33`, rejects an early B1 advance without faulting, publishes tick `34`, and joins the one task cleanly.
- The real-QUIC session trace replaces the transport during Bell resolution, retains the same binding/observer, rejects the stale generation, accepts the current destination-free advance, and observes B1 tick `33` on both pre- and post-reconnect observers.
- `131/131` `sim_content` library tests pass.
- `326/326` `server_app` library tests pass, along with every enabled binary, integration, and doc target. Existing PostgreSQL and long-soak tests remain behind their explicit disposable-environment gates.
- `cargo fmt --check`, `git diff --check`, and strict all-target/all-feature Clippy for `sim_content` and `server_app` pass.
- Hosted CI [`29646020543`](https://github.com/MikeyPar/Gravebound/actions/runs/29646020543) is green for the authoritative live-frame primitive at exact commit `b50a8c0`; the session-task integration run remains pending.

## Presentation candidate

Commit `a6fb62e` adds the unregistered [`Open`/`Selected`/`Refused` B4 shrine-state pack](../../assets/core/dungeons/bell_bargain_state_review/v1/README.md). The deterministic rebuild reproduces twelve manifest hashes; 96px readability plus 1280x720 and 1920x1080 standard/reduced review mocks pass visual inspection. It remains outside registries and content hashes until a stored durable B4 projection and optimized native review exist.

## Explicit boundary

The driver can traverse live combat rooms only when their server-owned lifecycle permits an advance. It does not yet project the committed/replayed Bargain transaction into B4, construct Sir Caldus at B6, commit room/boss rewards or pending inventory, expose the stable exit, arbitrate all terminal producers, or enable ordinary native admission.

## Current Next Step

Bind the existing durable Bargain transaction to B4 inside the same task. Exact replay must return the stored result; changed material must conflict; dropped/unknown outcomes must remain frozen; only the stored `Selected`, `Refused`, or authoritative no-offer result may permit B4 -> B5. Then construct Sir Caldus, durable rewards/pending inventory, the stable B6 exit, and terminal composition.
