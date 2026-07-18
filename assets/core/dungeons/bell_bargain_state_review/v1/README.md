# Bell Bargain shrine state review pack v1

> **UNREGISTERED REVIEW CANDIDATE — NOT RUNTIME — NOT A CONTENT-HASH INPUT**

## Scope and gap selection

This pack fills one narrowly defined M03 presentation gap: the in-world `room.bell.rest_01` shrine had one accepted, unregistered `Open`-like landmark candidate, while the durable Core Bargain authority already exposes three relevant offer states: `Open`, `Selected`, and `Refused`. No existing asset distinguishes the two committed outcomes from the unresolved shrine.

The audit found no higher-priority uncovered raster state:

- Sir Caldus review v1-v6 already covers identity, attacks, idle/recovery, Bell Ring, Stop Ring, and defeat-to-pre-reward presentation.
- `bell_fixed_route_landmarks/v1` already covers B0, the base B4 shrine, and the gated B6 exit.
- `core_choice_icons.svg` and the completed `GB-M03-05D` surface already cover the exact three Bargain choices, copy, affected-stat comparisons, confirmation, refusal, and unavailable cells.
- Existing Bell enemy, hostile-VFX, pending-loot, microrealm-landmark, death, successor, inventory, and terminal packs cover their current M03 review seams.

This package therefore adds only three state variants of the existing B4 shrine:

| Frame | Server projection required | Presentation read | SHA-256 |
|---|---|---|---|
| `frames/state/01.png` | `Open` | Three equally lit talismans; unresolved optional choice | `c394735455ec95c7eed0449d67661786b9f7afd06d585a5c1ef23ab497c19b81` |
| `frames/state/02.png` | `Selected { bargain_id }` | One sealed center talisman; stored choice committed | `98b6193ad2742b02762886446c002f3cb7055843498bccdeb13f142126247770` |
| `frames/state/03.png` | `Refused` | Three inert talismans and a broken ash loop; refusal committed | `ff93b07aca480e82353986bf0a9f031201a3af6b86477fbfb91cce94f8e56fa0` |

All frames are `192x192` RGBA with bottom-center anchor `[96,192]`, transparent corners, and identical nontransparent bounds `(40,16)-(152,192)` at alpha threshold 8. Frame 01 is byte-identical to the existing unregistered B4 landmark seed. Frames 02-03 use one shared `176/192` nearest-neighbor scale correction so the shrine does not grow after a decision.

## Three-authority alignment

1. `Gravebound_Production_GDD_v1_Canonical.md`: `BRG-001`-`005` require exactly three legal choices, selection or penalty-free refusal, durable life persistence, affected-stat disclosure, and server-safe compatibility; `DNG-003`-`005` define the rest-room position within a server-owned dungeon state machine; `UI-001`/`002`/`010`/`011` and `ART-001`/`002`/`004`/`005`/`006`/`020`/`030` define view-model ownership, reduced-effects information parity, certified resolutions, restrained dark-fantasy pixel language, metadata, and review gates.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-014` defines immutable `Open -> Selected(candidate_id) | Refused` transitions and requires a selected Bargain to become life-persistent before UI closure; `CONT-ROOM-007` fixes B4 as the sole rest/Bargain room in `layout.core_private_life_01`; Core permits only Bell Debt, Cinder Hunger, and Lantern Ash; `CONT-VALID-001` requires deterministic reachable Bargain states.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` requires the complete B0-B6 private route, `GB-M03-05` requires one three-choice Bargain shrine, and the M03 gate requires a player-completable route without developer commands while art/UI burn-up remains validator-clean and authority-safe.

The live `GB-M03-03G` handoff still requires durable B4 route composition before normal admission. These images do not satisfy that implementation or evidence gate.

## Presentation choices versus gameplay authority

Presentation recommendations:

- `Open`: preserve the existing three equally lit talismans; no one option is visually preselected.
- `Selected`: retain all three talismans, darken the unchosen pair, and use one closed brass seal around the center presentation token. The renderer may later map the actual selected ID through UI/icon treatment; this review frame does not encode a Bargain ID.
- `Refused`: retain all three inert talismans and show a small broken ash loop distinct from the selected seal.
- Standard-effects mocks add four sparse, detached ambient motes around `Open` and `Selected`; reduced-effects mocks remove them. The three state sprites themselves remain identical between modes so mechanics are never color/effects-only.

Authoritative rules:

- Only a stored server projection may choose the frame. Client input, animation completion, proximity, or the shrine sprite cannot select/refuse a Bargain, apply mechanics, advance an aggregate version, open B5, or authorize `B4 -> B5`.
- A dropped response, in-flight mutation, unknown durable outcome, stale projection, or reconnect must not optimistically switch to `Selected` or `Refused`. The renderer should retain the last acknowledged state and show an independent pending/error UI.
- `Unavailable`, emergency one/two-candidate cells, and no-offer 10-Ash behavior remain native UI/authority states and are intentionally not represented by a fourth environmental sprite.
- The state art does not identify which of Bell Debt, Cinder Hunger, or Lantern Ash was chosen. Exact identity, boon/curse copy, before/after values, and unavailable cells remain in the authoritative Bargain panel.

## Generation and reproducibility

