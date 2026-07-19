# GB-M03-03G Caldus extraction prerequisites evidence

**Result:** PASS for the automatic execution, coherent post-reward custody, append-only pending-inventory projection, and transport-independent extraction-binding prerequisites. This record does not claim the final Caldus-to-extraction composition, hosted PostgreSQL execution, normal admission, or parent closure.

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md`: `LOOT-002`, `LOOT-010`, `LOOT-033`, `LOOT-060`, `TECH-015`, and `TECH-021`-`023` require server-owned pending custody, exact mutation authority, retry-safe durable outcomes, and reconnect-safe terminal ownership.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-REWARD-003`, `CONT-BOSS-001`, and the Core private-life/Bell Sepulcher records require Sir Caldus reward completion before the stable exit and preserve exact Core content authority.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-04`, `GB-M03-08`, and the M03 exit gate require ordinary route publication, restart/replay safety, bounded protocol evolution, and no duplicate terminal or item result.

## Implemented contract

- Commit `f4ad323` adds one session-owned `CorePrivateCaldusRewardRuntime`. It freezes one handoff, retries only classified retryable coordinator failures, stops closed on fatal authority loss, acknowledges only the matching durable result, and advances the same driver to `BossExitReady` exactly once.
- Commit `e5f7dc8` adds `load_current_danger_extraction_snapshot_v1`. It reuses the terminal-first private-life bootstrap transaction without invoking crash restoration, validates the exact selected living danger root and promoted content, rejects unresolved reward mutations, and returns current post-reward account/character/world/inventory/life versions with bounded `RunBackpack`, `PersonalGround`, and material custody.
- Commit `1bd230a` appends protocol 1.19 `CorePendingInventoryStateV1` at reliable-event discriminant 21. The server projects storage authority without reauthoring identities, custody, or versions; canonical ordering, uniqueness, capacities, content binding, and compatibility validation fail closed. The explicit 1.18 encoder preserves the prior route frame hash.
- Commit `47ad6c3` adds exact registered-actor lookup and transport-independent session binding. A matching extraction actor can be retained while the session is `LinkLost`; reconnect attaches it to the session-owned reliable writer before the new transport generation becomes visible. Foreign account, character, or route generation fails before binding.
- Commit `42a632b` closes impossible custody shapes at every projection boundary. Equipment cannot share one pending slot, Red Tonic stacks are homogeneous and capped at six, run materials are capped at 99, staged lineages cannot project extraction authority, and Core rejects material families excluded by `CONT-REWARD-003`.
- Commit `be52c29` adds exact extraction-permit abort. Only the current account/character/generation/permit may reopen `BossExitReady`; the route version advances monotonically and stale, changed, foreign, retired, or replacement permits cannot clear newer authority.
- Commit `f5c12b3` completes ordered worker publication. After durable reward acknowledgement the worker loads the coherent storage snapshot, sends pending inventory before the matching `BossExitReady` route state at the same authoritative defeat tick, retains both events, and replays them only on newer writer generations. The private-life session installs the writer before reconnect visibility, removes the current writer on `LinkLost`, and can clear the exact registered extraction binding without a transport lease when another terminal wins.
- Commit `3f749dd` consumes these prerequisites in the exact production actor construction/registration and session-binding seam recorded separately in the [activation evidence](GB-M03-03G-caldus-extraction-activation-evidence.md).
- Normal admission and ordinary feature advertisement remain disabled. Complete automatic-worker and hosted lifecycle proof is the next integration seam.

## Verification

- Strict all-feature/all-target Clippy passed with warnings denied for `protocol`, `persistence`, and `server_app`.
- Protocol: `88/88` tests passed.
- Persistence: `246/246` tests passed.
- Server library: the prerequisite source passed `354/354`; cumulative activation source `3f749dd` plus replay/cleanup corrections passes `360/360`, including exact reservation abort, ordered real-QUIC publication/replay, coherent projection, actor/session activation, replay-owned rollback, bounded shutdown cleanup, and terminal-completion release.
- `cargo check -p server_app --test postgres_caldus_victory` passed, so the hosted custody journey compiles.
- `TEST_DATABASE_URL` was absent locally. Hosted PostgreSQL execution and real-QUIC restart/adverse evidence are therefore explicitly unclaimed.

## Current Next Step

Commit `3f749dd` completes the exact construction/registration/session-binding boundary with reservation-before-snapshot, coherent storage versions, fresh/replayed rollback ownership, bounded cleanup, and terminal-completion release; see [activation evidence](GB-M03-03G-caldus-extraction-activation-evidence.md). Next drive the complete automatic worker and prove active transport, response loss, reconnect, transport-free `LinkLost`, competing terminal, completion release, shutdown, and zero residue, then run hosted PostgreSQL/real-QUIC adverse and restart evidence before bound normal-server construction or admission.
