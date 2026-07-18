# M03 Sir Caldus combat-silhouette drafts

## Status and scope

These two PNGs are non-runtime concept-art drafts for the missing `sprite.boss.sir_caldus` family. They are not registered in a runtime manifest, are not an asset-bundle replacement, and must not enable any route, capability, or later-stage content.

The drafts belong to M03 only:

- `Gravebound_Production_GDD_v1_Canonical.md`: the Core Prototype has one boss; `SIM-001` fixes 32 x 32 source environment tiles and `SIM-002` fixes the top-down orthographic camera. `ENC-010` defines Sir Caldus, Bell-Bound Knight, including his shield arcs and charge lane.
- `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-007` fixes M03 to `B0 -> B1 -> B2 -> B3 -> B4 -> B5 -> B6`; `CONT-BOSS-001` and `CONT-BOSS-002` bind the B6 `boss.sir_caldus` encounter to Shield Arc and Charge Lane behavior.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` owns the six-room dungeon -> boss -> Hall route. The M03 burn-up permits one major boss and 35 percent ship-quality share; temporary assets must be labeled and cannot enter M07 bundles.

## Files and verification

| File | Intended pose | Dimensions / mode | SHA-256 | Alpha verification |
|---|---|---|---|---|
| `sir-caldus-shield-arc-draft-alpha.png` | Shield-ready / `boss.caldus.shield_arc` art direction | 1672 x 941, RGBA | `b5353ad590a9188a939e22cc075e86d687498f370d2a34cf3632a173124b1c10` | transparent corners; nontransparent bounding box `(507,70)-(1185,871)` |
| `sir-caldus-charge-draft-alpha.png` | Charge-launch / `boss.caldus.charge_lane` art direction | 1254 x 1254, RGBA | `803e890c3ec9028e885924605c9983a772c3357709db35acbda6d1e1a3fe91c4` | transparent corners; nontransparent bounding box `(201,76)-(1035,1173)` |

Both alpha PNGs were visually inspected after background removal. They contain no requested text, label, logo, watermark, UI, or third-party mark.

## Provenance

- Generation method: OpenAI built-in image generation.
- Source staging files: `tmp/m03-asset-generation/sir-caldus-combat-silhouette/` (ignored, not part of this handoff).
- Background removal: generated on a flat `#ff00ff` chroma-key background, then processed with the installed `remove_chroma_key.py` helper using `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`.
- Visual reference inspected: the project-owned `assets/core/death/core_death_portraits.runtime.png` atlas, whose ninth cell maps to `portrait.boss.sir_caldus`. The reference informed the project palette only: wet stone, tarnished brass, ash, moss, bone, and restrained candlelight.
- No third-party image, artist, game, trademark, or style name was supplied to the generator.

## Exact prompts

### Shield Arc draft

```text
Use case: stylized-concept
Asset type: production draft reference for the missing M03 `sprite.boss.sir_caldus` top-down combat-silhouette family.
Primary request: create a single isolated, full-body top-down / 3-quarter game-sprite concept of Sir Caldus for a dark fantasy pixel-art action RPG. He is the Bell Sepulcher's only Core major boss: a tall armored knight, broad silhouette, tarnished brass and blackened iron plate, a barred visor or cage-like helm, a weathered dark mantle, several small ritual bells and heavy chain details, and a large battered shield held in a readable defensive posture. His stance must communicate the shield-arc attack and remain instantly legible from a top-down 16:9 combat camera.
Scene/backdrop: perfectly flat solid #ff00ff chroma-key background, no floor plane.
Style/medium: handcrafted 2D pixel-art game asset concept, chunky deliberate pixel clusters, crisp readable outline, restrained candlelit brass highlights, ash-black and moss-green shadow accents; coherent with a gothic portrait atlas featuring wet stone, tarnished brass, bone, ash, and candlelight. Not photorealistic.
Composition/framing: one centered full-body subject, generous even padding, no cropping, occupy about 70% of canvas height, clear feet and shield silhouette.
Lighting/mood: low-key sepulchral, readable brass rim light only on the subject.
Color palette: charcoal, umber, blackened iron, muted olive, antique brass; no vivid colors apart from the flat chroma key background.
Materials/textures: pitted iron, oxidized brass, thick weathered cloth, links of chain, worn leather.
Text (verbatim): none.
Constraints: background must be exactly uniform #ff00ff with no shadow, gradient, texture, reflections, floor, particles, or lighting variation. Do not use magenta in the subject. No cast shadow. No logos, UI, border, grid, labels, watermark, extra characters, weapons, floating effects, or text.
```

