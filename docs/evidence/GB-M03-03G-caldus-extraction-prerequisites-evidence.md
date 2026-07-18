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
- Normal admission and ordinary feature advertisement remain disabled. The worker does not yet publish the snapshot or construct/register the extraction actor; those are the next integration seam.

## Verification

- Strict all-feature/all-target Clippy passed for `protocol`, `client_bevy`, `persistence`, and `server_app`.
- Protocol: `87/87` tests passed.
- Persistence: `246/246` tests passed.
- Native route model: `8/8` focused tests passed.
- Server library: `349/349` tests passed, including coherent projection and active/`LinkLost` extraction-binding coverage.
- `cargo check -p server_app --test postgres_caldus_victory` passed, so the hosted custody journey compiles.
- `TEST_DATABASE_URL` was absent locally. Hosted PostgreSQL execution and real-QUIC restart/adverse evidence are therefore explicitly unclaimed.

## Current Next Step

Extend the session-owned Caldus worker to load, project, publish, and retain the coherent pending-inventory snapshot; then construct/register the production extraction actor only from the exact committed `BossExitReady` authority and call the transport-independent session binding. Add rollback/abort handling so no post-permit failure can strand `TerminalPending`. Prove active transport, response loss, reconnect, `LinkLost`, competing terminal, restart, and zero-residue behavior before bound normal-server construction or admission.
