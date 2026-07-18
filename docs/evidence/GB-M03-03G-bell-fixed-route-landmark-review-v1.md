# GB-M03-03G Bell fixed-route landmark review v1

## Authority and scope

This candidate review uses all three design documents:

1. `Gravebound_Production_GDD_v1_Canonical.md` `DNG-003`-`006`, `BRG-001`-`002`, `DTH-011`, and `ART-001`/`002`/`004`/`020`/`030` define the safe B0 arrival, optional B4 Bargain, post-reward B6 exit, and readability hierarchy.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-ROOM-002`, `CONT-ROOM-007`, and `CONT-BOSS-001` define the fixed B0-B6 layout, rest-room shrine anchor, and committed-reward gate for the stable exit.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`, `GB-M03-05`, and `GB-M03-08` require these route pivots to remain server-authoritative and visibly reviewable without artwork becoming state authority.

Commit `ea6445c` adds an isolated, unregistered package at `assets/core/dungeons/bell_fixed_route_landmarks/v1`. It changes no asset registry, content/runtime hash, collision, interaction range, room activation, Bargain offer, reward receipt, extraction result, or terminal authority.

## Reviewed candidates

- `bell-vestibule-ward.192.png`: B0 quiet arrival ward, SHA-256 `99708039e032b26e0143deddc2bca580b4868b7a37190eed9ffbad1f5c39263a`.
- `bell-bargain-shrine.192.png`: B4 optional three-talisman Bargain anchor, SHA-256 `c394735455ec95c7eed0449d67661786b9f7afd06d585a5c1ef23ab497c19b81`.
- `bell-post-reward-exit.192.png`: B6 stable return gate candidate, SHA-256 `a557fd0780af62ae67f5b9a67c37fb969bc3a5cf73ee5129c84249854df7b531`.

The manifest retains the exact prompts, raw sources, alpha processing, normalization, anchors, authority citations, and source/runtime/preview hashes. Main-agent review parsed the manifest, recomputed all eight recorded hashes, verified three `192x192` 32-bit RGBA runtime canvases, and inspected the original-resolution and nearest-neighbor 2x sheets.

## Review result

The set is accepted as a versioned art candidate only. B0 reads as a warm, quiet arrival landmark without resembling an exit; B4 preserves a restrained violet optional-risk language and exactly three talismans; B6 reads as an open ash-white return path distinct from the closed Bell entrance. The existing B6 content asset ID is recorded only as a possible later binding and remains unmodified.

Registration remains blocked until the fixed B0-B6 runtime exists and optimized native review proves authoritative visibility timing, collision/anchor alignment, prompt priority, grayscale/accessibility distinction, protected playfield, and standard/reduced-effects presentation at both certified resolutions. In particular, B6 must never render before the committed boss reward and stable-exit authority.

## Current Next Step

Compose fixed-room combat, Bargain, rewards, pending inventory, and terminal outcomes on the completed exclusive 30 Hz driver. Exercise B0/B4/B6 against those authoritative timelines before proposing any registry or content-hash change.
