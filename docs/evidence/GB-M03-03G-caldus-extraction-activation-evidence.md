# GB-M03-03G Caldus extraction activation evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-011` and `TECH-015`/`021`-`023` require successful extraction to commit before transfer, remain single-writer, survive response loss/reconnect, and replay without duplicate or lost authority.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-BOSS-001` fixes the post-reward Sir Caldus exit, stable exit identity, Hall destination, and ordinary successful-extraction semantics.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-08`, and the M03 exit gates require the complete private route, restart preservation, retry safety, no duplication, and scripted no-command journeys.

## Implemented boundary

Commit `3f749dd` reserves the exact committed `BossExitReady` route before the final PostgreSQL custody read, preserves the original storage version vector beside its wire projection, constructs the production extraction authority only from the committed Caldus exit, registers it in the shared extraction directory, and binds it to the existing private-life writer through the exact danger lease.

Commit `e7905ea` extracts the worker's complete post-acknowledgement path into one auditable boundary and proves that reservation reaches `TerminalPending` before the custody authority is called, then coherent snapshot projection, retained publication construction, and activation occur in canonical order.

Fresh and replayed ownership is explicit at both the route-permit and extraction-actor layers. Only the fresh reservation owner may construct or roll back; exact replay can reuse only an already-registered identical actor. Altered version replay, early concurrent replay, stale binding, actor-construction failure, and session-binding failure cannot reopen the winning permit. Reservation abort retries while the owner remains live, reports a visible fault, and exits promptly when shutdown is already active.

The retained pending-inventory/`BossExitReady` publisher blocks danger-owner teardown while extraction is unresolved. Exact Hall installation or an exact competing-terminal cleanup releases that publisher and permits later microrealm teardown. Reconnect and `LinkLost` continue to use the existing generation-safe session/extraction handoff rather than creating another writer.

## Verification

- `cargo fmt --all`: pass.
- `cargo clippy -p server_app --all-targets --all-features -- -D warnings`: pass.
- `cargo test -p server_app --lib`: `361/361` pass.
- Focused real-QUIC/session activation proof passes exact registration, shared writer identity, early replay rejection, exact replay, altered inventory/character-version conflict, unresolved-terminal teardown rejection, exact loser cleanup, route reopening, and zero residue.
- Focused shutdown regression proves a persistent abort failure cannot hang after shutdown is already active.
- Focused terminal-completion regression proves retained publication changes from teardown-blocking to releasable only after the exact completion signal.
- `git diff --check`: pass before commit.

The focused composition test directly exercises the activation seam after setting an exact server-owned danger binding, and the automatic-order test exercises the production post-acknowledgement function itself. The full local worker now drives `CaldusRewardPending -> durable resolution -> acknowledgement` into that path and proves retained QUIC replay; the session-composition test separately proves the exact extraction actor/writer across reconnect and `LinkLost`. The timed all-server-target command exceeded the local execution window and is not claimed. Hosted PostgreSQL, process restart, production-session adverse coordination, and journey evidence remain open.

Commit `e62587b` extends the real-QUIC composition proof across two `LinkLost` boundaries. The uncommitted extraction actor remains registered without a transport, the next authenticated connection receives the exact microrealm and extraction leases, and its extraction publisher resolves to the same shared reliable writer. Exact transport-free retirement then aborts the uncommitted actor, reopens the reserved route, and a third authenticated connection proves that the retired extraction actor does not resurrect while the continuing microrealm binding remains valid. Strict server all-target/all-feature Clippy and all `361/361` server library tests pass locally.

Commit `6d4fed6` drives the actual reward worker from `CaldusRewardPending` through the complete local pipeline. A retryable durable-authority failure reuses the byte-identical frozen handoff, the validated durable result is acknowledged exactly once, the route reaches `TerminalPending` before custody loading, and extraction activates exactly once. The first generation's two QUIC publications are deliberately left unacknowledged and discarded; two newer generations receive the retained pending-inventory event before the identical `BossExitReady` event without another resolution, acknowledgement, snapshot, reservation, or activation. Exact terminal completion stops the publisher, transport-independent competing-terminal cleanup reopens the route, and shutdown reports zero residue. Strict all-target/all-feature server Clippy and all `362/362` server library tests pass locally.

## Current Next Step

Hosted run [`29667827330`](https://github.com/MikeyPar/Gravebound/actions/runs/29667827330) proves Windows release construction, all four B3 tests, and all eight Caldus PostgreSQL tests green. Its later `postgres_safe_inventory` failure is an independent globally reused lifecycle reward-request fixture, isolated by `3da08c5`; it does not weaken Caldus or item uniqueness authority. Obtain a fully green mandatory hosted run for the cumulative head. The production tick prerequisite is now closed separately by [`GB-M03-03G-live-authoritative-tick-evidence.md`](GB-M03-03G-live-authoritative-tick-evidence.md); next compose live death, the five-producer terminal loop, danger activation, and dispatch behind the all-or-nothing authority proof. Do not advertise or bind normal admission until those owners and their shutdown order are proven together.
