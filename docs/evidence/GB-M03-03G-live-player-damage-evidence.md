# GB-M03-03G live player-damage evidence

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-004`, `SIM-010`, `COM-002`, `DTH-001`, and `DTH-010` require server-authored combat, exact source/target validation, ordered damage history, lethal-first terminal resolution, and a trustworthy ten-second death trace.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-010`, `CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001` define the fixed 30 Hz domain and the closed microrealm, B0-B6, and Sir Caldus damage producers.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-06`, `GB-M03-08`, and `GB-M03-13` require the ordinary private route and its death, Recall, extraction, and Echo outcomes to consume committed server facts rather than reconstructed client state.

## Implemented contract

Commits `06ef41f` and `bb67be7` retain immutable projectile origins and exact melee/charge origins in simulation events, then project every applied player hit from the live microrealm, fixed-dungeon, and Sir Caldus frames into `CorePrivatePlayerDamageFactV1`.

Each fact binds the committed tick, canonical event ordinal, direct-hit cause, stable source content and entity IDs, pattern/attack ID, raw and final damage, damage type, pre/post health, and finite source position. Projection follows simulation execution order: normal lane contacts before hostile projectiles, B3 charge contacts before hostile projectiles, and Caldus charge contacts before hostile projectiles.

The projection rejects foreign targets, foreign ticks, unknown promoted patterns, source disagreement, non-finite positions, invalid damage arithmetic, discontinuous same-tick health, nonfinal lethality, and any frame whose lethal flag disagrees with its facts. Debug-invulnerable contacts are explicitly omitted because they do not apply player damage. Projection finishes before the route compare-and-swap and local runtime state swap, so a malformed fact cannot partially commit a frame.

This record closes only the scene-fact boundary. It does not claim the durable ten-second trace, clocks, deeds, terminal competition, or death transaction. Normal capability advertisement remains disabled.

## Verification

- `cargo fmt --all -- --check`: pass.
- `cargo test -p sim_core --lib`: `399/399` pass.
- `cargo test -p sim_content --lib`: `132/132` pass.
- `cargo test -p server_app --lib`: `367/367` pass.
- `cargo clippy -p sim_core -p sim_content -p server_app --all-targets --all-features -- -D warnings`: pass.
- Focused projection tests cover all 15 promoted attack-pattern mappings, source origin, ordinal/lethal ordering, foreign target, foreign application tick, health discontinuity, and debug-invulnerable omission.
- An independent three-authority review confirmed that the implemented producer order matches current simulation execution. Its target, tick, continuity, and closure findings were corrected before this evidence record.

Hosted run [`29668693282`](https://github.com/MikeyPar/Gravebound/actions/runs/29668693282) is fully green for the immediately preceding `84d9b39` checkpoint across Linux formatting/lint/tests/content/schema validation, mandatory PostgreSQL transactions, optimized Windows release construction, and optimized native death frames. Hosted proof for the two damage-fact commits remains pending and is not claimed by that run.

## Current Next Step

Add a bounded, lossless, acknowledged driver-to-terminal feed for damage-bearing frames. The driver must not expose the next committed tick until the terminal owner acknowledges trace ingestion, and observer `watch` state must remain presentation-only because it can coalesce frames. Then compose lifetime/deeds/custody authority and all five terminal producers behind the all-or-nothing private-life builder. Keep normal admission disabled until that owner graph, durable death path, restart behavior, and shutdown order pass together.
