# SPEC-CONFLICT-030 - M03 ResolutionHold recovery authority

**Status:** Accepted on 2026-07-16 under the owner's standing instruction to implement the recommended resolution without further approval prompts.

## Authorities reviewed

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-011`, `LOOT-002`, `LOOT-050`, `LOOT-060`, and `TECH-021`-`023` require successful extraction without accepted-item loss, an inaccessible no-expiry `ResolutionHold`, blocked unrelated play until resolution, deterministic storage, immutable retry, and crash-safe commits.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-HUB-001`/`002` place the Core Vault and Overflow stations in Lantern Halls, require lowest-index inventory behavior, and define `storage_resolution_required` as the authored block reason.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`/`08` and the M03 restart, idempotency, nonduplication, and complete-private-loop gates require the minimum Hold resolver now; `GB-M05-03`/`10` retain salvage and automatic Overflow expiry.
4. Accepted [`SPEC-CONFLICT-029`](SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md) fixes Hold as extraction-owned logical stacks and limits M03 to bounded read, server-planned move, or confirmed explicit destruction.

## Gaps

The authorities do not state:

- whether a later Hold-to-Overflow move starts a new 72-hour deadline or retains the original extraction deadline;
- whether a selected logical stack may be partially moved;
- whether the client may choose a destination or merge target;
- which aggregate versions advance when the final held stack clears the character block;
- how exact replay identifies a Hold mutation independently of the immutable extraction result.

Restarting the Overflow deadline would let repeated recovery actions extend extraction-only storage indefinitely. Client-authored destinations would create a second inventory planner. Partial movement would make one logical-stack identity unstable across retries.

## Accepted resolution

1. Protocol evolution is append-only. Protocol `1.16` preserves message-kind bytes `1`-`20`, appends bounded ResolutionHold query kind `21` and mutation kind `22`, and negotiates `core_resolution_hold_v1`. The ordinary Core route remains unadvertised and disabled until `GB-M03-08` closes.
2. Query authority is the authenticated account's selected, living Hall character. It groups current item custody by `(terminal_extraction_id, stack_index)`, returns at most eight logical stacks and 64 durable UIDs, validates homogeneous template/kind/content and authored stack capacity, orders UIDs unsigned ascending, and publishes a server-derived stack digest.
3. One mutation binds a nonzero mutation ID, selected character, extraction ID, stack index, action, expected account/character/world/inventory versions, Core item/content revision, stack digest, issue time, and canonical payload hash. Exact stored replay is checked before current aggregate validation; altered reuse is an audited idempotency conflict.
4. `Move` resolves the complete selected logical stack atomically. The client supplies no destination, slot, split, or merge target. The server uses current locked capacity and deterministic order:
   - merge a consumable stack only when one existing CharacterSafe stack, then one Vault stack, can accept the complete held quantity;
   - otherwise use the lowest empty CharacterSafe slot, then lowest empty Vault slot, then lowest empty eligible Overflow slot;
   - equipment always uses the lowest empty destination in that same order;
   - Belt is never a Hold-recovery destination;
   - if no single destination accepts the complete stack, write nothing and return `storage_full`.
5. Hold-to-Overflow retains the original extraction provenance and `extracted_at`; its deadline remains exactly `extracted_at + 72 hours`. A deadline at or before the mutation's authoritative commit time is not an eligible Overflow destination. M03 never resets, extends, deletes, or salvages an expired Overflow row.
6. `DestroyConfirmed` destroys the complete selected logical stack, retains original extraction provenance, records reason `resolution_hold_destroyed`, and writes one immutable per-item ledger plus one normalized mutation result. It grants no Ash, material, salvage value, replacement, or other benefit. The protocol command is accepted only from the final explicit destructive-confirmation action and binds the queried stack digest.
7. Inventory version advances exactly once for every accepted move or destruction. Account version advances only when the destination is account-owned Vault or Overflow. Character and world versions advance exactly once only when the transaction removes the final held stack and changes character security from `StorageResolutionRequired` to normal Hall authority. Other versions remain unchanged.
8. Final-clear detection scans all current Hold rows for the character, not only the selected extraction ID. Until none remain, danger entry and every unrelated inventory, crafting, gifting, or terminal mutation keep returning `storage_resolution_required`.
9. The original extraction result, placement map, receipt, audit, and outbox remain immutable historical authority. Hold recovery adds a separate serializable writer, replay receipt, normalized transition projection, conflict audit, and immutable item ledgers; it never edits the original extraction graph.
10. The Hall resolver auto-opens after a stored extraction with `storage_resolution_required=true` and after Hall login/reconnect while the character block is present. M03 shows item identity, quantity, destination preview, move, and permanent-destroy confirmation only. It does not invent salvage values or expose manual deposits into Overflow/Hold.

## Scope

This resolution covers only the M03 ResolutionHold query, whole-stack move, explicit destruction, durable replay, Hall gating, and presentation boundary. It does not enable salvage, Forge, crafting, gifting, Overflow expiry execution, manual Overflow/Hold deposit, party inventory, production namespace cutover, or M04+ content.
