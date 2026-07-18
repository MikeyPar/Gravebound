# Sir Caldus B5→B6 staging-transition review pack v1

> **UNREGISTERED REVIEW CANDIDATE — NOT RUNTIME — NOT A CONTENT-HASH INPUT**

## Scope and non-duplicative gap

This isolated pack covers the missing player-visible transition between the cleared B5 Mire Bridge and B6 Sir Caldus encounter: a four-state threshold lock, a text-free five-notch countdown dial, and a text-free introduction banner frame. It includes standard and reduced-effects variants plus static review compositions at `1280×720` and `1920×1080`.

The pre-work audit found and intentionally left these existing assets unchanged:

- `assets/core/dungeons/bell_bridge_combat_review/v1`: exact B5 room surface and Chain Sentry cross-lane presentation.
- `assets/core/bosses/sir_caldus/combat_presentation/v1`: exact B6 Bell Court surface and every canonical combat telegraph family.
- `assets/core/bosses/sir_caldus/review/v1..v6`: Caldus identity, attack poses, idle/recovery, and defeat presentation.
- `assets/core/ui/caldus_resolution_states/v1`: durable defeat/reward state communication.
- `assets/core/dungeons/bell_fixed_route_landmarks/v1`: stable post-reward exit candidate.

This pack does not redraw B5, B6, Sir Caldus, combat telegraphs, reward states, or the dungeon exit. It adds only the boundary/staging/countdown/introduction language those packs did not own.

## Three-authority alignment

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DNG-006` requires a visible five-second boss-boundary countdown, locks living participants when it closes, and prohibits late entry. `ENC-005` limits the identity introduction to four seconds. `ART-001`–`006`, `ART-020`, `ART-030`, `UI-010`, `UI-011`, and `QA-007` require restrained dark-fantasy art, non-hostile decorative language, reduced-effects parity, manifest metadata, anchor/silhouette stability, and review at `1280×720` and `1920×1080` including a sealed-arena warning.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-ROOM-002` fixes `arena.boss.caldus_01` as an `18×18` radius-eight Bell Court with west boundary `(0,9)`, stage `(2.5,9)`, group anchors `(2.5,6)/(2.5,12)`, and boss `(9,9)`. `CONT-ROOM-007` fixes the M03 route as `B0→B1→B2→B3→B4→B5→B6`. `CONT-BOSS-001` pauses the scheduler until all connected entrants load or ten seconds elapse, then runs the visible five-second ready countdown, closes the door, locks `N_locked`, clears the safe-entry radius, and runs Caldus's exact `2500 ms` invulnerable/nonattacking introduction before Phase 1.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` owns the Character Select → Hall → micro-realm → six-room dungeon → boss → Hall route and its one Core major boss. The M03 gate still requires a developer-command-free private loop, restart/retry proof, 25 journeys, native visual evidence, and human comprehension. This asset review cannot close or enable that gate.

## Candidate files and authority boundary

| Candidate | Files | Intended role |
|---|---|---|
| Four-state bell lock | `frames/boss_lock/01..04.{standard,reduced}.png` and `runtime/boss-lock-state-sheet.{standard,reduced}.1024x256.png` | Pose-only threshold states: dormant/open, arming, sealed, and introduction resonance. Runtime state chooses the frame. Pixels do not choose or advance state. |
| Ready-countdown dial | `runtime/boss-ready-countdown-dial.{standard,reduced}.128.png` | Text-free five-notch framing material. Runtime renders authoritative remaining time/progress; the dial has no baked countdown state. |
| Caldus introduction frame | `runtime/caldus-introduction-frame.{standard,reduced}.512x96.png` | Text-free frame behind runtime-authored boss identity copy. It has no phase or duration semantics. |

Every lock frame is `256×256 RGBA`, uses one shared center/scale crop, has transparent corners, and preserves byte-identical alpha between standard and reduced variants. The sheets are convenience atlases only; the content/renderer must bind a stable asset ID rather than infer semantics from filenames or frame order.

The art may not author or infer load completion, the ten-second loading bound, countdown tick, connected/living participant status, `N_locked`, door collision, late-entry rejection, safe-radius clearing, invulnerability, attack ability, introduction duration, scheduler tick, combat phase, reward, exit, route state, or normal-route admission. A stale or out-of-order presentation event must never move the authoritative boss-lock state.

## Exact generation prompt and provenance

Generation used the OpenAI built-in image-generation tool in `stylized-concept` mode. Three existing project review images were supplied only as palette/material/camera references: the standard B6 combat-presentation mock, the standard B5 Mire Bridge mock, and the v1 Caldus sprite review mock. No external image, artist, brand, game, or style reference was supplied.

