# Sir Caldus Charge Stop Ring review pack v5

> **UNREGISTERED REVIEW CANDIDATE — NOT RUNTIME — NOT A CONTENT-HASH INPUT**

## Scope and priority

This single cohesive candidate pack fills the highest-priority uncovered Sir Caldus presentation gap in the remaining fixed B0-B6 route: the immediate `boss.caldus.charge_stop_ring` follow-through after Charge Lane. Earlier unregistered packs already cover the locked Caldus identity, Shield Arc, Charge Lane, idle/recovery, and ordinary Bell Ring. Separate existing candidates cover pending-loot risk and the post-reward B6 exit, so this pack does not duplicate either surface.

The strip is pose-only. It does not register `sprite.boss.sir_caldus`, modify `content/core_dev/caldus.assets.json`, enter any runtime/content hash, alter route admission, or define projectile count, gap direction, timing, attack origin, collision, damage, reward eligibility, reward commitment, or exit visibility.

## Three-authority basis

1. `Gravebound_Production_GDD_v1_Canonical.md`: `ENC-010` fixes the Phase 2 Charge Lane and immediate 14-projectile Stop Ring with the opposite two-shot gap; `ART-001`, `ART-002`, `ART-004`, `ART-005`, `ART-006`, `ART-020`, and `ART-030` fix the dark-fantasy pixel direction, 96-192 px boss footprint, strip-first pipeline, combat hierarchy, manifest metadata, and review gates.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-002`, `CONT-ROOM-007`, `CONT-BOSS-001`, `CONT-BOSS-002`, and the registered Core Caldus asset inventory bind the B6 arena, exact charge/stop scheduler, authored boss radii, stable-exit gate, and `boss.caldus.charge_stop_ring` presentation family.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` owns the Core Character Select -> Hall -> micro-realm -> six-room dungeon -> boss -> Hall route, one Core major boss, and the M03 cumulative asset burn-up. Temporary candidates remain labeled and outside release bundles.

The current `GB-M03-03/03G` handoff requires fixed B0-B6 combat, Sir Caldus, committed rewards, pending inventory, and a stable B6 exit before normal admission. This pack advances only the dependency-neutral Caldus art review lane while that runtime work proceeds.

## Candidate output

The four-frame strip is stored under `frames/charge_stop_ring/`:

| Frame | Pose/read | Bounds at alpha >= 8 | SHA-256 |
|---|---|---:|---|
| `01.png` | Byte-locked terminal Charge Lane braking pose from v2 | `(26,13)-(166,192)` | `b09b565c36f8e97398ef8d3f7e937cd72080e15291d1d35a64bb333b69130f52` |
| `02.png` | Planted stop; bell-cluster recoil | `(36,16)-(156,192)` | `0e8f5756cdf485d74a571d20074c6c3589c32c87b1e0087f9054afab542076c8` |
| `03.png` | Compact radial-release commitment | `(18,16)-(174,192)` | `e3865a5f7558af07f4f65c6792762e0a574352c1b40d0da7c2a18b86c97e20ce` |
| `04.png` | Guarded recovery | `(36,16)-(156,192)` | `af31d824374f5185192a16ab39dfc2e333308f4912063326961aaf035d919e38` |

Every frame is `192 x 192` RGBA with bottom-center anchor `[96,192]`, transparent corners, one connected silhouette at alpha threshold 8, and positive top/side clearance. Frames 2-4 share a normalized 176 px content height; frame 1 stays byte-identical to the accepted v2 terminal pose for seamless Charge Lane handoff. Uncompressed memory is `147,456` bytes per frame and `589,824` bytes for the strip.

## Sources and processing

- Generation method: OpenAI built-in image generation.
- Visual references: the locked v1 Caldus seed and accepted v2 Charge Lane contact sheet only. No third-party image, game, artist, brand, trademark, or style name was supplied.
- Raw chroma source: `source/sir-caldus-charge-stop-ring.raw-chroma.png`, `2172 x 724` RGB, SHA-256 `4f4dd4d21b9bbc206557bde09cb8a3e26eeb0cebb7770e69abf427d2678c292d`.
- Alpha source: `source/sir-caldus-charge-stop-ring.alpha.png`, `2172 x 724` RGBA, SHA-256 `44ad57ddbd63f50d1dd07d76d74a831187c12b80f87100eac891f6d68af09c92`.
- Chroma removal used the installed image-generation helper with `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`; sampled key was `#f605f1`.
- The sprite-pipeline normalizer split one four-slot source, applied one shared bottom-center frame contract, and locked frame 1 to v2 Charge Lane frame 04. Review then caught frames 2-3 touching the top edge; only normalization was corrected by fitting generated frames 2-4 to a shared 176 px maximum content height with bottom-center placement. No pose or silhouette was repainted.

