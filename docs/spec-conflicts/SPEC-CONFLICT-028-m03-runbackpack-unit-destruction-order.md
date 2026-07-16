# SPEC-CONFLICT-028 - M03 stacked RunBackpack destruction order

**Status:** Accepted on 2026-07-16 under the owner's standing instruction to implement the recommended resolution without further approval prompts.

## Authorities reviewed

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `LOOT-002`, `LOOT-033`, and `LOOT-060` require every equipped and pending durable item unit to be destroyed exactly once while retaining item identity and ledger history.
2. `Gravebound_Content_Production_Spec_v1.md`: Core enables stackable Belt and pending consumable custody and requires deterministic, retry-safe terminal mutations.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-02`, `GB-M03-06`, and the M03 atomicity/nonduplication gates require one stable destruction graph across retry and restart.
4. Accepted [`SPEC-CONFLICT-009`](SPEC-CONFLICT-009-m03-death-memorial.md): primary destruction order is Equipment, Belt `(slot, unit UID)`, RunBackpack slot, PersonalGround tuple, then UTF-8 material ID.

## Gap

Consumable stacks use one durable `item_uid` row per unit. Multiple pending units may therefore share one RunBackpack index. Ordering only by RunBackpack index does not define a total order for those rows and made the typed validator reject a legal stacked slot even though the database closure already used `item_uid` as its final item tie-break.

## Accepted resolution

1. Preserve the approved primary order exactly.
2. Within one RunBackpack index, order durable units by unsigned `item_uid`, matching the existing Belt rule and the PostgreSQL deferred graph validator.
3. Derive each permadeath item-ledger event ID from `(death_id, mutation_id, item_uid)` under a dedicated domain-separated hash. Callers may supply raw custody only; they may not author destruction ordinals, post-item versions, or ledger IDs.
4. The server-owned pure planner sorts arbitrary input into this total order, advances every item/material version exactly once, destroys the complete run-material quantity, and rejects duplicate identities, invalid bounds, or version overflow.
5. The PostgreSQL transaction independently reloads and locks current custody and rejects any omitted, extra, stale, wrongly secured, or wrongly located source.

## Scope

This clarification changes no player-facing loss rule, capacity, stack size, item identity, or extraction behavior. It only supplies the missing deterministic tie-break and ledger identity required to implement the already approved `GB-M03-06C` contract.
