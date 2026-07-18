# Sir Caldus B6 combat-presentation review pack v1

> **UNREGISTERED REVIEW CANDIDATE — NOT RUNTIME — NOT A CONTENT-HASH INPUT**

## Scope and non-duplicative gap

This isolated pack covers the missing presentation layer around `arena.boss.caldus_01`: the exact 18×18 Bell Court surface, its circular radius-eight walkable read, and shape-first review masks for Sir Caldus's four canonical attack families plus the ordered Phase 3 gap previews. It does not create another Caldus character strip or another defeat/reward badge.

The pre-work inventory audit found these already-owned candidates and intentionally left them unchanged:

- `assets/core/bosses/sir_caldus/review/v1` through `v6`: Caldus identity, Shield Arc/Charge Lane motion, idle/recovery, Bell Ring pose, Charge Stop Ring pose, and defeat transition.
- `assets/core/ui/caldus_resolution_states/v1`: defeated/reward-unresolved and reward-committed/still-at-risk HUD badges.
- `assets/core/ui/pending_loot_risk/v1`: pending-inventory risk communication.
- `assets/core/dungeons/bell_fixed_route_landmarks/v1`: the stable post-reward B6 exit candidate.

This pack adds only the arena/floor and external telegraph language those character/UI candidates deliberately did not encode.

## Three-authority alignment

1. `Gravebound_Production_GDD_v1_Canonical.md`: `SIM-010`–`014` keep combat state, timing, and collision server-owned; `COM-005`–`006` require origin, shape, color, timing, counterplay, safe-corridor validation, and hostile render priority; `DNG-003`–`006` own boss staging/activation/completion; `ENC-005`/`ENC-010` define the authored arena, learning/pressure/final phases, Shield Arc, Bell Ring, Charge Lane, Stop Ring, and ordered final previews; `ART-001`–`006`, `ART-020`, and `ART-030` require muted environments, reserved high-contrast hostile language, pointed physical cues, thick Major/Severe outlines, reduced-effects parity, provenance, minimum-scale review, and no decorative false positives; `QA-007` requires both certified resolutions and accessibility projectile presets.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-010`/`CONT-013` fix axes, hostile-duration rounding, pattern IDs, and counterplay; `CONT-ROOM-002` and `CONT-ROOM-007` place the exact `18×18` radius-eight arena at B6 with west door, center, staging/group anchors, and four cardinal charge endpoints; `CONT-PATTERN-003` fixes each pattern's count, arc/width/range, warning, damage family, and gap; `CONT-BOSS-001`/`CONT-BOSS-002` fix boss radii, movement constraints, direction-lock time, child-release semantics, ring indices, Phase 3 preview order, and the stable-exit boundary.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` owns Character Select → Hall → micro-realm → six-room dungeon → boss → Hall, one Core major boss, and the M03 room/VFX burn-up. The M03 exit gate still requires a developer-command-free private loop, restart/retry proof, 25 journeys, and human comprehension. This unregistered art review cannot close or enable that gate.

The deterministic tie resolutions in `SPEC-CONFLICT-022` and `SPEC-CONFLICT-024` were also consulted for east-index ring order, the Stop Ring's opposite adjacent-pair selection, and cardinal Charge Lane projection. They refine missing arithmetic only; the three design documents remain the governing product/content/scope authorities.

## Candidate files and runtime contract

| File | Review role | Contract |
|---|---|---|
| `runtime/caldus-bell-court.576x576.png` | Opaque 18×18, 32 px/tile Bell Court candidate | Visual floor only. Compiled arena data owns walkability, collision, door, center, stage/group anchors, and cardinal endpoints. |
| `runtime/telegraph-physical-major.{standard,reduced}.32.png` | Transparent repeating Physical Major/Severe material | Renderer may tile/rotate it inside a server-projected shape; it supplies no width, length, damage, timing, or hit state. |
| `runtime/telegraph-veil-major.{standard,reduced}.32.png` | Transparent repeating Veil Major material | Same boundary; violet family plus bone-white pointed core does not author Bell Ring geometry. |
| `frames/telegraphs/01`–`08` | Arena-scale standard/reduced Shield Arc, Bell Ring, Charge Lane, and Stop Ring masks | Review-only orientation snapshots. All are 576×576 RGBA with transparent corners. |
| `frames/telegraphs/09`–`14` | Ordered A/B/C Phase 3 gap-preview masks | Show only reserved gap previews; they do not insert an ordinary 800 ms Bell Ring warning before child emissions. |

The exact candidate reads are:

- Shield Arc: east-facing review orientation, five pointed paths spread evenly across the authored 60° fan, `650 ms`, Physical Major. Runtime rotates every fan from the server-owned target snapshot and must use its pattern descriptor rather than these pixels for offsets or projectile origins.
- Bell Ring: index `0` east, 18 clockwise indices, ordinary start `0`, adjacent omitted indices `0/1/2`, `800 ms`, Veil Major. The teal double-bracket and absent hostile paths carry the three-shot gap without depending on red/green.
- Charge Lane: east cardinal review orientation, exact authored `1.2`-tile width and `6.5`-tile nominal travel, direction lock at `+700 ms`, movement `+1000..+1550 ms`, Physical Severe. The server selects/locks the cardinal axis and collision-truncated endpoint.
- Charge Stop Ring: east-charge endpoint, 14 clockwise indices, omitted adjacent pair `6/7` under the approved opposite-direction tie rule, child release at charge end, Physical Major. The parent lane warning and opposite-gap marker are its warning; it is never independently scheduled.
- Phase 3: gap starts `0`, `5`, and `10` are previewed A/B/C for `600 ms` each. One/two/three bone notches preserve order in grayscale and reduced effects. Emissions at `2200/3000/3800 ms` consume those reservations without another implicit telegraph.

