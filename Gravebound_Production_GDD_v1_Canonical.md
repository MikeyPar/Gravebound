# Gravebound: The Ashen Veil
## Production Game Design Document — AI-Implementable Baseline

| Field | Value |
|---|---|
| Document ID | `GB-GDD-001` |
| Version | `1.0.0` |
| Status | Approved baseline for prototype and Early Access development |
| Date | 2026-07-10 |
| Canonical format | Markdown; this file is the source of truth |
| Companion plan | `Gravebound_Development_Roadmap_v1.md` |
| Content production contract | `Gravebound_Content_Production_Spec_v1.md` |
| Replaces | `Gravebound_Ashen_Veil_GDD.html` for implementation decisions |
| Engine | Rust stable, Bevy 0.19, native Windows PC first |
| Initial business model | Free-to-play Early Access; cosmetics and supporter packs only |

---

## Contents

0. [How to use this document](#0-how-to-use-this-document)
1. [Product contract](#1-product-contract)
2. [Scope and release definitions](#2-scope-and-release-definitions)
3. [Player journey and loops](#3-player-journey-and-loops)
4. [Input, camera, and interaction](#4-input-camera-and-interaction)
5. [Simulation and fairness contract](#5-simulation-and-fairness-contract)
6. [Combat system](#6-combat-system)
7. [Classes and oaths](#7-classes-and-oaths)
8. [Character and account progression](#8-character-and-account-progression)
9. [Death, recall, extraction, and recovery](#9-death-recall-extraction-and-recovery)
10. [Items, loot, crafting, and storage](#10-items-loot-crafting-and-storage)
11. [Veil Bargains](#11-veil-bargains)
12. [Fallen Hero Echoes](#12-fallen-hero-echoes)
13. [Lantern Halls and the public realm](#13-lantern-halls-and-the-public-realm)
14. [Dungeon system](#14-dungeon-system)
15. [Enemies, patterns, and bosses](#15-enemies-patterns-and-bosses)
16. [Parties, contribution, and communication](#16-parties-contribution-and-communication)
17. [Economy](#17-economy)
18. [Monetization and commercial fairness](#18-monetization-and-commercial-fairness)
19. [UI, HUD, accessibility, and onboarding](#19-ui-hud-accessibility-and-onboarding)
20. [Art, animation, VFX, and audio](#20-art-animation-vfx-and-audio)
21. [Technical architecture](#21-technical-architecture)
22. [Content data and authoring](#22-content-data-and-authoring)
23. [Security, anti-cheat, privacy, and support](#23-security-anti-cheat-privacy-and-support)
24. [Telemetry and business model](#24-telemetry-and-business-model)
25. [Quality assurance and playtesting](#25-quality-assurance-and-playtesting)
26. [Live operations and content cadence](#26-live-operations-and-content-cadence)
27. [Release gates](#27-release-gates)
28. [Development roadmap summary](#28-development-roadmap-summary)
29. [AI implementation task template](#29-ai-implementation-task-template)
30. [Risks, deferrals, glossary, and references](#30-risks-deferrals-glossary-and-references)

---

## 0. How to use this document

### 0.1 Purpose

This document defines the product that is to be built. It intentionally makes concrete decisions so that a human or AI implementer can create features without inventing mechanics, changing scope, or resolving contradictions independently.

The companion roadmap defines implementation order, effort, milestone gates, and release criteria. If a feature is absent from both documents, it is out of scope until added through change control.

### 0.2 Normative language

- **MUST**: required for the feature to be accepted.
- **MUST NOT**: prohibited.
- **SHOULD**: expected unless an issue documents a specific technical reason.
- **MAY**: optional and safe to omit.
- Numeric values labeled **prototype default** MUST be implemented as data, not hard-coded, and may be changed only after playtest evidence.

### 0.3 Decision precedence

When specifications conflict, use this order:

1. Safety, legal, payment, and privacy requirements.
2. This GDD's explicit feature rules and acceptance criteria.
3. The companion content production specification's exact IDs and data records.
4. The companion roadmap's current milestone scope.
5. Versioned content data generated from the content production specification.
6. Existing code behavior.
7. The old GDD and concept documents.

An implementer MUST NOT silently choose between conflicting rules. Create an issue with prefix `SPEC-CONFLICT`, cite both locations, and stop only the conflicting work.

### 0.4 Stable identifiers

Every feature has an ID. Code modules, telemetry events, tests, content records, and work items SHOULD reference these IDs.

| Prefix | Domain |
|---|---|
| `PRD` | Product and scope |
| `LOOP` | Player journey and loops |
| `SIM` | Simulation and timing |
| `COM` | Combat |
| `CLS` | Classes and oaths |
| `PROG` | Character/account progression |
| `DTH` | Death, recall, and recovery |
| `LOOT` | Items, drops, crafting, and storage |
| `BRG` | Veil Bargains |
| `ECH` | Fallen Hero Echoes |
| `WRLD` | Open realm |
| `DNG` | Dungeons |
| `ENC` | Enemies and bosses |
| `SOC` | Party and social systems |
| `ECO` | Economy |
| `MON` | Monetization |
| `UI` | User interface and accessibility |
| `ART` | Art, animation, VFX, and audio |
| `TECH` | Architecture, networking, and persistence |
| `TEL` | Telemetry and analytics |
| `QA` | Testing and release quality |

### 0.5 Change control

Any change affecting permadeath, paid products, item loss, player power, drop odds, recall, hit detection, or leaderboard fairness requires:

1. A written change request with the affected feature IDs.
2. A player-impact statement.
3. Updated automated tests and telemetry.
4. Approval recorded in a decision log.
5. Migration behavior for live accounts and content.

### 0.6 AI implementation contract

For every implementation task, an AI agent MUST receive:

- Feature IDs in scope.
- Files or modules allowed to change.
- Content records or schema involved.
- Required tests.
- Acceptance criteria copied from this GDD.
- Explicit out-of-scope behavior.

The agent MUST:

- Reuse shared simulation rules rather than duplicate formulas.
- Add or update tests in the same change.
- Keep gameplay values in versioned data.
- Avoid adding dependencies without an architecture decision record.
- Avoid creating adjacent features that were not requested.
- Report unimplemented edge cases rather than inventing behavior.

The task template appears in Section 29.

---


## 1. Product contract

### PRD-001 — High concept

**Gravebound: The Ashen Veil** is a top-down 2D cooperative bullet-hell action RPG. Players create mortal heroes, enter a hostile shared realm, assemble behavior-changing equipment, descend into instanced dungeons, and permanently lose the active hero and carried combat equipment on death. The account persists as the Gravebound order: its memorials, discoveries, cosmetics, crafting knowledge, and the hostile Echoes left by fallen heroes remain.

### PRD-002 — Product promise

> Build a mortal hero, make dangerous bargains, survive readable bullet hell, and leave behind new content when the hero dies.

The game MUST feel:

- Playable within 45 seconds of login.
- Mechanically interesting before the first rare drop.
- Fair when lethal.
- Painful but immediately replayable after death.
- Social without requiring a schedule.
- Valuable at low population.
- Horizontally expandable without permanent stat inflation.

### PRD-003 — Signature differentiator: death creates content

Death is not only item destruction. Qualifying dead heroes create Fallen Hero Echo records used by memorial screens, personal Requiem encounters, and later world events. Echo rewards never restore the dead character or its destroyed combat items.

The signature loop is:

`Create hero -> build identity -> take bargains -> achieve deeds -> die -> create Echo -> confront legacy -> create next hero`

### PRD-004 — Primary audience

Primary audience:

- PC players who enjoy difficult action RPGs, bullet hells, roguelites, loot experimentation, and cooperative PvE.
- Players comfortable with loss when the rules are explicit and recovery is fast.
- Players who value mastery, collections, prestige cosmetics, and memorable co-op stories.

Secondary audience:

- Short-session MMO players.
- Build optimizers.
- Streamers and challenge runners.
- Social collectors.

The game is not designed for players seeking a low-risk power fantasy, AFK progression, PvP competition, or guaranteed preservation of equipped items.

### PRD-005 — Design pillars

1. **Every death has a legible cause.** No invisible, ambiguous, or monetized death reversal.
2. **Items alter verbs.** Important items change projectile behavior, ability timing, positioning, or risk rather than only raising damage.
3. **Death produces future play.** Memorials and Echoes turn a lost hero into account and world content.
4. **Fast back to agency.** Rerolling and reaching meaningful combat take seconds, not menus and chores.
5. **Shared danger, independent dignity.** Cooperation helps, but loot theft, body blocking, mandatory roles, and grief dragging are prohibited.
6. **Horizontal longevity.** New content expands strategies and challenges rather than invalidating old equipment each season.
7. **Fair commerce.** Payments never improve combat, drop value, death survival, ranked opportunity, storage, active-character capacity, market capacity, or recovery utility in any mode.

### PRD-006 — Definition of fun

A session succeeds when the player experiences at least three of the following:

- Reads and survives a dangerous pattern.
- Makes a build choice with a visible gameplay consequence.
- Chooses between safety and a clearly communicated reward.
- Helps or is helped by another player.
- Discovers a new enemy, item, room, bargain, or codex entry.
- Extracts something worth protecting.
- Dies with a comprehensible cause and wants to reroll.

### PRD-007 — Non-goals

The following are explicitly out of scope for Early Access launch:

- PvP.
- Player housing or player-owned hub decoration.
- A global auction house or unrestricted direct trade.
- Forty-player instanced dungeons.
- Mandatory tank/healer/damage group composition.
- Paid functional storage or active character slots.
- Paid character power, resurrection, insurance, keys, loot boosts, or experience boosts.
- Browser launch parity.
- Mobile or console clients.
- Voice chat.
- User-created executable content or mods on official servers.
- Fully freeform procedural boss arenas.
- An infrastructure requirement that assumes 10,000 real launch players.

### PRD-008 — Content rating and safety

- Target rating: Teen-equivalent dark fantasy.
- Blood may be stylized; graphic gore, sexual violence, gambling simulation, and torture detail are excluded.
- Text communication requires mute, block, report, profanity filtering, rate limits, and evidence capture before public release.
- Purchased randomized loot boxes are prohibited.

---


## 2. Scope and release definitions

### PRD-020 — Product stages

| Stage | Purpose | Required playable scope | Explicitly absent |
|---|---|---|---|
| First Playable | Prove movement, shooting, danger, loot, death, and instant restart | 1 class, combat arena, 3 enemies, 1 benchmark boss, 12 prototype equipment templates plus Red Tonic, local simulation | Accounts, networking, realm, crafting, store |
| Core Prototype | Prove a complete 15–25 minute life | 1 class, both oath choices, 1 small region, 1 dungeon, 1 boss, vault, deterministic death, 18 items | Public service, guilds, market, seasons |
| Networked Vertical Slice | Prove server authority and co-op clarity | 2 classes, 8-player realm, parties of 4, 2 dungeons, 2 bosses, account persistence, Echo prototype | Store, global chat, guilds, browser |
| Closed Alpha | Prove retention and economy with real users | 3 classes, Mire of Bells, 3 dungeons, 4 major encounters, 90 items, parties of 8, basic crafting and moderation | Paid products, market, season pass |
| Public Steam Playtest | Prove acquisition, onboarding, and live operations | Closed Alpha scope with polished first hour, analytics consent, crash reporting, Steam Playtest packaging | Purchases, permanent live economy |
| Early Access | First commercial release | 3 classes, 1 region, 3 dungeons, 4 major bosses, 90 items, Echoes, Veil Bargains, cosmetics/supporter pack, operations tools | Global market, browser, guilds, full seasons; functional paid utility is permanently prohibited |
| Version 1.0 | Stable live product | 5 classes, 2 regions, 6 dungeons, 8 major bosses, 180 items, first validated shared-world chapter, controller support | Guilds, trading, and 40-player dungeons unless each is separately validated |

### PRD-021 — Early Access content target

Early Access MUST NOT ship merely because the content count is met. It must meet all quality and retention gates in Sections 25 and 27.

| Content | Required quantity | Notes |
|---|---:|---|
| Classes | 3 | Vanguard, Arbalist, Witch; two oaths each; all abilities available from level 1 |
| Regions | 1 | Mire of Bells |
| Standard dungeons | 3 | Three difficulty bands; 1–8 players |
| World climax encounters | 1 | Bell Warden encounter |
| Dungeon bosses | 3 | One per dungeon |
| Minibosses | 6 | Shared by tagged biome pools |
| Normal enemies | 18 | At least 9 unique behavior families |
| Item templates | 90 | At least 24 behavior-changing templates |
| Black Uniques | 12 | Four per class or legal cross-class family mix |
| Veil Bargains | 12 | All are optional life-persistent boon/curse choices under BRG-001 |
| Cosmetic sets | 12 | At least half earnable through play |
| Tutorials/practice encounters | 5 | Training Crypt, three class tutorials, Mastery Trial; Practice Echo reuses the Requiem arena and does not add a sixth content asset |

### PRD-022 — Platform order

1. Windows native development build.
2. Windows Steam build.
3. Linux/Steam Deck evaluation after Closed Alpha.
4. Browser technical spike MAY occur after the native Networked Vertical Slice passes, but cannot enter the Early Access critical path.
5. Browser public release is reconsidered only after native Early Access stabilizes and memory, input, networking, browser-support, and business gates pass.

No feature may delay the First Playable merely to support a future browser build. Shared simulation and content data MUST remain platform-neutral.

### PRD-023 — Low-population requirement

All core progression MUST remain available with one player online. Specifically:

- Realms scale events to one participant.
- Standard dungeons scale from one to eight players.
- Realm climax access has a deterministic time fallback.
- No launch-critical reward requires a guild.
- No launch-critical item requires a market purchase.
- Matchmaking helps but never gates solo entry.

---


## 3. Player journey and loops

### LOOP-001 — First-launch flow

The exact first-launch sequence is:

1. Boot splash and asset verification.
2. Accessibility quick setup: contrast preset, screen shake, flash intensity, master volume.
3. Local profile or platform authentication.
4. Training Crypt.
5. Scripted tutorial death with no account loss.
6. Death summary explaining `lost`, `preserved`, and `created`.
7. Character creation from every class enabled in the current bundle: one choice in Core, two in Slice, and all three launch classes in Alpha or later.
8. Lantern Halls spawn facing the Realm Gate.
9. Guided Mire of Bells entry.
10. First event, first dungeon portal, and first real extraction prompt.

The tutorial death MUST be announced as a lesson before it occurs. It MUST NOT pretend to be a genuine unexpected loss.

Training Crypt uses a fixed, nonpersistent Grave Arbalist tutorial avatar with the starter primary, Grave Mark, Slipstep, Red Tonic, and no inventory persistence. Tutorial Recall teleports that avatar to the current training checkpoint rather than Lantern Halls. The player's real class is chosen only after the scripted tutorial death.

### LOOP-002 — Moment-to-moment loop

Every 3–10 seconds the player SHOULD perform this loop:

1. Read enemy origin, telegraph, and projectile lanes.
2. Move or stop to occupy a safe pocket.
3. Aim and fire primary attack.
4. Decide whether an ability creates enough value to justify its cooldown or commitment.
5. Reassess health, exits, party state, and new threats.

Primary fire may be held. Survival MUST require movement and pattern reading, not repeated clicking.

### LOOP-003 — Encounter loop

1. Enter an encounter boundary.
2. Observe a readable introduction of the primary mechanic.
3. Defeat pressure enemies or complete an objective.
4. Survive a combined pattern.
5. Receive a break window and reward feedback.
6. Choose to continue, inspect quickly, recall, or enter a portal.

### LOOP-004 — Character-life loop

1. Create a level-1 character with complete core abilities.
2. Reach level 10 and select an oath.
3. Reach level 20 and begin target farming.
4. Form a build through items, bargains, and crafting.
5. Attempt increasingly dangerous encounters.
6. Recall and bank stable rewards or continue for greater value.
7. Die permanently or retire voluntarily after a qualifying deed.
8. Record memorial and, when eligible, create a Fallen Hero Echo.
9. Start a new character using saved presets and account knowledge.

### LOOP-005 — Account loop

Account persistence includes:

- Vaulted equipment and materials.
- Cosmetics and wardrobe appearances.
- Codex discoveries.
- Recipes and crafting knowledge.
- Memorial records and Echo records.
- Class mastery achievements.
- Titles, banners, and profile presentation.
- Non-power quality-of-life unlocks earned through play.

It MUST NOT include permanent combat stats applied to all new characters.

### LOOP-006 — Session targets

| Session | Expected accomplishment |
|---|---|
| 5 minutes | Complete an event, gain levels, or enter a short dungeon |
| 15 minutes | Complete a dungeon or materially alter a build |
| 30 minutes | Reach an oath, defeat a boss, or secure a valuable extraction |
| 45 minutes | Complete a realm cycle or meaningful character chapter |
| 90+ minutes | Chain dungeons, play socially, pursue mastery or pinnacle goals |

### LOOP-007 — First-hour target timeline

| Elapsed time | Target experience |
|---:|---|
| 0:00–0:45 | Boot to actionable tutorial control |
| 0:45–4:00 | Learn movement, aim, attack, abilities, potion, and recall |
| 4:00–5:00 | Experience scripted training death and reroll explanation |
| 5:00–7:00 | Create real character and enter realm |
| 7:00–12:00 | Complete first public event and equip first behavior-changing item |
| 12:00–20:00 | Enter first dungeon with optional party |
| 20:00–27:00 | Defeat first dungeon boss and stabilize loot |
| 27:00–35:00 | Bank loot, salvage, and understand vault safety |
| 35:00–50:00 | Select first Veil Bargain or reach oath threshold |
| 50:00–60:00 | Pursue self-chosen goal: second dungeon, event chain, or safe logout |

---


## 4. Input, camera, and interaction

### SIM-001 — Units and coordinates

- One world tile equals `1.0` simulation unit.
- Canonical environment tile art is `32 × 32` source pixels.
- Positions use `f32` in simulation memory and quantized fixed precision on the wire.
- World origin is top-left for authored tile data; runtime transform conversion may use a centered render origin.
- Gameplay calculations MUST use simulation units, never source pixels or display pixels.

### SIM-002 — Camera

- Camera is top-down orthographic.
- Default visible world area at 16:9 is approximately `24 × 13.5` tiles.
- User zoom range is `20 × 11.25` to `30 × 16.875` tiles.
- Zoom cannot alter aggro, enemy activation, or projectile simulation.
- Camera centers on the local player with critically damped smoothing of `80 ms`.
- Camera shake is presentation-only and defaults to a maximum displacement of `0.18` tiles.
- Boss arenas may lock maximum zoom but MUST NOT zoom closer than the user's default.

### SIM-003 — Action map

| Action ID | Keyboard/mouse | Controller | Rules |
|---|---|---|---|
| `move` | WASD | Left stick | Normalized diagonal movement |
| `aim` | Mouse position | Right stick | Independent from movement |
| `primary_fire` | Left mouse | Right trigger | Hold allowed |
| `ability_1` | Right mouse | Left trigger | Class-defining offense/control |
| `ability_2` | Space | Left bumper | Defense/mobility/support |
| `consumable_1` | Q | X / Square | Belt slot 1 |
| `consumable_2` | E | Y / Triangle | Belt slot 2 |
| `interact` | F | A / Cross | Portal, shrine, NPC, confirm world interaction |
| `recall` | Hold R | Hold View/Back | Rules in DTH-010 |
| `ping_wheel` | Middle mouse | Right bumper | Directional/context ping |
| `inventory` | I or Tab | D-pad Up | Overlay; world never pauses online |
| `map` | M | D-pad Left | Overlay |
| `pause_menu` | Escape | Menu/Start | Menu; world never pauses online |

All actions MUST be rebindable. Destructive or irreversible actions require confirmation if performed outside combat urgency.

### SIM-004 — Input processing

- Client samples input each rendered frame.
- Client sends the latest compact action state at `30 Hz`.
- Server simulates inputs at a fixed `30 Hz`.
- Primary fire and ability presses include sequence numbers.
- Ability inputs are buffered for up to `100 ms` before cooldown completion.
- Recall start and cancel are reliable events.
- A menu that captures pointer or keyboard input MUST block combat actions but MUST NOT pause the server.

### SIM-005 — Movement defaults

- Baseline player movement speed: `5.0 tiles/second`.
- Baseline input-direction acceleration/deceleration smoothing: `60 ms`; this changes direction response, not final speed.
- Legal final movement speed range in ordinary builds: `4.5–5.6 tiles/second`.
- Diagonal input is normalized to the same magnitude as cardinal input.
- Player hurtbox radius: `0.25 tiles`.
- Player physical collision radius: `0.30 tiles`.
- Players do not physically collide with other players.
- Players may collide with authored walls and solid enemy bodies.
- Enemies MUST NOT push a player through a solid wall.

### SIM-006 — Interaction priority

When multiple interactables overlap, priority is:

1. Explicit confirmation dialog.
2. Dungeon or realm exit.
3. Loot stabilization object.
4. Veil Bargain shrine.
5. Portal.
6. NPC.
7. Lore or cosmetic object.

The selected target receives an outline and label before activation.

---


## 5. Simulation and fairness contract

### SIM-010 — Fixed simulation

- Authoritative simulation tick: `30 Hz`.
- Client target render rate: `60 FPS`; uncapped option permitted.
- Server owns position, health, status, projectiles, AI, drops, inventory mutation, death, recall completion, and rewards.
- Client predicts only its own movement and immediate presentation of locally initiated attacks.
- Server result is final.
- Simulation time uses integer ticks. Content durations are authored in milliseconds and ordinarily compile using round-to-nearest. Hostile telegraph durations and any authored fairness minimum compile with ceiling so their simulated duration is never shorter than the specified value.

### SIM-011 — Determinism

- Enemy patterns use a pattern ID, content version, start tick, origin, parameters, and 64-bit seed.
- Server and client may reproduce visual projectile paths from the same descriptor.
- Server collision simulation is authoritative even if client rendering diverges.
- Content tests MUST replay a known seed and compare deterministic state hashes at selected ticks.
- Randomness affecting rewards is generated only on the server and logged with a non-reversible audit reference.

### SIM-012 — Latency target

- Primary design latency: `80 ms` round-trip.
- Required fair-play test latency: `120 ms` round-trip with `1%` packet loss.
- Degraded but supported: `180 ms`; UI shows warning.
- Above `250 ms` sustained for 5 seconds: UI shows severe warning and disables entry confirmation for sealed pinnacle arenas unless the player explicitly overrides.
- The game MUST show a compact network-health indicator during combat.

### SIM-013 — Spawn safety

- Players entering a realm or dungeon receive `2.0 seconds` of invulnerability.
- Invulnerability ends immediately on primary fire, ability use, consumable use, or movement beyond `1.0 tile` from spawn.
- No hostile entity or hostile projectile may be active within `3.0 tiles` of an entry spawn.
- Dungeon rooms MUST NOT activate before all connected party members finish loading or `10 seconds` elapse; late members remain at a safe vestibule.

### SIM-014 — Content versioning

- Every live simulation instance pins one immutable content bundle version.
- Hot configuration changes apply only to new instances unless explicitly marked presentation-only.
- Characters may move between instances only after inventory changes are committed.
- A content rollback MUST NOT reinterpret already-created item affixes. Item migrations require an explicit migration version.

---


## 6. Combat system

### COM-001 — Combat model

Combat is real-time, top-down, projectile-heavy PvE. Skill is expressed through movement, aim, pattern reading, target priority, timing, and choosing when not to attack.

Rules:

- No friendly fire.
- Enemy projectile collision uses the player hurtbox, not sprite bounds.
- A hostile projectile is consumed on a successful player hit unless its content record explicitly has `pierces_players: true`.
- Players receive a `100 ms` global hostile-projectile hit grace after a projectile hit. Ground hazards and damage-over-time effects use their own tick rules and do not consume this grace.
- A nonpiercing projectile colliding during projectile grace is consumed and deals zero damage. A piercing projectile records that player as grace-ignored, continues, and can never damage that player later in its lifetime. Both emit `projectile_grace_ignored` in debug/telemetry.
- Critical hits do not exist in Early Access. No item, class, Bargain, or hidden RNG may create a critical hit; a future crit system requires a separate combat revision.
- Passive random evasion, cheat death, and automatic resurrection are prohibited.

### COM-002 — Damage resolution

Resolve a direct hit in this order:

1. Validate source, target, collision, and immunity.
2. Read `raw_damage` and `damage_type`.
3. Apply attacker multipliers.
4. Apply target resistance for the damage type, capped to `25%` unless a temporary ability explicitly raises the cap.
5. Apply the strongest valid directional/self/ally damage-reduction effect. Reductions do not stack; Guard and allied shelter therefore use the stronger value.
6. Apply armor mitigation: `armor_reduction = min(armor, damage_after_reduction × 0.35)`.
7. Round post-armor damage half-up to an integer with a minimum of `1` when the hit remains positive.
8. Absorb `min(current_barrier, post_armor_damage)` and reduce the barrier by that amount.
9. Apply an explicit content health-damage cap if and only if the validated attack record declares one; ordinary attacks declare none. Personal Requiem Echo attacks declare `35% target max health` under ECH-003.
10. Apply the remaining damage to health. A fully absorbing barrier permits `0` health damage.
11. Emit one damage event containing every intermediate value and any cap reduction.
12. If health is `0`, resolve death in the same tick before later actions.

Formula:

```text
resisted = raw_damage × (1 - clamp(resistance, -0.25, 0.25))
reduced = resisted × (1 - strongest_valid_damage_reduction)
armor_reduction = min(armor, reduced × 0.35)
post_armor = max(1, round_half_up(reduced - armor_reduction))
barrier_absorbed = min(current_barrier, post_armor)
health_damage = post_armor - barrier_absorbed
health_damage = min(health_damage, declared_health_damage_cap_or_infinity)
```

Negative resistance increases damage. Total positive resistance from ordinary gear is capped at `25%`. Armor is intentionally unable to erase major hits.

Resolved-stat caps after class, oath, equipment, Bargain, and status calculations:

- Persistent movement: `4.5–5.6 tiles/second`; temporary Frostbind may reduce to `4.0`.
- Primary attack interval: no lower than `70%` of the weapon template interval.
- Ability cooldown: no lower than `60%` of authored cooldown and never below `2.0 seconds` unless the ability explicitly has charges and passes review.
- Maximum-health multiplier from all multiplicative losses: no lower than `0.70` of the class's level-adjusted base health.
- Generic all-damage bonus from nonconditional modifiers: maximum `+50%`.
- Incoming-damage multiplier from voluntary risk effects: maximum `1.35`.
- Damage reduction granted by other players: maximum `35%`; strongest applies rather than stacking.

### COM-003 — Player damage categories

Enemy attacks are tagged for tuning and telemetry:

| Category | Final damage as target max-health share | Use |
|---|---:|---|
| Chip | 1–8% | Sustained pressure; may be dense |
| Pressure | >8–18% | Requires attention; common boss projectile |
| Major | >18–35% | Strong telegraph and limited overlap |
| Severe | >35–60% | Rare; clear audiovisual warning |
| Execution | >60% | Only sealed pinnacle mechanics with at least 1,000 ms warning |

Standard Early Access content MUST NOT contain Execution attacks. No standard attack may remove more than 60% of a legal minimum-health build after mitigation.

### COM-004 — Healing and sustain

- Self-healing from an ability may restore at most `20%` max health per activation.
- Party healing applies full value from the strongest heal received in a `1.0 second` window, `50%` from the second, and `0%` from further external heals.
- A character may receive at most `35%` max health from external healing in any rolling `10 second` window.
- Health potion restores `30%` max health over `0.4 seconds` and has a `2.0 second` shared potion cooldown.
- Taking damage does not cancel potion healing.
- Lifesteal is converted to a fixed on-hit heal with an internal cooldown; uncapped percentage lifesteal is prohibited.

### COM-005 — Telegraph contract

Every hostile attack MUST define:

- Origin silhouette or ground origin.
- Shape cue.
- Color family.
- Audio cue for Major, Severe, or Execution damage.
- Telegraph duration.
- Projectile or hazard lifetime.
- Counterplay verb.

Minimum timings:

| Attack | Minimum first-use telegraph | Minimum repeated-use telegraph |
|---|---:|---:|
| Chip | 250 ms | 200 ms |
| Pressure | 400 ms | 300 ms |
| Major | 650 ms | 500 ms |
| Severe | 900 ms | 750 ms |
| Execution | 1,200 ms | 1,000 ms |

An attack may use the repeated-use value only after the same encounter has shown the complete mechanic once.

### COM-006 — Pattern fairness budgets

All patterns MUST be validated at player speed `4.5 tiles/second`, hurtbox radius `0.25`, `120 ms` round-trip latency, and no movement ability.

If an encounter can apply Frostbind while another mandatory pattern remains active, that combination MUST also pass at `4.0 tiles/second`; otherwise the content compatibility validator must prohibit the overlap.

- Normal content minimum traversable safe corridor: `0.80 tiles`.
- Pinnacle content minimum corridor: `0.65 tiles`.
- No hostile projectile may spawn within `1.25 tiles` of a player unless a ground telegraph has existed for at least `750 ms`.
- No newly spawned projectile may reach a current player position in less than `350 ms`, except a visibly repeating beam with an inactive warning phase.
- Standard encounter logical hostile projectile cap: `300` in the local interest area.
- Boss encounter cap: `500`.
- Friendly projectile opacity defaults to `35%` for other players and is user-adjustable from `10–60%`.
- Hostile projectiles always render above friendly projectiles, loot beams, and decorative particles.
- Pattern combinations MUST declare compatibility tags. The server MUST reject forbidden combinations.

Corridor width is measured edge-to-edge between opposing hostile projectile/hazard boundaries after projectile radii are applied. The Standard validator must find a continuous player-center path at least `0.40 tiles` from each boundary (`0.25` hurtbox radius plus `0.15` clearance). Pinnacle validation uses `0.325 tiles` per side. Decorative sprite bounds do not participate.

### COM-007 — Status effects

| Status ID | Effect | Default duration | Rules |
|---|---|---:|---|
| `bleed` | Damage equal to configured value every 30 ticks | 3 s | Cannot reduce below 1 HP in tutorial; lethal elsewhere |
| `frostbind` | `-20%` movement speed | 2.5 s | Final speed never below 4.0 tiles/s |
| `silence` | Disables Ability 1 and Ability 2 | 2 s | Never disables movement, potion, or recall |
| `hex` | `+15%` curse damage received and `-25%` healing received | 5 s | Refreshes duration; does not stack magnitude |
| `guardbreak` | Armor and guard effectiveness `-35%` | 4 s | Clear broken-shield icon |
| `radiance` | `+20%` healing received; removes one minor negative status on apply | 5 s | Positive status |
| `exhaustion` | Disables movement-skill recast | 2 s | Does not reduce normal movement |
| `marked` | Enables a personal boss pattern | Encounter-defined | Ground ring and overhead icon mandatory |

Status magnitude from duplicate sources does not stack unless the status record explicitly defines stacks. Duration refresh behavior is data-driven.

Player-applied status rules by target are exact:

| Status | Normal enemy | Elite/miniboss | Major boss/Fallen Hero Echo |
|---|---|---|---|
| Bleed | Full | Full | `50%` authored tick damage; normal duration |
| Frostbind | `-20%` move | `-10%` move; duration `50%` | Movement-immune; application damage still occurs; show `IMMUNE` |
| Silence | Immune; NPC AI has no player ability slots | Immune | Immune |
| Hex | Full | Full | Full curse-damage vulnerability and healing penalty |
| Guardbreak | Full | `-20%` armor/guard effectiveness for `50%` duration | `-10%` armor, no guard-effectiveness change, `2 s` maximum |
| `graveled_mark` | Full | Full | Full Mark direct damage and owner primary bonus; one Mark per Arbalist |

All Veil Witch primary, Hex Bloom, and Withering Echo damage has tag `curse_damage`; Hex's `+15%` applies only to damage carrying that tag. Damage type remains separately authored for resistance. Nailkeeper/Bellwire may trigger on bosses and deal damage, but their Frostbind movement component follows the table. Encounter `marked` is unrelated to `graveled_mark`.

### COM-008 — Ability commitment types

Each active ability MUST use at least one commitment type:

- Cooldown.
- Charge.
- Cast time.
- Recovery time.
- Resource cost.
- Position lock.
- Self-exposure or health cost.

No ability may provide more than `750 ms` continuous invulnerability. Total invulnerability from all sources is capped at `1.0 second` per rolling `8 seconds`.

### COM-009 — Combat acceptance criteria

- At 120 ms simulated latency, predicted local movement reconciles without a visible correction greater than `0.35 tiles` in 99% of ordinary samples.
- Every damaging object can be rendered with hitbox debug display.
- A deterministic test can replay a boss pattern for 60 seconds with identical server hashes.
- Accessibility presets preserve distinct hostile attack shapes in grayscale.
- A minimum-speed, no-ability test bot can survive every mandatory Standard pattern when executing the authored safe path.
- Damage telemetry records source, pattern, raw damage, mitigated damage, target state, latency, and content version.

---

## 7. Classes and oaths

### CLS-001 — Class model

At Early Access, the playable classes are:

1. Ashen Vanguard — close-range guard and retaliation.
2. Grave Arbalist — precision range and controlled repositioning.
3. Veil Witch — delayed area control and curse interactions.

Every class has:

- One permitted weapon family.
- Primary fire supplied by the equipped weapon.
- Ability 1 available at level 1.
- Ability 2 available at level 1.
- One passive available at level 1.
- A choice between two oaths at level 10.

There are no mandatory group roles. Every class MUST complete all Standard content solo using legal self-found equipment.

### CLS-002 — Shared class formulas

- Level damage multiplier: `1.0 + 0.015 × (level - 1)`.
- Weapon hit damage: `weapon_damage × level_damage_multiplier × outgoing_modifiers`.
- Ability damage values are authored as a multiplier of `W`, where `W` is the equipped weapon's deterministic displayed hit damage before attack-rate adjustments.
- Ability raw damage is `ability_coefficient × W × level_damage_multiplier × ability_specific_modifiers × outgoing_modifiers`. `W` uses the content specification's item-level/template/rarity/affix formula and excludes character level and character-wide outgoing modifiers. Ordinary production weapons have no per-hit or instance base-damage roll.
- Global cooldown after an ability begins: `150 ms`.
- A character may change oath only in Lantern Halls.
- First oath change per character is free. Later changes cost `40 Ash Shards` each.
- Oath changes are rejected while inventory or character mutations are pending.

### CLS-010 — Ashen Vanguard

| Property | Prototype default |
|---|---:|
| Starting max health | 160 |
| Health per level after 1 | 6 |
| Starting armor | 6 |
| Armor growth | +1 at levels 5, 9, 13, and 17 |
| Base movement speed | 4.8 tiles/s |
| Weapon family | Sword |

**Primary weapon grammar — Sword**

- Default attack is an `80°` arc centered on aim direction.
- Default range is `2.1 tiles`.
- Default attack rate is `1.6 attacks/second`.
- An enemy may be hit once per swing.
- A visible windup of at least `100 ms` precedes the damaging arc.

**Ability 1 — Guard Arc**

- Cooldown: `6.0 seconds`.
- Active duration: `600 ms`.
- Guard angle: `120°` centered on aim direction.
- Incoming direct projectile damage from the guarded direction is reduced by `75%` before armor.
- Guard does not affect ground hazards, status damage, or projectiles striking from outside the arc.
- The first successful guarded hit stores one Retort charge for `2.0 seconds`.
- Releasing Guard or reaching duration end consumes the charge and emits a `2.5 tile`, `90°` retaliation arc dealing `1.5W`.
- Only one Retort charge may exist.

**Ability 2 — Cinder Rush**

- Cooldown: `7.0 seconds`.
- Travel: `2.4 tiles` over `260 ms` toward movement input, or aim direction if movement input is neutral.
- Grants `40%` direct-damage reduction during travel; it does not grant invulnerability.
- Stops at solid collision and may not cross closed doors or void tiles.
- Deals `0.75W` to each enemy crossed, once per cast.
- Applies Exhaustion for `1.5 seconds`.

**Passive — Last Ember**

- After receiving a Major or Severe direct hit, gain `+10 armor` for `2.0 seconds`.
- Internal cooldown: `10 seconds`.
- Does not trigger from self-costs or damage over time.

**Oath — Bell Retort**

- Guard Arc may store up to two Retort charges.
- Each charge after the first adds `0.65W` and `0.25 tiles` to the retaliation.
- Guard Arc cooldown increases by `1.0 second`.
- Cinder Rush damage is reduced by `25%`.

**Oath — Ashen Bastion**

- Guard Arc creates a `1.5 tile` ally shelter behind the Vanguard for its active duration.
- Allies in the shelter receive `25%` projectile-damage reduction.
- Vanguard outgoing damage is reduced by `10%`.
- Multiple Bastion shelters do not stack; the strongest applies.

### CLS-020 — Grave Arbalist

| Property | Prototype default |
|---|---:|
| Starting max health | 120 |
| Health per level after 1 | 4 |
| Starting armor | 2 |
| Armor growth | +1 at levels 7 and 14 |
| Base movement speed | 5.1 tiles/s |
| Weapon family | Crossbow |

**Primary weapon grammar — Crossbow**

- Default attack is one narrow bolt.
- Default range is `9.5 tiles`.
- Default attack rate is `2.2 attacks/second`.
- Default projectile radius is `0.10 tiles`.
- Bolts stop on first enemy unless the weapon explicitly grants pierce.

**Ability 1 — Grave Mark**

- Cooldown: `5.0 seconds`.
- Fires one `12 tiles/second` bolt with `11 tile` range.
- Deals `1.8W` and applies `graveled_mark` for `4.0 seconds`.
- Arbalist primary hits against the marked target deal `+15%` damage.
- Only one target per Arbalist may be marked; applying a new mark removes the old one.

**Ability 2 — Slipstep**

- Cooldown: `8.0 seconds`.
- Travel: `2.0 tiles` over `180 ms` in movement direction or backward from aim if movement is neutral.
- Grants `25%` direct-damage reduction during travel; no invulnerability.
- The next primary shot within `1.5 seconds` gains `+30%` projectile speed and one pierce.
- Applies Exhaustion for `1.5 seconds`.

**Passive — Stillness**

- After movement magnitude remains below `20%` for `600 ms`, gain Focused.
- Focused grants `+10%` projectile speed and `+8%` primary damage.
- Focused ends immediately when movement exceeds `20%`, Slipstep begins, or damage is received.

**Oath — Long Vigil**

- Focused activates after `350 ms` rather than `600 ms`.
- Grave Mark range increases by `2 tiles` and mark bonus becomes `20%`.
- Max health is reduced by `10%`.

**Oath — Nailkeeper**

- Grave Mark creates a `1.25 tile` nail trap at the first enemy or wall impact.
- Trap arms after `400 ms`, lasts `5 seconds`, deals `0.9W`, and Frostbinds for `1.5 seconds`.
- Maximum two active traps per Arbalist; oldest is removed when a third is created.
- Primary attack rate is reduced by `8%`.

### CLS-030 — Veil Witch

| Property | Prototype default |
|---|---:|
| Starting max health | 112 |
| Health per level after 1 | 4 |
| Starting armor | 1 |
| Armor growth | +1 at levels 10 and 18 |
| Base movement speed | 5.0 tiles/s |
| Weapon family | Hex Focus |

**Primary weapon grammar — Hex Focus**

- Default attack is one orb.
- Default range is `7.5 tiles`.
- Default attack rate is `1.7 attacks/second`.
- On first enemy or range expiration, orb bursts in a `0.65 tile` radius.
- Main target receives full damage; other targets receive `65%`.

**Ability 1 — Hex Bloom**

- Cooldown: `6.5 seconds`.
- Target range: `7 tiles`.
- Telegraph: `600 ms`, radius `1.6 tiles`.
- Active duration: `4.0 seconds`.
- Deals `0.30W` every `500 ms` to enemies in the area and applies Hex for `2.0 seconds`.
- One Witch may own at most two active Blooms.

**Ability 2 — Veil Fold**

- Cooldown: `9.0 seconds`.
- Teleports up to `2.5 tiles` toward the target position.
- Destination must be walkable and visible to the server.
- If requested destination is illegal, choose the furthest legal point on the segment; if none is at least `0.5 tiles` away, the cast fails and spends no cooldown.
- Grants `250 ms` invulnerability beginning on accepted server activation.
- Applies Exhaustion for `2.0 seconds`.

**Passive — Withering Echo**

- When a Hexed non-boss enemy dies, it bursts after `250 ms` for `0.45W` in `0.8 tiles`.
- A target may be hit by at most one Withering Echo from the same Witch every `500 ms`.
- Boss phase adds do not chain more than one generation.

**Oath — Orchard Rot**

- Hex Bloom duration increases to `5 seconds`.
- The final tick deals `0.8W` and heals the Witch for `3%` max health per enemy hit, capped at `9%` per Bloom.
- Veil Fold cooldown increases by `1.5 seconds`.

**Oath — Saltglass**

- Primary orbs reflect once from solid walls if no enemy was hit.
- Reflected orb damage is `70%` and burst radius is `0.8 tiles`.
- Hex Bloom radius is reduced by `20%`.
- Max health is reduced by `8%`.

### CLS-040 — Class acceptance criteria

- Each class completes every Standard boss solo using median item rolls and no Black Unique.
- Each class contributes meaningful damage in an eight-player party without a mandatory support class.
- No oath increases simulated median solo damage or effective health by more than `15%` over the other oath when using its intended play pattern.
- All movement abilities respect walls, sealed gates, Exhaustion, and server authority.
- Class tutorial rooms explain each ability with one required action and one survival test.
- Class death and clear rates are segmented by player experience; a controlled difference above `20%` from median requires investigation.

---


## 8. Character and account progression

### PROG-001 — Character levels

- Level cap: `20`.
- Core abilities exist at level 1.
- Oath selection unlocks at level 10.
- Level 20 unlocks no direct stat spike; it marks access to endgame objectives.
- XP is character-specific and lost on death.
- Account XP does not grant combat stats.

### PROG-002 — Cumulative XP thresholds

| Level | Total XP | Level | Total XP |
|---:|---:|---:|---:|
| 1 | 0 | 11 | 3,300 |
| 2 | 100 | 12 | 4,000 |
| 3 | 250 | 13 | 4,800 |
| 4 | 450 | 14 | 5,700 |
| 5 | 700 | 15 | 6,700 |
| 6 | 1,000 | 16 | 7,800 |
| 7 | 1,350 | 17 | 9,000 |
| 8 | 1,750 | 18 | 10,300 |
| 9 | 2,200 | 19 | 11,700 |
| 10 | 2,700 | 20 | 13,200 |

### PROG-003 — XP awards

| Source | Base XP |
|---|---:|
| Outer Reeds or Tier I normal enemy | 5 |
| Sunken Parish or Tier II normal enemy | 10 |
| Bellmarsh Heart or Tier III normal enemy | 15 |
| Realm elite | 60 |
| Tier I / II / III dungeon miniboss | 120 / 220 / 350 |
| Minor event | 120 |
| Major event | 300 |
| Tier I dungeon boss | 450 |
| Tier II dungeon boss | 800 |
| Tier III dungeon boss | 1,200 |
| Bell Warden world climax | 1,500 |
| First account clear of boss | +50% once |

Capture the enemy's XP band at spawn; moving it across a world boundary cannot change XP. An ordinary enemy grants full, undivided personal XP only to a living player within `16 tiles` at death who, during the prior `10 seconds`, dealt at least `1` actual health damage to that enemy or delivered effective healing/damage prevention to a player who did. A pack member may qualify independently. Elite, miniboss, event, and boss XP uses SOC-010 presence/contribution eligibility and the source's authored encounter boundary rather than the 16-tile rule. A first-account-clear bonus is `round_half_up(base_xp × 0.50)`, granted once per exact boss ID. Requiem, practice, tutorial, summoned `no_reward` entities, and Echo/modifier adds grant `0 XP`.

### PROG-004 — Mastery fast-track

After an account reaches level 20 and completes one Tier II dungeon on a class:

- New characters of that class may choose the Mastery Trial.
- The trial is a fixed-loadout, no-loot, no-permadeath five-minute encounter.
- Passing creates the character at level 10 with starter equipment and no oath selected.
- Trial attempts have no daily limit.
- This feature is earned only and cannot be purchased.
- Ranked fixed-start challenges may require level 1 regardless of mastery.

### PROG-005 — Account persistence

Persistent account records:

- Class unlocks; all launch classes are free and initially unlocked.
- Class mastery and trial eligibility.
- Vault contents.
- Currency wallets.
- Recipe discoveries.
- Codex entries.
- Cosmetics and entitlements.
- Memorials and Echoes.
- Settings and saved character appearance presets.

Account persistence MUST NOT add global max health, armor, damage, movement speed, drop chance, or death protection.

### PROG-006 — Voluntary retirement

- A living level-20 character with at least one Tier III boss clear may retire in Lantern Halls.
- Retirement requires zero pending inventory. `CharacterSafe` items are atomically auto-deposited to lowest empty Vault slots before confirmation; if the Vault lacks capacity, retirement is rejected with `storage_full`. Overflow/ResolutionHold are extraction-only and cannot be used to prepare retirement.
- Retirement destroys equipped combat gear only after two confirmations; pending inventory cannot exist in the Hall.
- Vault items are unaffected.
- Retirement creates a memorial tagged `retired`, not an Echo.
- Retirement awards the same cosmetic memorial credit as a qualifying death but no death-cause achievements.
- Retirement cannot be reversed by support.

### PROG-007 — Functional-build metric

The recovery KPI uses one deterministic definition:

```text
effective_item_level =
  item_level + rarity_bonus

rarity_bonus:
  Worn 0
  Forged 0.5
  Oathed 1
  Relic 2
  Sainted 3
  Black Unique 3

functional_power_score =
  0.35 × weapon_effective_level
  + 0.25 × relic_effective_level
  + 0.25 × armor_effective_level
  + 0.15 × charm_effective_level
```

A character has a **functional build** when all are true:

- Character is level 20.
- All four equipment slots contain nonstarter items.
- `functional_power_score ≥ 12.0`.
- Character is not in Practice mode or an administrative/test state.

Recovery time begins at final death commit and ends on the first authoritative tick satisfying this definition.

---


## 9. Death, recall, extraction, and recovery

### DTH-001 — Normal death transaction

When authoritative health reaches zero in a permadeath-enabled instance, the server MUST atomically:

1. Reject later character actions.
2. Record the lethal damage event and prior 10 seconds of combat trace.
3. Mark character state `dead` with immutable death ID.
4. Destroy all equipped combat items and remaining equipped belt consumables.
5. Destroy all pending RunBackpack inventory and every run-material-pouch stack.
6. Preserve vault items, currencies, cosmetics, codex, recipes, mastery, and settings.
7. Create a memorial record.
8. Create an Echo record if eligibility rules pass.
9. Commit item-destruction ledger events and character death in one idempotent transaction.
10. Send the death summary only after durable commit succeeds.

There is no corpse recovery, random salvage, item insurance, resurrection, or support restoration except verified systemic server faults under a published incident policy.

### DTH-002 — Non-permadeath contexts

Permadeath is disabled only in:

- Training Crypt.
- Class tutorials.
- Mastery Trial.
- Explicit Practice Echoes with zero gameplay rewards.
- Developer and automated test environments.

The HUD MUST display a persistent `PRACTICE — NO LOOT / NO PERMADEATH` label in such contexts.

### DTH-010 — Emergency Recall

- Input: hold Recall for `400 ms`.
- Player may move at `75%` speed while channeling.
- Player cannot fire, cast, interact, or use consumables while channeling.
- Damage does not cancel Recall.
- If health reaches zero before the completion tick, death wins.
- On completion, the server moves the character to Lantern Halls.
- Equipped items and remaining belt consumables survive Recall.
- All pending RunBackpack inventory and every run-material-pouch stack are destroyed.
- Recall has no currency cost or daily limit.
- Recall remains available in every Early Access dangerous area, including all boss arenas. Future sealed contracts are governed by DTH-012 and are not active Early Access content.
- The client MUST show channel progress and whether pending items will be abandoned.

### DTH-011 — Successful extraction

Successful extraction occurs by:

- Walking through the normal realm return gate.
- Completing a dungeon and using its exit portal.
- Using an authored extraction lantern after completing its encounter requirement.
- Clean server retirement during planned maintenance.

On extraction:

1. Pending run inventory becomes extracted inventory.
2. Extracted items become eligible for vault deposit, salvage, crafting, or permitted gifting.
3. Character and item state commit before instance transfer.
4. If inventory capacity is exceeded, excess items enter the Overflow Cache defined in LOOT-050.

If normal safe storage and Overflow are both full, remaining extracted items enter `ResolutionHold`. Extraction still succeeds and no accepted item is deleted. The character cannot enter danger, craft, gift, or perform other inventory mutations until every held item is moved to legal storage, salvaged, or explicitly destroyed. ResolutionHold has no expiry and is not player-usable storage.

### DTH-012 — Sealed arena contract (post-Early-Access only)

Early Access contains no Recall lock. This contract is reserved for a future optional difficulty feature and MUST remain disabled until a separate playtest/release decision approves it.

Before entering a sealed arena, the player sees a gate panel containing:

- Encounter name and difficulty.
- `Recall unavailable after entry` warning.
- Recommended item-power band.
- Current network health.
- Practice availability.
- Confirm and cancel actions.

The seal activates only when the player or party confirms. Party members who decline remain outside and are not teleported in. The arena restores Recall after boss defeat, encounter reset, or verified server fault.

### DTH-020 — Death summary

The death screen MUST show, in this order:

1. Hero name, class, level, lifetime, and final deed.
2. Killer, attack name, damage, damage type, and source position.
3. Last five damage events as a timeline.
4. Network state at death.
5. `Lost`: equipped items and pending inventory.
6. `Preserved`: account records, currency, vault, cosmetics, recipes.
7. `Created`: memorial and Echo status.
8. Primary action: `Create successor`.
9. Secondary actions: inspect trace, memorial, return to character select.

The primary action creates a legal successor from the last appearance/class preset and enters character select in at most two confirmations.

### DTH-021 — Recovery targets

Prototype targets:

- Death screen interactive within `2 seconds` of durable death commit.
- Death to successor control: median under `15 seconds`, p95 under `30 seconds`.
- Experienced death to functional build: median `30–45 minutes`.
- At least `70%` of eligible deaths followed by successor entering combat within `120 seconds`.
- At least `80%` of surveyed players correctly identify the lethal cause.
- At least `70%` rate the death `4/5` or better for fairness.

---


## 10. Items, loot, crafting, and storage

### LOOT-001 — Equipment slots

Early Access uses four equipment slots:

| Slot | Function |
|---|---|
| Weapon | Defines primary attack geometry, cadence, range, and base damage |
| Relic | Modifies one or both class abilities or passive |
| Armor | Health, armor, resistance, and mobility tradeoffs |
| Charm | Conditional offense, utility, status, or risk/reward behavior |

Additional combat slots are prohibited before Version 1.0 unless playtests show inadequate build depth and UI complexity remains acceptable.

### LOOT-002 — Inventory state axes

Every item instance stores three independent axes:

```text
location:
  Equipped(slot)
  RunBackpack(index)
  CharacterSafe(index)
  Vault(index)
  Overflow(index)
  ResolutionHold(extraction_id, index)
  Destroyed(reason)

security_state:
  AtRiskEquipped
  AtRiskPending
  Safe

provenance:
  Starter | Drop | Craft | Gift | Grant | Migration
```

`Starter` is provenance, not a location. Starter items have zero salvage value, occupy an equipment slot, are `AtRiskEquipped` in danger, and are destroyed normally on death. A successor receives new starter item instances.

The run backpack contains `8` pending slots. A pending item may be equipped in the field. The new item moves to `Equipped/AtRiskEquipped`; the replaced item moves to `RunBackpack/AtRiskPending`. If the backpack is full, the swap is rejected.

`CharacterSafe` has exactly `8` equipment/consumable stack slots. It exists only in safe instances and is the character's short transfer tray, not additional long-term storage. Equipped slots and the two belt slots do not consume CharacterSafe capacity.

Emergency Recall intentionally secures the one currently equipped item per slot and destroys remaining `AtRiskPending` items. This is a permitted slot-by-slot stabilization choice, not an exploit: equipping a new drop risks the replaced item, and death still destroys all `AtRiskEquipped` and `AtRiskPending` items.

Before a character enters any dangerous instance, every unequipped item in `CharacterSafe` MUST either be deposited into the Vault or explicitly moved to `RunBackpack/AtRiskPending`. The default UI action is automatic lowest-index Vault deposit; Overflow and ResolutionHold never accept manual deposits. If the Vault lacks capacity, the player must salvage/destroy an item or deliberately risk it in RunBackpack. No unequipped item may remain `Safe` while carried into danger.

Extraction placement is deterministic and occurs in the same transaction:

1. Existing equipped items and belt stacks change security to `Safe` without changing slot.
2. Credit the run-material pouch under LOOT-033.
3. Visit RunBackpack indices ascending. For a stackable consumable, merge into matching nonfull belt stacks, then CharacterSafe stacks, then Vault stacks, each by ascending index.
4. Put any remaining stack or unstackable item into the lowest empty CharacterSafe index, then lowest empty Vault index, then lowest empty Overflow index.
5. If no legal slot remains, put it in `ResolutionHold(extraction_id,next_index)`.
6. Clear each source RunBackpack index only as its destination write commits. Retry by extraction ID returns the stored placement map.

Overflow is extraction-only. Forge preflights one output destination in lowest empty CharacterSafe index, then lowest empty Vault index; if neither exists, reject with `storage_full` before charging. Forge never writes Overflow or ResolutionHold. Temper/Reforge mutate the existing item in place. Manual vault withdrawal rejects when CharacterSafe is full.

An operation that lowers pending capacity, including removing `bargain.ashen_pack`, is rejected while current pending count exceeds the resulting capacity. The UI identifies which items must be dropped, equipped, or extracted first.

### LOOT-003 — Rarity ladder

| Rarity | Affixes | Identity |
|---|---:|---|
| Worn | 0 | Starter and tutorial items |
| Forged | 0 plus implicit | Stable baseline |
| Oathed | 1 | Early specialization |
| Relic | 2 | Main build items |
| Sainted | 3 | High-end rare crafting target |
| Black Unique | Fixed signature plus 1 variable supporting affix | Behavior-defining chase item |

No Mythic tier exists at Early Access. Prestige comes from rolls, deeds, cosmetics, and challenge records rather than a hidden higher power tier.

### LOOT-004 — Item level and power budget

- Item level range: `1–20`.
- An item's base-stat budget is determined by slot, item level, and rarity.
- An affix consumes a configured budget measured in `affix_points`.
- Total affix points may not exceed the rarity budget.
- Damage multipliers from all affixes on one item are additive within their family before global multiplication.
- Movement speed from all equipment is capped at `+8%` and `-8%` before status effects.
- Equipment-only primary attack-rate increase is capped at `20%`.
- Equipment-only ability cooldown reduction is capped at `20%`.
- Ordinary gear cannot grant invulnerability, resurrection, passive evasion, or Recall acceleration.
- Unique signature mechanics may alter geometry or conditions but obey class and combat caps.

Prototype rarity budgets:

| Rarity | Affix points |
|---|---:|
| Oathed | 10 |
| Relic | 20 |
| Sainted | 30 |
| Black Unique variable affix | 10 |

### LOOT-005 — Affix families

- `offense`: damage, attack rate, projectile speed, pierce, area.
- `ability`: cooldown, charges, radius, duration, class-specific rules.
- `defense`: health, armor, resistance, barrier behavior.
- `mobility`: movement within cap, recoil reduction, terrain penalty.
- `status`: application strength, duration, resistance.
- `utility`: pickup radius, potion rules, secret reveal, pending capacity.
- `risk`: benefit paired with max-health loss, increased incoming damage, reduced healing, or constrained movement.

Each affix record MUST define permitted slots, item-level range, budget, exclusions, roll range, display format, and tags. Affixes with the same exclusivity group cannot coexist.

### LOOT-010 — Personal drop algorithm

Every reward table uses typed rolls; an `equipment_roll` can never resolve to a material and a `material_roll` can never resolve to equipment.

For each eligible player:

1. Encounter emits a personal loot-roll request with source and reward table ID.
2. Server verifies eligibility and weekly/first-clear state.
3. Server resolves the table's exact counts/chances of `equipment_roll`, `material_roll`, and `universal_item_roll`.
4. For an `equipment_roll`, select usability: `75%` current-class weapon/relic, `10%` other-class weapon/relic, `15%` universal armor/charm; then select the slot. Template legality is resolved after item level and rarity.
5. For a `universal_item_roll`, select `50%` armor and `50%` charm; it never selects a material.
6. For a `material_roll`, select only a material or consumable allowed by the source table.
7. Select item level and rarity from the source's explicit table. Materials ignore rarity.
8. Select a legal template.
9. Allocate the immutable item UID from `(reward_request_id, roll_index)` and persist/reserve that UID before affix generation; a retry reuses it.
10. Roll affixes using that UID from legal weighted pools without replacement by exclusivity group.
11. Finalize the same item record with template, level, rarity, affixes, provenance, and source.
12. Place equipment/consumables in pending inventory, or leave them as a personal ground drop for `60 seconds` if inventory is full. Materials create run-local `AtRiskPending` pouch stacks under LOOT-033 and never credit the safe wallet before extraction.

Other players cannot see or pick up personal ground drops unless an eligible gift is initiated after extraction.

The wipeable single-class Core bundle uses the content specification's explicit compatibility rules: unavailable other-class weight moves to current-class, Core rarity is fixed Forged, Caldus caps item level at10, and enabled T1 shared templates accept levels1–10. These exceptions end in Slice, are forbidden in Alpha/EA, and no Core item migrates into a later durable namespace.

### LOOT-011 — Prototype drop quantities

| Source | Personal reward |
|---|---|
| Normal enemy | 8% chance of one `universal_item_roll`; 12% independent chance of one `material_roll` |
| Elite | One guaranteed `equipment_roll`; 25% chance of one `material_roll` |
| Minor event | One `equipment_roll` plus one `material_roll` |
| Major event | Two `equipment_roll`s plus one `material_roll` |
| Dungeon miniboss | One `equipment_roll`; 35% chance of a second `equipment_roll`; source-specific material check where LOOT-032 defines one |
| Dungeon boss | Two `equipment_roll`s, one `material_roll`, one boss-fragment check |
| World climax | Three `equipment_roll`s, two `material_roll`s, one family-fragment check, one cosmetic/codex check |

Exact rarity odds live in versioned reward tables and are visible in the boss codex as percentages after the source is first defeated.

### LOOT-012 — Bad-luck protection

- Each dungeon boss and the Bell Warden world climax grants one nontradeable family Fragment on the first three account clears per UTC week.
- `20` matching Fragments directly grant one selected unowned, account-bound boss Unique. If all boss Uniques are owned, the player chooses either one duplicate account-bound Unique or `80 Ash Shards + 40 Lantern Marks`.
- Fragment count and guarantee options are visible in the codex.
- Fragments survive death and cannot be purchased, gifted, or traded.
- Direct Unique drops remain possible before the guarantee.

### LOOT-020 — Early Access item distribution

The 90 item templates are allocated as follows:

| Category | Count |
|---|---:|
| Vanguard weapons/relics | 16 |
| Arbalist weapons/relics | 16 |
| Witch weapons/relics | 16 |
| Shared armor | 18 |
| Shared charms | 18 |
| Consumable and crafting-material item definitions | 6 |
| Black Uniques included above | 12 total |
| Starter variants included above | 6 total |

At least 24 templates MUST alter attack or ability behavior. Pure numerical upgrades are concentrated in Worn, Forged, and Oathed tiers.

### LOOT-030 — Crafting actions

Only four actions ship in Early Access:

1. **Salvage** — destroy an extracted item for the exact Ash value in LOOT-031; Early Access Salvage grants no material.
2. **Forge** — create a known non-Unique template using the exact band recipe below; output account-bound.
3. **Temper** — once per item, improve one numeric affix by `10%` of the remaining distance to maximum; bind item.
4. **Reforge** — once per item, replace one non-signature affix with another legal affix from the same family pool; bind item.

Deterministic operation details:

- Forge: player selects one discovered non-Unique template and an available output band whose fixed output level meets the template's minimum level. Affix priority by slot is Weapon `[weapon_force, long_reach, quickened, fast_flight]`; Relic `[occult_force, hastened_rite, vitality]`; Armor `[vitality, plating, warded, fleet]`; Charm `[vitality, steady_hand, hastened_rite, warded]`, using full IDs from the content specification. Traverse in order, skip illegal/excluded entries, and take the first `affix_count` candidates. Each value is the exact legal tier midpoint rounded half-up to its stored unit. The content build fails if a forgeable template cannot fill its recipe.
- Temper: player selects one eligible numeric affix. `increase = max(one_stored_unit, ceil_to_stored_unit((affix_max - current_roll) × 0.10))`; new roll is `min(affix_max, current_roll + increase)`. An affix already at maximum is ineligible.
- Reforge: player selects both the affix to remove and one replacement from the displayed legal same-family candidates. The replacement roll is the exact midpoint of its legal tier range, rounded half-up to the affix's stored unit.
- Reforge cannot select the removed affix ID, create duplicate/exclusive-family conflict, change a Unique signature, or produce an empty/no-op result.
- If no legal replacement exists, the server rejects before charging.
- Costs use the current item's salvage band and are charged atomically with the successful mutation. Retry returns the original result.
- No Early Access crafting action rolls random output.

Unique signatures cannot be rerolled. Consecration, affix locks, multi-affix rerolls, sets, durability, and repairs are deferred.

### LOOT-031 — Salvage values

| Item band | Ash |
|---|---:|
| Starter | 0 |
| Tier I | 4 |
| Tier II | 12 |
| Tier III | 36 |
| Tier IV/Sainted | 80 |
| Black Unique | 80 |

Band assignment is exact: starter provenance uses Starter; any Black Unique uses the Black Unique salvage row and Tier IV crafting costs; any Sainted item uses Tier IV; otherwise item levels `1–6` use Tier I, `7–13` Tier II, and `14–20` Tier III. Rarity does not otherwise promote a band. Persist the resolved band on the item instance.

### LOOT-032 — Consumable and material definitions

These six definitions complete the 90-item catalog allocation:

| ID | Type | Exact behavior/source | Cap and sink |
|---|---|---|---|
| `consumable.red_tonic` | Belt consumable | Restore `30%` max health over `0.4 s`; shared potion cooldown `2 s` | Stack 6 per belt/backpack slot; consumed on use |
| `consumable.purifying_salt` | Belt consumable | Remove Bleed and Hex, then restore `5%` max health; shared potion cooldown `2 s` | Stack 3; consumed on use |
| `material.bell_brass` | Crafting material | Minor realm event: 1; Bell Sepulcher boss: 2; Mire Elite: 10% chance of 1 | Wallet cap 999; Tier I Forge/Reforge |
| `material.funeral_root` | Crafting material | Major realm event: 1; Root Chapel boss: 2; Root secret room: 1 | Wallet cap 999; Tier II Forge/Reforge |
| `material.saltglass_shard` | Crafting material | Drowned Reliquary boss: 2; Bell Warden: 1; Tier III miniboss: 25% chance of 1 | Wallet cap 999; Tier III/IV Forge/Reforge |
| `material.echo_ember` | Cosmetic material | First reward-eligible Requiem defeat: 1; Restless Dead: 25% dungeon-completion check; Saint's Debt: 20% fallback check only when no support Unique is eligible | Wallet cap 99; Echo memorial palette costs 5, Echo grave-marker style costs 10 |

Materials are pending and at risk until successful extraction, then convert to account-wallet balances. They cannot be gifted or traded. Equipped belt consumables are at risk on death, preserved by Emergency Recall, and replenished only from owned stacks or starter grants.

### LOOT-033 — Run material pouch

- A dangerous run has a separate four-entry material pouch; it does not consume the eight equipment/consumable backpack slots.
- One entry exists per material ID and holds up to `99` units. Matching rewards merge into the existing entry under the reward idempotency key.
- A material reward creates a run-local `AtRiskPending` stack and a pending instance event; it does not mutate the safe account wallet.
- Before granting quantity, compute `available = wallet_cap - safe_wallet_balance - current_pending_quantity`. Grant `min(reward_quantity, max(0, available))` and show a cap notice for omitted quantity. Spending safe wallet currency/materials is unavailable in danger, so this reservation remains valid.
- If a future bundle adds a fifth material type or a stack would exceed `99`, the excess remains a personal ground stack for `60 seconds`; pickup merges only when entry/cap space exists.
- Successful extraction atomically consumes pouch stacks and credits the safe material wallet. Death and Emergency Recall destroy all pouch stacks. A committed extraction wins over a later disconnect; otherwise crash recovery follows the danger-entry restore rules and grants no pending material.
- Material stacks cannot be equipped, gifted, salvaged, forged, or manually moved to `CharacterSafe`.

### LOOT-034 — Exact crafting recipes and costs

| Band | Forge output | Forge cost | Temper cost | Reforge cost |
|---|---|---:|---:|---:|
| Tier I | Item level 6, Oathed, 1 priority affix | 20 Ash + 2 Bell Brass | 8 Ash | 16 Ash + 1 Bell Brass |
| Tier II | Item level 13, Oathed, 1 priority affix | 60 Ash + 4 Funeral Root | 24 Ash | 48 Ash + 1 Funeral Root |
| Tier III | Item level 18, Relic, 2 priority affixes | 120 Ash + 6 Saltglass Shards | 72 Ash | 144 Ash + 1 Saltglass Shard |
| Tier IV/Sainted/Black Unique | No Forge recipe | — | 160 Ash | 320 Ash + 2 Saltglass Shards |

Temper uses no material. Forge is non-Unique and account-bound. Reforge uses the persisted crafting band. A Unique's fixed signature is never eligible; only its one variable supporting affix may be Tempered/Reforged. Migrated items require an explicit band before mutation.

### LOOT-040 — Party gifting

At Early Access, there is no unrestricted trade. An extracted drop may be gifted only when all conditions are true:

- Recipient was in the same party, instance, and source encounter.
- Gift occurs within `10 minutes` of the item's successful extraction. The item stores `extracted_at`; dungeon duration does not consume the gift window.
- Item has not been crafted, modified, equipped after extraction, or previously gifted.
- Item is not a Fragment, currency, material wallet entry, cosmetic entitlement, or account-bound reward.
- Gifted item becomes account-bound.
- One immutable gift ledger event records giver, recipient, source, item, and timestamps.

### LOOT-050 — Free storage

- Active character slots: one per released class plus one flex slot. Adding a class automatically grants one slot to all accounts.
- Initial gear vault: `160 slots`.
- Four account milestones each unlock `20` more slots, for `240` total.
- Material wallet: unlimited material types with template-defined caps; LOOT-032 values override the system maximum of `99,999`.
- Wardrobe, codex, currencies, and memorial metadata have no paid capacity limit.
- Overflow Cache: `20 slots`, extraction-only, `72-hour` expiry, automatic salvage on expiry.
- ResolutionHold: extraction-transaction overflow only, no expiry, inaccessible during play, and blocks danger entry until resolved. It cannot receive manual deposits.
- Loadout templates reference existing items and do not create storage.
- Functional storage and active character slots are not sold.

### LOOT-060 — Loot acceptance criteria

- No test sequence of duplicated requests can create two live records for one item UID.
- A death transaction destroys every equipped and pending item exactly once.
- A successful extraction, server restart, or retry cannot duplicate or lose an accepted item mutation.
- For every material ID, `safe wallet + active pending pouch + committed spends` is conserved across concurrent grant, cap, extraction, death, Recall, disconnect, crash restoration, and request retry fixtures.
- A material can enter the safe wallet only through a committed extraction transaction; a pouch can never survive death or Emergency Recall.
- Median expert recovery to configured functional-build power remains `30–45 minutes`.
- At least `30%` and no more than `55%` of extracted nonstarter gear is salvaged over a rolling 28-day mature cohort.
- Every Black Unique has an automated test for its signature interaction and prohibited combinations.

---


## 11. Veil Bargains

### BRG-001 — Purpose and lifecycle

Veil Bargains are optional paired boon-and-curse choices that create identity during one character's life.

- A qualifying shrine normally presents exactly three legal Bargains using the deterministic offer contract in the content specification.
- Player may choose one or refuse all.
- Chosen Bargain persists until character death, retirement, or purge.
- Maximum active Bargains: `3`.
- Each completed milestone in BRG-002 grants one `earned_bargain_slot`, maximum three.
- Bargains cannot be purchased or rerolled with premium currency.
- One Bargain may be purged in Lantern Halls for `50 Ash`; the earned slot remains and the next qualifying shrine offers a replacement.
- A bargain's curse MUST remain meaningful after all gear and oath interactions.

### BRG-002 — Shrine schedule

A character may earn Bargain offers from:

- First major realm event after level 5.
- First Tier II dungeon boss after level 10.
- First Tier III dungeon boss after level 15.

The Core Prototype has one explicit temporary trigger: on the first `layout.core_private_life_01` Sepulcher Knight clear at level 5+, grant slot one and activate its rest-room shrine. This trigger is disabled in Slice and later bundles, where the three production milestones above are authoritative.

If active Bargains are fewer than earned slots because of a purge, the next additional production milestone shrine presents a replacement offer. If no slot is earnable/unfilled, a repeated qualifying shrine grants exactly `10 Ash`; first-time codex discovery is a separate idempotent grant and never replaces the Ash. A shrine never creates a fourth slot.

### BRG-003 — Early Access Bargain catalog

| ID | Boon | Curse |
|---|---|---|
| `bargain.cinder_hunger` | `+18%` direct outgoing damage | `-12%` max health |
| `bargain.glass_pulse` | Ability cooldowns `-20%` | Damage received `+12%` |
| `bargain.bell_debt` | Every fifth primary attack repeats after `300 ms` for `50%` damage | Primary attack rate `-15%` |
| `bargain.lantern_ash` | Potion healing `+40%` | Only one consumable belt slot is active |
| `bargain.grave_weight` | `+15` armor | Movement speed `-8%` before final cap |
| `bargain.salt_oath` | Negative-status duration `-25%` | Healing received `-20%` |
| `bargain.hollow_aim` | Projectile speed and range `+20%` | Max health `-10%` |
| `bargain.rooted_bloom` | After standing nearly still for `600 ms`, area damage `+25%` | Buff ends on movement and outgoing direct damage is `-8%` while inactive |
| `bargain.funeral_pace` | Kill grants `+10%` attack rate for `3 s`, up to two stacks | Taking direct damage removes stacks and gives `-10%` damage for `2 s` |
| `bargain.saints_debt` | Healing, barriers, and authored damage reduction given to allies are `30%` more effective, subject to global caps | Personal outgoing damage `-15%` |
| `bargain.veil_mirror` | Player projectiles reaching max range split into two `40%` damage fragments at `±20°` | Base primary damage `-18%` |
| `bargain.ashen_pack` | Pending run inventory `+2 slots` | Outgoing ability damage `-10%` |

### BRG-004 — Compatibility

- `cinder_hunger` and `glass_pulse` cannot be offered together to a character already below `90%` of class base max health through other Bargains.
- Final maximum health is clamped by the global `0.70` floor after all Bargain and oath multipliers.
- `grave_weight` cannot reduce final normal movement below `4.5 tiles/s`.
- `saints_debt` is offered only when the class/oath/relic exposes the `support` tag and is not offered in solo fixed-loadout challenges.
- `veil_mirror` applies only to tagged `projectile` primaries and must define a nonprojectile fallback of `+12% range, -10% damage`.
- All final stats are passed through global caps after Bargain calculations.

### BRG-005 — Acceptance criteria

- A shrine never offers a duplicate or illegal Bargain.
- The choice UI shows before/after values for health, damage, cooldown, movement, and healing affected.
- A player can refuse without penalty.
- Bargain state survives reconnect and instance transfer.
- Simulation tests cover all class, oath, and Bargain combinations at minimum and maximum legal item stats.

---


## 12. Fallen Hero Echoes

### ECH-001 — Eligibility

A death creates an Echo when all are true:

- Character reached level 10.
- Character spent at least `10 minutes` in permadeath-enabled combat instances.
- Character completed at least one dungeon boss or two major realm events during that life.
- Death was not caused by a verified server incident or administrative action.
- Character has not already produced an Echo from the same death ID.

All level-5-or-higher deaths create memorials even if they do not create Echoes.

### ECH-002 — Echo record

```text
EchoRecord {
  echo_id
  death_id
  account_id
  character_name_snapshot
  class_id
  oath_id
  level
  appearance_snapshot
  appearance_theme_id
  weapon_signature_tag
  relic_signature_tag
  active_bargain_ids[]
  deed_tags[]
  killer_content_id
  killer_pattern_id
  death_region_id
  power_band
  created_at
  state: Dormant | Available | Defeated | Archived | Disabled
  content_version
}
```

No item UID or item roll is copied into usable Echo rewards.

### ECH-003 — Personal Requiem encounter

- Each account may have one `Available` Requiem at a time.
- New eligible Echoes queue chronologically; player may archive an available Echo without reward to reveal the next.
- Requiem is a solo or party-of-up-to-four optional instance.
- Entry costs nothing and has normal permadeath unless Practice is explicitly selected.
- Practice gives no rewards and cannot mark the Echo defeated.
- A five-second ready countdown locks `N_locked=1–4`; Echo health scales by recorded power band and `N_locked`, never by current gear and never rescaled after a departure, death, disconnect, or Recall.
- Normal entrants must meet the content specification's minimum character level for the recorded band. Practice has no level gate and no permadeath/rewards.
- Every Echo attack declares the explicit COM-002 health-damage cap of `35%` target max health. Echoes kill through readable combinations, not a single Severe hit.

### ECH-004 — Controlled encounter assembly

An Echo encounter uses authored modules only:

1. `class_primary_module` chosen only by class in Early Access.
2. `class_ability_module` chosen only by oath in Early Access.
3. `bargain_module` chosen from at most one active Bargain.
4. `death_memory_module` chosen by killer pattern family.
5. Standard break phase after every two attack modules.

Weapon/relic signature tags select presentation skins only in Early Access and never change Echo geometry, damage, timing, or module choice. The assembler MUST validate module compatibility and fall back to the class default if any referenced content is missing or disabled. It MUST NOT generate projectile parameters outside the COM-006 budgets.

### ECH-005 — Encounter phases

| Phase | Echo health | Behavior |
|---|---:|---|
| Remembrance | 100–70% | Primary module shown alone, then ability module alone |
| Accusation | 70–35% | Primary plus one death-memory pattern; 3-second break every 20 seconds |
| Last Light | 35–0% | Ability plus one Bargain module; speed `+10%`, no new pattern language |

Echo phase thresholds are monotonic. Compute threshold health with `ceil(initial_max_health × threshold)`. A hit or status tick that would cross the next threshold clamps health to it and discards overkill. During the `2 second` transition the Echo is untargetable and damage-immune, emits no damage, and pauses its negative/positive status durations and ticks. Healing cannot restore an earlier phase. Clear the prior phase's hostile entities as specified by the content scheduler.

### ECH-006 — Rewards

An owner-qualified first defeat grants the owner:

- Memorial title progress.
- One guaranteed untradeable `Echo Ember` cosmetic material when reward-eligible.
- Codex completion for that hero.
- `20 Lantern Marks` when reward-eligible.
- A chance at an appearance unlock from the earnable, Echo-eligible appearance pool associated with the hero's visual theme. Purchased or entitlement-only appearances are never copied or granted.

Each living, present, contribution-qualified helper receives one non-economic helper deed per unique Echo. A reward-eligible helper also receives `5 Lantern Marks`, but no Echo Ember or appearance roll. A dead, Recalled, disconnected, or contribution-ineligible participant receives only memorial/deed credit already earned before defeat and receives no item, currency, XP, Ember, or cosmetic roll.

It never grants the dead hero, original gear, equivalent replacement gear, premium currency, or leaderboard power.

Only the first three contribution-qualified normal Requiem completions per account per Monday `00:00 UTC` week are reward-eligible for economic rewards. A death using the same killer-content ID and killer-pattern ID as an already rewarded death in that account/week grants memorial/codex/deed progress but no Marks, Ember, or cosmetic roll; the duplicate still consumes its weekly ordinal. Telemetry tracks `marks_per_active_hour`, repeated death pairs, and time-to-eligibility for abuse and pacing review. Exact attempt eligibility, owner-success requirements, state transitions, and helper rewards are in the content production specification.

### ECH-007 — Future public Echoes

Public-realm and guild Echo encounters are deferred until after Early Access. The stored record format supports them, but no public spawn scheduler is required for Early Access.

### ECH-008 — Acceptance criteria

- Replaying Echo creation for one death ID produces one record.
- Disabling a content module substitutes a safe default without corrupting the record.
- Every generated encounter passes deterministic pattern and reachability validation.
- At least `60%` of unprompted Vertical Slice testers identify Echoes or Bargains as a distinctive feature.
- Echo rewards never contain a combat item UID derived from the dead character.

---


## 13. Lantern Halls and the public realm

### WRLD-001 — Lantern Halls

Lantern Halls is the noncombat account hub. It contains:

- Character spawn and Realm Gate.
- Vault and Overflow Cache.
- Forge and salvage station.
- Memorial Wall and Requiem portal.
- Class tutorial portals.
- Wardrobe and cosmetic preview.
- Party assembly area.
- Dungeon contract NPC.
- Store entrance; store panels are disabled before commerce milestone.

No hostile damage, PvP, item dropping, or character death is possible in Lantern Halls. Server-side inventory mutations still require authoritative confirmation.

### WRLD-002 — Mire of Bells topology

Early Access ships one authored `128 × 128 tile` macro map with seeded event, enemy, prop, and portal placements. It has three danger bands:

| Band | Recommended level | Purpose | Visual landmarks |
|---|---:|---|---|
| Outer Reeds | 1–7 | Onboarding, leveling, first event | Grave road, lantern posts, shallow water |
| Sunken Parish | 6–14 | Major events, elites, Tier I/II portals | Chapels, bell bridges, drowned courtyards |
| Bellmarsh Heart | 12–20 | High danger, Tier III access, climax | Great belfry, deep mire, reliquary causeway |

The map MUST contain at least two walkable routes between adjacent bands. No critical route relies on a destructible object, random bridge, or simultaneous player switch.

### WRLD-003 — Realm population

- Early Access hard cap: `40 active players` per realm.
- New realm opens when all healthy realms are at `36` players or greater.
- Matchmaker prefers a healthy existing realm with `8–32` players.
- A realm below `4` players for `15 minutes` after its climax begins retirement after the next safe boundary.
- Core progression remains solo-completable.
- Hub instances are separate and do not count toward realm cap.

### WRLD-004 — Realm cycle

Each realm uses this state machine:

```text
Booting -> Gathering -> RisingPressure -> BellSiege -> Climax -> Aftermath -> Retiring
```

Alpha-and-later production schedule:

| State | Entry condition | Maximum duration | Behavior |
|---|---|---:|---|
| Gathering | Realm ready | 15 min | Outer and Parish events; pressure gain ×1 |
| RisingPressure | 3 cycle credits or Gathering timer | 20 min | More elites; every terminal qualifying event still adds exactly one success or pressure credit |
| BellSiege | 7 cycle credits or 35 min since boot | 10 min | Bell Tower Siege becomes primary event |
| Climax | Siege success or 45 min since boot | 8 min | Bell Warden arena opens; solo fallback always works |
| Aftermath | Warden defeated or climax timer expires | 5 min | Exit gates and reward NPCs; no new events |
| Retiring | Aftermath ends | 2 min | Warnings, successful automatic extraction, shutdown |

A Slice/M04 realm intentionally omits BellSiege and Climax: `Booting -> Gathering -> RisingPressure -> Aftermath -> Retiring`. It uses the same `3`-credit/15-minute Gathering transition and exits RisingPressure at `7` credits or minute `35`; Content CONT-WORLD-008 is executable authority. Siege and the Warden first enable in Alpha/M05.

An Alpha-and-later realm must reach Climax by minute `45` even if no event succeeds. Players may accelerate but cannot permanently block the cycle.

A successful qualifying realm event adds one success credit; a failed/abandoned qualifying event adds one pressure credit. `cycle credits = success credits + pressure credits`. Pressure credits advance only the realm clock/state and never grant loot, portal discovery, or success flags. Exact minor/major classification, five-second director evaluation, site pools/weights/cooldowns, allocation RNG, and Siege scheduling are Content CONT-WORLD-007/008.

### WRLD-005 — Event director

- Simultaneous active event slots: `clamp(1 + floor(active_players / 8), 1, 5)`.
- Events are selected from legal zone and cooldown pools using the realm seed.
- The same event type cannot start twice consecutively in one zone when another eligible type/site exists; if excluding it would empty the legal zone pool, it may repeat after its normal cooldown.
- Event health and spawn scaling use living participants measured at activation.
- Event damage never scales with participant count.
- Events despawn only after failure, completion, or `5 minutes` without an eligible player within `20 tiles`.

### WRLD-006 — Group scaling

Let `N` be locked eligible living participants, clamped `1–8` for dungeon rooms and `1–20` for a local public event cell.

```text
regular_enemy_health = base × (1 + 0.25 × (N - 1))
room_spawn_budget = base × min(2.25, 1 + 0.45 × sqrt(N - 1))
elite_health = base × (1 + 0.75 × (N - 1))
boss_health = base × (1 + 0.72 × (N - 1))
```

For public events above eight participants, boss health uses `N = min(actual, 20)` and spawn budget is divided among spatial sub-objectives. At eight players, regular-enemy health is `2.75×`, Elite health is `6.25×`, and instanced boss health is `6.04×`; combined room work targets group clears only `10–25%` faster than solo. Damage, projectile speed, and gap widths do not scale. Full-dungeon duration fixtures, not formula inspection alone, determine acceptance.

### WRLD-010 — Ritual Interrupt event

1. Spawn three ritual sigils at authored anchors at least `8 tiles` apart.
2. Lock `N` eligible participants and activate `min(3, N)` sigils; remaining sigils are dormant and harmless.
3. Each sigil receives a fresh `90 second` corruption timer on activation and spawns its authored defender budget. If any active timer reaches zero, the event fails.
4. An active sigil is invulnerable until its local defenders are defeated. It then becomes interruptible: eligible presence within `2 tiles` pauses the timer, and holding Interact for `3 seconds` breaks it.
5. Breaking a sigil activates the next dormant sigil, if any, with a fresh timer. This permits one player to complete all three sequentially.
6. Breaking one spawns a Pressure wave using remaining enemy budget.
7. Completing all spawns a Bell Acolyte elite and a Tier I portal roll.
8. Failure increases realm pressure but does not remove future dungeon access.

### WRLD-011 — Funeral Caravan event

1. Caravan follows a fixed path of `40–60 tiles` over approximately `150 seconds`.
2. A visible `2.5 tile` Lantern Aura moves with it and suppresses shallow-water slow.
3. Three ambush anchors activate sequentially.
4. At each anchor, an optional side shrine adds one elite and one extra personal reward roll.
5. Caravan health cannot be healed by players and scales with participant count.
6. If destroyed, players retain enemy drops but receive no completion chest.

### WRLD-012 — Drowned Bell Recovery event

1. Spawn four bell fragments in a `16 tile` area.
2. Interacting begins a `2 second` carry action; carrier loses `10%` movement speed and cannot Recall until dropping the fragment.
3. While carrying, tapping Interact drops the fragment at the carrier's feet after `300 ms`; using either ability drops it immediately before the ability resolves.
4. Damage does not drop the fragment. Death, disconnect/LinkLost, instance transfer, or event failure drops it immediately at the last legal ground position; if none exists, return it to its authored spawn.
5. Recall input while carrying returns typed error `carrying_objective`; the HUD points to the Interact drop action.
6. Holding Interact for `1 second` at the central frame deposits a carried fragment permanently for that event.
7. Deposit all four fragments. Solo timer: `150 seconds`; add `15 seconds` per locked participant up to `240 seconds`.
8. Timer expiry despawns undeposited fragments and fails the event. Completion grants Tier II portal progress and one account-first codex page.

### WRLD-013 — Bell Tower Siege event

1. Activates during BellSiege state at the Great Belfry.
2. Three floors unlock sequentially; each is an authored open combat cell.
3. Floor 1: two enemy waves.
4. Floor 2: destroy two Anchor enemies while avoiding lane patterns.
5. Floor 3: defeat the Bell Warden's herald miniboss.
6. Success opens the Bell Warden climax arena and one portal for each discovered dungeon.
7. Failure at the timer still opens Climax with no bonus chest.

### WRLD-020 — Anti-grief rules

- Enemies leash to an authored home cell and reset after `12 tiles` beyond leash center.
- Hostile enemies cannot enter realm spawn, event reward, extraction, or portal vestibules.
- Players cannot body-block players or allied deployables.
- Opening a portal cannot consume another player's resource.
- Another player cannot alter an individual's Bargain choices or personal loot.
- Repeated attempts to drag enemies against a leash generate abuse telemetry.

### WRLD-030 — Realm acceptance criteria

- One player can trigger and complete every progression-critical event.
- The realm reaches Climax within `45 minutes` without player success.
- Automatic retirement performs a successful extraction, never a death.
- Event director never exceeds local enemy or projectile budgets.
- Spawn and return gates remain safe in 10,000 seeded director simulations.
- Four, twenty, and forty connected clients can see consistent event state.

---


## 14. Dungeon system

### DNG-001 — Early Access dungeons

| ID | Name | Band | Players | Target time | Recommended level | Boss |
|---|---|---|---:|---:|---:|---|
| `dungeon.bell_sepulcher` | Bell Tower Sepulcher | Tier I | 1–8 | 6–9 min | 5–12 | Sir Caldus |
| `dungeon.root_chapel` | Root Chapel of Veyr | Tier II | 1–8 | 10–14 min | 10–20 | Mother Veyr |
| `dungeon.drowned_reliquary` | Drowned Reliquary | Tier III | 1–8 | 15–20 min | 20 | Salt Confessor |

### DNG-002 — Access

- World events spawn personal-eligibility public portals for `90 seconds`.
- A portal is never sold.
- After discovering a dungeon, the Lantern Cartographer offers an account-bound contract if the account has not seen that dungeon's world portal in the prior `15 minutes` of active realm play.
- Contract entry is free for Tier I, costs `20 Ash` for Tier II, and costs `1 Veil Seal` for Tier III.
- Hall contract cost is quoted and charged separately to every ready entrant. A world-event portal has no entrant cost.
- Contract transaction states are `quoted -> reserved -> committed` or `refunded`. Reserve a healthy instance first, commit each eligible entrant's charge before teleport, and refund every committed charge if transfer setup fails.
- Duplicate reservation/commit requests return the original idempotent result.
- Practice versions unlock after first normal boss encounter and grant no persistent rewards.

### DNG-003 — Instance state machine

```text
Allocating -> Vestibule -> ActiveRooms -> BossWarning -> BossActive -> Completed -> Exiting -> Closed
```

- Vestibule is safe and permits party loading for up to `20 seconds`.
- Room activation is server-owned.
- Normal exit is available from Vestibule before the first room.
- Emergency Recall follows DTH-010.
- BossWarning shows encounter, practice, recommendation, and network state. Recall remains available in every Early Access boss.
- Completed state stabilizes all pending loot when the player uses the exit portal.

### DNG-004 — Room graph

| Dungeon | Main-path combat rooms | Branch rooms | Rest/Bargain rooms | Boss room |
|---|---:|---:|---:|---:|
| Bell Sepulcher | 4–5 | 0–1 | 1 | 1 |
| Root Chapel | 6–7 | 1–2 | 1 | 1 |
| Drowned Reliquary | 8–9 | 2 | 1 | 1 |

“Branch rooms” counts ordinary combat/utility branches only. The optional Secret room is tracked separately and does not count toward this column.

Generation algorithm:

1. Derive the attempt RNG exactly as Content CONT-ROOM-006 from the dungeon ID, content version, immutable `dungeon_seed`, and attempt index.
2. Select authored entrance and boss templates.
3. Build one main path with the required combat-room count.
4. Insert required rest/Bargain room at `55–75%` main-path progress.
5. Add branch paths no longer than two rooms.
6. Assign room templates without repeating the same template consecutively.
7. Assign enemy budget, hazard budget, reward nodes, and modifier sockets.
8. Validate reachability, arena size, safe spawn, safe exit, door connectivity, threat, modifier compatibility, and objective references.
9. Reject and retry with attempt indices `0..9`; never add the attempt index to or otherwise mutate the numeric dungeon seed.
10. If all attempts fail, load the authored fallback layout and emit telemetry.

Boss arenas are authored and never selected from arbitrary room geometry.

### DNG-005 — Room activation and completion

- A room locks participant count when the first player crosses its activation boundary.
- Other party members have `8 seconds` to cross before doors close.
- Players left behind receive a safe join portal after the room completes.
- Doors never close on a player hurtbox.
- Room completes when all required enemies/objectives resolve.
- A `2 second` quiet period follows completion before doors open.
- Enemies and hostile projectiles are cleared at room completion unless explicitly persistent and harmless.
- If living participants inside an active room reach zero while living party members remain outside, wait `3 seconds`, clear room enemies/projectiles/unsecured room drops, restore the room to its authored initial state, reopen doors, and permit a new activation with recomputed scaling. Deaths and Recalls remain final.

### DNG-006 — Boss participant lock

- Party sees a `5 second` ready countdown at the boss boundary.
- Living players inside when it closes define `N_locked`.
- No late entry after closure.
- Death or Recall does not reduce health scaling.
- Dead players may spectate or leave; no revive or downed state exists.
- If living boss participants reach zero while living party members remain outside, wait `5 seconds`, clear hostile entities/unsecured boss drops, restore boss/arena to initial state, reopen BossWarning, and permit a new participant lock with recomputed scaling. Recall is restored throughout the reset.

### DNG-010 — Dungeon modifiers

| ID | Rule | Reward modifier |
|---|---|---|
| `modifier.fevered_veil` | Each eligible enemy pattern reserves its original plus a repeat `500 ms` later. For original count `n` and repeat count `k=ceil(n/2)`, clone indices `floor(((2j+1)×n)/(2k))` for `j=0..k-1`; normal damage; never execute an eligible original without its reserved repeat | Multiply eligible curse-material outcome weight by `1.20` and renormalize; if none exists, one independent `20%` Funeral Root check at successful exit |
| `modifier.candleless` | Minimap reveals only visited rooms; world visibility unchanged | Multiply base secret insertion probability by `1.20`: Tier I/II/III become `24%/36%/48%` |
| `modifier.glass_floor` | Player input-direction smoothing increases from the SIM-005 baseline `60 ms` to `120 ms`; final speed unchanged | Multiply eligible precision-material outcome weight by `1.20` and renormalize; if none exists, one independent `20%` Saltglass Shard check at successful exit |
| `modifier.saints_debt` | External healing cap reduced from `35%/10 s` to `20%/10 s` | After Black Unique rarity is selected, multiply each eligible support-tagged template weight by `1.20` and renormalize that eligible Unique pool; if no support Unique is eligible, instead grant one independent `20%` personal Echo Ember check per successful dungeon |
| `modifier.oathfire` | Player outgoing and incoming direct damage each `+10%` | At boss reward, one extra equipment roll after zeroing Forged weight and renormalizing Oathed+ weights |
| `modifier.restless_dead` | Elites spawn one class-neutral Echo add at `30%` scaled Elite health | One independent `25%` Echo Ember check per eligible player at successful dungeon exit |

Tier I has zero or one modifier. Tier II has one. Tier III has one or two from the compatibility matrix. `fevered_veil` and `oathfire` cannot coexist in Early Access.

### DNG-020 — Secret rooms

- Secret entrances are visually hinted by a consistent cracked-bell motif.
- Detection never requires a paid item or a class.
- Attacking or interacting with the hint for `1 second` reveals the doorway.
- Secret rooms contain lore, materials, or a challenge chest; never a progression-exclusive item.
- Maximum one secret room per generated dungeon.

### DNG-030 — Failure behavior

- If all living players die or Recall, the instance closes after `30 seconds`.
- Server crash follows TECH-023: a committed death/extraction wins; otherwise living characters restore to the danger-entry restore point in Lantern Halls and all post-entry unsecured gains are revoked. A crash never creates a final death.
- Invalid content discovered at allocation rejects entry and refunds the contract cost.
- A stuck-room watchdog opens safe extraction and disables rewards after `120 seconds` without legal progress.

### DNG-040 — Dungeon acceptance criteria

- 10,000 seeds per dungeon pass reachability and fairness validation with zero invalid shipped seeds.
- Fallback-layout rate is below `0.1%`; any higher rate blocks content release.
- Solo and groups of 2, 4, and 8 can complete each dungeon in its target time band using recommended gear.
- Group clear target is `10–25%` faster than solo, not proportional to group size.
- No modifier combination violates COM-006.
- Instance crash and retry tests never duplicate, lose, or incorrectly destroy durable inventory.

---


## 15. Enemies, patterns, and bosses

### ENC-001 — Enemy roles and budget

| Role | Spawn cost | Maximum in one Standard room | Purpose |
|---|---:|---:|---|
| Fodder | 1 | Budget-limited | Pacing and target flow |
| Pressure | 3 | 4 | Force movement |
| Disruptor | 4 | 2 | Change priority or space |
| Anchor | 6 | 2 | Objective/high-health center |
| Elite | 10 | 1 | Mini encounter and guaranteed loot |

Solo Standard room base budget is `12–16`. A room may contain at most two Disruptors or one Elite unless it is an authored miniboss room.

### ENC-002 — Early Access enemy catalog

| ID | Role | Movement | Primary behavior |
|---|---|---|---|
| `enemy.drowned_pilgrim` | Fodder | Walk at 2.2 toward player | One 3-shot `30°` fan every 2.2 s; Chip |
| `enemy.mire_leech` | Fodder | Rush at 3.0, retreat after contact | Contact strike then 1.5 s retreat; Pressure |
| `enemy.toll_crow` | Pressure | Orbit at 4 tiles | Dive lane with 0.7 s telegraph every 4 s; Pressure |
| `enemy.bell_reed` | Pressure | Stationary | Six-shot ring with two-shot gap every 3 s; Pressure |
| `enemy.chapel_wisp` | Disruptor | Orbit nearest Anchor | Places 1-tile Frostbind pool, 0.8 s telegraph, every 5 s |
| `enemy.mudbound` | Anchor | Slow approach at 1.2 | Frontal shield; rear safe attack; 5-shot fan every 3 s |
| `enemy.bell_acolyte` | Pressure | Maintain 6 tiles | Alternating left/right 5-shot fans every 1.8 s |
| `enemy.chain_sentry` | Anchor | Stationary | Two perpendicular lane telegraphs every 4.5 s |
| `enemy.sepulcher_knight` | Elite | Pursue then reset | 5-tile charge lane, stop ring, 6 s cycle |
| `enemy.choir_skull` | Disruptor | Fixed orbit anchor | Two-arm rotor active for 4 s in a 6 s cycle; 2 s quiet |
| `enemy.root_thrall` | Fodder | Zigzag approach | Single slow orb that blooms into 4 Chip shots |
| `enemy.maskfruit` | Pressure | Retreat at 4 tiles | Lobbed 1-tile poison bloom, 0.8 s telegraph |
| `enemy.bloom_widow` | Disruptor | Circle at 5 tiles | Two web lanes causing Frostbind, 5 s cycle |
| `enemy.orchard_cantor` | Anchor | Stay near allies | 3 s heal channel, interrupt by `8%` max-health damage |
| `enemy.brine_husk` | Fodder | Direct approach | Three slow wide shots; Chip |
| `enemy.salt_novice` | Pressure | Maintain 7 tiles | Two fast Needles separated by 0.4 s; Pressure |
| `enemy.confession_mirror` | Disruptor | Stationary | Announces one reflected lane; does not reflect player projectiles dynamically |
| `enemy.tide_mourner` | Elite | Move between anchors | Telegraphed push wave plus delayed Needle fan |

All damage, speed, count, and timings are content data. Enemy AI uses explicit state machines; it does not infer tactics from animation state.

### ENC-003 — Pattern primitives

Supported primitives:

- `single_aimed`
- `fan`
- `ring_with_gap`
- `rotor`
- `telegraphed_lane`
- `ground_circle`
- `expanding_bloom`
- `memory_sequence`
- `personal_mark`
- `push_wave`

New primitives require fixed-input trace/state-hash playback, debug visualization, schema addition, and COM-006 validation before content may reference them.

### ENC-004 — Pattern data contract

```text
PatternDefinition {
  pattern_id
  schema_version
  telegraph_id
  audio_cue_id
  telegraph_ms
  counterplay_tag
  aim_mode
  repeat_count
  repeat_interval_ms
  projectile_template_id
  shot_count
  base_angle_degrees
  angle_step_degrees
  angular_velocity_degrees_per_second
  speed_tiles_per_second
  acceleration_tiles_per_second_squared
  projectile_radius
  lifetime_ms
  raw_damage
  damage_type
  damage_band
  damage_tags[]
  health_damage_cap_basis_points?
  statuses[]
  projectile_disposition
  echo_memory_family
  attack_group_rule
  gap_indices[]
  threat_cost
  maximum_active_instances
  compatibility_tags[]
  cancel_on_phase_change
}
```

### ENC-005 — Boss template

Every boss MUST contain:

1. Authored arena and safe entrance.
2. Identity introduction of no more than `4 seconds`.
3. Learning phase showing each core pattern alone.
4. Pressure phase combining learned patterns.
5. At least one `3–5 second` break/damage window.
6. Final phase that accelerates learned language by no more than `15%` and adds no new pattern type below `20%` health.
7. Soft enrage that changes cadence, not unavoidable damage.
8. Personal loot and stable exit.

### ENC-010 — Sir Caldus, Bell-Bound Knight

```text
arena: circle, radius 8 tiles
solo_health: 7,200
armor: 10
recommended_level: 10
recommended_item_level: 8
target_solo_duration: 150–210 seconds
soft_enrage: 360 seconds; remaining scheduler downtime -15%; no damage/speed increase
```

**Phase 1 — 100–70%**

- Exact `7.8 s` loop: Shield Arc starts at `0`, `1.8`, and `3.6 s`; each has a `650 ms` telegraph, five aimed projectiles over `60°`, speed 7, radius 0.12, raw damage 24, and lifetime 2.5 s.
- Bell Ring starts at `6.0 s` in that loop and consumes the next Shield slot: `800 ms` telegraph and unique bell sound; 18 radial projectiles; omit three adjacent shots; gap advances five indices clockwise each cast; speed 5; damage 32.
- At 70%: cancel attacks; `4 s` break; Caldus takes `+25%` damage and emits no damage.

**Phase 2 — 70–35%**

- Exact `15 s` loop: Charge Lane starts at `0` and `7.5 s`; show `1.2 tile` lane for `1.0 s`; direction locks after `700 ms`; travel 6.5 tiles over `550 ms`; contact damage 48 once.
- Charge stop emits 14 radial projectiles; omit two opposite the charge direction; damage 28; speed 5.
- Shield Arc starts at `3.0`, `5.2`, `10.5`, and `12.7 s` in the loop while not charging.
- At 35%: repeat `4 s` break.

**Phase 3 — 35–0%**

- Exact `8 s` loop: preview gap A at `0–0.6 s`, B at `0.6–1.2 s`, and C at `1.2–1.8 s`; wait `400 ms`.
- Emit three Bell Rings at `2.2`, `3.0`, and `3.8 s` using preview order.
- Start one Shield Arc at `6.0 s`; no other Shield starts occur in this loop.
- Below 20%, intervals shorten `10%`; no new attack.

Group variant: at 4+ players Shield Arc targets two distinct players `400 ms` apart; at 7–8 it targets three. Ring count, gap, speed, and damage never scale.

### ENC-011 — Mother Veyr, Orchard Saint

```text
arena: rectangle, 18 × 14 tiles, four harvest rows
solo_health: 12,000
armor: 8
recommended_level: 15
recommended_item_level: 13
target_solo_duration: 180–240 seconds
soft_enrage: 420 seconds; remaining scheduler downtime -10%; no damage/speed increase
```

**Phase 1 — 100–70%**

- Root Rows every `5 s`: two of four rows telegraph `800 ms`, activate for `1.2 s`, damage 34 and Frostbind 1.5 s.
- Seed Fan every `2 s`: seven shots over `90°`, speed 5, damage 20.
- Every third Seed Fan blooms at max range into four outward Chip shots.

**Break at 70%**

- `4 s` Harvest window; four Maskfruit adds spawn but do not attack for first `1 s`.
- Veyr takes `+20%` damage.

**Phase 2 — 70–35%**

- Rotating Harvest: safe row advances clockwise every `1.5 s` for four steps; unsafe rows show `650 ms` warning before activation.
- Poison Blooms target `min(3, ceil(N_locked/3))` players every `6 s`; `900 ms` ground warning; damage 30 and Hex 3 s.
- Seed Fan interval `2.4 s`.

**Phase 3 — 35–0%**

- Alternate Root Rows and Rotating Harvest; never overlap their impact ticks.
- Veyr summons two Root Thralls every `12 s`, maximum four alive.
- Below 20%, only the independent Root Thrall summon interval shortens `10%`; the combat-pattern loop remains `12 s` and no new attacks appear.

### ENC-012 — Salt Confessor

```text
arena: circle, radius 9 tiles, four dry anchor platforms
solo_health: 18,000
armor: 15
recommended_level: 20
recommended_item_level: 18
target_solo_duration: 210–300 seconds
soft_enrage: 480 seconds; remaining scheduler downtime -12%; no damage/speed increase
```

**Phase 1 — 100–75%**

- Exact `14 s` loop: Tide Push starts at `0` and `7.0 s`; each has a `1 s` circular warning, radial push of `1.5 tiles`, and 18 damage; four dry anchors negate push but not other shots.
- Crystal Needles starts at `1.8`, `3.4`, `5.0`, `8.8`, `10.4`, and `12.0 s`: three aimed shots at `-8°, 0°, +8°`; speed 10; damage 28.

**Phase 2 — 75–45%**

- Confession Mark every `8 s`: target `min(3, ceil(N_locked/3))` players; `1 s` personal lane warning; marked player drops a delayed six-shot ring after `1.2 s`, damage 24.
- Tide Push continues every `9 s`.
- Crystal Needles alternate clockwise/counterclockwise fan offset.

**Break at 45%**

- `5 s`; boss kneels, no attacks, takes `+25%` damage.

**Phase 3 — 45–0%**

- Two Tide Pushes `2 s` apart, then `4 s` recovery.
- Confession Mark follows recovery; never overlaps Tide impact.
- Memory Needles: preview three dry anchors, then fire lane volleys leaving those anchors safe.
- Below 20%, intervals shorten `12%`; no new patterns.

### ENC-013 — Bell Warden world climax

```text
arena: authored public belfry, 22 × 18 tiles
solo_health: 10,000
armor: 12
local participant cap for scaling: 20
target_duration: 180–300 seconds
soft_enrage: 480 seconds; remaining scheduler downtime -15%; no damage/speed increase
```

**Phase 1 — 100–70%**

- Warden Ring every `3.2 s`: `650 ms` bell telegraph; 24 radial projectiles; omit four adjacent shots; gap advances seven indices clockwise each cast; speed 4.5; radius 0.13; raw damage 24; lifetime 4 s.
- Belfry Lanes starts at `8.0 s` of each `16 s` phase cycle and consumes the otherwise-due `9.6 s` Warden Ring: select two nonadjacent lanes from six authored lane anchors; show `1.2 tile` lanes for `900 ms`; active for `800 ms`; raw damage 38 once per lane activation.
- Bell Acolytes every `12 s`: spawn `2 + floor((N_locked - 1) / 5)`, maximum five living Acolytes. Skip the spawn when the cap is reached.
- At 70%: cancel attacks and hostile phase projectiles; `4 s` break; Warden takes `+20%` damage and emits no attacks.

**Phase 2 — 70–35%**

- Arena displays four local cells using harmless gold boundary lines. Each player is assigned to the nearest cell for add targeting; crossing cells remains allowed.
- Cell Anchor every `12 s`: create one Anchor in each cell containing an eligible player, maximum four. Each Anchor has 600 solo health scaled as an Elite and emits a 10-shot local ring every `4 s`, omitting three adjacent shots; speed 4; damage 22.
- Verdict Lanes starts at `2.0 s`, `9.5 s`, and `17.0 s` of each `24 s` phase cycle: target `min(3, max(1, ceil(N_locked / 3)))` distinct players; show `1 tile` personal lane for `1 s`; impacts are staggered `400 ms`; raw damage 40.
- Warden Ring continues every `4 s` while no Verdict impact is pending.
- At 35%: remove surviving Anchors without rewards; cancel attacks; `5 s` break; Warden takes `+25%` damage.

**Phase 3 — 35–0%**

- Bell Memory every `9 s`: preview three safe cell indices for `500 ms` each; after a `700 ms` pause, activate the complementary unsafe cells in the same three-step order for `800 ms` each; raw damage 36 once per step.
- One Warden Ring starts at `4.85 s` and emits at `5.5 s` of each `9 s` Bell Memory cycle; it never impacts during a memory-cell activation.
- No new adds spawn.
- At `25%`, all Phase 3 intervals shorten `10%`; no new attack language appears.

- World boss cannot use Execution damage and never locks Recall.
- Reward eligibility uses SOC-010 contribution.

### ENC-014 — Miniboss catalog

Miniboss health uses Elite group scaling. Damage and projectile speed never scale.

| ID | Location | Solo health / armor | Exact repeating kit |
|---|---|---:|---|
| `miniboss.sepulcher_knight` | Bell Sepulcher | 1,600 / 8 | Charge Lane every 6 s: 0.9 s warning, 5-tile charge, 34 damage; stop emits 10-shot ring, two-shot gap, 20 damage. Shield Fan every 2.2 s between charges: five shots over 50°, 18 damage. |
| `miniboss.choir_abbot` | Bell Sepulcher | 1,900 / 6 | Two-arm rotor for 3.5 s at 35°/s, one shot per arm each 0.35 s, 18 damage; 2.5 s recovery. Recovery ends with 16-shot ring, four-shot gap, 26 damage. |
| `miniboss.masked_gardener` | Root Chapel | 2,800 / 8 | Targeted Bloom every 5 s: 0.8 s warning, 1.1-tile radius, 28 damage plus Hex 3 s. Seven-shot Seed Fan every 1.8 s, 20 damage. Maximum two active Blooms. |
| `miniboss.rootmother` | Root Chapel | 3,200 / 12 | Two of four authored Root Lanes every 5.5 s: 0.8 s warning, 32 damage plus Frostbind 1.5 s. Summons two Root Thralls every 10 s, maximum four alive. Five-shot fan every 2.4 s, 18 damage. |
| `miniboss.tide_mourner` | Drowned Reliquary | 4,500 / 14 | Tide Push every 7 s: 1 s warning, 1.2-tile push and 22 damage. Delayed Needle Fan 0.8 s later: five shots over 32°, speed 10, 30 damage. Single aimed Needle every 1.8 s otherwise, 24 damage. |
| `miniboss.salt_mirror` | Drowned Reliquary | 5,000 / 16 | Previews three lane anchors for 0.5 s each, waits 0.7 s, then activates them in preview order at 0.6 s intervals for 34 damage. Six-shot gap ring every 3 s between sequences, 24 damage. No dynamic reflection of player projectiles. |

Each miniboss has a `3 second` introduction in which it can move but cannot deal damage, a target solo duration of `35–65 seconds`, one guaranteed personal item roll, and at least `2 seconds` of quiet time after defeat.

### ENC-020 — Encounter acceptance criteria

- Each boss passes 100 fixed-input trace/state-hash playback fixtures at every supported party size.
- A minimum-speed, no-ability bot has at least one authored collision-free route for every mandatory pattern.
- No phase exceeds projectile or threat budget.
- No player can be targeted by two personal Major attacks whose impact windows overlap.
- Phase change cancels or safely resolves old projectiles according to content data.
- Soft enrage remains theoretically survivable and never raises damage or projectile speed.

---


## 16. Parties, contribution, and communication

### SOC-001 — Party model

- Party size: `1–8`.
- Any player may create a party in Lantern Halls or a realm.
- Join methods: Steam friend invite, six-character expiring join code, recent-player invite, or public party listing.
- Public party listing contains leader, target activity, region, language tag, current size, and item-power recommendation.
- Joining never teleports directly into active boss combat.
- Party leader may choose activity, start ready checks, remove members outside combat, and transfer leadership.
- Leadership transfers automatically on disconnect after `30 seconds` or voluntary leave.
- Party membership changes are rejected during an active boss participant lock.

### SOC-002 — Portal flow

1. Leader or portal owner selects `Open for party`.
2. Server verifies each member's region, state, and required contract.
3. A ready panel shows eligible, ineligible, ready, loading, and declined states.
4. Start occurs when all eligible members are ready or leader begins with the ready subset.
5. For a Hall contract, the server quotes and commits the configured cost for every ready entrant after instance reservation; members unable to pay are marked ineligible before start. World-event portals are free.
6. Declined/ineligible members remain in current safe state.
7. Reconnect returns a member to the party vestibule or safe spectator state according to instance phase.

### SOC-003 — Pings

Early Access pings:

- Danger.
- Retreat.
- Group here.
- Heal/support here.
- Portal.
- Loot/Bargain.
- Boss target.
- Ready.

Rules:

- Maximum three pings per player per five seconds.
- Ping lifetime: `3 seconds`; portal ping `6 seconds`.
- Muting a player hides their pings and emotes.
- Pings use icon, label, world marker, and optional restrained sound.
- Pings cannot be placed outside the local player's explored or visible world data.

### SOC-004 — Text and voice scope

- Public free-text chat is disabled for Early Access.
- Party text chat MAY ship only if mute, block, report, rate limits, filtering, and evidence capture are complete.
- System messages and predefined quick phrases are available.
- Voice chat is not implemented; players may use platform voice externally.
- Character and party listing names pass normalization, profanity, impersonation, and reserved-name checks.

### SOC-010 — Contribution

```text
contribution_units =
  direct_damage
  + 0.80 × effective_healing_to_others
  + 0.60 × damage_prevented_on_others
  + objective_units
```

Every reward source defines `encounter_contribution_reference_health`. For a boss, it equals locked scaled boss health. For a nonboss event, it is an authored value equal to the scaled health-equivalent of required enemies/objectives. All percentage thresholds and objective-unit grants below use this field; they never assume an event has a boss.

Boss/event reward eligibility requires:

- Presence for at least `50%` of active duration, except fights under `20 seconds`.
- Contribution of at least `0.5% × encounter_contribution_reference_health`.
- No inactivity interval over `20 seconds`.
- Alive at defeat for combat items, currency, XP, or crafting rewards. A player who legitimately died during the fight may receive only memorial/deed/participation credit; death never routes combat loot into safe account storage.
- Not completing Emergency Recall before defeat.
- Valid session and anti-cheat state.

One qualifying objective action grants units equal to `2%` of `encounter_contribution_reference_health`, capped at two credits per player per encounter.

### SOC-011 — Support accounting

- Effective healing counts only health actually restored.
- Damage prevention counts only barrier or authored mitigation applied to another player against damage that would otherwise occur.
- Overhealing and overlapping weaker barriers count zero.
- Self-healing and self-mitigation do not count as support contribution.
- One support action cannot be credited to two sources.

### SOC-020 — Blocking and reporting

- Block hides party invites, listings, pings, emotes, text, and whispers from the blocked account.
- Report categories: abusive name, harassment, spam, cheating, griefing, inappropriate cosmetic/display, other.
- Report captures involved account references, current instance, recent predefined communications, recent party actions, and client/server build versions.
- Reporter text is optional and length-limited.
- Reporting never informs the reported player immediately.
- Support actions are immutable audit events.

### SOC-030 — Social acceptance criteria

- A four-player party can form by invite, enter a dungeon, reconnect one member, and exit without developer commands.
- A blocked player cannot contact, ping, or invite the blocker.
- Support builds can meet contribution without dealing more than `1%` of group damage.
- Proximity-only inactive players receive no boss reward.
- Party membership and leader state remain consistent across one server restart or recover to a documented disbanded state without item loss.

---


## 17. Economy

### ECO-001 — Economy goals

- Found loot is the primary source of combat items.
- Death destroys meaningful supply.
- Crafting creates predictable recovery, not perfect items.
- Trade is unnecessary for progression.
- Currency is never sold, transferable, or convertible to premium value.
- No durability or repair system exists.
- Economy configuration is server-owned, versioned, simulated, and observable.

### ECO-002 — Item lifecycle

```text
Personal drop
  -> RunBackpack/AtRiskPending OR run material pouch/AtRiskPending
  -> equip at risk OR carry at risk
  -> successful extraction
  -> CharacterSafe / Vault / Overflow OR safe material wallet
  -> vault / equip / gift / salvage / craft
  -> death destruction OR intentional sink
```

Emergency Recall preserves only currently equipped gear and destroys pending backpack inventory plus all run-material-pouch stacks. Normal extraction secures pending items and atomically credits pouch materials. Death destroys equipped and pending items, including the pouch.

### ECO-003 — Currencies

| Currency | Persistence | Source | Use | Cap | Tradeable |
|---|---|---|---|---:|---|
| Ash Shards | Account | Salvage, events, account goals | Forge, Temper, Reforge, contracts, oath/Bargain purge | 99,999 | No |
| Veil Seals | Account | Tier II/III bosses and mastery | Tier III contracts, targeted recipes | 120; earn 30 per published Monday 00:00 UTC week | No |
| Lantern Marks | Account cosmetic | Achievements, Echoes, free chapter rewards | Earnable cosmetic catalog | 99,999 | No |

Early Access uses direct platform-priced cosmetic SKUs only. No premium-currency wallet is implemented. A premium wallet requires a future separately approved specification covering grants, spent-balance refunds, chargebacks, regional value, financial liability, and reconciliation.

### ECO-004 — Currency rules

- Every currency mutation includes reason code, source ID, content version, idempotency key, before balance, delta, and after balance.
- A balance cannot go negative.
- Cap overflow is rejected before the source resolves or converted only by an explicitly displayed rule.
- Currency cannot be dropped, gifted, mailed, or traded.
- Each gameplay currency, currently Ash Shards and Veil Seals, requires at least two repeatable sources and two meaningful sinks. Cosmetic Lantern Marks require at least two earn sources and one maintained cosmetic-catalog sink.

### ECO-005 — Exact currency awards and prices

| Source | Ash Shards | Veil Seals | Lantern Marks |
|---|---:|---:|---:|
| Minor realm event | 10 | 0 | 0 |
| Major realm event | 25 | 0 | 0 |
| Bell Sepulcher boss | 12 | 0 | 0 |
| Root Chapel boss | 24 | 1 | 0 |
| Drowned Reliquary boss | 40 | 2 | 0 |
| Bell Warden | 50 | 2 | 10 once per account per UTC week |
| Weekly mastery objective | 0 | 5 | 20 |
| Owner reward-eligible Requiem | 0 | 0 | 20, subject to ECH-006 cap |
| Helper reward-eligible Requiem | 0 | 0 | 5, subject to ECH-006 cap |
| Achievement | Configured 0–100 | 0 | Configured 5–50 |

| Sink | Price |
|---|---:|
| Tier II Hall contract | 20 Ash per ready entrant |
| Tier III Hall contract | 1 Veil Seal per ready entrant |
| Oath change after free change | 40 Ash |
| Bargain purge | 50 Ash |
| Targeted non-Unique recipe discovery | 10 Veil Seals |
| Earnable cosmetic accessory | 50 Lantern Marks |
| Earnable full appearance | 150 Lantern Marks |
| Earnable memorial/grave style | 100 Lantern Marks, or Echo Ember price when specified |

All weekly limits reset Monday at `00:00 UTC` and are displayed in the relevant wallet/codex UI. A reward that would exceed a cap shows the capped result before activity confirmation where practical.

### ECO-010 — Economy health targets

| Metric | Healthy prototype range | Response outside range |
|---|---:|---|
| Ash spent / created, new/recovering accounts over 28 days | 0.70–0.95 | Tune recovery sources/sinks |
| Ash spent / created, mature accounts over 90 days | 0.90–1.05 | Add attractive horizontal sinks or adjust sources |
| Tier III+ supply destroyed / created | 0.75–1.05 | Adjust recovery or source rates |
| Extracted nonstarter gear salvaged | 30–55% | Below: sinks weak; above: items uninteresting |
| Expert death-to-functional-build | 30–45 min | Above: recovery too harsh; below: death lacks weight |
| Boss Unique guarantee reached | 5–20% of earners | Above: natural drop too rare; below: pity may be irrelevant |
| Overflow expiry auto-salvage | Under 5% | Improve capacity/UX if higher |

Also track median and p90 wallet balance, cap-hit rate, and estimated days of ordinary spending held by cohort.

Metrics MUST be segmented by class, experience cohort, item band, party size, and content source.

### ECO-020 — Trading rollout

**Prototype and Closed Alpha:** no trade, market, mail, or player currency transfer.

**Early Access:** only party gifting under LOOT-040.

**Optional post-launch direct trade:** requires 90 incident-free days for the inventory ledger, fraud/support tools, and a separate approved economy specification. Only unmodified Tier I–III rares and explicitly enabled materials may be considered.

**Global market:** not scheduled. It requires all of:

- At least `5,000 DAU` for eight consecutive weeks.
- At least `500` qualified daily sellers.
- 90 incident-free inventory-ledger days.
- Immutable provenance, delayed settlement, fraud review, price history, rollback, and support tools.
- Evidence that self-found completion remains healthy.

Paid listings, paid limits, paid search placement, and paid settlement speed are permanently prohibited.

### ECO-030 — Economy acceptance criteria

- A 90-day low/base/high simulation covers drop creation, extraction, death loss, salvage, crafting, gifting, and inactive hoarding.
- No replayed request duplicates currency or items.
- Every created item has one source and every destroyed item has one terminal reason.
- Support can reconstruct one account's item/currency history from ledger events.
- The game remains completable in a self-found account simulation.

---


## 18. Monetization and commercial fairness

### MON-001 — Hard rule

A purchase MUST NOT improve combat, expected loot value, death survival, recovery speed, inventory power, competitive opportunity, dungeon access, queue position, trade access, or leaderboard performance.

Specifically prohibited:

- Combat equipment, stats, XP, materials, Ash, Seals, or gameplay consumables.
- Drop boosts, pity progress, crafting discounts, or extra reward rolls.
- Resurrection, insurance, auto-Recall, invulnerability, or death restoration.
- Dungeon keys or contracts.
- Functional vault capacity or active character slots.
- Market listings, fee discounts, or trade permissions.
- Paid seasonal score or challenge attempts.
- Randomized paid loot boxes.

### MON-002 — Allowed entitlement types

The commerce schema may grant only:

```text
CharacterAppearance
WeaponAppearance
Emote
Nameplate
MemorialStyle
GraveMarker
GuildDecoration   // data type reserved; not sold before guilds exist
MusicPack
AnnouncerPack
ChapterCosmeticAccess
```

Catalog validation MUST reject any SKU containing a gameplay item, currency, material, storage, character slot, market permission, combat modifier, dungeon access, or death utility.

### MON-003 — Cosmetic readability

- Cosmetics never alter hitboxes, class-recognition silhouette, animation timing, or combat-critical audio.
- Alternate friendly projectiles display only to the owner by default.
- Other players see standard projectiles unless they explicitly enable allied cosmetics.
- Cosmetic particles obey per-player count, opacity, radius, and lifetime budgets.
- Refund or entitlement revocation restores the default appearance safely.
- A cosmetic preview is available before purchase.

### MON-010 — First monetized build

Monetization is absent from First Playable through Public Steam Playtest. No pre-Early-Access purchase may promise permanent live persistence.

The base game is free-to-play: the Steam base-app price is `US$0`, downloading/creating an account requires no gameplay purchase, and every class, region, dungeon, boss, item source, Bargain, modifier, Echo encounter, level, and functional progression path is available without payment. No paid gameplay DLC, required subscription, energy, key, or access pass ships in Early Access. Revenue comes only from the allowed direct cosmetic/supporter entitlements below.

Early Access store scope:

- One `US$19.99` Founder Supporter Pack containing one class-neutral skin set, grave marker, nameplate, memorial frame, and emote.
- Exactly ten direct-purchase cosmetic SKUs grouped into five commercial sets.
- Exactly six earnable cosmetic sets available through play.
- Together with the Founder set, these form the exact 12-set PRD-021 launch manifest; IDs, entitlements, sources, prices, previews, and fallback behavior are Content CONT-COS-001.
- No paid Chapter/season pass at initial release.

Recommended starting price bands:

| Product | USD reference band |
|---|---:|
| Recolor/accessory | $2.99–4.99 |
| Full character appearance | $7.99–11.99 |
| Memorial/profile bundle | $4.99–7.99 |
| Themed multi-cosmetic bundle | $14.99–24.99 |

Regional prices use the platform's current localized guidance. Prices, contents, and refund effects are displayed before purchase.

### MON-011 — Purchase transaction

1. Client requests server-created order with SKU and platform identity.
2. Server validates catalog version, region, account state, and ownership.
3. Platform handles payment authorization.
4. Platform callback reaches purchase service.
5. Service writes immutable transaction and idempotent entitlement grant.
6. Client receives entitlement only after durable grant.
7. Reconciliation job finds missing, duplicate, refunded, and charged-back transactions.
8. Refund/reversal removes entitlement and safely equips defaults.

Successful charge without a durable entitlement or durable retry record is a severity-one defect. Commerce uses the definitions in MON-012; the circuit breaker automatically disables new checkout on any wrong/duplicate grant or when `grant_success_5m <99.9%` across the latest 100 authorized orders.

### MON-012 — Commerce health definitions

- `grant_success_5m`: percentage of authorized orders with the correct durable entitlement within five minutes. Operating SLO: `≥99.99%`.
- `wrong_or_duplicate_grant`: an entitlement granted to the wrong account/SKU or more than once. Release and operating tolerance: exactly `0`.
- `unreconciled_24h`: percentage of authorized orders without a correct terminal/retry state after 24 hours. Required: `<0.01%`.
- Automatic checkout circuit breaker: any `wrong_or_duplicate_grant`, or `grant_success_5m <99.9%` across the latest 100 authorized orders.
- Between `99.9%` and `99.99%`, checkout may remain available only while an incident owner is engaged and the backlog is decreasing; this is degraded, not healthy.

### MON-013 — Commerce records and reconciliation

Every order stores:

```text
internal_order_id
platform_order_id
platform_transaction_id
environment
account_id
sku_id
catalog_version_and_contents_snapshot
customer_amount
customer_currency
tax_treatment
estimated_or_reported_platform_fee
refund_amount
chargeback_amount
chargeback_fee
settlement_batch_id
state
created_at
updated_at
reconciliation_attempts
last_error_code
```

Legal states:

```text
created
authorization_pending
authorized
grant_pending
granted
refund_pending
reversed
chargeback
reconciled
failed
```

- State transitions are append-only audit events; the current state is a derived/indexed view.
- Operational order/entitlement reconciliation runs hourly.
- Platform settlement reconciliation runs daily when a statement is available.
- Alerts cover aged `authorization_pending`, `grant_pending`, `refund_pending`, and unreconciled terminal states.
- Early Access has no purchase gifting.
- Already-owned standalone SKUs are rejected before checkout.
- A bundle containing any already-owned entitlement is rejected; owned-item proration requires a future explicit platform/refund specification.
- Account deletion removes or anonymizes player data as legally permitted, but required financial/tax/fraud records follow the documented statutory-retention exception.

### MON-020 — Post-launch Chapter Chronicle

Chronicle is reconsidered only after two consecutive 12-week content chapters ship on time.

If approved:

- Price: `US$9.99` reference.
- Rewards: cosmetics, emotes, memorial/profile items, and Lantern Marks only.
- No mandatory daily quests or login streak.
- Progress comes from ordinary play and account-wide challenges.
- Purchased Chronicles remain completable after their featured chapter.
- Featured cosmetics return through an archive within 12 months.
- No paid tier skips at initial Chronicle release.

### MON-030 — Monetization acceptance criteria

- Catalog validator rejects every prohibited entitlement fixture.
- Duplicate, reordered, delayed, missing, refunded, and chargeback callbacks produce one correct final entitlement state.
- `grant_success_5m ≥99.99%`, `wrong_or_duplicate_grant = 0`, and `unreconciled_24h <0.01%` before public commerce.
- Cosmetics pass hitbox, silhouette, hostile-projectile, and performance checks.
- No SKU is tradable or convertible to gameplay currency.

---


## 19. UI, HUD, accessibility, and onboarding

### UI-001 — UI technology and ownership

- Native Early Access HUD and menus use Bevy UI or an approved Bevy-compatible UI layer.
- Simulation owns no render widgets.
- UI reads view models produced from authoritative/predicted game state.
- Browser account/store pages, if later built, use a DOM shell; text-heavy browser UI is not forced into the canvas.
- Opening inventory, map, settings, or dialogs never pauses an online simulation.
- Modal UI blocks combat input and visibly indicates that the world continues.

### UI-002 — Client screen state machine

```text
Boot
 -> PatchCheck
 -> Authentication
 -> AccessibilityQuickSetup?       // first launch only
 -> TrainingCrypt?                 // first launch until completed
 -> CharacterSelect
 -> LanternHalls
 -> RealmLoading -> Realm
 -> DungeonLoading -> Dungeon
 -> Reconnect
 -> DeathSummary -> Memorial -> CharacterSelect
```

Every transition supports `loading`, `success`, `recoverable_error`, and `fatal_error` states. Recoverable errors preserve the prior safe state.

### UI-003 — Combat HUD layout

The center `60%` of the viewport and lower-middle aiming corridor MUST remain free of persistent panels.

| Location | Persistent elements |
|---|---|
| Top-left | Health frame, level/XP, class/oath icon, network warning |
| Bottom-left | Collapsible party frames; maximum eight rows |
| Bottom edge, left of center | Ability 1, Ability 2, two consumables, Recall state |
| Top-right | Minimap with event/portal markers |
| Right edge | One active event/boss objective card; collapsible |
| Center | Transient telegraphs, interaction prompt, damage direction only |
| Bottom-right | Compact pending inventory capacity and loot feed |

Persistent HUD coverage SHOULD remain below `20%` of viewport area at 1920×1080.

### UI-004 — Health frame

- Displays current/max health as number and bar.
- Shows barrier as separate overlay, not false health.
- At `35%` health, frame gains restrained pulse and one warning sound.
- At `15%`, pulse intensifies but never obscures play.
- Incoming damage briefly shows lost segment.
- Hover or settings expands armor/resistance breakdown.
- Color, shape, label, and audio communicate critical health; color alone is insufficient.

### UI-005 — Ability and status UI

- Ability icon displays binding, cooldown sweep, numeric seconds below 3 seconds, charges, silence, and invalid-target reason.
- Recall icon displays availability, pending-loss warning, and `400 ms` channel progress.
- Status tray sorts: lethal/negative, control, positive, informational.
- Status icon shows remaining duration and source in tooltip.
- Maximum eight visible statuses; overflow groups noncritical buffs behind an expandable count.

### UI-006 — Inventory overlay

- Opens on Tab/I and does not pause.
- Player movement MAY continue; fire/abilities are disabled while pointer interacts with inventory.
- Displays four equipped slots, pending backpack, and item comparison.
- Pending items use a distinct ash border and `Lost on death or Emergency Recall` label.
- Field equip requires drag/drop or select plus confirm; swapped item location is previewed before mutation.
- Item comparison prioritizes behavior text, then damage/defense changes, then affixes.
- Advanced numerical breakdown is expandable, not the default first read.

### UI-007 — Character select

Each slot shows:

- Name, class, level, oath, appearance.
- Equipped item-power band.
- Current safe/danger state.
- Last played time.
- Primary `Play` action.
- Secondary `Details`, `Rename appearance preset`, and eligible `Retire` action.

Dead characters never appear as playable. They appear in Memorials.

### UI-008 — Character creation

Required choices:

- Class.
- Character name from account display name plus optional hero epithet; names are validated.
- Appearance using owned/default cosmetics.
- Optional successor preset.

Oath is not chosen until level 10. The creation screen shows class difficulty, range, survivability, primary verb, and two 15-second preview clips.

### UI-009 — Death summary

Implements DTH-020. `Create successor` is the largest action. The store, paid products, and promotional offers MUST NOT appear on the death screen or until the successor has returned to control.

### UI-010 — Accessibility settings

Required before Closed Alpha:

- Full keyboard/mouse rebinding.
- Mouse sensitivity and aim cursor scale.
- Colorblind-safe hostile projectile themes.
- Hostile projectile outline thickness.
- Friendly-effect opacity `10–60%`.
- Damage-number off/minimal/full.
- Screen shake `0–100%`, default 50%.
- Flash intensity `0–100%`, default 50%.
- Reduced motion.
- High-contrast telegraphs.
- UI scale `80–150%`.
- Text/chat scale independent of HUD.
- Hold/toggle options for primary fire and Guard Arc where applicable.
- Separate master, music, ambience, hostile cue, UI, and voice volume.

Controller bindings are defined in the action map but full controller certification is a Version 1.0 gate, not an Early Access blocker.

### UI-011 — Supported layout

- Early Access minimum resolution: `1280 × 720`.
- Reference: `1920 × 1080`.
- Ultrawide expands peripheral world view only to the configured maximum camera range; it does not reveal inactive threats.
- UI respects safe margins of `24 px` at 1080p, scaled with UI scale.
- Text never renders below an effective `14 px` at minimum UI scale.
- Modal actions have visible keyboard focus and controller focus metadata even before controller certification.

### UI-020 — Onboarding gates

The tutorial is complete only after the player:

1. Moves through four directional gates.
2. Aims separately from movement.
3. Holds primary fire and defeats a target.
4. Uses both fixed training-avatar abilities.
5. Takes Chip damage and uses a potion.
6. Avoids one clearly telegraphed Major attack.
7. Starts and cancels Recall.
8. Completes Recall.
9. Experiences announced scripted death.
10. Correctly confirms what is lost and preserved in normal play.

Tutorial may be skipped only after completion on the account.

### UI-030 — UI acceptance criteria

- First actionable screen appears within the boot budget in TECH-070.
- Core combat remains readable with an eight-player party and all allowed HUD elements.
- No persistent panel covers center or lower-middle combat corridor.
- All interactive elements have default, hover, active, focus, disabled, loading, success, and error states as applicable.
- Every menu is keyboard operable.
- Reduced-motion and flash settings suppress nonessential effects without hiding mechanics.
- Screenshot QA covers combat, boss warning, inventory, death, hub, and 1280×720 layouts.

---


## 20. Art, animation, VFX, and audio

### ART-001 — Visual direction

- Dark-fantasy pixel art with restrained environments and high-contrast combat language.
- Materials: wet stone, tarnished brass, ash, salt, bone, moss, candlelight, stained glass.
- World palettes are muted; hostile projectiles and telegraphs have reserved contrast.
- Player class silhouette and weapon profile remain identifiable under cosmetics.
- Decorative detail MUST NOT resemble hostile bullets, loot beams, exits, or safe zones.

### ART-002 — Pixel and sprite standards

| Asset | Source footprint | Notes |
|---|---:|---|
| Environment tile | 32×32 | Nearest-neighbor scaling only |
| Player body | 32×32 | Bottom-center anchor; weapon rendered separately |
| Normal enemy | 32×32 or 48×48 | Threat readable by silhouette |
| Elite | 64×64 | Affix indicator separate from sprite |
| Boss | 96×96 to 192×192 | Attack origin authored explicitly |
| Projectile | 6×6 to 16×16 | Collision radius stored separately |
| Item icon | 32×32 | Shape-first rarity readability |

### ART-003 — Player animation minimum

For each class body and cardinal facing:

- Idle: 2 frames at 3 FPS.
- Walk: 4 frames at 8 FPS.
- Hurt: 2 frames, 160 ms total.
- Death: 6 frames, 600 ms total; presentation only.
- Ability commitment pose: 2–4 frames by class.

Weapon aim rotates independently with authored hand/pivot offsets. Animation timing never determines authoritative damage timing.

### ART-004 — Sprite production pipeline

1. Approve one in-game seed frame with correct silhouette, palette, proportions, and anchor.
2. Build a transparent reference canvas around the approved frame.
3. Generate or draw an entire animation strip in one pass; do not independently generate frames by default.
4. Normalize all frames to one scale and bottom-center anchor.
5. Lock frame 1 back to the approved seed where the animation begins at the base pose.
6. Render a preview sheet.
7. Inspect at actual game scale and in motion.
8. Import only after transparency, drift, silhouette, and anchor gates pass.

Every animation request MUST specify character identity, facing, palette family, silhouette, frame count, slot layout, transparent background, and no scenery or labels.

### ART-005 — Combat color/shape language

| Meaning | Shape | Color family | Secondary cue |
|---|---|---|---|
| Physical projectile | Pointed/needle | Bone white/red edge | Sharp attack sound |
| Curse | Diamond/spiral | Violet | Rotating inner mark |
| Frostbind | Hexagonal | Pale cyan | Crystalline trail |
| Poison/bleed zone | Organic ring | Sickly gold/green | Pulsing boundary pattern |
| Major/Severe | Thick outline | Damage-family color plus white core | Unique pre-impact sound |
| Friendly attack | Thinner/low-opacity | Class palette | No hostile outline |
| Healing/safe | Open lantern ring | Warm gold | Cross/lantern glyph |

No attack depends on red/green distinction alone.

### ART-006 — VFX budgets

- Hostile effects receive render priority 1.
- Player and objective readability: priority 2.
- Local-player attacks: priority 3.
- Other-player attacks: priority 4.
- Cosmetics and ambience: priority 5.
- When effect budget is exceeded, remove priorities 5 then 4; never remove hostile telegraphs.
- Camera shake from simultaneous sources uses maximum magnitude, not sum.
- Full-screen flashes are prohibited; use localized bloom/edge cues.

### ART-010 — Audio hierarchy

Mix priority:

1. Severe/Ultimate telegraph.
2. Low-health warning.
3. Boss phase/objective cue.
4. Party danger/retreat ping.
5. Local ability confirmation.
6. Loot rarity.
7. Music and ambience.
8. Cosmetic sounds.

Each boss pattern family has one recognizable cue. Repeated Chip projectiles use pooled, quiet sounds to avoid fatigue.

### ART-011 — Music scope

Early Access minimum:

- Lantern Halls loop.
- Mire of Bells exploration layers: calm, pressure, climax.
- Three dungeon loops.
- Four boss layers/stingers.
- Death/memorial cue.
- Black Unique drop stinger.

Music transitions are presentation-only and keyed from authoritative encounter phase events.

### ART-020 — Asset manifest

Every asset entry includes:

```text
asset_id
asset_type
source_path
runtime_bundle
content_dependencies[]
anchor
dimensions
animation_fps
collision_metadata_reference
palette/readability_tags[]
audio_priority
memory_budget_bytes
platform_variants{}
license/source_record
```

Runtime content references stable asset IDs, never filenames.

### ART-030 — Art acceptance criteria

- Animation proportions and anchors do not drift between frames.
- Player class reads at reference and minimum zoom.
- Hostile projectiles remain identifiable in grayscale and colorblind presets.
- Eight-player screenshots preserve enemy, hostile bullet, safe-zone, and exit hierarchy.
- Cosmetic combinations do not obscure hitboxes, class silhouette, boss origin, or hostile cues.
- Preview sheet and in-engine capture are stored with each approved animation set.

---


## 21. Technical architecture

### TECH-001 — Architecture goals

- Deliver a local First Playable without throwaway gameplay code.
- Reuse the same simulation rules in local, test, server, and replay contexts.
- Keep simulation independent from Bevy rendering and UI.
- Use one authoritative modular-monolith backend until measured scale requires separation.
- Make every durable mutation idempotent and auditable.
- Design shard boundaries without deploying unnecessary distributed infrastructure.

### TECH-002 — Toolchain

- Language: Rust stable pinned in `rust-toolchain.toml`.
- Client engine: Bevy `0.19.x`, exact patch pinned in `Cargo.lock`.
- Async runtime: Tokio in server process.
- Serialization: Serde; compact binary protocol after schema contract is tested.
- Gameplay content: versioned JSON validated by JSON Schema plus semantic validators.
- Durable database: PostgreSQL beginning with Complete Private Loop milestone.
- Local tests MAY use ephemeral PostgreSQL containers; SQLite is not a production-behavior substitute for transactional tests.
- Native real-time transport: QUIC with datagram support where available and reliable streams for critical events.
- Browser transport is deferred and must not shape Early Access delivery beyond protocol abstraction.

### TECH-003 — Workspace structure

This tree is the target Early Access workspace, not a three-day foundation deliverable. M00 creates only `client_bevy`, `sim_core`, `sim_content`, `content_schema`, and `tools_content` plus the minimum content/test folders needed by M01. M02 adds `protocol`, `server_app`, and `bot_client`; M03 adds `persistence` and `telemetry`; M06 adds `platform_steam` and production operations wiring.

```text
gravebound/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  crates/
    sim_core/              # fixed-step entities, collision, combat, AI, patterns
    sim_content/           # compiled definitions and semantic validation
    content_schema/        # serde types, JSON schemas, migrations
    protocol/              # versioned network messages and error enums
    client_bevy/           # rendering, input, audio, UI, prediction, interpolation
    server_app/            # modular authoritative server and orchestration
    persistence/           # PostgreSQL repositories, transactions, ledger
    platform_steam/        # auth, builds, commerce integration
    telemetry/             # event contracts and exporters
    tools_content/         # validators, packer, seed runner, balance reports
    bot_client/            # headless load and journey client
  content/
    classes/
    abilities/
    enemies/
    patterns/
    encounters/
    rooms/
    dungeons/
    items/
    affixes/
    loot_tables/
    bargains/
    localization/
    manifests/
  migrations/
  tests/
    deterministic/
    content/
    protocol/
    persistence/
    journeys/
    load/
  docs/
    decisions/
    runbooks/
```

### TECH-004 — Module ownership

| Module | Owns | Must not own |
|---|---|---|
| `sim_core` | Fixed-step rules, entities, collision, health, AI, projectiles, encounter state | Sprites, widgets, database connections, platform APIs |
| `sim_content` | Validated immutable gameplay definitions | Live mutable account state |
| `client_bevy` | Presentation ECS, input, prediction, interpolation, audio, UI view models | Authoritative death, drops, inventory mutation |
| `server_app` | Sessions, authoritative instances, routing, mutation commands | Rendering or client settings |
| `persistence` | Transactions, snapshots, ledgers, migrations | Combat logic |
| `platform_steam` | Platform identity, depot/build hooks, purchase calls | Entitlement truth without server ledger |
| `telemetry` | Typed events, batching, privacy filtering | Gameplay decisions |

### TECH-005 — Server modular monolith

Early Access server application contains modules:

- Gateway/session.
- Matchmaker.
- Hub instances.
- Realm instances.
- Dungeon instances.
- Account/character.
- Inventory/economy ledger.
- Party/presence.
- Moderation hooks.
- Live configuration.
- Telemetry export.
- Admin/support API.

Modules communicate through typed in-process commands/events and repository interfaces. A module may become a separate service only after an architecture decision identifies an independent scaling, reliability, or security boundary with measured evidence.

Redis, Kafka/NATS, Kubernetes, Agones, multi-region writers, and hot-shard migration are not Early Access requirements.

### TECH-006 — Runtime modes

```text
LocalLab      client + sim_core in one process, ephemeral state
LocalStack    client + server_app + PostgreSQL on one machine
TestRegion    remote server_app, wipeable test database, invited clients
Production    remote server_app, durable database, Steam identity, commerce gate
Headless      sim_core or bot_client with no renderer
Replay        deterministic trace playback using pinned content version
```

No gameplay rule may exist only in `LocalLab` client code.

### TECH-010 — Network handshake

Client sends:

```text
ClientHello {
  protocol_major
  protocol_minor
  client_build_id
  platform
  supported_compression[]
  content_manifest_hash
  auth_ticket
  locale
}
```

Server replies with either:

```text
ServerHello {
  session_id
  protocol_major
  protocol_minor
  required_client_build
  content_bundle_version
  server_tick_rate
  snapshot_rate
  region_id
  feature_flags
}
```

or a typed rejection:

```text
Maintenance
UpdateRequired
ProtocolUnsupported
AuthenticationFailed
AccountSuspended
RegionFull
ContentMismatch
RateLimited
InternalRetryable
```

### TECH-011 — Network channels

| Channel | Reliability | Examples |
|---|---|---|
| Input | Sequenced latest-state/datagram | Movement, aim, held fire |
| Action | Reliable ordered | Ability press, Recall start/cancel, interact |
| Snapshot | Latest-state/datagram | Entity transforms, health, phase state |
| Pattern | Reliable ordered with start tick | Deterministic enemy pattern descriptors |
| Mutation | Reliable ordered/idempotent | Pickup, equip, extract, vault, craft |
| Control | Reliable ordered | Join, transfer, reconnect, time sync, errors |
| Social | Reliable ordered | Party, ping, moderation acknowledgments |

### TECH-012 — Update rates

- Server simulation: `30 Hz`.
- Client input send: `30 Hz` with state coalescing.
- World snapshot: `15 Hz` baseline; local critical entities may use `20 Hz` if budget permits.
- Remote interpolation delay: prototype default `100 ms`.
- Time sync recalculates every `5 seconds` and on reconnect.
- Pattern events arrive at least `250 ms` before first active projectile where telegraph rules permit.

### TECH-013 — Interest management

- Realm uses a uniform spatial grid with `8 × 8 tile` cells.
- Client interest includes cells intersecting visible camera plus `4 tile` safety margin.
- Party members, active event objectives, current boss, and targeted personal mechanics remain relevant regardless of nearby cell until safely released.
- Inventory, account, and party mutation events are never dropped by interest management.
- Cosmetic entity updates are first to degrade under bandwidth pressure.

### TECH-014 — Prediction and reconciliation

- Client predicts only local movement and presentation start of accepted-input attacks.
- Server validates speed, acceleration, collision, cooldown, resource, and state.
- Correction below `0.10 tiles` is blended over `100 ms`.
- Correction `0.10–0.35 tiles` blends over `60 ms` and emits debug metric.
- Correction above `0.35 tiles` snaps, displays a network warning in debug/test builds, and records anomaly telemetry.
- Client never predicts item grant, death finality, extraction success, or purchase entitlement.

### TECH-015 — Reconnect and disconnect

- A dropped connection enters `LinkLost` for `3 seconds` while character remains vulnerable and performs automatic Emergency Recall behavior.
- Reconnecting before resolution reattaches to the same state.
- If death commits first, death is final.
- If Recall commits first, reconnect returns to Lantern Halls.
- Duplicate sessions invalidate the older transport only after authoritative handoff.
- An instance/server crash is not a legitimate death source. A committed death or extraction transaction wins; otherwise TECH-023 restores the danger-entry restore point and revokes post-entry unsecured gains.

### TECH-020 — Durable entities

Minimum persistent records:

- Account.
- Platform identity.
- Entitlement.
- Character and state version.
- Character equipment references.
- Item instance and item ledger event.
- Vault and Overflow Cache location.
- Currency balance and ledger event.
- Recipe/codex/mastery record.
- Memorial and Echo record.
- Party gift event.
- Death event and combat-trace reference.
- Dungeon/boss clear and pity progress.
- Purchase transaction and reconciliation state.
- Moderation action/report.

### TECH-021 — Idempotency

Every mutation command contains:

```text
mutation_id
account_id
character_id?
expected_state_version
command_type
payload_hash
issued_at
```

Rules:

- Repeating the same mutation ID and payload returns the original result.
- Reusing the mutation ID with a different payload is rejected and audited.
- State-version mismatch returns current safe state and requires client refresh.
- Item, currency, death, extraction, gift, craft, and purchase mutations are transactional.

### TECH-022 — Death transaction isolation

- Character row or aggregate uses a single-writer lock/version.
- Death transaction reads and locks character plus all equipped/pending item locations.
- It marks death, appends item destruction events, writes memorial/Echo, and increments state version atomically.
- Failure before commit leaves prior state intact and retryable.
- After commit, no command with prior character version can mutate items or character.
- A death summary is never shown before commit acknowledgement.

### TECH-023 — Persistence cadence and crash restore

- Inventory/account mutations persist immediately.
- Immediately before transfer from safety into a dangerous instance, create one `entry_restore_point` containing character level/XP, health, oath, Bargains, equipped item IDs, belt stacks, and all safe aggregate versions.
- Every `30 seconds` in danger, write a `danger_checkpoint` for process-resume/debug purposes. It never changes item security and is never used as the account restore point after an unrecoverable instance crash.
- If death or extraction committed before the crash, that committed transaction is final.
- Otherwise restore the character in Lantern Halls from `entry_restore_point`: entry equipment/belt/health/XP return exactly, field-equipped drops and all other pending drops/materials are revoked, and post-entry consumable use is rolled back.
- Entry equipment moved into the pending backpack by a field swap is restored to its original slot; the unsecured replacement is revoked. Item provenance/ledger events record the crash restoration.
- Instance journals retain enough state to investigate final `60 seconds` of combat and mutation commands.
- Automated fixtures crash immediately before/after pickup, field equip, potion use, death commit, and extraction commit.
- Database backups target recovery point `≤5 minutes` and recovery time `≤30 minutes` before commerce.

### TECH-030 — Authentication

- Local/test builds use explicit test identities separated from production namespace.
- Public Steam Playtest and Early Access use Steam authentication tickets validated server-side.
- Display name is not account authority.
- Auth tokens are short-lived and never logged.
- Pre-Early-Access accounts and data are explicitly wipeable.
- Final durable namespace begins only after two migration/wipe rehearsals and a public permanence declaration.

### TECH-040 — Live configuration

Feature flags and live content schedules are server-signed/versioned records containing:

- Config ID/version.
- Owner.
- Start/end timestamp.
- Target environment/cohort.
- Validation result.
- Kill switch.
- Last-known-good version.
- Audit reason.

Config cannot change item ownership, death result, or purchase entitlement retroactively without an explicit migration.

### TECH-050 — Admin/support tools

Before Closed Alpha, authorized staff can:

- Find account by safe identifiers.
- View character history and current state.
- View item provenance and currency ledger.
- Inspect death trace and content version.
- View party/session history.
- Disable a content definition for new instances.
- Correct an entitlement through an audited command.
- Apply mute, suspension, or ban with reason/expiry.

Tools use least privilege, staff authentication, audit logs, and separate read/write roles.

### TECH-060 — Build and release

- `Cargo.lock` is committed.
- CI runs format, lint, unit, deterministic, content validation, protocol compatibility, persistence integration, and Windows release build.
- Builds have immutable build IDs and content manifest hashes.
- Client, server, and content versions appear in diagnostics.
- Release supports previous-client rejection, server rollback, content rollback, and database migration rollback plan.
- A clean machine must build and run documented local commands.

### TECH-070 — Performance budgets

**Client reference target**

- Windows 10/11, 4-core 3.0 GHz-class CPU, 8 GB RAM, GTX 1050-class GPU.
- 1920×1080, 60 FPS target.
- Frame time p95 `≤16.7 ms`, p99 `≤33.3 ms` in ordinary play.
- 800 hostile projectile stress fixture, 40 enemies, and standard effects maintains at least 60 FPS on target or meets documented reduced-effects fallback.
- Memory after 30-minute realm/dungeon loop `≤1.5 GB` and no monotonic leak.

**Server instance target**

- 30 Hz tick budget `33.3 ms`.
- Tick p95 `≤20 ms`, p99 `≤30 ms` at tested capacity.
- Realm supports 40 players plus configured enemies/projectiles.
- Instance capacity is measured, not assumed; scheduler preserves `30%` CPU headroom.

**Network target**

- Average downstream `≤15 KB/s` per player in ordinary play.
- Boss spike `≤40 KB/s`.
- Average upstream `≤3 KB/s`.
- Load and latency tests use combat behavior, not idle sockets.

**Boot target**

- Installed native build to first actionable screen: median `≤10 seconds`, p95 `≤20 seconds` on target hardware excluding first platform login.
- Realm/dungeon transfer: median `≤5 seconds`, p95 `≤10 seconds`.

### TECH-080 — Architecture acceptance criteria

- LocalLab and authoritative server produce matching deterministic combat hashes for shared fixtures.
- Four humans and 16 bots complete a two-hour Networked Vertical Slice without state divergence or memory growth.
- At 100 ms RTT, 20 ms jitter, and 1% loss, controls remain playable and accepted deaths match authoritative traces.
- Mutation retry suite produces zero item/currency duplication.
- Server crash tests never create final deaths.
- Clean clone/build/test/run instructions pass from two clean environments.
- No production gameplay value is hard-coded outside approved defaults/tests.

---


## 22. Content data and authoring

### TECH-100 — Content principles

- All gameplay content is immutable versioned data loaded through typed schemas.
- Stable content IDs are lowercase dot-separated strings.
- Runtime code handles mechanics; data composes approved mechanics.
- Content cannot introduce executable scripts in Early Access.
- Unknown fields fail validation in production bundles.
- Missing localization, asset, reward, pattern, or reference blocks packaging.

### TECH-101 — Common content header

```json
{
  "id": "enemy.drowned_pilgrim",
  "schema_version": 1,
  "content_version": "ea.1.0.0",
  "enabled": true,
  "release_stage": "core",
  "localization_name_key": "enemy.drowned_pilgrim.name",
  "localization_description_key": "enemy.drowned_pilgrim.description",
  "asset_ids": ["sprite.enemy.drowned_pilgrim", "portrait.enemy.drowned_pilgrim"],
  "tags": ["mire", "fodder", "undead"],
  "numeric_payload": {},
  "source_document_feature_id": "CONT-ENEMY-002"
}
```

This header is identical to Content CONT-001/003. Domain schemas add typed fields beside the header; every numeric domain field is also represented in normalized `numeric_payload` for canonical hashing. Generated checked-in JSON, not this illustrative formatting, is authoritative.

### TECH-102 — Item template and instance examples

```json
{
  "id": "item.weapon.crossbow.grave_repeater",
  "schema_version": 1,
  "content_version": "ea.1.0.0",
  "enabled": true,
  "release_stage": "core",
  "localization_name_key": "item.weapon.crossbow.grave_repeater.name",
  "localization_description_key": "item.weapon.crossbow.grave_repeater.description",
  "asset_ids": ["icon.item.weapon.crossbow.grave_repeater"],
  "tags": ["slot.weapon", "class.grave_arbalist", "family.crossbow", "primary.projectile"],
  "slot": "weapon",
  "class_tags": ["class.grave_arbalist"],
  "capability_tags": ["family.crossbow", "primary.projectile", "modifiable.W", "modifiable.interval", "modifiable.range", "modifiable.projectile_speed", "permits_pierce"],
  "projectile_pattern_id": "player.arbalist.single_bolt",
  "numeric_payload": {
    "minimum_item_level": 4,
    "template_damage_scalar_bp": 8400,
    "attack_interval_micros": 382000,
    "range_milli_tiles": 8500,
    "projectile_speed_milli_tiles_per_second": 14000
  },
  "source_document_feature_id": "CONT-CATALOG-020"
}
```

```json
{
  "item_uid": "018f-example-stable-uid",
  "template_id": "item.weapon.crossbow.grave_repeater",
  "creation_event_id": "reward-example-0007",
  "item_level": 8,
  "rarity": "Relic",
  "affixes": [
    { "affix_id": "affix.offense.weapon_force", "tier": 2, "value_bp": 600 },
    { "affix_id": "affix.offense.quickened", "tier": 2, "value_bp": 500 }
  ],
  "displayed_W": 20,
  "salvage_band": "Tier II",
  "provenance": "Drop",
  "security_state": "AtRiskPending",
  "content_version": "ea.1.0.0"
}
```

For the instance example, `W=round_half_up((15+0.95×7)×0.84×1.03×1.06)=20`. Template and instance are distinct records; rarity, item level, affixes, provenance, security state, and UID never live on the shared template.

### TECH-103 — Encounter phase example

```json
{
  "phase_id": "boss.caldus.phase_1",
  "health_min_exclusive": 0.70,
  "health_max_inclusive": 1.0,
  "entry_break_ms": 0,
  "scheduler": {
    "type": "priority_timeline",
    "actions": [
      { "at_ms": 0, "pattern_id": "boss.caldus.shield_arc" },
      { "at_ms": 1800, "pattern_id": "boss.caldus.shield_arc" },
      { "at_ms": 3600, "pattern_id": "boss.caldus.shield_arc" },
      { "at_ms": 6000, "pattern_id": "boss.caldus.bell_ring" }
    ],
    "loop_ms": 7800
  },
  "group_variants": [
    { "minimum_players": 4, "target_count": 2, "target_stagger_ms": 400 },
    { "minimum_players": 7, "target_count": 3, "target_stagger_ms": 400 }
  ]
}
```

### TECH-104 — Required validators

- Schema/type/range validation.
- Unique ID validation.
- Reference and localization validation.
- Asset manifest validation.
- Affix budget/exclusion validation.
- Class/item compatibility validation.
- Pattern fairness/threat validation.
- Dungeon reachability and safe-spawn validation.
- Modifier compatibility validation.
- Loot-table probability sum and source validation.
- Economy source/sink lint.
- Monetization entitlement allowlist validation.
- Content-version and migration validation.

### TECH-105 — Authoring workflow

1. Create/change data in a feature branch.
2. Run schema and semantic validators locally.
3. Generate deterministic preview or seed report.
4. Run automated fixture tests.
5. Review gameplay/readability in a content test scene.
6. Approve content with owner and target bundle.
7. Package immutable bundle and manifest hash.
8. Deploy to test environment.
9. Run smoke journey and rollback check.
10. Promote exact bundle to production.

### TECH-106 — Tools order

Build tools in this order:

1. Command-line schema validator.
2. Deterministic pattern preview scene.
3. Dungeon seed batch runner and report.
4. Item/affix table report and power-budget linter.
5. Loot/economy simulator.
6. Browser-based or native encounter editor only after data workflow proves cumbersome.
7. Live-ops console before public operations.

A bespoke visual editor is not a First Playable requirement.

---


## 23. Security, anti-cheat, privacy, and support

### TECH-120 — Threat model

| Threat | Required control |
|---|---|
| Speed/teleport | Authoritative movement, acceleration/collision validation, sequence checks |
| Fire/cooldown modification | Server cooldown/resource state and action validation |
| Forged hit/drop | Server collision and reward ownership only |
| Packet replay/reorder | Session authentication, sequence windows, idempotency |
| Item/currency duplication | Single-writer aggregate, transactional ledger, mutation IDs |
| Account theft | Platform auth, short sessions, suspicious-session invalidation, support recovery |
| Bot farming | Behavior telemetry, rate limits, source caps, manual review; no client trust |
| RMT/fraud | No unrestricted trade; bound gifting; purchase reconciliation |
| Chat abuse | Public text disabled; filtering/reporting if party text ships |
| DDoS | Platform/edge protections, connection limits, queues, status communication |
| Admin abuse | Least privilege, MFA, immutable staff audit, separated write roles |

### TECH-121 — Validation policy

- Client sends intentions, never authoritative results.
- Malformed, impossible, stale, duplicate, or unauthorized commands return typed errors and may increment anomaly score.
- Ordinary bad-network behavior is distinguished from malicious patterns.
- Enforcement uses reviewed evidence; anomaly score alone does not permanently ban.
- Native client anti-tamper MAY provide signals but is not a correctness dependency.

### TECH-122 — Death dispute data

For every final death, retain according to privacy policy:

- Death ID and authoritative tick.
- Position and health history.
- Last ten seconds of received damage/statuses.
- Relevant pattern events and content version.
- Player input/Recall state needed for reconstruction.
- Ping, jitter, loss, disconnect, and correction data.
- Equipped item IDs and destruction ledger references.
- Instance/server health at death.

Support restoration is limited to verified systemic server fault. It cannot be purchased or granted selectively to favored customers.

### TECH-123 — Privacy

- Telemetry uses internal pseudonymous account IDs.
- Do not record raw IP beyond security retention needs.
- Do not include email, platform ID, or private social data in public Echo records.
- Privacy notice describes essential service telemetry, optional diagnostics, retention, and deletion/contact process.
- Analytics export excludes staff/test accounts from product KPIs while retaining security logs.

### TECH-124 — Secrets

- No secret in repository, client build, logs, or content data.
- Production secrets use managed environment injection and rotation.
- Database credentials are least privilege and environment-specific.
- Purchase callbacks authenticate platform source.

### TECH-125 — Security acceptance criteria

- Integration tests reject teleport, speed, illegal fire rate, forged hit, stale input, duplicate pickup, replayed craft, and duplicate purchase callback.
- Support can reconstruct item and death history without direct database edits.
- Staff write actions require explicit reason and appear in audit log.
- Secret scanner and dependency vulnerability scan run in CI.
- Incident runbooks exist for duplication, account compromise, unfair death, purchase mismatch, outage, and data restore.

---


## 24. Telemetry and business model

### TEL-001 — Common event envelope

Every gameplay/product event contains:

```text
event_id
event_name
event_schema_version
occurred_at_utc
pseudonymous_account_id
character_id?
session_id
build_id
content_bundle_version
platform
region_id
environment
cohort_tags[]
```

### TEL-002 — Required events

- `account_created`
- `session_started`, `session_ended`
- `tutorial_step_completed`
- `character_created`, `character_entered_combat`
- `level_reached`, `oath_selected`, `bargain_offered`, `bargain_selected`, `bargain_declined`
- `realm_entered`, `event_started`, `event_completed`, `event_failed`
- `dungeon_entered`, `dungeon_completed`, `dungeon_recalled`, `dungeon_extracted`
- `boss_started`, `boss_phase_changed`, `boss_defeated`
- `damage_received`, sampled/aggregated as configured
- `character_died`
- `successor_created`, `successor_entered_combat`
- `echo_created`, `requiem_started`, `requiem_completed`
- `item_created`, `item_picked_up`, `item_equipped`, `item_extracted`, `item_destroyed`, `item_salvaged`, `item_crafted`, `item_gifted`
- `currency_earned`, `currency_spent`
- `party_created`, `party_joined`, `party_activity_started`
- `client_crash`, `disconnect`, `reconnect`, `server_tick_health`
- `store_impression`, `sku_viewed`, `order_created`, `checkout_started`, `payment_authorized`, `entitlement_granted`, `entitlement_revoked`, `purchase_failed`, `refund`, `chargeback`, `reconciliation_completed`, `settlement_received`

### TEL-003 — Death event fields

`character_died` additionally contains:

- Death ID.
- Class, level, oath, active Bargains.
- Lifetime and play session duration.
- Killer and pattern IDs.
- Damage type, raw/final damage, pre-hit health.
- Statuses.
- Dungeon/room/boss phase.
- Party size and contribution.
- Item-power band.
- Ping/jitter/loss/correction state.
- Recall state.
- Cause enum: `direct_hit`, `damage_over_time`, `environment`, `disconnect`, `server_fault`, `administrative_restore`.

`server_fault` cannot remain a final-death result.

### TEL-004 — KPI definitions

- D1/D7/D30: account returns on corresponding UTC calendar day after account creation.
- Tutorial completion: reaches first real character creation after scripted death.
- First-dungeon time: account creation to first normal dungeon entry.
- Fair-death understanding: survey plus whether player can select correct killer/attack from four choices.
- Post-death reroll: successor enters permadeath-enabled combat within `120 seconds` of death summary availability.
- Recovery time: death to successor reaching configured functional-build threshold.
- Crash-free session: no unhandled client termination during session.
- Purchase success: authorized platform transaction reaches correct durable entitlement without manual intervention.
- Three-session rate: percentage of eligible new accounts beginning at least three sessions of `10+ minutes` within seven UTC days of account creation.
- Boss Unique guarantee reached: percentage of accounts with at least one eligible boss clear that redeem 20 matching Fragments before receiving every boss Unique naturally; measured per boss over a 90-day window.
- Refund rate: refunded gross customer amount divided by settled gross customer amount for the same 30-day purchase cohort, excluding fraud-test/staff orders.
- Chargeback rate: charged-back gross customer amount divided by settled gross customer amount for the same 30-day purchase cohort.
- Monthly payer conversion: unique MAU with at least one settled real-money purchase divided by eligible MAU in the UTC calendar month; excludes staff/test/refunded-in-full fraud orders.
- Monthly payer ARPPU: settled NetCashReceived before fixed costs divided by unique valid payers in the same UTC calendar month.

### TEL-005 — Product gates

Use at least `500` new-player accounts for retention decisions, `100` eligible deaths for death-flow decisions, and `10,000` item lifecycle events for economy decisions.

| Gate | Proceed | Iterate | Stop expansion/reconsider |
|---|---:|---:|---:|
| Tutorial completion | ≥75% | 60–74% | <60% |
| First death understandable/fair | ≥80% | 70–79% | <70% |
| Reroll within 120 s | ≥70% | 55–69% | <55% |
| D1 retention | ≥35% | 25–34% | <25% |
| D7 retention | ≥15% | 8–14% | <8% after two focused iterations |
| D30 retention | ≥7% | 4–6% | <4% |
| Crash-free before commerce | ≥99.5% | 99.0–99.49% | <99.0% |
| `grant_success_5m` | ≥99.99% | 99.9–99.989% | Circuit-break checkout below 99.9% |
| `wrong_or_duplicate_grant` | 0 | — | Any occurrence disables checkout |
| `unreconciled_24h` | <0.01% | 0.01–0.05% | >0.05% |
| Refund rate | <3% | 3–5% | >5% |
| Chargeback rate | <1% | 1–1.5% | >1.5% |

### TEL-010 — Unit-economics model

Maintain a spreadsheet with tabs:

`Inputs`, `Cohorts`, `Revenue`, `Variable Costs`, `Fixed Costs`, `Scenarios`, `Sensitivity`.

Required inputs:

- Peak and average CCU, DAU, MAU, new MAU.
- Peak CCU/DAU and DAU/MAU ratios.
- Retention by source/cohort.
- Monthly payer conversion and ARPPU.
- Founder-pack conversion and price.
- Refund, chargeback, platform/payment fee, and sales-tax treatment.
- Server cost per concurrent-player-hour.
- Egress, storage, observability, support, moderation, fraud, and service costs.
- Fully loaded payroll, contractors, software, legal/accounting, localization, art/content, and marketing.
- Acquisition spend and attributed new accounts.
- Regional currency/FX mix and refund/chargeback reserve.

Formulas:

```text
RecurringGrossRevenue = MAU × payer_conversion × monthly_ARPPU
FounderGrossRevenue = new_MAU × founder_conversion × founder_price

NetCashReceived =
  customer_spend
  - tax_withheld_by_platform
  - refunds
  - chargebacks
  - chargeback_fees
  - platform_fee
  - withholding_tax

InfrastructureCost =
  average_CCU × 730 × server_cost_per_CCU_hour
  + egress + storage + observability

SupportCost = MAU × tickets_per_MAU × cost_per_ticket

Contribution =
  NetCashReceived - InfrastructureCost - SupportCost
  - moderation_cost - fraud_cost - other_variable_costs

OperatingProfit = Contribution - fixed_monthly_costs
ContributionPerMAU = net_cash_received_per_MAU - variable_cost_per_MAU
BreakEvenMAU = fixed_monthly_costs / ContributionPerMAU
CAC = acquisition_spend / attributed_new_accounts
LTV180_before_CAC = sum(net contribution per acquired account over first 180 days, excluding acquisition spend)
```

Use platform statement fields as the financial source of truth and do not subtract a tax or fee twice when the statement is already net of it. Reconcile model categories to accounting cash/statement totals monthly.

### TEL-011 — Planning scenarios

Planning assumptions are not forecasts:

| Assumption | Bear | Base | Upside |
|---|---:|---:|---:|
| Monthly payer conversion | 1.0% | 2.5% | 4.0% |
| Monthly payer ARPPU | $10 | $18 | $25 |
| Refund + chargeback | 4% | 2% | 1% |
| D30 retention | 4% | 7% | 12% |

Base illustration using `Peak CCU / DAU = 10%` and `DAU / MAU = 25%`:

| Peak CCU | Estimated MAU | Recurring gross | After 30% platform fee and 2% refunds, before other costs |
|---:|---:|---:|---:|
| 500 | 20,000 | $9,000/month | ~$6,174 |
| 2,000 | 80,000 | $36,000/month | ~$24,696 |
| 10,000 | 400,000 | $180,000/month | ~$123,480 |

Replace ratios with measured telemetry immediately. This model demonstrates why the product must survive financially and mechanically well below 10,000 CCU.

### TEL-012 — Business gates

- Net revenue per MAU exceeds variable cost per MAU.
- Base break-even MAU is no more than `60%` of validated reachable MAU forecast.
- Maintain at least 12 months cash runway before recurring live-service headcount expansion.
- Do not scale paid acquisition until conservatively modeled or observed `LTV180 / CAC ≥ 3.0`.
- Target paid-acquisition payback within 12 months.
- Require positive three-month trailing contribution before materially increasing fixed costs.
- Commerce work begins only after Public Playtest retention and reliability gates pass.

### TEL-013 — Approved business baseline v1

Before commercial Early Access, finance/product owners sign and version a scenario sheet meeting at least:

- D30 retention `≥7%` on the decisive public cohort.
- Fixed payroll expansion is prohibited until observed trailing-90-day contribution is positive; before launch this is a signed post-launch guard, not a claim that 90 live days already exist.
- Contribution margin `(NetCashReceived - variable costs) / NetCashReceived ≥50%` in the Base scenario.
- At least `12 months` cash runway after the planned hiring/content commitment.
- Base-case break-even MAU no greater than `60%` of validated reachable MAU.
- Support tickets no greater than `2% of MAU per month` in the Base scenario.
- Support first-response targets: P0/security/commerce incident `30 minutes`; account/purchase/death dispute `1 business day`; general request `2 business days`.
- Fully loaded cost of one six-to-eight-week content pack no greater than `50%` of the conservative 180-day incremental contribution attributed to that pack.
- No paid-acquisition scale until `LTV180_before_CAC / CAC ≥3.0` with a payback period no longer than 12 months.

Any change to these thresholds creates a new signed baseline version; it cannot be changed merely to pass a release gate.

---


## 25. Quality assurance and playtesting

### QA-001 — Quality strategy

Testing is part of each feature, not a final phase. The minimum stack is:

1. Pure unit tests for formulas and state transitions.
2. Deterministic simulation tests.
3. Content schema and semantic validation.
4. Persistence and idempotency integration tests.
5. Protocol compatibility and malicious-input tests.
6. Headless journey tests with bots.
7. Human feel/readability playtests.
8. Screenshot and video review for representative states.
9. Load, soak, failure, backup, and rollback tests.

Automated tests prove correctness and capacity. Only human testing proves fun, readability, fairness, and reroll desire.

### QA-002 — Required test commands

The repository MUST expose documented commands equivalent to:

```text
format
lint
unit-test
determinism-test
content-validate
persistence-test
protocol-test
journey-test
load-test
build-windows-release
run-local-lab
run-local-stack
```

Exact command names may differ, but one command performs each job and CI uses the same entry points.

### QA-003 — Deterministic fixtures

Required fixtures:

- Player movement against walls at cardinal/diagonal inputs.
- Each class primary, ability, passive, and oath.
- Every status and cap interaction.
- Each pattern primitive.
- Every boss phase at 1, 2, 4, and 8 players.
- Bargain selection and compatibility.
- Item generation from known seed.
- Dungeon generation from known seed.
- Death transaction.
- Recall/extraction transaction.
- Echo assembly from known death record.

Fixtures compare state hashes at selected ticks and produce a human-readable diff on mismatch.

### QA-004 — Content validation suites

- Validate all shipped content on every change.
- Run 10,000 dungeon seeds per dungeon before bundle promotion.
- Run safe-path solver for every boss phase and legal modifier combination.
- Enumerate all class × oath × Bargain × item-cap interactions for global cap violations.
- Run loot distribution simulation for at least one million personal boss rolls per reward-table revision.
- Run 90-day economy simulation before Closed Alpha and every major drop/crafting change.

### QA-005 — Journey tests

Headless or automated clients MUST execute:

1. New account -> tutorial -> create character -> realm.
2. Realm event -> pick up/equip item -> normal extraction -> vault.
3. Realm -> dungeon -> boss -> exit -> stable loot.
4. Take Bargain -> transfer -> reconnect -> Bargain persists.
5. Die -> item destruction -> memorial/Echo -> successor -> combat.
6. Emergency Recall -> equipped survives -> pending inventory destroyed.
7. Party form -> ready -> dungeon -> member disconnect/reconnect -> exit.
8. Gift eligible item -> second gift rejected.
9. Craft/retry same mutation -> one result.
10. Purchase sandbox -> duplicate callback -> refund -> correct entitlement.

### QA-006 — Adverse-network matrix

Test all critical journeys under:

| RTT | Jitter | Loss | Reordering | Expected |
|---:|---:|---:|---:|---|
| 20 ms | 0 | 0 | 0 | Baseline |
| 80 ms | 10 ms | 0.5% | 0.1% | Fully supported |
| 120 ms | 20 ms | 1% | 0.5% | Fair-play target |
| 180 ms | 40 ms | 2% | 1% | Degraded warning; functional |
| 250 ms | 80 ms | 5% | 2% | Severe warning; player is advised to Recall |
| Outage 0.5–5 s | — | 100% | — | Reconnect/LinkLost resolution |

Duplicate packets and stale/replayed actions are included separately.

### QA-007 — Visual review states

Capture at `1280×720`, `1920×1080`, and one ultrawide reference:

- First actionable tutorial view.
- Solo realm fight.
- Forty-player realm event at effect budget.
- Eight-player dungeon room.
- Each boss phase.
- Inventory with pending item warning.
- Bargain choice.
- Sealed-arena warning.
- Low-health and network-warning HUD.
- Death summary.
- Character select and memorial.
- Every accessibility projectile preset.

Canvas/render screenshots are mandatory; DOM or widget-tree assertions cannot prove playfield readability.

### QA-008 — Human playtest protocol

For each scheduled test:

1. Record build/content version and tester cohort.
2. Give no verbal coaching beyond the in-game experience for onboarding tests.
3. Observe first confusion, first damage, first item, first dungeon, first death, and reroll action.
4. Ask tester to identify death cause before showing detailed trace.
5. Ask open-ended: `What felt distinctive?`, `What would make you stop?`, `What do you want to do next?`.
6. Separate observed behavior from opinion.
7. Segment by genre familiarity.
8. File issues with reproduction, player impact, evidence, likely owner, and severity.

### QA-009 — Severity

| Severity | Definition | Examples |
|---|---|---|
| P0 | Active security/data/commerce disaster | Duplication, incorrect durable death, successful charge without entitlement |
| P1 | Release blocker or widespread loss/unplayability | Crash loop, unavoidable boss attack, character stuck, progression blocked |
| P2 | Material defect with workaround | UI state failure, isolated balance exploit, reconnect inconvenience |
| P3 | Minor polish/documentation issue | Cosmetic alignment, noncritical copy, rare visual pop |

P0 triggers immediate kill switch/rollback and incident process. Early Access release requires zero open P0/P1.

### QA-010 — Feature definition of done

A feature is complete only when:

- Feature behavior and state owner match this GDD.
- Values are data-driven.
- Legal and rejected state transitions are tested.
- Loading, empty, success, disabled, error, disconnect, and retry UI states exist where relevant.
- Telemetry contract is implemented and privacy-reviewed.
- Accessibility/readability behavior is verified.
- Content validation and automated tests pass.
- Debug view or support evidence exists.
- Feature flag/kill switch exists if live risk warrants it.
- Rollback and migration behavior are documented.
- Acceptance criteria can be executed by a different agent/person without interpretation.

### QA-020 — QA acceptance criteria

- CI blocks invalid content and failing critical tests.
- Release candidate completes `72 hours` of production-shaped soak.
- 100,000 randomized inventory/death/purchase mutation sequences create zero duplication or impossible resurrection.
- Crash-free session and server tick gates meet Section 27.
- Visual review has no unresolved hostile-projectile, safe-zone, exit, or center-screen obstruction issue.

---


## 26. Live operations and content cadence

### PRD-100 — Pre-Early-Access operations

- No formal season.
- All test progress is explicitly wipeable.
- Run one shared `Chapter 0` content theme.
- Use fixed daily dungeon seed and one rotating legal modifier only for test coverage, not retention pressure.
- No purchases until durable Early Access namespace and commerce gate.
- Stage and rollback-test at least four weeks of expected fixes/small content before launch.

### PRD-101 — Early Access cadence

| Cadence | Deliverable |
|---|---|
| Immediate | Security, duplication, purchase, crash, and unavoidable-death hotfixes |
| Weekly rotation | Dungeon favor, fixed-loadout challenge, one world modifier; no login streak |
| Every two weeks | Bug-fix and tuning build |
| Every six to eight weeks | Content pack: one encounter/pattern variant, four items, one earnable cosmetic, one or two paid cosmetics |
| Every 12 weeks | Chapter: one world rule, one boss/remix, four to six items, challenge ladder, six to ten cosmetics |

Begin formal named seasons only after two Chapters ship on time and the next Chapter is at least `80%` complete.

### PRD-102 — Ethical retention rules

- No mandatory daily quest or login streak.
- Missing a rotation does not permanently reduce power.
- Weekly goals may accumulate or be completed through ordinary play.
- Store rotation changes visibility, not permanent availability.
- No death-screen offer.
- No paid FOMO tied to combat advantage.
- Session-ending reminders, maintenance notices, and healthy stopping points are explicit.

### PRD-103 — Population structure

- Maintain one shared population at launch.
- Chapters are shared-world overlays and fixed-loadout ladders, not separate realm populations/economies.
- Do not create separate seasonal/permanent queues until each target region sustains at least `1,500 peak CCU` for eight weeks.
- Low population consolidates realm creation at safe boundaries; core progression remains solo.

### PRD-104 — Incident rules

- Confirmed duplicate grant or ledger mismatch disables affected source/craft immediately.
- Checkout circuit breaker follows MON-012: any wrong/duplicate grant or `grant_success_5m <99.9%` across the latest 100 authorized orders.
- Unfair encounter death disables that encounter for new instances pending investigation.
- Economy exploit uses narrow ledger-based correction rather than broad rollback when possible.
- Compensation is account-wide and consistent for affected cohorts; never selective paid-customer resurrection.
- Incident communication states impact, current safety, mitigation, and follow-up without exposing exploit details prematurely.

### PRD-105 — Content retirement

- Horizontal items are not invalidated solely to force new acquisition.
- A broken item may be rebalanced with public notes and item-version migration.
- Cosmetic ownership persists.
- Removed encounters retain memorial/codex history.
- Chapter mechanics may enter an archive pool only after readability, balance, and maintenance review.

### PRD-106 — Live-ops acceptance criteria

- Every scheduled config has owner, validation, kill switch, and rollback target.
- One bad modifier, item, encounter, or SKU can be disabled without full server shutdown.
- First post-launch content pack deploys and rolls back in test without schema emergency.
- Operations team can identify realm, build, content, account, death, and purchase state from one incident ID chain.

---


## 27. Release gates

### QA-100 — First Playable gate

- Build enters controllable combat in under `10 seconds` on development target.
- Movement, aim, held primary fire, two abilities, potion, damage, death, and restart work.
- Three enemy roles and one three-phase test boss exist.
- Eight of ten testers identify their killer correctly.
- Seven of ten voluntarily restart and want another attempt.
- Median combat feel rating is at least `4/5`.
- Deterministic and stress fixtures pass.

If this fails, tune combat. Do not add progression/content as compensation.

### QA-101 — Complete Private Loop gate

- Character select -> hub -> realm -> dungeon -> boss -> extraction -> vault works without developer commands.
- Permadeath transaction, memorial, successor, and one Bargain work.
- Server restart preserves committed inventory/death state.
- Replayed mutations create no duplicates.
- Median login-to-control under `30 seconds`.
- Median death-to-successor control under `15 seconds`.
- Post-death reroll within two minutes at least `70%`.

### QA-102 — Vertical Slice gate

- Two classes, two dungeons, 45 items, six Bargains, three modifiers, and personal Requiem exist.
- 25–50 testers complete at least three scheduled sessions.
- At least `60%` name Echoes or Bargains as distinctive without prompting.
- At least `70%` reach first dungeon within `15 minutes`.
- At least `70%` reroll after a meaningful death.
- At least `70%` rate death fairness `4/5` or better.
- 10,000 dungeon seeds produce zero shipped invalid seeds.

### QA-103 — Closed Alpha gate

Use at least `100` external participants:

- Tutorial completion `≥70%`.
- Median first session `≥35 minutes`.
- D1 `≥30%`; D7 `≥12%`.
- Three-session rate in seven days `≥25%`.
- Reroll `≥70%`.
- Fair-death rating `≥70%` at 4/5+.
- Would recommend current state `≥60%`.
- Crash-free sessions `≥99.5%`.
- No known duplication, impossible resurrection, or permanent state-loss defect.

M05 retention results are directional because the cohort is below TEL-005's 500-account decisive threshold. They determine whether the product is healthy enough to enter M06; they do not validate commercial launch assumptions.

If product metrics fail, run at most two focused two-week combat/onboarding/death-loop iterations before reconsidering scope. Do not substitute content volume for retention.

### QA-104 — Public Steam Playtest gate

- Target at least `2,500 accounts` and `500 D1-eligible users`.
- Tutorial completion `≥75%`.
- D1 `≥35%`, D7 `≥15%`, three-session rate `≥30%`.
- At least `500 D30-eligible accounts` with D30 `≥7%`; extend the Playtest if the cohort has not matured.
- At least `100` surveyed real permadeaths with correct lethal-cause identification `≥80%` and fairness rating `4/5+ ≥75%`.
- First dungeon within 15 minutes `≥70%`.
- Reroll `≥70%`.
- Crash-free `≥99.7%`.
- Served-region latency p95 `≤120 ms`.
- Server tick p95 `≤20 ms`, p99 `≤30 ms`.
- 72-hour synthetic soak at 3× forecast creates no unrecoverable state.
- Backup restore meets RPO/RTO.
- Break-even model is documented and credible.

Commerce work begins only after this decisive public gate.

### QA-105 — Early Access release candidate

All conditions are mandatory:

- Feature/content freeze for seven days.
- Zero open P0/P1 defects.
- Every accepted P2 has owner and visible workaround.
- Crash-free sessions `≥99.8%`.
- Three-times-forecast load remains within budgets.
- Client/content rollback rehearsed.
- Database backup and redeploy rehearsed.
- 100,000 randomized mutation sequences produce zero duplication/impossible resurrection.
- Every death has authoritative cause, latency, state, and content trace.
- Payment sandbox duplicate/delayed/missing/refund/reconciliation tests pass.
- Store contains cosmetics only and all are previewable.
- Support, incident, moderation, privacy, and accounting owners are named.
- 72-hour release-candidate soak has no severity-one incident.

### QA-106 — Version 1.0 gate

- Five polished classes.
- Two realm regions.
- Six dungeons and eight major bosses.
- Exactly 180 item templates with 30–40 behavior-changing Uniques.
- Two successful content updates delivered six to eight weeks apart.
- One nonmonetized shared-world Chapter ruleset validated without population split.
- One 12-week Chapter delivered on time and the next Chapter at least `80%` complete before declaring cadence sustainable.
- Controller support and complete accessibility review.
- Economy stable for 90 days.
- Content pipeline meets cadence.
- D30, operating margin, support load, and runway meet approved business model.

---


## 28. Development roadmap summary

The detailed work breakdown is in `Gravebound_Development_Roadmap_v1.md`.

### PRD-120 — Planning assumption

Calendar targets assume approximately four dedicated contributors covering gameplay/network, backend/tools, content/design, and art/UI, with part-time QA/audio/operations. A solo developer directing AI should retain the exact milestone order and expect approximately `2.5–4×` the elapsed time.

### PRD-121 — Milestones

| ID | Target | Outcome | Public status |
|---|---:|---|---|
| `GB-M00` | Calendar days 1–3 | Minimum reproducible M01 workspace, toolchain, CI, and task contracts | None |
| `GB-M01` | Working days 4–13 | Ten-day local combat laboratory and First Playable | 10-person blind cohort |
| `GB-M02` | Weeks 3–5 | Authoritative network loop | Internal |
| `GB-M03` | Weeks 6–8 | First complete private character life | 10–20 invited |
| `GB-M04` | Weeks 9–13 | Distinctive Vertical Slice with Bargains/Echoes | 25–50 invited |
| `GB-M05` | Weeks 14–18 | Closed external Alpha with EA content scope | 100–300 accounts |
| `GB-M06` | Weeks 19–26 | Public Steam Playtest, extended until D30 cohort matures | 2,500+ accounts; CCU capped |
| `GB-M07` | Weeks 27–32 | Commerce, durability, polish, release candidate | Private/staged |
| `GB-M08` | Week 33 + 30 days | Commercial Early Access and stabilization | Public |
| `GB-M09` | 6–12 months after EA | Health-gated Version 1.0 | Public |

These are target timeboxes, not permission to waive a gate.

### PRD-122 — Critical stop/go decisions

- After M01: Is combat fun without progression? If no, tune it.
- After M03: Does permadeath create immediate reroll desire? If no, fix fairness/recovery.
- After M04: Are Echoes and Bargains meaningfully distinctive? If no, redesign them.
- After M05: Do external users voluntarily return? If no, do not add content breadth.
- After M06: Are retention and unit economics credible? If no, do not build commerce.
- After M07: Can the team preserve accounts, purchases, and deaths under failure? If no, do not launch.

### PRD-123 — Fastest playable commitment

The first product demonstration is M01, not Early Access. It MUST contain:

- Grave Arbalist.
- One arena.
- Three enemies.
- One three-phase test boss.
- Twelve prototype equipment templates plus Red Tonic.
- Movement, aiming, primary fire, two abilities, potion.
- Fair death recap.
- Restart into control in under three seconds.
- Hitbox, pattern, spawn, performance, and time-scale debug tools.

No account, store, realm scheduler, crafting, guild, market, browser, or production deployment work may delay this build.

Exact First Playable entity IDs, arena coordinates, drop odds, item values, and boss timelines are defined in `Gravebound_Content_Production_Spec_v1.md`; implementations MUST use those records rather than inventing equivalents.

---


## 29. AI implementation task template

Use this template verbatim for implementation work:

```text
Task ID:
Title:
Milestone:
Feature IDs:

Player-visible outcome:

In scope:
-

Out of scope:
-

Authoritative owner:
Client presentation owner:

Dependencies:
- Code modules:
- Content IDs/schemas:
- Previous tasks:

Exact rules and prototype defaults:
-

Legal state transitions:
-

Rejected inputs and typed errors:
-

Network messages/version behavior:
-

Persistence and idempotency:
-

UI states:
- Loading
- Empty
- Success
- Disabled
- Error
- Disconnected
- Retry

Telemetry events/fields:
-

Accessibility/readability:
-

Tests required:
- Unit
- Deterministic
- Integration
- Adversarial/failure
- Content validation
- Visual/manual

Feature flag/kill switch:
Rollback/migration behavior:

Acceptance criteria:
1.
2.
3.

Files/modules allowed to change:
-

Completion report must include:
- Changed files
- Tests run and results
- Acceptance evidence
- Remaining edge cases or spec conflicts
```

### PRD-130 — Task splitting rules

- One task SHOULD own one authoritative state change or one player-visible vertical behavior.
- Do not split server rule from its minimum client feedback and tests if that would leave an unusable intermediate state.
- Content population tasks follow schema/mechanic implementation tasks.
- A task exceeding five independently testable outcomes should be split.
- A failed test or unresolved spec conflict cannot be relabeled as future polish.
- AI agents may propose a change request but may not alter this GDD implicitly through code.

### PRD-131 — Feature registry fields

Maintain a machine-readable registry with:

```text
feature_id
title
milestone
status
authoritative_owner
dependencies[]
content_ids[]
telemetry_events[]
test_ids[]
feature_flag
spec_version
implementation_commit
known_limitations[]
```

---


## 30. Risks, deferrals, glossary, and references

### PRD-140 — Risk register

| Risk | Probability/impact | Leading indicator | Mitigation/trigger |
|---|---|---|---|
| Combat is technically correct but not fun | High/critical | M01 feel score or reroll desire fails | Stop content; tune movement, aim, patterns, feedback |
| Permadeath feels unfair | High/critical | Cause identification/fairness below gates | Improve telegraphs, damage bands, network grace, trace UX |
| Recovery is too slow | Medium/high | Functional-build recovery >45 min | Improve deterministic forge/drop pacing, not paid relief |
| Death feels meaningless | Medium/high | Recovery <30 min and low attachment | Increase build/story identity and deeds before raw grind |
| Echo/Bargain hook is not distinctive | Medium/critical | Unprompted recall <60% | Redesign at M04; do not hide with more content |
| Group fights become unreadable | High/high | Eight-player screenshot/death spikes | Reduce group FX, target count, projectile budget; keep max 8 |
| Economy inflates or starves | Medium/high | Sink/destruction ratios outside ranges | Tune sources/sinks using segmented simulation |
| Paid cosmetics cannot support team | High/critical | Base break-even MAU exceeds reachable forecast | Keep team/costs small; improve retention/catalog; reconsider model before expansion |
| Low population blocks progression | Medium/critical | Long waits/empty realms | Solo scaling, deterministic climax, Hall contracts, one population |
| Rust/Bevy iteration is slower than expected | Medium/high | M01/M02 slip with tooling friction | Keep content data simple; benchmark early; reconsider only through ADR |
| Browser work derails native | High/high if started early | Platform-specific failures on critical path | Defer public browser; only bounded spike after network slice |
| Backend overengineering delays fun | High/high | Infra tasks before M01/M03 gates | Modular monolith; no premature service/orchestration split |
| Duplication or wrong death corrupts trust | Medium/critical | Ledger mismatch or irreproducible death | Idempotent transactions, kill switches, traces, no launch with P0/P1 |
| Live cadence burns out team | High/high | Chapter backlog <80%; repeated slip | No seasons until two ordinary Chapters ship on time |
| Bots/RMT emerge | Medium/high | Repetitive farms, gift graph anomalies | No open trade, server authority, review tools, source caps |

### PRD-141 — Explicit deferrals

| Feature | Reason | Reconsideration gate |
|---|---|---|
| Browser/WASM public client | Doubles performance, transport, packaging, and QA surface | Native EA stable, measured demand, profitable operation |
| macOS/Linux/Steam Deck certification | Platform QA before product validation | After EA stabilization and demand measurement |
| Global trading/auction house | Fraud, bots, inflation, discovery distortion | ECO-020 gates and separate approved spec |
| Guilds/guild halls | Social/backend/moderation scope | After party participation proves value; Version 1.0 candidate |
| 20–40 player dungeons | Readability and balance risk | Only after 8-player visual/performance proof |
| Multiple release regions | Operations cost before audience known | Latency demand and sustainable population |
| Seasons/battle pass | Cadence debt and fragmentation | Two Chapters on time, next 80% complete |
| Paid storage/slots | Economic power in permadeath | Not planned |
| Deep crafting | Inventory administration and balance | 90 days of satisfying found-loot economy |
| Public text chat | Moderation burden | Tooling/staffing and explicit value case |
| Voice chat | Safety and operations | External platform solution if justified |
| Full public replay sharing | Storage/privacy/version cost | Short trace system proven first |
| Kubernetes/microservice split | Slower delivery | Measured independent scaling/reliability boundary |
| Multi-region failover | Cost and complexity | Proven audience and operational maturity |
| Eight classes/five regions/16 dungeons | Content before validation | Post-1.0 expansion |
| Mobile/console | Different control/product needs | No commitment |

### PRD-142 — Glossary

| Term | Meaning |
|---|---|
| Account | Persistent Gravebound order and ownership identity |
| Character/hero | Mortal playable entity subject to permadeath |
| Pending run inventory | Loot not secured by normal extraction; lost on death/Recall |
| Equipped | Combat items active on a living hero; lost on death, preserved by Recall |
| Emergency Recall | Fast escape preserving hero/equipment but abandoning pending loot |
| Extraction | Normal successful exit that secures pending loot |
| Oath | Level-10 class specialization, changeable only in Hall |
| Veil Bargain | Optional paired boon/curse lasting for one character life |
| Memorial | Persistent record of a dead or retired hero |
| Echo | Controlled encounter derived from a qualifying death record |
| Requiem | Personal instance for confronting one account Echo |
| Realm | Shared public combat instance and event cycle |
| Dungeon | Instanced 1–8 player authored/procedural room sequence |
| Content bundle | Immutable versioned set of validated gameplay definitions/assets |
| Functional build | Configured item-power threshold sufficient for recommended Standard content |
| Chapter | Shared-world content update; does not split population by default |

### PRD-143 — External decision references

- Bevy 0.19 release notes: <https://bevy.org/news/bevy-0-19/>
- Bevy official WebGPU/WASM examples and support warning: <https://bevy.org/examples-webgpu/>
- Steam Early Access guidance: <https://partner.steamgames.com/doc/store/earlyaccess?language=english>
- Steam free-to-play guidance: <https://partner.steamgames.com/doc/store/freetoplay?l=english&language=english>
- Steam microtransaction and fraud guidance: <https://partner.steamgames.com/doc/features/microtransactions>

### PRD-144 — Final product acceptance statement

Gravebound is ready to expand only when players demonstrably enjoy the combat without progression, understand and accept death, immediately want a successor, and name Echoes/Bargains as a reason to choose this game over alternatives. Technical scale, content volume, and monetization do not substitute for those results.

---
