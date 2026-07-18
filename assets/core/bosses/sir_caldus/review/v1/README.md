# Sir Caldus renderer-review pack v1

> **UNREGISTERED REVIEW CANDIDATE — NOT RUNTIME — NOT A CONTENT-HASH INPUT**

This pack advances the committed M03 Sir Caldus silhouette into renderer-sized review material without changing `sprite.boss.sir_caldus`, any Core manifest, any gameplay source, or any route capability. The four images are key poses for art-direction, anchor, camera-readability, and later animation planning; they are not an animation and have no authored frame rate.

## Authority and selection

- `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-002`, `ART-001`, `ART-002`, `ART-004`, `ART-020`, and `ART-030` require a top-down orthographic read, a 96-192 px boss footprint, one approved seed frame, shared bottom-center normalization, preview inspection, and provenance.
- `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-007`, `CONT-BOSS-001`, and `CONT-BOSS-002` bind the only Core major boss to B6, Shield Arc, and Charge Lane.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` owns the one Core major boss. M03 Hall and microrealm presentation remains graybox; coherent vertical-slice and ship-quality Hall art remains later-owned.
- `docs/tasks/GB-M03-03G.md`: its current next step explicitly permits the committed Sir Caldus drafts to advance only through reviewed renderer-sized derivation and forbids content-hash entry before review.

The physical Core inventory otherwise contains the death-portrait atlas, item icons, Oath/Bargain choice icons, and fonts. Core encounter sprites and player-visible world markers remain symbolic or graybox. This pack therefore targets the one art lane explicitly open beside `GB-M03-03G`; it does not broaden M03 into M04+ art production.

## Candidate files

All candidate frames are `192 x 192` RGBA. The shared renderer anchor is bottom-center `[96, 192]`; the nontransparent art is normalized through one shared 176 px content scale, leaving 16 px of top safety padding. Uncompressed memory is 147,456 bytes per frame and 589,824 bytes for the four-frame strip.

| File | Review role | Nontransparent bounds | SHA-256 |
|---|---|---:|---|
| `sir-caldus-idle-guard.seed.png` | Locked guard/idle seed | `(36,16)-(156,192)` | `da0c0b90ca1cc390ba4a59ea3a8991313c95f2a2a339123c36b174da99ad4ea5` |
| `frames/01-guard-idle.png` | Exact copy of the locked seed | `(36,16)-(156,192)` | `da0c0b90ca1cc390ba4a59ea3a8991313c95f2a2a339123c36b174da99ad4ea5` |
| `frames/02-shield-arc-brace.png` | Shield Arc brace key pose | `(28,26)-(147,192)` | `3ee4ae19dc5fcfc80da897718a872fa89332e4077ed1b45ecd0e5bb4806659cd` |
| `frames/03-charge-lane-drive.png` | Charge Lane drive key pose | `(26,43)-(149,192)` | `0b05c2b64351489eb659649485ee064fb970f5e98fba5f61869f44380a231ff0` |
| `frames/04-recovery.png` | Broad recovery key pose | `(36,16)-(156,192)` | `a2251f1b6ff1c8580a7cdf83a524905f260c4f62e7bade839531ed40692e11f3` |
| `sir-caldus-key-poses.strip.png` | Four equal 192 px slots in the order above | `(36,16)-(732,192)` | `d7cfae2b77f701029a3dccf31a845b9c3e99e3e10af9e39121cf60b79cd123d1` |

Source and preview hashes, exact post-process parameters, backdrop identities, and ART-020-style candidate metadata are in `sir-caldus-renderer-review.source.json`.

## Preview and visual QA

- `previews/sir-caldus-key-poses.192px-checkerboard.png` is the exact 1x frame review. Identity, palette, shield side, facing, and the lower-center anchor remain consistent.
- `previews/sir-caldus-key-poses.96px-readability.png` is a nearest-neighbor half-scale stress check. The helmet, bell-brass mass, shield, planted brace, forward drive, and recovery stance remain distinguishable; fine shield engraving appropriately collapses before the silhouette does.
- Four visibly watermarked `review-mock` files composite the unchanged Shield Arc frame at exact 192 px onto the existing optimized `GB-M03-03E` phase-one standard/reduced-effects backdrops at 1280x720 and 1920x1080. These files are asset-review contexts, not native captures and not gameplay evidence.
- Original-resolution inspection found no crop, slot crossing, extra character, text, logo, scenery, cast shadow, magenta spill, or disconnected alpha component in the retained frames.

The pack is a candidate pass, not a runtime pass. It has not been imported into Bevy, animated, collision-tested, motion-tested, registered, validator-tested as content, or accepted as ship-quality art.

## Generation and cleanup record

- Generation method: OpenAI built-in image generation.
- Identity reference: `Concept Art/m03-sir-caldus-combat-silhouette-drafts/sir-caldus-shield-arc-draft-alpha.png`, SHA-256 `b5353ad590a9188a939e22cc075e86d687498f370d2a34cf3632a173124b1c10`.
- The charge draft was inspected for action intent but was not supplied to either accepted generation, preventing its different helmet/shield design from contaminating the locked identity.
- Accepted raw chroma sources are retained in `source/`. They were generated on a flat magenta field, then processed first with the installed image-generation skill's `remove_chroma_key.py` helper using `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`.
- The sprite-pipeline helpers normalized the whole strip under one shared scale and locked frame 01 back to the accepted seed. The 176 px normalized result was bottom-centered in a 192 px renderer frame.
- Chroma removal exposed tiny disconnected edge remnants in the first strip source. The retained frames use the permitted local mask fallback only after the built-in helper: at alpha threshold 8, keep the largest 4-connected silhouette and clear all smaller components. This removed 2, 36, 42, and 1 pixels from frames 01-04 respectively; no main-silhouette pixel was repainted.
- A cleanup regeneration, SHA-256 `2ab10202cfa2932de086a71c95b3b4a75a8936e1170f4ef462ca22acd34c752a`, was rejected because the fourth figure crossed its equal-slot boundary. It is not copied into the workspace.
- No third-party image, game, artist, brand, or style name was supplied to the generator.

## Exact prompts

### Guard/idle seed

```text
Intended use: candidate production seed sprite for Gravebound's 2D top-down orthographic boss renderer review, not concept art.

