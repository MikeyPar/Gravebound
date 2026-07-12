# SPEC-CONFLICT-007 — M03 progression, item, and Vault contract

**Status:** Awaiting owner decision

**Raised:** 2026-07-12

**Blocks:** Affected `GB-M03-04` subpackages and `GB-M03-02C`; does not block `GB-M03-03A` or `03B`

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The three authorities agree that `GB-M03-04` owns Core levels 1–10, exact XP eligibility, the 18-item Core manifest, four equipment slots, two Belt slots, eight pending slots, eight CharacterSafe slots, and the initial 160-slot Vault. Under approved `SPEC-CONFLICT-005`, the same reviewed package supplies `GB-M03-02C` item/Vault rows, ledger entries, transactions, and restart evidence.

They also agree on exact Grave Arbalist level scaling, reward profiles, source levels, item formulas, starter equipment, item-security states, deterministic retry, and server authority. Seven details remain contradictory or incomplete enough that implementation would otherwise invent durable behavior.

## Decisions requested

### 1. Core level-10 XP overflow

**Conflict:** M03 exposes levels 1–10 while the GDD's full cap is 20. Level 10 begins at total XP `2,700`, but the authorities do not say whether additional Core XP is discarded, stored, or banked toward a later stage.

**Recommended resolution:** Core total XP saturates at `2,700`. It stores no hidden level-11 progress and banks no overflow. The Core namespace remains wipe-only, so later stage promotion does not migrate this capped progression.

### 2. Current health when maximum health changes

**Conflict:** Leveling and equipment can increase or decrease resolved maximum health, but no authority defines the matching current-health mutation. Percentage preservation would heal on increases; an unchecked decrease could kill during a stat rebuild.

**Recommended resolution:** Preserve absolute current health when maximum health increases. On a decrease, clamp current health to the new maximum with a living floor of `1`. A stat rebuild neither heals nor kills; only an authoritative damage/death transition may kill.

### 3. Missing boss and event XP profile IDs

**Conflict:** The Content Production Specification supplies exact XP values and requires every XP source to resolve a profile, but names profile IDs only for ordinary/Elite/miniboss sources. Bosses and events therefore cannot satisfy strict reference closure without invented IDs.

**Recommended resolution:** Authorize these exact stable IDs:

- `xp.event_minor`
- `xp.event_major`
- `xp.boss_caldus`
- `xp.boss_veyr`
- `xp.boss_confessor`
- `xp.world_warden`

M03 enables `xp.boss_caldus` immediately. Later IDs remain disabled until their owning stage. Values continue to come only from the authored XP tables; the IDs add no fallback values.

### 4. Belt and personal-ground item locations

**Conflict:** GDD `LOOT-002` describes its item-location list as exhaustive but omits both the two required Belt slots and the 60-second personal ground-drop state used when pending inventory cannot accept a reward.

**Recommended resolution:** Add these exact typed locations:

- `Belt(index)` with legal indices `0..1`.
- `PersonalGround(instance_id, pickup_id, expires_at_tick)`.

Personal-ground items remain `AtRiskPending`. Expiry atomically transitions the item to `Destroyed(ground_expired)` and appends the matching ledger entry. No untyped or ownerless ground state is legal.

### 5. Durable Red Tonic stack identity

**Conflict:** Durable items require unique identity and provenance, while Red Tonics merge into stack-six Belt slots. The authorities do not define which UID survives a merge, how multi-unit rewards derive identities, or which unit is consumed.

**Recommended resolution:** Persist one UID and provenance record per consumable unit; Belt/backpack stacks are projections grouping those units. A quantity reward derives deterministic unit ordinals from its one stored reward result without additional RNG. Display order and consumption use ascending unsigned UID, and use removes the lowest UID. No merge rewrites provenance or destroys identity implicitly.

### 6. Item content revision before Core promotion

**Conflict:** Every item instance requires an immutable `content_version`, but approved `SPEC-CONFLICT-004` forbids assigning `core.1.0.0` before complete Core promotion. Persisting an invented release version or mutable `core-dev` label would make replay ambiguous.

**Recommended resolution:** Wipeable development item instances use immutable revisions named `core-dev.blake3.<full-manifest-hash>`. Each revision identifies exact bytes, cannot migrate, and is accepted only in the wipeable Core development namespace. Formal Core promotion performs the already-required wipe before creating any instance labeled `core.1.0.0`.

### 7. Full-backpack field-swap contradiction

**Conflict:** One rule says an equipped replacement returns to the pending source index; another says the swap rejects whenever the backpack is full. A `RunBackpack` source necessarily vacates one index during the same atomic mutation, while a `PersonalGround` source does not.

**Recommended resolution:**

- `RunBackpack -> Equipped`: the replaced equipped item moves into the exact source index vacated by the incoming item, so a full backpack does not reject.
- `PersonalGround -> Equipped`: the replaced item requires the lowest genuinely empty `RunBackpack` index; otherwise the mutation rejects without moving either item.

The authoritative preview names the exact replacement destination before confirmation.

## Ownership clarification

`GB-M03-04` owns the first atomic starter initializer: Worn Pine Crossbow, Worn Cracked Mark Lens, empty Armor/Charm, two Grant-provenance Red Tonics in Belt slot 1, and empty Belt slot 2. `GB-M03-07` must reuse this initializer for successors rather than create a second starter model. Existing wipeable `GB-M03-01` characters receive one idempotent transactional backfill when `GB-M03-04D` lands.

## Recommended delivery split

- `GB-M03-04A`: exact progression, XP eligibility/first-clear marker, durable XP, level stats, and HUD.
- `GB-M03-04B`: production item/reward schemas, immutable development revision, and first end-to-end behaviors.
- `GB-M03-04C`: exact Core 18-item content, assets, localization, behavior data, and reward closure.
- `GB-M03-04D`: starter backfill, reward UID finalization, durable consumable units, pending/ground placement, and item ledger.
- `GB-M03-04E`: four-slot field equipment, deterministic swaps, resolved behavior, and comparison/confirmation UI.
- `GB-M03-04F`: CharacterSafe inventory, Vault 160, Hall transfers, danger-entry preflight hook, and PostgreSQL durability.
- `GB-M03-04G`: real-QUIC/PostgreSQL lifecycle, adversarial, visual, performance, and closure evidence for both `GB-M03-04` and `GB-M03-02C`.

The normal route remains gated under `SPEC-CONFLICT-006` until oath, death, extraction, and Recall semantics are complete.

## Approval requested

Approve all seven recommended resolutions, or provide amendments. Once approved, `GB-M03-04A` and `04B` can proceed in parallel with the approved early world-flow subpackages without weakening the later route gate.
