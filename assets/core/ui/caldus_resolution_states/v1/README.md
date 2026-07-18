# Sir Caldus resolution-state review pack v1

> **UNREGISTERED REVIEW CANDIDATE — NOT RUNTIME — NOT A CONTENT-HASH INPUT**

## Scope and route correction

This pack fills one narrow presentation gap at the Caldus defeat-to-reward boundary: a shape-first two-state HUD badge pair for `boss defeated / reward unresolved` and `reward committed / inventory still at risk`. It does not duplicate the existing Caldus defeat strip, pending-loot markers, or stable post-reward exit. Those accepted review candidates appear only in clearly watermarked static mocks so the full sequencing relationship can be judged.

The request called this the “B5 Caldus” slice, but the canonical route does not: `CONT-ROOM-007` defines B5 as `room.bell.bridge_01` combat and B6 as `arena.boss.caldus_01`. This pack is therefore B6-only. It makes no runtime, registry, content-hash, room, route, reward, inventory, or extraction change.

## Three-authority alignment

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DNG-003/005/006`, `ENC-005/010`, `LOOT-002/010`, `DTH-011`, `UI-003/005/006`, and `ART-001/002/004-006/020/030` require server-owned room/boss/reward/extraction state, personal loot plus a stable exit, explicit pending-risk communication, shape-first visual hierarchy, deterministic sprite packaging, and native review.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-007` places Sir Caldus at B6; `CONT-BOSS-001/002` defines combat and creates the stable exit only on committed boss reward; `CONT-REWARD-003/004` defines the personal Caldus reward and the Core override. The badge never encodes a roll, amount, item identity, or destination.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-04`, and `GB-M03-08` require the private route, pending inventory, and extraction/Recall semantics. An art projection cannot close those work packages or enable normal admission.

## Candidate states

Both files are `96 × 96` RGBA, use center anchor `[48,48]`, have transparent corners, and remain readable at the `48 px` minimum review scale.

| Frame | Stored presentation projection | Dominant non-color cue | Alpha bounds | SHA-256 |
|---|---|---|---:|---|
| `frames/state/01.png` | `DefeatedRewardUnresolved` | large open upper seal gap | `(7,9)-(88,96)` | `5702fa22ff475a01240c372b69416d53688fcc72e58d8d9f0dea07c93c7618a6` |
| `frames/state/02.png` | `RewardCommittedAtRisk` | upper receipt clasp plus broken lower tether | `(7,0)-(88,96)` | `0fa2163b2217c514754bdf6e19ad72ebfa0abcbcfd038a5805e24ff5348c91b7` |

The closed upper clasp communicates a durable reward result, while the broken lower tether deliberately prevents the second state from reading as secured inventory, successful extraction, Vault storage, or a completed victory medal. Both states have two four-connected alpha components at threshold 8 because the inert bell remains visually detached from its surrounding authority seal.

## Source, processing, and provenance

- Generation: OpenAI built-in image generation, one two-slot strip in a single pass.
- Raw chroma source: `source/caldus-resolution-state-strip.raw-chroma.png`, `1774 × 887` RGB, SHA-256 `d0dbacfd67252704c82644993fa690f359b1b50e3314ae3f409000de697f758f`.
- Alpha source: `source/caldus-resolution-state-strip.alpha.png`, `1774 × 887` RGBA, SHA-256 `6b6c1242d2503fea46c31456dc783e813049dbca94b137854009da7f83a5b945`.
- Chroma removal used the installed helper with `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`; sampled key was `#f905e6`.
- The installed sprite normalizer split two equal slots, applied one shared scale, and centered them on `96 × 96` canvases.
- `source/build_review_artifacts.py` deterministically rebuilds the exact/minimum sheets and all four static mocks. It reads the existing Caldus defeat, pending-risk, and stable-exit candidates only for review composition.
- No third-party image, game, artist, brand, trademark, or style name was supplied.

## Exact generation prompt

