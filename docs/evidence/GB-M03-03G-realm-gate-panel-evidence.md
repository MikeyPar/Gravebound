# GB-M03-03G Realm Gate confirmation evidence

## Authority

This correction reads the three design authorities together:

1. `Gravebound_Production_GDD_v1_Canonical.md` requires the Lantern Halls Realm Gate to lead the ordinary Core route without bypassing durable world-transfer authority.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-HUB-001/002` requires an instant Realm Gate panel showing the permitted realm, population, network health, and an explicit `Enter`; Escape closes without mutation.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03/03G` requires the ordinary no-command Character Select → Hall → danger route and fail-closed admission evidence.

## Implemented boundary

- An authoritative `Opened(RealmGate)` result now opens presentation state only. It no longer queues a world transfer.
- The panel displays the permitted Core Micro-realm, private population, current snapshot/network synchronization state, explicit `[Enter] Enter`, and `[Esc] Close`.
- Only an Enter press while the Realm Gate panel is open constructs the canonical `UsePortal(station.realm_gate)` command.
- Escape continues to use the server-owned `ClosePanel` interaction. Enter outside this panel creates no Realm Gate command.
- The existing server-owned CharacterSafe preflight, restore-root creation, mutation identity, version checks, and world-transfer result remain unchanged.

## Focused evidence

- `cargo test -p client_bevy --lib realm_gate_open_result_requires_explicit_enter_command`: PASS (`1/1`).
- `cargo clippy -p client_bevy --lib -- -D warnings`: PASS.
- `cargo fmt --all` and `git diff --check`: PASS.

## Current Next Step

Exercise this exact panel through the production-server/PostgreSQL/real-QUIC ordinary-route harness and include it in the next optimized tester capture. Do not close `GB-M03-03/03G` until Enter reaches durable danger control, Escape performs no mutation, 25 full journeys pass, and the required current timing/visual evidence is published.