```text
Use case: stylized-concept
Asset type: 2D dark-fantasy game prop animation review strip for the Gravebound B5-to-B6 Sir Caldus boss threshold
Input images: Image 1 is the established Sir Caldus combat-presentation review mock and visual palette reference; Image 2 is the established Mire Bridge review mock and visual palette reference; Image 3 is the established Sir Caldus sprite review mock and identity reference. Use them only for palette, material, camera, and silhouette continuity. Do not copy their UI, scene layouts, characters, or text.
Primary request: create one single horizontal row of exactly four equal square sprite slots showing the same compact black-iron bell-lock gate mechanism in four clearly distinct presentation states: 1 dormant/open with split iron jaws relaxed, 2 arming with jaws drawing inward and a restrained amber bell glow, 3 fully sealed with interlocked jaws and a bright bone-white central latch, 4 introduction resonance with the sealed mechanism and one restrained concentric amber resonance accent.
Scene/backdrop: perfectly flat solid #FF00FF chroma-key background for local background removal. The background must be one uniform color with no shadows, gradients, texture, reflections, floor plane, or lighting variation.
Subject: a wall-mounted boss-threshold lock built from weathered black iron, tarnished brass bell hardware, a small central bell clapper, and heavy symmetrical locking jaws; original design; no readable letters, numerals, heraldry, faces, skulls, crosses, or religious symbols.
Style/medium: high-fidelity hand-painted 2D dark-fantasy game prop, crisp silhouette, restrained indie-studio production art, 55-degree three-quarter top-down camera consistent across all four slots.
Composition/framing: exactly four equal square slots in one horizontal strip; exactly one complete mechanism per slot; same scale, angle, center, and anchor in every slot; generous clean gutters and padding; no element touches any slot boundary.
Lighting/mood: muted charcoal and black iron, tarnished warm brass, tiny bone-white latch highlight; ominous but readable; standard effects presentation.
Color palette: charcoal #171b1d, iron gray, aged brass #9a6c25, restrained amber #d99a2b, bone white #eadfbd. Do not use #FF00FF in the subject.
Constraints: pose/state only; crisp opaque edges suitable for chroma removal; no cast shadow; no floor; no environment; no character; no weapons; no projectiles; no damage telegraph; no loot beam; no exit portal; no text; no labels; no numbers; no UI; no border; no watermark; no logo. State 4 resonance must remain compact around the mechanism and cannot resemble a hostile attack, safe zone, healing, or portal. The art must not imply countdown duration, participant count, route state, collision, or gameplay authority.
Avoid: photorealism, 3D render, ornate excess, bright red hostile language, violet Veil projectiles, teal exit language, green healing language, blue magic, decorative particles, floating debris, smoke, bloom obscuring the silhouette.
```

Built-in source: `source/sir-caldus-boss-lock.raw-chroma.png`, SHA-256 `9f9489d296fe44e1a64a87284b5f3b152b3c9230c2d0991efa4e63f07f375377`.

Chroma removal used `C:/Users/micha/.codex/skills/.system/imagegen/scripts/remove_chroma_key.py` with `--auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill`. Alpha source: `source/sir-caldus-boss-lock.alpha.png`, SHA-256 `5804500d78f1f7d6a1ed6c0d949c92ed08f1f68578741854a9ad7ae1c841b502`.

`source/build_review_artifacts.py` performs the exact slot crop, normalization, reduced-effects transform, deterministic text-free material drawing, static mock composition, and hash generation. Existing B6 arena and accepted Caldus idle files are read as review dependencies and are neither copied nor modified. Review labels use local Segoe UI; runtime candidate pixels do not depend on a font.

## Deterministic rebuild and validation

From the repository root:

```powershell
python assets/core/bosses/sir_caldus/staging_transition/v1/source/build_review_artifacts.py --root .
python assets/core/bosses/sir_caldus/staging_transition/v1/source/build_review_artifacts.py --root . --verify
```

The retained build verifies 21 hashed PNG files, exact runtime dimensions, nonempty alpha, transparent frame corners, byte-identical standard/reduced frame alpha, non-identical visible color treatment, and all four required static mock dimensions. Standard/reduced changed RGB pixel counts are `29,246`, `31,519`, `30,097`, and `33,227` for dormant through introduction. `SHA256SUMS.txt` is the byte-level evidence manifest.

Review evidence:

- `previews/boss-lock-actual-scale-review.png`: standard/reduced actual-scale state, dial, and frame sheet.
- `previews/caldus-staging-transition.{standard,reduced}.1280x720.review-mock.png`.
- `previews/caldus-staging-transition.{standard,reduced}.1920x1080.review-mock.png`.

These are explicitly labeled static review compositions, not native captures. They prove only candidate hierarchy and readability.

## Reversible choices and integration assumptions

- The authorities require a boss warning/countdown and door closure but do not prescribe a literal lock prop. A compact wall-mounted bell lock is the least-speculative reversible extension of the Bell Sepulcher material language. It can be removed without changing simulation, content, or layout.
- The lock is shown at the west boundary because `arena.boss.caldus_01` has its only entrance at `(0,9)`. Native projection must use the compiled arena transform rather than hard-coded screen placement.
- The five notches communicate countdown category but contain no progress. Native UI must draw authoritative time and accessible state; the review's numeral `3` is mock-only.
- Warm amber is reserved here for objective/boss-state presentation, not a healing/safe ring. The latch, jaw closure, five notches, and label provide non-color cues. Native playtesting must verify it is not mistaken for healing, loot, exit, or hostile geometry.
- State 4's compact resonance accent is decorative and optional. Reduced effects suppresses intensity without changing lock silhouette. It must not expand into the playfield or become an attack warning.
- The generated source contains small cross-slot screw heads as ordinary hardware marks. They do not function as readable symbols at runtime scale; if native review reads them as religious or healing iconography, replace only those fasteners before registration.

Before registration or content-hash promotion, optimized native playback must prove event ordering, inherited authoritative tick behavior, state rollback/reset, stale-event rejection, entry-door alignment, safe-radius readability, exact five-second/`2500 ms` timing, no premature combat, standard/reduced/high-contrast parity, UI scale `80–150%`, and capture at both certified resolutions. Audio remains a separate requirement.

## Current Next Step

Integrate this pack only as a review dependency in the route-bound `CorePrivateCaldusRuntime`, then capture optimized native staging → countdown → closed-door introduction evidence at `1280×720` and `1920×1080` in standard and reduced-effects modes. Do not register or promote it until the runtime proves authoritative event ordering and exact timing.

No gameplay logic, content record, asset registry, content hash, route gate, task/audit document, root README, distribution file, existing asset, commit, or push is changed by this pack.

