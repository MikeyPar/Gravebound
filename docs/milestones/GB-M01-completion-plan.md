# GB-M01 completion plan and authoritative requirement matrix

## Document purpose

This is the executable completion contract for `GB-M01`, the ten-working-day local Combat Laboratory / First Playable. It expands every M01 roadmap row into exact scope, dependency order, automated evidence, human evidence, and a current completion state.

An implementation agent MUST use this file with the three authoritative sources; this file does not supersede them:

1. `Gravebound_Production_GDD_v1_Canonical.md` controls system behavior and acceptance.
2. `Gravebound_Content_Production_Spec_v1.md` controls exact content IDs and records.
3. `Gravebound_Development_Roadmap_v1.md` controls milestone scope and order.

If those sources conflict, the agent MUST file a `SPEC-CONFLICT` issue citing both locations and stop only the conflicting work. Existing code is not authority over the documents. Safety, legal, privacy, and payment requirements outrank all three.

## Status convention

| Value | Meaning |
|---|---|
| `PASS` | A completion audit exists and records passing implementation, automated verification, and accepted evidence. |
| `PENDING` | The row is incomplete or its completion audit/evidence has not passed. Work may be in progress. |

The status in this plan is intentionally conservative. `GB-M01-01A` through `GB-M01-02E` are `PASS`. GitHub Actions are explicitly excluded by user direction; every completed ticket passed its documented local build, deterministic, runtime, and evidence gates.

## Authoritative milestone contract

| Field | Exact requirement |
|---|---|
| Milestone | `GB-M01` |
| Timebox | Ten working days beginning on calendar day 4 and ending no earlier than calendar day 13 |
| Product stage | First Playable / Local First Playable |
| Runtime mode | `LocalLab`: `client_bevy + sim_core` in one process with ephemeral state |
| Content bundle | `fp.1.0.0`; promoted IDs and bytes are immutable |
| Objective | Prove enjoyable movement, shooting, danger, loot, death understanding, and immediate restart before accounts, networking, or content scale |
| Player | Level-1 `class.grave_arbalist`; primary, Grave Mark, Slipstep, Stillness; no oath |
| Arena | `arena.prototype.bell_laboratory_01` |
| Encounters | Three normal waves, then `boss.prototype.bell_proctor` |
| Enemies | `enemy.drowned_pilgrim`, `enemy.bell_reed`, `enemy.chain_sentry`, using the explicit `fp.1.0.0` overrides in `CONT-FP-004` |
| Loot | Exactly 12 `item.prototype.*` equipment templates plus shared `consumable.red_tonic`; prototype records do not count toward the production 90 |
| Persistence | None. All run items/entities/stacks are destroyed on restart; only local best-time telemetry may survive a completed run |
| Exit population | At least 10 testers not involved in the tested features (`GB-TP02`) |
| Failure response | Continue M01 combat tuning. Do not add classes, accounts, realm scheduling, crafting, progression breadth, or unrelated content |

Explicitly out of scope: accounts, durable persistence, server networking, realm scheduling, production dungeons, crafting, salvage, gifting, extraction, store/commerce, guilds, market, browser client, production deployment, other classes, oaths, and adjacent systems not named here.

No gameplay rule may exist only in Bevy presentation code. `sim_core` owns fixed-step rules, entity state, collision, damage, AI, projectiles, and encounter state; `sim_content` owns validated immutable definitions; `client_bevy` owns input sampling, presentation, audio, UI, and view models.

## Blocking prerequisites and dependency order

All M01 work depends on the passed M00 foundation:

- `GB-M00-01`: minimum workspace with `client_bevy`, `sim_core`, `sim_content`, `content_schema`, and `tools_content`.
- `GB-M00-02`: pinned toolchain, lockfile, format/lint rules, logging, and environment template.
- `GB-M00-03`: documented build, test, LocalLab, LocalStack, and headless commands.
- `GB-M00-04`: CI for format, lint, unit tests, content validation, and Windows release build.
- `GB-M00-05`: fixed `30 Hz` simulation clock and deterministic RNG.
- `GB-M00-06`: stable entity/content IDs and feature registry.
- `GB-M00-07`: minimum versioned class, ability, enemy, pattern, arena, item, and drop-table schemas.
- `GB-M00-08`: fixed-input trace runner and selected-tick state hashes.

The executable M01 order is:

```text
01A -> 01B -> 02A -> 02B -> 02C -> 02D -> 02E -> 11
                                                    |
                                                    +-> 03A -> 03B -> 03C
                                                                  |
                                                                  +-> 04A -> 04B -> 04C
                                                                                      |
                                                                                      +-> 05A -> 05B
03*/04*/05* + 11 -> 06A -> 06B -> 07A -> 07B
all simulation/content -> 08A -> 08B -> 09 -> 10A -> 10B -> GATE
```

Parallel work is legal only after its direct dependency is stable. Art may begin only from an approved seed frame and locked footprint/collision. Item population follows the item schema and one end-to-end weapon behavior. Boss population follows pattern primitives, debug overlays, and the fairness validator.

## Requirement matrix

### Input, player, and class kit

