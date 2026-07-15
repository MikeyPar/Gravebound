# SPEC-CONFLICT-027 - M03 crash-restore completeness

**Status:** Accepted on 2026-07-14 under the owner's standing instruction to implement the recommended resolution without further approval prompts.

## Authorities reviewed

1. Canonical GDD `TECH-015`, `TECH-019`, `TECH-021`, and `TECH-023`: a server crash is not a death; a prior committed death or extraction wins; otherwise restoration must return the exact danger-entry equipment, Belt, health, and XP, revoke post-entry unsecured gains, roll back consumable use, and record provenance.
2. Content Production Specification `CONT-014` and `CONT-HUB-002`: Bargain milestone/offer state and its 10-Ash fallback are atomic durable mutations, while Realm Gate admission performs CharacterSafe preflight and creates the restore point before danger transfer.
3. Development Roadmap `GB-M03-02`, `GB-M03-06`, `GB-M03-08`, and the M03 exit gates: persistence and terminal outcomes must survive retry/restart without duplicate item, currency, character, death, or terminal results.
4. Accepted [`SPEC-CONFLICT-009`](SPEC-CONFLICT-009-m03-death-memorial.md): lifetime never rolls back, `permadeath_combat_ticks` rolls back to its danger-entry value, and a committed death remains final.

## Gap

The V2 restore root captures progression, Equipped/Belt identities, Oath/Bargains, and life clocks, but that is not sufficient to distinguish every entry-baseline holding from post-entry gain or to replay an exact, auditable crash outcome. In particular, a player may deliberately enter with `RunBackpack` items already at risk; consumed Belt identities need restorable tombstones; danger can create Bargain offer/decision state and a 10-Ash fallback; and no stored result yet binds replay, component completion, or normalized restored/revoked changes.

Applying a blanket "revoke all pending" rule would destroy pre-entry property. Restoring the entry Ash balance wholesale could erase unrelated account mutations. Completing component restores independently could expose a partial recovery. These outcomes conflict with the authorities even though the original V2 graph is internally complete.

## Accepted resolution

1. Publish a forward-only V3 contract in migration `0034`; never edit published migrations `0031`-`0033`. Because Core persistence is wipeable and the normal route remains disabled, migration `0034` must fail closed if any Active or Dormant V2 restore graph exists before changing root meaning.
2. V3 has five mandatory components under one account/character/root authority: progression; inventory entry baseline; Oath/Bargain provenance; life metrics; and Ash wallet version. The root uses `snapshot_contract_version=3`, a complete component mask, canonical component digests, and deferred relational completeness checks.
3. The inventory baseline contains every entry item in Equipment, Belt, and `RunBackpack`, including item UID, item version, exact location/slot, security state, and content/provenance authority. Its maximum is 24 identities: four equipment, twelve durable Belt units, and eight backpack slots. `PersonalGround` is never an entry-baseline location.
4. Belt consumption transitions durable units to a retained consumed state rather than deleting their identity. Crash recovery may restore only a unit named in the V3 baseline. Every restored or revoked item receives a crash-specific immutable ledger event; ordinary movement or destruction reasons cannot masquerade as recovery.
5. Pre-entry `RunBackpack` identities are restored exactly. Post-entry `RunBackpack` and `PersonalGround` items are revoked in deterministic location order. Post-entry run-material stacks are revoked in UTF-8 material-ID order. Rows and provenance remain available for audit; recovery does not silently delete authority.
6. The Oath/Bargain component stores each active Bargain's acquisition offer, source reward/milestone, and content revision. Crash recovery preserves entry state, revokes post-entry offers/selections/refusals/milestone dispositions, and makes retry of a revoked command return its stored revoked terminal result instead of recreating or blocking on stale state.
7. The Ash component snapshots only the wallet version. Recovery compensates accepted danger-bound earns that occurred after entry, including the exact 10-Ash Bargain fallback, in ledger order and exactly once. It does not overwrite the account balance or reverse unrelated account mutations. If safe concurrent account activity makes compensation ambiguous, recovery fails closed for investigation rather than guessing.
8. One server-owned crash request carries a mutation ID and canonical request hash. One immutable result stores the original typed outcome, root binding, post aggregate versions, normalized item/material/Bargain/Ash changes, and its own payload hash. Identical replay returns that result without new writes; changed payload is an audited idempotency conflict.
9. The complete coordinator owns the only commit-capable recovery transaction. Component routines stage changes in the caller-owned transaction and cannot mark themselves independently visible. Deferred constraints require exactly one complete result for a crash-restored root, bind result versions to component versions, and enforce every stored child count.
10. The coordinator locks account, character, restore root, location/lineage, progression, inventory/items, materials, Oath/Bargain, life metrics, and Ash in one documented order. A previously committed death, extraction, or Recall supersedes restoration. Otherwise it preserves lifetime, rolls `permadeath_combat_ticks` back, restores entry health even when current uncommitted health is zero, advances each changed aggregate once, returns the living character to Lantern Halls, closes the lineage as crash-failed, and atomically changes the root from Active to CrashRestored.
11. Validation distinguishes a stored entry snapshot, whose health must be positive, from current danger state, whose health may be zero before terminal commit. Missing/foreign/stale/corrupt components, digests, child counts, locations, content authority, or terminal state abort the whole transaction.
12. The planner supports the existing authoritative pending-item capacity rather than inventing a smaller recovery cap. Hosted proof must cover immediate recovery, response loss, restart replay, health-zero recovery, field swaps, full Belt consumption, pre-entry backpack preservation, post-entry item/material revocation, Bargain and Ash reversal, clock semantics, concurrent requests, terminal precedence, boundary failure injection, corruption, and resource cleanup.

## Scope

This resolution defines Core danger-entry capture and unrecoverable instance-crash restoration only. It does not add player incident compensation, restore a committed death/extraction/Recall, enable normal routes, change ordinary death/Recall loss rules, or authorize later M04+ systems.