### Charge Lane draft

```text
Use case: stylized-concept
Asset type: alternate production draft reference for the missing M03 `sprite.boss.sir_caldus` top-down combat-silhouette family.
Primary request: create a single isolated, full-body top-down / 3-quarter game-sprite concept of the same Sir Caldus, now in a readable charge-launch posture. He is a tall dark-fantasy Bell Sepulcher knight in blackened iron and tarnished brass, with a barred cage-like helm, weathered black mantle, heavy chains, small ritual bells, and a battered shield turned aside to open a long forward charge lane. Lean him forward with one plated boot braced behind; preserve a very broad, unmistakable combat silhouette with the shield, mantle, helm, and bell chain still distinct.
Scene/backdrop: perfectly flat solid #ff00ff chroma-key background, no floor plane.
Style/medium: handcrafted 2D pixel-art game asset concept, chunky deliberate pixel clusters, crisp readable outline, restrained candlelit brass highlights, ash-black and moss-green shadow accents; visually coherent with a gothic 3 by 3 portrait atlas of wet stone, tarnished brass, bone, ash, and candlelight. Not photorealistic.
Composition/framing: one centered full-body subject, generous even padding, no cropping, occupy about 70% of canvas height, clear feet and charge direction.
Lighting/mood: low-key sepulchral, readable brass rim light only on the subject.
Color palette: charcoal, umber, blackened iron, muted olive, antique brass; no vivid colors apart from the flat chroma key background.
Materials/textures: pitted iron, oxidized brass, thick weathered cloth, links of chain, worn leather.
Text (verbatim): none.
Constraints: background must be exactly uniform #ff00ff with no shadow, gradient, texture, reflections, floor, particles, or lighting variation. Do not use magenta in the subject. No cast shadow. No logos, UI, border, grid, labels, watermark, extra characters, weapons, floating effects, or text.
```

## Intended sprite-sheet conversion

These high-resolution, illustration-style drafts are not drop-in game sprites. After art-direction selection:

1. Convert the selected silhouette into an approved, renderer-sized top-down pixel sprite sheet with a shared origin/foot anchor and collision/hurtbox review.
2. Produce readable idle, Shield Arc wind-up/release/recover, and Charge Lane wind-up/travel/recover frames. The pose timing must remain presentation-only and align with the authoritative `ENC-010` and `CONT-BOSS-002` telegraph schedule.
3. Validate readability at the GDD camera range of 20 x 11.25 to 30 x 16.875 tiles and in normal/reduced-effects modes.
4. Only after review, add the approved runtime derivative through the Core asset registry/hash pipeline. Do not replace existing content hashes with these concept files.

## Current Next Step

The exact route prerequisites through contiguous client delivery are green at `a893acf` under hosted CI [`29624519851`](https://github.com/MikeyPar/Gravebound/actions/runs/29624519851); the shared server writer is locally green at `71005d1` and awaits hosted proof. The art next step is to select and redraw the approved silhouette at renderer scale, establish one shared foot/origin anchor, and produce idle, Shield Arc, Charge Lane, and recover frames for camera/readability review in standard and reduced-effects modes. Only a reviewed derivative may enter the Core asset registry and content hashes. In parallel, `GB-M03-03G` proceeds through terminal-first composition, live simulation/terminal ownership, and real-QUIC proof; normal `core_world_flow_integration`, Character Select `Play`, and Realm Gate interaction remain disabled until those owning slices and evidence pass.

## Licensing and provenance review

The handoff records the generation method, exact prompts, source reference role, dimensions, hashes, and post-process method. No external asset was copied or provided as model input, and no third-party brand or artist was named. Before a runtime derivative ships, the project owner should still perform its normal commercial-rights, artifact provenance, visual-similarity, and repository asset-registry review; this note is provenance evidence, not an independent legal clearance.
