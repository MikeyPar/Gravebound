# GB-M03-03G Caldus terminal-arbitration evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md` `DNG-006`, `LOOT-002`, `LOOT-010`, `TECH-015`, `TECH-021`, `TECH-022`, and `TECH-023` require personal at-risk rewards, durable replay, atomic terminal precedence, and crash-safe restoration.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-BOSS-001`/`002` and `CONT-REWARD-003` fix the Caldus attempt, personal reward, progression, and reward-gated exit order.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` and `GB-M03-08` require the boss result and all terminal outcomes to compose without duplication, resurrection, or a client-authored destination.

Approved `SPEC-CONFLICT-023` fixes stable Caldus identities and reward-before-exit ordering. Approved `SPEC-CONFLICT-029` requires death, extraction, Recall, disconnect recovery, and fault restoration to arbitrate against the same durable danger authority.

## Delivered contract

Commits `308767f` and `57e4ca2` close the database-bound race that previously blocked automatic Caldus execution.

- The exact Bell `entry_restore_point_id` now crosses the consuming microrealm -> fixed dungeon -> Caldus staging -> frozen defeat handoffs with the existing account, character, lineage, route, attempt, and progression authority.
- Fresh Caldus item rewards now lock the account before inventory, check exact replay first, and then require the selected living normal-security character at the exact active restore root, danger world location, and open lineage.
- Fresh Caldus progression uses the same account-first order. A Hall location, foreign character, changed root/lineage, closed root, or committed terminal winner cannot append XP.
- Caldus exit finalization locks every eligible owner account in byte order, checks exact exit replay, revalidates every owner danger root, and only then verifies the stored item/XP terminals and appends the stable exit.
- Exact item, progression, and exit replays remain readable after a later terminal closes the root. Fresh late writes return typed fail-closed authority errors.
- The production coordinator supplies the authority only from the authenticated frozen defeat. The client cannot author the restore point, lineage, reward destination, XP result, or exit material.
- Normal route admission, automatic session execution, pending-inventory publication, and production extraction binding remain disabled.

## Verification

Local Windows verification at exact source `57e4ca2`:

- Persistence library: `244 passed`, `0 failed`.
- Server library: `345 passed`, `0 failed`.
- Focused active-danger, Caldus exit, coordinator, frozen-runtime, and same-task driver suites: pass.
- Strict `persistence` and `server_app` all-target, all-feature Clippy with warnings denied: pass.
- `cargo fmt --all`, all-target compilation, and `git diff --check`: pass.

The hosted PostgreSQL suite now includes two additional adverse contracts: terminal closure between the item and progression phases blocks fresh XP/exit while preserving exact item replay, and a fully committed Caldus result replays exactly after terminal closure. They compile as part of all targets but are not claimed executed locally because `TEST_DATABASE_URL` is absent. Hosted execution, injected Recall/death concurrency, restart, and response-loss evidence remain required before closure.

## Current Next Step

Commit `3f749dd` now constructs/registers production extraction from the exact committed `BossExitReady` authority plus coherent storage versions, binds it through the danger lease, and protects terminal ownership under replay and cleanup; see [activation evidence](GB-M03-03G-caldus-extraction-activation-evidence.md). Next prove the complete automatic-worker competing-terminal, reconnect, transport-free-`LinkLost`, completion-release, and zero-residue lifecycle, then run the hosted PostgreSQL/real-QUIC adverse and restart matrix. Keep normal admission disabled.