Create exactly ONE full-body Sir Caldus sprite, preserving the referenced character identity exactly: the same blackened iron plate, tarnished brass edging, central brass helmet stripe, closed vertical-bar visor, dark charcoal cowl, bell-and-chain details, olive-black torn cloth, and the same large dark coffin/kite shield with brass ribs and round boss. Do not redesign, add a weapon, duplicate the character, or change costume proportions.

Camera and pose:
- orthographic top-down three-quarter game camera, approximately 55 degrees downward
- facing toward the bottom of the canvas
- neutral guarded idle pose with both feet readable and the shield held on the character's screen-right
- compact boss combat silhouette, broad shoulders and shield clearly separable from the body
- bottom-center ground contact aligned on one clean horizontal baseline
- entire figure and shield fully inside the canvas with generous even padding

Composition:
- square canvas
- exactly one character, centered
- uniform flat saturated chroma-key magenta background, exact solid #FF00FF edge to edge
- no transparency, gradient, texture, scenery, floor, shadow, glow, particles, labels, border, UI, or extra objects

Style:
- authentic dark-gothic pixel-art production sprite
- crisp deliberate pixel clusters and hard silhouette
- restrained wet-stone, black iron, tarnished brass, bone, and moss palette
- readable after nearest-neighbor reduction into a 192x192 boss frame
- no anti-aliased painterly edges, no poster composition, no photorealism
```

### Four key poses

```text
Intended use: candidate production key-pose spritesheet for Gravebound's 2D top-down orthographic boss renderer review.

Edit the provided four-slot reference canvas into exactly ONE horizontal row of FOUR equal, non-overlapping Sir Caldus sprite slots. The existing sprite in the leftmost slot is the identity and scale anchor. Preserve one character identity across every slot: same blackened iron plate, tarnished brass trim, central brass helmet stripe, closed vertical-bar visor, charcoal cowl, bell-and-chain arrangement, olive-black torn cloth, and the exact same dark coffin/kite shield with brass ribs and round boss. Same body proportions, palette, shield scale, facing direction, and pixel-art cluster language in every slot. Do not redesign the helmet or shield, add a weapon, lose bells, duplicate limbs, or vary armor.

