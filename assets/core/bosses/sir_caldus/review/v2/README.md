# Sir Caldus motion-strip review pack v2

## Scope and authority

This is an unregistered, review-only candidate pack for `sprite.boss.sir_caldus`. It advances the motion/anchor review explicitly left by the v1 renderer pack without changing the asset registry, compiled content, hashes, runtime bundle, route admission, or game behavior.

It is grounded in all three authorities:

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-002`, `ART-001` through `ART-006`, `ART-020`, `ART-030`, and `ENC-010`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-007`, `CONT-BOSS-001`, `CONT-BOSS-002`, and `CONT-PATTERN-003`.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, the M03 one-major-boss burn-up, and the rule that temporary assets remain labeled and outside release bundles.

The source revision was `3ce5504dcedfe0de568a3546574ae517513a85c9`. The locked identity input is the v1 guard seed, `../v1/sir-caldus-idle-guard.seed.png`, SHA-256 `da0c0b90ca1cc390ba4a59ea3a8991313c95f2a2a339123c36b174da99ad4ea5`.

## Candidate outputs

Two four-frame, 192 x 192 px RGBA strips use the same bottom-center anchor `(96,192)`:

| Strip | Reading order | Result |
|---|---|---|
| `frames/shield_arc` | guard, windup, Shield Arc release, recovery | Candidate pass for anchor/silhouette review. |
| `frames/charge_lane` | guard, compact windup, charge travel, braking recovery | Candidate pass for anchor/silhouette review after one corrected regeneration. |

Frame 01 of both strips is byte-identical to the locked v1 seed. These strips are presentation candidates only; they do not define animation timing, attack origin, collision, damage, or authority. The exact shield/charge timing remains owned by `CONT-BOSS-002` and the server simulation.

`previews/` contains 192 px and nearest-neighbor 96 px contact sheets plus watermarked standard/reduced-effects review mocks at 1280 x 720 and 1920 x 1080. Mocks deliberately retain the existing native evidence's symbolic Caldus marker behind the candidate sprite, so they cannot be mistaken for native captures.

## Generation and post-processing

- Generation method: OpenAI built-in image generation, using the v1 seed as the sole visual reference.
- Raw Shield Arc: `source/sir-caldus-shield-arc-motion.raw-chroma.png`, 1983 x 793 RGB.
- Raw Charge Lane: `source/sir-caldus-charge-lane-motion.raw-chroma.png`, 2172 x 724 RGB.
- Each raw image used a flat magenta chroma field. The installed image-generation helper removed it with `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`.
- The game-studio normalization helper split four horizontal slots, applied one scale per strip, locked frame 01 to the v1 seed, and bottom-centered every frame in a 192 px renderer canvas.
- A post-helper connected-component cleanup retained the one largest alpha component per non-seed frame. It removed 18 pixels from Shield Arc frame 02 and one disconnected pixel from the corrected Charge Lane frame 03; no main silhouette was repainted.
- The first Charge Lane attempt is retained under `source/rejected/`, `frames/rejected/`, and `previews/rejected/`. Its normalized travel frame touched the left frame edge, violating the authored gutter/crop requirement. It is not a candidate output.

## Exact prompts

### Shield Arc

```text
Create exactly one horizontal four-frame animation strip of the locked Sir Caldus character for Shield Arc: reference guard idle, shield-raised windup, outward shield-sweep release, and lowered-shield recovery. Preserve blackened iron plate, tarnished brass edging, central brass helmet stripe, closed vertical-bar visor, charcoal cowl, bells/chains, olive-black torn cloth, and the dark coffin/kite shield on screen-right. Use a 55-degree top-down three-quarter camera facing down-screen, one complete full-body character per equal slot, shared foot baseline, and wide empty gutters. Use a uniform #FF00FF chroma-key field. Make crisp dark-gothic pixel art readable after 192 px nearest-neighbor normalization. No weapon, extra limbs, effects, telegraph, text, UI, shadow, scenery, grid, or frame crossing.
```

### Charge Lane correction

```text
Create exactly one horizontal row of four equal square slots for the locked Sir Caldus Charge Lane: reference guard idle, compact windup, forward-driving down-screen travel, and braking recovery. Preserve the locked identity, screen-right shield, downward-facing 55-degree top-down camera, palette, scale, and shared foot baseline. Every sprite pixel must remain inside its own slot; each side requires at least ten percent empty #FF00FF gutter, especially the travel frame's left edge. Use crisp dark-gothic production pixel art only. No lane, projectile, glow, blur, text, UI, shadow, scenery, extra limbs, duplicate character, separator, or frame crossing.
```

## Review result and current next step

At 192 px and 96 px, the helmet/visor, brass shield mass, planted/forward movement language, and recovery poses remain visually distinct. Alpha corners are transparent; every accepted frame has exactly one connected silhouette, a shared bottom-center anchor, and no frame-edge crop. Standard and reduced-effects mocks use the identical action-frame geometry and retain hostile telegraphs above the candidate sprite.

This is a **candidate pass**, not a runtime pass. It has not received in-engine motion playback, collision/hurtbox/origin review, asset-registry validation, native capture, animation-state integration, or content-hash promotion. It must not enable `core_world_flow_integration` or any normal route capability.

Recommended `GB-M03-03G` Current Next Step update: add that the unregistered v2 pack now has reviewed Shield Arc and Charge Lane strips with transparent 192 px shared anchors, 96 px readability, and static standard/reduced camera mocks; the remaining art gate is in-engine motion/anchor/collision-origins review and native evidence before any registry/hash proposal.

## Provenance and licensing

No third-party image, game, artist, brand, trademark, or style name was provided to the generator. The pack records raw source, transformations, prompts, rejected output, hashes, and inspection evidence. It is not independent commercial-rights or visual-similarity clearance; normal project review remains required before any runtime use.