| Ticket | Authoritative features and exact outcome | Dependencies | Required automated evidence | Required visual/manual evidence | PASS | PENDING |
|---|---|---|---|---|:---:|:---:|
| `GB-M01-01A` | `SIM-001`, `SIM-002`, `SIM-010`, `TECH-001`, `TECH-006`, `CONT-FP-002`. One tile equals one simulation unit; top-down orthographic camera; about `24 x 13.5` visible tiles at 16:9; zoom contract `20 x 11.25` through `30 x 16.875`; `80 ms` critically damped follow; presentation-only shake max `0.18` tile; exact arena/grid presentation. | M00-05, M00-07, M00-08 | Exact arena compile/geometry tests, unit conversion, camera invariance, fixed-step tests, content validation, deterministic trace. | Accepted debug and release arena capture. | PASS | |
| `GB-M01-01B` | `SIM-003` through `SIM-005`, `CLS-020`. Rebind-ready action map; WASD normalized movement; independent aim boundary; Arbalist speed `5.1 tiles/s`; `60 ms` movement response; hurtbox radius `0.25`, physical radius `0.30`; shell/pillar collision; camera remains presentation-only. | 01A | Cardinal/diagonal/stop/reversal traces; wall, pillar, corner, and invalid-state tests; rebinding tests; camera convergence tests. | Controllable player and movement diagnostics in LocalLab. | PASS | |
| `GB-M01-02A` | `LOOP-002`, `SIM-003`, `SIM-004`, `CLS-020`, `CONT-FP-006`. Independent mouse aim and held primary. Initial Pine Crossbow: damage `20`, interval `455 ms`, range `9.5`, speed `12`, radius `0.10`, one bolt, no pierce. | 01B | Exact definition compilation; first shot and held cadence; press sequence; range/lifetime; locked origin/aim; stable projectile IDs; selected-tick golden. | Crossbow, aim guide, reticle, bolts, cooldown/range diagnostics. | PASS | |
| `GB-M01-02B` | `COM-001`, `COM-009`. Swept projectile collision against exact arena solids and enemy hurtboxes; earliest collision; stable tie order; zero-pierce terminal event; sprite bounds never decide hits. Health/damage remains deferred to 05A. | 02A | Face/corner/tangent/overlap/high-speed sweeps; solid-before-enemy and stable-ID ties; collision-vs-expiry; same-tick event ordering; exact collision golden. | Live hitbox, expanded solid, enemy-contact, and solid-contact overlays with `DAMAGE DEFERRED`. | PASS | |
| `GB-M01-02C` | `CLS-020`, `COM-007`, exact FP ability. Grave Mark cooldown `5.0 s`; bolt speed `12`, range `11`, `1.8W`; `graveled_mark` `4.0 s`; owner primary bonus `+15%`; exactly one target per Arbalist; replacement removes old. Ability buffer is at most `100 ms`. | 02B | Exact authored-ms validation; sequenced/buffered input; enemy/solid/range terminal cases; apply/refresh/replace/expiry; same-tick Mark-before-primary ordering; deterministic intent/hash. | Distinct Mark projectile/impact/rings, target and timer, cooldown/GCD, raw intent, unchanged-health seam. | PASS | |
| `GB-M01-02D` | `CLS-020`, `COM-007`, `COM-008`. Slipstep cooldown `8.0 s`; `2.0` tiles over `180 ms`; movement direction or backward aim when neutral; `25%` direct-damage reduction while traveling; no invulnerability; next primary within `1.5 s` gains `+30%` speed and one pierce; Exhaustion `1.5 s`; walls stop travel; enemies do not body block. | 02B; GCD integration with 02C | Exact authored-ms compile; input/GCD/buffer/Exhaustion rejection; five-segment swept movement; exact endpoint; solid clamp; empowered shot consume/expiry; stable multi-contact/ignore set; rollback on rejected state. | Exact trail/end point, two distinct pierce contacts, cooldown/Exhaustion/cast/shot counters, unchanged-health seam. | PASS | |
| `GB-M01-02E` | `CLS-020`, `UI-005`. Stillness: movement magnitude below `20%` for `600 ms` grants Focused; Focused gives `+10%` projectile speed and `+8%` primary damage; ends immediately when movement exceeds `20%`, Slipstep begins, or damage is received. HUD exposes binding, Focused state/source, cooldown/status duration. Still Eye override belongs to 07B integration: activation `400 ms`, damage `+6%`, speed remains `+10%`. | 02C, 02D | Exact threshold ticks; below/equal/above boundary; movement, Slipstep, and future damage cancellation order; primary modifier; item override; state hash; invalid arithmetic rollback. | Focused acquisition/break and modified shot are readable without color alone. | PASS | |
| `GB-M01-11` | `SIM-003`, `COM-004`, `CONT-FP-007`, `CONT-FP-010`. `consumable.red_tonic`: Q/belt input; restore `30%` max health over `400 ms`; shared potion cooldown `2.0 s`; taking damage does not interrupt; consumed on use; stack cap `6`; run starts with two in belt slot 1; exact merge/overflow behavior; Undertaker Knot override `35%` and `2.5 s`; audio/UI feedback. | 02E; minimum belt state from 07A may be implemented first if isolated | Exact heal ticks/rounding; missing/empty/stack-full cases; shared cooldown; damage during restore; item override; consumption and restart cleanup. | Belt count, healing progression, cooldown, rejected input, and audio confirmation. | PASS | |

### Enemies, patterns, boss, and combat contract

