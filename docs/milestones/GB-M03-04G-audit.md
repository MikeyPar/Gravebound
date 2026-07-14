# GB-M03-04G completion audit

**Status:** PASS on main implementation commit `c5691ac`; hosted CI [`29373978792`](https://github.com/MikeyPar/Gravebound/actions/runs/29373978792) is green.

## Three-authority closure

| Authority | Closure |
|---|---|
| `Gravebound_Production_GDD_v1_Canonical.md` | `LOOP-005`, `LOOT-001`-`005`, `LOOT-010`, `LOOT-020`, `LOOT-050`, `LOOT-060`, `TECH-020`/`021`/`023`, `UI-001/003/006/011/030`, and `QA-003` are represented by a content-bound, server-planned, immediately durable lifecycle with exact identity, custody, placement, version, receipt, and ledger evidence. |
| `Gravebound_Content_Production_Spec_v1.md` | `manifest.items.core_18`, the exact Core starter/reward profiles, Hall/Vault/Realm Gate identities, lowest-index placement, localization, and immutable content revision are the only executable authorities used by the lifecycle and native inspection surface. |
| `Gravebound_Development_Roadmap_v1.md` | `GB-M03-02`/`04` now have real-transport PostgreSQL restart preservation, retry nonduplication, 25 measured journeys, a median login-to-control far below 30 seconds, and parent evidence while the later terminal/player-route packages remain closed. |

## Acceptance evidence

| Criterion | Evidence | Result |
|---|---|---|
| Complete canonical signature | The typed signature covers selected character, exact content authority, level/XP and receipts, four Equipment slots, two Belt slots, eight RunBackpack slots, eight CharacterSafe slots, 160 Vault slots, item UID/provenance/security/location/stack state, aggregate versions, mutation receipts, and ordered ledgers. | PASS |
| Real lifecycle composition | The disposable authenticated real-QUIC journey composes character selection, starter initialization, Caldus XP/first-clear, reward placement, field equipment, and safe transfer through production services and PostgreSQL repositories. | PASS |
| Reconnect and process restart | Canonical bytes and digest remain identical before reconnect, after a newly authenticated endpoint, and after a newly bound pool/server endpoint on the same database. | PASS |
| Replay and response loss | Exact retry returns the stored result; altered payload conflicts. Dropping a committed response and retrying on a fresh endpoint creates no duplicate item, receipt, placement, ledger transition, reward, XP, or aggregate advance. | PASS |
| Concurrency and capacity | Duplicate sessions converge on one fresh and one replayed result. Final-slot claims and danger-entry/manual-transfer races serialize without over-capacity or partial state. | PASS |
| Fail-closed authority | Stale/foreign identity, invalid item revision, malformed/oversized frames, structural receipt corruption, semantic replay-hash corruption, unavailable PostgreSQL, and injected write failures expose no partial or caller-authored state. | PASS |
| Normal route remains closed | Every endpoint proves a typed nonmutating Realm Gate rejection; normal runtime does not advertise safe-inventory integration or enable Character Select Play, Vault interaction, allocation, extraction, death, or Core promotion. | PASS |
| Performance and cleanup | 25 journeys: login `10.083/14.989/18.611 ms` median/p95/max; mutation `7.471/10.127/14.474 ms`. Median login is below 30 seconds, endpoints close, and zero idle PostgreSQL transactions remain. | PASS |
| Native inspection | The optimized [visual matrix](../evidence/GB-M03-04G-visual-manifest.md) passes 1280x720 and 1920x1080 in standard/reduced modes with exact build/content IDs, SHA-256 hashes, 96%+ framebuffer coverage, complete nine-item projection, and 49% protected Hall corridor. | PASS |

The complete adverse mapping is recorded in [`GB-M03-04G-adverse-matrix.md`](../evidence/GB-M03-04G-adverse-matrix.md).

## Cumulative verification

- Local: `cargo fmt --all -- --check`.
- Local: `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Local: `cargo test --workspace --locked`.
- Local: both strict content validators and generated-schema clean-diff gate.
- Local: focused lifecycle-surface tests, strict client Clippy, and optimized Windows construction.
- Hosted: CI `29373978792` passed cumulative Linux quality/content/schema tests, all 10 safe-inventory PostgreSQL tests plus the remaining mandatory shared-database suites, and the Windows release build on `c5691ac`.

## Deferred ownership

This closure does not enable extraction/Recall conversion, Overflow, ResolutionHold, death destruction, memorial/Echo writes, successor recovery, salvage/crafting, station interaction, normal Realm Gate admission, production namespace writes, or Core promotion.

## Handoff

Proceed to `GB-M03-06A` with `GB-M03-02D` at additive migration `0031`. Build immutable death/summary/memorial/trace records and exact danger-entry Equipped/Belt risk capture before adding player-visible terminal behavior.
