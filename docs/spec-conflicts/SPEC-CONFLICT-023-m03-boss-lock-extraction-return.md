# SPEC-CONFLICT-023 — Boss lock, committed extraction, and Hall return resolution

**Status:** Owner-approved by standing authorization on 2026-07-13

**Blocks:** `GB-M03-03E` participant staging/reset, personal reward closure, stable exit creation, committed extraction handoff, and authoritative Hall return

## Authorities consulted

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `DTH-010`–`011`, `DNG-006`, `ENC-005`, `ENC-010`, `WRLD-006`, `SOC-010`, and `TECH-004`.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-010`, `CONT-ROOM-002`, `CONT-ROOM-005`–`007`, `CONT-BOSS-001`–`002`, Core reward overrides, and strict validation requirements.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-08`, the M03 restart/idempotency gates, and approved `SPEC-CONFLICT-006` ownership and route gate.

## Conflict

The authorities define loading timeout, ready countdown, immutable living lock, reset, committed personal reward, stable exit, DTH-011 extraction, and safe Hall transfer. They do not assign stable IDs, define connected/loaded roster sampling, specify reward-terminal exit ordering, or separate 03E's committed-transfer seam from 08's inventory/Overflow/ResolutionHold transaction. They also leave replay and disconnect ordering at the exit implicit.

## Approved resolution

1. Boss staging begins when the first living entrant reaches the staging volume. The staged roster contains connected arena entrants and remains mutable until door closure. Start the five-second countdown when every current staged entrant reports loaded, or exactly 300 ticks after staging begins. Entrants arriving before countdown completion join the staged roster; entrants arriving after door closure are rejected. At closure, living entrants ordered by immutable party slot then entity ID become `N_locked=1..8`; M03 admission remains capacity one while validators exercise all eight counts. If zero living entrants remain at closure, consume no attempt ordinal, leave the door open, and return to BossWarning.
2. Introduction begins only after door closure, lock commit, hostile clear, and entrance-radius clear. Caldus is invulnerable and nonattacking for exactly 75 ticks. Death, Recall, departure, and disconnect never rescale the locked health. Recall remains legal throughout staging, combat, breaks, reward, and exit states.
3. If living locked participants reach zero while a living party member remains outside, start an exact 150-tick reset. Reentry by a still-living locked participant cancels the timer. At expiry clear hostiles, projectiles, unsecured drops, pending reward/exit state, and arena mutation; restore Caldus, reopen BossWarning, and capture a fresh roster/scale. With no living party member anywhere, the instance remains fail closed for the owning retirement/crash package and creates no reward or exit.
4. Introduce stable content ID `portal.exit.dungeon.bell_sepulcher`, asset ID `sprite.portal.exit.dungeon.bell_sepulcher`, and tags `[portal,dungeon_exit,successful_extraction,requires_committed_boss_reward]` at `(2.5,9)`. This is distinct from the existing microrealm ingress `portal.dungeon.bell_sepulcher`. The compiler rejects any other Core boss-exit binding, position, semantic tags, asset, or localization closure.
5. Derive encounter, personal reward request, exit instance, extraction request, and extraction receipt IDs from the immutable run lineage with separate BLAKE3 domain prefixes and length-delimited canonical fields. Retries reuse their IDs. Duplicate and out-of-order messages are idempotent; a payload mismatch for an existing ID fails closed.
6. Eligible reward owners are locked participants who satisfy `SOC-010`: present for at least 50% of active duration unless the fight lasts under 20 seconds, contribution at least 0.5% of locked scaled boss health, no inactivity over 20 seconds, alive and present at the authoritative defeat tick, not Recalled, and valid session/anti-cheat state. Each receives the exact `reward.boss_caldus` Core bundle and progression award through the existing durable idempotent services. The stable exit appears only after every eligible owner's reward request reaches a durable terminal committed result. An empty eligible set creates neither reward nor exit.
7. 03E owns a typed `ExtractionRequested` → `ExtractionCommitted` receipt seam and may transfer only after consuming the matching committed receipt. `GB-M03-08` owns the DTH-011 transaction that converts pending items, routes excess to Overflow/ResolutionHold, and commits character/item state. A test authority may issue the receipt for 03E evidence, but production code must not fabricate or bypass it.
8. The committed receipt permanently supersedes crash restore for that run and binds the character to `SafeArrival::HallDefault`. Only then may the world-flow coordinator durably commit Hall location and emit the transfer payload. Disconnect or retry after extraction commit cannot undo extraction, duplicate reward, reuse the exit, or return to danger. A Hall-transfer failure retries with the same receipt and arrival.
9. Normal Character Select `Play`, production Realm Gate admission, Bell portal traversal, seeded layout selection, and the complete player route remain disabled until their roadmap owners pass. The 03E test/showcase seam is explicit and cannot advertise route readiness.

This decision supplies missing identity, ordering, and package boundaries. It does not implement `GB-M03-08` inventory extraction, Overflow, ResolutionHold, loss rules, production route admission, or reconnect UX.

## Approval record

The owner authorized all future recommended GB-M03 resolutions without an approval pause on 2026-07-13. This resolution was adopted under that standing authorization.
