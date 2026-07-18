# GB-M03-03G Core Bell VFX review v1

## Authority and scope

This candidate review uses all three design documents:

1. `Gravebound_Production_GDD_v1_Canonical.md` `ART-001`-`006`, `ART-020`, `ART-030`, and `COM-005`-`006` define readable hostile silhouettes, telegraph priority, reduced-effects parity, and combat counterplay.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-007` and `CONT-ENEMY-001`-`002` define the exact Bell pack, 900 ms spawn warning, Drowned Pilgrim three-shot fan, and Bell Reed six-projectile gap ring.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03` requires optimized standard/reduced-effects visual evidence before normal route admission.

Commit `ffd74d6` adds an isolated, unregistered candidate package at `assets/core/effects/core_bell_microrealm_vfx/v1`. It does not alter a content record, registry, runtime bundle, collision shape, damage rule, warning duration, or content hash.

## Reviewed candidates

- `pack-bell-spawn-warning.96.png`: 96x96 broken ash/iron warning ring, SHA-256 `8dc06aed914263a4b5c02fe683880eed6aed35c7bdb50ec033d5d776b036d56e`.
- `drowned-pilgrim-fan-cue.64.png`: 64x64 cue with exactly three physical directions at -15/0/+15 degrees, SHA-256 `250448296bd1f143fe04a753875a8d186182e3ea06d9ae8ed62400eb688eb71a`.
- `bell-reed-gap-ring-cue.96.png`: 96x96 cue with exactly six separated Veil markers and the authored adjacent two-index gap, SHA-256 `29bc27d18f36e7ebade5111360c32f1f01abe5649cecc50a44674ead40f37141`.

The source manifest retains the exact prompts, raw generated sources, alpha postprocess, anchors, runtime/source/preview hashes, design citations, and no-authority boundary. Main-agent verification parsed the manifest, recomputed all eight recorded hashes, checked runtime dimensions/RGBA mode, and inspected the runtime-scale and nearest-neighbor 2x review sheets at original resolution.

## Review result

The set is accepted as a versioned art candidate only. The ring reads as hostile and not safe/interactable; the Pilgrim cue has exactly three clear directions; the Reed cue has exactly six marks and one obvious adjacent gap. Registration remains blocked until the live renderer proves actual warning cadence, rotated gap states, grayscale separation, effects priority, and standard/reduced-effects readability at 1280x720 and 1920x1080.

## Current Next Step

Exercise the three candidates against the authoritative live warning/attack timelines in an optimized native build. Bind registry IDs only after timing, scale, rotation, accessibility, and reduced-effects evidence passes without changing simulation authority.
