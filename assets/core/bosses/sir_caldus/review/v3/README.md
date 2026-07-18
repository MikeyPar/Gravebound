# Sir Caldus idle and recovery review pack v3

## Scope

This unregistered candidate adds the two remaining non-attack transition strips needed to review Sir Caldus as a coherent M03 boss renderer: a four-frame guard idle loop and a four-frame post-attack recovery-to-guard loop. It does not register `sprite.boss.sir_caldus`, modify content hashes or runtime bundles, enable the private route, or define simulation timing.

The pack follows all three design authorities:

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-002`, `ART-001` through `ART-006`, `ART-020`, `ART-030`, and `ENC-010`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-007`, `CONT-BOSS-001`, `CONT-BOSS-002`, and `CONT-PATTERN-003`.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, its one-major-boss target, and its temporary-asset/ship-quality requirements.

It derives from the locked v1 guard seed (`../v1/sir-caldus-idle-guard.seed.png`, SHA-256 `da0c0b90ca1cc390ba4a59ea3a8991313c95f2a2a339123c36b174da99ad4ea5`) and is visually continuous with v2's attack strips. Source revision: `1f9d04d756017c3ea8daed0c22bfc16d2c1d5c11`.

## Accepted candidates

| Strip | Frames | Seed lock | Intended presentation reading |
|---|---|---|---|
| `frames/idle` | guard, rise/settle, opposite weight shift, return guard | Frame 01 is byte-identical to v1 seed. | Subtle active boss idle; no gameplay timing implied. |
| `frames/recovery` | lowered-shield recovery, shield return, near guard, guard | Frame 04 is byte-identical to v1 seed. | Post-attack return-to-guard; no attack effect or timing implied. |

Every accepted frame is 192 x 192 RGBA, has one connected alpha silhouette, transparent corners, a shared bottom-center anchor `(96,192)`, and content clearance from the top and side frame boundaries. Non-seed frames use the v1/v2 176 px content height before bottom-center placement.

`previews/` contains 192 px and nearest-neighbor 96 px contact sheets, plus clearly watermarked review mocks at standard/reduced effects and both 1280 x 720 and 1920 x 1080. These are static composites over existing native evidence and retain the symbolic Caldus marker; they are not native captures.

## Generation and normalization

- Generation: OpenAI built-in image generation using only the locked v1 seed and an accepted v2 contact sheet as visual references.
- Chroma removal: the built-in-path helper `C:/Users/micha/.codex/skills/.system/imagegen/scripts/remove_chroma_key.py` with `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`.
- Initial strip splitting: game-studio's `normalize_sprite_strip.py`, four horizontal slots, 192 px output, v1 guard anchor.
- Final normalization: one 176 px maximum content height and bottom-center placement across each non-seed frame, followed by 4-connected largest-component cleanup at alpha threshold 8. The cleanup removed only disconnected chroma/resize debris; no silhouette pixels were repainted.
- Rejected material is retained under `source/rejected/`, `frames/rejected/`, and `previews/rejected/`. The first generated sheets were rejected because normalized silhouettes contacted the top canvas edge; one recovery intermediate was also rejected because its seed lock incorrectly replaced the intended lowered-shield opener.

## Exact prompts

### Idle loop correction

```text
Create exactly one horizontal row of four equal slots for Sir Caldus: neutral guard, a slight breathing/cloak/bell settle, an opposing subtle weight shift with shield stable, and return guard. Preserve the locked blackened-iron/tarnished-brass/closed-visor/cowl/bells/olive-cloth/coffin-shield identity, a 55-degree top-down three-quarter down-screen camera, screen-right shield, and shared foot baseline. Keep every silhouette at least ten percent of a slot height below the top and eight percent from both side gutters. Use only uniform #FF00FF chroma key and crisp dark-gothic pixel art. No attack, effects, UI, text, scenery, shadow, crop, or extra limbs.
```

### Recovery loop correction

```text
Create exactly one horizontal row of four equal slots for Sir Caldus: lowered-shield recovery with torso rising, shield return, near guard with cloth/bells settling, and neutral guard. Preserve the locked blackened-iron/tarnished-brass/closed-visor/cowl/bells/olive-cloth/coffin-shield identity, a 55-degree top-down three-quarter down-screen camera, screen-right shield, and shared foot baseline. Keep all pixels inside their slots with ten percent top clearance and eight percent side gutters. Use only uniform #FF00FF chroma key and crisp dark-gothic pixel art. No attack, effects, UI, text, scenery, shadow, crop, or extra limbs.
```

## Review state and limitation

Static review passes at 192 px and 96 px: visor, shield mass, stable guard, subtle idle shifts, lowered-shield recovery, and return guard are legible. The standard/reduced mocks retain telegraph hierarchy and use identical sprite geometry in each effects mode.

This remains a candidate pass only. In-engine animation playback, authored attack-origin/hurtbox alignment, collision review, frame cadence selection, registry validation, content-hash promotion, and native evidence remain separate gates. No generated art may alter M03 route admission before those gates close.

## Provenance

No third-party image, game, artist, brand, trademark, or style name was provided. The source manifest records raw and alpha sources, frame and preview hashes, transforms, prompts, and rejected attempts. It is not commercial-rights or visual-similarity clearance; normal project review remains required before runtime use.