| Ticket | Authoritative features and exact outcome | Dependencies | Required automated evidence | Required visual/manual evidence | PASS | PENDING |
|---|---|---|---|---|:---:|:---:|
| `GB-M01-03A` | `CONT-FP-004` override for `enemy.drowned_pilgrim`: Fodder; HP `85`, armor `0`, hurtbox `0.34`, speed `2.2`, aggro `10`, leash `12`; spawn telegraph `900 ms`; approach to distance `5`; fan locks at `300 ms` windup, offsets `-15/0/+15`, speed `5.5`, radius `0.12`, lifetime `2.2 s`, physical Chip `8`, origin `0.45`, threat `3`, memory `fan_projectile`; normal-enemy reward table. | 11; M00 enemy/pattern schemas; 02B projectile/collision seam | Exact state/tick trace, aim lock, projectile payload, spawn-safe/no-early-hit, deterministic hash. | Spawn, approach, fan origin/shape, and death/drop readability. | PASS | |
| `GB-M01-03B` | `CONT-FP-004` override for `enemy.bell_reed`: Pressure; HP `130`, armor `2`, hurtbox `0.42`, stationary, aggro `11`, leash `12`; spawn `900`, dormant `500`; `3 s` ring; first/repeat warnings `450/300 ms`; 8 indices, omit adjacent starting `0,1`, advance start `+3 mod 8`; emit 6 at speed `4.5`, radius `0.13`, lifetime `3 s`, veil Chip `10`, threat `6`, radial memory; normal reward. | 03A | Exact gap sequence, first/repeat warning, cycle trace, payload/hash, no-spawn-hit. | Ring gap is recognizable and followable in grayscale. | PASS | |
| `GB-M01-03C` | `CONT-FP-004` override for `enemy.chain_sentry`: Anchor; HP `300`, armor `5`, hurtbox `0.55`, stationary, aggro/leash `13`; spawn `900`, dormant `700`; cycle `4.5 s`; axes `0/90` then `45/135` alternating; lane width `0.9` to arena collision; first/repeat warning `800/650 ms`, active `350 ms`; once/player/cast; physical Pressure `22`, threat `12`, lane memory; normal reward. | 03B | Exact axis alternation, wall extent, first/repeat warning, once-per-cast hit group, cycle/hash. | Lane origin, shape, active interval, and safe space remain readable. | PASS | |
| `GB-M01-04A` | `ENC-003`, `ENC-004`, `COM-005`, `COM-006`. Generalize the three already populated FP attack forms into data-driven `fan`, `ring_with_gap`, and `telegraphed_lane` primitives plus a fixed timeline. Each attack has stable pattern/telegraph/audio IDs, type/band, duration/lifetime, counterplay, memory family, disposition, threat, cap, compatibility, and phase-cancel policy. Validate at speed `4.5`, radius `0.25`, `120 ms` RTT, no ability; normal safe corridor `0.80`; no newly spawned projectile reaches current position below `350 ms`; normal cap `300`, boss cap `500`. | 03A-C; M00 schemas and trace | Schema/semantic rejection; tick-ceiling telegraphs; primitive goldens; minimum-speed safe solver; corridor, cap, threat, compatibility, and forbidden-combination tests. | Timeline/threat/pattern debug view matches simulation. | PASS | |
| `GB-M01-04B` | `CONT-FP-005` Bell Proctor base and phase 1. Fixed at `(24,12)`; HP `3000`, armor `4`, hurtbox `0.65`; intro `2 s`; target solo duration `75-110 s`; soft enrage `180 s`; reward `reward.prototype.boss`. P1 `100-70%`, loop `7200 ms`: fan warn/fire `0/400`, `2400/2800`; ring `5600/6250`. Fan: five offsets `-20/-10/0/+10/+20`, speed `6`, r `0.12`, life `3 s`, veil Chip `12`, threat `5`. Ring: 16 indices at `22.5`, omit four adjacent, start `0`, advance `+5 mod 16`, warning `650`, speed `4.5`, r `0.13`, life `4 s`, veil Pressure `15`, threat `12`. | 03A-C, 04A, arena/waves | Exact compiled record and golden event ticks; fan/ring geometry; target duration instrumentation; safe route; caps; fixed hash. | Intro, identity, fan and ring counterplay readable. | PASS | |
| `GB-M01-04C` | `CONT-FP-005` phases 2/3 and breaks. Each threshold cancels old timeline/projectiles once, then `3 s` no-attack break with `+20%` damage received. P2 `70-35%`, loop `10000`: fans `0/400`, `2400/2800`; ring `4200/4850`; cross `7000/7900`. Cross alternates `0/90` and `45/135`, width `1.0`, warning `900`, active `500`, physical Major `28`, once/cast, threat `12` per lane/max `24`; no fan/ring impact within `500 ms`. P3 `35-0%`, loop `10000`: preview A `0-500`, wait `400`, ring `900`; preview B `1000-1500`, wait `300`, ring `1800`, gap `A+4`; fan `4000/4400`; cross `6500/7400`; fan `8400/8800`. Below `20%`, loop restarts at `9000`; soft enrage only shortens remaining downtime `15%`. | 04B | Exact threshold/cancel/break trace; phase timelines; ring preview order; cross exclusion window; below-20 and enrage rules; 20 full deterministic boss runs; no crash/soft lock/unavoidable route/hash mismatch. | Phase/break/readability review and cause-identifiable deaths. | PASS | |
| `GB-M01-05A` | `COM-001` through `COM-003`. Damage order: validate -> raw/type -> attacker multipliers -> resistance clamped `-25%..25%` -> strongest reduction -> armor reduction `min(armor, reduced*0.35)` -> half-up positive minimum 1 -> barrier -> declared cap -> health -> full event -> same-tick death. No critical hits, random evasion, cheat death, or resurrection. Bands by final max-health share: Chip `1-8%`; Pressure `>8-18%`; Major `>18-35%`; Severe `>35-60%`; Execution `>60%` and prohibited in FP. | 02B, ability intent seams, enemies/boss | Formula and rounding boundaries; negative/positive resistance; strongest reduction; armor; barrier; declared/no cap; event intermediate values; same-tick lethal order; band validation; forbidden mechanics. | Health frame, lost segment, damage direction, band/debug labels. | PASS | |
| `GB-M01-05B` | `COM-005`, `COM-006`, `COM-009`. Every hostile attack has origin, shape, color family, telegraph, lifetime, counterplay, and audio for Major+. Minimum first/repeat warnings: Chip `250/200 ms`, Pressure `400/300`, Major `650/500`. Exact FP records may exceed these and must validate. Hostile effects render over friendly/loot/decorative effects; shapes remain distinct in grayscale. | 04A, 05A, exact FP attacks | Semantic validator over every enabled attack; safe corridors; spawn/reach timing; threat/projectile cap; grayscale metadata; missing cue/field rejection. | First-use/repeat, audio priority, grayscale/high-contrast review. | PASS | |

