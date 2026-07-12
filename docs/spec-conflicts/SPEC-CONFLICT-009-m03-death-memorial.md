# SPEC-CONFLICT-009 — M03 permanent-death and memorial contract

**Status:** Owner-approved on 2026-07-12

**Raised:** 2026-07-12

**Blocks:** `GB-M03-06`, `GB-M03-02D`, and qualifying player-visible death integration with `GB-M03-13`

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The three authorities agree that authoritative health reaching zero in a permadeath-enabled instance must reject later actions and atomically commit the death, deterministic item destruction, combat trace, memorial, and any eligible Fallen Hero Echo before showing the death summary. The Content Production Specification exposes the Memorial Wall in Core, and the roadmap requires restart durability and all-or-nothing death/Echo persistence.

The current repository contains only the local First Playable death transaction and network death finality. Durable item locations, memorial/death rows, and the Echo projector intentionally remain deferred. Five rules are contradictory or incomplete enough that implementation would otherwise invent permanent loss or legacy behavior.

## Decisions requested

### 1. Memorial threshold

**Conflict:** `DTH-001` unconditionally creates a memorial for every normal committed permadeath. `ECH-001` separately says all level-5-or-higher deaths create memorials even without an Echo, which could be read as prohibiting memorials below level 5. The Memorial Wall lists memorials without a level filter.

**Recommended resolution:** `DTH-001` governs: every committed normal permadeath at every level creates exactly one death memorial. Treat the level-5 sentence as a nonexclusive guarantee, not a lower-level prohibition. `DTH-002` practice/tutorial deaths create none. Voluntary retirement retains its separate `retired` memorial path. Key a death memorial uniquely by `death_id`, so retry cannot duplicate it.

### 2. Death identity and immutable durable records

**Conflict:** `DTH-020`, `TECH-020`, `TECH-022`, and the Memorial Wall require an exact durable snapshot, but no executable death, summary, or memorial schema exists and no durable death-ID derivation is defined.

**Recommended resolution:** Generate an RFC 4122 UUIDv7 `death_id` once when the authoritative lethal transition is accepted, journal it, and use `(character_id, "permadeath-v1")` as the final-death idempotency key. Enforce one final death per character; identical retry returns the stored outcome.

Add three typed immutable records:

- `DeathEvent`: death/account/character IDs, pre/post aggregate versions, commit time/tick, instance/region/room/content revision, typed cause, lethal killer/pattern/attack IDs, raw/final damage, damage type, pre-hit health, fixed-point source position, statuses, Recall/network snapshot, combat-trace reference, and ordered destruction-ledger references.
- `DeathSummarySnapshot`: one-to-one with the death; approved Core hero label, class, level, Oath, Bargains, lifetime, final deed, ordered last-five damage events, and exact `Lost`/`Preserved`/`Created` projections including Echo status.
- `MemorialRecord`: keyed and foreign-keyed by `death_id`, with account, `death_at`, summary revision, and Core default presentation snapshot. It stores no Early Access entitlement and copies no item roll.

Persist stable IDs, numbers, and content revision—not localized strings. A typed `server_fault` result is not a final player death.

### 3. Deterministic destruction scope and ledger order

**Conflict:** `DTH-001` names equipment, Belt, RunBackpack, and run-material pouch, while the broader loss rule destroys every `AtRiskEquipped` and `AtRiskPending` item. Approved `SPEC-CONFLICT-007` adds durable per-unit Belt identity and `PersonalGround`, neither of which appears in the older death list.

**Recommended resolution:** A death destroys every item owned by the character in `AtRiskEquipped` or `AtRiskPending`, explicitly including Equipped, both Belt slots' durable units, RunBackpack, PersonalGround, and run-material pouch stacks. It never destroys CharacterSafe or Vault state. Transition each item to `Destroyed(permadeath)`; do not delete its row.

Use this canonical destruction-ledger order:

1. Equipment: Weapon, Relic, Armor, Charm.
2. Belt: index, then unsigned unit UID.
3. RunBackpack: index.
4. PersonalGround: `(instance_id, pickup_id, item_uid)`.
5. Run pouch: material ID in UTF-8 byte order.

Key each entry by `(death_id, ordinal)`. An illegal or unknown at-risk location fails the whole transaction.

### 4. Lifetime, Echo combat time, and final deed

**Conflict:** `DTH-020` requires lifetime and final deed, while `ECH-001` requires ten minutes in permadeath-enabled combat plus boss/event deeds. No authority defines the clocks, crash-restore interaction, or deterministic deed selection.

**Recommended resolution:**

- `lifetime_ticks` counts authoritative 30 Hz ticks while a living character is controllable in Hall or danger, including the vulnerable three-second LinkLost window, but excluding character select, loading, and offline time. Snapshot `lifetime_ms = floor(ticks × 1000 / 30)`.
- `permadeath_combat_ticks` counts living ticks after committed danger entry through death, extraction, or Recall, including staging and vulnerable LinkLost. Crash restoration rolls it back to the entry value. Echo eligibility requires exactly `>= 18,000` ticks.
- Persist idempotent reward-qualified completion IDs. Eligibility requires at least one dungeon-boss ID or two distinct major-realm-event IDs.
- Add `deed.core.sepulcher_knight_defeated` and `deed.core.sir_caldus_defeated`. Select the latest by `(achieved_tick, deed_id)`. If none exists, use `deed.none` with exact en-US copy `No final deed recorded.`

### 5. `GB-M03-06`/`GB-M03-13` Echo atomicity

**Conflict:** The roadmap separates death (`06`) from Echo record/promotion (`13`), but the M03 exit gate requires a qualifying death to write death, destruction, memorial, and Dormant Echo atomically or write none. `CONT-ECHO-009` additionally promotes the oldest Dormant Echo when the account has no Available Echo.

**Recommended resolution:** `GB-M03-06` owns one SQL death transaction and a mandatory in-transaction Echo-projector seam. `GB-M03-13` supplies real eligibility, snapshot construction, and promotion before qualifying player-visible deaths are enabled. Never create an Echo asynchronously after death commit.

For an eligible death, insert the Echo as `Dormant`, then run oldest-first `(created_at, echo_id)` promotion under the same account lock. If no Echo is Available, the new row may commit as `Available`; immutable transition audit records both `Dormant` and `Dormant -> Available`. Publish outbox/telemetry only after the transaction commits.

## Recommended delivery split

- `GB-M03-06A` / `GB-M03-02D`: typed records, PostgreSQL migration/repository boundary, and reliable protocol.
- `GB-M03-06B`: lifetime/combat clocks, deeds, cause selection, and ten-second trace capture.
- `GB-M03-06C`: deterministic destruction planner, ledger, single-writer transaction, idempotency, crash, and race tests.
- `GB-M03-06D`: durable-ack-gated `DTH-020` UI and Memorial Wall.
- `GB-M03-06E`: `GB-M03-13` Echo projector, PostgreSQL/QUIC/restart/adversarial/visual/performance closure.

`GB-M03-03B`, `04D`/`04E`, and `05` supply required location, item identity/ledger, and Oath/Bargain snapshots. `07` consumes the committed successor path; `08` shares death/Recall/extraction arbitration; `09`/`10` consume telemetry/support seams. The route remains gated under approved `SPEC-CONFLICT-006`.

## Approval record

The owner approved all five recommended resolutions without amendment on 2026-07-12. The death/memorial schemas and transaction boundary may proceed exactly as recorded above without weakening permanent-loss or Echo atomicity.
