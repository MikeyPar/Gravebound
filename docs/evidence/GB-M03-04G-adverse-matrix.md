# GB-M03-04G item and Vault lifecycle evidence matrix

**Status:** PASS on implementation commit `c5691ac` and hosted CI [`29373978792`](https://github.com/MikeyPar/Gravebound/actions/runs/29373978792).

## Three design authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: `LOOP-005`, `LOOT-001`-`005`, `LOOT-010`, `LOOT-020`, `LOOT-050`, `LOOT-060`, `TECH-020`, `TECH-021`, and `TECH-023` require deterministic server-owned item custody, immediate durable mutation, exact retry, and no accepted-item loss or duplication.
- `Gravebound_Content_Production_Spec_v1.md`: `manifest.items.core_18`, the Core progression/reward profiles, exact Hall/Vault identities, lowest-index placement, strict content hashes, and `CONT-VALID-001/002` are the executable data and fixture authority.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-02`, `GB-M03-04`, and the M03 exit gates require PostgreSQL restart preservation, mutation nonduplication, 25 scripted journeys, median login-to-control below 30 seconds, and evidence before parent closure.

The matrix is layered deliberately. The disposable real-QUIC journey composes production services and PostgreSQL repositories; focused protocol, repository, and coordinator tests retain exact malformed-frame, serializable-transaction, and rollback boundaries without duplicating gameplay authority in the harness.

## Matrix

| Adverse case | Closest authoritative evidence | Required invariant | State |
|---|---|---|---|
| Exact replay and altered payload | `postgres_safe_inventory::real_quic_safe_inventory_replays_across_a_new_endpoint`; `persistence::postgres_foundation::safe_inventory_transfer_is_atomic_replay_safe_and_restart_durable` | exact mutation ID and payload return the stored placement/result; changed payload conflicts without mutation | PASS |
| Lost committed response | `postgres_safe_inventory::dropped_quic_response_retries_the_stored_lifecycle_result` | abandoned response plus exact retry creates one receipt, placement, ledger transition, and aggregate advance | PASS |
| Reconnect and process/pool restart | `postgres_safe_inventory::real_quic_safe_inventory_replays_across_a_new_endpoint` | full typed signature, canonical bytes, and digest are identical before reconnect, after reconnect, and after a new pool/endpoint | PASS |
| Duplicate authenticated sessions | `postgres_safe_inventory::duplicate_quic_sessions_converge_on_one_inventory_commit` | both sessions observe one accepted result; exactly one is fresh and one replayed | PASS |
| Final-slot capacity race | `postgres_safe_inventory::concurrent_claims_for_final_vault_slot_have_one_winner` | one serialized winner, one stale loser, one ledger row, one receipt, and no over-capacity state | PASS |
| Concurrent danger entry/manual transfer | `postgres_world_flow::concurrent_manual_transfer_and_entry_have_one_serial_storage_move` | one legal account-first serial order; no double move or partial danger root | PASS |
| Mid-write provider failure | `postgres_safe_inventory::injected_ledger_failure_rolls_back_item_versions_and_receipt`; `postgres_world_flow::concurrent_entry_has_one_lineage_and_provider_failure_rolls_back_every_row` | item, ledger, receipt, placement, aggregate, route, and lineage writes commit together or not at all | PASS |
| PostgreSQL unavailable before mutation | `postgres_safe_inventory::database_outage_returns_a_state_free_quic_rejection` | wire result is state-free `ServiceUnavailable`; reopened canonical signature is unchanged | PASS |
| Structurally corrupt stored receipt | `postgres_safe_inventory::corrupt_receipt_fails_closed_over_quic_without_a_second_mutation` | count/placement contradiction is rejected by persistence and canonical signature; wire exposes no stored state | PASS |
| Semantically corrupt replay hash | `safe_inventory::replay_validation_recomputes_hash_and_command_shape`; real-QUIC corruption stage in `postgres_safe_inventory` | replay hash/source/destination are recomputed and bound to the command; altered durable result fails closed | PASS |
| Stale/foreign authority | stale-version and foreign-character frames in `postgres_safe_inventory::real_quic_safe_inventory_replays_across_a_new_endpoint`; repository binding tests | authenticated account, selected living Hall character, source custody, and aggregate versions bind every mutation | PASS |
| Malformed or oversized input | `protocol::safe_inventory` validation tests; `protocol::codec::safe_inventory_framing_rejects_malformed_and_oversized_bytes` | invalid identity/hash/version/index/result shape, truncated bytes, and frames beyond 64 KiB fail before authority or mutation | PASS |
| Invalid item content revision | exact safe-inventory repository revision validation; `postgres_safe_inventory::foreign_item_content_revision_fails_closed_before_storage_mutation` | any item outside the compiled Core revision fails closed before planning or writing | PASS |
| Normal-route admission | Realm Gate assertion on every `run_quic_transfers` endpoint; normal runtime remains `CoreSafeInventoryAuthority::Disabled` | no normal route feature advertisement, station admission, world allocation, location change, extraction, death, or Core promotion | PASS |
| Resource cleanup | QUIC endpoints explicitly close and wait idle; `postgres_safe_inventory::completes_25_item_lifecycle_journeys_within_login_budget` | no retained connection task, endpoint, or idle database transaction after each journey | PASS |

## Performance and visual gates

- Run 25 serial deterministic Caldus/item journeys without developer commands. Record login-to-control and mutation round-trip median, p95, and maximum; fail if median login-to-control is not below 30 seconds.
- Capture the optimized native lifecycle inspection surface at 1280×720 and 1920×1080 in standard and reduced-effects modes. Record build identity, Core content revision, dimensions, mode, scenario, and SHA-256 for every artifact.
- Inspect the original-resolution files for clipping, overlap, color-only meaning, illegible item/version/provenance data, or playfield-corridor obstruction.

Hosted CI completed 25 journeys with login-to-control `10.083 ms` median, `14.989 ms` p95, and `18.611 ms` maximum. Mutation round-trip was `7.471 ms` median, `10.127 ms` p95, and `14.474 ms` maximum. The fixture verified zero idle PostgreSQL transactions after cleanup. The optimized four-capture matrix, build identity, hashes, and original-resolution inspection are recorded in [`GB-M03-04G-visual-manifest.md`](GB-M03-04G-visual-manifest.md).

## Scope boundary

This evidence does not enable normal Character Select `Play`, Realm Gate or Vault station interaction, extraction/Recall conversion, Overflow, ResolutionHold, death destruction, successor recovery, salvage/crafting, parties, later rarity/affixes, production namespace writes, or Core promotion. Those remain fail closed until their named packages pass.
