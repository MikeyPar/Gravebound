# GB-M03-03G pending-loot risk review v1

## Authority and scope

This candidate review uses all three design documents:

1. `Gravebound_Production_GDD_v1_Canonical.md` `DTH-010`, `LOOT-002`, `LOOT-010`, `LOOT-033`, and `UI-003`/`005`/`006` define AtRiskPending loss, personal ground drops, the separate run-material pouch, capacity disclosure, and the mandatory loss label.
2. `Gravebound_Content_Production_Spec_v1.md` `CONT-REWARD-001`, `CONT-REWARD-003`-`004`, and `CONT-ROOM-007` define the Core reward context and Bell Brass without authorizing a runtime binding from artwork alone.
3. `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`, `GB-M03-04`, and `GB-M03-08` require the pending inventory and terminal loss/safety transitions to be server-owned and visibly testable.

Commit `370b53e` adds an isolated, unregistered candidate package at `assets/core/ui/pending_loot_risk/v1`. It does not alter reward odds, item generation, ownership, lifetime, capacity, pending inventory, terminal placement, a registry, or a content hash.

## Reviewed candidates

- `pending-slot-risk-frame.64.png`: 64x64 unsecured-slot frame, SHA-256 `b83c8f2f9fa51d20307c49bce56b8917bcebc4b6d48c7c8a68c9a51f7cf6df8d`.
- `personal-ground-drop-marker.64.png`: 64x64 personal unsecured-drop marker, SHA-256 `ae0aff277041bd0a9478d20319a31e629755bbbe6a9dcb75a7eaa95b2129dbb3`.
- `bell-brass-pouch-risk.48.png`: 48x48 at-risk material-pouch token, SHA-256 `f289be7eeda941708cc8c938c7fbf7ea5c308940ff067413b306a36cc3623e27`.

The source manifest retains exact prompts, raw sources, alpha normalization, anchors, all runtime/source/preview hashes, three-authority citations, and the current reduced-development-reward caveat. Main-agent verification parsed the manifest, recomputed all eight recorded hashes, checked dimensions and RGBA mode, and inspected both review sheets at original resolution.

## Review result

The set is accepted as a versioned art candidate only. The slot frame reads as unsecured rather than disabled; the drop marker reads as a personal pickup rather than secured treasure; the pouch is distinct from equipment and safe currency. None substitutes for the mandated textual loss label or authoritative timer/capacity state. Registration remains blocked until the real pending-inventory UI and world-drop projection prove safe/at-risk/resolution-hold distinction, grayscale readability, ownership, timer presentation, and standard/reduced-effects captures at both certified resolutions.

## Current Next Step

Compose the server-owned reward and pending-inventory route first. Then exercise these candidates against full backpack, personal-drop expiry, material-pouch capacity, death/Recall loss, and extraction safety before proposing explicit registry bindings.