### Death, loot, debug, accessibility, and gate evidence

| Ticket | Authoritative features and exact outcome | Dependencies | Required automated evidence | Required visual/manual evidence | PASS | PENDING |
|---|---|---|---|---|:---:|:---:|
| `GB-M01-06A` | `COM-002`, `CONT-FP-001`, `CONT-FP-009`, `CONT-FP-010`. Local, nonpersistent death transaction: health zero wins in the same tick, rejects later actions, captures lethal event/trace, destroys all current run entities/items/stacks, and returns control in a fresh run within `3 s`. Do not implement durable `DTH-001`, memorial, Echo, or account state. | 05A, run inventory/entity ownership | Lethal-vs-later-action order; one cleanup; no surviving hostile/item/projectile/stack; fresh seed/default loadout; repeat/retry safety; restart time instrumentation. | Death transition and fresh controllable run; no implication of persistence. | PASS | |
| `GB-M01-06B` | `DTH-020` presentation subset, `PRD-123`, `QA-100`, `CONT-FP-008/010`. Cause display identifies killer, attack, damage, type, source, and recent timeline before/alongside restart. Boss completion summary shows clear time, damage taken, potion uses, lethal cause if any, current/best time; primary `Run Again`; Escape keeps cleared arena; pause menu exposes same action. | 06A | View-model states; correct killer/pattern binding; summary metrics; Run Again cleanup; Escape/no-restart; pause action; control under `3 s`. | Blind tester is asked to name killer before detailed trace; UI remains keyboard operable. | PASS | |
| `GB-M01-07A` | `LOOT-001`, constrained `LOOT-002`, `CONT-FP-010`. Four slots Weapon/Relic/Armor/Charm; prototype backpack capacity `8` nonbelt stacks; two belt slots. Explicit Equip; otherwise first backpack index. Swap sends old item to first empty index and rejects when full. No-capacity pickup remains `60 s`. Reward panel uses identical capacity rules and offers Drop existing, Leave reward, Equip/Take; no silent destruction. No gifting/salvage/crafting/extraction. | M00 item schema; 02A; 06A ownership | Slot legality, deterministic placement, explicit confirmation, full-backpack rejection, field swap, pickup expiry, reward parity, restart cleanup. | Inventory overlay preserves center/lower-middle playfield and shows behavior before flat stats. | PASS | |
| `GB-M01-07B` | `CONT-FP-006`, `CONT-FP-008`. Populate the exact 12 fixed templates and five reward tables listed below; no random affixes. Demonstrate at least four visibly different behaviors. | 07A, 02C-E, 11 | Exact count, IDs and payloads; all weight sums; independent normal checks; without-replacement/nonduplicate choices; deterministic known-seed rewards; item overrides; restart destruction. | Field-equip proof for at least four changes in geometry/cadence/ability/passive/potion behavior. | PASS | |
| `GB-M01-08A` | `PRD-123`, `COM-009`, `SIM-011`, `CONT-FP-001/009`. Debug overlay for hitboxes, spawn/anchors/wave state, pattern timeline/threat, fixed seed, state hash, and performance counters. Default selectable seed is hexadecimal `B311A501`. | Simulation/content rows | Overlay reads authoritative data; toggles do not mutate gameplay; selected seed produces identical spawn/attack/damage/drop ticks and hashes. | All shapes/labels readable and explicitly debug-only. | PASS | |
| `GB-M01-08B` | Roadmap package 08. Developer-only time scale and invulnerability controls; timeline/cap/performance display. Controls must not leak into ordinary playtest metrics or shipped defaults. Use the proposed defaults below until an authority decision replaces them. | 08A | Integer-tick event order preserved; normalized trace invariant; invulnerability blocks health mutation without hiding collision/events; disabled by default. | Clear developer-mode labeling and active-state indication. | PASS | |
| `GB-M01-09` | `TECH-070`, `COM-009`, roadmap gate. Repeatable stress fixture: 800 hostile projectiles, 40 enemies, standard effects; target PC Windows 10/11, 4-core 3.0 GHz-class CPU, 8 GB RAM, GTX 1050-class GPU, 1920x1080. Maintain `60 FPS`; ordinary p95 frame `<=16.7 ms`, p99 `<=33.3 ms`; 30-minute loop memory `<=1.5 GB`, no monotonic leak. | 04C, 05B, 08A/B | Fixed scenario/seed, frame-time percentile report, projectile/enemy counts, 30-minute memory samples/leak check, deterministic 60-second boss replay, 20 full boss runs. | Capture full/reduced-effects modes and confirm hostile telegraphs are never culled. | PASS | |
| `GB-M01-10A` | `UI-010`, `UI-030`, `ART-005/006/030`, `COM-006/009`. First accessibility pass: shape plus color; no red/green-only information; grayscale-distinct attacks; hostile priority never culled; high contrast; reduced motion; screen shake `0-100%` default `50`; flash `0-100%` default `50`; friendly opacity `10-60%`; full-screen flashes prohibited. Proposed exact M01 subset is below. | Presentation for all attacks/HUD | Settings boundary/value tests; render-plan priority tests; snapshot matrix; reduced-motion/flash must preserve mechanics; keyboard focus. | Screenshots at 1280x720 and 1920x1080 for combat, boss warning, low health, inventory, and every M01 projectile preset. | PASS | |
| `GB-M01-10B` | `TEL-001` through `TEL-004`, `QA-008`. Local privacy-safe instrumentation records build/bundle/cohort, session/run, boss start/phase/defeat, damage, death/killer, item pickup/equip/destroy, restart, crash, and survey answers. Observe first confusion/damage/item/death/restart; ask killer before trace; ask movement/shooting/dodging and open questions; segment genre familiarity; separate observation from opinion. | All player-visible rows | Schema validation, required-field tests, local sentinel identity policy, event ordering, restart correlation, no raw personal identifiers, deterministic export fixture. | Dry-run survey and evidence form; cohort exclusion check. | PASS | |
| `GB-M01-GATE` | `QA-100` and roadmap M01 exit. Execute the external blind test and publish report/stop-go. All conditions in the gate section below are conjunctive. | Every row above | Full CI/evidence suite, content validation, deterministic/stress report, 20-run boss report, immutable build/bundle IDs. | At least 10 eligible blind testers; survey/observation gate; recorded go/no-go. | PASS | |