The review-only boss/player markers are drawn by the builder and are not candidate sprite assets. The existing Caldus sprite packs are neither copied nor transformed here.

## Exact deterministic art brief and provenance

No diffusion/image-generation prompt was used. The image-generation skill was reviewed first, and its own decision rule favors code-native work when exact authored geometry, parity, and reproducibility are more important than unconstrained raster invention. `source/build_review_artifacts.py` is therefore the complete visual source and transformation pipeline.

The exact human brief was:

```text
Use case: code-native boss-arena and hostile-telegraph review asset.
Asset type: unregistered M03 B6 Bell Court floor, transparent cue materials, exact static telegraph masks, accessibility checks, and review mocks.
Primary request: render arena.boss.caldus_01 at 18×18 tiles and 32 pixels per tile with a walkable circle centered (9,9), radius 8, west three-tile entrance centered at (0,9), boss origin (9,9), stage (2.5,9), group anchors (2.5,6)/(2.5,12), and charge endpoints (1,9)/(17,9)/(9,1)/(9,17). Review canonical Shield Arc, Bell Ring, Charge Lane, Charge Stop Ring, and ordered Phase 3 gaps from exact server descriptors.
Style/medium: deterministic dark-fantasy pixel art; wet charcoal flagstone, black iron shell, tarnished brass fixtures, bone-white physical cores, muted red Physical edge, violet Veil edge, and teal absent-gap brackets; nearest-neighbor presentation.
Constraints: muted decoration cannot resemble hostile bullets, loot beams, exits, healing, or safe zones. Standard and reduced variants retain identical core mechanical geometry and non-color cues. Art cannot author target lock, width, index, endpoint, timing, collision, damage, phase, reward, exit, or route state. No text in runtime assets; no new Caldus sprite; no reward/resolution badge; no registry/content/hash/gameplay/gate change.
```

Source provenance is project-authored Pillow code and the three project design documents listed above. No third-party art, game, brand, artist, style, trademark, stock asset, external font asset, or generated image is included. Review-mock labels use the locally installed Segoe UI font but embed only rasterized labels; runtime pixels do not depend on fonts.

## Deterministic rebuild and verification

From the repository root:

```powershell
python assets/core/bosses/sir_caldus/combat_presentation/v1/source/build_review_artifacts.py --root .
python assets/core/bosses/sir_caldus/combat_presentation/v1/source/build_review_artifacts.py --root . --verify
```

The first command rebuilds every PNG and `SHA256SUMS.txt`; the second verifies checked-in bytes without writing. Verification covers every listed SHA-256 entry, required dimensions/modes, nonempty alpha, transparent frame corners, all fourteen telegraph frames, standard/reduced core-geometry parity at alpha ≥128, nonzero optional-effect difference for every standard/reduced pair, pairwise core-mask distinction across all mechanics/preview orders, grayscale light/dark separation in runtime materials, and both required mock resolutions. The retained build's smallest standard/reduced visual difference is `2,345` RGBA pixels; its smallest pairwise core-mask difference is `2,358` pixels. Byte-identical labeled mocks require the same Pillow version and local Segoe UI font files; the runtime arena/material/frame pixels do not use fonts.

## Review evidence

- `previews/caldus-telegraphs.arena-scale.png`: six representative exact-arena scenes at 1×.
- `previews/caldus-combat.{standard,reduced}.50pct.png`: nearest-neighbor minimum-scale sheets.
- `previews/caldus-combat.reduced.50pct.grayscale.png`: reduced-effects grayscale stress sheet.
- `previews/caldus-combat.{standard,reduced}.1280x720.review-mock.png`.
- `previews/caldus-combat.{standard,reduced}.1920x1080.review-mock.png`.

The mocks are explicitly watermarked static, unregistered, and non-native. They demonstrate arena/telegraph hierarchy only; they are not evidence that B6 combat, reward commitment, exit creation, extraction, or the normal route is live.

## Ambiguities and remaining gates

- The authorities define five projectiles over 60° but do not spell out the intermediate rendering offsets. This review uses evenly spaced `-30/-15/0/+15/+30°`, matching the expanded five-over-60 descriptor. Runtime must consume the authoritative projectile descriptor; a content/runtime mismatch overrides this review art and blocks registration.
- The `1.2`-tile Charge Lane cannot be represented as an exact integer pixel width at 32 px/tile (`38.4 px`). The overlay uses a nearest review raster while the source record remains the sole geometric authority. Native rendering should derive the subpixel boundary from simulation/world coordinates.
- Phase 3 teal wedges are presentation recommendations for an absent hostile sector, not permanent safe zones, healing, extraction, or collision. Native testing must confirm that moving A/B/C brackets remain readable without resembling the later stable exit.
- Only one east-facing Charge Lane is shown. North/south/west rotation, diagonal target tie selection, solid truncation, moving targets, 1/4/7-player Shield target counts, overlapping fans, phase cancellation, and full safe-path validation remain simulation/native evidence.
- Audio is not produced here. Each boss family still requires its authored recognizable cue and Major/Severe mix priority.

Before any registry or content-hash proposal, optimized native playback must prove world-space alignment, target/axis rotation, exact 30 Hz timing, collision/hurtbox/origin visibility, no false warnings, standard/reduced/grayscale/colorblind parity at both certified resolutions, response-loss/reconnect stability, and clean transition into authoritative defeat/reward/exit states. Commercial-rights and visual-similarity review also remain required.

No gameplay logic, content record, registry, content hash, route gate, task/audit document, Current Next Step section, root README, distribution artifact, commit, or push is changed by this pack.
