# Gravebound: The Ashen Veil
## Content Production Specification — Exact Early Access Data Baseline

| Field | Value |
|---|---|
| Document ID | `GB-CONTENT-001` |
| Version | `1.0.0` |
| Status | Canonical implementation contract |
| Date | 2026-07-10 |
| Parent design | `Gravebound_Production_GDD_v1_Canonical.md` |
| Delivery order | `Gravebound_Development_Roadmap_v1.md` |
| Target | Windows Steam Early Access; Rust stable; Bevy 0.19 |

---

## Contents

0. [Authority and implementation rules](#0-authority-and-implementation-rules)
1. [Shared content conventions](#1-shared-content-conventions)
2. [First Playable content pack](#2-first-playable-content-pack)
3. [Item math and affixes](#3-item-math-and-affixes)
4. [Exact 90-template item catalog](#4-exact-90-template-item-catalog)
5. [Reward and rarity tables](#5-reward-and-rarity-tables)
6. [Mire of Bells macro map](#6-mire-of-bells-macro-map)
7. [Rooms and dungeon layouts](#7-rooms-and-dungeon-layouts)
8. [Enemy and encounter records](#8-enemy-and-encounter-records)
9. [Fallen Hero Echo modules](#9-fallen-hero-echo-modules)
10. [Exact dungeon-modifier execution](#10-exact-dungeon-modifier-execution)
11. [Hub, tutorial, practice, cosmetics, and localization](#11-hub-tutorial-practice-cosmetics-and-localization)
12. [Validation and promotion manifest](#12-validation-and-promotion-manifest)

---

## 0. Authority and implementation rules

### CONT-000 — Purpose

This file supplies the exact content IDs, values, coordinates, pools, and fallback behavior intentionally omitted from the system-level GDD. An implementation agent MUST translate these records into versioned game data without renaming IDs or inventing substitutes. A balance change changes data and fixtures; it does not silently change the GDD's system rules.

When documents conflict, the GDD controls system behavior and safety; this specification controls exact content records; the roadmap controls delivery order. Record the conflict instead of guessing.

### CONT-001 — Complete-record rule

Every runtime definition MUST contain:

```text
id
schema_version
content_version
enabled
release_stage
localization_name_key
localization_description_key
asset_ids[]
tags[]
numeric_payload
source_document_feature_id
```

Missing required data is a build error. The runtime MUST NOT supply an undocumented gameplay default. A fallback is legal only when this file names it explicitly.

### CONT-003 — Tabular expansion contract

Markdown rows are canonical authoring shorthand, not incomplete runtime records. `tools_content` MUST materialize and validate a complete record using these exact rules:

- `schema_version=1`, `content_version` equals the promoted bundle, and `enabled=true` iff the record belongs to that bundle's manifest.
- `release_stage` is the earliest manifest containing the ID: `fp`, `core`, `slice`, or `alpha`; `playtest/ea` reuse the Alpha gameplay manifest unless this file explicitly adds presentation/operations data.
- `localization_name_key = id + ".name"`; `localization_description_key = id + ".description"`; canonical `en-US` values and stage manifests follow CONT-LOC-001.
- Item `asset_ids=["icon."+id]`; enemy/miniboss/boss `asset_ids=["sprite."+id,"portrait."+id]`; room/arena `asset_ids=["tilemap."+id]`; ability/pattern/modifier `asset_ids=["vfx."+id,"audio."+id]`. Packaging requires each derived ID.
- `source_document_feature_id` is the nearest enclosing `CONT-*` heading ID.
- `numeric_payload` is the normalized fixed-point form of every numeric cell/sentence in that record. Omitted optional schema fields are absent, never defaulted.
- `tags` are the base/capability tags in CONT-ITEM-004 or the explicit role/family/type/status tags in the owning row. A packer may not infer gameplay tags from display names.

The generated JSON is checked into the implementation repository so diffs show every expanded field. A fixture expands this Markdown contract and compares a canonical JSON hash; handwritten JSON divergence fails CI.

### CONT-002 — Release stages

```text
fp       First Playable / M01
core     Core Prototype / M03
slice    Networked Vertical Slice / M04
alpha    Closed Alpha / M05
playtest Public Steam Playtest / M06
ea       Early Access / M08
```

Content may ship earlier than its stage but cannot be required by an earlier gate. Bundle IDs are exact: First Playable=`fp.1.0.0`, Core=`core.1.0.0`, Slice=`slice.1.0.0`, Alpha=`alpha.1.0.0`, Public Playtest=`playtest.1.0.0`, and Early Access=`ea.1.0.0`. M01 stable shared IDs also exist in later bundles. Increment the patch component for a payload-only correction within one stage, the minor component for a backward-compatible manifest addition, and the major component for schema or deterministic-behavior incompatibility; never reuse a promoted bundle ID for different bytes. A stage promotion creates the next named stage bundle even when its gameplay manifest reuses Alpha records.

Global stage allowlists:

| Domain | Core / M03 | Slice / M04 | Alpha / M05 |
|---|---|---|---|
| Classes | `class.grave_arbalist` | add `class.ashen_vanguard` | add `class.veil_witch` |
| Oaths | `oath.arbalist.long_vigil`, `oath.arbalist.nailkeeper` | add `oath.vanguard.bell_retort`, `oath.vanguard.ashen_bastion` | add `oath.witch.orchard_rot`, `oath.witch.saltglass` |
| Bargains | `bargain.cinder_hunger`, `bargain.bell_debt`, `bargain.lantern_ash` | add `bargain.glass_pulse`, `bargain.hollow_aim`, `bargain.funeral_pace` | add `bargain.grave_weight`, `bargain.salt_oath`, `bargain.rooted_bloom`, `bargain.saints_debt`, `bargain.veil_mirror`, `bargain.ashen_pack` |
| Modifiers | none | `modifier.candleless`, `modifier.glass_floor`, `modifier.oathfire` | add `modifier.fevered_veil`, `modifier.saints_debt`, `modifier.restless_dead` |
| Dungeons | Bell Sepulcher | add Root Chapel | add Drowned Reliquary |
| Echo gameplay | record/promotion only | full personal Requiem modules | same plus final balance |

Item, encounter, room, landmark, and event allowlists are specified in their owning sections. The stage compiler validates only enabled records and combinations, while the Alpha/EA compiler enforces full counts.

---

## 1. Shared content conventions

### CONT-010 — Units and axes

- Distance is in simulation tiles; one tile is `1.0` unit.
- World and room origin is the northwest corner. `+x` is east and `+y` is south.
- Integer tile `(x,y)` occupies `[x,x+1) × [y,y+1)`; its center is `(x+0.5,y+0.5)`.
- Angles are degrees clockwise from east. An aimed offset of `+10°` rotates toward screen-south from an east-facing vector.
- Arena-local coordinates use the arena center as `(0,0)` unless the record explicitly declares another origin; First Playable, dungeon rooms, and Requiem explicitly use northwest origins.
- Durations are integer milliseconds. Ordinary durations compile to 30 Hz ticks round-to-nearest; hostile telegraphs and authored fairness minima use ceiling-to-tick and can never simulate shorter than authored.
- Speeds are tiles/second. Acceleration is tiles/second squared.
- Health and raw damage are nonnegative integers after `round_half_up`.
- Stored percentages use integer basis points: `10000 = 100%`.

### CONT-011 — Deterministic rounding

```text
round_half_up(x): floor(x + 0.5) for nonnegative x
ceil_to_unit(x, unit): ceil(x / unit) * unit
clamp(x, low, high): min(high, max(low, x))
```

Evaluate multipliers in the order written by the GDD, carry at least signed 64-bit fixed-point precision at `1/10000`, and round health/damage only once immediately before the damage pipeline or durable write. Never use platform-native unordered-map iteration to choose content.

### CONT-012 — Deterministic ordering and random draws

- Sort content candidates by stable UTF-8 ID before applying weights.
- Weighted selection uses positive integer weights and a half-open draw `[0,total_weight)`.
- One reward request consumes draws in this order: roll presence, roll rarity, usability category, slot, template, affix IDs in display order, affix values in display order, cosmetic check.
- Skipped optional branches consume no draw.
- A retry reuses the original idempotency key and stored result; it never draws again.
- Encounter layout draws use a separate stream from reward draws.
- Record the RNG algorithm and exact crate version in `ADR-001`; changing either requires a content-version migration and new golden fixtures.

### CONT-013 — Common combat defaults

Unless a record overrides a value:

```text
enemy_contact_damage: 0
enemy_collision_radius: 0.35
enemy_hurtbox_radius: 0.30
projectile_radius: 0.12
projectile_lifetime_ms: derived from range / speed, rounded up to a tick
target_reacquire_ms: 250
out_of_combat_reset_ms: 5000
spawn_invulnerability_ms: 1000
enemy_drop_on_reset: false
phase_projectile_policy: cancel_on_phase_change
```

Every hostile attack declares an `echo_memory_family`, damage band, telegraph, sound cue, projectile disposition, and threat cost. The content compiler rejects omitted fields.

Common normalized pattern fields are `acceleration_tiles_per_second_squared=0`, `pierces_players=false`, `statuses=[]`, and `cancel_on_phase_change=true` unless an exact record below overrides them. `telegraph_id=pattern_id+".telegraph"` and `audio_cue_id=pattern_id+".warning"`; Major/Severe patterns additionally reference `audio_cue_id+".major"`. For a projectile group, each projectile is a distinct hit group; a lane/ground/push activation is one hit group and can damage a player at most once per authored activation. `maximum_active_instances` is the exact maximum found by the owning fixed scheduler's 30 Hz golden trace and is persisted in expanded JSON; a trace exceeding that persisted integer fails CI.

For enemy-table shorthand, canonical `pattern_id = "pattern." + source_content_id + "." + attack_snake_id`; each attack sentence's bold/code label or final noun phrase supplies the written `attack_snake_id` and is checked against a committed manifest. `telegraph_id=pattern_id+".telegraph"`, `audio_cue_id=pattern_id+".warning"`. Ordinary projectiles use `consume_on_player_or_solid`; piercing records use `consume_on_solid_or_after_pierce_limit`; lanes/pools use `expire_at_authored_end`; charges use `one_contact_hit_per_cast`. Counterplay is `strafe` for aimed/fan, `follow_gap` for rings, `leave_telegraph` for lanes/charges, `leave_circle` for ground zones, `attack_rear` for Mudbound, and `interrupt_channel` for Cantor. Every attack still materializes these fields in checked-in JSON.

### CONT-014 — Deterministic Bargain offers

On a qualifying milestone, atomically grant the next `earned_bargain_slot` (maximum3) and create one immutable offer keyed by the milestone reward idempotency key. Core additionally treats the first `miniboss.sepulcher_knight` clear in `layout.core_private_life_01` at level5+ as slot-one milestone; this binding is disabled in Slice/Alpha.

Build the candidate set from enabled stage Bargains that are not already active, satisfy every BRG-004 tag requirement/exclusion, and leave the character within resolved-stat caps. Sort by UTF-8 ID. For each candidate compute:

```text
version = UTF8(content_version)
candidate = UTF8(candidate_id)
score = BLAKE3(ASCII("bargain-offer-v1\0")
               || source_reward_event_id_rfc4122_16_raw_bytes
               || character_id_rfc4122_16_raw_bytes
               || little_endian_u32(len(version)) || version
               || little_endian_u32(len(candidate)) || candidate)
```

Sort by unsigned lexicographic 32-byte `score`, then candidate ID, and persist the first three IDs in that order. Selection/refusal retries return the same offer; one transaction changes `Open -> Selected(candidate_id)` or `Open -> Refused`. A selected ID becomes life-persistent before the UI closes. Build validation enumerates all reachable stage/class/oath/active-Bargain states and requires at least three legal candidates for every ordinary production offer.

If an emergency live disable leaves only one/two legal persisted candidates, show those plus disabled `UNAVAILABLE` cells; refusal remains legal and the earned slot remains unfilled until selection. With zero, resolve `Unavailable`, keep the earned slot, grant exactly10 Ash once by offer ID, and let the next qualifying shrine retry. When no new/unfilled slot exists, do not create an offer: grant exactly10 Ash; first-time codex discovery is a separate idempotent write.

---

## 2. First Playable content pack

### CONT-FP-001 — Scope and persistence

- Bundle ID: `fp.1.0.0`.
- Arena ID: `arena.prototype.bell_laboratory_01`.
- Playable character: level-1 Grave Arbalist with the GDD's primary, Grave Mark, Slipstep, and Stillness values.
- This bundle is local and nonpersistent. Prototype item instances never enter account persistence, migration, gifting, crafting, or the production 90-item count.
- One run is three normal waves followed by `boss.prototype.bell_proctor`.
- Death deletes the current prototype run and returns the player to control in a new run within `3 seconds`.
- Fixed simulation seed is selectable in debug; default seed is hexadecimal `B311A501`.

### CONT-FP-002 — Arena geometry

Arena-local coordinates have a northwest origin. Walkable bounds are exactly `32 × 24 tiles`; a solid one-tile shell surrounds them.

- Player spawn: `(4,12)`.
- Boss spawn: `(24,12)`.
- Solid pillar rectangles: `[10,5,2,3]`, `[10,16,2,3]`, `[20,5,2,3]`, `[20,16,2,3]`.
- Wave anchors: `N1=(8,3)`, `N2=(16,3)`, `N3=(24,3)`, `S1=(8,21)`, `S2=(16,21)`, `S3=(24,21)`, `E1=(29,8)`, `E2=(29,16)`, `W1=(3,8)`, `W2=(3,16)`, `C=(16,12)`.
- Reward pedestal: `(4,4)`.
- Debug-only Red Tonic refill pedestal: `(4,20)`.
- Start every run with `2 × consumable.red_tonic`.
- Initial equipment: `item.prototype.weapon.pine_crossbow`, `item.prototype.relic.dented_scope`, `item.prototype.armor.reedcloth_wraps`, empty Charm.

### CONT-FP-003 — Exact wave sequence

| Wave | Spawn set | Start rule | Completion reward |
|---|---|---|---|
| 1 | `enemy.drowned_pilgrim` at `N1`, `N3`, `S1`, `S3` | `1.5 s` after the first player move/fire | `reward.prototype.wave_1` |
| 2 | `enemy.bell_reed` at `N2`, `S2`; Pilgrims at `W1`, `W2`, `E1`, `E2` | Wave 1 reward panel closes | `reward.prototype.wave_2` |
| 3 | `enemy.chain_sentry` at `C`; Bell Reeds at `(8,6)`, `(8,18)`; Pilgrims at `E1`, `E2`, `N3` | Wave 2 reward panel closes | `reward.prototype.wave_3` |
| Boss | `boss.prototype.bell_proctor` at `(24,12)` | Wave 3 panel closes; `2 s` introduction | `reward.prototype.boss` |

- Every spawn has a `900 ms` ground telegraph and cannot attack before it completes.
- Clear hostile projectiles when a wave completes. Wait `1.5 s`, then open rewards.
- Selecting a reward equips it if its slot is empty; otherwise it enters the prototype backpack.
- Reward UI does not pause simulation, but no hostile entity exists while it is open.
- Prototype wave-budget costs are Drowned Pilgrim=`1`, Bell Reed=`3`, Chain Sentry=`6`; therefore Waves1/2/3 total exactly4/10/15.

### CONT-FP-004 — Prototype enemy records

These records override the same stable IDs only in bundle `fp.1.0.0`.

#### `enemy.drowned_pilgrim`

```text
role: Fodder
health: 85
armor: 0
hurtbox_radius: 0.34
movement_speed: 2.2
aggro_radius: 10
leash_radius: 12
state: SpawnTelegraph(900) -> Acquire -> ApproachUntilDistance(5.0)
       -> AttackWindup(300) -> FireFan -> Recover(1900) -> Acquire
```

`FireFan` locks aim at windup start and fires three physical Chip projectiles at `-15°/0°/+15°`, speed `5.5`, radius `0.12`, lifetime `2.2 s`, raw damage `8`, origin `0.45 tiles` forward, threat cost `3`, `echo_memory_family=fan_projectile`.

#### `enemy.bell_reed`

```text
role: Pressure
health: 130
armor: 2
hurtbox_radius: 0.42
movement_speed: 0
aggro_radius: 11
leash_radius: 12
state: SpawnTelegraph(900) -> Dormant(500) -> RingTelegraph
       -> FireGapRing -> RecoverUntilCycle(3000) -> RingTelegraph
```

First telegraph is `450 ms`; later telegraphs are `300 ms`. Define eight indices at `45°`, emit six, omit adjacent indices starting `0,1`, then advance omitted-start by `3 mod 8` each cast. Projectiles: speed `4.5`, radius `0.13`, lifetime `3.0 s`, veil Chip damage `10`, threat `6`, `echo_memory_family=radial_projectile`.

#### `enemy.chain_sentry`

```text
role: Anchor
health: 300
armor: 5
hurtbox_radius: 0.55
movement_speed: 0
aggro_radius: 13
leash_radius: 13
state: SpawnTelegraph(900) -> Dormant(700) -> LaneTelegraph
       -> LaneImpact -> RecoverUntilCycle(4500) -> ToggleOrientation -> LaneTelegraph
```

First cast uses axes `0°/90°`; second uses `45°/135°`; alternate. Each centered lane reaches arena collision, width `0.9`; telegraph `800 ms` first and `650 ms` later; active `350 ms`; one hit/player/cast; physical Pressure damage `22`; threat `12`; `echo_memory_family=lane_or_beam`.

### CONT-FP-005 — Benchmark boss `boss.prototype.bell_proctor`

```text
display_name: Bell Proctor
health: 3000
armor: 4
hurtbox_radius: 0.65
position: fixed (24,12)
target_solo_duration: 75–110 seconds
soft_enrage_ms: 180000
introduction_ms: 2000
reward_table: reward.prototype.boss
```

The boss does not move, summon, or inflict status. Recall is disabled for the entire local Combat Laboratory by CONT-FP-010, not by this boss. Phase thresholds immediately cancel the old timeline and its hostile projectiles, then create a `3 s` break with `+20%` damage received and no attacks.

Shared patterns:

- **Aimed Fan (`pattern.prototype.bell_proctor.aimed_fan`):** lock aim on400 ms telegraph start; offsets `-20°,-10°,0°,+10°,+20°`; speed `6`; radius `0.12`; lifetime `3 s`; veil Chip damage `12`; threat5; `echo_memory_family=fan_projectile`; strafe; `consume_on_player_or_solid`.
- **Gap Ring (`pattern.prototype.bell_proctor.gap_ring`):** 16 indices at `22.5°`; omit four adjacent, initially starting index `0`, then advance start `5 mod 16`; telegraph650 ms unless the Phase3 memory preview replaces it; speed `4.5`; radius `0.13`; lifetime `4 s`; veil Pressure damage `15`; threat12; `echo_memory_family=radial_projectile`; follow gap; `consume_on_player_or_solid`.
- **Cross Lanes (`pattern.prototype.bell_proctor.cross_lanes`):** alternate axes `0°/90°` and `45°/135°`; two width-`1.0` lanes extend to collision; telegraph `900 ms`; active `500 ms`; physical Major damage `28` once/cast; threat12/lane, maximum24; `echo_memory_family=lane_or_beam`; leave lanes; `expire_at_authored_end`. No fan/ring may impact within `500 ms` of its impact.

| Phase | Loop | Exact timeline |
|---|---:|---|
| `100–70%` | `7200 ms` | Fan warn/fire `0/400`; Fan `2400/2800`; Ring `5600/6250` |
| `70–35%` | `10000 ms` | Fan `0/400`; Fan `2400/2800`; Ring `4200/4850`; Cross `7000/7900` |
| `35–0%` | `10000 ms` | Preview gap A `0–500`, wait400, emit Ring A at900; preview gap B `1000–1500`, wait300, emit Ring B at1800 with start index `A+4 mod16`; Fan warn/fire `4000/4400`; Cross warn/impact `6500/7400`; Fan `8400/8800` |

Below `20%`, Phase 3 restarts at `9000 ms` instead of `10000 ms`; no other value changes. Soft enrage shortens only remaining loop downtime by `15%` and never changes telegraphs, damage, speed, count, or geometry.

### CONT-FP-006 — Twelve prototype equipment templates

All values are fixed; there are no random affixes.

| ID | Slot/rarity | Exact behavior |
|---|---|---|
| `item.prototype.weapon.pine_crossbow` | Weapon/Worn | Fixed damage `20`; interval `455 ms`; range `9.5`; speed `12`; radius `0.10`; one bolt; no pierce. |
| `item.prototype.weapon.grave_repeater` | Weapon/Forged | Fixed damage `17`; interval `360 ms`; range `8.5`; speed `11`; radius `0.10`; one bolt; no pierce. |
| `item.prototype.weapon.longbolt_crossbow` | Weapon/Oathed | Fixed damage `28`; interval `600 ms`; range `12`; speed `15`; radius `0.09`; one bolt; no pierce. |
| `item.prototype.weapon.scatterbow` | Weapon/Relic | Three bolts at `-8°/0°/+8°`, each fixed damage `12`; interval `520 ms`; range `8`; speed `10.5`; radius `0.10`; one target receives at most two bolts/attack; displayed single-target `W=24`. |
| `item.prototype.relic.dented_scope` | Relic/Worn | Grave Mark range becomes `12`; otherwise unchanged. |
| `item.prototype.relic.mark_lens` | Relic/Oathed | Grave Mark lasts `6 s`; marked primary bonus becomes `12%`. |
| `item.prototype.relic.slip_clasp` | Relic/Oathed | Slipstep cooldown becomes `7 s`; empowered-shot window becomes `1.0 s`. |
| `item.prototype.armor.reedcloth_wraps` | Armor/Worn | `+8` max health. |
| `item.prototype.armor.parish_leather` | Armor/Forged | `+20` max health; `+2` armor; movement `×0.98`. |
| `item.prototype.armor.saltglass_coat` | Armor/Oathed | Max health `×0.92`; `+1` armor; `+12%` veil resistance. |
| `item.prototype.charm.still_eye` | Charm/Oathed | Stillness activates at `400 ms`; Focused damage becomes `+6%`; projectile speed remains `+10%`. |
| `item.prototype.charm.undertaker_knot` | Charm/Oathed | Red Tonic heals `35%` instead of `30%`; shared cooldown becomes `2.5 s`. |

### CONT-FP-007 — Red Tonic

```text
id: consumable.red_tonic
belt_stack_cap: 6
restore_max_health_basis_points: 3000
restore_duration_ms: 400
shared_potion_cooldown_ms: 2000
damage_interrupts_restore: false
consumed_on_use: true
```

### CONT-FP-008 — Prototype rewards

Global equipment weights sum to `100`: Pine Crossbow `12`, Grave Repeater `10`, Longbolt `6`, Scatterbow `6`, Dented Scope `12`, Mark Lens `8`, Slip Clasp `8`, Reedcloth `12`, Parish Leather `10`, Saltglass Coat `6`, Still Eye `6`, Undertaker Knot `4`.

| Table | Exact result |
|---|---|
| `reward.prototype.normal_enemy` | Independent `8%` global-equipment check and `10%` Red Tonic check; both may succeed |
| `reward.prototype.wave_1` | One weapon: Pine `35`, Repeater `30`, Longbolt `20`, Scatterbow `15`; plus one Tonic |
| `reward.prototype.wave_2` | One relic: Dented `40`, Mark Lens `30`, Slip Clasp `30`; one armor: Reedcloth `40`, Parish `35`, Saltglass `25` |
| `reward.prototype.wave_3` | One Charm: Still Eye `60`, Undertaker `40`; plus one nonduplicate global equipment selection |
| `reward.prototype.boss` | Three distinct global equipment selections without replacement; two Tonics; show clear time, damage taken, potion uses, lethal cause if any, current/best time |

All prototype rewards are destroyed when the run restarts.

### CONT-FP-009 — Acceptance fixtures

- `fixture.fp_arena_reachability`: radius-`0.30` player reaches every anchor.
- `fixture.fp_min_speed_paths`: a `4.5 tiles/s`, no-ability bot has a collision-free route through every mandatory boss pattern.
- `fixture.fp_wave_budget`: wave budgets are exactly `4`, `10`, `15`.
- `fixture.fp_no_spawn_hit`: no attack hits within `350 ms` after its spawn telegraph.
- `fixture.fp_reward_sums`: every percentage table totals `100%`.
- `fixture.fp_item_count`: exactly 12 prototype equipment templates plus the production Red Tonic reference.
- `fixture.fp_restart`: no prototype item or hostile entity survives restart.
- `fixture.fp_boss_phase_cancel`: each threshold cancels once.
- `fixture.fp_seed_trace`: same seed and fixed inputs produce identical spawn/attack/damage/drop ticks and state hashes.

### CONT-FP-010 — Prototype inventory, drops, Recall, and replay

- Equipment slots are Weapon/Relic/Armor/Charm; prototype backpack capacity is `8` nonbelt item stacks; belt has two slots. Tonic stack cap is `6`.
- The two starting Tonics occupy belt slot 1. Picking up a Tonic first merges belt slot 1, then belt slot 2 if it already contains Tonic, then one backpack stack; otherwise it remains a local personal ground stack for `60 s`.
- All three prototype enemies bind `reward_table=reward.prototype.normal_enemy`. Successful checks spawn a visible local personal ground pickup at the death position after `250 ms`; walk within `0.75 tiles` or press Interact within `1.25 tiles` to pick up.
- Equipment pickup fills an empty matching equipment slot only after explicit `Equip` confirmation; otherwise it fills the first backpack index. Field swap sends the old item to the first empty backpack index; reject swap when full. A pickup with no capacity remains for `60 s`, then disappears.
- Wave/boss reward-panel selections use the same capacity rules. The panel shows `Drop existing item`, `Leave reward`, and `Equip/Take`; no selection silently destroys an owned item.
- Prototype items bind to the local run and cannot be gifted, salvaged, crafted, or extracted.
- Recall input returns typed error `recall_unavailable_combat_laboratory`; HUD shows `RECALL UNAVAILABLE — LOCAL TEST`. This exception exists only in nonpersistent `fp.1.0.0`.
- Boss defeat opens the completion summary and a single primary `Run Again` action. Activating it destroys every run entity/item/stack, preserves only local best-time telemetry, and creates a fresh default-seed run with control in ≤`3 s`. Escape closes the summary but keeps the cleared empty arena; the pause menu exposes the same `Run Again` action.

---

## 3. Item math and affixes

### CONT-ITEM-001 — Level bands and rarity base

| Tier | Item levels |
|---|---:|
| T1 | `1–6` |
| T2 | `7–13` |
| T3 | `14–20` |

Ordinary weapon/relic templates remain legal above their minimum level. Only new-character defaults may be Worn. Generated ordinary equipment may be Forged, Oathed, Relic, or Sainted. Armor/Charms cannot be Black Unique in Early Access.

```text
rarity_base_multiplier:
  Worn         0.950
  Forged       1.000
  Oathed       1.015
  Relic        1.030
  Sainted      1.045
  BlackUnique  1.030
```

The multiplier affects weapon `W` and positive armor-family base values only. It never increases a tradeoff, cooldown, interval, movement penalty, or named Charm effect.

### CONT-ITEM-002 — Weapon and relic formulas

Let `L` be item level and `R` the rarity multiplier.

```text
sword_raw_W     = 20.00 + 1.25 * (L - 1)
crossbow_raw_W  = 15.00 + 0.95 * (L - 1)
hex_focus_raw_W = 19.00 + 1.15 * (L - 1)

W = round_half_up(
  family_raw_W
  * template_damage_scalar
  * R
  * (1 + sum(weapon_W_affixes))
)
```

`W` is the class-ability formula input. Conditional primary bonuses and signature components do not change displayed `W`.

| Family | Baseline interval | Range | Speed | Shape |
|---|---:|---:|---:|---|
| Sword | `625 ms` | `2.1` | n/a | `80°` arc |
| Crossbow | `454.545 ms` | `9.5` | `14` | one radius-`0.10` bolt |
| Hex Focus | `588.235 ms` | `7.5` | `11` | orb; `0.65` burst; secondary targets `65%` |

```text
final_attack_interval = authored_template_interval / (1 + total_attack_rate_bonus)

rarity_resonance_bonus:
  Worn 0.000; Forged 0.000; Oathed 0.005; Relic 0.010;
  Sainted 0.015; BlackUnique 0.010

relic_resonance = 1.000 + 0.004 * (L - 1) + rarity_resonance_bonus
```

Attack becomes legal on the first tick at/after timer expiry. Equipment attack-rate and cooldown caps remain GDD limits. Relic Resonance multiplies authored ability/passive damage only—not healing, reduction, invulnerability, movement, status/telegraph duration, or fixed signature damage unless explicitly inherited.

### CONT-ITEM-003 — Deterministic equipment counters

“Every Nth release” counters live on the equipped item instance in danger. Equip, unequip, transfer, destruction, or safe-instance entry resets them to zero. A released primary increments even on a miss. There are no critical hits, passive evasion, random combat procs, resurrection, or Recall acceleration.

### CONT-ITEM-004 — Exact base capability tags

Every template receives its slot/class/family tags plus these capability tags; fixed signature prose adds no implicit capability.

| Template group | Capability tags |
|---|---|
| All Sword weapons | `primary.arc`, `modifiable.W`, `modifiable.interval`, `modifiable.range`, `modifiable.primary_area` |
| All Crossbow weapons | `primary.projectile`, `modifiable.W`, `modifiable.interval`, `modifiable.range`, `modifiable.projectile_speed` |
| All Hex Focus weapons | `primary.projectile`, `primary.burst`, `modifiable.W`, `modifiable.interval`, `modifiable.range`, `modifiable.projectile_speed`, `modifiable.primary_area` |
| All Vanguard relics | `ability.guard`, `ability.rush`, `modifiable.ability_area`, `modifiable.positive_duration` |
| All Arbalist relics | `ability.mark`, `ability.slip`, `passive.stillness`, `modifiable.positive_duration`, `outgoing.status`, `self_mark_bonus` |
| All Witch relics | `ability.bloom`, `ability.fold`, `passive.withering`, `modifiable.ability_area`, `modifiable.positive_duration` |
| Armor | `slot.armor`, `modifiable.defense`, `modifiable.mobility`, `modifiable.utility` |
| Charm | `slot.charm`, `modifiable.offense`, `modifiable.ability`, `modifiable.defense`, `modifiable.mobility`, `modifiable.utility` |

Add `permits_pierce` only to `pine_crossbow`, `grave_repeater`, `pilgrim_longbolt`, `saltglass_piercer`, and `gravehook`. Add `ability.retort` and `modifiable.rush_damage` to every Vanguard relic except: Crownless Bulwark has neither; Saint's Last Wall has `ability.retort` but not `modifiable.rush_damage`. Add `black_unique` to the 12 Unique rows. Add `support` to Witness Pin and Mercy Salt. Add both `support` and `support_unique` to Crownless Bulwark, Root of Veyr, Bellwire Harness, and Saint's Last Wall—one Black Unique in each boss family.

Template-specific `affix_exclusions` are the missing capabilities implied above; there are no additional hidden exclusions. The affix compiler intersects permitted slot, required capability, Unique permission, item tier, and exclusivity groups.

### CONT-AFFIX-001 — Affix generation

Every affix costs exactly `10 affix_points`: Oathed gets one, Relic two, Sainted three, and Black Unique one supporting affix.

```text
seed = BLAKE3(server_secret_epoch_seed || item_uid || creation_event_id || content_version)
rng  = ChaCha8Rng(seed)
```

For each display-order slot: build legal candidates by slot/class/capability/item-level/Unique permission/exclusions; sort by ID; select by integer weight; remove selected/shared-exclusion candidates; uniformly select an inclusive stored step; persist ID/tier/value/content version. The build fails if any legal template/rarity cannot fill its affix count. Reforge remains player-selected at the exact legal midpoint under the GDD.

Range syntax is inclusive. `3–5 step 1` means `3,4,5`.

Milestone manifests:

- `manifest.affixes.core` is empty. M03 equipment instances are fixed Forged templates; the Core bundle uses `rarity.core_fixed = Forged 10000` so no partially implemented rarity can request affixes.
- `manifest.affixes.slice_12` enables exactly `weapon_force`, `quickened`, `long_reach`, `fast_flight`, `occult_force`, `hastened_rite`, `vitality`, `plating`, `warded`, `fleet`, `steadfast`, and `tonicwise`, using their full `affix.*` IDs below.
- `manifest.affixes.alpha_29` enables all 23 generic and six class-specific rows below.

M04 switches from `rarity.core_fixed` to the ordinary source rarity profiles only after all 12 enabled affixes pass completeness for every enabled template/rarity.

### CONT-AFFIX-002 — Generic catalog

| ID | Legal slots/requirements | T1 | T2 | T3 | Exclusion groups | Weight | Unique |
|---|---|---|---|---|---|---:|---|
| `affix.offense.weapon_force` | Weapon | W `+3–5%` step1 | `+5–7%` | `+7–9%` | `direct_output` | 100 | Yes |
| `affix.offense.quickened` | Weapon | Rate `+3–5%` step1 | `+5–7%` | `+7–9%` | `primary_cadence` | 100 | Yes |
| `affix.offense.long_reach` | Weapon | Range `+5–7%` | `+8–10%` | `+11–13%` | `primary_range` | 90 | Yes |
| `affix.offense.fast_flight` | Crossbow/Hex weapon | Speed `+8–12%` step2 | `+12–16%` | `+16–20%` | `projectile_flight` | 90 | Yes |
| `affix.offense.wide_pattern` | Sword arc/Hex burst | Area `+6–8%` | `+9–12%` | `+13–16%` | `primary_area` | 70 | No |
| `affix.offense.piercing` | Legal single-bolt Crossbow | unavailable | `+1` pierce | `+1` pierce | `primary_pierce` | 50 | No |
| `affix.offense.steady_hand` | Charm | Primary `+3–5%` | `+5–7%` | `+7–9%` | `direct_output` | 90 | No |
| `affix.ability.occult_force` | Relic/Charm | Ability `+4–6%` | `+6–8%` | `+8–10%` | `ability_output` | 100 | Yes |
| `affix.ability.hastened_rite` | Relic/Charm | CDR `3–5%` | `5–7%` | `7–9%` | `ability_cadence` | 100 | Yes |
| `affix.ability.ritual_span` | Vanguard/Witch relic with legal area | Area `+6–8%` | `+9–12%` | `+13–16%` | `ability_area` | 70 | No |
| `affix.ability.lingering` | Relic with legal positive duration | `+6–8%` | `+9–12%` | `+13–16%` | `ability_duration` | 70 | No |
| `affix.defense.vitality` | Armor/Relic/Charm | Health `+5–8` | `+10–15` | `+17–24` | `max_health_mod` | 100 | Yes |
| `affix.defense.plating` | Armor | Armor `+1` | `+1–2` | `+2–3` | `armor_mod` | 100 | No |
| `affix.defense.warded` | Armor/Charm | Resist `+2–3%` step0.5 | `+3.5–5%` | `+5.5–7%` | `resistance_mod` | 80 | No |
| `affix.mobility.fleet` | Armor/Charm | Move `+1–2%` step0.5 | `+2–3%` | `+3–4%` | `movement_mod` | 80 | No |
| `affix.status.steadfast` | Armor/Charm | Status `-8–12%` step2 | `-12–16%` | `-16–20%` | `self_status_duration` | 80 | No |
| `affix.status.keeper` | Arbalist/Witch relic | Applied status `+8–12%` step2 | `+12–16%` | `+16–20%` | `outgoing_status_duration` | 70 | No |
| `affix.utility.tonicwise` | Armor/Charm | Potion `+6–10%` step2 | `+10–14%` | `+14–18%` | `potion_output` | 70 | No |
| `affix.utility.reaching_hand` | Armor/Charm | Pickup `+0.25–0.50` step0.25 | `+0.50–0.75` | `+0.75–1.00` | `pickup_radius` | 60 | No |
| `affix.utility.deep_pockets` | Armor/Charm | unavailable | Pending `+1` | Pending `+1` | `pending_capacity` | 40 | No |
| `affix.risk.blood_honed` | Weapon/Charm | Direct `+7–9%`, health `-4%` | `+9–11%`, `-5%` | `+11–13%`, `-6%` | `direct_output,max_health_mod` | 45 | No |
| `affix.risk.glass_rite` | Relic/Charm | CDR `5–7%`, incoming `+4%` | `7–9%`,`+5%` | `9–11%`,`+6%` | `ability_cadence,incoming_risk` | 45 | No |
| `affix.risk.grave_weighted` | Armor | Armor `+2`, move `-2%` | `+3`,`-3%` | `+4`,`-4%` | `armor_mod,movement_mod` | 45 | No |

`ritual_span` may modify only Guard angle, Bastion shelter radius, Nailkeeper trap radius, Hex Bloom radius, or Black Lantern explosion radius. `lingering` may modify Guard active time, Mark duration, Nailkeeper trap life, Hex Bloom active time, or Bellwire life. Neither changes movement, invulnerability, Exhaustion, hostile telegraphs, cooldowns, or safe corridors. Equipment pending capacity is capped at `+2` before `bargain.ashen_pack`.

### CONT-AFFIX-003 — Class-specific catalog

| ID | Slot | T1 | T2 | T3 | Exclusion | Weight |
|---|---|---|---|---|---|---:|
| `affix.vanguard.retort_force` | Vanguard relic with Retort | `+0.10–0.14W` step0.02 | `+0.14–0.18W` | `+0.18–0.22W` | `retort_output` | 80 |
| `affix.vanguard.rush_force` | Vanguard relic with Rush damage | `+8–12%` step2 | `+12–16%` | `+16–20%` | `rush_output` | 80 |
| `affix.arbalist.mark_pressure` | Arbalist relic with self-mark bonus | `+2–3 pp` step0.5 | `+3–4 pp` | `+4–5 pp` | `mark_bonus` | 80 |
| `affix.arbalist.stillness_force` | Arbalist relic | Focused `+2–3 pp` step0.5 | `+3–4 pp` | `+4–5 pp` | `focused_bonus` | 80 |
| `affix.witch.bloom_force` | Witch relic | Bloom `+6–10%` step2 | `+10–14%` | `+14–18%` | `bloom_output` | 80 |
| `affix.witch.echo_force` | Witch relic | Withering Echo `+10–14%` step2 | `+14–18%` | `+18–22%` | `echo_output` | 80 |

Class-specific affixes are never legal on Black Uniques. A template that removes the mechanic removes its affix candidate.

### CONT-AFFIX-004 — Required metadata expansion

Every row in CONT-AFFIX-002/003 has `affix_points=10`, `tags=[numeric,<family>]`, and `family` equal to the ID's second segment (`offense`, `ability`, `defense`, `mobility`, `status`, `utility`, `risk`). Vanguard/Arbalist/Witch-specific rows use `family=ability`. Reforge “same family” means this field only.

If a later tier omits `step`, it inherits the exact stored unit below:

| Stored unit | Affix IDs |
|---|---|
| `100 basis points` | `weapon_force`, `quickened`, `long_reach`, `wide_pattern`, `steady_hand`, `occult_force`, `hastened_rite`, `ritual_span`, `lingering`, `blood_honed`, `glass_rite`; `grave_weighted` movement component |
| `200 basis points` | `fast_flight`, `steadfast`, `keeper`, `tonicwise`, `rush_force`, `bloom_force`, `echo_force` |
| `50 basis points` | `warded`, `fleet`, `mark_pressure`, `stillness_force` |
| `1 integer` | `piercing`, `vitality`, `plating`, `deep_pockets`; `grave_weighted` armor component |
| `250 milli-tiles` | `reaching_hand` |
| `0.02W coefficient` | `retort_force` |

Display formats are exact: single percentages use `"{signed_value}% {stat_display_name}"`; flat health/armor/pierce/pending use `"{signed_integer} {stat_display_name}"`; distance uses `"{signed_value:0.00} tiles Pickup Radius"`; coefficient uses `"{signed_value:0.00}W Retort Damage"`; percentage-point class bonuses use `"{signed_value:0.0} pp {stat_display_name}"`. Composite Risk rows display both clauses separated by `"; "` in the order shown in the catalog. `grave_weighted`, `blood_honed`, and `glass_rite` additionally receive tag `composite`; all Risk rows receive `voluntary_risk`.

### CONT-AFFIX-005 — Resolution order

```text
1 class base; 2 level growth; 3 item family/template base;
4 specified rarity multiplier; 5 flat affixes; 6 same-family percentages;
7 fixed tradeoffs/multiplicative health losses; 8 oath/Bargain;
9 global caps; 10 field-boundary rounding
```

Percentage-point changes add to an authored percentage. Multipliers multiply the resolved coefficient. All affix RNG occurs only at item creation.

---

## 4. Exact 90-template item catalog

### CONT-CATALOG-001 — Count invariant

| Category | Ordinary | Black Unique | Total |
|---|---:|---:|---:|
| Vanguard weapons | 6 | 2 | 8 |
| Vanguard relics | 6 | 2 | 8 |
| Arbalist weapons | 6 | 2 | 8 |
| Arbalist relics | 6 | 2 | 8 |
| Witch weapons | 6 | 2 | 8 |
| Witch relics | 6 | 2 | 8 |
| Shared armor | 18 | 0 | 18 |
| Shared charms | 18 | 0 | 18 |
| Consumables/materials | 6 | 0 | 6 |
| **Total** | **78** | **12** | **90** |

The final six are exactly `consumable.red_tonic`, `consumable.purifying_salt`, `material.bell_brass`, `material.funeral_root`, `material.saltglass_shard`, and `material.echo_ember`, with behavior in GDD LOOT-032.

### CONT-CATALOG-002 — Milestone item manifests

`manifest.items.core_18` contains exactly:

- Weapons: `item.weapon.crossbow.pine_crossbow`, `item.weapon.crossbow.grave_repeater`, `item.weapon.crossbow.pilgrim_longbolt`, `item.weapon.crossbow.mourners_fan`.
- Relics: `item.relic.arbalist.cracked_mark_lens`, `item.relic.arbalist.long_lens`, `item.relic.arbalist.barbed_ledger`, `item.relic.arbalist.slip_clasp`.
- All six T1 Armor IDs in CONT-CATALOG-040.
- Charms: `item.charm.ember_tooth.t1`, `item.charm.bell_locket.t1`, `item.charm.salt_knot.t1`.
- `consumable.red_tonic`.

Count: `4+4+6+3+1=18`; every equipment template is legal by level 10.

Core is wipeable and uses one explicit compatibility override: enabled T1 Armor/Charm templates accept item levels1–10 so every universal Core roll has a legal template. Core source item levels are normal1–6, Elite2–8, miniboss5–10, and Caldus8–10. Slice removes this override and restores production T1 legality1–6; no Core item migrates forward.

`manifest.items.slice_45` contains exactly:

- Thirteen Vanguard weapon/relic rows: every Vanguard row except `item.weapon.sword.thorn_processional`, Alpha-source `item.weapon.sword.cinder_procession`, and `item.relic.vanguard.saints_last_wall`.
- Thirteen Arbalist weapon/relic rows: every Arbalist row except `item.weapon.crossbow.gravehook`, Alpha-source `item.weapon.crossbow.saltglass_repeater`, and `item.relic.arbalist.bellwire_harness`.
- The Core shared set: all six T1 Armor rows plus `item.charm.ember_tooth.t1`, `item.charm.bell_locket.t1`, `item.charm.salt_knot.t1`.
- Five higher-band shared rows: `item.armor.ashplate.t2`, `item.armor.ashplate.t3`, `item.armor.pilgrim.t2`, `item.charm.ember_tooth.t2`, `item.charm.ember_tooth.t3`.
- `consumable.red_tonic`, `material.bell_brass`, `material.funeral_root`, `material.saltglass_shard`, and `material.echo_ember`.

Count: `13+13+9+5+5=45`. This is a strict superset of Core and guarantees at least one enabled Armor and Charm template in T1/T2/T3. The six omitted class rows and every other shared row first enable in Alpha; `consumable.purifying_salt` also first enables in Alpha.

`manifest.items.alpha_90` contains every production row in CONT-CATALOG-001. Manifests are strict supersets: Core ⊂ Slice ⊂ Alpha. M01 instead uses its 12 isolated prototype equipment rows plus the shared Red Tonic, requiring 13 icons.

`manifest.items.behavior_change_25` is the exact Early Access set tagged `behavior_change`; an item qualifies only when its authored template changes attack geometry/count/cadence, adds a status or spawned gameplay object, changes an ability state machine, or adds an ally/enemy interaction beyond a flat unconditional stat:

- Vanguard weapons (7): `item.weapon.sword.broadbell_falchion`, `item.weapon.sword.pilgrim_fang`, `item.weapon.sword.bell_greatsword`, `item.weapon.sword.twin_ash_sabre`, `item.weapon.sword.thorn_processional`, `item.weapon.sword.bell_cleaver_caldus`, `item.weapon.sword.cinder_procession`.
- Vanguard relics (4): `item.relic.vanguard.wide_guard`, `item.relic.vanguard.oath_clasp`, `item.relic.vanguard.crownless_bulwark`, `item.relic.vanguard.saints_last_wall`.
- Arbalist weapons (4): `item.weapon.crossbow.mourners_fan`, `item.weapon.crossbow.saltglass_piercer`, `item.weapon.crossbow.coffin_nail`, `item.weapon.crossbow.saltglass_repeater`.
- Arbalist relics (3): `item.relic.arbalist.witness_pin`, `item.relic.arbalist.executioners_ledger`, `item.relic.arbalist.bellwire_harness`.
- Witch weapons (4): `item.weapon.hex_focus.twin_candle`, `item.weapon.hex_focus.root_spiral`, `item.weapon.hex_focus.mask_last_spring`, `item.weapon.hex_focus.saltglass_prism`.
- Witch relics (3): `item.relic.witch.mercy_salt`, `item.relic.witch.root_of_veyr`, `item.relic.witch.black_lantern`.

Count: `7+4+4+3+4+3=25`. The compiler adds the tag only to these IDs and fails if an ID is missing, duplicated, loses its qualifying behavior, or the set falls below the GDD minimum of24.

### CONT-CATALOG-003 — Production starter kits

Each new/successor character receives exactly two new Worn, zero-salvage equipment instances, empty Armor/Charm, and `2 × consumable.red_tonic` placed in belt slot 1. Belt slot 2 is empty.

| Class | Weapon | Relic |
|---|---|---|
| Ashen Vanguard | `item.weapon.sword.rusted_cleaver` | `item.relic.vanguard.dented_shield` |
| Grave Arbalist | `item.weapon.crossbow.pine_crossbow` | `item.relic.arbalist.cracked_mark_lens` |
| Veil Witch | `item.weapon.hex_focus.funeral_focus` | `item.relic.witch.cracked_censer` |

Starter instances use `provenance=Starter`; the Tonics use `provenance=Grant`, are at risk with the character, and are replenished only on a new successor—not on ordinary Hall return.

### CONT-CATALOG-010 — Vanguard weapons

| ID | Minimum/type | Exact primary behavior |
|---|---|---|
| `item.weapon.sword.rusted_cleaver` | 1 / Ordinary | `damage_scalar=1.00`; baseline `625 ms`, `2.1`, `80°`, `100 ms` windup; starter. |
| `item.weapon.sword.broadbell_falchion` | 4 / Ordinary | `damage_scalar=0.92`; `625 ms`; `2.1`; `110°`; `120 ms` windup. |
| `item.weapon.sword.pilgrim_fang` | 7 / Ordinary | `damage_scalar=1.06`; `625 ms`; `2.55`; `55°`; `120 ms` windup. |
| `item.weapon.sword.bell_greatsword` | 10 / Ordinary | `damage_scalar=1.12`; `725 ms`; `2.3`; `85°`; `170 ms` windup. |
| `item.weapon.sword.twin_ash_sabre` | 14 / Ordinary | `damage_scalar=0.78`; `500 ms`; `2.1`; `55°`; odd releases center `-25°`, even `+25°`. |
| `item.weapon.sword.thorn_processional` | 18 / Ordinary | `damage_scalar=0.96`; baseline. Every fourth released primary that hits applies Bleed: three `0.12W` ticks at 1 s intervals; refresh, no stack. |
| `item.weapon.sword.bell_cleaver_caldus` | 8 / Unique / Caldus | `damage_scalar=1.05`; `675 ms`; `2.2`; `90°`. Every third release is `210°`, range `2.6`, damage `1.25W`; `220 ms` windup, movement `×0.70` during windup plus `150 ms` recovery. |
| `item.weapon.sword.cinder_procession` | 18 / Unique / Bell Warden | `damage_scalar=1.00`; `650 ms`; baseline arc. Swing deals `0.80W` and leaves arc patch `1.2 s`; entering enemy takes `0.22W` once/patch; max two; health `×0.92`. Patch is primary signature damage. |

### CONT-CATALOG-011 — Vanguard relics

| ID | Minimum/type | Exact replacement |
|---|---|---|
| `item.relic.vanguard.dented_shield` | 1 / Ordinary | No kit change; starter. |
| `item.relic.vanguard.wide_guard` | 4 / Ordinary | Guard `150°`; projectile reduction `65%`. |
| `item.relic.vanguard.ember_spur` | 7 / Ordinary | Rush `2.8 tiles/300 ms`; reduction `30%`; damage/cooldown unchanged. |
| `item.relic.vanguard.quiet_bell` | 10 / Ordinary | Guard cooldown `5.1 s` before oath; Retort `1.20W`. |
| `item.relic.vanguard.ram_sigil` | 14 / Ordinary | Rush `2.2`; `1.15W`; `8 s`; duration/reduction `260 ms/40%`. |
| `item.relic.vanguard.oath_clasp` | 18 / Ordinary | Guard `700 ms`, base cooldown `6.5 s`; Bell Retort second charge adds `0.80W/0.35`; Bastion shelter `1.8`; its `-10%` damage remains. |
| `item.relic.vanguard.crownless_bulwark` | 8 / Unique / Caldus | Guard `360°/800 ms/55%`; allies within `1.5` tiles receive `20%` projectile reduction, strongest applies; tags `support,support_unique`; cannot Retort; owner cannot move/fire/cast during Guard. |
| `item.relic.vanguard.saints_last_wall` | 18 / Unique / Bell Warden | Rush endpoint shelter radius `1.8`, duration `2 s`, `20%` projectile reduction, strongest applies, tags `support,support_unique`; Rush damage zero, distance `2.2`, cooldown `9 s`. |

`saints_debt` makes Last Wall shelter `26%` before caps.

It also makes Crownless Bulwark's ally reduction `26%` before caps.

### CONT-CATALOG-020 — Arbalist weapons

| ID | Minimum/type | Exact primary behavior |
|---|---|---|
| `item.weapon.crossbow.pine_crossbow` | 1 / Ordinary | `damage_scalar=1.00`; baseline; starter. |
| `item.weapon.crossbow.grave_repeater` | 4 / Ordinary | `damage_scalar=0.84`; `382 ms`; range `8.5`; speed `14`. |
| `item.weapon.crossbow.pilgrim_longbolt` | 7 / Ordinary | `damage_scalar=1.10`; `550 ms`; range `11.5`; speed `17`; radius `0.09`. |
| `item.weapon.crossbow.mourners_fan` | 10 / Ordinary | `damage_scalar=0.82`; `520 ms`; range `7.5`; bolts `-12°/0°/+12°`; max one hit/target/release. |
| `item.weapon.crossbow.saltglass_piercer` | 14 / Ordinary | `damage_scalar=0.90`; baseline; one pierce; first `1.00W`, second `0.70W`, then ends. |
| `item.weapon.crossbow.gravehook` | 18 / Ordinary | `damage_scalar=0.95`; baseline; owner-marked target gets additional `+12%`, additive with class Mark. |
| `item.weapon.crossbow.coffin_nail` | 12 / Unique / Mother Veyr | `damage_scalar=1.05`; `500 ms`; baseline range/speed. Hit on owner's marked target consumes it after damage and schedules radius-`0.8` burst after `350 ms` for `0.90W`; Mark cooldown `7 s`. |
| `item.weapon.crossbow.saltglass_repeater` | 16 / Unique / Salt Confessor | `damage_scalar=0.92`; `500 ms`; every third release three bolts `-16°/0°/+16°`, each `0.70W`, max one hit/target; otherwise `1W`; range `7.6`, speed `11.5`. |

Coffin Nail burst survives target death and cannot consume another Arbalist's mark.

### CONT-CATALOG-021 — Arbalist relics

| ID | Minimum/type | Exact replacement |
|---|---|---|
| `item.relic.arbalist.cracked_mark_lens` | 1 / Ordinary | No kit change; starter. |
| `item.relic.arbalist.long_lens` | 4 / Ordinary | Mark range `13`; speed `15`; damage `1.55W`. |
| `item.relic.arbalist.barbed_ledger` | 7 / Ordinary | Mark `6 s`; primary bonus `20%`; direct damage `1.45W`. |
| `item.relic.arbalist.slip_clasp` | 10 / Ordinary | Slip `2.5/220 ms`; reduction `20%`; cooldown `9 s`. |
| `item.relic.arbalist.witness_pin` | 14 / Ordinary | Mark `1.35W`; primary bonus `10%`; target outgoing player-direct damage `-8%`, strongest applies; `support`. |
| `item.relic.arbalist.stillness_stock` | 18 / Ordinary | Focus after `800 ms`; bonuses `+15%` projectile speed and `+14%` primary damage; break rules unchanged. |
| `item.relic.arbalist.executioners_ledger` | 12 / Unique / Mother Veyr | Mark tracks owner's actual health damage; target death or `15%` max-health total resets Slip cooldown and ends Mark, once/Mark; Mark `1.35W`, bonus `8%`. Exclude overkill/other players/reflection/environment. |
| `item.relic.arbalist.bellwire_harness` | 16 / Unique / Salt Confessor | Slip leaves start-end wire `1.5 s`; first enemy crossing takes `0.80W`, Frostbind `1.25 s`, and deals `10%` less direct damage to players for `2 s`, strongest applies; wire removed; max one; tags `support,support_unique`; no travel reduction; cooldown `9 s`. |

`saints_debt` makes Witness Pin reduction `10.4%` before caps.

It makes Bellwire Harness's triggered reduction `13%` before caps.

### CONT-CATALOG-030 — Witch weapons

| ID | Minimum/type | Exact primary behavior |
|---|---|---|
| `item.weapon.hex_focus.funeral_focus` | 1 / Ordinary | `damage_scalar=1.00`; baseline; starter. |
| `item.weapon.hex_focus.wide_mourner` | 4 / Ordinary | `damage_scalar=0.92`; range `7`; burst `0.90`; secondary `55%`. |
| `item.weapon.hex_focus.salt_needle` | 7 / Ordinary | `damage_scalar=1.08`; `640 ms`; range `9.5`; speed `13`; burst `0.35`; secondary `75%`. |
| `item.weapon.hex_focus.twin_candle` | 10 / Ordinary | `damage_scalar=0.82`; `650 ms`; range `7`; two orbs `-7°/+7°`; max one hit/target/release. |
| `item.weapon.hex_focus.root_spiral` | 14 / Ordinary | `damage_scalar=0.88`; `625 ms`; baseline geometry; burst Frostbind `0.75 s`. |
| `item.weapon.hex_focus.black_psalm` | 18 / Ordinary | `damage_scalar=0.92`; baseline; damage to Hexed `×1.18` after weapon modifiers. |
| `item.weapon.hex_focus.mask_last_spring` | 12 / Unique / Mother Veyr | `damage_scalar=0.82`; range `6.5`; damaging burst may create spring, ICD `2 s`, max two, lasts `1.5 s`, owner entry consumes/heals `2.5%`; cannot spawn from prop/invulnerable target. |
| `item.weapon.hex_focus.saltglass_prism` | 16 / Unique / Salt Confessor | `damage_scalar=0.90`; range `6.8`; first wall hit before enemy splits reflected orbs `±25°`, each `0.55W`, no resplit; Saltglass oath makes `0.65W` without another reflection; secondary burst `45%`. |

### CONT-CATALOG-031 — Witch relics

| ID | Minimum/type | Exact replacement |
|---|---|---|
| `item.relic.witch.cracked_censer` | 1 / Ordinary | No kit change; starter. |
| `item.relic.witch.bloom_censer` | 4 / Ordinary | Bloom radius `1.85`; ticks `0.27W`; timing/cap unchanged. |
| `item.relic.witch.long_wick` | 7 / Ordinary | Bloom duration `5 s`; cooldown `7.5 s`; max two. |
| `item.relic.witch.folded_shroud` | 10 / Ordinary | Fold cooldown `8 s`; distance `2.1`; invulnerability `200 ms`. |
| `item.relic.witch.echo_thorn` | 14 / Ordinary | Withering `0.60W`, radius `1.0`; Bloom ticks `0.27W`; limits unchanged. |
| `item.relic.witch.mercy_salt` | 18 / Ordinary | Enemies in Bloom deal players `8%` less direct damage, strongest; owner Bloom/Withering `×0.90`; `support`. |
| `item.relic.witch.root_of_veyr` | 12 / Unique / Mother Veyr | At telegraph end choose nearest enemy within `1.6` of target point, tie lowest entity ID; attach/follow for `4 s`, radius `1.25`; attached target deals `8%` less direct damage to players, strongest applies; if none place normally with no reduction; max one Bloom; cooldown `8 s`; tags `support,support_unique`. |
| `item.relic.witch.black_lantern` | 18 / Unique / Bell Warden | Accepted Fold leaves origin lantern; after `900 ms`, radius `1.1`, `1.10W`, Hex `2 s`, inherits ability damage; Fold no invulnerability, cooldown `10.5 s`. |

`saints_debt` makes Mercy Salt reduction `10.4%` before caps.

It makes Root of Veyr's attached-target reduction `10.4%` before caps.

### CONT-CATALOG-040 — Shared armor formulas and templates

```text
final_base_health = round_half_up(raw_health * R)
final_base_armor  = round_half_up(raw_armor * R)
final_resistance  = round_to_0.1pp(raw_resistance * R)
```

Affixes add afterward; fixed implicits do not rarity-scale.

| Family | Raw positive stats | Fixed implicit |
|---|---|---|
| Ashplate | `health=6+1.00L`; `armor=2+0.30L` | Movement `-3%` |
| Gravehide | `health=14+1.80L`; `armor=0.5+0.10L` | none |
| Saltglass | `health=6+0.80L`; `armor=0.5+0.08L`; resist=`4.0%+0.30%L` | Healing received `-8%` |
| Pilgrim | `health=6+0.80L`; `armor=0.05L` | Movement `+4%` |
| Rootweave | `health=10+1.40L`; `armor=0.5+0.10L` | Bleed/Frostbind/Silence/Hex/Guardbreak duration `-12%` |
| Bellguard | `health=8+1.00L`; `armor=1+0.22L` | On Major/Severe direct hit, barrier `round_half_up((5+L)R)` for `3 s`, ICD `12 s` |

Bellguard cannot retrigger while its barrier is active; normal barrier combination rules apply.

| Family | T1 ID/name | T2 ID/name | T3 ID/name |
|---|---|---|---|
| Ashplate | `item.armor.ashplate.t1` Sootplate Vest | `item.armor.ashplate.t2` Ashplate Coat | `item.armor.ashplate.t3` Processional Plate |
| Gravehide | `item.armor.gravehide.t1` Worn Gravehide | `item.armor.gravehide.t2` Stitched Gravehide | `item.armor.gravehide.t3` Ossuary Hide |
| Saltglass | `item.armor.saltglass.t1` Salt-Thread Jerkin | `item.armor.saltglass.t2` Saltglass Hauberk | `item.armor.saltglass.t3` Reliquary Mail |
| Pilgrim | `item.armor.pilgrim.t1` Roadworn Wraps | `item.armor.pilgrim.t2` Pilgrim Mantle | `item.armor.pilgrim.t3` Far-Walker Shroud |
| Rootweave | `item.armor.rootweave.t1` Rootstitch Vest | `item.armor.rootweave.t2` Chapel Rootweave | `item.armor.rootweave.t3` Elder Briar Robe |
| Bellguard | `item.armor.bellguard.t1` Brass-Rib Harness | `item.armor.bellguard.t2` Bellguard Cuirass | `item.armor.bellguard.t3` Warden Carapace |

T1/T2/T3 legal levels are `1–6`, `7–13`, and `14–20` respectively.

### CONT-CATALOG-050 — Shared Charms

Named effects are exact and not rarity-scaled.

| ID | Levels | Exact named effect |
|---|---:|---|
| `item.charm.ember_tooth.t1` Ash Chip | 1–6 | After `2 s` without primary release, next release `+15%` direct damage; consumed hit/miss. |
| `item.charm.ember_tooth.t2` Ember Tooth | 7–13 | Same, `+20%`. |
| `item.charm.ember_tooth.t3` Cinder Fang | 14–20 | Same, `+25%`. |
| `item.charm.bell_locket.t1` Tin Bell | 1–6 | Potion healing `+10%`. |
| `item.charm.bell_locket.t2` Grave Bell | 7–13 | Potion healing `+15%`. |
| `item.charm.bell_locket.t3` Last Bell | 14–20 | Potion healing `+20%`. |
| `item.charm.salt_knot.t1` Salt Thread | 1–6 | Bleed/Frostbind/Silence/Hex/Guardbreak duration `-10%`; not Exhaustion/encounter Marked. |
| `item.charm.salt_knot.t2` Salt Knot | 7–13 | Same, `-15%`. |
| `item.charm.salt_knot.t3` Salt Rosary | 14–20 | Same, `-20%`. |
| `item.charm.pilgrim_spur.t1` Worn Spur | 1–6 | After `3 s` without direct damage, move `+2%`; ends on direct damage. |
| `item.charm.pilgrim_spur.t2` Pilgrim Spur | 7–13 | Same, `+3%`. |
| `item.charm.pilgrim_spur.t3` Far-Step Spur | 14–20 | Same, `+4%`. |
| `item.charm.funeral_root.t1` Root Bead | 1–6 | XP-enemy kill heals `1.0%` max, ICD `2 s`; `no_on_kill` excluded. |
| `item.charm.funeral_root.t2` Funeral Root | 7–13 | Same, `1.5%`. |
| `item.charm.funeral_root.t3` Briar Heart | 14–20 | Same, `2.0%`. |
| `item.charm.black_candle.t1` Soot Wick | 1–6 | At ≤35% health, ability `+10%`; healing always `-5%`. |
| `item.charm.black_candle.t2` Black Candle | 7–13 | Ability `+15%`; healing `-7.5%`. |
| `item.charm.black_candle.t3` Eclipse Taper | 14–20 | Ability `+20%`; healing `-10%`. |

Boss death triggers Funeral Root once. Phase transitions and `no_on_kill` summons do not.

---

## 5. Reward and rarity tables

### CONT-REWARD-001 — Roll types

- `equipment_roll`: class weapon/relic or shared armor/charm; never material.
- `universal_item_roll`: `50%` armor, `50%` Charm; never weapon/relic/material.
- `material_roll`: only the table's consumable/material stacks; never equipment.
- Equipment usability weights are GDD values: current-class weapon/relic `75%`, other-class weapon/relic `10%`, universal armor/charm `15%`. Within current/other class choose weapon/relic `50/50`; within universal choose armor/charm `50/50`.
- For `other-class`, form one pool containing every legal template for the selected slot from all released noncurrent classes, sort by ID, and select uniformly unless the source's Unique pool supplies explicit weights. Do not select an other class first and bias a two-class release.
- Choose uniformly among legal ordinary templates after tier/source restrictions unless a Unique pool below supplies explicit candidates. Sort IDs before the draw.
- Choose item level uniformly across the inclusive source range, then build the legal template pool for that exact level.
- Realm sources use the containing cell's Outer/Parish/Heart profile. Bell Sepulcher uses Outer, Root Chapel uses Parish, and Drowned Reliquary uses Heart for normal/Elite profiles. Explicit event/miniboss/boss profiles override location. Moving an enemy across a boundary does not change the profile captured at spawn.

### CONT-REWARD-002 — Source levels and rarity basis points

Every row totals `10000`. These are per `equipment_roll`.

| Reward profile | Item level | Forged | Oathed | Relic | Sainted | Black Unique |
|---|---:|---:|---:|---:|---:|---:|
| `rarity.normal_outer` | 1–6 | 7000 | 2600 | 400 | 0 | 0 |
| `rarity.normal_parish` | 5–13 | 5500 | 3500 | 900 | 100 | 0 |
| `rarity.normal_heart` | 11–20 | 3500 | 4000 | 2200 | 300 | 0 |
| `rarity.elite_outer` | 2–8 | 5000 | 3500 | 1400 | 100 | 0 |
| `rarity.elite_parish` | 7–15 | 3000 | 4000 | 2600 | 400 | 0 |
| `rarity.elite_heart` | 13–20 | 2000 | 3500 | 3700 | 800 | 0 |
| `rarity.minor_event` | 2–8 | 4000 | 4000 | 1800 | 200 | 0 |
| `rarity.major_event` | 7–15 | 2000 | 4000 | 3300 | 700 | 0 |
| `rarity.miniboss_t1` | 5–10 | 1500 | 4500 | 3200 | 800 | 0 |
| `rarity.miniboss_t2` | 10–16 | 500 | 3500 | 4500 | 1500 | 0 |
| `rarity.miniboss_t3` | 15–20 | 0 | 2000 | 5000 | 3000 | 0 |
| `rarity.boss_caldus` | 8–11 | 0 | 3000 | 4800 | 1900 | 300 |
| `rarity.boss_veyr` | 12–17 | 0 | 2000 | 4700 | 2800 | 500 |
| `rarity.boss_confessor` | 17–20 | 0 | 1000 | 4000 | 4200 | 800 |
| `rarity.world_warden` | 18–20 | 0 | 1000 | 3500 | 4500 | 1000 |

Worn is starter/tutorial only and therefore omitted. For `universal_item_roll`, add the profile's Black Unique weight to Sainted before drawing because Armor/Charm cannot be Unique. For any equipment usability pool with zero legal source Uniques, also move its Unique weight to Sainted before the rarity draw; do not draw and reroll.

### CONT-REWARD-003 — Exact source quantities

| Source profile | Exact personal processing |
|---|---|
| `reward.normal_outer/parish/heart` | Independent `8% universal_item_roll`; independent `12% material_roll` |
| `reward.elite_outer/parish/heart` | One equipment roll using matching elite rarity; independent `25% material_roll` |
| `reward.event_minor` | One equipment roll `rarity.minor_event`; one material roll |
| `reward.event_major` | Two equipment rolls `rarity.major_event`; one material roll |
| `reward.miniboss_t1/t2/t3` | One matching equipment roll; independent `35%` second matching equipment roll; T3 also has independent `25% material.saltglass_shard ×1` |
| `reward.boss_caldus` | Two equipment rolls; one 100%-singleton `material_roll` yielding Bell Brass ×2; `weekly_family_fragment_check(caldus)` |
| `reward.boss_veyr` | Two equipment rolls; one 100%-singleton `material_roll` yielding Funeral Root ×2; `weekly_family_fragment_check(veyr)` |
| `reward.boss_confessor` | Two equipment rolls; one 100%-singleton `material_roll` yielding Saltglass Shard ×2; `weekly_family_fragment_check(confessor)` |
| `reward.world_warden` | Three equipment rolls; one 100%-singleton Saltglass Shard ×1 `material_roll`; one `material.heart_bonus` roll; `weekly_family_fragment_check(warden)` and cosmetic/codex checks |

### CONT-REWARD-004 — Material pools

Weights total `100` and are selected only after a material roll exists.

Material reward tags are fixed: `material.bell_brass=[bell_craft]`, `material.funeral_root=[root_craft,curse_material]`, `material.saltglass_shard=[salt_craft,precision_material]`, `material.echo_ember=[echo_cosmetic]`; consumables have no material reward tag.

| Pool | Outcomes/weights |
|---|---|
| `material.normal_outer` | Red Tonic `85`; Purifying Salt `15` |
| `material.normal_parish` | Red Tonic `75`; Purifying Salt `25` |
| `material.normal_heart` | Red Tonic `60`; Purifying Salt `40` |
| `material.elite_outer` | Bell Brass ×1 `40`; Red Tonic `50`; Purifying Salt `10` |
| `material.elite_parish` | Bell Brass ×1 `40`; Red Tonic `40`; Purifying Salt `20` |
| `material.elite_heart` | Bell Brass ×1 `40`; Red Tonic `30`; Purifying Salt `30` |
| `material.minor_event` | Bell Brass ×1 `100` |
| `material.major_event` | Funeral Root ×1 `100` |
| `material.heart_bonus` | Red Tonic `50`; Purifying Salt `50` |

Secret-room equipment and material grants are defined exclusively by CONT-ROOM-008; no reward profile adds a second secret-room material roll. Requiem and modifier material checks are defined in Sections 9–10.

Stage pool binding prevents references to disabled item definitions:

| Stage | Exact override |
|---|---|
| Core | All equipment uses `rarity.core_fixed=Forged 10000` and the Core source levels in CONT-CATALOG-002. With only one released class, move the unavailable10% other-class usability weight to current-class, producing85% current weapon/relic and15% universal Armor/Charm. Every normal/Elite material roll uses `consumable.red_tonic ×1` weight100. `reward.boss_caldus` uses two equipment rolls plus one singleton material roll of Red Tonic×2; family Fragment/cosmetic checks are disabled. No Core source references Brass, Root, Saltglass, Ember, or Purifying Salt. |
| Slice | Every normal material pool is Red Tonic×1 weight100. Every Outer/Parish/Heart Elite material pool is Bell Brass×1 weight40 and Red Tonic×1 weight60. Minor/major event, Caldus, Veyr, Root-secret, Glass Floor, and Requiem sources use their ordinary Brass/Root/Saltglass/Ember outcomes. `material.heart_bonus` is Red Tonic×1 weight100. Family Fragment checks and Veil Seal awards remain disabled until Alpha/M05 implements them. No Slice source references Purifying Salt. |
| Alpha/Playtest/EA | Use the production tables above and every CONT-REWARD-003 quantity/check exactly. |

The stage compiler expands these as bundle-specific payloads under the same stable reward IDs and verifies that every outcome definition belongs to that stage's exact item manifest.

### CONT-REWARD-005 — Black Unique pools and fragments

All candidates in a pool have base integer weight `100`. Apply `modifier.saints_debt` only as Section 10 specifies.

| Source/pity family | Eligible Black Uniques |
|---|---|
| `unique.caldus` / `fragment.caldus` | `item.weapon.sword.bell_cleaver_caldus`; `item.relic.vanguard.crownless_bulwark` |
| `unique.veyr` / `fragment.veyr` | `item.weapon.crossbow.coffin_nail`; `item.relic.arbalist.executioners_ledger`; `item.weapon.hex_focus.mask_last_spring`; `item.relic.witch.root_of_veyr` |
| `unique.confessor` / `fragment.confessor` | `item.weapon.crossbow.saltglass_repeater`; `item.relic.arbalist.bellwire_harness`; `item.weapon.hex_focus.saltglass_prism` |
| `unique.warden` / `fragment.warden` | `item.weapon.sword.cinder_procession`; `item.relic.vanguard.saints_last_wall`; `item.relic.witch.black_lantern` |

Direct selection uses only templates whose catalog row says Unique. The validator enforces exactly 12 unique catalog templates globally and rejects any non-Unique pool entry.

`rarity.boss_caldus`, `rarity.boss_veyr`, `rarity.boss_confessor`, and `rarity.world_warden` bind one-to-one to `unique.caldus`, `unique.veyr`, `unique.confessor`, and `unique.warden`. No other Early Access reward profile can roll Black Unique rarity.

Fragment redemption at `20` matching Fragments offers every unowned Black Unique in that family sorted by ID. If none is unowned, use the GDD duplicate-or-currency choice. Fragment grant limits and Monday reset use GDD LOOT-012.

`weekly_family_fragment_check(family)` atomically reads `weekly_family_clear_ordinal_before`; grant one Fragment iff it is `<3`, then increment the ordinal for every eligible clear. Retry uses the clear idempotency key and cannot increment/grant twice.

---

## 6. Mire of Bells macro map

### CONT-WORLD-001 — Compilation

The world is exactly `128 × 128` tiles, indices `0–127`, northwest origin. Cell `C(cx,cy)` covers `x=cx×16..cx×16+15`, `y=cy×16..cy×16+15`, with `cx,cy=0..7`.

Milestone manifests are exact and cumulative:

- `manifest.world.core`: `landmark.realm_gate`, `landmark.lantern_fork`; no realm event.
- `manifest.world.slice`: Core plus `landmark.sunken_chapel`, `landmark.west_bell_bridge`, `landmark.funeral_orchard`; events `event.ritual_interrupt`, `event.funeral_caravan`.
- `manifest.world.alpha`: Slice plus `landmark.drowned_courtyard`, `landmark.east_bell_bridge`, `landmark.reliquary_causeway`, `landmark.drowned_bell_frame`, `landmark.great_belfry`; events `event.drowned_bell_recovery`, `event.bell_tower_siege`.

M03 uses its two landmarks in a bounded graybox micro-realm. M05 enables the complete cell grid/cycle/climax. No other Early Access realm event exists. Portals, paths, event sites, and event-only Elite variants are child records of the earliest enabled parent that references them and do not increment the landmark/event headline counts.

`world.core_microrealm_01` is an M03-only `48×48` clear/mud map with solid shell, northwest origin, Realm Gate rectangle `x=4..13,y=38..47`, player spawn `(8.5,40.5)`, Lantern Fork safe circle center `(24.5,24.5)` radius5, Bell portal anchor `(40.5,8.5)` radius3, and width5 roads along `(8.5,40.5)->(24.5,40.5)->(24.5,24.5)->(40.5,24.5)->(40.5,8.5)`. Spawn anchors are `(8.5,8.5)`, `(16.5,8.5)`, `(24.5,8.5)`, `(8.5,16.5)`, `(16.5,16.5)`, `(32.5,16.5)`, `(8.5,24.5)`, `(16.5,32.5)`, `(32.5,32.5)`; reject safe/portal/road conflicts, then use the first eight remaining in `(y,x)` order. `(32.5,24.5)` is intentionally not an anchor because it lies on the road. The macro-cell ordinary scheduler is disabled. On first entrant movement beyond1 tile or primary release, wait1 s and spawn one `pack.bell.01` with900 ms warnings. State is `Dormant -> Active -> Cleared`; it never respawns after clear. If all living participants leave while Active, wait5 s, clear/reset the pack to Dormant, and preserve deaths/Recalls. The Bell portal is active only in Cleared. There is no realm cycle, Siege, or retirement.

M04 loads only this exact production-map cell mask: `(0,2),(1,2),(2,2),(0,3),(1,3),(2,3),(0,4),(1,4),(2,4),(0,5),(1,5),(2,5),(0,6),(1,6),(2,6),(0,7),(1,7),(2,7)`. Any door/road crossing to a disabled adjacent cell ends at a visible solid `Veil Gate` with a `CONTENT IN DEVELOPMENT` label and a `4-tile` no-hostile safe strip. Realm cap is eight at M04; M05 removes the mask and uses the full 40-player rules.

Compile in order: place cell template; rotate clockwise by `90° × ((3cx+5cy) mod 4)`; carve permanent roads; apply landmarks; reserve event/portal/spawn/safe volumes; place seeded nonblocking decoration. Decoration never changes collision, slow, road, landmark, event, portal, or spawn geometry.

For local continuous point `(x,y)` in a `16×16` cell, clockwise transforms are `R0=(x,y)`, `R90=(16-y,x)`, `R180=(16-x,16-y)`, `R270=(y,16-x)`, then add cell origin. Whole-tile rectangle `[x,y,w,h]` transforms to: R0 unchanged; R90 `[16-(y+h),x,h,w]`; R180 `[16-(x+w),16-(y+h),w,h]`; R270 `[y,16-(x+w),h,w]`. Door directions rotate with the same turn count. Golden fixtures cover all four rotations for every cell.

Shallow water multiplies ordinary ground velocity by `0.85` for players and nonflying enemies after persistent speed resolution; deep water is solid. It does not affect Rush/Slip/Fold, knockback, projectiles, Toll Crow, Chapel Wisp, or Choir Skull. Road floor overrides water. Multiple terrain effects use the single strongest slow and never stack. Funeral Caravan is explicitly terrain-immune.

### CONT-WORLD-002 — Exact cell grid

| `cy \ cx` | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
|---:|---|---|---|---|---|---|---|---|
| 0 | OR | OG | OR | OG | OR | OG | OR | OR |
| 1 | OG | PC | PC | PB | PC | PC | PB | OG |
| 2 | OR | PC | HM | HM | HM | HM | PC | OR |
| 3 | OG | PB | HM | HR | HB | HM | PC | OG |
| 4 | OR | PC | HM | HB | HB | HM | PB | OR |
| 5 | OG | PC | HM | HR | HM | HM | PC | OR |
| 6 | OR | PB | PC | PC | PB | PC | PC | OG |
| 7 | OG | OG | OR | OG | OR | OG | OR | OG |

Local rectangles are `[x,y,width,height]` whole tiles before rotation.

| Code/template | Band | Exact local terrain/collision |
|---|---|---|
| `OR / cell.outer_reeds_01` | Outer | Walkable mud; shallow water `[0,0,5,5]`, `[11,9,5,7]`; no solid. |
| `OG / cell.outer_grave_road_01` | Outer | Walkable; solid graves `[2,2,2,3]`, `[12,3,2,2]`, `[3,12,3,2]`, `[11,11,3,3]`. |
| `PC / cell.parish_courtyard_01` | Parish | Walkable; ruins `[2,2,3,5]`, `[11,9,3,5]`, `[7,1,2,3]`. |
| `PB / cell.parish_bridge_01` | Parish | Deep-water solid `[0,0,16,5]`, `[0,11,16,5]`; bridge local `y=5..10`. |
| `HM / cell.heart_mire_01` | Heart | Deep-water solids `[0,0,5,6]`, `[11,10,5,6]`; shallow `[10,0,6,4]`, `[0,12,7,4]`. |
| `HR / cell.heart_roots_01` | Heart | Root solids `[2,2,3,7]`, `[11,7,3,7]`, `[6,12,4,2]`. |
| `HB / cell.heart_belfry_01` | Heart | Stone; solid `2×2` pillars with southwest corners `(3,3)`, `(11,3)`, `(3,11)`, `(11,11)`. |

Bands are `O*=Outer Reeds 1–7`, `P*=Sunken Parish 6–14`, `H*=Bellmarsh Heart 12–20`.

### CONT-WORLD-002A — Ordinary spawn anchors and packs

Each cell begins with nine local candidate centers sorted row-major: `(4,4),(8,4),(12,4),(4,8),(8,8),(12,8),(4,12),(8,12),(12,12)`. Rotate/translate with CONT-WORLD-001, then reject collision, deep water, landmark/event/portal reservation, road distance `<3`, safe volume, or current player distance `<6`. One live entity occupies one anchor. If a pack lacks enough compatible anchors, skip that pack for the spawn cycle.

| Pack | Band | Members / spawn budget |
|---|---|---|
| `world.outer.01` | Outer | 6 Drowned Pilgrims + 1 Mudbound / 12 |
| `world.outer.02` | Outer | 4 Mire Leeches + 1 Toll Crow + 1 Sepulcher Knight / 12 |
| `world.outer.03` | Outer | 6 Drowned Pilgrims + 2 Bell Reeds / 12 |
| `world.outer.04` | Outer | 4 Mire Leeches + 2 Drowned Pilgrims + 1 Mudbound / 12 |
| `world.parish.01` | Parish | 6 Drowned Pilgrims + 2 Bell Acolytes / 12 |
| `world.parish.02` | Parish | 6 Drowned Pilgrims + 1 Chain Sentry / 12 |
| `world.parish.03` | Parish | 2 Drowned Pilgrims + 2 Bell Acolytes + 1 Choir Skull / 12 |
| `world.parish.04` | Parish | 2 Root Thralls + 1 Chapel Wisp + 1 Mudbound / 12 |
| `world.heart.01` | Heart | 6 Root Thralls + 2 Maskfruit / 12 |
| `world.heart.02` | Heart | 4 Root Thralls + 1 Bloom Widow + 1 Orchard Cantor / 14 |
| `world.heart.03` | Heart | 6 Brine Husks + 2 Salt Novices / 12 |
| `world.heart.04` | Heart | 2 Brine Husks + 1 Confession Mirror + 1 Tide Mourner / 16 |

World spawn-budget costs are exact by enemy ID: Drowned Pilgrim/Mire Leech/Root Thrall/Brine Husk=`1`; Toll Crow=`2`; Bell Reed/Bell Acolyte/Maskfruit/Salt Novice=`3`; Chapel Wisp/Choir Skull/Bloom Widow/Confession Mirror=`4`; Mudbound/Chain Sentry/Sepulcher Knight/Orchard Cantor=`6`; Tide Mourner=`10`. Every written world-pack budget MUST equal its member-cost sum; adding a world-pack member without an explicit cost is a compile error.

Every `10 s`, target ordinary active-pack count is `clamp(ceil(active_players/4),1,10)`. If below target, choose the least recently used enabled cell containing or adjacent to a living player, tie `(cy,cx)`, then choose uniformly from compatible band packs sorted by ID using stream `(realm_seed, ordinary_spawn_ordinal)`. Spawn the full pack with `900 ms` ground warnings. A cleared/reset pack puts its cell on `45 s` cooldown. Packs grant ordinary/Elite personal rewards by each member's role/location profile; the pack itself adds no reward.

A stage may select only a pack whose every member ID is enabled in `manifest.encounters.<stage>`. Core micro-realm uses only `pack.bell.01`. Slice world packs are exactly `world.outer.01`, `.03`, `.04`; `world.parish.01`–`.04`; and `world.heart.01`, `.02`. Alpha enables all twelve. Filter before the uniform pack draw; an empty legal set leaves the cell unused for that spawn evaluation and emits `world_pack_unavailable`.

### CONT-WORLD-003 — Roads

Carve walkable, non-slow floor for tile centers within `2.5` of each axis-aligned centerline; a further `1.0` shoulder forbids solid props. Hostile anchors are at least `3.0` away unless event-attached.

| ID | Ordered points |
|---|---|
| `path.outer_grave_loop` | `(8.5,120.5)->(8.5,8.5)->(120.5,8.5)->(120.5,120.5)->(8.5,120.5)` |
| `path.parish_loop` | `(24.5,103.5)->(24.5,24.5)->(103.5,24.5)->(103.5,103.5)->(24.5,103.5)` |
| `path.heart_loop` | `(40.5,87.5)->(40.5,40.5)->(87.5,40.5)->(87.5,87.5)->(40.5,87.5)` |
| `path.west_inward` | `(8.5,103.5)->(24.5,103.5)->(24.5,72.5)->(40.5,72.5)->(40.5,64.5)->(52.5,64.5)` |
| `path.east_inward` | `(120.5,103.5)->(103.5,103.5)->(103.5,72.5)->(87.5,72.5)->(87.5,64.5)->(75.5,64.5)` |
| `path.north_causeway` | `(64.5,8.5)->(64.5,24.5)->(64.5,40.5)->(64.5,52.5)` |
| `path.south_approach` | `(64.5,120.5)->(64.5,103.5)->(64.5,87.5)->(64.5,76.5)` |
| `path.belfry_cross` | `(40.5,64.5)->(52.5,64.5)->(75.5,64.5)->(87.5,64.5)` |

West/east inward are independent mandatory Outer-to-Heart routes. No event closes both.

### CONT-WORLD-004 — Landmarks and portals

| Landmark | Anchor | Exact override |
|---|---:|---|
| `landmark.realm_gate` | `(8.5,120.5)` | Walkable `x=4..13,y=116..125`; spawn `(8.5,117.5)`; safe radius `6`. |
| `landmark.lantern_fork` | `(40.5,103.5)` | Walkable radius `5`; cardinal lanterns; no hostile anchor radius `6`. |
| `landmark.sunken_chapel` | `(24.5,40.5)` | Courtyard `x=18..31,y=35..46`; north wall opening at `x=24.5`. |
| `landmark.drowned_courtyard` | `(103.5,40.5)` | Courtyard `x=97..110,y=35..46`; shallow corners; dry radius `3`. |
| `landmark.west_bell_bridge` | `(24.5,72.5)` | Walkable `x=20..29,y=64..81`; no slow. |
| `landmark.east_bell_bridge` | `(103.5,72.5)` | Walkable `x=99..108,y=64..81`; no slow. |
| `landmark.reliquary_causeway` | `(64.5,24.5)` | Stone `x=61..68,y=8..40`; surrounding `3` tiles deep water except roads. |
| `landmark.funeral_orchard` | `(40.5,87.5)` | Walkable `x=34..47,y=81..94`; six fixed nonblocking perimeter trees. |
| `landmark.drowned_bell_frame` | `(88.5,88.5)` | Walkable radius `10`; deposit radius `1`, nonsolid. |
| `landmark.great_belfry` | `(64.5,64.5)` | Authored `x=52..76,y=36..92`; siege geometry below. |

Portal anchors reserve walkable radius `3`; hostiles cannot enter radius `4` while active. Ordinary dungeon event portals last `90 s`; `portal.bell_warden.climax` remains active until the realm leaves Climax.

| Portal ID | Coordinate/use |
|---|---|
| `portal.bell_sepulcher.west_ritual` | `(24.5,44.5)`, west Ritual |
| `portal.bell_sepulcher.east_ritual` | `(103.5,44.5)`, east Ritual |
| `portal.root_chapel.south_caravan` | `(44.5,87.5)`, south Caravan reward and Slice Root discovery |
| `portal.bell_sepulcher.north_caravan` | `(79.5,40.5)`, north Caravan reward |
| `portal.root_chapel.east` | `(103.5,56.5)`, east progress |
| `portal.drowned_reliquary.causeway` | `(64.5,24.5)`, Tier III |
| `portal.siege.bell_sepulcher` | `(60.5,40.5)`, Siege discovered portal |
| `portal.siege.root_chapel` | `(64.5,40.5)`, Siege discovered portal |
| `portal.siege.drowned_reliquary` | `(68.5,40.5)`, Siege discovered portal |
| `portal.bell_warden.climax` | `(64.5,37.5)`, Warden encounter transfer |

Inactive/locked anchors are ordinary floor with no interactable.

For an account eligible for an active portal, the server commits `dungeon_discovered.<dungeon_id>` and updates `last_world_portal_seen_active_realm_seconds` when the character first comes within20 tiles while the portal is in interest/visible state; line-of-sight is not required. A successful portal transfer also performs both writes. The 15-minute Hall-contract suppression counts only accumulated seconds while that account has a living character in a realm, pauses in Hall/dungeons/offline, and compares against this stored active-realm clock. Retry by portal-visibility event ID cannot double-write discovery.

### CONT-WORLD-005 — Realm event anchors

**Ritual Interrupt:** west sigils `(18.5,50.5)`, `(24.5,62.5)`, `(34.5,52.5)` use west portal; east `(93.5,50.5)`, `(103.5,62.5)`, `(110.5,48.5)` use east portal. Each reserves radius `3`. Defender anchors are sigil plus `(3,0),(0,3),(-3,0),(0,-3)`, filled clockwise from east.

**Funeral Caravan:** south path is `52 tiles`; north path is `56 tiles`; constant speed is `path_length/150 tiles/s`, stop exactly at ambush, resume `2 s` after clear, never displaced.

| Site | Path | Ambush anchors | Optional shrine anchors |
|---|---|---|---|
| `site.caravan_south` | `(16.5,111.5)->(32.5,111.5)->(32.5,95.5)->(44.5,95.5)->(44.5,87.5)` | `(28.5,111.5)`,`(32.5,95.5)`,`(44.5,95.5)` | `(28.5,107.5)`,`(36.5,91.5)`,`(44.5,99.5)` |
| `site.caravan_north` | `(111.5,16.5)->(95.5,16.5)->(95.5,32.5)->(79.5,32.5)->(79.5,40.5)` | `(99.5,16.5)`,`(95.5,32.5)`,`(83.5,32.5)` | `(99.5,20.5)`,`(91.5,36.5)`,`(83.5,28.5)` |

Each ambush has groups `5 tiles` left/right of travel, three anchors/group spaced `1.5` perpendicular.

**Drowned Bell Recovery:** `site.drowned_bell_southeast`; frame `(88.5,88.5)`; fragments `(80.5,84.5)`, `(96.5,84.5)`, `(84.5,96.5)`, `(92.5,96.5)`; defenders `(80.5,88.5)`, `(96.5,88.5)`, `(88.5,80.5)`, `(88.5,96.5)`, `(82.5,82.5)`, `(94.5,82.5)`, `(82.5,94.5)`, `(94.5,94.5)`; event square `(80.5,80.5)` through `(96.5,96.5)`; successful Tier II progress uses east Root portal.

### CONT-WORLD-006 — Bell Tower Siege geometry

| Floor | Walkable bounds | Activation | Objective/spawn anchors |
|---|---|---:|---|
| 1 | `x=54..74,y=77..89` | `(64.5,90.5)` | `(57.5,80.5)`,`(64.5,80.5)`,`(71.5,80.5)`,`(57.5,86.5)`,`(64.5,86.5)`,`(71.5,86.5)` |
| 2 | `x=54..74,y=59..71` | `(64.5,73.5)` | Anchors `(58.5,65.5)`,`(70.5,65.5)`; lane origins `(64.5,60.5)`,`(64.5,70.5)` |
| 3 | `x=54..74,y=41..53` | `(64.5,55.5)` | Herald `(64.5,47.5)`; auxiliaries `(57.5,44.5)`,`(71.5,44.5)`,`(57.5,50.5)`,`(71.5,50.5)` |

Safe stair corridors: Floor1→2 `x=62..67,y=72..76`; Floor2→3 `x=62..67,y=54..58`; Floor3→portal `x=62..67,y=36..40`. Gates stay solid until prior completion; then clear hostile entities/projectiles before opening. Warden arena is a separate `22×18` public encounter cell.

### CONT-WORLD-007 — Executable realm-event profiles

All event enemy health uses locked GDD event scaling. Event spawn-budget costs are Drowned Pilgrim/Root Thrall/Brine Husk=`1`; Bell Reed/Bell Acolyte/Maskfruit/Salt Novice=`3`; Chapel Wisp/Choir Skull/Confession Mirror=`4`; Mudbound/Chain Sentry=`6`. An `EventPack` has the explicit base budget `b` written below. Compute `spawn_multiplier=min(2.25,1+0.45×sqrt(N_locked-1))` and `q(b)=2` iff `round_half_up(b×spawn_multiplier)≥2b`, otherwise `q(b)=1`. Run the authored EventPack once; when `q(b)=2`, repeat that entire EventPack exactly once after it clears, using the same internal wave order and delays. Any residual budget is unused, and no EventPack repeats more than once. Objective vulnerability/progress waits for its repeated copy to clear. The copy uses ordinary member rewards and is not a separate event completion reward. Damage, speed, and gaps do not scale. Personal eligibility uses SOC-010. Ritual is the one Early Access minor event: success grants one success credit,10 Ash,120 XP, and `reward.event_minor`. Caravan and Drowned Bell are major events: each success grants one success credit,25 Ash,300 XP, and `reward.event_major`. Failure grants no completion reward/XP and adds one pressure credit.

**`event.ritual_interrupt`:** at each activated sigil, Defender EventPack `ritual.defender` has base budget12: Wave A is four Drowned Pilgrims; after clear and `1 s`, Wave B is two Bell Reeds plus two Pilgrims. When `q(12)=2`, repeat the complete A→B sequence after `1 s`. Only after every required Defender copy clears does the GDD three-second interrupt become legal. Breaking a sigil queues Pressure EventPack `ritual.pressure` after `750 ms`; its base budget8 roster is two Bell Acolytes plus two Pilgrims, and it repeats once after `1 s` only when `q(8)=2`. After all three pressure packs clear and all three sigils break, spawn `elite.bell_acolyte_ritual` at the middle sigil. Its death completes the event. The matching site Ritual portal opens with probability `10000/10000` for `90 s`.

`elite.bell_acolyte_ritual` has `release_stage=slice`, tags `[event_variant,elite,no_normal_count]`, base HP600, armor6, default collision/hurtbox, Elite health scaling, move3.0/maintain6, and `reward.elite_parish`. Its sole pattern is `pattern.elite.bell_acolyte_ritual.alternating_fan`: cycle1800, warn400 first/300 repeated, alternating offsets `[-50,-35,-20,-5,10]` then `[-10,5,20,35,50]` degrees, speed6, range9, radius0.11, veil Pressure damage16, threat7, `echo_memory_family=fan_projectile`, counterplay `strafe`, disposition `consume_on_player_or_solid`. Its derived cues/assets use its own ID. It grants no additional event-variant roll beyond `reward.elite_parish`; the separate event completion reward resolves only after its committed death.

**`event.funeral_caravan`:** caravan radius1, base HP1500, armor10, no resistance/healing, health `round_half_up(1500×(1+0.40×(N_locked-1)))`; use the site's exact `path_length/150` speed while moving. It pauses at each ambush until every required EventPack copy clears, then waits2 s. Each ambush is a separate base-budget12 EventPack and repeats once after `1 s` when `q(12)=2`: Ambush1 is three Bell Acolytes plus three Pilgrims; Ambush2 is one Chapel Wisp, one Mudbound, and two Pilgrims; Ambush3 is one Chain Sentry plus two Bell Acolytes. Every ordinary member is Slice-enabled. Enemies target the nearest player within5 tiles, else caravan; tie player entity ID. Hostile projectiles/contact can hit the caravan through the ordinary pipeline. At each stop, any eligible player may activate the optional shrine with a `1 s` hold before the ordinary pack clears; it spawns `miniboss.sepulcher_knight` under Elite health scaling with its standard miniboss reward suppressed and `xp_profile=xp.realm_elite:60`, then grants one extra `rarity.elite_outer` equipment roll to eligible players on its death. Maximum one shrine activation/stop. Route completion with HP>0 grants completion rewards. At≥50% remaining HP, south opens `portal.root_chapel.south_caravan`; north opens `portal.bell_sepulcher.north_caravan`; duration90 s. Destruction ends the event with ordinary enemy drops only.

**`event.drowned_bell_recovery`:** on activation run one base-budget12 EventPack of five Root Thralls, one Maskfruit, and one Chapel Wisp across sorted defender anchors; repeat it after `1 s` when `q(12)=2`. Each deposited fragment queues one base-budget12 EventPack of two Maskfruit, two Root Thralls, and one Chapel Wisp; only one deposit pack or its repeat is active, and the next queued pack starts1 s after the previous EventPack fully clears. Carry/deposit/timer rules remain GDD WRLD-012. Completion sets realm `root_portal_progress=1`; threshold is exactly1, so atomically open `portal.root_chapel.east` for90 s and reset progress to0. Completion also sets the `drowned_bell` bit in `tier3_progress_bits`.

Ritual success sets `ritual`; Caravan success sets `caravan`. When all three `ritual|caravan|drowned_bell` bits are set during RisingPressure or later, open `portal.drowned_reliquary.causeway` for90 s and clear all three bits after successful portal creation. Failed portal allocation preserves bits and retries every10 s. Bits are realm-local and reset at retirement.

**`event.bell_tower_siege`:** one `600 s` event timer covers all floors. `event_elapsed=0` is the BellSiege state-entry tick; the allocation at +5 s, approach announcement, activation crossing, floor transitions, and process restart do not reset it. Floor1 Wave A is a base-budget12 EventPack of three Bell Acolytes plus three Pilgrims on the six anchors; after all required copies clear, Wave B begins2 s later as a separate base-budget12 EventPack of two Bell Reeds, one Choir Skull, and two Pilgrims. Floor2 is a base-budget12 EventPack of two `enemy.chain_sentry` at its Anchor coordinates; their perpendicular lane patterns originate from their positions and use the listed room-scale line origins only for VFX extension. Each of these EventPacks repeats once after `1 s` when `q(12)=2`. Floor3 spawns world-event Elite `elite.belfry_herald`. Herald target duration is60–120 s and reward is part of the Siege chest, not a separate item roll.

`elite.belfry_herald` has `release_stage=alpha`, tags `[event_variant,event_elite,no_normal_count,no_major_boss_count,no_separate_reward,no_xp]`, base HP6500, armor12, collision radius0.70, hurtbox radius0.62, boss health scaling with event-activation `N_locked=clamp(living_eligible,1,20)` that never rescales, move3.0/maintain6, and a3 s invulnerable/nonattacking intro. It has no contact damage and no reward profile.

- `pattern.elite.belfry_herald.gap_ring`: telegraph start every3500 ms; first warn650 and repeated warn500; snapshot the current target direction and omit the nearest three adjacent indices from a 16-index ring, tie lower clockwise index; speed5, range12, radius0.13, veil Pressure damage22, threat13, memory `radial_projectile`, counterplay `follow_gap`, disposition `consume_on_player_or_solid`, distinct bell warning audio.
- `pattern.elite.belfry_herald.personal_lanes`: telegraph start every7000 ms and consumes a coincident/due gap ring. Choose the next two distinct living slots from a persistent rotating immutable-participant cursor and advance the cursor by two. Snapshot each target at start; each line runs from Herald through that point to arena collision. If two width1 lanes overlap anywhere within4 tiles of the Herald or their centerline angle differs by less than30 degrees, scan the remaining distinct living slots cyclically for a legal second target; after one full scan with none, emit only the first lane. With one living target, also emit one lane. Warn900, active400, veil Major damage32 once/player/cast, threat12/lane and maximum24/cast, memory `lane_or_beam`, counterplay `leave_telegraph`, disposition `expire_at_authored_end`, Major audio.
- `pattern.elite.belfry_herald.summon`: at intro completion and every12000 ms, spawn two Bell Acolytes at lowest free auxiliary-anchor IDs, maximum four living; skip unavailable spawns. Adds have ordinary parish rewards suppressed and tags `[summoned,no_reward,no_on_kill]`.

At or below25% initial maximum health, starts scheduled after the threshold-crossing tick use intervals3150/6300/10800 ms; an already scheduled start is not moved. No new attack appears. Equal-time priority is lane, summon, ring. The content compiler reserves combined lane/ring/add threat, and cancels the lower-priority ring rather than postponing it.

Floor1/2/3 must complete by event elapsed `180/360/600 s`; missing a boundary fails immediately, clears Siege hostiles, and transitions to Climax without bonus. Siege is a major event. Success clears hostiles, grants each eligible player300 XP, `reward.event_major` plus one extra `rarity.major_event` equipment roll and `25 Ash`, opens `portal.bell_warden.climax`, and makes each `portal.siege.*` visible only to accounts that discovered that dungeon. Dungeon portals use90 s and retry-safe allocation; the Warden portal follows the Climax-state lifetime in CONT-WORLD-008.

### CONT-WORLD-008 — Executable realm director

The authoritative director evaluates on every server tick divisible by150 (`5 s`). It does nothing with zero living realm players. On state entry, ordinary event allocation begins after10 s. `target_slots=clamp(1+floor(active_players/8),1,5)`; in the Alpha cycle BellSiege reserves one slot for Siege and Climax/Aftermath/Retiring target zero ordinary slots. A terminal event frees its slot immediately, but replacement waits until the next evaluation.

Stage-enabled sites are exact: Slice has `ritual_interrupt@west` and `funeral_caravan@south`; Alpha adds Ritual east, Caravan north, Drowned Bell southeast, and Siege at Great Belfry. A non-Siege site is eligible only when it is inactive, its site cooldown and type cooldown expired, its geometry is enabled, and a living eligible player is within20 tiles. Successful site cooldown is180 s; failed/abandoned cooldown is90 s; event-type cooldown is60 s. Exclude the last event type used in the same west/east/north/south/heart zone when another legal type exists; if exclusion empties the zone pool, permit it rather than leaving a permanent vacancy.

Candidate weight by state is:

| Event/site candidate | Gathering | RisingPressure | BellSiege nonreserved slots (Alpha+) |
|---|---:|---:|---:|
| Ritual west or east | 30 | 20 | 10 |
| Caravan south or north | 20 | 20 | 10 |
| Drowned Bell southeast | 0 (ineligible) | 40 | 20 |

Sort eligible `(event_id,site_id)` pairs by UTF-8 tuple and select with the CONT-012 weighted draw using stream `(realm_seed,"event-director-v1",allocation_ordinal)`. Increment ordinal for every nonempty selection attempt and persist the result before spawn. If no candidate exists, leave the slot empty and retry next evaluation. Spawn begins5 s after allocation with a realm announcement and map marker. Lock `N_locked=clamp(living eligible players within20 tiles at activation,1,20)`; departures never rescale health.

Successful Ritual, Caravan, or Drowned Bell adds one `success_credit`; terminal failure/abandon adds one `pressure_credit`. `cycle_credit=success_credit+pressure_credit`; both counters are nonnegative and realm-local. Credits accelerate the cycle but only success flags unlock portals/rewards. The Slice/M04 state machine is exactly `Booting -> Gathering -> RisingPressure -> Aftermath -> Retiring`: Gathering enters RisingPressure at `cycle_credit≥3` or its15-minute timer; RisingPressure enters Aftermath at `cycle_credit≥7` or35 minutes since boot; Aftermath lasts5 minutes; Retiring warns for2 minutes, performs successful automatic extraction for remaining living characters, then shuts down. Slice never enters BellSiege or Climax and cannot schedule Siege or Warden content. The Alpha-and-later state machine is `Booting -> Gathering -> RisingPressure -> BellSiege -> Climax -> Aftermath -> Retiring`, with the GDD state durations and the same Gathering/RisingPressure thresholds. Any state transition cancels unstarted allocations but lets active ordinary events finish until their own terminal condition, with no new replacement.

In the Alpha-and-later cycle only, BellSiege entry sets `event_elapsed=0`, allocates `event.bell_tower_siege` exactly5 s later regardless of player proximity, and announces a60 s approach window; neither allocation nor Floor1 activation resets the timer. Floor1 activates when the first eligible player crosses its activation boundary. If nobody activates by the end of the10-minute BellSiege state, Siege fails with `not_started`. Siege success enters Climax after its reward/portal transaction; failure or the state deadline enters Climax without bonus. Climax creates/keeps the Warden portal active immediately through the state's8-minute end and retries failed portal service/instance allocation every10 s. The first committed Warden defeat/reward transaction atomically closes the portal and enters Aftermath; if two cells defeat on one tick, ascending instance ID obtains the realm transition lock first, while every qualified defeat still resolves its own rewards. Every cell already `Assembling` or `BossActive` continues independently to defeat, abandonment, or its own10-minute deadline measured from Phase1 start; its rewards remain valid and its exit returns Hall if the source realm has retired. No new cell allocates after portal close. Realm absolute timers use server boot monotonic ticks and survive process restart from the persisted state start.

### CONT-WORLD-009 — Event contribution references

At event activation persist `regular_scale=1+0.25×(N_locked-1)`, `elite_scale=1+0.75×(N_locked-1)`, and `boss_scale=1+0.72×(N_locked-1)`. Let `q12=q(12)` and `q8=q(8)` under CONT-WORLD-007. `round-scaled(...)` means apply `regular_scale` and `round_half_up` to each listed enemy instance independently, then sum; never round one aggregate. The exact `encounter_contribution_reference_health` is:

```text
Ritual = 3 * (q12 * round-scaled(4*85 + 2*140 + 2*85)
              + q8 * round-scaled(2*160 + 2*85))
         + round_half_up(600 * elite_scale)

Caravan = q12 * round-scaled(3*160 + 3*85
                             + 110 + 360 + 2*85
                             + 380 + 2*160)
          + round_half_up(1500 * (1 + 0.40*(N_locked-1)))

Drowned Bell = q12 * round-scaled(5*105 + 150 + 110
                                  + 4*(2*150 + 2*105 + 110))
               + round_half_up(1000 * regular_scale)

Siege = q12 * round-scaled(3*160 + 3*85
                           + 2*140 + 150 + 2*85
                           + 2*380)
        + round_half_up(6500 * boss_scale)
```

Required EventPack repetitions are already represented by `q12`/`q8`. Optional Caravan shrine enemies, Herald summons, and reset-only enemies are excluded from the reference denominator, but actual health damage/support against eligible event-tagged entities still adds contribution units.

Objective actions use SOC-010's `2% of reference` and two-credit/player cap:

- Ritual: the account whose3 s Interact completion commits for a sigil; simultaneous completion ties earliest channel-start tick then binary account ID.
- Caravan: at Ambush2 clear and route completion, grant one action to each living participant who accumulated at least10 s inside the moving Lantern Aura since the prior checkpoint.
- Drowned Bell: the carrier whose1 s deposit completion commits for each fragment.
- Siege: no separate objective-unit action; damage, healing, and prevention are the contribution paths.

Presence begins at the event's first hostile/objective activation and ends at terminal commit; authored noncombat gate waits are excluded. The reference, presence ticks, objective credits, and contribution totals are stored with the reward resolution so retry cannot change eligibility.

---

## 7. Rooms and dungeon layouts

### CONT-ROOM-001 — Coordinate contract

Declared size is walkable interior with northwest local origin; engine adds a one-tile solid shell. Doors lie on `x=0`, `x=width`, `y=0`, or `y=height`, opening width `3`. Clockwise rotation transforms all fields; reflection is prohibited. Unspecified floor is walkable. `F/P/D/A/E` mean Fodder/Pressure/Disruptor/Anchor/Elite-compatible. Optional hazard sockets are inert unless selected. Spawns warn ≥`750 ms` and cannot be within `3 tiles` of an open entry door.

Room manifests are cumulative: `manifest.rooms.core` is all nine Bell rows; `manifest.rooms.slice` adds all eleven Root rows for 20; `manifest.rooms.alpha` adds all fourteen Drowned rows for 34. Arena rows count because they require distinct collision, hazard, spawn, art, and validation assets.

### CONT-ROOM-002 — Bell Sepulcher templates

| ID / size / doors | Exact collision and sockets | Exact spawn/utility anchors |
|---|---|---|
| `room.bell.vestibule_01` `13×11`; W/E `(0,5.5)/(13,5.5)` | none | Safe `(3,5.5)`; exit `(10,5.5)` |
| `room.bell.cross_01` `17×17`; all midpoints | Solid `2×2` centered `(5,5),(12,5),(5,12),(12,12)`; lanes `(2,8.5)->(15,8.5)`, `(8.5,2)->(8.5,15)`, width1 | F `(3,3),(8.5,3),(14,3),(3,14),(8.5,14),(14,14)`; P `(3,8.5),(14,8.5)`; A `(8.5,8.5)` |
| `room.bell.nave_01` `15×21`; N/S `(7.5,0)/(7.5,21)` | Pews `[3,6,3,1],[9,6,3,1],[3,14,3,1],[9,14,3,1]`; ring `(7.5,10.5)` r3 | F `(3,4),(7.5,4),(12,4),(3,17),(7.5,17),(12,17)`; P/D `(3,10.5),(12,10.5)`; D/A `(7.5,10.5)` |
| `room.bell.bridge_01` `23×11`; W/E mid | Deep water `y=0..2,9..10`; bridge `y=3..8`; lanes `x=7.5,15.5`, width1 | F `(4,4),(8,7),(12,4),(16,7),(19,4),(20,7)`; P on lanes; A `(11.5,5.5)` |
| `room.bell.choir_01` `19×15`; all midpoints | Pillars center `(5,4),(14,4),(5,11),(14,11)`, size1.5; rotor center | F `(3,3),(9.5,3),(16,3),(3,12),(9.5,12),(16,12)`; P/D `(4,7.5),(15,7.5)`; A center |
| `room.bell.knight_01` `19×15`; W/E mid | Charge lanes `x=2..17`, y `3.5/7.5/11.5`, width1.2 | `miniboss.sepulcher_knight (13.5,7.5)`; stage `(3,7.5)`; adds `(9.5,3.5),(9.5,11.5)` |
| `room.bell.rest_01` `15×13`; W/E mid | no hazards/hostiles | Shrine `(7.5,4.5)`; stabilization `(7.5,8.5)` |
| `room.bell.secret_01` `11×11`; S `(5.5,11)` | ring center `(5.5,5.5)` r3 | D/P center; chest `(5.5,2.5)` |
| `arena.boss.caldus_01` `18×18`; W `(0,9)` | Walkable circle center `(9,9)` radius8; charge endpoints `(1,9),(17,9),(9,1),(9,17)` | Boss `(9,9)`; stage `(2.5,9)`; group `(2.5,6),(2.5,12)` |

### CONT-ROOM-003 — Root Chapel templates

| ID / size / doors | Exact collision and sockets | Exact spawn/utility anchors |
|---|---|---|
| `room.root.vestibule_01` `13×11`; W/E mid | none | Safe `(3,5.5)`; exit `(10,5.5)` |
| `room.root.harvest_rows_01` `20×16`; W/E mid | Lanes x`2..18`, y`2.5/6/10/13.5`, width1 | F `(4,4),(10,4),(16,4),(4,12),(10,12),(16,12)`; P/D `(6,8),(14,8)`; A center |
| `room.root.courtyard_01` `19×19`; all mid | Planters `2×2` centered `(5,5),(14,5),(5,14),(14,14)`; blooms `(5,9.5),(9.5,5),(14,9.5),(9.5,14)` r1.25 | F corners/mid x `3,9.5,16` at y`3,16`; P/D `(3,9.5),(16,9.5)`; A center |
| `room.root.split_hedge_01` `21×15`; W/E mid, N `(10.5,0)` | Hedge x`10..11`, y`1..5,10..14`; passage y7.5; web lanes y4/11, x2..19 | F `(4,3),(8,5),(13,5),(17,3),(4,12),(8,10),(13,10),(17,12)`; P/D `(5,7.5),(8,7.5),(13,7.5),(16,7.5)`; A `(10.5,7.5)` |
| `room.root.canopy_01` `17×21`; N/S mid | Root circles r1.2 `(4,7),(13,7),(4,14),(13,14)`; blooms `(8.5,6),(8.5,10.5),(8.5,15)` | F `(3,4),(8.5,4),(14,4),(3,17),(8.5,17),(14,17)`; P/D `(4,10.5),(13,10.5)`; A center |
| `room.root.rotunda_01` `21×21`; all mid | Center planter r2; walkable ring r3..8; cardinal radial lanes | F `(10.5,3),(18,10.5),(10.5,18),(3,10.5)`; P/D `(5,5),(16,5),(5,16),(16,16)` |
| `room.root.gardener_01` `19×15`; W/E mid | Blooms `(6,4),(13,4),(6,11),(13,11)` r1.1 | Gardener `(13,7.5)`; stage `(3,7.5)` |
| `room.root.rootmother_01` `20×16`; W/E mid | Four root lanes as harvest rows | Rootmother `(14,8)`; adds `(5,4),(10,4),(5,12),(10,12)` |
| `room.root.rest_01` `15×13`; W/E mid | no hazards/hostiles | Shrine `(7.5,4.5)`; stabilization `(7.5,8.5)` |
| `room.root.secret_01` `13×13`; S `(6.5,13)` | blooms `(4,4),(9,4),(4,9),(9,9)` | challenge `(6.5,6.5)`; chest `(6.5,2.5)` |
| `arena.boss.veyr_01` `18×14`; W `(0,7)` | Rows y`2,5.33,8.67,12`, width1; lateral corridors | Boss `(13,7)`; stage `(2.5,7)`; adds `(5,3),(9,3),(5,11),(9,11)` |

### CONT-ROOM-004 — Drowned Reliquary templates

| ID / size / doors | Exact collision and sockets | Exact spawn/utility anchors |
|---|---|---|
| `room.drowned.vestibule_01` `15×11`; W/E mid | none | Safe `(3,5.5)`; exit `(12,5.5)` |
| `room.drowned.dry_platforms_01` `21×17`; W/E mid | Shallow except dry r2 `(5,4.5),(16,4.5),(5,12.5),(16,12.5)` plus width3 corridors | F at dry centers and `(10.5,4.5),(10.5,12.5)`; P/D `(5,8.5),(16,8.5)`; A/E center |
| `room.drowned.needle_hall_01` `25×13`; W/E mid | Lanes y`3/6.5/10`, width0.9; pillars `(8,3),(17,10)` r1 | F `(5,3),(12.5,3),(20,3),(5,10),(12.5,10),(20,10)`; P `(8,6.5),(12.5,6.5),(17,6.5)`; D `(12.5,6.5)` |
| `room.drowned.tide_cross_01` `21×21`; all mid | Dry cross x`8..13` or y`8..13`; else shallow; push center | F `(4,4),(10.5,4),(17,4),(4,17),(10.5,17),(17,17),(7,10.5),(14,10.5)`; P/D `(4,10.5),(17,10.5)`; A/E center |
| `room.drowned.mirror_gallery_01` `19×19`; all mid | Mirrors `[5,4,1,4],[13,4,1,4],[5,11,1,4],[13,11,1,4]`; opposing lanes | F x`3,9.5,16`, y`3,16`; D `(5.5,9.5),(13.5,9.5)`; P `(9.5,6),(9.5,13)` |
| `room.drowned.causeway_01` `25×11`; W/E mid | Dry y`3..8`; deep water y`0..2,9..10`; push x`8,17` | F `(4,4),(8,7),(12.5,4),(17,7),(21,4),(21,7)`; P `(8,5.5),(12.5,5.5),(17,5.5)`; D `(12.5,5.5)` |
| `room.drowned.confession_court_01` `21×17`; W/E mid, N `(10.5,0)` | Dry r1.5 `(5,4.5),(16,4.5),(5,12.5),(16,12.5)`; lanes H1 `(1,4)->(20,4)`, H2 `(1,8.5)->(20,8.5)`, H3 `(1,13)->(20,13)`, V1 `(5,1)->(5,16)`, V2 `(10.5,1)->(10.5,16)`, V3 `(16,1)->(16,16)` | F `(3,8.5),(7,8.5),(14,8.5),(18,8.5)`; P `(10.5,4),(10.5,13)`; D `(5,4.5),(16,12.5)`; A/E center |
| `room.drowned.brine_islands_01` `23×17`; W/E mid | Dry r2 `(4,4),(11.5,4),(19,4),(7.5,12.5),(15.5,12.5)`; else shallow | F dry centers plus `(11.5,12.5)`; P/D `(7.5,8.5),(15.5,8.5)`; A/E center |
| `room.drowned.mourner_01` `21×17`; W/E mid | Dry anchors as court; push center | Mourner `(15,8.5)`; stage `(3,8.5)` |
| `room.drowned.salt_mirror_01` `21×17`; W/E mid | Horizontal lanes y`4,8.5,13`; vertical x`5,10.5,16`, width1 | Salt Mirror `(15,8.5)`; stage `(3,8.5)` |
| `room.drowned.tide_shrine_01` `15×15`; S mid | Push center; dry circles r1.5 at `(7.5,3.5),(11.5,7.5),(7.5,11.5),(3.5,7.5)`; lanes H `(1,7.5)->(14,7.5)`, V `(7.5,1)->(7.5,14)` | F at four dry centers; P `(10.5,5),(4.5,5)`; D `(7.5,7.5)`; reward `(7.5,12.5)` |
| `room.drowned.rest_01` `15×13`; W/E mid | no hazards/hostiles | Shrine `(7.5,4.5)`; stabilization `(7.5,8.5)` |
| `room.drowned.secret_01` `13×13`; N mid | Lanes x`3,6.5,10`; dry reward `(6.5,10)` | D center; chest `(6.5,10)` |
| `arena.boss.salt_confessor_01` `20×20`; S `(10,20)` | Walkable circle center `(10,10)` r9; dry r1.25 `(10,3),(17,10),(10,17),(3,10)` | Boss center; stage `(10,18)` |

### CONT-ROOM-005 — Authored fallback graphs

Fallback ignores seed and uses exact nodes. Graph grid is logical only; each edge joins the listed rotated doors with a straight, walkable, non-slow corridor exactly `4 tiles` long and `3 tiles` wide, then places the destination room without overlap. Adjacent horizontal main nodes connect `E→W`. Rotation is clockwise. One live spawned entity may occupy each spawn anchor. “Branch rooms” in GDD DNG-004 excludes separately rolled Secret rooms; Bell/Root/Drowned have 1/1/2 ordinary branch rooms respectively. Participant scaling still applies; no substitutions or modifier sockets are random.

#### `layout.bell_sepulcher.fallback_01`

| Node/grid/rot | Template | Encounter |
|---|---|---|
| `B0 (0,0) 0` | `room.bell.vestibule_01` | none |
| `B1 (1,0) 0` | `room.bell.cross_01` | 6 Pilgrims, 2 Bell Reeds |
| `B2 (2,0) 90` | `room.bell.nave_01` | 6 Pilgrims, 2 Bell Acolytes, 1 Choir Skull; budget16 |
| `B3 (3,0) 0` | `room.bell.knight_01` | Sepulcher Knight |
| `B4 (4,0) 0` | `room.bell.rest_01` | rest/Bargain |
| `B5 (5,0) 0` | `room.bell.bridge_01` | 6 Pilgrims, 1 Chain Sentry |
| `B6 (6,0) 0` | `arena.boss.caldus_01` | Sir Caldus |
| `BB1 (1,-1) 0` | `room.bell.choir_01` | `miniboss.choir_abbot` |
| `BS1 (1,-2) 0` | `room.bell.secret_01` | `encounter.secret.bell_01` |

Edges add `B1.N→BB1.S`, `BB1.N→BS1.S`.

#### `layout.root_chapel.fallback_01`

| Node/grid/rot | Template | Encounter |
|---|---|---|
| `R0 (0,0) 0` | `room.root.vestibule_01` | none |
| `R1 (1,0) 0` | `room.root.harvest_rows_01` | 6 Root Thralls, 2 Maskfruit |
| `R2 (2,0) 0` | `room.root.courtyard_01` | 5 Root Thralls, 1 Maskfruit, 1 Chapel Wisp |
| `R3 (3,0) 0` | `room.root.split_hedge_01` | 2 Root Thralls, 2 Maskfruit, 2 Chapel Wisps |
| `R4 (4,0) 0` | `room.root.gardener_01` | Masked Gardener |
| `R5 (5,0) 0` | `room.root.rest_01` | rest/Bargain |
| `R6 (6,0) 90` | `room.root.canopy_01` | 6 Root Thralls, 1 Orchard Cantor |
| `R7 (7,0) 0` | `room.root.rootmother_01` | Rootmother |
| `R8 (8,0) 0` | `arena.boss.veyr_01` | Mother Veyr |
| `RB1 (2,-1) 0` | `room.root.rotunda_01` | 4 Root Thralls, 2 Maskfruit, 1 Bloom Widow; budget14 |
| `RS1 (3,-1) 0` | `room.root.secret_01` | `encounter.secret.root_01` |

Edges add `R2.N→RB1.S`, `R3.N→RS1.S`.

#### `layout.drowned_reliquary.fallback_01`

| Node/grid/rot | Template | Encounter |
|---|---|---|
| `D0 (0,0) 0` | `room.drowned.vestibule_01` | none |
| `D1 (1,0) 0` | `room.drowned.dry_platforms_01` | 6 Brine Husks, 2 Salt Novices |
| `D2 (2,0) 0` | `room.drowned.needle_hall_01` | 3 Brine Husks, 3 Salt Novices |
| `D3 (3,0) 0` | `room.drowned.mourner_01` | Tide Mourner |
| `D4 (4,0) 0` | `room.drowned.tide_cross_01` | 8 Brine Husks, 1 Confession Mirror |
| `D5 (5,0) 0` | `room.drowned.mirror_gallery_01` | 4 Brine Husks, 2 Confession Mirrors |
| `D6 (6,0) 0` | `room.drowned.rest_01` | rest/Bargain |
| `D7 (7,0) 0` | `room.drowned.causeway_01` | 3 Brine Husks, 3 Salt Novices |
| `D8 (8,0) 0` | `room.drowned.salt_mirror_01` | Salt Mirror |
| `D9 (9,0) 0` | `room.drowned.confession_court_01` | 2 Brine Husks, 2 Salt Novices, 1 Mirror |
| `D10 (10,0) 0` | `room.drowned.brine_islands_01` | 6 Brine Husks, 2 Salt Novices |
| `D11 (11,0) 90` | `arena.boss.salt_confessor_01` | Salt Confessor |
| `DB1 (4,-1) 0` | `room.drowned.tide_shrine_01` | 4 Brine Husks, 2 Salt Novices, 1 Confession Mirror; budget14 |
| `DB2 (9,-1) 90` | `room.drowned.dry_platforms_01` | 6 Brine Husks, 2 Salt Novices; budget12 |
| `DS1 (5,1) 0` | `room.drowned.secret_01` | `encounter.secret.drowned_01` |

Edges add `D4.N→DB1.S`, `D9.N→DB2.S`, and `D5.S→DS1.N`.

### CONT-ROOM-006 — Seeded generation profiles

The server supplies immutable `dungeon_seed:u64` and promoted `content_version:string`. Attempt `a=0..9` initializes the layout stream from `BLAKE3(ASCII("dungeon-layout-v1\0") || little_endian_u32(len(UTF8(dungeon_id))) || UTF8(dungeon_id) || little_endian_u32(len(UTF8(content_version))) || UTF8(content_version) || little_endian_u64(dungeon_seed) || little_endian_u32(a))`; use the ADR-001 RNG over that 32-byte digest. Lengths are byte lengths. Never add `a` to `dungeon_seed`. Reward RNG is a different stream.

| Profile | Main combat count | Ordinary branch count | Regular-template weights | Required special placement |
|---|---:|---:|---|---|
| `generation.bell_sepulcher.v1` | uniform 4–5 | 1 | cross40, nave35, bridge25 | Knight main at 30–55%; Choir Abbot one-room branch; rest at 55–75% |
| `generation.root_chapel.v1` | uniform 6–7 | uniform 1–2 | harvest20, courtyard20, split20, canopy20, rotunda20 | Gardener at 25–45%; Rootmother at 70–85%; rest at 55–75% |
| `generation.drowned_reliquary.v1` | uniform 8–9 | 2 | dry14, needle14, tide-cross14, mirror14, causeway12, confession12, brine12, tide-shrine8 | Mourner at 25–45%; Salt Mirror at 70–85%; rest at 55–75% |

Main combat slots are integer indices `0..combat_count-1`. For each required main-path placement in the exact order below, convert its inclusive percentage range `[low,high]` to `lo=ceil(low×(combat_count-1)/100)` and `hi=floor(high×(combat_count-1)/100)`, build the ascending list of still-free integer indices in `[lo,hi]`, and draw one uniformly using the attempt layout stream. An empty list fails the attempt; no clamping or nearest-free fallback is legal. A miniboss replaces the regular template at its selected combat slot. A rest marker inserts the rest room immediately after its selected combat slot, does not replace or increment `combat_count`, and cannot share a marker with a miniboss. Placement order is Bell `Knight, rest`; Root `Gardener, Rootmother, rest`; Drowned `Mourner, Salt Mirror, rest`.

Main graph is a chain. After main placements, branch attachments use distinct nonentrance/nonboss main nodes, candidate indices sorted ascending before uniform draws; every ordinary branch is one combat room, and Bell's required Choir Abbot occupies its authored branch. Secret is separate and is processed after ordinary branches: roll Tier I/II/III `20%/30%/40%` after Candleless adjustment, then attach the dungeon's matching Secret template and CONT-ROOM-008 encounter to a uniformly selected room with a free compatible door. Failure to attach means the generation attempt fails; maximum one Secret.

For each unreserved regular slot, build templates with a compatible incoming/outgoing door and not equal to the preceding main template; sort by ID, apply the integer weights above, draw, then choose uniformly among compatible rotations sorted `0,90,180,270`. Branch rotation follows the same rule. Reflection is illegal. Connect all doors with the exact four-tile corridors from CONT-ROOM-005.

Regular encounter packs are selected uniformly from compatible rows after sorting pack ID. Spawn every member simultaneously using one distinct compatible anchor, assigning sorted enemy IDs to sorted `(y,x)` anchors after a `900 ms` group telegraph.

| Pack | Exact members / budget |
|---|---|
| `pack.bell.01` | 6 Drowned Pilgrims + 2 Bell Reeds / 12 |
| `pack.bell.02` | 6 Drowned Pilgrims + 2 Bell Acolytes / 12 |
| `pack.bell.03` | 6 Drowned Pilgrims + 1 Chain Sentry / 12 |
| `pack.bell.04` | 2 Drowned Pilgrims + 2 Bell Reeds + 1 Choir Skull / 12 |
| `pack.root.01` | 6 Root Thralls + 2 Maskfruit / 12 |
| `pack.root.02` | 5 Root Thralls + 1 Maskfruit + 1 Chapel Wisp / 12 |
| `pack.root.03` | 4 Root Thralls + 1 Bloom Widow + 1 Orchard Cantor / 14 |
| `pack.root.04` | 2 Maskfruit + 2 Chapel Wisps / 14 |
| `pack.drowned.01` | 6 Brine Husks + 2 Salt Novices / 12 |
| `pack.drowned.02` | 5 Brine Husks + 1 Salt Novice + 1 Confession Mirror / 12 |
| `pack.drowned.03` | 2 Brine Husks + 1 Tide Mourner / 12 |
| `pack.drowned.04` | 4 Brine Husks + 2 Salt Novices + 1 Confession Mirror / 14 |

Dungeon spawn-budget costs are exact by enemy ID: Drowned Pilgrim/Root Thrall/Brine Husk=`1`; Bell Reed/Bell Acolyte/Maskfruit/Salt Novice=`3`; Chapel Wisp/Choir Skull/Bloom Widow/Confession Mirror=`4`; Chain Sentry/Orchard Cantor=`6`; Tide Mourner=`10`. A dungeon pack row's written budget MUST equal the sum; a future dungeon-pack ID requires an explicit cost.

For a regular dungeon room, lock `N=1..8` at room activation and compute:

```text
target_budget = round_half_up(base_budget
  * min(2.25, 1 + 0.45*sqrt(N-1)))
remaining = target_budget - base_budget
```

Wave0 is the exact authored roster. Build `unit_cycle` by expanding its counts and sorting units by descending spawn cost, then enemy ID, then occurrence ordinal. While `remaining` can pay at least one cycle member, scan cyclically from the cursor for the first member whose cost fits, append it to the expansion roster, subtract cost, and advance the cursor to the slot after that member; stop when no member fits after one full scan. Split expansion into waves of at most the Wave0 entity count, preserving roster order. Reuse the room's compatible anchors after the prior wave clears; start each expansion wave after1 s with900 ms ground warnings. Room completion/reward waits for all waves. An entity whose template role is `Elite` uses `elite_health`; every other normal-enemy role uses `regular_enemy_health`. Expansion copies retain the source member's role, reward profile, and XP binding; damage never scales. The validator stores target/actual budget for every pack and N and requires `0≤target-actual<minimum_member_cost`.

If a selected template has no compatible pack, that template is removed and the template draw repeats from remaining weights; if none remains, fail the attempt. Base independent hazard budget is exactly `0` in Early Access; authored sockets are activated only by their bound encounter or modifier. Reward nodes are only required miniboss, rest/Bargain, Secret, and boss rewards—no random chest insertion.

Modifier draw: Tier I has `50%` none and `50%` one uniform enabled legal modifier; Tier II exactly one uniform enabled modifier; Tier III `50%` one and `50%` one legal pair. Sort IDs/pairs before drawing. Validate the complete graph, encounter anchors, modifier-reserved threat, reachability, entry/exit, and room objectives. After ten failures, load CONT-ROOM-005 fallback with a separately drawn legal modifier set and emit the failed seed/reasons.

### CONT-ROOM-007 — M03 fixed private-life layout

`layout.core_private_life_01` is the Bell fallback main chain `B0→B1→B2→B3→B4→B5→B6` with branches disabled. “Six-room dungeon” means four combat rooms `B1/B2/B3/B5`, one rest room `B4`, and one boss room `B6`; the safe vestibule `B0` is not counted. M03 always uses this layout and `rarity.core_fixed`; seeded generation first becomes player-facing in M04.

### CONT-ROOM-008 — Secret encounters and rewards

Secret encounters are deterministic two-wave locked-room encounters. The first living player crossing the activation boundary locks `N=1..8`; the ordinary DNG-005 eight-second join window and reset rules apply. Each wave uses its written base budget and the exact CONT-ROOM-006 expansion algorithm independently, including role-specific health scaling, expansion order, `1 s` inter-wave delay, and `900 ms` ground warnings. Unless a row pins a member to one coordinate, assign enemy instances sorted by `(enemy_id,occurrence_ordinal)` to anchors sorted by `(y,x)`. Secret completion occurs only after all required base and expansion enemies in both waves are dead. The presence clock begins at Wave A's warning start and ends at the terminal reward commit; a reset discards that attempt's presence/contribution state.

| Encounter / release | Wave A exact roster, anchors, budget | Wave B exact roster, anchors, budget | One personal completion grant |
|---|---|---|---|
| `encounter.secret.bell_01` / Slice | 2 Bell Reeds + 2 Drowned Pilgrims at `(3,3),(8,3),(3,8),(8,8)` / 8 | 1 Chain Sentry at `(5.5,5.5)` / 6 | one equipment roll using `rarity.elite_outer`; `material.bell_brass ×1` |
| `encounter.secret.root_01` / Slice | 4 Root Thralls at `(3,6.5),(10,6.5),(6.5,3),(6.5,10)` / 4 | 2 Maskfruit at `(4,4),(9,9)` + 1 Chapel Wisp at `(6.5,6.5)` / 10 | one equipment roll using `rarity.elite_parish`; `material.funeral_root ×1` |
| `encounter.secret.drowned_01` / Alpha | 2 Salt Novices + 4 Brine Husks assigned by sorted enemy ID to `(3,3),(6.5,3),(10,3),(3,9),(6.5,9),(10,9)` / 10 | 1 Confession Mirror at `(6.5,6.5)` + 2 Brine Husks at `(3,6.5),(10,6.5)` / 6 | one equipment roll using `rarity.elite_heart`; `material.saltglass_shard ×1` |

The personal completion grant is resolved exactly once per living, present participant who satisfies SOC-010 session/anti-cheat/inactivity requirements, was present for at least `50%` of secret active ticks, and contributed damage plus credited support equal to at least `0.5%` of the sum of every required hostile's separately rounded scaled maximum health. There are no objective-action credits. Items/materials enter the run backpack/pouch as AtRiskPending, and death or Recall before the reward commit makes that participant ineligible. Reward retry reuses one `(instance_id,secret_encounter_id,account_id)` resolution ID.

---

## 8. Enemy and encounter records

### CONT-ENEMY-001 — Shared AI state contract

All 18 enemies use `SpawnTelegraph(900) -> Acquire -> Move/Position -> Telegraph -> Attack -> Recover -> Acquire`. Acquire chooses nearest living, damageable player within aggro radius; tie lowest entity ID. Aim/position locks at telegraph start. If no legal target exists for `5 s`, reset to spawn, restore health, clear hostile output, and grant no reward. Aggro/leash are `12/16 tiles` unless listed. Group health uses GDD scaling; damage, speed, count, timing, gaps, and status never scale.

“Cycle” is measured telegraph-start to next telegraph-start. Projectiles end on player/solid collision unless `pierce` is named. Every state/attack below is required data, not animation-driven logic.

Milestone manifests are exact:

- `manifest.encounters.core`: normal enemies `drowned_pilgrim`, `mire_leech`, `bell_reed`, `bell_acolyte`, `chain_sentry`, `choir_skull`; minibosses `sepulcher_knight`, `choir_abbot`; boss `sir_caldus`; all use their full prefixed IDs.
- `manifest.encounters.slice`: Core plus normal `chapel_wisp`, `mudbound`, `root_thrall`, `maskfruit`, `bloom_widow`, `orchard_cantor`; minibosses `masked_gardener`, `rootmother`; boss `mother_veyr`.
- `manifest.encounters.alpha`: Slice plus normal `toll_crow`, `sepulcher_knight`, `brine_husk`, `salt_novice`, `confession_mirror`, `tide_mourner`; minibosses `tide_mourner`, `salt_mirror`; bosses `salt_confessor`, `bell_warden`.

Thus cumulative counts are `6/2/1`, `12/4/2`, and `18/6/4`. Enemy and miniboss namespaces are distinct even where the readable suffix matches.

### CONT-ENEMY-002 — Exact catalog

| ID | Role; HP/armor; movement | Exact attack cycle |
|---|---|---|
| `enemy.drowned_pilgrim` | Fodder; `85/0`; approach `2.2`, stop at5 | Every `2.2 s`: warn `300 ms`; fan offsets `-15/0/+15°`, speed5.5, range7, r0.12, physical Chip `10`, threat3, memory `fan_projectile`. |
| `enemy.mire_leech` | Fodder; `70/0`; approach/rush `3.0`, retreat `3.5` for `1.5 s` | At distance≤2.5 warn `400 ms` first/`300 ms` repeated, charge fixed `2 tiles/500 ms`, physical Pressure `12` once; then retreat; cycle `2.5 s`, threat2, memory `charge_or_contact`. |
| `enemy.toll_crow` | Pressure; `120/1`; orbit radius4 at3.2 | Every `4 s`: lane width0.8, length6 through target, warn `700 ms`; sweep along lane speed7, physical Pressure `22` once; reset to orbit `1 s`, threat8, memory `charge_or_contact`. |
| `enemy.bell_reed` | Pressure; `140/2`; stationary | Every `3 s`: warn `450 ms` first/`300` later; 8 indices, omit adjacent pair, emitted6; gap start0 then `+3 mod8`; speed4.5, range9, r0.13, veil Pressure `12`, threat6, memory `radial_projectile`. |
| `enemy.chapel_wisp` | Disruptor; `110/0`; orbit nearest Anchor radius2 at3.0, otherwise target radius5 | Every `5 s`: snapshot target point, circle r1 warn `800 ms`; pool `2.5 s`, tick veil Chip `6` each500 ms, Frostbind `1.0 s`, one tick/player/500 ms, threat8, memory `damage_over_time_or_status`. |
| `enemy.mudbound` | Anchor; `360/8`; approach `1.2`, stop5 | Frontal `120°` facing aim reduces player direct projectiles `70%`; every `3 s` warn `400 ms`, fan5 over60°, speed5, range8, physical Pressure `15`; shield turns toward target only outside windup/attack; rear is unguarded; threat8, memory `fan_projectile`. |
| `enemy.bell_acolyte` | Pressure; `160/2`; maintain6 at3.0 | Every `1.8 s`, warn `400 ms` first/`300 ms` repeated; alternate offsets `-50,-35,-20,-5,+10°` then `-10,+5,+20,+35,+50°`; speed6, range9, veil Pressure `16`, r0.11, threat7, memory `fan_projectile`. |
| `enemy.chain_sentry` | Anchor; `380/6`; stationary | Every `4.5 s`, warn `900 ms`; first axes0/90°, next45/135°, alternate; width0.9, extends to room collision, active350 ms, physical Major `28` once/cast, threat12, memory `lane_or_beam`. |
| `enemy.sepulcher_knight` | Elite; `950/8`; pursue `2.4`, reset after charge | Every `6 s`: lane width1.0/length5 warn900, charge `550 ms`, physical Major `34` once; stop emits 10-shot ring with two target-opposite adjacent gaps, speed5, physical Pressure damage20. Between charges fan5/50° every2.2 s, speed6, physical Pressure damage18. Threat16; memories charge/radial/fan by lethal pattern. |
| `enemy.choir_skull` | Disruptor; `150/1`; orbit anchor radius3 at2.8 | Rotor cycle `6 s`: warn650 first/500 repeated; for `4 s`, two opposite arms rotate clockwise `35°/s`, each emits every400 ms, speed4.5, range7, veil Pressure `14`; then quiet2 s; threat10, memory `rotating_projectile`. |
| `enemy.root_thrall` | Fodder; `105/1`; zigzag toward5 at2.3, lateral sign flips each1 s | Every `2.6 s`: warn400 first/300 repeated, orb speed3.5/range6/r0.16, nature Pressure `14`; on enemy/solid/range, endpoint warns350 then four cardinal nature Chip fragments speed4/range3/damage8/r0.10; threat4, memory by lethal orb/fragment pattern. |
| `enemy.maskfruit` | Pressure; `150/2`; retreat inside4, approach outside7, speed3 | Every `4.5 s`: target circle r1 warn800; impact nature Chip `10`, pool `3 s`, tick `6` each500 ms, Hex2 s; one tick/player/500 ms; max two pools/entity, threat8, memory `ground_zone`. |
| `enemy.bloom_widow` | Disruptor; `190/3`; circle target radius5 at3.2 | Every `5 s`: two parallel width0.8 lanes through target, centers offset ±1.2 perpendicular, warn700, active500; nature Chip `12` once/lane and Frostbind1.25 s; player max one lane hit/cast, threat10, memory `lane_or_beam`. |
| `enemy.orchard_cantor` | Anchor; `340/5`; remain within5 of most injured ally at2 | Every `8 s`: channel `3 s`, pulse at1/2/3 s heals other nonboss enemies within5 for `6%` their max each; interrupt when actual health damage during channel reaches `8%` Cantor max; interrupted channel heals no further; no self-heal; threat6, memory `summon_add`. |
| `enemy.brine_husk` | Fodder; `125/3`; approach6 at1.9 | Every `2.4 s`: warn350; three wide shots offsets `-20/0/+20°`, speed4, range8, brine Chip `14`, r0.15, threat4, memory `fan_projectile`. |
| `enemy.salt_novice` | Pressure; `175/4`; maintain7 at2.8 | Every `2.2 s`: lane warn450; aimed Needle speed10/range11/r0.09, salt Pressure `20`; second identical fixed-aim Needle `400 ms` later; threat8, memory `aimed_projectile`. |
| `enemy.confession_mirror` | Disruptor; `240/7`; stationary | Every `5 s`: select authored lane whose centerline is nearest target, tie lane ID; warn900, active350, salt Pressure `30` once; never reflects player shots; threat10, memory `lane_or_beam`. |
| `enemy.tide_mourner` | Elite; `1200/12`; move among authored dry anchors at2.4, tie anchor ID | Every `7 s`: radial warn1 s; push1.2 tiles plus brine Pressure `22`; `800 ms` later fixed five-shot fan/32°, speed10, salt Pressure `30`; between waves aimed Needle every1.8 s, damage24; threat18, memories environmental/fan/aimed by lethal pattern. |

Damage type identifiers used above are `physical`, `veil`, `nature`, `brine`, and `salt`; absent player resistance treats each as zero resistance. Designers cannot add a type without UI icon, log string, resistance mapping, and death-recap test.

XP is source-context data, not an enemy-template roll. Capture `xp_profile` at spawn: Outer/Bell=`xp.normal_t1:5`, Parish/Root=`xp.normal_t2:10`, Heart/Drowned=`xp.normal_t3:15`; a row tagged Elite in a realm uses `xp.realm_elite:60`; miniboss IDs bind `xp.miniboss_t1/t2/t3:120/220/350`; Caldus/Veyr/Confessor/Warden bind `450/800/1200/1500`. Event completion uses GDD `120/300` minor/major once, independent of enemy XP. Eligibility/radius/first-clear rounding is GDD PROG-003. `no_reward` implies XP0. The expanded spawn/reward record stores the selected profile so movement or retry cannot change it.

### CONT-ENEMY-003 — Miniboss and boss binding

The six miniboss state machines, values, and exact kits are canonical GDD ENC-014 records and bind to these rooms/rewards:

| ID | Room | Reward profile |
|---|---|---|
| `miniboss.sepulcher_knight` | `room.bell.knight_01` | `reward.miniboss_t1` |
| `miniboss.choir_abbot` | `room.bell.choir_01` authored replacement | `reward.miniboss_t1` |
| `miniboss.masked_gardener` | `room.root.gardener_01` | `reward.miniboss_t2` |
| `miniboss.rootmother` | `room.root.rootmother_01` | `reward.miniboss_t2` |
| `miniboss.tide_mourner` | `room.drowned.mourner_01` | `reward.miniboss_t3` |
| `miniboss.salt_mirror` | `room.drowned.salt_mirror_01` | `reward.miniboss_t3` |

Major bindings are `boss.sir_caldus -> arena.boss.caldus_01 -> reward.boss_caldus`, `boss.mother_veyr -> arena.boss.veyr_01 -> reward.boss_veyr`, `boss.salt_confessor -> arena.boss.salt_confessor_01 -> reward.boss_confessor`, and `boss.bell_warden -> public belfry 22×18 -> reward.world_warden`. Their complete timelines are GDD ENC-010 through ENC-013; content records MUST transcribe them field-for-field and golden tests compare every authored attack tick.

### CONT-PATTERN-001 — Normal-enemy completion records

CONT-ENEMY-002 plus CONT-013 fully defines ordinary single-pattern rows. These exact records complete every multi-component row; no component inherits an unstated damage type, band, warning, threat, disposition, or memory family:

| Pattern ID | Exact payload and geometry | Warning/counterplay | Threat, memory, disposition |
|---|---|---|---|
| `pattern.enemy.sepulcher_knight.charge_lane` | physical Major34; width1, length5, fixed charge over550 ms, one contact hit | 900 ms; leave lane | 8; `charge_or_contact`; `one_contact_hit_per_cast` |
| `pattern.enemy.sepulcher_knight.stop_ring` | physical Pressure20; 10-index ring, omit target-opposite adjacent2, speed5, range8, r0.12 | parent-only release at charge end; parent lane warning and opposite gap are its cue; never schedulable alone | 8; `radial_projectile`; `consume_on_player_or_solid` |
| `pattern.enemy.sepulcher_knight.shield_fan` | physical Pressure18; five/50°, speed6, range8, r0.12 | 400 ms first/300 repeated; strafe | 5; `fan_projectile`; `consume_on_player_or_solid` |
| `pattern.enemy.choir_skull.rotor` | veil Pressure14; two opposite arms, 35°/s, emit every400 ms for4 s, speed4.5, range7, r0.12 | 650 ms first/500 repeated arm preview; move with rotation | 10; `rotating_projectile`; `consume_on_player_or_solid` |
| `pattern.enemy.root_thrall.orb` | nature Pressure14; speed3.5, range6, r0.16 | 400 ms first/300 repeated; strafe | 1; `aimed_projectile`; `consume_on_player_or_solid` |
| `pattern.enemy.root_thrall.fragment_bloom` | nature Chip8; four cardinal shots, speed4, range3, r0.10 | child-only; enemy/solid/range endpoint displays350 ms flower pulse before release; leave pulse | 4; `radial_projectile`; `consume_on_player_or_solid` |
| `pattern.enemy.salt_novice.needle_pair` | salt Pressure20 each; two identical speed10/range11/r0.09 needles at release0/+400 ms, one fixed aim | one450 ms lane warning remains visible through second release; leave lane | 8 total; `aimed_projectile`; `consume_on_player_or_solid` |
| `pattern.enemy.tide_mourner.tide_push` | brine Pressure22; radial displacement1.2 plus one hit | 1000 ms ring; move perpendicular/use dry anchor | 8; `environmental`; `expire_at_authored_end` |
| `pattern.enemy.tide_mourner.needle_fan` | salt Major30; five/32°, speed10, range12, r0.09; release800 ms after Push impact | emitter fan preview from+400 to+800 ms after Push; strafe | 5; `fan_projectile`; `consume_on_player_or_solid` |
| `pattern.enemy.tide_mourner.aimed_needle` | salt Pressure24; speed10, range12, r0.09 | 400 ms first/300 repeated line; strafe | 1; `aimed_projectile`; `consume_on_player_or_solid` |

`enemy.orchard_cantor`'s healing channel is nonhostile: it has `raw_damage=0`, no damage band, no killer/death-memory family, `threat_cost=6` only for room allocation, and disposition `interruptible_channel`. It can never be persisted as a lethal pattern ID.

### CONT-PATTERN-002 — Miniboss golden metadata

All miniboss patterns use their GDD cadence and these complete records. Summoned adds use their ordinary attack records but tags `[summoned,no_reward,no_on_kill]` where the owning kit says no reward.

Each miniboss spawns with900 ms ground warning, contact damage0 except its authored charge, default resistance0, phase projectile policy `cancel_on_reset`, and Elite health scaling locked at room activation. Target nearest living room participant, tie entity ID; personal/rotating mechanics instead use immutable participant-slot order. With no living participant inside for the DNG-005 reset window, clear it/projectiles/adds and restore the authored initial state. Exact locomotion is:

| Miniboss | Collision / hurtbox | Locomotion |
|---|---:|---|
| Sepulcher Knight | `0.55 / 0.48` | Pursue at2.4 until distance3.5; stop to attack; charge endpoint becomes new home for next Acquire. |
| Choir Abbot | `0.55 / 0.48` | Stationary at room center; rotate facing only for presentation. |
| Masked Gardener | `0.50 / 0.43` | Maintain6 tiles at2.6: retreat inside4.5, approach outside7, strafe clockwise between. |
| Rootmother | `0.65 / 0.58` | Stationary on authored anchor; summons use lowest free add anchor `(y,x)`. |
| Tide Mourner | `0.60 / 0.52` | Move among authored dry anchors at2.4, selecting farthest from nearest player, tie anchor ID; stop while warning/attacking. |
| Salt Mirror | `0.60 / 0.52` | Stationary on authored anchor; facing is presentation-only. |

| Pattern ID | Exact payload and geometry | Warning/counterplay | Threat, memory, disposition |
|---|---|---|---|
| `miniboss.sepulcher_knight.charge_lane` | physical Major34; width1, length5, charge550 ms | 900 ms; leave lane | 8; `charge_or_contact`; `one_contact_hit_per_cast` |
| `miniboss.sepulcher_knight.stop_ring` | physical Pressure20; 10 indices omit target-opposite adjacent2, speed5/range8/r0.12 | parent charge warning; never standalone; follow opposite gap | 8; `radial_projectile`; `consume_on_player_or_solid` |
| `miniboss.sepulcher_knight.shield_fan` | physical Pressure18; five/50°, speed6/range8/r0.12 | 400/300 ms; strafe | 5; `fan_projectile`; `consume_on_player_or_solid` |
| `miniboss.choir_abbot.rotor` | veil Pressure18; opposite arms, 35°/s, two shots every350 ms for3.5 s, speed4.5/range7/r0.12 | 650/500 ms arm preview; move with rotor | 12; `rotating_projectile`; `consume_on_player_or_solid` |
| `miniboss.choir_abbot.recovery_ring` | veil Major26; 16 indices omit target-facing adjacent4, speed4.5/range8/r0.12 | final650 ms of recovery is gap preview with Major bell audio; follow gap | 12; `radial_projectile`; `consume_on_player_or_solid` |
| `miniboss.masked_gardener.targeted_bloom` | nature Major28 and Hex3 s once/player/bloom; r1.1 persists6 s with no tick damage | 800 ms circle; leave circle | 4 each, max8; `ground_zone`; `expire_at_authored_end` |
| `miniboss.masked_gardener.seed_fan` | nature Pressure20; seven/90°, speed5/range8/r0.12 | 400/300 ms; strafe | 7; `fan_projectile`; `consume_on_player_or_solid` |
| `miniboss.rootmother.root_lanes` | nature Major32 + Frostbind1.5 s; choose two authored rows, active600 ms, once/cast | 800 ms; leave rows | 12/lane, max24; `lane_or_beam`; `expire_at_authored_end` |
| `miniboss.rootmother.fan` | nature Pressure18; five/60°, speed5/range8/r0.12 | 400/300 ms; strafe | 5; `fan_projectile`; `consume_on_player_or_solid` |
| `miniboss.rootmother.summon` | two `enemy.root_thrall`, max4 living; no direct damage | 900 ms spawn circles; destroy/priority | 8/cast; `summon_add`; `expire_with_add` |
| `miniboss.tide_mourner.tide_push` | brine Pressure22; displacement1.2 plus one hit | 1000 ms; use dry anchor/move perpendicular | 8; `environmental`; `expire_at_authored_end` |
| `miniboss.tide_mourner.needle_fan` | salt Major30; five/32°, speed10/range12/r0.09, release800 ms after Push | emitter fan preview+400–800 after Push; strafe | 5; `fan_projectile`; `consume_on_player_or_solid` |
| `miniboss.tide_mourner.aimed_needle` | salt Pressure24; speed10/range12/r0.09 | 400/300 ms; strafe | 1; `aimed_projectile`; `consume_on_player_or_solid` |
| `miniboss.salt_mirror.memory_lanes` | salt Major34; selected authored width1 lanes active400 ms once/step | preview each500 ms, wait700, then activate at600 ms intervals; leave previewed lane | 12/step; `lane_or_beam`; `expire_at_authored_end` |
| `miniboss.salt_mirror.gap_ring` | salt Pressure24; eight indices omit target-facing adjacent2, emit6, speed5/range8/r0.12 | 650/500 ms; follow gap | 6; `radial_projectile`; `consume_on_player_or_solid` |

### CONT-PATTERN-003 — Major-boss golden metadata

Pattern IDs below are the only boss-damage IDs in Early Access. Pattern warnings start at scheduler timestamps unless identified as a child release/impact. `cancel_on_phase_change=true` for all.

| Pattern ID | Exact payload and geometry | Warning/counterplay | Threat, memory, disposition |
|---|---|---|---|
| `boss.caldus.shield_arc` | physical Major24; five/60°, speed7/range17.5/r0.12 | 650 ms; strafe | 5/fan; `fan_projectile`; `consume_on_player_or_solid` |
| `boss.caldus.bell_ring` | veil Major32; 18 indices omit adjacent3, speed5/range20/r0.13 | 800 ms + Major bell; follow gap | 15; `radial_projectile`; `consume_on_player_or_solid` |
| `boss.caldus.charge_lane` | physical Severe48; width1.2, travel6.5 over550 ms, one hit | 1000 ms + Severe charge; leave lane | 18; `charge_or_contact`; `one_contact_hit_per_cast` |
| `boss.caldus.charge_stop_ring` | physical Major28; 14 indices omit the two adjacent indices centered opposite charge, speed5/range18/r0.13 | child release at charge end; parent lane and opposite-gap marker are warning; never standalone | 12; `radial_projectile`; `consume_on_player_or_solid` |
| `boss.veyr.root_rows` | nature Major34 + Frostbind1.5 s; selected two full width1 rows active1.2 s once/cast | 800 ms + root audio; leave rows | 12/row, max24; `lane_or_beam`; `expire_at_authored_end` |
| `boss.veyr.seed_fan` | nature Pressure20; seven/90°, speed5/range9/r0.12 | 400/300 ms; strafe | 7; `fan_projectile`; `consume_on_player_or_solid` |
| `boss.veyr.seed_bloom_fragment` | nature Chip10; every third fan endpoint shows400 ms flower pulse then emits four cardinal speed4.5/range4/r0.10 fragments | endpoint pulse; leave bloom | 4; `radial_projectile`; `consume_on_player_or_solid` |
| `boss.veyr.rotating_harvest` | nature Major34 + Frostbind1.5 s; all three unsafe rows active500 ms once/step | 650 ms per step + root audio; follow safe row | 24/step; `environmental`; `expire_at_authored_end` |
| `boss.veyr.poison_bloom` | nature Major30 + Hex3 s; r1.25 impact once, no residue | 900 ms personal circle + Major audio; leave circle | 4/target, max12; `ground_zone`; `expire_on_impact` |
| `boss.confessor.crystal_needles` | salt Major28; three at -8/0/+8°, speed10/range12/r0.09 | 650/500 ms line fan + Major audio; strafe | 3; `aimed_projectile`; `consume_on_player_or_solid` |
| `boss.confessor.tide_push` | brine Pressure18 + radial displacement1.5; one hit | 1000 ms ring; occupy dry anchor for displacement immunity | 18; `environmental`; `expire_at_authored_end` |
| `boss.confessor.confession_ring` | salt Pressure24; six evenly spaced speed5/range6/r0.12 shots from marked player; ignore source permanently | personal lane warns1000 ms, emit at+1200 with distinct confession audio; leave lane then gap | 6/mark, max18; `radial_projectile`; `consume_on_player_or_solid` |
| `boss.confessor.memory_needles` | salt Major28; each volley uses 24 indices and omits deduplicated safe-anchor gaps, speed10/range12/r0.09 | three500 ms anchor previews +700 ms wait; stay at previewed anchors | 18/volley, max36 active; `environmental`; `consume_on_player_or_solid` |
| `boss.warden.warden_ring` | veil Pressure24; 24 indices omit adjacent4, speed4.5/range18/r0.13 | 650/500 ms + distinct Warden bell; follow gap | 20; `radial_projectile`; `consume_on_player_or_solid` |
| `boss.warden.belfry_lanes` | veil Major38; two authored nonadjacent width1.2 lanes, active800 ms once/cast | 900 ms + Major bell; leave lanes | 12/lane, max24; `lane_or_beam`; `expire_at_authored_end` |
| `boss.warden.cell_anchor_ring` | veil Pressure22; 10 indices omit target-facing adjacent3, speed4/range6/r0.12 | 650 first/500 repeated local gap; follow gap/destroy Anchor | 7/Anchor; `radial_projectile`; `consume_on_player_or_solid` |
| `boss.warden.verdict_lanes` | veil Major40; rotating distinct personal width1 lanes, active400 ms once/cast | 1000 ms each, impacts stagger400 ms + Major audio; leave personal lane | 12/lane, max36; `lane_or_beam`; `expire_at_authored_end` |
| `boss.warden.bell_memory_cells` | veil Major36; three unsafe cells active800 ms at each of three steps, one hit/player/step | ordered safe-cell previews500 ms each +700 ms wait; follow current safe cell | 12/step; `environmental`; `expire_at_authored_end` |

The compiler derives cue IDs under CONT-013, materializes every common field, and fails if a GDD scheduler names an ID outside these tables. The authored `damage_band` is the intended baseline tuning label; every actual hit also computes its per-target COM-003 category after mitigation for telemetry/death recap. Fairness fixtures enumerate legal minimum-health builds and reject a pattern whose warning is shorter than the most severe resolved fixture category or whose damage exceeds the Standard-content limit.

### CONT-BOSS-001 — Scheduler rules

Boss-local time pauses only during authored phase breaks/transitions. A listed time is telegraph start unless the row explicitly labels it `impact`, `emission`, `volley`, or `child release`; then it is that exact event tick and no second implicit telegraph is inserted. At equal timestamps priority is phase transition, movement/arena mechanic, personal Major, radial pattern, aimed pattern, add spawn. “Replace” consumes the lower-priority due action rather than postponing it. Target selection uses living locked participants ordered by immutable party slot; rotating selections retain a cursor and skip ineligible slots. Seeded index ties use lowest index/ID. Phase thresholds/breaks use the GDD and cancel scheduled actions/projectiles.

For Caldus/Veyr/Confessor, the room's listed stage point has a radius3 safe entrance. Pause the scheduler until all connected entrants load or10 s elapse, then run the DNG-006 visible5 s ready countdown. Close the entry door on its completion; living entrants define immutable `N_locked=1..8`. Clear the radius, then run an invulnerable/nonattacking introduction of2500/3000/3000 ms respectively. No hostile exists within3 tiles of stage before Phase1. Recall remains legal. On committed boss reward, clear hostiles and create the stable dungeon-exit interactable at Caldus `(2.5,9)`, Veyr `(2.5,7)`, or Confessor `(10,18)`; it extracts under DTH-011. Allocation/reward retry reuses the same instance/exit ID. Warden uses its more detailed CONT-BOSS-005 entry.

Boss collision/hurtbox radii are Caldus `0.70/0.62`, Veyr `0.75/0.66`, Confessor `0.75/0.66`, Warden `0.80/0.70`; resistance/contact damage are0. Caldus is stationary at center in Phases1/3 and moves only under Phase2/Charge rules. Veyr, Confessor, and Warden remain on their authored boss anchors; knockback/forced player movement cannot move them. Ordinary aimed target is nearest living locked participant, tie immutable slot; explicit rotating/personal rules override. Bosses never leash; zero living locked participants invokes the owning dungeon/public-cell reset, not an AI reset.

Boss-spawned ordinary enemies use `regular_enemy_health` with the boss's `N_locked`, their normal kit/damage, and tags `[boss_add,no_item_reward,no_xp,no_echo]`. Actual damage/support involving them counts contribution, but they are excluded from boss reference health. They may trigger ordinary on-kill effects unless a specific record says `no_on_kill`; chain generation remains capped by the GDD. Reset/phase cleanup destroys them without a kill event.

Soft enrage changes only schedules created after its threshold tick and never moves an already scheduled start. For a loop phase, let `e` be the latest authored damaging emission/activation time in the base loop and factor `f` be the boss's GDD downtime multiplier; `enraged_loop=e+ceil_to_tick((base_loop-e)×f)`. When a low-health loop is also active, use the shorter legal loop but never earlier than one tick after `e`. For independent recurring timers, multiply each next interval by `f` and ceil to a tick. Telegraph, active time, projectile lifetime, speed, damage, count, and geometry never change. Golden safe-path/threat validation covers the enraged schedule.

### CONT-BOSS-002 — `boss.sir_caldus` golden schedulers

Use all ENC-010 projectile/damage/geometry values.

| Phase | Loop and exact pattern starts |
|---|---|
| 1 | `7800 ms`: Shield Arc `0,1800,3600`; Bell Ring `6000` replacing the next Shield. Ring gap starts at index0 and advances `+5 mod18` per cast. |
| 2 | `15000 ms`: Charge Lane `0,7500`; charge locks direction at +700, begins +1000, ends +1550, then Stop Ring immediately. Shield Arc `3000,5200,10500,12700`. Outside charge Caldus moves toward arena center at2 tiles/s and stops within0.25. |
| 3 | `8000 ms`: preview gap A `[0,600]`, B `[600,1200]`, C `[1200,1800]`; wait400; child Ring emissions at2200/3000/3800 use preview order and do not add ordinary800 ms ring telegraphs; Shield Arc telegraph starts6000. Preview gaps are the next three ordinary `+5 mod18` starts. |

At 4–6 players, each Shield pattern start targets slots `cursor` and next eligible at +400 ms; at7–8, three targets at +0/+400/+800. Advance cursor one after the final target. These are separate complete Shield fans and reserve combined threat. Below20% Phase3 loop restarts at7200 instead of8000. Soft-enrage factor is0.85 under CONT-BOSS-001.

### CONT-BOSS-003 — `boss.mother_veyr` golden schedulers

Rows are indexed north-to-south `0..3`. Root-pair sequence is `(0,2),(1,3),(0,1),(2,3)` and persists across phases. Seed Fan bloom counter persists within a phase and resets to zero on transition; every third fan blooms into four cardinal fragments using ENC-011 values.

| Phase | Loop and exact starts |
|---|---|
| 1 | `10000 ms`: Root Rows `0,5000`; Seed Fans `600,2600,4600,6600,8600`. Root warning/activation uses ENC-011; fan aim snapshots on its start. |
| 2 | `12000 ms`: Rotating Harvest warning starts0/1500/3000/4500 and impacts650/2150/3650/5150; safe row at step0=`(cast_index + seed_mod4) mod4`, then advances clockwise each step. Poison Blooms start500/6500, target `min(3,ceil(N/3))` rotating slots, warn900. Seed Fans start1000/3400/5800/8200/10600. |
| 3 | Combat loop `12000 ms`: Root Rows warning starts0, impacts800, and its1.2 s activation ends2000. Rotating Harvest warning starts3000/4500/6000/7500 and impacts3650/5150/6650/8150. No Seed Fan; Root Thrall summoning uses the independent timer below. |

At70% the four Maskfruit use add anchors in `(y,x)` order. Phase2 safe-row cursor increments one after each complete Harvest. In Phase3, the combat-pattern loop remains12000 ms. Root Thrall summoning is an independent timer initialized at phase time0 with a12000 ms interval and maximum four living; below20%, only future summon intervals become10800 ms, while an already scheduled summon is not moved and existing adds do not despawn. Soft-enrage factor is0.90 under CONT-BOSS-001. Poison target points are captured at telegraph start; equal-distance row/cell decisions use lower index.

### CONT-BOSS-004 — `boss.salt_confessor` golden schedulers

Dry anchors are indexed north/east/south/west `0..3` from CONT-ROOM-004. Tide Push warning lasts1000; every player inside dry-anchor radius1.25 at impact ignores push displacement but still takes its18 raw damage.

| Phase | Exact scheduler |
|---|---|
| 1 | Loop `14000 ms`: Tide Push starts0/7000; Crystal Needles start1800/3400/5000/8800/10400/12000. |
| 2 | Independent timers initialize Needle0, Mark2000, Push5000. After execution add1600/8000/9000 ms respectively. Priority Mark > Push > Needle. If a Needle telegraph/impact would occur within500 ms of a Mark-ring or Push impact, consume that Needle and schedule its next ordinary interval. Needle fan rotation alternates `+6°,-6°` added to offsets `-8/0/+8°`. |
| 3 | Loop `16000 ms`: Push starts0 and2000 (impacts1000/3000); recovery3000–7000; Confession Mark starts7000, personal ring emits at8200; Memory Needles preview starts10000 and volleys begin12200/12800/13400; idle to loop. |

Confession Mark target count/slot rotation follows ENC-012. Each marked player shows its lane for1000; at +1200 emits six speed5/range6/r0.12 shots, damage24, orientation=`60°×marked_slot_index + 15°×cast_index`; that ring permanently ignores its source player. Memory Needles chooses safe anchors `[s,(s+1) mod4,(s+3) mod4]`, `s=cast_index mod4`; preview each for500 then wait700. Each volley is a 24-index ring, speed10/r0.09/damage28, omitting the two adjacent indices centered toward each of the three previewed anchors; gap indices are deduplicated. Below20%, Phase3 next loop begins at14080; all actions finish by13400. No Crystal Needles occur in Phase3. Soft-enrage factor is0.88; Phase2's three independent intervals become ceiling-to-tick `1600/8000/9000 ×0.88`, while loop phases use CONT-BOSS-001.

### CONT-BOSS-005 — Bell Warden arena and scheduler

`arena.boss.bell_warden_01` is a clear `22×18` northwest-origin rectangle, solid shell, south door `(11,18)`, boss `(11,8)`, entry spawn `(11,16)`, staging `(11,15),(7,15),(15,15)`, and post-victory exit `(11,16.5)`. The climax portal assigns an entrant to the oldest healthy `Assembling` cell from the same realm with fewer than20 occupants, tie instance ID, or allocates a new cell; first entry starts a10 s assembly window and the twentieth ends it early. The radius3 entrance is safe and the scheduler pauses. At assembly end run a visible5 s ready countdown, close the door, and lock `N_locked=clamp(living occupants,1,20)`; later portal entrants use another assembling cell. Departure/death/Recall never rescales. Clear the volume, run the authored3 s boss introduction, and begin Phase1; ordinary2 s entry invulnerability still follows SIM-013. No hostile may exist within3 tiles of an entrant before Phase1. Recall is legal during staging and every combat phase, and returns the character under the ordinary Recall loss rules. The exit interactable appears only after the reward transaction commits. Six width1.2 belfry lanes are: L0 `(11,8)->(22,8)`, L1→`(17,18)`, L2→`(5,18)`, L3→`(0,8)`, L4→`(5,0)`, L5→`(17,0)`. Nonadjacent pair sequence is `(0,2),(1,3),(2,4),(3,5),(4,0),(5,1)`. Add anchors are `(3,4),(19,4),(3,13),(19,13),(8,5),(14,5)`.

If a Warden cell has zero living participants, wait5 s, clear boss/adds/projectiles/unsecured drops, mark the cell `Abandoned`, return no reward, and close it; committed deaths/Recalls remain final. The realm climax portal may allocate a fresh full-health cell only while the realm remains in Climax. A cell never carries health, rewards, or `N_locked` into another allocation.

Each cell deadline is exactly10 minutes from its Phase1 start. On the deadline tick, an authoritative boss defeat resolves before timeout; otherwise cancel attacks, clear boss/adds/projectiles/unsecured drops, mark the cell `TimedOut`, and grant no Warden reward. Create a no-cost successful-extraction exit for30 s at `(11,16.5)`; using it applies ordinary successful extraction. At its expiry, successfully auto-extract every living survivor and close the cell. Deaths and completed Recalls remain final, and retry reuses the stored terminal outcome.

Cells are C0 `[0,0,11,9]`, C1 `[11,0,11,9]`, C2 `[0,9,11,9]`, C3 `[11,9,11,9]`; boundary ownership uses `<` on north/west and `>=` on south/east. Anchor centers are `(5.5,4.5),(16.5,4.5),(5.5,13.5),(16.5,13.5)`. Boundaries are visual/noncolliding.

| Phase | Loop and exact starts |
|---|---|
| 1 | `16000 ms`: Warden Ring starts0/3200/6400/12800. Belfry Lanes start8000, use the next pair, and consume the otherwise-due Ring start9600; the ring cadence remains anchored to phase time. Acolyte spawn is independent at phase0 and every12000: request `2+floor((N_locked-1)/5)`, but spawn only `max(0,min(requested,5-living_acolytes))`; choose farthest free anchors from nearest player, ties anchor order. |
| 2 | `24000 ms`: Cell Anchors at0/12000 in each occupied cell lacking one. Verdict Lanes at2000/9500/17000. Warden Rings at500/4500/8500/12500/16500/20500, but consume a Ring if its impact would overlap any Verdict impact window. Verdict targets rotating slots and impacts at +1000 with400 ms staggering. |
| 3 | `9000 ms`: form ordered safe cells `[s,(s+1) mod4,(s+3) mod4]`, `s=cast_index mod4`; preview safe step0/1/2 at0–500/500–1000/1000–1500, then wait700. At2200–3000, every cell except safe[0] activates; at3000–3800, every cell except safe[1] activates; at3800–4600, every cell except safe[2] activates. Each activation deals36 once/player/step. Warden Ring starts4850 and emits after650 at5500. |

Cell Anchor HP is600×Elite scaling, no armor/reward/XP/triggers; its 10-shot ring starts every4000 ms from spawn, omits three target-facing adjacent shots, speed4/damage22. At phase2 start assign each player to current cell for add targeting; crossing remains legal and retarget assignment updates only at the next Anchor cast. At25%, Phase3 loop becomes8100 ms. Soft-enrage factor is0.85 under CONT-BOSS-001. The Warden never uses Execution damage or disables Recall. All GDD ENC-013 base health, armor, breaks, damage, participant cap, and reward rules remain exact.

---

## 9. Fallen Hero Echo modules

### CONT-ECHO-000 — Requiem arena and compiled attack metadata

`arena.echo.requiem_01` is a clear `20 × 18` walkable rectangle with a solid one-tile shell, south door centered `(10,18)`, Echo spawn `(10,8)`, player staging `(10,15)`, `(7,15)`, `(13,15)`, `(10,13.5)`, reward/exit `(10,16.5)`, and center `(10,9)`. Boundary centers used by Saltglass are north `(10,0.5)`, east `(19.5,9)`, south `(10,17.5)`, west `(0.5,9)`. Environmental edge strips are the interior's outermost `2.0 tiles`. No prop, pillar, slow tile, hazard, or alternate arena ships in Early Access. Entry uses the GDD five-second participant lock; the Echo cannot attack during staging/phase introductions; Recall remains available.

All Echo attack records compile these mandatory fields:

```text
damage_type: veil
damage_band_by_power_band: computed under CONT-ECHO-001 against that
  band's legal minimum-health entrant using GDD COM-003 thresholds
health_damage_cap_basis_points: 3500
telegraph_cue: telegraph.echo.<primitive>
audio_cue: audio.echo_warning; add audio.echo_major for Major
projectile_disposition: echo_arm_350ms_then_consume_on_player_or_solid
compatibility_tags: [echo_only, phase_scheduler_owned, no_external_modifier]
```

Counterplay tags compile by primitive: aimed/fan=`strafe`; ring=`follow_gap`; lane/environment=`leave_telegraph`; ground/pool=`leave_circle`; add/pylon/wisp=`destroy_or_dodge`; guard=`attack_unguarded_or_wait`; bounce=`leave_previewed_polyline`; trap=`avoid_reentry`. Threat cost is deterministic: each simultaneously active projectile `1`, lane `6`, ground hazard `4`, trap `4`, temporary add/pylon/wisp `12`, damaging moving front `8`. Sum the maximum simultaneous values for the module; reject above `96` or the GDD logical projectile cap. Healing/guard-only effects add zero. The compiled record stores the resulting integer; it is not calculated differently at runtime.

`echo_arm_350ms_then_consume_on_player_or_solid` means the projectile moves/renders normally but is nonhostile for the first `350 ms`. Any player overlapped during an unarmed tick is added to that projectile's permanent ignore set. At arming, an overlapping ignored player takes no damage; the projectile continues and can hit a different player. This is the sole Echo close-range exception and guarantees no Echo projectile can hit a player sooner than `350 ms` after spawn.

Echo telegraphs use ceiling-to-tick compilation and a conservative floor of `650 ms` first use / `500 ms` repeated use even when a listed warning is shorter. Module times below are schedule offsets; every damaging spawn still receives its listed warning. Fixed fixtures assert cue, counterplay, band, compatibility, disposition, and threat fields for every module combination.

### CONT-ECHO-001 — Power band

Compute inside the death transaction before equipment destruction; persist only band/signature tags, never item UIDs/rolls.

```text
rarity_bonus_tenths:
  Worn 0; Forged 5; Oathed 10; Relic 20; Sainted 30; BlackUnique 30

slot_effective_tenths = item_level * 10 + rarity_bonus_tenths
functional_tenths = round_half_up(
  (35*weapon_effective_tenths
   +25*relic_effective_tenths
   +25*armor_effective_tenths
   +15*charm_effective_tenths) / 100
)
echo_power_index_tenths = round_half_up(
  (character_level*10 + functional_tenths) / 2
)
```

Missing slot is zero. Read committed state immediately before death.

| Band | Index tenths | Solo HP | Armor | Damage unit `D` | Move |
|---|---:|---:|---:|---:|---:|
| 1 | 0–89 | 1000 | 2 | 16 | 3.5 |
| 2 | 90–119 | 1350 | 4 | 19 | 3.6 |
| 3 | 120–149 | 1800 | 6 | 22 | 3.7 |
| 4 | 150–179 | 2350 | 8 | 25 | 3.8 |
| 5 | 180+ | 3000 | 10 | 28 | 3.9 |

Normal minimum levels and reference minimum max health are: Band1 `level10/104`, Band2 `level12/109`, Band3 `level15/118`, Band4 `level18/126`, Band5 `level20/132`. Reference health is the level-adjusted Witch base multiplied by the legal `0.70` max-health floor and rounded half-up. For each coefficient/band, compute `reference_damage=round_half_up(coefficient×D)` and classify `reference_damage/reference_health` with GDD COM-003; persist that band-indexed result.

`scaled_health=round_half_up(base*(1+0.72*(N_locked-1)))`, `N_locked=1..4`. Party departure/death does not reduce it. Nothing else scales. Echo resistance/contact damage are zero; smoothing `120 ms`. Last Light move `×1.10`, cap4.5. Prefer living owner, else lowest party slot. Preferred distance: Vanguard3.25, Arbalist7, Witch5.5. Remembrance/Last Light strafe clockwise; Accusation counterclockwise. Participant order is owner then immutable helper slots, skipping absent/dead/Recalled.

### CONT-ECHO-002 — Phase scheduler

Each phase repeats a 20 s cycle: module A at `0`, module B at `8`, nonattacking break `17–20`.

| Phase | A | B |
|---|---|---|
| Remembrance `100–70%` | class primary | oath |
| Accusation `70–35%` | class primary | death-memory |
| Last Light `35–0%` | oath | selected Bargain |

Modules stop new hazards by cycle17 and expire all damage by20. Thresholds are `ceil(initial_max_health×0.70)` and `ceil(initial_max_health×0.35)`, processed one at a time; clamp to threshold, discard overkill, clear hostile entities, pause statuses/ticks, become untargetable/damage-immune/nonattacking `2 s`, then restart cycle0. Phases never reverse after healing. Ordinary break does not clear early. Snapshot aim/position at each telegraph. Weapon/relic signature tags affect presentation only in EA.

### CONT-ECHO-003 — Class primary modules

- `echo.primary.vanguard`: at relative `0,2.25,4.50 s`, target wedge `90°/5 tiles`, warn `750 ms` then `500`; seven offsets `-45,-30,-15,0,15,30,45°`, speed6, range5, `1.00D`, r0.12.
- `echo.primary.arbalist`: at `0,2.25,4.50`, next target; lanes `-8/0/+8°`, width0.30, warn750 then500; speed11, range11, r0.10; center `1.15D`, side `0.70D`.
- `echo.primary.witch`: at `0,2,4`, next target path warn750; orb speed5/range7.5/r0.16/`1D`; first enemy/wall/range bursts eight fragments every45°, starting incoming angle+22.5°, speed4.5/range3.5/r0.10/`0.55D`.

### CONT-ECHO-004 — Six oath modules

Missing oath fallback: Bell Retort, Long Vigil, or Orchard Rot by class.

- `echo.oath.bell_retort`: casts0/4 s; target Guard `120°` for1 s, projectile reduction60%, max2 charges one/distinct damage event; end emits `7+2c` evenly over `90+10c°`, speed6/range6/`0.90D`; clear charges.
- `echo.oath.ashen_bastion`: at0 warn900 three r0.60 pylons radius2.2 at target angle+0/120/240°, clamp inward until walkable; each HP4% scaled Echo, expires6.5; any alive grants20% direct reduction; each fires six-ring at1.5/2.9/4.3/5.7, speed4.5/range5/`0.45D`; no rewards/triggers.
- `echo.oath.long_vigil`: casts0/2.1/4.2/6.3; next target lane width0.60, warn1000 then650; bolt speed14/range14/r0.11/`1.50D`; fixed aim.
- `echo.oath.nailkeeper`: casts0/2.2/4.4; snapshot next target, r1.25 warn750; trap lasts4 s, first entering player takes`0.90D` and Frostbind1.5 then removes; inside-at-arm must exit/reenter; expiry harmless.
- `echo.oath.orchard_rot`: casts at `t=0` and `t=2.5`; target r1.8 warns `[t,t+0.9]`, activates `t+0.9`; ordinary `0.35D` ticks at `t+0.9,1.65,2.40,3.15,3.90,4.65`; final `1.00D` tick at `t+5.40` replaces—not adds to—the next ordinary tick. Each player hit by a final heals Echo0.75% scaled max, cap3%/module and module-start health.
- `echo.oath.saltglass`: casts0/2.4/4.8; candidate bounce points are north/east/south/west boundary centers; minimize Echo→point+point→target, tie N/E/S/W; both segments warn750; fixed polyline speed7, r0.14, pre-bounce`0.65D`, post`1D`, destroy after segment or12 total.

### CONT-ECHO-005 — Bargain selection and modules

Sort valid active Bargain IDs. Select deterministically:

```text
version_bytes = UTF8(content_version)
digest = BLAKE3(ASCII("echo-bargain-v1\0")
                || death_id_rfc4122_16_raw_bytes
                || little_endian_u32(len(version_bytes))
                || version_bytes)
index = little_endian_u64(digest[0..8]) mod count
```

`content_version` is restricted to ASCII lowercase letters, digits, dots, and hyphens. UUID text formatting and locale normalization never enter the hash.

No/disabled/invalid selection uses `echo.bargain.safe_pulse`: warn900; 10-shot rings at0.9/4.0, omit two adjacent target-facing shots (72° gap), speed5/range6/`0.65D`.

| Bargain | Exact module |
|---|---|
| `bargain.cinder_hunger` | At0/2.4/4.8 warn650, nine shots over70°, speed6.5/range7/`1.18D`; Echo receives12% more direct damage during module. |
| `bargain.glass_pulse` | At0/1.8/3.6/5.4 warn750, 12-ring with two target-facing adjacent omissions, speed5.5/range6/`0.75D`; Echo receives12% more direct. |
| `bargain.bell_debt` | Five aimed 5-shot/48° fans at0/1.15/2.30/3.45/4.60; warn750 then400; speed6/range7/`0.80D`; repeat fifth exactly300 ms later at50%, no retarget. |
| `bargain.lantern_ash` | Warn900; at0.9 heal4% scaled max, capped module-start health, and 10-ring with72° target gap; repeat only ring at3.9; speed5/range6/`0.75D`. |
| `bargain.grave_weight` | Move×0.92; at0/3.5 snapshot target and warn900 five r0.65 circles at `(0,0),(±1.5,0),(0,±1.5)`, clamp toward arena center; each `1.20D` once. |
| `bargain.salt_oath` | At0/2.2/4.4 warn800, 14-ring with two target-gap, speed4.8/range6/`0.65D`, Hex2 s; Echo healing received−20% during module. |
| `bargain.hollow_aim` | At0/2.2/4.4 warn750 lanes `-6/0/+6°`; speed13.2/range12; center`1D`, sides`0.60D`; Echo receives10% more direct. |
| `bargain.rooted_bloom` | Echo stationary; at0 warn900 r1.5 at each living participant, max3 by order; persists6 s, `0.375D`/750 ms; player max one tick/500 ms across overlaps. |
| `bargain.funeral_pace` | First ring warns750; later rings warn500; impacts at0.75/2.25/3.75/5.25 with counts8/10/12/12, speeds4.5/5/5.5/5.5, `0.60D`; omit two adjacent target-facing shots. |
| `bargain.saints_debt` | Warn900 two support wisps ±3 perpendicular to target axis, each5% scaled HP; while any alive Echo outgoing×0.85 and direct reduction15%; each fires six at1.5/3/4.5/6, speed4.5/range5/`0.45D`; no rewards/triggers. |
| `bargain.veil_mirror` | Fire0.75/1.85/2.95/4.05/5.15 after warn750 then400; base speed6/range6/`0.82D`; range expiry splits ±20° for range4/`0.328D`; collision does not split. |
| `bargain.ashen_pack` | From0–0.9 warn first fan/satellite spawn; at0.9 spawn opposite satellites radius2, clockwise90°/s; each aims at1.5/3/4.5/6 with a visible500 ms emitter charge, speed7/range8/`0.50D`; Echo fans seven/72° at0.9/3.15/5.4, first warned900 and later warned500, speed6/range7/`0.90D`. |

### CONT-ECHO-006 — Death-memory modules

Every damaging pattern declares one family; resolve using recorded content version. Missing uses `unknown`.

| Family | Exact module |
|---|---|
| `aimed_projectile` | Target lanes at0/2/4/6, warn750 then500; speed10/range11/`1D`. |
| `fan_projectile` | Seven-shot/90° target fans0/2.6/5.2, warn750 then500; speed6/range6/`0.75D`. |
| `radial_projectile` | 12-rings at0.9/3.1/5.3 after warn900 then500, omit two target-adjacent; speed5/range6/`0.65D`. |
| `rotating_projectile` | Six-spoke volleys0.9/2.2/3.5/4.8/6.1, orientation+15° each; first warn900; speed4.5/range6/`0.60D`. |
| `lane_or_beam` | At0/4 cross of perpendicular width0.75 lanes, warn900, active400, `1.40D` once/activation. |
| `ground_zone` | At0/2.5/5 target r1.1, warn900, `1.10D` once, no residue. |
| `charge_or_contact` | At0/2.7/5.4 target lane width1, warn800; damaging front speed8, `1.20D` once; Echo stays. |
| `summon_add` | Warn900 then two shades, each3% scaled HP; aimed `0.45D` each1.5 s, speed6/range7; expire module end, no rewards/triggers. |
| `damage_over_time_or_status` | At0/2.5/5 target r1.4 warn900; pool3 s, `0.25D`/500 ms, Hex2 s; one tick/player/500 ms. |
| `environmental` | N/S strips depth2 warn0–0.9, active0.9–1.5/`1.20D`; E/W warn2.6–3.5, active3.5–4.1. At time5.6 snapshot owner; for N/S and E/W compute minimum player-center distance to either strip, choose greater (tie N/S), warn chosen pair5.6–6.5, activate6.5–7.1. |
| `unknown` | `echo.bargain.safe_pulse`. |

### CONT-ECHO-007 — Attempt record and entry

Normal start requires Echo Available, owner initiator, party1–4, all living in Hall with no unresolved mutation, every character meeting the Band1/2/3/4/5 minimum level `10/12/15/18/20`, explicit ready and full-permadeath acknowledgement, and durable attempt plus danger-entry restore points. Practice may use Available/Defeated/Archived at any level, changes no state, grants nothing, and uses the persistent `PRACTICE — NO LOOT / NO PERMADEATH` rule.

```text
RequiemAttempt {
  attempt_id, echo_id, owner_account_id, instance_id,
  mode: Normal | Practice,
  participant_account_ids[], participant_character_ids[],
  n_locked, scaled_initial_echo_health,
  content_version, reward_seed_reference,
  started_at, last_heartbeat_at, lease_expires_at,
  state: Active | OwnerSucceeded | Failed | Aborted,
  resolution_reason
}
```

Acquire account/character locks by ascending binary account ID, then character ID. Persistent Echo stays Available during attempt; one unique active-attempt lease/Echo prevents concurrent normals. The healthy instance heartbeats every `10 s` and renews the lease to `now+60 s`.

After `60 s` without a healthy heartbeat, one transaction marks Active→Aborted with `instance_heartbeat_expired`, releases the lease, applies GDD danger-entry crash restoration to characters without a later committed death/extraction, and leaves Echo Available. Owner committed death, Recall, or expiry of the global TECH-015 `3 s` LinkLost/automatic-Recall window marks the attempt Failed, clears the encounter, safely returns living helpers without rewards, and leaves the Echo Available; the owner's death/Recall remains final. Helper death/Recall/3-second disconnect resolution does not fail the owner but makes that helper ineligible. Resolution retries return the stored outcome.

### CONT-ECHO-008 — Eligibility and rewards

Set `encounter_contribution_reference_health=scaled_initial_echo_health`. At defeat, a participant qualifies only if present≥50% active duration (waived under20 s), contribution≥0.5% reference health, no inactivity>20 s, alive, not Recalled, valid session/anti-cheat. Normal success requires owner qualification; otherwise Failed, Echo Available, nobody rewarded.

On owner-qualified defeat atomically compare/swap Available→Defeated, grant owner memorial-title/codex, grant each qualified helper one non-economic deed per unique Echo, and promote oldest Dormant.

For each qualified account: increment its Monday-week normal completion ordinal; >3 is not economic; a killer content/pattern pair already rewarded that week is not economic and still consumes ordinal.

| Role when economic-eligible | Marks | Ember | Appearance |
|---|---:|---:|---:|
| Owner | 20 | 1 | 10% |
| Helper | 5 | 0 | 0% |

At death, derive and snapshot theme only from class—not equipped/purchased cosmetics: Vanguard=`theme.echo.vanguard_ash`, Arbalist=`theme.echo.arbalist_ash`, Witch=`theme.echo.witch_ash`. Exact one-entry pools are:

| Theme | Earnable entitlement | Earliest stage |
|---|---|---|
| `theme.echo.vanguard_ash` | `appearance.echo.vanguard.remembered_ash` | Slice |
| `theme.echo.arbalist_ash` | `appearance.echo.arbalist.remembered_ash` | Slice |
| `theme.echo.witch_ash` | `appearance.echo.witch.remembered_ash` | Alpha |

The 10% roll selects the one unowned entry; if already owned or not stage-enabled, the pool is empty and grants nothing. These are earnable/Echo-eligible and never purchased. Reward writes, entitlement, ordinal, pair record, and state transition are idempotent by attempt ID. Dead/Recalled/disconnected/ineligible entrants get no item, currency, XP, Ember, or cosmetic roll.

### CONT-ECHO-009 — State transitions

| From | Trigger | To |
|---|---|---|
| none | Eligible death commits | Dormant |
| Dormant | Account has no Available; oldest `(created_at,echo_id)` | Available |
| Available | Owner-qualified normal defeat commits | Defeated |
| Available | Owner archives outside active attempt | Archived |
| Dormant/Available | Administrative/content disable | Disabled |
| Disabled | Re-enabled | Dormant, then ordinary promotion check |

At most one Available/account via database constraint or aggregate lock. Defeated/Archived are terminal. Practice never changes state. Death, Recall, disconnect, failed owner eligibility, allocation failure, or noncommitted server failure leaves Available. A committed death within Requiem can create a new Dormant Echo normally. Retry same attempt returns stored result. Terminal/Disabled Available transition promotes oldest Dormant.

---

## 10. Exact dungeon-modifier execution

### CONT-MOD-001 — `modifier.fevered_veil`

Only finite hostile-projectile patterns marked `fevered_repeat_eligible=true` qualify; the field has no default. Explicitly true patterns are:

```text
pattern.enemy.drowned_pilgrim.fan
pattern.enemy.bell_reed.gap_ring
pattern.enemy.mudbound.shield_fan
pattern.enemy.bell_acolyte.alternating_fan
pattern.enemy.sepulcher_knight.stop_ring
pattern.enemy.sepulcher_knight.shield_fan
pattern.enemy.brine_husk.wide_fan
pattern.enemy.salt_novice.needle_pair
pattern.enemy.tide_mourner.needle_fan
pattern.enemy.tide_mourner.aimed_needle
boss.caldus.shield_arc
boss.caldus.bell_ring
boss.caldus.charge_stop_ring
boss.confessor.crystal_needles
boss.confessor.confession_ring
boss.warden.warden_ring
miniboss.sepulcher_knight.stop_ring
miniboss.sepulcher_knight.shield_fan
miniboss.choir_abbot.recovery_ring
miniboss.masked_gardener.seed_fan
miniboss.rootmother.fan
miniboss.tide_mourner.needle_fan
miniboss.tide_mourner.aimed_needle
miniboss.salt_mirror.gap_ring
```

Every other Early Access pattern is explicitly false, including beams, lanes, summons, continuous rotors, contact/charge bodies, ground hazards, pushes, transitions, and primary projectiles that spawn secondary blooms.

Flatten spawn commands ordered `(scheduled_tick, authored_emitter_index, authored_projectile_index)`. Let count `n`, `k=ceil(n/2)`, and select `floor(((2j+1)n)/(2k))` for `j=0..k-1`. Clone each selected command exactly `500 ms` after its original, preserving transform, aim, fields, damage, and RNG—never retarget/reroll.

Dungeon allocation validates every eligible pattern under the chosen room/modifier pair against worst-case threat/corridor/Frostbind rules. At pattern scheduling, atomically reserve threat for original plus complete clone set. If unavailable, delay both up to `2 s`; if still unavailable, cancel both: ordinary/miniboss AI enters a harmless `1 s` Recover then resumes its cycle, while a boss advances to the next scheduled action without replacement. Never execute an eligible original without its repeat. Runtime player proximity cannot cancel a reserved repeat. Emit reservation/delay/fallback events. Promotion requires zero repeat-only cancellations and at least five emitted repeat sets on every fallback-layout combat path containing eligible patterns.

Reward: when the ordinary material pool contains an outcome tagged `curse_material`, multiply its integer weight by `6/5` then renormalize; if none, independent `20% material.funeral_root ×1` check once at successful exit.

### CONT-MOD-002 — `modifier.candleless`

- Minimap initializes empty except the vestibule and player's current room.
- Crossing a room activation boundary permanently reveals that room polygon, its open doors, and already traversed corridor centerlines for that instance/character.
- Party exploration is shared only while members are in the same instance; reconnect restores the server record.
- World rendering, telegraphs, ping markers inside visible world space, boss warnings, and exit indicators are unchanged.
- Base secret insertion chances are Tier I `20%`, Tier II `30%`, Tier III `40%`. Candleless uses `round_half_up(base_basis_points ×1.20)`: `24%/36%/48%`. Maximum one secret remains.

### CONT-MOD-003 — `modifier.glass_floor`

Normal smoothing changes `60→120 ms`:

```text
desired_velocity = normalize_or_zero(input) * resolved_max_speed
max_delta = resolved_max_speed * fixed_dt / smoothing_seconds
next = move_towards(current, desired_velocity, max_delta)
```

Apply to acceleration, braking, reversal, diagonal change, and wall slide; never overshoot. Final speed unchanged. Rush/Slip/Fold, knockback, conveyors, and forced movement ignore it. Client/server use the same pinned value. Reward multiplies outcomes tagged `precision_material` by `6/5`, then renormalizes; if none, independent `20% material.saltglass_shard ×1` at successful exit.

### CONT-MOD-004 — `modifier.saints_debt`

External healing cap becomes `20% max health / rolling 10 s`. It does not change the chance of Unique rarity. After Unique rarity and final legality pool:

```text
adjusted_weight = base_weight * 6 if support_unique else base_weight * 5
probability = adjusted_weight / sum(adjusted_weight)
```

If zero eligible `support_unique` candidates, leave Unique weights unchanged and grant each reward-eligible player one independent `20% material.echo_ember ×1` check per successful dungeon. Apply cap behavior; no reroll. Emit exactly one of `saints_debt_unique_weight_applied` or `saints_debt_material_fallback` per eligible player/completion.

### CONT-MOD-005 — `modifier.oathfire`

Multiply player outgoing direct damage and incoming direct damage by `1.10` in their GDD pipeline positions. Do not change hazards/status ticks unless tagged direct. At boss reward, grant one extra `equipment_roll` using the boss's item-level/source pool after setting Forged weight to zero and renormalizing Oathed/Relic/Sainted/Unique integer weights. If no legal Unique exists, ordinary no-Unique fallback still applies.

### CONT-MOD-006 — `modifier.restless_dead`

When an Elite finishes its spawn telegraph, create one `enemy.modifier_echo_shade` at the legal spawn anchor farthest from the entry door, tie anchor ID.

```text
health = round_half_up(elite_scaled_initial_health * 0.30)
armor=0; move=3.0; preferred_distance=5; no contact damage
cycle=3500 ms; telegraph=600 ms
attack=6-shot ring; omit two adjacent target-facing shots
speed=5; range=7
raw_damage: Tier I 8; Tier II 11; Tier III 14
damage_type=veil; damage_band=Chip; echo_memory_family=radial_projectile
projectile_disposition=consume_on_player_or_solid; threat_cost=6
```

The ring warns with `telegraph.echo_shade.ring` and `audio.echo_shade.ring`; its target-facing gap is the counterplay. The record expands with `release_stage=alpha`, `asset_ids=[sprite.enemy.modifier_echo_shade,portrait.enemy.modifier_echo_shade]`, and tags `[modifier_add,no_normal_count,no_on_kill,no_reward]`.

Shade expires on elite death/reset/room completion; no loot, XP, contribution, on-kill, memorial, or Echo record. One shade/Elite. Reward: each reward-eligible player gets one independent `25% material.echo_ember ×1` check per successful dungeon, not per Elite.

### CONT-MOD-007 — Compatibility

Tier I chooses zero/one; Tier II exactly one; Tier III one or two. A two-modifier pair is legal only where marked Yes:

| Pair with | Fevered | Candleless | Glass | Saints | Oathfire | Restless |
|---|---:|---:|---:|---:|---:|---:|
| Fevered | — | Yes | No | Yes | No | Yes |
| Candleless | Yes | — | Yes | Yes | Yes | Yes |
| Glass | No | Yes | — | Yes | Yes | Yes |
| Saints | Yes | Yes | Yes | — | Yes | Yes |
| Oathfire | No | Yes | Yes | Yes | — | Yes |
| Restless | Yes | Yes | Yes | Yes | Yes | — |

Sort legal pairs by concatenated IDs before seeded selection. Duplicate modifier is illegal.

---

## 11. Hub, tutorial, practice, cosmetics, and localization

### CONT-HUB-001 — Lantern Halls geometry and stage manifest

`hub.lantern_halls_01` is a noncombat `64×48` northwest-origin rectangle with a solid one-tile shell. Default spawn is `(32,42)`, character-select return is `(32,44)`, Realm Gate interactable is `(32,3)`, and every permanent station has a clear radius2. Desks are solid rectangles `[6,35,12,2]`, `[46,35,12,2]`, `[6,7,14,2]`, `[44,7,14,2]`; the central memorial plinth `[29,22,6,4]` is solid. Walkable aisles are at least3 tiles wide, and a radius0.30 player has a direct path from spawn to every enabled station. The entire instance is safe: no hostile/damage/projectile/pickup/drop action can be created.

Each slash-separated table label expands to independent stable IDs. In particular the tutorial IDs are exactly `portal.tutorial.vanguard`, `portal.tutorial.arbalist`, and `portal.tutorial.witch`; coordinates/stage columns correspond left-to-right. No slash is part of a runtime ID.

| ID | Coordinate | Core | Slice | Alpha+ |
|---|---:|---:|---:|---:|
| `station.realm_gate` | `(32,3)` | On | On | On |
| `station.vault` / `station.overflow` | `(10,38)` / `(15,38)` | On | On | On |
| `station.memorial_wall` | `(10,10)` | On | On | On |
| `station.oath_shrine` | `(24,18)` | On | On | On |
| `station.requiem_portal` | `(16,10)` | Closed | On | On |
| `station.party_circle` | center `(32,30)`, r6 | Closed | On | On |
| `npc.lantern_cartographer` | `(32,18)` | Closed | On | On |
| `station.wardrobe` | `(48,10)` | Closed | On | On |
| `station.forge` / `station.salvage` | `(50,38)` / `(55,38)` | Closed | Closed | On |
| `portal.tutorial.vanguard/arbalist/witch` | `(24,12)` / `(32,12)` / `(40,12)` | Closed/Closed/Closed | Closed/Closed/Closed | On/On/On |
| `portal.mastery_trial` | `(40,18)` | Closed | Closed | On |
| `station.store_preview` | `(54,10)` | Closed | Closed | Closed until M07 commerce flag; then On |

Closed stations remain collision-free floor with a gray `AVAILABLE IN A LATER TEST` placard and no interaction. Core's Realm Gate lists only `world.core_microrealm_01`; Slice lists the single M04 realm slice; Alpha lists healthy production Mire realms. Instance choice is lowest population, then realm creation tick, then instance ID. Return/extraction always spawns `(32,42)` with inventory placement already committed.

### CONT-HUB-002 — Exact station scripts

Every interaction requires range1.5, a500 ms server-authoritative Interact hold unless the row says instant, and one open panel/player. Escape closes without mutation.

| Station | Exact interaction/result |
|---|---|
| Realm Gate | Instant panel shows permitted realm, population, network health, and `Enter`; accepted transfer runs CharacterSafe preflight/deposit and creates danger restore point. |
| Vault/Overflow | Instant inventory panel; transfers use lowest-index destination and GDD LOOT-002/050. Overflow displays expiry and salvage value. |
| Memorial Wall | Read-only newest-first memorial list `(death_at desc,death_id)`; select opens exact DTH-020 snapshot. |
| Oath Shrine | Oath tab: at level10 with no oath, show the class's two exact oaths and permanent-life warning; first selection is free. Later change requires40 Ash, two confirmations, safe inventory, and no unresolved mutation. Bargain tab: list active Bargains in acquisition order; selected purge costs50 Ash, requires two confirmations, removes exactly that Bargain, and leaves its earned slot unfilled for CONT-014. |
| Requiem Portal | List Available first, then Dormant/Defeated/Archived by `(created_at,echo_id)`; Normal/Practice buttons run CONT-ECHO-007 preflight. |
| Party Circle | Instant party/invite/join-code/ready panel; standing in circle has no gameplay effect. |
| Lantern Cartographer | Show only discovered dungeons and exact per-entrant cost; quote expires after30 s; ready/charge/instance transaction follows SOC-002. |
| Wardrobe | Preview owned and store-preview appearances on a nondamageable mannequin; `Equip` accepts owned only. |
| Forge/Salvage | Four GDD crafting actions; output/storage preflight precedes charge. Salvage confirmation shows exact Ash and no material. |
| Class tutorial portal | Show class name, `PRACTICE — NO LOOT / NO PERMADEATH`, and Enter; no item transfer. |
| Mastery portal | Show eligibility, fixed loadout,300 s limit, no-loot/no-permadeath, Pass/Cancel. |
| Store preview | Before M07, return `stage_disabled` and expose no catalog/order endpoint; after commerce flag, preview precedes checkout and catalog is MON-002 allowlisted. |

Typed closed reasons are `stage_disabled`, `level_required`, `discovery_required`, `storage_resolution_required`, `party_not_ready`, `insufficient_currency`, `content_disabled`, and `service_unavailable`. No NPC invents alternate rewards.

### CONT-TUTORIAL-001 — Training Crypt

`tutorial.training_crypt_01` has `release_stage=alpha` and is a fixed nonpersistent `48×20` room with solid shell, walkable corridor `y=4..16`, checkpoint `(35,10)`, and one-way gates at x=`8,14,20,27,34,41`. Avatar is level1 Grave Arbalist with FP Pine Crossbow, Dented Scope, Reedcloth Wraps, empty Charm, two Red Tonics, fixed max health120, armor2, move5.2. Gear never enters production inventory. A gate opens only after its step's committed completion; leaving/reconnecting restores the latest step and default local state.

| Step | Exact objective |
|---:|---|
| 1 | Touch pads `(4,7),(6,10),(4,13)` in order, then hold primary toward east for1 s. |
| 2 | Destroy three20-HP stationary dummies `(11,7),(12,10),(11,13)`; one primary hit maximum/dummy/release. |
| 3 | Apply Grave Mark to80-HP dummy `(17,10)` and land three marked primary hits before Mark ends; reset dummy/Mark on failure. |
| 4 | A width1 lane at x24 warns900 ms and activates500 ms every3 s for10 nonlethal damage; accept one Slipstep that crosses from x≤22 to x≥25 without touching the active lane. |
| 5 | At `(30,10)`, altar deals nonlethal damage equal to40% max; use Red Tonic and wait until its400 ms restore commits. Refill one Tonic on retry only. |
| 6 | At checkpoint `(35,10)`, hold Recall400 ms. Tutorial Recall returns to the same checkpoint after500 ms, preserves tutorial gear, and completes the recall lesson. |
| 7 | Survive10 s in `arena [36,5]..[41,15]`: emitter `(39,10)` warns650 then emits an8-index ring every2.5 s, omits target-facing adjacent2, speed4.5/range6/r0.12/physical Chip5. Health reaching1 pauses hazards, restores full health/checkpoint after1 s, and retries without death. |
| 8 | At `(44,10)`, show `SCRIPTED DEATH LESSON — NO ITEMS OR CHARACTER WILL BE LOST`; only explicit `Begin lesson` starts a3 s countdown. `tutorial.scripted_bell` then sets the practice avatar to0, writes no durable death/item event, and opens the tutorial death summary with Lost=`nothing`, Preserved=`account`, Created=`lesson complete`. |

Closing the summary persists `training_complete`, destroys the avatar instance, and opens real class creation. First-action control begins before Step1 with no cutscene. Every prompt has input-icon, text, voice-subtitle key, and replay button. Skipping is available only after `training_complete` exists on the account.

### CONT-TUTORIAL-002 — Class tutorial template

`arena.class_tutorial_01` has `release_stage=alpha` and is clear `28×18`, shell, spawn `(3,9)`, dummy anchors `(12,5),(17,9),(12,13)`, emitter `(23,9)`, exit `(25,15)`. It instantiates the selected class at level10 with starter weapon/relic, `item.armor.pilgrim.t1`, `item.charm.ember_tooth.t1`, two Tonics, no oath, no loot/permadeath/XP. Failed objectives restore full health and reset only that objective after1 s.

- Vanguard: hit five primary arcs; Guard two separate five-shot physical Chip8 fans from emitter; Rush across the marked line x=14 and hit the center dummy with Rush; survive two fan cycles.
- Arbalist: hit five primaries; Mark center dummy and land three marked primaries; Slip across x=14 then hit one dummy with the empowered primary; hold Stillness Focused for2 continuous seconds; survive two fan cycles.
- Witch: burst one orb into at least two dummies; keep center dummy in Hex Bloom for four ticks; Fold across x=14; kill a40-HP Hexed dummy and hit another with Withering Echo; survive two fan cycles.

Emitter fans warn500 ms, use five offsets `-30,-15,0,15,30°`, speed5/range8/r0.12/damage8, and cannot reduce below1 HP. Completing all objectives grants only `class_tutorial_complete.<class_id>` codex state and opens exit. No stage requires tutorial completion to use a class.

### CONT-PRACTICE-001 — Mastery Trial

`arena.mastery_trial_01` has `release_stage=alpha` and is a `24×20` shell with spawn `(3,10)`, boss `(18,10)`, four solid `2×2` pillars southwest `(8,4),(14,4),(8,14),(14,14)`, and enemy anchors `(6,5),(12,5),(18,5),(6,15),(12,15),(18,15),(20,10)`. Timer is300 s from first movement/fire; pause menu does not pause it. Trial avatar is level10, no oath, two Tonics, and fixed Forged level6 gear:

| Class | Weapon / Relic / Armor / Charm |
|---|---|
| Vanguard | Rusted Cleaver / Dented Shield / `item.armor.ashplate.t1` / `item.charm.ember_tooth.t1` |
| Arbalist | Pine Crossbow / Cracked Mark Lens / `item.armor.pilgrim.t1` / `item.charm.ember_tooth.t1` |
| Witch | Funeral Focus / Cracked Censer / `item.armor.rootweave.t1` / `item.charm.ember_tooth.t1` |

Wave1: four Drowned Pilgrims. Wave2: two Bell Reeds plus four Pilgrims. Wave3: one Chain Sentry, two Bell Acolytes, two Pilgrims. Use anchors in listed order,900 ms spawn warnings, ordinary kits, no rewards. Clear projectiles, wait2 s between waves. After Wave3, spawn `boss.practice.mastery_caldus`: `release_stage=alpha`, tags `[practice_variant,no_major_boss_count,no_reward,no_xp,no_echo]`, HP3600, armor8, Caldus solo scheduler/pattern metadata and thresholds, no group variant/additional scaling,3 s intro; its derived sprite/portrait asset aliases resolve to the production Caldus source assets. `arena.mastery_trial_01` carries `[practice,no_dungeon_room_count]`. Defeat by timer tick to pass; equal-tick lethal damage to boss wins before timeout. Avatar death/timeout fails and returns Hall with no character/account mutation. Pass atomically creates the requested real successor at level10 with `character_xp=2700`, production starter kit, no oath, and records mastery use; trial equipment is destroyed. Retry unlimited.

### CONT-HUB-VALID-001 — Hub/onboarding fixtures

- Radius0.30 reachability and three-tile aisle tests cover every stage manifest and all station paths.
- Every closed/enabled station returns the exact typed state for Core/Slice/Alpha and commerce off/on.
- Training fixed-input traces cover success, each retry, reconnect at every gate, scripted-death nonpersistence, and skip-after-completion.
- Each class tutorial objective is achievable with its fixed loadout and cannot grant an item, XP, currency, death, or Echo.
- Each class completes100 fixed-seed Mastery Trials under300 s with the reference bot; timeout/death/retry creates no character, while pass creates exactly one level10 successor.

### CONT-COS-001 — Exact Early Access cosmetic and SKU manifest

Commerce remains disabled through Public Playtest. The Early Access catalog contains exactly 12 cosmetic sets: six earnable sets and six commercial sets. Commercial scope is exactly one Founder Supporter Pack SKU plus ten direct cosmetic SKUs; no runtime catalog row outside this allowlist may be displayed, quoted, ordered, or granted. USD references below are Steam base prices before platform-localized regional pricing and tax.

Earnable set grants are account-bound and nontransferable. `First Lantern` grants immediately; each other row first requires the named account-qualified achievement and then one idempotent Lantern Mark purchase from the Wardrobe catalog. Purchase key is `(account_id,set_id,"earnable-cosmetic-v1")`; a retry returns the stored grant.

| Earnable set ID / display name | Exact unlock and Mark cost | Exact entitlements |
|---|---|---|
| `cosmetic.set.earnable.first_lantern` / First Lantern | commit `training_complete`; 0 Marks | `nameplate.first_lantern`, `emote.first_lantern` |
| `cosmetic.set.earnable.bellbreaker` / Bellbreaker | first reward-qualified Sir Caldus defeat; 80 Marks | `grave_marker.bellbreaker`, `nameplate.bellbreaker` |
| `cosmetic.set.earnable.rootbound` / Rootbound | first reward-qualified Mother Veyr defeat; 120 Marks | `grave_marker.rootbound`, `nameplate.rootbound` |
| `cosmetic.set.earnable.salt_witness` / Salt Witness | first reward-qualified Salt Confessor defeat; 160 Marks | `grave_marker.salt_witness`, `memorial_frame.salt_witness` |
| `cosmetic.set.earnable.last_bell` / Last Bell | first reward-qualified Bell Warden defeat; 200 Marks | `memorial_frame.last_bell`, `nameplate.last_bell` |
| `cosmetic.set.earnable.echo_borne` / Echo-Borne | first owner-qualified Normal Requiem defeat; 200 Marks | `grave_marker.echo_borne`, `memorial_frame.echo_borne` |

The Founder set is `cosmetic.set.commercial.founder_lantern` / Founder Lantern. SKU `sku.gb.founder_supporter` / Founder Lantern Supporter Pack has US reference price `$19.99` and grants exactly `appearance.vanguard.founder_lantern`, `appearance.arbalist.founder_lantern`, `appearance.witch.founder_lantern`, `grave_marker.founder_lantern`, `nameplate.founder_lantern`, `memorial_frame.founder_lantern`, and `emote.founder_lantern`. The three appearance entitlements are class-fitted variants of one class-neutral visual theme and are sold only together in this SKU.

| Commercial set ID / display name | Direct SKU A / USD / entitlements | Direct SKU B / USD / entitlements |
|---|---|---|
| `cosmetic.set.commercial.cinder_vigil` / Cinder Vigil | `sku.gb.cinder_vigil_appearance` / `$9.99` / `appearance.vanguard.cinder_vigil` | `sku.gb.cinder_vigil_profile` / `$4.99` / `nameplate.cinder_vigil`, `emote.cinder_vigil` |
| `cosmetic.set.commercial.saltglass_witness` / Saltglass Witness | `sku.gb.saltglass_witness_appearance` / `$9.99` / `appearance.arbalist.saltglass_witness` | `sku.gb.saltglass_witness_profile` / `$4.99` / `nameplate.saltglass_witness`, `emote.saltglass_witness` |
| `cosmetic.set.commercial.funeral_bloom` / Funeral Bloom | `sku.gb.funeral_bloom_appearance` / `$9.99` / `appearance.witch.funeral_bloom` | `sku.gb.funeral_bloom_profile` / `$4.99` / `nameplate.funeral_bloom`, `emote.funeral_bloom` |
| `cosmetic.set.commercial.bellkeeper` / Bellkeeper | `sku.gb.bellkeeper_grave` / `$3.99` / `grave_marker.bellkeeper` | `sku.gb.bellkeeper_memorial` / `$4.99` / `memorial_frame.bellkeeper`, `nameplate.bellkeeper` |
| `cosmetic.set.commercial.quiet_grave` / Quiet Grave | `sku.gb.quiet_grave_emote` / `$2.99` / `emote.quiet_grave` | `sku.gb.quiet_grave_memorial` / `$4.99` / `grave_marker.quiet_grave`, `memorial_frame.quiet_grave` |

Every set has `store_card.<set_id>`. Every entitlement has `icon.<entitlement_id>` and `preview.<entitlement_id>` plus exactly one runtime asset by prefix: appearance/marker=`sprite.<entitlement_id>`, nameplate/frame=`ui.<entitlement_id>`, emote=`animation.<entitlement_id>`. All appearance previews show all affected classes from front, side, and combat silhouette; profile cosmetics preview against light/dark memorial backgrounds. The ordinary hostile/friendly projectile, hitbox, animation-timing, and combat-audio assets remain unchanged.

Order fulfillment grants only the entitlements written for the purchased SKU under the GDD idempotent order ledger. Refund/reversal removes only those entitlements. If a removed appearance is equipped, atomically equip `appearance.default.ashen_vanguard`, `appearance.default.grave_arbalist`, or `appearance.default.veil_witch` according to the character class. If another removed cosmetic is equipped, atomically equip `grave_marker.default`, `nameplate.default`, `memorial_frame.default`, or `emote.none` by slot before revocation commits.

These seven fallback records have `release_stage=ea`; provision them at live-namespace account creation and idempotently backfill every account during GB-M08-00 before admission. They are always owned, have tags `[baseline_default,non_entitlement,non_sku,no_set_count]`, cannot be sold/revoked/traded, and do not count among the 12 cosmetic sets:

| Default record | Exact runtime asset alias | Canonical `en-US` name / description |
|---|---|---|
| `appearance.default.ashen_vanguard` | `sprite.appearance.default.ashen_vanguard -> sprite.class.ashen_vanguard.base` | Default Ashen Vanguard / Standard Ashen Vanguard appearance. |
| `appearance.default.grave_arbalist` | `sprite.appearance.default.grave_arbalist -> sprite.class.grave_arbalist.base` | Default Grave Arbalist / Standard Grave Arbalist appearance. |
| `appearance.default.veil_witch` | `sprite.appearance.default.veil_witch -> sprite.class.veil_witch.base` | Default Veil Witch / Standard Veil Witch appearance. |
| `grave_marker.default` | `sprite.grave_marker.default -> sprite.hub.grave_marker.base` | Default Grave Marker / Standard memorial grave marker. |
| `nameplate.default` | `ui.nameplate.default -> ui.nameplate.base` | Default Nameplate / Standard account nameplate. |
| `memorial_frame.default` | `ui.memorial_frame.default -> ui.memorial_frame.base` | Default Memorial Frame / Standard memorial portrait frame. |
| `emote.none` | `animation.emote.none -> animation.empty_one_frame` | No Emote / No emote is equipped. |

Default appearance/marker/nameplate/frame records are visible in Wardrobe as always-owned fallback previews using the ordinary mannequin/background; `emote.none` appears only as the `None` selector and has no animated/store preview. A missing default/source alias, asset/localization/required commercial preview, entitlement shared by two commercial SKUs, wrong price, nonallowlisted SKU, or gameplay-capability tag fails the EA catalog build.

### CONT-LOC-001 — Canonical English localization and closure

`en-US` is the required and only advertised Early Access language. Optional locales may be packaged only when they reach 100% key parity with the matching promoted `en-US` manifest. Cumulative required manifests are `manifest.localization.fp.en-US`, `.core.en-US`, `.slice.en-US`, `.alpha.en-US`, `.playtest.en-US`, and `.ea.en-US`; each contains every key referenced by that stage's enabled content, UI, tutorial, error, store, legal, and support surfaces, sorted by UTF-8 key.

Canonical content values are produced mechanically, so implementation requires no copy invention:

1. The `.name` value is the explicit display name written beside the ID. If none is written, use the final ID component, replace `_` with spaces, and Unicode-titlecase each word; approved overrides are `t1→I`, `t2→II`, `t3→III`, `xp→XP`, `ui→UI`, `hp→HP`, `vfx→VFX`, and the proper names already capitalized in this document.
2. For a table-authored record, `.description` is every nonidentity cell joined left-to-right with `; `. For a heading/prose-authored record, it is that record's complete normative source span through the next peer heading. Convert Markdown to plain text by removing code/emphasis delimiters, retaining link labels, converting list boundaries to `; `, converting paragraph boundaries to `\n`, and collapsing other whitespace runs to one space. Preserve symbols, numbers, units, capitalization, and punctuation exactly. Do not truncate; tooltip/detail panels scroll.
3. SKU names are the exact display names in CONT-COS-001. SKU descriptions are generated as `Cosmetic only. Grants: {localized entitlement names joined by comma}. No gameplay power.` The Founder description additionally begins `Supports Gravebound development. `.
4. An entitlement name is its final ID component under the same token transform; where the cosmetic set table supplies a display name, prepend that set display name and the type in parentheses, for example `Cinder Vigil (Nameplate)`.
5. Any visible literal written in quotation marks or backticks in the GDD/content specification is its canonical `en-US` value. Code may introduce no new player-visible literal or localization-key reference without adding it to the checked-in stage manifest.

The exact Hall closed-reason values are:

| Key | `en-US` value |
|---|---|
| `error.stage_disabled` | Available in a later test. |
| `error.level_required` | Required level not reached. |
| `error.discovery_required` | Discover this dungeon in the realm first. |
| `error.storage_resolution_required` | Resolve storage before continuing. |
| `error.party_not_ready` | Every party member must be ready. |
| `error.insufficient_currency` | Not enough required currency. |
| `error.content_disabled` | This content is temporarily unavailable. |
| `error.service_unavailable` | Service unavailable. Try again. |

`tools_content` writes canonical UTF-8 JSON maps and a source-location sidecar. Build validation rejects duplicate keys, empty values, unresolved interpolation variables, control characters other than `\n`, a referenced key absent from the current stage, or an enabled store SKU whose name/description/entitlement/price-disclosure strings are absent. Copy editing changes values only through a reviewed localization diff and requires a bundle patch increment under CONT-002.

---

## 12. Validation and promotion manifest

### CONT-VALID-001 — Build-time invariants

The content bundle fails when any condition fails. Counts are stage-aware:

| Bundle stage | Items | Affixes | Classes/oaths | Bargains/modifiers | Normal/miniboss/major | Rooms | Landmarks/events |
|---|---:|---:|---:|---:|---:|---:|---:|
| Core | 18 | 0 | 1 / 2 | 3 / 0 | 6 / 2 / 1 | 9 | 2 / 0 |
| Slice | 45 | 12 | 2 / 4 | 6 / 3 | 12 / 4 / 2 | 20 | 5 / 2 |
| Alpha/Playtest/EA | 90 | 29 | 3 / 6 | 12 / 6 | 18 / 6 / 4 | 34 | 10 / 4 |

- Enabled IDs must equal the owning section's exact allowlist; disabled later-stage records do not participate in earlier combination/count tests.
- Core/Slice/Alpha contain 17/40/84 equipment templates plus 1/5/6 consumable/material definitions, totaling18/45/90; they contain 2/4/6 enabled starter equipment defaults respectively. Alpha additionally enforces12 Black Uniques,18 Armor,18 Charms.
- Alpha/Playtest/EA contain exactly the 25 `behavior_change` IDs in CONT-CATALOG-002; all resolve to an enabled equipment template and retain a qualifying non-flat behavior.
- Commerce is disabled in FP/Core/Slice/Alpha/Playtest. EA contains exactly the 12 CONT-COS-001 sets: six earnable and six commercial, with exactly one Founder Supporter Pack SKU and ten direct cosmetic SKUs; SKU-to-entitlement closure, preview assets, prices, revocation fallbacks, and zero gameplay tags all validate.
- Every reference resolves; `(bundle_id,id)` is unique. Cross-bundle duplicate IDs are legal only for the explicit CONT-FP-004 stable-ID overrides; every other stable ID has one payload lineage. No production pool references a prototype item.
- Reference closure is checked separately for every stage, including reward outcomes, event packs, practice aliases, assets, localization, modifiers, and tutorial loadouts; a disabled record can never satisfy an enabled reference.
- Every promoted stage's `en-US` manifest has exact key/value/reference closure under CONT-LOC-001; any advertised additional locale has identical key coverage and no empty/interpolation-invalid value.
- Every reward weight table totals its declared integer total and has at least one legal outcome for every supported class/slot/source combination.
- Core/Slice/Alpha reward expansion uses only its CONT-REWARD-004 stage outcomes and exactly the stage-enabled item definitions.
- Every rarity can fill its exact affix count without exclusion collision.
- All fixed geometry passes player-radius reachability; mandatory paths have two world-band routes; no spawn/telegraph overlaps safe entrance.
- Every hostile attack has damage type/band, telegraph, memory family, disposition, and threat cost.
- Every enabled boss timeline exactly matches its authored golden event list.
- Every dungeon pack budget equals its member-cost sum; for N1–8 the deterministic expansion roster leaves less than one cheapest member of unused target budget and fits compatible reused anchors.
- Every enabled Secret template binds exactly one stage-legal CONT-ROOM-008 encounter; its wave budgets, anchors, reference health, AtRiskPending reward, and retry key validate for N1–8.
- Every realm-director state/site candidate, five-second evaluation, cooldown, credit transition, and contribution denominator matches CONT-WORLD-008/009 for N1–20.
- Every reachable Bargain state has at least three ordinary legal candidates; emergency fewer-than-three behavior and offer retry are deterministic.
- Every spawn source resolves one exact XP profile and every `no_reward` source resolves XP0.
- Hub/tutorial/practice records carry count-exclusion tags and never increment dungeon-room, normal-enemy, miniboss, or major-boss headline counts.
- Event-only Elite variants carry explicit count-exclusion tags, complete attack payloads, locked scaling, and reward suppression/ownership; their IDs cannot enter normal, miniboss, or major-boss headline counts.
- Modifier-generated variants carry `no_normal_count` plus their reward/XP/Echo exclusions and cannot enter any headline encounter count.
- Run-material fixtures cover merge, cap reservation, ground overflow, concurrent grant idempotency, extraction commit, death, Recall, disconnect before commit, disconnect after commit, and wallet/pouch conservation.
- Echo fixtures cover every band entry gate; immutable `N_locked`; 350 ms projectile arming/ignore sets; 35% post-barrier health-damage cap; monotonic phase clamp/overkill discard/status pause; owner/helper failure; and the 10 s heartbeat/60 s lease watchdog.
- Boss/event fixtures cover every supported locked participant count, safe entrance, Recall availability, transition cancellation, add cap, deadline failure, reward ownership, and retry-safe portal allocation.
- Content count and cumulative roadmap burn-up agree.

### CONT-VALID-002 — Required deterministic fixtures

```text
content_schema_and_semantics
all_enabled_item_class_oath_bargain_combinations_min_max
affix_candidate_completeness
reward_distribution_10m_draws_per_profile
item_lifecycle_100k_mutation_sequences
run_material_pouch_transaction_matrix
mire_reachability_and_safe_routes_10k_seeds
each_dungeon_10k_seeds_plus_fallback
boss_100_fixed_traces_per_party_size
realm_event_all_participant_counts_and_deadlines
realm_director_state_site_cooldown_credit_matrix
echo_all_enabled_band_class_oath_bargain_memory_combinations
echo_attempt_lease_failure_and_retry_matrix
modifier_all_enabled_legal_pairs_all_enabled_dungeons
minimum_speed_no_ability_safe_corridors
fp_seed_trace_and_restart
hub_training_class_tutorial_mastery_journeys
```

Statistical reward fixtures use fixed seeds and assert exact deterministic hashes plus observed probability within `max(0.1 percentage point, 5 standard errors)` of configured probability.

Modifier promotion additionally asserts: Glass Floor traces use `120 ms` client/server smoothing for acceleration, stop, reversal, diagonal, and wall slide; any Frostbind overlap is solved at `4.0 tiles/s`; Oathfire recomputes final damage shares and rejects attacks whose telegraph category becomes illegal; Fevered reserves original+repeat threat, emits clones exactly `500 ms` after source commands, never emits an original alone, stays within caps, and produces at least five repeats on every eligible fallback path. All legal two-modifier pairs rerun these altered-physics assertions.

### CONT-VALID-003 — Bundle promotion record

```text
ContentPromotion {
  bundle_id
  git_commit
  schema_version
  content_version
  expanded_content_manifest_hash
  asset_manifest_hash
  localization_manifest_hash
  validator_report_hash
  deterministic_fixture_hashes[]
  balance_report_hash
  approved_by_design
  approved_by_engineering
  approved_by_qa
  approved_at
  rollback_bundle_id
}
```

No unlabeled temporary asset, disabled required record, unapproved balance override, unresolved warning, or missing rollback bundle may enter M06 or later. Runtime instances pin one promoted bundle from allocation through closure.