## Exact First Playable records

### Arena and wave script

`arena.prototype.bell_laboratory_01` uses a northwest origin, exactly `32 x 24` walkable tiles, and a solid one-tile shell.

- Player spawn `(4,12)`; boss spawn `(24,12)`.
- Pillars `[10,5,2,3]`, `[10,16,2,3]`, `[20,5,2,3]`, `[20,16,2,3]`.
- Anchors: `N1=(8,3)`, `N2=(16,3)`, `N3=(24,3)`, `S1=(8,21)`, `S2=(16,21)`, `S3=(24,21)`, `E1=(29,8)`, `E2=(29,16)`, `W1=(3,8)`, `W2=(3,16)`, `C=(16,12)`.
- Reward pedestal `(4,4)`; debug-only Tonic refill `(4,20)`.
- Initial equipment: Pine Crossbow, Dented Scope, Reedcloth Wraps, empty Charm; two Red Tonics in belt slot 1.

| Wave | Exact spawns | Start | Reward | Budget |
|---|---|---|---|---:|
| 1 | Pilgrims at N1, N3, S1, S3 | `1.5 s` after first player move/fire | `reward.prototype.wave_1` | 4 |
| 2 | Reeds at N2/S2; Pilgrims at W1/W2/E1/E2 | Wave 1 panel closes | `reward.prototype.wave_2` | 10 |
| 3 | Sentry at C; Reeds at `(8,6)`/`(8,18)`; Pilgrims at E1/E2/N3 | Wave 2 panel closes | `reward.prototype.wave_3` | 15 |
| Boss | Bell Proctor at `(24,12)` | Wave 3 panel closes, then `2 s` introduction | `reward.prototype.boss` | Authored boss |

Every spawn has a `900 ms` ground telegraph and cannot attack before it completes. Wave completion clears hostile projectiles, waits `1.5 s`, then opens rewards. Reward UI does not pause simulation, but no hostile entity exists while it is open.

Normal drops appear at the death position after `250 ms`; walking within `0.75` tile or Interact within `1.25` picks them up. A capacity-blocked pickup remains for `60 s`. Recall always returns typed error `recall_unavailable_combat_laboratory` and HUD text `RECALL UNAVAILABLE - LOCAL TEST`.

### Exact item catalog

