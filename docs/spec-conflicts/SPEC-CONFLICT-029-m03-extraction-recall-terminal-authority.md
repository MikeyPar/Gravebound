# SPEC-CONFLICT-029 - M03 extraction and Emergency Recall terminal authority

**Status:** Accepted on 2026-07-16 under the owner's standing instruction to implement the recommended resolution without further approval prompts.

## Authorities reviewed

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-010`, `DTH-011`, `LOOT-002`, `LOOT-033`, `LOOT-050`, `LOOT-060`, `TECH-015`, and `TECH-021`-`023` define the player-visible Recall/extraction rules, deterministic placement, terminal precedence, replay, and crash restoration.
2. `Gravebound_Content_Production_Spec_v1.md`: `CONT-HUB-001`/`002`, the Core microrealm and dungeon/boss contracts, and `CONT-VALID-001` require Recall in every dangerous Core area, committed placement before Hall arrival, exact station ownership, and material conservation.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-08`, and the M03 atomicity, restart, nonduplication, and complete-private-loop gates require production terminal mutations rather than the existing evidence-only extraction seam.
4. Accepted [`SPEC-CONFLICT-009`](SPEC-CONFLICT-009-m03-death-memorial.md) and [`SPEC-CONFLICT-027`](SPEC-CONFLICT-027-m03-crash-restore-completeness.md) require one shared terminal winner, committed-result precedence, and exact danger-entry restoration when no terminal mutation commits.

## Gaps

The three authorities fix the gameplay result but leave several implementation-critical boundaries implicit:

- `DTH-010` names RunBackpack and pouch loss while `LOOT-002` describes Recall as destroying remaining `AtRiskPending` custody; it does not explicitly resolve character-owned PersonalGround rows.
- Extraction placement names logical stacks, while Core persistence stores one durable UID per consumable unit.
- `LOOT-050` requires a 72-hour Overflow expiry, but Roadmap `GB-M05-10` owns the automatic expiry/salvage job after M03.
- `ResolutionHold` blocks play until resolved, but M03 has no Forge/salvage implementation.
- The existing schema-26 Caldus extraction result is explicitly evidence-only and closes the restore root/lineage before any production inventory stabilization.
- Exact terminal identity, retry binding, same-tick reward ordering, and Recall's relationship to life-scoped Oath/Bargain state are not otherwise fixed.

Leaving these seams implicit would permit partial terminal commits, lost or duplicated durable UIDs, permanently stranded characters, or a second extraction writer that disagrees with the shared terminal arbiter.

## Accepted resolution

1. `GB-M03-08` owns one production terminal-inventory contract for successful extraction, explicit Emergency Recall, and automatic LinkLost Recall. It composes the existing five-producer terminal coordinator; lethal death wins a same-tick conflict, and any already committed terminal result is immutable.
2. Production extraction reuses the server-issued Caldus extraction request/receipt identity where applicable but replaces the evidence-only commit with one serializable transaction. No production path may first commit the schema-26 evidence result and mutate inventory afterward.
3. A terminal request binds the authenticated account, selected living character, lineage, restore point, content revision, server-issued terminal identity, canonical payload hash, and exact pre-mutation aggregate versions. Clients never author destinations, destruction lists, placement maps, post versions, or the final Recall terminal identity.
4. Extraction atomically:
   - changes every Equipped/Belt unit from `AtRiskEquipped` to `Safe` without changing its slot;
   - credits the complete run-material pouch to the safe wallet under `LOOT-033`;
   - visits RunBackpack indices ascending and durable UIDs unsigned ascending within one projected stack;
   - merges stackable consumables into Belt, CharacterSafe, then Vault by ascending slot;
   - places every remainder into the lowest empty CharacterSafe, Vault, then 20-slot Overflow location;
   - places any further logical stack into `ResolutionHold(extraction_id,index)`;
   - records the exact placement/material/version map before closing the lineage and enabling Hall transfer.
5. All durable consumable UIDs in one projected ResolutionHold stack share one extraction ID and logical stack index. Template homogeneity, unit ordering, and the authored stack cap remain mandatory.
6. Character-owned PersonalGround is not accepted RunBackpack inventory and is not extracted. It remains instance-local for ordinary expiry/instance cleanup. Extraction may not move it into safe storage or include it in the accepted-item placement map.
7. Explicit and automatic Recall share one loss writer. Equipped and remaining Belt units become `Safe` in place. Every character-owned `AtRiskPending` item, including RunBackpack and PersonalGround custody, plus every run-material-pouch stack is destroyed with retained provenance and immutable ledgers. This prevents terminal Hall state from retaining danger-owned pending custody.
8. Recall does not end the character life. It preserves Oath, active Bargains, Bell debt, progression, safe storage, currency, and other life-scoped state; it closes only the current danger lineage/restore point, stops the combat clock, commits the Recall result, and returns the living character to Hall.
9. One unresolved reward/item mutation and one terminal mutation may not publish competing state. A reward committed before terminal planning is included in the locked snapshot; otherwise extraction/Recall is absent or typed-rejected for that tick and retried from refreshed authority.
10. Append item locations after existing `Consumed=7`: `Overflow=8` and `ResolutionHold=9`. Overflow is account-owned, `Safe`, capacity 20, and stores `expires_at=committed_at+72 hours`. M03 records and displays expiry authority but never silently deletes or salvages an expired row; the automatic expiry/salvage executor remains `GB-M05-10`.
11. M03 supplies the minimum ResolutionHold recovery surface: read the stored hold, move a selected logical stack to legal CharacterSafe/Vault/Overflow capacity, or explicitly destroy it with confirmation and an immutable ledger. Salvage, crafting, gifting, and manual deposits into Overflow/Hold remain disabled.
12. Exact retry returns the stored terminal result and placement/destruction/material map before current-state validation. Reusing the identity with any altered binding or payload returns an audited idempotency conflict. Response loss, reconnect, and process restart reconstruct only from the committed stored result.
13. Character/world, inventory, life-clock, and any genuinely changed account/Vault/material aggregates advance exactly once. The result stores the complete pre/post version vector. Closing the restore point/lineage, committing the terminal receipt/outbox/audit, and making Hall transfer admissible occur in the same transaction or none occur.
14. A committed extraction or Recall supersedes later disconnect/crash recovery. If no terminal transaction commits, the existing `TECH-023` danger-entry restoration contract remains authoritative and no extraction/Recall result is synthesized.

## Scope

This resolution defines the M03 production terminal inventory transaction, minimum Hold recovery, and protocol/replay authority. It does not enable Forge, salvage values, automatic Overflow expiry, gifting, party extraction, public realms, incident compensation, production namespace cutover, or M04+ content. Normal route admission remains closed until `GB-M03-08`, `GB-M03-07`, and the parent integration gates pass.
