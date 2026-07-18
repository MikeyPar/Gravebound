# GB-M03-03G durable B3 reward-authority evidence

**Status:** Local implementation is accepted through automatic-runtime commit `3f4ecaf`. Normal route admission remains disabled. Production PostgreSQL coordinator/secret composition, hosted inactivity and restart execution, end-to-end response-loss proof, and hosted cumulative CI remain required before this slice can close the parent route.

## Three-authority basis

1. `Gravebound_Production_GDD_v1_Canonical.md` `PROG-003`, `BRG-001`-`005`, `DNG-005`, and `SOC-010` require server-owned miniboss XP/loot eligibility, an exact eight-tick normal reward delay, a temporary level-five Core Bargain, presence/contribution/inactivity/life/Recall/trust checks, and replay-safe life persistence.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-014`, `CONT-REWARD-003`-`004`, `CONT-ROOM-007`, and `CONT-ENEMY-003` bind the Sepulcher Knight to B3, `reward.miniboss_t1`, `xp.miniboss_t1`, and the B3-before-B4 fixed route.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`-`05` require the reward, progression, Bargain, reconnect, and persistent private-route authorities to compose without client-authored outcomes or duplicate grants.

## Implemented contract

- `PostgresCoreB3RewardCoordinator` consumes only the immutable handoff from the sole B3 simulation owner. It derives domain-separated reward/source identities and rejects early delivery, wrong content/profile/reference health, foreign authority, invalid delay, and structurally impossible evidence. SOC-010 presence, contribution, inactivity, life, Recall, and session-trust failures instead commit the progression-owned `NotEligible` terminal and return an opaque `Ineligible` proof with no item, XP, Bargain offer, or fabricated no-offer milestone.
- Progression and the optional Bargain milestone commit before loot. The progression command is constructed while its aggregate locks are held, eliminating a stale-version window; an ineligible participant cannot leave personal items behind.
- Migration `0064_b3_no_offer_disposition_v1.sql` adds result code `3` for a durable below-level or already-consumed `NoOffer` disposition. Its partial unique index reserves once-per-life uniqueness only for an actually earned milestone, so a nonqualifying B3 receipt cannot consume a later legal level-five trigger.
- Exact replay reconstructs the stored progression/milestone and reward authorities. Changed payloads, stale/foreign ownership, or malformed stored projections fail closed.
- The fixed-dungeon task freezes at the exact B3 reward-due handoff, rejects normal advance, and resumes only after an opaque account/character/lineage-bound durable resolution is acknowledged. Exact acknowledgement suppresses the pending handoff permanently instead of re-entering the reward freeze on the next frame. `GrantedNoOffer` and `IneligibleNoOffer` enter B4 already resolved as authoritative `NoOffer`; `GrantedOffer` retains the ordinary selection/refusal path.
- Reward participation is driver-owned. Production-cadence no-op packets do not count as activity; movement, aim/primary edges, held movement/primary, and reliable abilities do. `LinkLost` marks the participant absent and session-invalid while danger ticks continue. Reconnect restores session authority under the same generation lock, so an old detach cannot overwrite a replacement transport.
- `CorePrivateB3RewardRuntime` follows the route binding rather than a QUIC connection. It observes the immutable pending frame, retries transient PostgreSQL failures with bounded shutdown-aware backoff, acknowledges the opaque durable result through the same single-writer driver task, and only then constructs reliable publication.
- Granted publication emits the existing progression event immediately before the existing route-state event; ineligible publication emits only the route result. The runtime retains that exact publication across transport loss, replays it once to every newer writer generation, ignores stale-generation detach, and joins before its driver owner shuts down. Session construction remains opt-in until the production coordinator and process-bound secret epoch are composed and proven.

## Verification

- `cargo check -p persistence -p sim_content -p server_app --all-targets --all-features`: pass.
- Strict `cargo clippy -p persistence -p sim_content -p server_app --all-targets --all-features -- -D warnings`: pass.
- `cargo test --workspace --all-features`: pass, including `333/333` server, `243/243` persistence, `131/131` sim-content, `193/193` native-client, and `388/388` simulation-core library tests plus enabled integration and documentation tests.
- Focused proof passes for the 605-frame no-op cadence, `LinkLost`/reconnect projection, real-QUIC private-life handoff lifecycle, B3 handoff evidence, exact eight-tick validation, structurally malformed rejection, durable eligible/ineligible replay, foreign-lineage rejection, no-offer schema projection, acknowledged-pending suppression, and B4-to-B5 gating.
- Commit `83c75a5` passes strict workspace all-target/all-feature Clippy and every workspace test. The hosted PostgreSQL test definition asserts an ineligible progression receipt with zero reward-request, item, and Bargain-milestone rows; local execution remains honestly unclaimed because `TEST_DATABASE_URL` is absent.
- Independent review found no remaining P0/P1 blocker after the activity-cadence and detach/reconnect ordering corrections.
- `git diff --check` passes. `cargo build --release -p server_app -p client_bevy` completed successfully after the command wrapper returned; both optimized Windows executables have fresh build timestamps.
- The hosted PostgreSQL test compiles and covers commit/replay, restart reconstruction, item provenance/security/location, and below-level no-offer. `TEST_DATABASE_URL` was unavailable locally, so hosted execution remains explicitly open.
- Commit `3f4ecaf` passes two focused automatic-runtime tests: transport-free transient retry followed by one same-task acknowledgement, and real-QUIC granted progression/route publication with exact contiguous replay across two writer generations plus stale-detach protection.
- `cargo check -p server_app --all-targets --all-features`, all four private-life session real-QUIC lifecycle tests, `cargo test --workspace --all-targets --all-features`, and strict workspace all-target/all-feature Clippy pass for `3f4ecaf`.

## Evidence strengthening still open

- Drive more than 600 no-op frames through the actual fixed-dungeon B3 runtime and coordinator, then prove the durable `NotEligible` resolution and zero reward rows in hosted PostgreSQL.
- Inject the real PostgreSQL coordinator and process-bound reward secret into production route construction, then force response loss, reconnect, and process restart through the full session boundary. The isolated automatic runtime already proves real-QUIC writer replacement and the formerly vulnerable stale-detach interleaving.
- Add a distinct anti-cheat invalidation transition if M03 introduces an anti-cheat authority separate from authenticated session/input validity.

## Current Next Step

Inject `PostgresCoreB3RewardCoordinator` with a process-bound `SecretRewardEpoch` at normal-route construction, then prove the integrated inactivity zero-row case and exact response-loss/reconnect/process-restart convergence against hosted PostgreSQL. After that evidence is green, implement the B5 bridge and authoritative Sir Caldus B6 route.
