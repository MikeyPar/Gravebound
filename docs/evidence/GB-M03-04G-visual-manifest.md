# GB-M03-04G item and Vault lifecycle visual evidence

**Status:** PASS on client implementation commit `a57d0e9`; captured from the optimized Windows release executable built from that exact client tree.

## Three design authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: `LOOP-005`, `LOOT-001`-`005`, `LOOT-010`, `LOOT-020`, `LOOT-050`, `LOOT-060`, `UI-001/003/006/011/030`, and `TECH-023` require a legible, server-owned item life with explicit custody, storage, provenance, mutation, and retry state.
- `Gravebound_Content_Production_Spec_v1.md`: `manifest.items.core_18`, the exact Core starter/reward records, English localization, Hall/Vault identities, and immutable item content revision are the only presentation data used by this disposable surface.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-04`, `GB-M03-02C`, and the M03 evidence gates require native inspection at 1280x720 and 1920x1080 in standard and reduced-effects modes before the item/Vault parent may close.

## Build identity

- Source commit: `a57d0e912c62864a485723860d50f983c3a169f8`.
- Command: `cargo build --locked --release -p client_bevy`.
- Executable: `target/release/client_bevy.exe`.
- Executable SHA-256: `cd1129acb6908ae4d1c00233712d73766f1bfd516a3e28a9dbdff7226918a91b`.
- Executable build time: 2026-07-14 15:38:03 -07:00.
- Core item content revision: `core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb`.
- Scenario: deterministic, read-only `04A`-`04F` lifecycle projection; normal Realm Gate and Vault station admission remain disabled.

## Artifact matrix

| Artifact | Dimensions | Mode | SHA-256 | Inspection |
|---|---:|---|---|---|
| [`GB-M03-04G-lifecycle-standard-1280x720.png`](GB-M03-04G-lifecycle-standard-1280x720.png) | 1280x720 | Standard | `1e9e30b786ba2b0e4c2db15aaff85497cddf4d8895424ce45f7f5dbfb8c634c5` | PASS |
| [`GB-M03-04G-lifecycle-reduced-1280x720.png`](GB-M03-04G-lifecycle-reduced-1280x720.png) | 1280x720 | Reduced effects | `1e9e30b786ba2b0e4c2db15aaff85497cddf4d8895424ce45f7f5dbfb8c634c5` | PASS |
| [`GB-M03-04G-lifecycle-standard-1920x1080.png`](GB-M03-04G-lifecycle-standard-1920x1080.png) | 1920x1080 | Standard | `8b1adabfce6cb1f1f5cbcea9ab86a30cecfac89410b22b0dadfc9722cf0abd2a` | PASS |
| [`GB-M03-04G-lifecycle-reduced-1920x1080.png`](GB-M03-04G-lifecycle-reduced-1920x1080.png) | 1920x1080 | Reduced effects | `8b1adabfce6cb1f1f5cbcea9ab86a30cecfac89410b22b0dadfc9722cf0abd2a` | PASS |

Standard and reduced-effects pixels are intentionally identical. The surface contains no motion, particles, flashes, post-processing, or color-only effect state, so one static `REDUCED-SAFE` render tree serves both CLI modes; the native window title and capture command retain the requested mode identity.

## Inspection record

- All four PNGs were decoded and checked at their exact dimensions. Raw framebuffer coverage is 96.3359% at 1280x720 and 96.9317% at 1920x1080, above the in-client 80% fail-closed publication floor.
- The full nine-item projection is present: four starter units, two reward equipment items, the two-unit reward tonic stack, and the CharacterSafe-to-Vault item.
- Selected character, exact content revision, level/XP, four equipment slots, two Belt slots, eight backpack slots, eight CharacterSafe slots, 160 Vault slots, occupied indices, item UIDs, provenance, security, locations, item versions, aggregate versions `A2/C1/W1/P2/I5`, receipt counts, and ordered ledger transitions are legible.
- The right inspection panel has no clipping or overlap. The left Hall corridor retains 49% of the viewport. The 16 px bottom-notice separation remains clear at both dimensions.
- Standard and reduced-effects captures contain the same information and use non-color text labels. No interaction, station enablement, route admission, extraction, death, successor, Overflow, ResolutionHold, or Core promotion is exposed.

## Verification

- Focused lifecycle-surface tests: 5 passed.
- `cargo clippy --locked -p client_bevy --all-targets -- -D warnings`: PASS.
- `cargo build --locked --release -p client_bevy`: PASS.
- Capture publication rejects blank, clear-only, sparse, unsupported-format, and malformed pixel buffers; retries use a fresh settle window and stop with a nonzero exit after four rejected frames without publishing an artifact.