```text
Use case: stylized-concept
Asset type: two-state 2D game HUD badge strip for the Gravebound M03 Sir Caldus boss-resolution boundary.
Primary request: Create exactly one horizontal row of two equal square icon slots. The pair must be the same compact iron-and-ash status seal, changing only its authoritative presentation state.
State 1, left: DEFEATED / REWARD UNRESOLVED. Show a small intact downward-facing tarnished-brass bell inside a clearly incomplete open ash-gray circular seal. The bell is inert and silent; the open gap is the dominant non-color cue that resolution is not committed.
State 2, right: REWARD COMMITTED / ITEMS STILL AT RISK. Show the same bell and same seal now closed by one small angular iron receipt clasp, with a short broken ember-red tether hanging below to preserve the separate “at risk pending” read. This is not secured loot and not extraction.
Scene/backdrop: perfectly flat uniform solid #FF00FF chroma-key background edge to edge and between icons; no floor, shadows, gradients, texture, reflections, separators, frames, or lighting variation in the background.
Style/medium: professional dark-gothic 2D pixel-art game UI, deliberate hard pixel clusters, restrained wet-charcoal iron / ash gray / tarnished brass / dark ember-red palette, shape-first accessibility, readable when normalized to two 96x96 RGBA icons and at 48px.
Composition/framing: exactly one complete centered seal in each equal slot; identical scale, geometry, camera, and center anchor; generous blank gutters; no crop or slot crossing.
Constraints: no text, letters, numbers, tooltip, panel, character, knight, weapon, shield, skull, blood, gore, chest, treasure, coin, item icon, loot beam, portal, doorway, safe-zone ring, healing cross, green, cyan, violet, bright gold glow, hostile projectile, attack telegraph, smoke, particles, rays, watermark, or #FF00FF inside either badge. State 2 must not resemble “safe,” “secured,” Vault, extraction, completion checkmark, padlock, or victory medal. Do not add a third object or icon.
```

## Review evidence

- `previews/caldus-resolution-states.96px.png`: exact-scale alpha checkerboard, SHA-256 `d45fb33a74baffaf787d50f3afa1c2748fc4a141afd03611883c10d424bf5842`.
- `previews/caldus-resolution-states.48px.png`: nearest-neighbor minimum-scale check, SHA-256 `2ac149300622ef2aaeb20ab05eaf7e42e0527992b152f36617a9a547cef78c1e`.
- Standard/reduced static review mocks exist at `1280 × 720` and `1920 × 1080`, are explicitly watermarked as non-native, preserve the playfield, show the inert Caldus silhouette in both states, keep the exit hidden while unresolved, and combine the committed badge with the existing pending-risk frame and stable-exit candidate.
- Mechanical validation passed for RGBA mode, exact dimensions, transparent corners, alpha bounds, two components per state, preview dimensions, and deterministic rebuild output.
- At the `48 px` minimum scale, `18.84%` of alpha samples differ between states and `29.99%` of grayscale samples differ by at least 12/255, confirming that the distinction does not depend on the ember accent alone.
- Original-resolution visual inspection passed for shared identity/scale, no crop or slot crossing, no text or combat cue baked into the icons, clear open-versus-clasped distinction, and a persistent at-risk tether at 48 px.

## Integration recommendation and remaining gates

Use these as supplemental HUD projections beside explicit localized status text; do not replace the existing Caldus defeat sprite, pending-item risk frame, or B6 stable-exit renderer asset.

Before any registry proposal:

1. The unresolved badge may appear only from stored boss-defeated/reward-unresolved authority, never merely because the defeat animation ended.
2. The committed-at-risk badge and stable exit may appear only after the durable personal reward commits. Retry must return the same reward/placement and exit identity.
3. UI text must continue to state that pending items are lost on death and Emergency Recall until successful extraction; this emblem never means “safe.”
4. Response loss, reconnect, restart, database outage, rollback, and stale frames must never cause a transient false-commit display.
5. Optimized native B6 capture must pass at `1280 × 720` and `1920 × 1080` in standard/reduced effects plus grayscale/colorblind review, along with HUD hierarchy, content-hash, rollback, licensing, and visual-similarity gates.

No Current Next Step, root README, task status, content registry, Rust module, executable, distribution artifact, commit, or push is changed by this pack.