Camera and alignment invariants:
- orthographic top-down three-quarter game camera, approximately 55 degrees downward
- every frame faces toward the bottom of the canvas
- exactly one full-body character per slot
- identical apparent scale
- bottom-center ground contact on one shared horizontal baseline
- entire figure and shield contained inside its own slot with clear gutters
- shield remains on the character's screen-right

Four slots, left to right:
1. Guard/idle anchor: match the existing leftmost reference pose.
2. Shield Arc brace: feet planted wide, torso turned slightly, shield swept forward and outward as a readable windup/brace; body remains visible behind the shield.
3. Charge Lane drive: low forward-driving pose toward the bottom of the canvas, shield leading, shoulders compressed, rear leg extended; clearly different from idle while retaining the same facing.
4. Recovery: shield lowered slightly outward, torso rising, weight settling into a broad stable stance; a readable vulnerable recovery, not another charge.

Composition:
- square canvas containing the centered horizontal four-slot row
- uniform flat saturated chroma-key magenta background, exact solid #FF00FF edge to edge and between figures
- no transparency, separators, grid, labels, text, numbers, scenery, floor, cast shadows, glow, particles, telegraphs, UI, border, or extra objects

Style:
- authentic dark-gothic pixel-art production asset, not concept art
- crisp deliberate pixel clusters and hard silhouettes
- restrained wet-stone, black iron, tarnished brass, bone, and moss palette
- each figure readable after nearest-neighbor reduction into an individual 192x192 boss frame
- no painterly anti-aliasing, poster composition, or photorealism
```

### Rejected cleanup regeneration

```text
Cleanup edit for a candidate production key-pose spritesheet. Preserve the same Sir Caldus identity, exact four left-to-right poses, top-down three-quarter camera, downward facing, palette, armor, bells, and shield design from the first reference. Use the second reference as the immutable identity and scale anchor for slot 1.

Output exactly one square canvas with one centered horizontal row of FOUR equal quarter-width slots:
1 guard/idle, 2 Shield Arc brace, 3 Charge Lane drive, 4 broad recovery stance.

Critical slot cleanup:
- exactly one complete, connected character silhouette inside each slot
- recenter each silhouette within its own quarter-width slot
- keep at least 6% of each slot width as empty magenta gutter on both left and right
- no figure, shield, cloth, pixel cluster, isolated speck, debris, or anti-aliased remnant may touch or cross a quarter-slot boundary
- all pixels outside the four main character silhouettes must be the same uniform solid #FF00FF
- shared bottom-center baseline and identical apparent scale
- full bodies and shields entirely contained
- shield stays on screen-right
- slot 4 must read as recovery: shield visibly lowered and angled outward, torso rising, feet planted broad

Uniform flat saturated #FF00FF background edge to edge and between figures. No transparency, separators, grid, text, numbers, scenery, floor, shadows, glow, particles, telegraphs, UI, border, or extra objects.

Authentic dark-gothic pixel-art production asset with crisp deliberate clusters, hard silhouettes, restrained black iron/tarnished brass/moss palette, designed for nearest-neighbor reduction to four individual 192x192 frames. Not concept art; no painterly edges or poster composition.
```

## Current Next Step

The guard seed, pose identity, 192 px frame, `[96,192]` anchor, 96 px readability, and standard/reduced camera read passed internal candidate review on 2026-07-17. Generate each complete idle, Shield Arc, Charge Lane, and recovery animation as one strip from the locked seed, normalize every frame with one shared scale, and inspect motion and anchor drift in-engine. Only after that separate runtime review may a Core asset-registry/content-hash change be proposed. Until then, `sprite.boss.sir_caldus` remains symbolic and normal Core route admission remains disabled under `GB-M03-03G`.

## Licensing and provenance note

This record documents inputs, prompts, generated sources, transformations, hashes, and visual review; it is not independent legal clearance. Before runtime use, the project owner should perform the normal commercial-rights, visual-similarity, repository-registry, and licensing review.
