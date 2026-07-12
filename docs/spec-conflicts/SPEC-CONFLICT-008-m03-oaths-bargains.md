# SPEC-CONFLICT-008 — M03 Oath and Bargain contract

**Status:** Approved by owner on 2026-07-12

**Raised:** 2026-07-12

**Blocks:** `GB-M03-05`; `GB-M03-05D`–`05F` also require the approved `GB-M03-12` dependency below

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The three authorities agree that Core enables the Grave Arbalist Oaths `oath.arbalist.long_vigil` and `oath.arbalist.nailkeeper`, the Bargains `bargain.cinder_hunger`, `bargain.bell_debt`, and `bargain.lantern_ash`, initial Oath selection at level 10, and a three-choice Bargain offer after the first qualifying Sepulcher Knight clear. They also agree that Oaths and accepted Bargains persist for the character's life, offers are deterministic, and durable mutations are authoritative, transactional, idempotent, and auditable.

The repository currently exposes only nullable Oath identity fields. It has no executable Oath/Bargain content records, mechanics, offer state, shrine contract, or Ash wallet. Five details are incomplete enough that implementation would otherwise invent durable gameplay or violate an exact content mutation.

## Decisions requested

### 1. Executable Core content and interaction contract

**Conflict:** The authorities name the records and most gameplay values, but do not define production schemas, asset/tag derivation, a stable Bargain shrine ID, the shrine interaction contract, or exact initial-Oath warning copy.

**Recommended resolution:**

- Add typed, data-only `OathRecord` and `BargainRecord` schemas with closed effect variants; runtime scripts and undocumented fallback values are prohibited.
- Derive each Oath/Bargain icon as `icon.<record-id>` and validate exact capability/effect tags against the owning mechanics.
- Author the temporary Core shrine as `shrine.bargain.bell_rest_01` in `room.bell.rest_01` at `(7.5, 4.5)`, with `asset_ids = ["sprite.shrine.bargain.bell_rest_01"]` and tags `[shrine, bargain, core_temporary_trigger]`.
- Use normal `SIM-006` interaction priority, range `1.5` tiles, and a `500 ms` authoritative hold. Only one Bargain panel may be open per player.
- Use this exact warning for initial Oath confirmation: `This oath persists for this character’s life. Changing it later costs 40 Ash and requires confirmation in Lantern Halls.`
- Compile these records into the unpromoted `core_dev` target defined by `SPEC-CONFLICT-006`; do not promote `core.1.0.0` early.

Initial Oath state is `Locked(level < 10) -> Eligible(level >= 10, no oath) -> Selected(id)`. Initial selection requires a living, selected, owned character in Lantern Halls, safe inventory state, no unresolved mutation, and an exact content revision/version. The atomic write records the Oath, aggregate version, idempotency result, and outbox event. Same-ID submission returns typed `already_selected` without spending or version change. Later Oath changes remain stage-disabled until the Ash wallet is integrated.

### 2. Deterministic Bell Debt repeat semantics

**Conflict:** `bargain.bell_debt` says every fifth primary shot repeats at half damage after a delay, but does not define what advances the counter, what the repeat snapshots, or how cancellation and lifecycle resets work.

**Recommended resolution:** Count accepted authoritative primary emissions, including legal misses but excluding generated repeats. The fifth accepted emission schedules exactly one non-recursive repeat at `+300 ms`. It snapshots the fifth shot's aim and resolved projectile/damage behavior, while its origin is the character's live position when the repeat emits. Apply `50%` after ordinary direct-damage modifier resolution, with half-up rounding at the normal damage boundary. The repeat spends no cooldown, resource, or counter step.

Cancel the repeat if the character is dead, transferred, or the corresponding primary is no longer legal. Keep the counter in the authoritative live-character aggregate so reconnect to the same instance and room changes preserve it. Reset it on Bargain acquisition/purge, death, retirement, or committed safe transfer. Journal it in checkpoints; do not write PostgreSQL for every shot.

### 3. Deterministic Nailkeeper trap semantics

**Conflict:** `oath.arbalist.nailkeeper` defines trap values but not impact precedence, actor re-entry, damage snapshots, overflow eviction, or wall impacts.

**Recommended resolution:** A normal Grave Mark enemy hit still deals `1.8W` and applies the four-second mark, then creates a trap at the exact enemy/solid contact point. Snapshot `W` at trap creation. A trap has radius `1.25`, arms after `400 ms`, lives `5 s`, and requires actors already inside at arming to exit and re-enter. The first legal enemy entry deals one `0.9W` direct hit, applies Frostbind for `1.5 s`, and consumes the trap. A wall impact creates the trap but deals no Grave Mark hit or mark.

