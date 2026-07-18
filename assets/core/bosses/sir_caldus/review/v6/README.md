# Sir Caldus defeat-transition review pack v6

> **UNREGISTERED REVIEW CANDIDATE — NOT RUNTIME — NOT A CONTENT-HASH INPUT**

## Scope and selection rationale

This is one cohesive four-frame candidate for the last genuinely uncovered Sir Caldus character state in the remaining M03 B0-B6 route: boss defeat and silence before the personal reward transaction commits. Existing v1-v5 candidates already cover Caldus identity, idle/recovery, Shield Arc, Charge Lane, ordinary Bell Ring, and Charge Stop Ring. Separate packages already cover pending-loot risk and the stable post-reward B6 exit. No prior pack covers the defeat-to-reward boundary.

The final frame deliberately retains an intact, inert Caldus silhouette. It communicates only `boss defeated / combat ended`; it does not depict a reward, secured inventory, stable exit, extraction, safe zone, or durable success. This pack does not create a new asset ID: it is a possible state within the existing unregistered `sprite.boss.sir_caldus` candidate.

## Three-authority alignment

1. `Gravebound_Production_GDD_v1_Canonical.md`: `ENC-010` defines Sir Caldus and his complete combat phases; `DNG-006`, `DTH-011`, `LOOT-010`, `TECH-004`, and `ART-001/002/004-006/020/030` keep boss/terminal/reward authority server-side while fixing the boss footprint, strip-first pipeline, visual hierarchy, metadata, and review requirements.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-002`, `CONT-ROOM-007`, `CONT-BOSS-001`, `CONT-BOSS-002`, and `CONT-REWARD-003/004` bind B6, Caldus combat, personal rewards, hostile cleanup, and the stable exit. The stable exit is created only after the boss reward commits; this art cannot advance that state.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-04`, and `GB-M03-08` require the complete private route, pending inventory, and extraction/Recall semantics before normal admission. `GB-M03-03` owns the one Core major boss and the M03 asset burn-up.

The live `GB-M03-03/03G` Current Next Step requires the fixed dungeon owner to join the persistent 30 Hz route, followed by authoritative Caldus combat, committed rewards/pending inventory, the stable B6 exit, and all five terminal producers. This pack changes none of those gates.

## Candidate frames

All files are `192 x 192` RGBA, use bottom-center anchor `[96,192]`, have transparent corners, and contain one four-connected silhouette at alpha threshold 8.

| Frame | Presentation read | Bounds | SHA-256 |
|---|---|---:|---|
| `frames/defeat/01.png` | Exact locked guard seed | `(36,16)-(156,192)` | `da0c0b90ca1cc390ba4a59ea3a8991313c95f2a2a339123c36b174da99ad4ea5` |
| `frames/defeat/02.png` | Decisive lethal stagger | `(21,16)-(171,192)` | `faaa902fc7189dbbd14a3386a79ccec03c8a22d7afa48f376aba75b35d65aee7` |
| `frames/defeat/03.png` | One-knee controlled collapse | `(25,57)-(167,192)` | `ef12d80438b91c19a2a837b95e0acf033bc2068909698b836aa7992c54f31258` |
| `frames/defeat/04.png` | Intact inert pre-reward silhouette | `(15,70)-(177,192)` | `f8cd7015a02016fd8d7ecb830e1923ce4b8a1999bfa79583c4824ea03bc82083` |

Frame 1 is byte-identical to the locked v1 guard seed. Generated frames 2-4 retain one shared scale correction (`176/188`) and the common bottom-center anchor, so the character naturally loses height through the collapse without per-frame scale drift. Uncompressed memory is `147,456` bytes per frame and `589,824` bytes for the strip.

## Source and provenance

- Generation method: OpenAI built-in image generation.
- Inputs: the locked v1 Caldus seed and the accepted v3 recovery contact sheet, used only for identity and motion continuity.
- No third-party image, game, artist, brand, trademark, or style name was supplied.
- Raw source: `source/sir-caldus-defeat.raw-chroma.png`, `2172 x 724` RGB, SHA-256 `abacad68fa1a18d4c0eba2d972907d10899b6c4f72f1902118d0f5c9200c81f3`.
- Alpha source: `source/sir-caldus-defeat.alpha.png`, `2172 x 724` RGBA, SHA-256 `743b3e8ae645c3dcc191fd3dd61c463dee5bc04c152a3e420f8e9bcf3f41b94b`.
- Chroma removal used the installed image-generation helper with `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`; sampled key was `#f903f8`.
- Strip normalization used the installed sprite-pipeline normalizer, four slots, a `192 x 192` frame, the v1 anchor, and frame-one lockback.
- Static review found only resize debris after the shared scale correction. Largest-four-connected-component cleanup at alpha threshold 8 removed 21/11/15 isolated pixels from frames 2/3/4. No main-silhouette pixel was repainted.

