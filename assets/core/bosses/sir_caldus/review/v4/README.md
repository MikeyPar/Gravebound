# Sir Caldus Bell Ring review pack v4

## Scope

This unregistered candidate supplies a four-frame, pose-only review strip for Sir Caldus's named `boss.caldus.bell_ring` family. It is deliberately separate from the violet ring, gap, projectile, collision, timing, and damage presentation. It does not register `sprite.boss.sir_caldus`, modify a content hash or runtime bundle, enable the private route, or define simulation timing.

It follows the three design authorities:

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-002`, `ART-001` through `ART-006`, `ART-020`, `ART-030`, and `ENC-010`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-007`, `CONT-BOSS-001`, `CONT-BOSS-002`, and `CONT-PATTERN-003`.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, its one-major-boss target, and its temporary-asset/ship-quality boundary.

The strip uses the locked v1 guard seed (`../v1/sir-caldus-idle-guard.seed.png`, SHA-256 `da0c0b90ca1cc390ba4a59ea3a8991313c95f2a2a339123c36b174da99ad4ea5`) as frame 01 and uses v3's accepted idle sheet only as a visual-continuity reference. Source revision: `beb547442f75a8129cd54175b364d6ba1eb2f83e`.

## Accepted candidate

| Strip | Frames | Seed lock | Intended presentation reading |
|---|---|---|---|
| `frames/bell_ring` | guard, chest-bell anticipation, hand-bell release, guarded return | Frame 01 is byte-identical to the v1 guard seed. | Bell-focused preparation and release; no ring, effect, collision, or timing is encoded in the pixels. |

All accepted frames are 192 x 192 RGBA, have one connected alpha silhouette at threshold 8, transparent corners, a shared bottom-center anchor `(96,192)`, a 16 px top margin, and positive side clearance. Non-seed frames use 176 px content height before bottom-center placement.

`previews/` contains 192 px and nearest-neighbor 96 px contact sheets, plus clearly watermarked static review mocks at standard/reduced effects and both 1280 x 720 and 1920 x 1080. The mocks composite frame 03 over existing phase-one evidence, replace the inherited attack badge with `BELL RING`, and retain the symbolic Caldus marker; they are not native captures.

## Generation and normalization

- Generation: OpenAI built-in image generation using the locked v1 guard seed and an accepted v3 idle contact sheet as references.
- Chroma removal: `C:/Users/micha/.codex/skills/.system/imagegen/scripts/remove_chroma_key.py` with `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`.
- Initial strip splitting: game-studio's `normalize_sprite_strip.py`, four horizontal slots, 192 px output, v1 guard anchor, and locked first frame.
- Final normalization: each non-seed silhouette was resized to 176 px maximum content height and placed at the shared bottom-center anchor. A 4-connected largest-component cleanup at alpha threshold 8 removed only isolated resize debris (three pixels from frame 02, four from frame 03, and five from frame 04); no silhouette shape was repainted.

## Exact prompt

```text
Use-case: stylized-concept. Generate a clean 2D raster animation review strip for the already-established Gravebound boss Sir Caldus. Reference image roles: first is the locked guard-pose seed; second is a four-frame motion/style contact sheet. Preserve the same masked black-steel knight identity, burnished gold trim, dark cloth tabard, ornate kite shield held on screen-right, and 55-degree 3/4 top-down game-camera read. One single horizontal row of exactly four equal square sprite slots on a perfectly flat solid #FF00FF chroma-key background. Each slot contains one complete full-body Sir Caldus with generous transparent-style padding and clearly blank #FF00FF gutters; no frame touches any edge. Pose progression must be the distinct Bell Ring attack family, pose-only: 1 guard / neutral start, 2 anticipation with non-shield arm drawing attention to the chest-bell cluster while shield eases aside, 3 restrained bell-ring release with chest/hand gesture forward, 4 return toward guarded stance. Shield must stay screen-right; do not make this a shield swing or a charging run. No rings, magic, particles, projectiles, telegraphs, energy glow, text, labels, UI, shadows, floor, reflections, watermark, extra characters, or background elements. The bright chroma key must not appear in the knight. High-fidelity painted dark-fantasy game sprite, crisp silhouette and readable gold bell cluster at reduced scale.
```

## Review state and limitation

Static review passes at 192 px and 96 px: the guard lock, chest-bell preparation, hand-bell release, screen-right shield, and recovery read distinctly from v2's Shield Arc and Charge Lane. The standard/reduced mocks use identical sprite geometry and preserve the external telegraph/gap hierarchy.

This remains a candidate pass only. In-engine playback, authoritative attack-origin/hurtbox alignment, collision review, exact frame cadence, registry validation, content-hash promotion, and optimized native evidence are separate gates. No generated art may alter M03 route admission before those gates close.

## Provenance

No third-party image, game, artist, brand, trademark, or style name was supplied. The source manifest records raw and alpha sources, frame and preview hashes, transforms, prompt, and static-mock inputs. It is not commercial-rights or visual-similarity clearance; normal project review remains required before runtime use.
