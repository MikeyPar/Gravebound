# B5 Mire Bridge combat readability review pack v1

> **UNREGISTERED REVIEW CANDIDATE — NOT RUNTIME — NOT A CONTENT-HASH INPUT**

## Scope and non-duplicative gap

This isolated pack addresses the next missing fixed-dungeon presentation seam in `GB-M03-03G`: B5, `room.bell.bridge_01` / “Mire Bridge.” Existing candidates already cover the Drowned Pilgrim and Chain Sentry silhouettes. This pack deliberately does not redraw them. It adds the missing exact 23×11 room surface and a shape-first review treatment for the Chain Sentry’s alternating cardinal/diagonal cross-lane windup.

The art communicates four facts without becoming gameplay authority:

- the dry six-tile crossing is visibly separated from authored north/south deep water;
- the two authored floor channels at x=7.5 and x=15.5 remain quiet environmental guides, not active damage cues;
- the active Chain Sentry cast is a thick Physical Major lane with white core, red edge, and chain-notch secondary marks;
- standard and reduced-effects variants preserve identical mechanical bounds and non-color information.

## Three-authority alignment

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DNG-003`–`005` require server-owned activation/completion and a quiet post-clear interval; `ENC-001`–`002` define the Chain Sentry as a stationary Anchor with two perpendicular lane telegraphs; `ART-001`, `ART-002`, `ART-005`, `ART-006`, `ART-020`, and `ART-030` require muted environments, reserved hostile contrast, Physical Major shape/color language, hostile-effect priority, complete metadata, minimum-scale and grayscale readability, and no decorative false positives; `UI-010`–`011` require high-contrast/reduced-motion parity and 1280×720/1920×1080 review.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-007` fixes B5 as the fourth combat room in the B0→B6 chain. `CONT-ENEMY-001`–`002` fix the B5 encounter to six Drowned Pilgrims plus one Chain Sentry, a 900 ms spawn warning, and the Chain Sentry’s 900 ms alternating 0°/90° then 45°/135° cross-lane casts, width 0.9 tiles, room-collision length, 350 ms active time, Physical Major 28. `CONT-VALID-001` requires no spawn/telegraph overlap with the safe entrance and complete hostile-attack telegraph metadata.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` requires the ordinary Character Select→Hall→micro-realm→six-room dungeon→boss→Hall route; the M03 exit gate requires completion without developer commands and full-loop proof. This pack supports—but cannot satisfy—that runtime gate.

The task/evidence audit also consulted `docs/tasks/GB-M03-03G.md`, `docs/evidence/GB-M03-03G-fixed-dungeon-combat-owner-evidence.md`, `docs/evidence/GB-M03-03G-live-fixed-room-driver-evidence.md`, and `docs/evidence/GB-M03-03G-durable-b4-task-binding-evidence.md`.

## Files and intended runtime contract

| File | Role | Contract |
|---|---|---|
| `runtime/mire-bridge.736x352.png` | Opaque 23×11, 32 px/tile room candidate | Visual surface only. Compiled room volumes own collision, deep water, doors, anchors, and lane geometry. |
| `runtime/chain-lane-pattern.standard.32.png` | Transparent 32×32 repeating hostile material | Renderer may tile/rotate it inside server-projected lane geometry. It does not supply width, axes, length, timing, damage, or collision. |
| `runtime/chain-lane-pattern.reduced.32.png` | Reduced-effects parity material | Same outer bounds and secondary chain-notch cue; removes only nonessential glow. |
| `frames/cross_lanes/01-cardinal.standard.png` | Room-scale static review overlay | Review-only composition at authored B5 dimensions. |
| `frames/cross_lanes/02-diagonal.standard.png` | Room-scale static review overlay | Review-only composition at authored B5 dimensions. |
| `frames/cross_lanes/03-cardinal.reduced.png` | Reduced-effects static review overlay | Review-only composition; same lane bounds. |
| `frames/cross_lanes/04-diagonal.reduced.png` | Reduced-effects static review overlay | Review-only composition; same lane bounds. |

All runtime/material/overlay PNGs are RGBA. The room is `736×352`; overlay corners are transparent; the lane material is `32×32`. Existing `48×48` enemy candidates are loaded only into static mocks and are not copied into this pack.

Authoritative binding must provide, per cast: actor/cast identity, target lock, exact axes, 0.9-tile width, collision-clipped line endpoints, telegraph start/end ticks, active end tick, damage disposition, and room state. The client may animate opacity/scroll from those values but must never start, rotate, widen, resolve, or clear a lane based on sprite time.

## Exact generation brief and provenance

No diffusion/image-generation prompt was used. Exact geometry and readability were more important than unconstrained concept generation, so `source/build_review_artifacts.py` is the complete deterministic source. Its exact human art brief was:

```text
Use case: code-native game environment and hostile-telegraph review asset.
Asset type: unregistered B5 Mire Bridge tilemap candidate plus Chain Sentry cross-lane material and static review overlays.
Primary request: render the exact room.bell.bridge_01 shell at 23×11 tiles and 32 pixels per tile. Preserve deep_water north y=[0,3) and south y=[9,11), west/east three-tile doors centered at y=5.5, dry crossing y=[3,9), authored quiet floor channels centered at x=7.5 and x=15.5, six Drowned Pilgrim anchors, and the Chain Sentry anchor at (11.5,5.5). Review cardinal 0°/90° and diagonal 45°/135° 900 ms windups centered on the sentry and clipped to dry room space.
Style/medium: crisp deterministic dark-fantasy pixel art at integer coordinates; wet charcoal flagstone, black-blue deep water, tarnished-brass channel hardware, bone-white Physical Major core, muted red hostile edge, chain-notch secondary marks; nearest-neighbor scaling only.
Constraints: muted environment cannot resemble hostile bullets, loot beams, exits, safe zones, Bargain violet, or healing gold. Active cues retain identical geometry in standard/reduced effects, remain readable in grayscale, and do not use red alone. Existing enemy candidates may appear only in labeled static review mocks. No text in runtime assets. No runtime registration, content mutation, gameplay authority, collision data, content hash, or feature-gate change.
```

Source provenance is project-authored Pillow code plus project-local enemy candidates from `assets/core/enemies/core_bell_encounter_trio/v1` for review composition only. No third-party art, game, brand, or artist reference was used. `SHA256SUMS.txt` records every pack file—including this README, the provenance JSON, builder, and outputs—except the hash manifest itself.

## Deterministic rebuild and verification

From the repository root:

```powershell
python assets/core/dungeons/bell_bridge_combat_review/v1/source/build_review_artifacts.py --root .
python assets/core/dungeons/bell_bridge_combat_review/v1/source/build_review_artifacts.py --root . --verify
```

The first command rebuilds every PNG and `SHA256SUMS.txt`, then verifies hashes, dimensions, RGBA modes, nonempty alpha, transparent overlay corners, and all required review sizes. The second verifies checked-in bytes without rewriting them.

Deterministic byte comparison requires the same Pillow major/minor version and the checked-in Segoe UI fonts used only in labeled review mocks. Runtime/environment/overlay pixels do not depend on fonts.

## Review evidence

- `previews/chain-sentry-cross-lanes.room-scale.png`: 2×2 exact-room contact sheet, cardinal/diagonal and standard/reduced.
- `previews/bridge-combat.50pct.png`: four 50% nearest-neighbor encounter reads. The player marker, enemy bodies, white cores, red edges, and chain notches remain separable.
- `previews/mire-bridge.standard.1280x720.review-mock.png`
- `previews/mire-bridge.reduced.1280x720.review-mock.png`
- `previews/mire-bridge.standard.1920x1080.review-mock.png`
- `previews/mire-bridge.reduced.1920x1080.review-mock.png`

The mocks are explicitly watermarked as static/non-native. They demonstrate spatial hierarchy only and are not evidence that B5 is live.

## Ambiguities and remaining gate

- The room record carries two static vertical `pattern_lane` volumes at x=7.5 and x=15.5, while the Chain Sentry’s authored attack originates at `(11.5,5.5)` and alternates perpendicular cardinal/diagonal axes to room collision. This pack treats the two record volumes as quiet floor channels and the active Sentry attack as a separate authoritative projection. The renderer/content owner must confirm that interpretation before registration.
- The authoritative attack width is 0.9 tiles; review overlays use a 32 px outer treatment at 32 px/tile for inspection. Runtime must derive exact width from simulation geometry, not sample opaque pixels.
- Diagonal clipping is demonstrated against the dry bridge band only. Native collision clipping, door behavior, water hierarchy, spawn-warning ordering, screen-space aliasing, colorblind themes, and optimized motion still require in-engine review.
- No pack asset authorizes B4→B5, B5 activation, enemy spawn, room completion, B5→B6, reward state, or normal-route admission.

No runtime registry, content hash, gameplay code, task status, Current Next Step, README, distribution artifact, gate, commit, or push is changed by this pack.
