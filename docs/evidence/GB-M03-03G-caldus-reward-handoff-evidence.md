# GB-M03-03G frozen Caldus reward handoff evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md` `DNG-006`, `LOOT-002`, `LOOT-010`, `SOC-010`, `TECH-015`, and `TECH-021` require server-owned personal reward eligibility, at-risk pending custody, durable idempotency, and no exit before the boss result is terminal.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-BOSS-001`/`002` and `CONT-REWARD-003` fix Sir Caldus's participant lock, maximum health, personal reward table, defeat order, and stable B6 exit.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires Sir Caldus, pending rewards, extraction, and return to compose in the ordinary private loop without developer-authored outcomes.

Approved `SPEC-CONFLICT-023` supplies the stable attempt identities and reward-before-exit order. Approved `SPEC-CONFLICT-029` keeps the later extraction inside the shared five-producer terminal authority.

## Delivered contract

Commit `78b7ff0` adds the immutable defeat-to-durable-result boundary inside the route-bound B6 owner.

- Eligibility tracking begins only on the authoritative combat-start tick, remains staged with movement/combat/encounter state, and commits only after the route compare-and-swap succeeds.
- Presence, direct contribution, longest continuous inactivity, defeat-tick life/presence, cumulative session validity, cumulative anti-cheat validity, connection state, and the last server-accepted reward-activity sequence are frozen with the participant lock.
- The inherited reward-activity watermark crosses B5 -> B6. A reset clears attempt-local eligibility while retaining the monotonic watermark, so abandoned attempts cannot donate presence or contribution to a later attempt.
- The defeat handoff binds the route lease and exact post-defeat state version, instance lineage, attempt ordinal, participant order, active duration, defeat tick, selected character, and expected progression version.
- Once `BossDefeated` commits, ordinary combat frames fail closed without advancing local tick or route state. No exit is visible while the durable outcome is absent, unknown, mismatched, or stale.
- A durable resolution is accepted only when its encounter, lineage, attempt, exit identity, eligible owner order, account, character, participant, and personal reward request identities match the frozen handoff.
- PostgreSQL's per-read `replayed` marker is normalized out of canonical result material. Fresh and response-loss replay projections therefore converge; a changed canonical request hash conflicts.
- Only the matching durable result constructs the exact compiled exit presentation and advances `BossDefeated -> BossExitReady`. Exact acknowledgement replay is read-only and does not churn the route version.
- Durable reward persistence and production extraction remain outside this owner. Normal admission remains disabled.

## Verification

Local Windows verification at exact source `78b7ff0`:

- Focused Caldus reward/runtime suite: `7 passed`, `0 failed`.
- Complete server library: `343 passed`, `0 failed`.
- Strict `server_app` all-target, all-feature Clippy with warnings denied: pass.
- `cargo fmt --all` and `git diff --check`: pass.
- Exact boundary coverage includes 600-tick inactivity eligibility, 601-tick rejection, inclusive active duration, sequence regression, reset isolation, cumulative trust/session rejection, frozen evidence, post-defeat frame rejection, fresh/replayed result convergence, changed-material conflict, and zero-residue actor shutdown.

## Explicit boundary

This historical slice freezes and acknowledges one exact durable Caldus result but does not call PostgreSQL itself. Later commits now construct the B6 owner, publish pending inventory, and bind production extraction through the persistent session; this file still does not claim complete automatic-worker or hosted restart/adverse evidence, and Character Select `Play` plus Realm Gate admission remain disabled.

## Current Next Step

Commit `3f749dd` completes exact production extraction construction/registration and danger-lease binding with replay-safe ownership; see [activation evidence](GB-M03-03G-caldus-extraction-activation-evidence.md). Next prove the complete automatic-worker active/reconnect/transport-free-`LinkLost`/competing-terminal lifecycle and hosted adverse/restart matrix. Keep normal admission disabled.