## Exact generation prompt

```text
Use case: stylized-concept
Asset type: 2D top-down boss defeat animation review strip for Gravebound
Input images: Image 1 is the immutable Sir Caldus identity, scale, palette, and frame-one seed; Image 2 is an accepted recovery contact sheet used only for motion continuity and the established sprite language.
Primary request: Create exactly one horizontal row of four equal square sprite slots showing Sir Caldus's one-way authoritative-defeat presentation, ending in a silent inert pre-reward pose.
Subject: Preserve exactly the established masked blackened-iron knight, tarnished brass trim, closed vertical-bar visor with central brass stripe, charcoal cowl, chest bells and chains, olive-black torn cloth, and large dark coffin/kite shield with brass ribs on screen-right. Same identity, proportions, palette, shield scale, armor construction, and camera read in every slot.
Camera/composition: orthographic 55-degree top-down three-quarter game camera, facing down-screen; exactly one complete character in each equal slot; one shared bottom-center ground baseline; consistent apparent scale; generous blank gutters; all figure and shield pixels contained inside each slot.
Pose sequence left to right: 1 exact neutral guarded seed stance; 2 decisive lethal stagger with torso recoiling and shield slipping down but still screen-right; 3 controlled collapse onto one knee with shield edge grounded and helmet bowing; 4 final inert defeated pose, kneeling low with head bowed and shield resting beside the body, unmistakably non-combatant but still a compact readable intact knight silhouette. The final pose is only boss defeat/silence before reward commitment, not a death explosion, loot event, portal, or safe state.
Scene/backdrop: perfectly uniform flat solid #FF00FF chroma-key background edge to edge and between figures, no variation.
Style/medium: crisp authentic dark-gothic production pixel art, hard readable silhouettes, deliberate pixel clusters, restrained wet-stone black iron/tarnished brass/bone/moss palette, designed for nearest-neighbor normalization into four 192x192 boss frames.
Constraints: no blood, gore, severing, skeleton, corpse decay, disappearance, ash cloud, smoke, particles, magic, glow, projectiles, telegraphs, damage numbers, reward, item, loot beam, chest, exit, portal, safe-zone ring, text, labels, UI, shadows, floor, scenery, reflections, watermark, weapon, extra limbs, duplicate character, separator, crop, or frame crossing. Do not use #FF00FF anywhere in the knight. Frame 4 must keep the full defeated silhouette present so renderer visibility cannot imply that the reward transaction committed.
```

## Preview and visual inspection

- `previews/sir-caldus-defeat.192px.png`: exact four-frame checkerboard sheet, SHA-256 `cb96d6e31587f3d5581e0a435b6e20251a12ed5f9b39babb03d0b6c1381e2f52`.
- `previews/sir-caldus-defeat.96px.png`: nearest-neighbor minimum-scale sheet, SHA-256 `380c5d7fd40a702a32b8848fcfdcfef189c796cdb9efe6bf1e2ed2bfa32a316f`.
- Four clearly watermarked standard/reduced-effects mocks cover `1280 x 720` and `1920 x 1080`. They use the existing final-rings evidence strictly as a worst-case silhouette-readability backdrop. They are static composites, not native captures, and do **not** propose that hostile rings remain after defeat.

Original-resolution inspection found no crop, frame crossing, shield-side reversal, identity redesign, gore, disappearance, baked combat cue, reward, exit, safe-state symbol, or connected-component debris. At 96 px, guard, lethal stagger, knee collapse, and the low inert silhouette remain distinct. The final silhouette stays identifiable in both effects modes and does not resemble a loot pickup, player marker, exit, or hostile projectile.

## Authority boundaries and review gates

The three authorities do not specify Caldus's exact defeat body animation. This strip is therefore a non-authoritative presentation recommendation. The following remain mandatory before any registry or content-hash proposal:

1. Authoritative lethal resolution cancels Caldus actions and prevents any further boss attack before renderer defeat playback.
2. Hostile cleanup, personal reward resolution, pending-inventory placement, and stable-exit creation remain server/durable events; animation completion cannot trigger or delay them.
3. The defeated silhouette must be distinguishable from a four-second `+25% incoming` phase break, ordinary recovery, player death, and LinkLost/fault states.
4. In-engine playback must prove shared anchor/scale, no apparent collision or hurtbox change, no sprite-origin drift, and no obstruction of remaining authoritative effects.
5. The UI must show the authoritative reward-pending/committed state independently. The B6 exit candidate may render only after the committed reward and stable-exit authority.
6. Standard/reduced effects, grayscale/colorblind settings, both certified resolutions, optimized native capture, registry validation, rollback evidence, licensing, and visual-similarity review must pass.

## Licensing note

This pack records exact source roles, prompt, transformations, dimensions, anchors, hashes, review contexts, and authority boundaries. It is not independent commercial-rights or visual-similarity clearance.
