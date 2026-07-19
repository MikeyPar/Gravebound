# GB-M03-03G Caldus extraction activation evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-011` and `TECH-015`/`021`-`023` require successful extraction to commit before transfer, remain single-writer, survive response loss/reconnect, and replay without duplicate or lost authority.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-BOSS-001` fixes the post-reward Sir Caldus exit, stable exit identity, Hall destination, and ordinary successful-extraction semantics.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-08`, and the M03 exit gates require the complete private route, restart preservation, retry safety, no duplication, and scripted no-command journeys.

## Implemented boundary

Commit `3f749dd` reserves the exact committed `BossExitReady` route before the final PostgreSQL custody read, preserves the original storage version vector beside its wire projection, constructs the production extraction authority only from the committed Caldus exit, registers it in the shared extraction directory, and binds it to the existing private-life writer through the exact danger lease.

Fresh and replayed ownership is explicit at both the route-permit and extraction-actor layers. Only the fresh reservation owner may construct or roll back; exact replay can reuse only an already-registered identical actor. Altered version replay, early concurrent replay, stale binding, actor-construction failure, and session-binding failure cannot reopen the winning permit. Reservation abort retries while the owner remains live, reports a visible fault, and exits promptly when shutdown is already active.

The retained pending-inventory/`BossExitReady` publisher blocks danger-owner teardown while extraction is unresolved. Exact Hall installation or an exact competing-terminal cleanup releases that publisher and permits later microrealm teardown. Reconnect and `LinkLost` continue to use the existing generation-safe session/extraction handoff rather than creating another writer.

## Verification

- `cargo fmt --all`: pass.
- `cargo clippy -p server_app --all-targets --all-features -- -D warnings`: pass.
- `cargo test -p server_app --lib`: `360/360` pass.
- Focused real-QUIC/session activation proof passes exact registration, shared writer identity, early replay rejection, exact replay, altered inventory/character-version conflict, unresolved-terminal teardown rejection, exact loser cleanup, route reopening, and zero residue.
- Focused shutdown regression proves a persistent abort failure cannot hang after shutdown is already active.
- Focused terminal-completion regression proves retained publication changes from teardown-blocking to releasable only after the exact completion signal.
- `git diff --check`: pass before commit.

The focused composition test directly exercises the activation seam after setting an exact server-owned danger binding. It does not yet drive the complete automatic `CaldusRewardPending -> durable acknowledgement -> reserve -> hosted PostgreSQL snapshot -> retained publication -> activate` path. The timed all-server-target command exceeded the local execution window and is not claimed. Hosted PostgreSQL, complete automatic-worker real-QUIC response loss/reconnect/`LinkLost`, process restart, competing-terminal, and journey evidence remain open.

## Current Next Step

Drive activation through the automatic Caldus reward worker with the exact production pipeline and prove active transport, response loss, reconnect, transport-free `LinkLost`, competing terminal, terminal-completion release, shutdown, and zero residue. Then execute the hosted PostgreSQL/real-QUIC restart and adverse matrix before bound normal-server construction or admission.
