# GB-M03-03G — Sir Caldus Bell Ring review v4

## Result

The unregistered v4 pack at [`assets/core/bosses/sir_caldus/review/v4`](../../assets/core/bosses/sir_caldus/review/v4/README.md) passes static candidate review for a four-frame, pose-only Bell Ring strip. It is not runtime or content evidence and carries no simulation authority.

This review uses the GDD's `ART-004` workflow and `ART-030` anchor/readability criteria, the precise Sir Caldus/Bell Ring definition in `CONT-BOSS-001`, `CONT-BOSS-002`, and `CONT-PATTERN-003`, and the roadmap's `GB-M03-03` temporary-asset boundary.

## Checks

| Check | Result |
|---|---|
| Normalization | Pass: every accepted frame is 192 x 192 RGBA, has shared `(96,192)` bottom-center anchoring, 176 px non-seed content height, 16 px top margin, and positive side clearance. |
| Seed continuity | Pass: frame 01 is byte-identical to the v1 guard seed (`da0c0b90…4ea5`). |
| Alpha integrity | Pass: all corners are transparent and each accepted frame has exactly one 4-connected alpha component at threshold 8. |
| Pose continuity | Pass: guard → chest-bell anticipation → hand-bell release → guarded return preserves the screen-right shield and avoids Shield Arc/Charge Lane movement. |
| Readability | Pass: at 192 px and 96 px, the gold bell cue, visor, shield mass, and release direction remain readable. No attack effect is baked into the raster. |
| Effects parity | Pass: identical frame-03 geometry is shown over standard and reduced-effects static phase-one backdrops at 1280 x 720 and 1920 x 1080. Every mock displays the matching `BELL RING` badge and is visibly watermarked `REVIEW MOCK / UNREGISTERED / NOT RUNTIME`. |
| File hygiene | Pass: source, alpha, frames, sheets, mocks, and manifest are versioned under `review/v4`; no registry, content bundle, task status, route gate, or runtime code changed. |

## Non-authoritative implementation recommendations

These are review findings, not modifications to the encounter contract.

| Topic | Recommendation |
|---|---|
| Timing | In the ordinary phase-one Bell Ring window (`CONT-BOSS-002`: 800 ms + Major bell at 6000 ms), a future approved renderer may map guard → anticipation → release → return across that existing telegraph. Phase-three child emissions expressly have no ordinary 800 ms telegraph; they must not automatically use this full strip. This pack changes no cadence or simulation state. |
| Origin | Obtain ring origin from the authoritative boss transform and authored attack-origin metadata only. The chest/hand bell is a visual cue, never an origin coordinate or an event source. |
| Hurtbox/collision | Preserve `CONT-BOSS-001` Caldus collision/hurtbox radii of `0.70` / `0.62`. The sprite cannot alter collision, hit detection, gap selection, or damage. |
| Ring/gap readability | Keep the hostile ring and its three-adjacent-index gap external to the sprite, visible in both standard and reduced effects, and never occluded by the hand-bell pose. The attack's `18` indices, `+5` gap advance, and range/speed/radius remain content/simulation data. |

## Principal hashes

| Artifact | SHA-256 |
|---|---|
| Raw chroma source | `30f89f3611ac7d50fef2f99654f5ce072267ff90d63d170762da543ca27e0955` |
| Alpha source | `9c49d5fca712c43f28e26185ba445d419d9d2e5db068d8283bbcbdb6c7172cad` |
| Frame 01 (locked guard) | `da0c0b90ca1cc390ba4a59ea3a8991313c95f2a2a339123c36b174da99ad4ea5` |
| Frame 02 (anticipation) | `37706a5ad9b33b4cff28c18c42b7712001b4809af841f2a891369659d76cf840` |
| Frame 03 (release) | `22d5de8960ba4bac03b0cc21518e0a304d2d2edf8162e7fd2c91c40be9143478` |
| Frame 04 (return) | `a8734fa766ffe3ba0e78210dfb2e4ac645db6bcbf0805b217f25ab5abca64861` |
| 192 px sheet | `58b89346a9c972d63716174c90c5059c09dd66392c94318e6558097e7ce401a4` |
| 96 px sheet | `31a4aeac942898c5f687ef89541a5f2c4f2c24ce6bd710bf2c623d71b3e1b85f` |

The source manifest records every source/frame/mock hash, reference roles, prompt, transforms, and static-mock inputs.

## Remaining limitation

The candidate has not been played in the native client; timed to authoritative Bell Ring events; compared with actual authored attack-origin, hurtbox, or collision metadata; entered the asset registry; hashed into content; or captured from an optimized build. Those distinct gates remain mandatory before any runtime proposal or M03 route-admission change.
