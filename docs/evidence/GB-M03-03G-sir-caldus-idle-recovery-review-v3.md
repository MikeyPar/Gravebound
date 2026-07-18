# GB-M03-03G — Sir Caldus idle and recovery review v3

## Result

The unregistered v3 pack at [`assets/core/bosses/sir_caldus/review/v3`](../../assets/core/bosses/sir_caldus/review/v3/README.md) passes static candidate review for its four-frame idle and four-frame post-attack recovery-to-guard strips. It remains presentation-only and cannot be used as runtime/content evidence.

This review applies the GDD's `ART-004` workflow and `ART-030` anchor/readability criteria, the exact M03 Caldus encounter context from `CONT-BOSS-001`/`002`, and the roadmap's temporary-asset boundary.

## Checks

| Check | Result |
|---|---|
| Normalization | Pass: accepted frames are 192 x 192 RGBA with a shared `(96,192)` bottom-center anchor and 176 px content height. |
| Seed continuity | Pass: idle frame 01 and recovery frame 04 are byte-identical to the v1 guard seed (`da0c0b90…4ea5`). |
| Alpha integrity | Pass: corners are transparent and every accepted frame has exactly one connected alpha component at threshold 8. |
| Bounds | Pass: accepted frame bounds preserve a 16 px top margin and positive side clearance. |
| Readability | Pass: 192 px and 96 px sheets retain the visor, shield mass, guard pose, lowered recovery, and return-to-guard sequence. |
| Effects parity | Pass: identical sprite geometry is shown over standard and reduced-effects static backdrops at 1280 x 720 and 1920 x 1080; every mock carries the non-runtime watermark. |
| Rejection record | Pass: raw first attempts with top-edge contact and the semantically incorrect seed-locked recovery opener are retained with reasons. |

## Principal hashes

| Artifact | SHA-256 |
|---|---|
| Idle alpha source | `5c1a8450cc521ddebafb9e6e222ee604bb716e4d7aa4b845ff29e06f5d91015b` |
| Recovery alpha source | `44f0fe6fb7c1131ca3e94de6c00c17fa38e5b61800efb0ffe962243cba5234c3` |
| Idle 192 px sheet | `888288ba118f73bdf204d7174e24a13f1a34fdd1b83f8f00e82a116c98c37001` |
| Idle 96 px sheet | `fa964a5018b599ec88f73087515048138484641fc25a929b12e4479641adab4e` |
| Recovery 192 px sheet | `a03eef25af1c3d40774c1d2675ff7f2df8a3591ec7d6f832faa5dc271c5f1218` |
| Recovery 96 px sheet | `922b5a0382bc93dd1a23498f6fbfef593f8edeed4b3d52c612b6dd313d08b7dd` |

The pack manifest records each accepted source/frame hash, every static mock hash, and all rejected attempts. Review artifacts may be regenerated only from the recorded inputs and prompts.

## Remaining limitation

The strips have not been played back in the client, timed against authoritative attack state, compared against authored collision/hurtbox/origin metadata, entered the asset registry, or captured from an optimized native build. Those steps are required before proposing any runtime/content-hash change.