| ID | Exact FP behavior |
|---|---|
| `item.prototype.weapon.pine_crossbow` | Worn Weapon; damage 20; interval 455 ms; range 9.5; speed 12; radius 0.10; one bolt; no pierce |
| `item.prototype.weapon.grave_repeater` | Forged Weapon; damage 17; interval 360 ms; range 8.5; speed 11; radius 0.10; one bolt; no pierce |
| `item.prototype.weapon.longbolt_crossbow` | Oathed Weapon; damage 28; interval 600 ms; range 12; speed 15; radius 0.09; one bolt; no pierce |
| `item.prototype.weapon.scatterbow` | Relic Weapon; three bolts `-8/0/+8`; each damage 12; interval 520 ms; range 8; speed 10.5; radius 0.10; one target max two bolts; displayed single-target `W=24` |
| `item.prototype.relic.dented_scope` | Worn Relic; Grave Mark range becomes 12 |
| `item.prototype.relic.mark_lens` | Oathed Relic; Grave Mark duration 6 s; marked-primary bonus becomes 12% |
| `item.prototype.relic.slip_clasp` | Oathed Relic; Slipstep cooldown 7 s; empowered-shot window 1.0 s |
| `item.prototype.armor.reedcloth_wraps` | Worn Armor; +8 max health |
| `item.prototype.armor.parish_leather` | Forged Armor; +20 max health; +2 armor; movement x0.98 |
| `item.prototype.armor.saltglass_coat` | Oathed Armor; max health x0.92; +1 armor; +12% veil resistance |
| `item.prototype.charm.still_eye` | Oathed Charm; Stillness 400 ms; Focused damage +6%; projectile speed remains +10% |
| `item.prototype.charm.undertaker_knot` | Oathed Charm; Red Tonic heal 35%; shared cooldown 2.5 s |

Global weights in the table's order are `12,10,6,6,12,8,8,12,10,6,6,4`.

| Reward ID | Exact result |
|---|---|
| `reward.prototype.normal_enemy` | Independent 8% global-equipment check and 10% Red Tonic check; both may succeed |
| `reward.prototype.wave_1` | One weapon: Pine 35, Repeater 30, Longbolt 20, Scatterbow 15; plus one Tonic |
| `reward.prototype.wave_2` | One relic: Dented 40, Mark Lens 30, Slip Clasp 30; one armor: Reedcloth 40, Parish 35, Saltglass 25 |
| `reward.prototype.wave_3` | One Charm: Still Eye 60, Undertaker 40; plus one nonduplicate global equipment selection |
| `reward.prototype.boss` | Three distinct global equipment selections without replacement; two Tonics; completion metrics |

All prototype rewards are destroyed on restart.

## Content and asset contract

Every runtime record MUST include `id`, `schema_version`, `content_version`, `enabled`, `release_stage`, localization name/description keys, `asset_ids[]`, `tags[]`, normalized `numeric_payload`, and `source_document_feature_id`. Missing data is a build error; runtime undocumented defaults are prohibited.

For `fp.1.0.0`, derived asset IDs are:

- Item: `icon.<item-id>`.
- Enemy/boss: `sprite.<id>` and `portrait.<id>`.
- Arena: `tilemap.<id>`.
- Ability/pattern: `vfx.<id>` and `audio.<id>`.

Generated expanded JSON MUST be checked in and compared to the canonical expansion hash. All enabled references, assets, and localization keys must close within the stage. Production pools cannot reference prototype items. The three FP enemies are the only explicit bundle-scoped stable-ID payload overrides; production payloads in `CONT-ENEMY-*` must not leak into `fp.1.0.0`.

M01 cumulative burn-up is one class, three normal enemies, one benchmark boss, one arena, 13 item icons, 10 combat VFX cues, 12 audio cues, five UI surfaces, and 20% ship-quality share. Temporary assets must be registered. The Combat Laboratory scene kit needs collision, anchors, lighting/palette, navigation, readability, localization, and fixed-trace validation.

## Automatable evidence plan

Each ticket completion report MUST list changed files, commands and results, acceptance evidence, remaining edge cases, and conflicts. Before changing a ticket from `PENDING` to `PASS`, create `docs/milestones/<ticket>-audit.md` and satisfy this sequence:

1. Run the repository command equivalent to `format` and fail on any diff.
2. Run full all-target lint with warnings denied.
3. Run unit and integration tests for the changed state owner and client boundary.
4. Run `content-validate` against strict `fp.1.0.0` and prove generated schemas/content have no unexpected diff.
5. Run the ticket's deterministic trace twice in separate processes and compare selected-tick hashes and exact event/state snapshots.
6. Run the unchanged M00 golden trace to prevent foundation regressions.
7. Build optimized Windows release and launch the exact semantic evidence scenario.
8. Assert the runtime log has no warning, error, or panic and the evidence file is atomically complete.
9. Inspect the image at actual game scale; reject incomplete GPU composites, unreadable labels, false hostile colors, or hidden mechanics.
10. Record evidence SHA-256 and the applicable local/CI gate policy. Do not claim GitHub verification if it was excluded.

The repository-level command is currently:

```powershell
tools\dev.cmd ci
```

Required named FP fixtures:

| Fixture | Assertion |
|---|---|
| `fixture.fp_arena_reachability` | Radius-0.30 player reaches every anchor |
| `fixture.fp_min_speed_paths` | Speed-4.5, no-ability bot has a collision-free route through every mandatory boss pattern |
| `fixture.fp_wave_budget` | Wave budgets exactly 4, 10, 15 |
| `fixture.fp_no_spawn_hit` | No attack hits within 350 ms after its spawn telegraph |
| `fixture.fp_reward_sums` | Every declared percentage table totals 100% |
| `fixture.fp_item_count` | Exactly 12 prototype equipment templates plus Red Tonic reference |
| `fixture.fp_restart` | No prototype item or hostile entity survives restart |
| `fixture.fp_boss_phase_cancel` | Each phase threshold cancels exactly once |
| `fixture.fp_seed_trace` | Same seed/inputs produce identical spawn, attack, damage, drop ticks and state hashes |

