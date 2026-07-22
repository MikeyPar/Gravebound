# GB-M03-03G Combat Presentation Integration Evidence

**Result:** Implementation gate PASS and packaged in optimized tester r20; exhaustive milestone evidence remains owner-deferred.

## Design authorities

This integration is governed by all three production authorities:

1. `Gravebound_Production_GDD_v1_Canonical.md`
   - `COM-005` requires telegraphs to communicate timing, geometry, severity, and damage type before resolution.
   - `UI-003` and `UI-004` define the compact combat HUD and the exact `35%` LOW HEALTH / `15%` CRITICAL health-frame thresholds.
   - `UI-010` requires accessibility settings to preserve combat information and timing.
   - `ENC-010` defines Sir Caldus's authored phases and learned pattern language.
2. `Gravebound_Content_Production_Spec_v1.md`
   - The Core fixed route assigns Drowned Pilgrims and Bell Reeds to B1, Sepulcher Knight to B3, Pilgrims and Chain Sentry to B5, and Sir Caldus to B6.
   - The enemy-kit and Caldus tables define exact authored fan, lane, ring, cross, and rotation behavior. Presentation may not infer or substitute geometry from display names.
3. `Gravebound_Development_Roadmap_v1.md`
   - `GB-M03-03` requires the complete Character Select -> Hall -> micro-realm -> six-room dungeon -> boss -> Hall route.
   - The M03 exit gate requires the route to work without developer commands and reserves cumulative journey, restart, nonduplication, timing, and cohort proof for formal closure.

## Delivered source

| Commit | Contract |
| --- | --- |
| `cb609e8` | Protocol 1.23 adds append-only, capability-negotiated actor bindings and bounded Pattern-channel combat-presentation events. Fixed-size Fan, AimedLane, Ring, Lanes, and Rotor payloads carry explicit Physical/Veil damage presentation. Simulation exposes authored offsets and charge movement state rather than client inference. |
| `7cbea6e` | The private-life server publishes complete content-bound actor identities and exact room/Caldus telegraphs. Initial reliable bindings precede danger presentation logically even when QUIC stream and datagram delivery reorder. Incomplete actor-binding sets fail closed. |
| `376650e` | The native client renders the Grave Arbalist, six Core normal enemies, Sepulcher Knight, and Sir Caldus only from known content IDs. It withholds danger actors until their matching binding is present, renders only server-supplied geometry, and adds the compact combat/Belt/Recall HUD with standard, reduced-motion, and high-contrast parity. |
| `a99ee82` | Strict server lint debt exposed by the integration is removed without changing authority or orchestration behavior. |

## Independent integration corrections

The root integration review found and corrected four production blockers before landing:

- Ring rendering originally assumed one fixed segment/gap shape; it now consumes the exact authored segment count, gap count, start angle, radius, and width.
- Fan rendering originally inferred geometry from pattern names; it now consumes the exact authored ray count, offsets, extent, width, and explicit damage presentation.
- B2 Drowned Pilgrim telegraphs were absent from the first binding table; the server now proves complete set equality for every required actor.
- Independent QUIC stream/datagram ordering could expose an unbound snapshot; the client now retains the snapshot but renders no danger actor until the reliable binding arrives.

Unknown content IDs, stale route generations, stale state versions, malformed fixed geometry, unsupported protocol versions, and incomplete binding sets are rejected rather than rendered with fallback gameplay meaning.

## Production-blocking verification

- `cargo test -p protocol core_combat_presentation --lib`: PASS (`5/5`).
- Focused server actor-binding completeness test: PASS (`1/1`).
- Focused client independent-delivery binding test: PASS (`1/1`).
- `cargo clippy -p client_bevy --lib --no-deps -- -D warnings`: PASS.
- `cargo clippy -p server_app --lib --no-deps -- -D warnings`: PASS.
- `cargo fmt --all` and `git diff --check`: PASS.
- Sepulcher Knight runtime sprite/provenance hashes, transparency, grayscale readability, scale hierarchy, and prohibited embedded cues: PASS. See `assets/core/enemies/sepulcher_knight/v1/previews/sepulcher-knight-scale-review.png`.

These are the production-blocking checks authorized for the implementation phase. The exhaustive adverse/restart suite, 25 scripted complete journeys, optimized live visual/performance matrix, private-cohort comprehension metric, backup/restore rehearsal, and external Steam/platform evidence remain explicitly deferred to the owner's final audit.

## Current Next Step

Run the owner-deferred formal acceptance sweep from optimized tester r20: exhaustive adverse/restart and 25-journey proof, live visual/performance evidence, backup/restore rehearsal, private-cohort comprehension, and external Steam/platform evidence. This next step remains governed by `Gravebound_Production_GDD_v1_Canonical.md`, `Gravebound_Content_Production_Spec_v1.md`, and `Gravebound_Development_Roadmap_v1.md`.
