# SPEC-CONFLICT-031 - M03 successor recovery authority

**Status:** Accepted on 2026-07-16 under the owner's standing instruction to implement recommended resolutions without further approval prompts.

## Authorities reviewed

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-020`, `DTH-021`, `UI-007`-`009`, `TECH-021`-`023`, and `QA-101` require a legal successor from the last class/appearance preset, at most two confirmations to return to control, durable retry, no playable dead identity, no commercial interruption, and measured recovery.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-CATALOG-003` fixes the exact class starter kit, new item identities, Worn/zero-salvage equipment, two distinct Grant Red Tonic units in Belt slot 1, and empty Armor/Charm/Belt slot 2.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-07` and the M03 exit gate require successor preset, starter kit, return to control, no duplicate character under retry, median death-to-control below 15 seconds, and successor combat within two minutes after at least 70% of eligible deaths.
4. Accepted [`SPEC-CONFLICT-004`](SPEC-CONFLICT-004-m03-core-identity.md), [`SPEC-CONFLICT-007`](SPEC-CONFLICT-007-m03-progression-items.md), and [`SPEC-CONFLICT-009`](SPEC-CONFLICT-009-m03-death-memorial.md) fix the Core name/appearance boundary, require reuse of the `04D` starter initializer, and make committed death the successor source. Accepted [`SPEC-CONFLICT-029`](SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md) keeps normal route admission disabled until successor recovery and parent integration close.

## Gaps

The authorities do not state:

- where every death stores its universal successor preset when Echo creation is optional;
- how the successor reclaims a dead character's roster slot without colliding with ordinary Create;
- whether successor creation and starter initialization may commit separately;
- which party derives successor, mutation, item, and selection identity;
- how retries distinguish exact replay from changed payload or a second successor attempt;
- which two confirmations satisfy the death-summary and return-to-control contracts;
- whether ordinary creation, Memorial, or a different account may consume the death.

Deriving a preset from an optional Echo would make non-Echo deaths unrecoverable. Committing identity before starter items would expose a legal character without its required kit. A process-local ID counter would duplicate after restart. Allowing ordinary Create to take the dead roster ordinal would strand the successor path.

## Accepted resolution

1. Every normal player-visible death atomically persists one immutable universal successor preset with the death graph. It stores class ID plus a typed appearance snapshot. Core stores the non-entitlement base-silhouette token fixed by `SPEC-CONFLICT-004`, never a production appearance entitlement or optional Echo row. The preset is copied from committed death-time authority and is not reconstructed from mutable content.
2. The latest unconsumed normal death reserves its exact `former_roster_ordinal` for that account. Ordinary Create rejects with typed `successor_resolution_required` while the reservation exists; selecting another already-living character remains legal. Only successful successor creation consumes the reservation.
3. Protocol evolution is append-only. Protocol `1.17` preserves message-kind bytes `1`-`22`, appends bounded successor kind `23`, and negotiates `core_successor_v1`. The request carries only server-issued death identity, nonzero client mutation identity, selected protocol/content revision, and canonical request hash. The client authors no character ID, roster ordinal, name, class, appearance, starter item, destination, provenance, or aggregate version.
4. One serializable account/death transaction locks the account, death/preset/reservation, former roster slot, selection, and aggregate heads in documented order. It creates the living character, progression, world/life/Oath state, inventory root, exact starter items, successor receipt/result, audit, outbox, reservation consumption, and selected-character pointer together or writes none.
5. Successor identity is server-derived from the immutable account/death/mutation binding using a domain-separated durable algorithm; it is never process-local or client-authored. Starter UIDs are produced only by the existing `server_app::starter_items::StarterItemPlan`/initializer revision for the newly derived character ID, so all four item identities are new and deterministic for exact replay.
6. The Core successor copies the stored Grave Arbalist class and locked base silhouette, derives the existing localized `Hero {former_roster_ordinal}` label, starts at level 1/XP 0/full 120 health, has no Oath or Bargains, and is safe in Character Select. It receives exactly Pine Crossbow and Cracked Mark Lens as Worn/Starter equipment plus two distinct Grant Red Tonic units in Belt slot 1; Armor, Charm, backpack, CharacterSafe, and Belt slot 2 are empty. Account, Vault, currency, Memorial, Echo, and dead-character history remain unchanged.
7. `(namespace, account_id, death_id)` permits exactly one successor. `(namespace, account_id, mutation_id)` stores the canonical payload/result. Exact retry returns the stored result before current-state validation; changed reuse conflicts and audits; concurrent writers serialize to the same result; a different death may produce a later independent successor.
8. Only the authenticated account may consume its latest committed normal terminal-summary death. Practice, retirement, server-fault restoration, foreign, nonfinal, already-consumed, corrupt, content-mismatched, or superseded death authority fails closed. Memorial stays permanently read-only and never exposes Create Successor.
9. The death-summary `Create successor` action is confirmation one. A stored success enters Character Select with the successor already selected; primary `Play` is confirmation two and returns to Hall/control. No rename, store, paid product, promotion, or optional detour appears before control. Exact response replay does not duplicate transition animation or selection.
10. The normal route continues to omit `core_successor_v1` until the disposable PostgreSQL/real-QUIC/restart/adverse/native package passes. `GB-M03-07` closure permits parent `GB-M03-03` integration; it does not independently enable Core promotion, public realms, parties, M04+ content, or commerce.

## Scope

This resolution covers only the immutable successor preset, reserved roster ordinal, append-only successor command/result, one atomic successor/starter/selection transaction, exact replay, two-confirmation native handoff, and recovery metrics. It does not add editable names, cosmetic entitlements, retirement, Requiem encounters, telemetry export, support tools, Steam runtime integration, production namespace cutover, or M04+ content.