Also required before the gate: `content_schema_and_semantics`, `minimum_speed_no_ability_safe_corridors`, `fp_seed_trace_and_restart`, deterministic movement/walls, every Arbalist primary/ability/passive, every used status/cap and pattern primitive, known-seed item/reward resolution, local death, a 60-second boss replay, and 20 complete boss runs.

Performance evidence MUST contain the target hardware description, build ID, bundle ID, fixture seed, duration, actual peak entities/projectiles, FPS, p95/p99 frame time, memory samples, effect mode, and raw report hash. Reduced effects may cull only priorities 5 then 4; hostile telegraphs are never culled.

## Ambiguity decisions and proposed defaults

These defaults prevent implementation agents from inventing behavior. A higher-authority decision or ADR may replace a proposed default; record that replacement explicitly.

| Topic | Decision for implementation |
|---|---|
| M01 HUD/menu ADR ID | Do not reuse or rewrite an existing ADR. The roadmap's architecture table names HUD/menu choice `ADR-002`, while the repository already uses ADR-002 for movement/camera. Create a `SPEC-CONFLICT` and allocate the next unused ADR ID for the HUD choice unless the owner assigns an ID. Existing historical ADRs remain immutable. |
| FP enemy values | Use `CONT-FP-004` payloads in `fp.1.0.0`. Do not use the later production `CONT-ENEMY-*` payloads. This is an explicit override, not a balance choice. |
| Baseline vs Arbalist movement | Use `5.1 tiles/s` for the player because `CLS-020` is class-specific. Retain `5.0` only as the generic baseline and validate the legal `4.5-5.6` resolved range. |
| Grave Mark radius/solid disposition | Continue the accepted ADR-005 decision: common `CONT-013` radius `0.12`; consume on solid; no mark or damage intent on solid/range expiry. |
| First shot and collision ordering | Preserve accepted ADR-003/004 behavior and goldens. Do not reinterpret cadence, range clamp, collision ties, or event order in later tickets. |
| Slipstep ordering | Preserve accepted ADR-006: Ability 1 wins same-tick GCD tie; Slipstep moves first segment before primary origin capture; empowerment expires before input on first ineligible tick; exact wall clamp/no slide; enemies do not body block. |
| Damage break for Stillness | Proposed default: apply lethal/nonlethal resolved health-or-barrier damage event, then break Focused in the same tick before later player actions. Zero-damage collision/grace events do not break Focused. Encode in the 02E ADR and golden before PASS. |
| Local death flow | Proposed default: on death, freeze the dead run, show the cause card immediately, and make `Run Again` the primary action; activation performs full cleanup and returns control within 3 seconds. Do not auto-skip the cause card. Boss victory uses the exact completion-summary behavior in `CONT-FP-010`. |
| Local telemetry identity | Proposed default: generate an ephemeral pseudonymous tester/session ID entered or created before the test; use `region_id="local"`, `environment="local_lab"`, bundle `fp.1.0.0`; store locally; export only the consented gate report; collect no account/email/name. Add `run_started` and `run_restarted` as versioned M01-local events because required gate behavior is otherwise uncorrelated. |
| M01 accessibility subset | Implement screen shake, flash intensity, reduced motion, high-contrast telegraphs, hostile shape/outline theme, and friendly opacity. Full keyboard rebinding UI, UI scale, text/chat scale, and controller certification remain later unless already required by a ticket. Mechanics must remain visible at every setting. |
| Debug time scale | Proposed values: paused single-step plus `0.25x`, `0.5x`, `1x`, and `2x`; default `1x`; fixed simulation still advances only whole ticks. Telemetry excludes non-1x runs from gate metrics. |
| Debug invulnerability | Proposed default: reject health reduction after collision/damage calculation while continuing to emit debug damage events marked `debug_invulnerable`; status application is also rejected; default off; exclude enabled runs from gate metrics. |
| Four behavior-change proof | Required proof set: Grave Repeater cadence, Scatterbow multi-bolt geometry, Mark Lens Mark duration/bonus, and Still Eye Focused timing/damage. Also test Slip Clasp and Undertaker Knot automatically. |
| Performance fallback | Gate report must show full-effects and documented reduced-effects results. M01 passes only if one supported mode holds 60 FPS at 1080p on target and never removes hostile telegraphs; clearly report which mode passes. |
| Determinism quantity | Minimum for M01: named FP seed/restart fixture, one identical 60-second boss replay, and 20 complete identical boss runs with no crash/soft lock/unavoidable pattern/hash mismatch. Run 100 solo traces if the general boss validator includes the prototype boss; never reduce the explicit 20-run gate. |
| Survey aggregation | Ask three separate 1-5 questions for movement, shooting, and dodging plus one overall combat-feel question. Require the median of each of the three and the overall median to be at least 4. Missing answers do not count as passes and the gate still requires 10 complete eligible surveys. |
| Asset count IDs | Before asset production PASS, create exact allowlists for 10 VFX, 12 audio, and five UI surfaces in the asset manifest with provenance. Do not invent unregistered extras to satisfy the count. |

## External tester gate

`GB-M01-GATE` is PASS only when every condition below is true on one immutable build/content pair:

