# GB-M03-03G Sir Caldus Stop Ring review v5

## Result

The unregistered v5 pack at [`assets/core/bosses/sir_caldus/review/v5`](../../assets/core/bosses/sir_caldus/review/v5/README.md) passes static candidate review for Sir Caldus's Charge Lane -> Stop Ring follow-through. It remains outside runtime registries and content hashes and carries no simulation authority.

This review applies the GDD's `ENC-010`, `ART-001`-`006`, `ART-020`, and `ART-030`; the fixed B6 and exact Caldus scheduler contracts in Content Spec `CONT-ROOM-002`, `CONT-ROOM-007`, `CONT-BOSS-001`, and `CONT-BOSS-002`; and roadmap `GB-M03-03`'s temporary-asset boundary.

## Checks

| Check | Result |
|---|---|
| Pose continuity | Pass: frame 01 is byte-identical to the accepted v2 Charge Lane terminal brake; frames 02-04 read as planted bell stop, radial-release commitment, and guarded recovery. |
| Normalization | Pass: four 192 x 192 RGBA frames use `[96,192]` bottom-center anchors, transparent corners, one connected silhouette, positive clearance, and a shared 176 px generated-frame height. |
| Minimum-scale readability | Pass: braking, bell stop, extended release hand, shield mass, and recovery remain distinct at nearest-neighbor 96 px. |
| Effects parity | Pass: identical release geometry remains readable in clearly watermarked standard/reduced static mocks at 1280 x 720 and 1920 x 1080. |
| Hash closure | Pass: all four frame hashes, two source hashes, two contact-sheet hashes, and four review-mock hashes recompute exactly from the source manifest. |
| Authority boundary | Pass: the sprite contains no ring, gap, lane, telegraph, damage, collision, reward, or exit pixels/data and cannot drive simulation timing. |

## Principal hashes

| Artifact | SHA-256 |
|---|---|
| Raw chroma source | `4f4dd4d21b9bbc206557bde09cb8a3e26eeb0cebb7770e69abf427d2678c292d` |
| Alpha source | `44ad57ddbd63f50d1dd07d76d74a831187c12b80f87100eac891f6d68af09c92` |
| Locked Charge Lane terminal | `b09b565c36f8e97398ef8d3f7e937cd72080e15291d1d35a64bb333b69130f52` |
| Planted bell stop | `0e8f5756cdf485d74a571d20074c6c3589c32c87b1e0087f9054afab542076c8` |
| Radial release | `e3865a5f7558af07f4f65c6792762e0a574352c1b40d0da7c2a18b86c97e20ce` |
| Guarded recovery | `af31d824374f5185192a16ab39dfc2e333308f4912063326961aaf035d919e38` |
| 192 px sheet | `eeaf3f50eb80c8d2bf83d1c977cbc1cf75d7f3ddc7ff52e615ff8666b0402a84` |
| 96 px sheet | `401fb07bf91e920d7e3d547993bff6aa724ff1d8668001b291b78f7008f7a235` |

The source manifest retains the exact prompt, reference roles, raw/alpha/frame/mock paths, transformations, dimensions, anchors, hashes, mock inputs, authority boundaries, and review gates.

## Remaining limitation

The candidate has not been played in the native client, timed to the authoritative Charge Lane/Stop Ring scheduler, aligned to the authored attack origin and `0.70/0.62` collision/hurtbox radii, reviewed with the real external 14-projectile opposite-gap ring, entered into the asset registry, hashed into content, or captured from an optimized build. Those gates remain mandatory before a runtime proposal.

## Current Next Step

After authoritative Caldus combat is bound to B6, run one combined v2-v5 in-engine motion review at minimum zoom, both effects modes, and both certified resolutions. Verify event-driven timing, attack-origin/hurtbox/collision alignment, anchor continuity, grayscale/colorblind opposite-gap readability, and committed reward -> pending-risk -> stable-exit ordering before any registry or content-hash change.