- Mode: OpenAI built-in image generation; one strip-first edit using the existing unregistered B4 shrine as the sole visual reference.
- Raw source: `source/bell-bargain-state-strip.raw-chroma.png`, `2172x724` RGB, SHA-256 `f54ad49d14ae2a1f477165790c3b8ea27cd6b1252784e665dbd2702dba8185f5`.
- Alpha source: `source/bell-bargain-state-strip.alpha.png`, `2172x724` RGBA, SHA-256 `de097daf2c0185796012f7add17ac2a5cce4b417f525fce949f5b2875d014419`.
- Chroma removal: installed image-generation helper with border auto-key (`#fb03f8`), soft matte, thresholds `12/220`, and despill.
- Strip normalization: installed sprite normalizer, three slots, `192x192`, shared bottom-center anchor, and exact frame-01 lockback. The two generated variants then receive one shared `176/192` scale correction.
- `source/build_review_artifacts.py` deterministically rebuilds scale correction, 192/96 checkerboard sheets, and all four clearly labeled static review mocks. It does not register assets or touch gameplay files.
- No third-party game, brand, artist, or commercial asset reference was supplied. The project-local seed is recorded in the source manifest.

## Exact generation prompt

```text
Use case: stylized-concept
Asset type: 2D top-down dark-fantasy game landmark state strip for the M03 Bell Sepulcher B4 Veil Bargain shrine
Input images: Image 1 is the existing unregistered B4 shrine candidate and is the immutable identity, architecture, scale, palette, camera, and bottom-anchor reference.
Primary request: Create exactly one horizontal row of THREE equal square sprite slots showing the same shrine in three server-owned presentation states, left to right: OPEN / SELECTED / REFUSED. Preserve the exact low cracked ash-stone altar, pointed iron arch, dark suspended censer, three hanging talisman positions, wet charcoal masonry, tarnished brass, top-down three-quarter camera, compact silhouette, and shared bottom-center baseline from Image 1.
State 1 OPEN: exactly three separate violet glass talismans glow equally; restrained incomplete violet halo segments behind them communicate an unresolved optional choice. No single talisman is emphasized.
State 2 SELECTED: exactly one center talisman remains lit and is enclosed by a compact closed tarnished-brass seal ring; the left and right talismans are visibly present but dark and inert. This means a stored choice was committed, not reward, healing, or safety.
State 3 REFUSED: all three talismans remain visibly present but are dark and inert; a small clean ash-white broken loop lies on the altar surface, clearly distinct from the closed selected seal, communicating voluntary refusal without penalty. No violet glow remains.
Scene/backdrop: perfectly uniform flat solid #FF00FF chroma-key background edge to edge and between slots; no floor plane, cast shadow, gradient, texture, reflection, border, separator, or lighting variation in the background.
Style/medium: crisp production-ready dark-gothic 2D pixel art, deliberate hard pixel clusters, restrained wet charcoal / black iron / tarnished brass / bone-ash palette, nearest-neighbor friendly, readable after normalization to three individual 192x192 sprites and at 96px.
Composition/framing: exactly one complete shrine centered in each equal slot, identical scale and anchor, generous blank gutters, no frame crossing, no crop.
Constraints: preserve shrine geometry and talisman count exactly; state changes only affect talisman light and the small presentation seal/loop. No text, letters, numbers, UI, tooltip, character, enemy, weapon, skull, treasure, chest, coin, loot beam, reward, exit, doorway, portal, healing symbol, safe-zone circle, hostile red telegraph, cyan, bright gold glow, smoke, particles, rays, extra talisman, extra object, watermark, or #FF00FF inside the shrine. Artwork is presentation only and must not imply that entering B5 is authorized.
```

## Review evidence

- `previews/bell-bargain-states.192px.png`: exact-scale alpha checkerboard, SHA-256 `5d9dc5db01b6cca526576af0be5baaab12d82ab85dea3c8aab04e60ec42b82fb`.
- `previews/bell-bargain-states.96px.png`: nearest-neighbor minimum-scale check, SHA-256 `7e1dcf7554ca1777ce1055b03e75cf6fa2be71f5d86c8d203712c35be52e3b6f`.
- Standard/reduced static review mocks exist at `1280x720` and `1920x1080`, are watermarked as non-native, preserve the center playfield, and show non-color labels for every state.
- Original-resolution visual inspection passed for exactly three talismans, consistent architecture/facing/anchor/scale, no crop, no frame crossing, clear closed-versus-broken seal distinction, no reward/exit/heal/safe-zone read, and readable state separation at 96 pixels.
- Mechanical inspection passed for RGBA mode, `192x192` dimensions, alpha bounds, transparent corners, locked seed identity, and deterministic preview dimensions. The seed's four alpha components are inherited unchanged; generated states each form one connected silhouette at alpha threshold 8.

## Remaining integration decision

Before registry or content-hash work, the renderer owner must choose one explicit binding strategy: three full shrine-state sprites, or one base shrine plus state overlays. The full-sprite strategy is the current recommendation because all three candidates share exact bounds and anchor, but only optimized native B4 review can validate tile/collision alignment, prompt priority, modal world-continuation behavior, reconnect/pending ordering, grayscale/colorblind distinction, and no apparent state change before the durable projection arrives.

No runtime registry, content hash, Rust module, root README, task status, Current Next Step, distribution artifact, feature gate, commit, or push is changed by this pack.