| Gate | PASS threshold | Evidence |
|---|---|---|
| Eligible population | At least 10 complete testers not involved in the tested features | Cohort roster with pseudonymous IDs, exclusion attestation, genre-familiarity segment |
| Boot/control | Controllable combat in under 10 seconds on development target | Automated timing plus session logs |
| Feature completeness | Movement, aim, held primary, Grave Mark, Slipstep, Stillness, potion, damage, death, restart; 3 roles; 3-phase boss | Build checklist and ticket audits |
| Killer understanding | At least 8 of 10 correctly identify what killed them before detailed trace | Four-choice/open response stored with authoritative killer/pattern |
| Reroll desire | At least 7 of 10 both voluntarily restart and say they want another attempt | Restart telemetry joined to survey response |
| Feel | Median movement, shooting, dodging, and overall combat-feel score each at least 4/5 | Ten complete survey rows and aggregation script |
| Performance | Target PC maintains 60 FPS at 1080p in stress fixture | Reproducible fixture report and raw samples |
| Determinism/reliability | 20 complete boss runs without crash, soft lock, unavoidable pattern, or inconsistent state hash | Fixed-input run manifest and hash comparison |
| Content correctness | All named FP fixtures and strict content validation pass | Validator and fixture report hashes |
| Accessibility/readability | No unresolved hostile-projectile, safe-zone, exit, grayscale, or center-screen obstruction defect | Screenshot matrix and issue closure |

Human protocol:

1. Record build ID, `fp.1.0.0` manifest hash, tester cohort, and consent.
2. Give no verbal coaching beyond the in-game experience.
3. Observe first confusion, first damage, first item, first death, and restart action.
4. Ask the tester to identify the death cause before exposing the detailed trace.
5. Ask: `What felt distinctive?`, `What would make you stop?`, and `What do you want to do next?`.
6. Record observed behavior separately from opinion and segment by genre familiarity.
7. File each defect with reproduction, player impact, evidence, owner, and severity.
8. Publish a gate report with raw eligible denominators, medians, fixture hashes, failures, and a recorded `PASS` or `PENDING` decision.

If any condition fails, `GB-M01-GATE` remains `PENDING`. Continue focused combat, readability, onboarding-to-combat, performance, or death-cause tuning. No later milestone scope is authorized by elapsed time alone.

## Current completion summary

| Range | Current state | Evidence policy |
|---|---|---|
| `GB-M01-01A` through `GB-M01-02B` | PASS | Local audits plus recorded clean verification in their audit files |
| `GB-M01-02C` | PASS | Local gate passed; GitHub Actions intentionally excluded |
| `GB-M01-02D` | PASS | Local gate passed; GitHub Actions intentionally excluded |
| `GB-M01-02E` | PASS | Local implementation, deterministic tests, optimized runtime, and inspected evidence passed |
| `GB-M01-11` | PASS | Base/Undertaker/restart integration and deterministic audible cue implementation pass; physical listening accepted under the owner-assumed gate |
| `GB-M01-03A` through `GB-M01-03C` | PASS | Strict content/simulation, shared damage/death/drop integration, optimized inspected evidence, and local cumulative gate passed |
| `GB-M01-04A` | PASS | Shared primitives, named safe-route fixture, compact live debug overlay, optimized inspected evidence, and cumulative local gate passed |
| `GB-M01-04B` through `GB-M01-04C` | PASS | Exact 34-record Bell content, composite runtime, phase/break/death goldens, 20 complete runs, and optimized active/completion evidence pass locally |
| `GB-M01-05A` | PASS | Exact resolver/grace/lethal integration plus the corrected six-attack 128-health/armor-2 reference fixture pass; Bell fan remains Chip |
| `GB-M01-05B` | PASS | Separate normal/boss manifests, exposure, grayscale grammar, priorities, and boss aggregate `41 / 36-of-500` pass |
| `GB-M01-06A` | PASS | Atomic local death/freeze, complete cleanup census, run-qualified fresh reconstruction, under-three-second measurement, optimized inspected evidence, and cumulative local gate passed |
| `GB-M01-06B` | PASS | Authoritative death recap plus real boss reward/summary, clear/best metrics, primary Run Again, Escape/pause, and modal regression pass |
| `GB-M01-07A` | PASS | Exact spatial reach, deterministic placement/reward parity, restart ownership, keyboard overlay/actions, optimized inspected evidence, and cumulative local gate passed |
| `GB-M01-07B` | PASS | Exact typed catalog/rewards, deterministic goldens, multi-bolt target cap, four live behaviors, optimized inspected evidence, and cumulative local gate passed |
| `GB-M01-08A` | PASS | Normal Wave 1–3 and live Bell Proctor authoritative overlay/hash/evidence pass |
| `GB-M01-08B` | PASS | Whole-tick time controls, transactional invulnerability, metric exclusion, labeling, and full 08A dependency pass |
| `GB-M01-09` | PASS | Final executable-derived build ID; verified 1080p full/reduced rendered captures; full mode 156.802 FPS, p95 14.092 ms, p99 18.162 ms; 181-sample 30-minute memory pass at 309,800,960-byte peak; exact 800/40 and no hostile-cue culling |
| `GB-M01-10A` | PASS | Exact controls, real thick-outline geometry, hostile no-cull invariant, and inspected 720p/1080p surface/preset matrix pass |
| `GB-M01-10B` | PASS | Privacy-safe schema/order/export, live opt-in adapter, executable-derived build identity, runbook/template, and owner-assumed operated human gate pass |
| `GB-M01-GATE` | PASS | Automated and target-performance evidence passes; every human threshold is accepted by explicit owner assumption without fabricated raw cohort records |