Exact hashes and ART-020-style candidate metadata are in `sir-caldus-charge-stop-ring-review.source.json`.

## Exact generation prompt

```text
Use case: stylized-concept
Asset type: 2D top-down boss animation review strip for Gravebound
Input images: Image 1 is the immutable Sir Caldus identity/scale seed; Image 2 is a motion/style reference for the existing Charge Lane progression only.
Primary request: Create exactly one horizontal row of four equal square sprite slots showing Sir Caldus's immediate Charge Stop Ring follow-through, pose-only.
Subject: Preserve exactly the established masked blackened-iron knight, tarnished brass trim, closed vertical-bar visor with central brass stripe, charcoal cowl, chest bells and chains, olive-black torn cloth, and large dark coffin/kite shield with brass ribs held on screen-right. Same proportions, palette, shield scale, and identity in every slot.
Camera/composition: orthographic 55-degree top-down three-quarter game camera, facing down-screen; exactly one complete full-body character in each equal slot; identical apparent scale; one shared bottom-center foot baseline; generous blank gutters; no character pixel may touch or cross a slot boundary.
Pose sequence left to right: 1 hard braking finish immediately after a forward shield charge, weight low and forward; 2 planted stop with torso recoiling upright and non-shield hand striking or lifting the chest bell cluster; 3 compact radial-release commitment with shoulders open and bell arm extended, shield still screen-right, clearly distinct from Shield Arc and ordinary Bell Ring; 4 guarded recovery settling back toward the neutral seed stance.
Scene/backdrop: perfectly uniform flat solid #FF00FF chroma-key background edge to edge and between figures, no variation.
Style/medium: crisp authentic dark-gothic production pixel art, hard readable silhouette, deliberate pixel clusters, restrained wet-stone black iron/tarnished brass/bone/moss palette, designed for nearest-neighbor normalization into four 192x192 boss frames.
Constraints: pose-only; no projectiles, rings, gap wedges, lanes, magic, particles, telegraphs, energy glow, motion blur, text, labels, UI, shadows, floor, scenery, reflections, watermark, weapons, extra limbs, duplicate characters, separators, or frame crossing. Do not use #FF00FF anywhere in the knight.
```

## Preview and inspection result

- `previews/sir-caldus-charge-stop-ring.192px.png` is the exact four-frame contact sheet; SHA-256 `eeaf3f50eb80c8d2bf83d1c977cbc1cf75d7f3ddc7ff52e615ff8666b0402a84`.
- `previews/sir-caldus-charge-stop-ring.96px.png` is the nearest-neighbor minimum-scale stress sheet; SHA-256 `401fb07bf91e920d7e3d547993bff6aa724ff1d8668001b291b78f7008f7a235`.
- Four clearly watermarked review mocks place the identical release frame over the existing charge-pressure evidence at standard/reduced effects and `1280 x 720`/`1920 x 1080`. They are static composites, retain the symbolic native marker, and are not native captures or gameplay evidence.
- Original-resolution inspection found no crop, slot crossing, scenery, shadow, text, effect, extra character, identity redesign, shield-side reversal, or disconnected alpha debris. At 96 px, the braking stance, bell-cluster stop, extended release hand, shield mass, and guarded recovery remain distinct. Standard and reduced mocks preserve the external lane/gap hierarchy because the sprite contains no hostile effect pixels.

## Scope boundaries and review gates

The pose is a non-authoritative art recommendation because neither design authority specifies Caldus's exact body gesture for Stop Ring. The authorities do specify that the Stop Ring emits immediately after Charge Lane and that its opposite gap, 14 projectiles, damage, speed, and scheduler are authoritative. A renderer must key any later animation to those events; animation state must never drive simulation or delay the ring.

Registration remains blocked until one combined v2-v5 in-engine review proves:

1. Charge Lane frame-04 -> v5 frame-01 continuity and no anchor/scale pop in motion.
2. Stop Ring release is visually distinct from ordinary Bell Ring and Shield Arc at minimum zoom.
3. The authored Caldus collision/hurtbox radii (`0.70/0.62`) and attack origin remain simulation-owned and unobscured.
4. The external 14-projectile ring and opposite gap remain readable in grayscale, colorblind settings, standard effects, and reduced effects at both certified resolutions.
5. B6 victory clears hostiles, committed personal reward precedes stable-exit visibility, pending loot remains visibly at risk, and this combat strip never appears during the safe post-reward state.
6. Optimized native capture, registry validation, content-hash review, licensing/similarity review, and rollback evidence pass before any runtime proposal.

## Provenance

This pack records the input roles, exact prompt, raw and normalized files, transformations, dimensions, anchors, hashes, static review contexts, and authority boundaries. It is not independent commercial-rights or visual-similarity clearance; the ordinary project review remains required before runtime use.