Allow at most two live traps. Creating a third removes the oldest by `(created_tick, trap_entity_id)`. Retain Grave Mark's base `5.0 s` cooldown, `12/s` projectile speed, `11`-tile base range, `1.8W` hit, and four-second mark. Nailkeeper multiplies the primary interval by `1.08`, subject only to the lower interval cap.

### 4. Lantern Ash Belt and healing composition

**Conflict:** `bargain.lantern_ash` reduces the Belt to one active slot and multiplies potion healing, but does not identify the active slot or composition order.

**Recommended resolution:** Belt index `0` remains active. Index `1` remains stored, visible, and locked; Bargain acquisition or purge never moves or destroys its contents. If index `0` is empty, the character has no active consumable until an authorized inventory mutation fills it.

For Red Tonic, apply item named-effect multipliers to the base `30%` maximum-health heal first, then apply Lantern Ash `×1.40` at `CONT-AFFIX-005` step 8, and round once at the final heal boundary. Retain the authored `0.4 s` delivery and `2 s` cooldown.

### 5. Real Ash wallet before Bargain fallback rewards

**Conflict:** `CONT-014` and `BRG-002` require an immediate, exactly-once `10 Ash` grant when a qualifying trigger has zero legal Bargain candidates or no new/unfilled slot. The roadmap otherwise places the Ash wallet in `GB-M03-12`, after `GB-M03-05`. Implementing the fallback first would require an unauthorized IOU, temporary currency, or silent rule omission.

**Recommended resolution:** Pull the minimal `GB-M03-12` Ash wallet and idempotent earn/spend ledger forward as a hard dependency of `GB-M03-05D` and parent `GB-M03-05` closure. `GB-M03-05A`–`05C` may proceed first; then close the wallet package before `05D`–`05F`.

For zero candidates, atomically transition the offer to `Unavailable`, preserve the earned unfilled slot, append one `+10 Ash` ledger event keyed by `(offer_id, "bargain-unavailable-v1")`, update balance/version, and store the result/outbox event. A retry returns the stored result without another credit. When there is no new/unfilled slot, create no offer and grant once under `(source_reward_event_id, "bargain-no-slot-v1")`. Concurrent or replayed calls produce exactly one ledger entry. Deferred-credit rows, temporary currency, and test-only exceptions are prohibited.

## Required aggregate and state contract

Use one versioned, single-writer character-life aggregate. Reliable interaction messages open Oath/Bargain views; reliable mutation messages perform commands. Persist the selected Oath, active Bargains in acquisition order, earned slots, immutable ordered offers and terminal states, source reward identity, milestone idempotency, aggregate version, mutation result, and outbox events. The qualifying Sepulcher Knight clear atomically records the clear/reward, earns the slot, and creates the offer—or commits none of them.

Bargain slot state is `Unearned -> EarnedUnfilled`. Offer state is `Absent -> Open(offer_id, ordered[3]) -> Selected(id) | Refused | Unavailable`. The active list is acquisition order with a maximum of three. Acceptance persists before the UI closes. Refusal has no resource or character-life penalty and replay returns the stored terminal result. Emergency one/two-candidate offers fill remaining cells with exact `UNAVAILABLE`; Core's ordinary enabled set has exactly three legal candidates.

At minimum, expose typed rejections for level, location, ownership, death, disabled content, unresolved mutation, state-version mismatch, idempotency conflict, payload-hash mismatch, illegal Oath, closed offer, candidate not offered, already selected, insufficient currency, and unavailable service. Approved localization must own player-facing copy. Emit telemetry seams for `oath_selected`, `bargain_offered`, `bargain_selected`, and `bargain_declined`.

## Recommended delivery split

- `GB-M03-05A`: typed content schemas, exact records, assets, localization, and unpromoted compiler closure.
- `GB-M03-05B`: initial Oath state, persistence, reliable protocol, confirmation UI, and idempotent selection.
- `GB-M03-05C`: Long Vigil and Nailkeeper mechanics with deterministic tests.
- Pull forward and close the minimal `GB-M03-12` Ash wallet and ledger.
- `GB-M03-05D`: qualifying milestone, deterministic offer, shrine interaction, acceptance/refusal/fallback transactions, and UI.
- `GB-M03-05E`: Cinder Hunger, Bell Debt, and Lantern Ash mechanics.
- `GB-M03-05F`: PostgreSQL/QUIC lifecycle, telemetry, adversarial, visual, performance, and route-gate closure evidence.

Later Oath swapping and Bargain purging remain fail-closed until their full Ash-backed mutations are implemented. The normal route remains gated through the owning `GB-M03-04`, `05`, `06`, and `08` packages.

## Approval requested

Approve all five recommended resolutions, or provide amendments. Approval allows `GB-M03-05A`–`05C` to proceed and makes the Ash-wallet dependency explicit; Bargain triggering and the shrine remain stage-disabled until that dependency passes.

## Decision

The owner approved all five recommended resolutions on 2026-07-12 without amendment.
