# Gravebound: The Ashen Veil
## Development Roadmap — First Playable to Version 1.0

| Field | Value |
|---|---|
| Document ID | `GB-ROADMAP-001` |
| Version | `1.0.0` |
| Date | 2026-07-10 |
| Canonical design | `Gravebound_Production_GDD_v1_Canonical.md` |
| Content production contract | `Gravebound_Content_Production_Spec_v1.md` |
| Planning unit | Five-day engineer-week unless stated otherwise |
| Primary objective | Put a fun local build in players' hands before building MMO breadth |
| First commercial target | Windows Steam Early Access |

---

## Contents

- [Delivery strategy](#1-delivery-strategy)
- [Planning assumptions](#2-planning-assumptions)
- [Locked Early Access scope](#3-locked-early-access-scope)
- [Milestone overview](#4-milestone-overview)
- [Dependency and parallel-work model](#5-dependency-and-parallel-work-model)
- [M00 — Foundation](#6-gb-m00--scope-lock-and-reproducible-foundation)
- [M01 — Ten-day combat laboratory](#7-gb-m01--ten-day-combat-laboratory)
- [M02 — Authoritative network loop](#8-gb-m02--authoritative-network-loop)
- [M03 — Complete private character life](#9-gb-m03--first-complete-private-character-life)
- [M04 — Distinctive Vertical Slice](#10-gb-m04--distinctive-vertical-slice)
- [M05 — Closed external Alpha](#11-gb-m05--closed-external-alpha)
- [M06 — Public Steam Playtest](#12-gb-m06--public-steam-playtest)
- [M07 — Early Access release candidate](#13-gb-m07--early-access-release-candidate)
- [M08 — Commercial Early Access](#14-gb-m08--commercial-early-access)
- [M09 — Version 1.0](#15-gb-m09--version-10)
- [Test population ladder](#16-test-population-ladder)
- [Initial implementation ordering index](#17-initial-implementation-ordering-index)
- [Critical-path controls](#18-critical-path-controls)
- [Deferred backlog](#19-explicitly-deferred-backlog)
- [Roadmap maintenance](#20-roadmap-maintenance)
- [Immediate next action](#21-immediate-next-action)

---

## 1. Delivery strategy

Ship increasingly complete playable slices:

```text
reproducible foundation
  -> local combat laboratory
  -> authoritative network loop
  -> complete private character life
  -> distinctive vertical slice
  -> closed external alpha
  -> public Steam Playtest
  -> Early Access release candidate
  -> commercial Early Access
  -> health-gated Version 1.0
```

Hard rules:

- Windows native PC first; Steam is the first public identity/distribution platform.
- Use Rust/Bevy throughout.
- The local prototype uses the same `sim_core` later executed by the server.
- Build one authoritative modular-monolith server, one PostgreSQL database, and one deployment region first.
- Pre-Early-Access accounts are explicitly wipeable. Perform the final wipe immediately before durable Early Access persistence begins.
- Do not accept money until retention, persistence, support, rollback, and purchase-reconciliation gates pass.
- Stop feature expansion at a failed validation gate.
- Browser, unrestricted trading, guilds, seasons, raids, and service decomposition are deferred.
- Dates are targets, not permission to waive acceptance criteria.

## 2. Planning assumptions

Calendar targets assume roughly four dedicated contributors:

| Workstream | Primary responsibility |
|---|---|
| Gameplay/network | `sim_core`, client movement/combat, prediction, patterns |
| Backend/tools | Server app, persistence, content tools, deployment, support tooling |
| Content/design | Classes, enemies, rooms, bosses, items, balance, playtests |
| Art/UI | Pixel assets, VFX, HUD/menus, audio coordination, accessibility |

Part-time needs:

- QA and external test coordination.
- Audio production.
- Security/operations review before public tests.
- Legal, privacy, accounting, and platform integration before commerce.

A solo developer directing AI should keep the same dependency order and expect approximately `2.5–4×` the elapsed time. Additional contributors shorten parallel content/presentation work but do not eliminate validation time.

## 3. Locked Early Access scope

| Category | Early Access scope |
|---|---|
| Platform | Windows 10/11 native Steam client; keyboard/mouse |
| Structure | One hub, one public realm region, three instanced dungeons |
| Classes | Ashen Vanguard, Grave Arbalist, Veil Witch |
| Group size | Solo–8 in dungeons; public realm cap initially 40 |
| Character progression | Levels 1–20; oath at 10; functional recovery in 30–45 minutes for experienced players |
| Encounters | Four major bosses, six minibosses, exactly 18 regular enemy definitions |
| Loot | 90 item templates, constrained affixes, 12 Black Uniques |
| Replay systems | 12 Veil Bargains, six dungeon modifiers, personal Fallen Hero Echo/Requiem |
| Account systems | Character select, equal free slots, vault, codex, memorials, mastery without permanent combat stats |
| Economy | Personal loot, death destruction, salvage, four deterministic crafting actions, party gifting only |
| Social | Steam friends, parties, join codes, pings, mute/block/report; no public text chat |
| Commerce | `US$0` free-to-play base game; cosmetics only; one Founder Supporter Pack SKU, exactly 10 direct cosmetic SKUs, and exactly six earnable cosmetic sets; Content CONT-COS-001 is the catalog allowlist |
| Operations | One region, queues, telemetry, admin tools, backup/restore, patch/content rollback |

No milestone may silently expand this table.

## 4. Milestone overview

| ID | Target timebox | Public status | Exit population |
|---|---:|---|---:|
| `GB-M00` | Days 1–3 | None | Automated |
| `GB-M01` | Working days 4–13 | Local First Playable | At least 10 blind testers |
| `GB-M02` | Weeks 3–5 | Internal network build | 16 bots, 4 humans |
| `GB-M03` | Weeks 6–8 | Complete private loop | 10–20 humans |
| `GB-M04` | Weeks 9–13 | Distinctive Vertical Slice | 25–50 humans |
| `GB-M05` | Weeks 14–18 | Closed external Alpha | 100–300 accounts; 30–50 peak CCU |
| `GB-M06` | Weeks 19–26 | Public Steam Playtest | 2,500+ accounts; 250 CCU cap; D30 matured |
| `GB-M07` | Weeks 27–32 | Early Access release candidate | Synthetic 3× forecast |
| `GB-M08` | Week 33 plus first 30 days | Commercial Early Access | Staged rollout |
| `GB-M09` | 6–12 months after EA | Version 1.0 | Health-gated |

### Stage-name mapping

| GDD product stage | Roadmap milestone |
|---|---|
| First Playable | `GB-M01` |
| Core Prototype | `GB-M03` |
| Networked Vertical Slice | `GB-M04` |
| Closed Alpha | `GB-M05` |
| Public Steam Playtest | `GB-M06` |
| Early Access release candidate | `GB-M07` |
| Early Access | `GB-M08` |
| Version 1.0 | `GB-M09` |

## 5. Dependency and parallel-work model

```text
M00 workspace/schemas/simulation
 ├─> M01 combat laboratory
 │    ├─> M02 authority/networking
 │    │    └─> M03 persistence/full loop
 │    │         └─> M04 signature systems/group content
 │    │              └─> M05 retention/economy Alpha
 │    │                   └─> M06 public operations
 │    │                        └─> M07 commerce/durability
 │    │                             └─> M08 launch
 │    │                                  └─> M09 1.0
 │    └─> content and presentation prototypes
 └─> CI, validators, test harness, and debug surfaces continue through every milestone
```

Parallel work is permitted only after its dependency is stable:

- Art may begin from an approved seed frame and locked collision/footprint.
- Item content may begin after item schema and one weapon behavior work end-to-end.
- Boss content may begin after pattern primitives, debug overlays, and fairness validator exist.
- Backend persistence may begin after the character/item state machine is approved.
- Commerce may begin only after Public Playtest gates pass.
- Content and art tracks start after M01 locks footprints and continue in parallel; their milestone burn-up is mandatory, not optional background work.

### Required architecture decisions

Each ADR MUST name the chosen option, rejected options, migration cost, owner, and validation fixture.

| ADR | Deadline | Decision |
|---|---:|---|
| `ADR-001` | M00 | Simulation/render boundary, fixed-tick ownership, deterministic RNG algorithm/crate version, stream partitioning, seed construction, and golden fixtures |
| `ADR-002` | M01 | HUD/menu layer: Bevy UI, retained-mode library, or narrowly scoped DOM-equivalent overlay |
| `ADR-003` | M02 start | QUIC implementation, datagram fallback, message codec, and protocol versioning |
| `ADR-004` | M03 start | PostgreSQL access layer, transaction boundaries, migrations, and test database policy |
| `ADR-005` | M03 | Telemetry, crash collection, privacy redaction, and event-version pipeline |
| `ADR-006` | M03 | Hosting/IaC baseline, TLS, DNS, secrets, environment isolation, and cost ceiling |
| `ADR-007` | M05 | Steam identity/depot binding and wipe-to-live account transition |

### Cumulative content and asset burn-up

These are implementation-complete, in-engine, validator-clean counts—not concept sketches. “Ship quality” means no known readability, anchor, licensing, or provenance defect; temporary assets are registered and cannot enter M07 bundles.

| Milestone | Classes | Normal enemies | Minibosses | Major bosses | Dungeon room templates | Realm landmark kits | Item icons | Combat VFX cues | Audio cues | UI surfaces | Ship-quality share |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| M01 | 1 | 3 | 0 | 1 benchmark | 1 arena | 0 | 13 | 10 | 12 | 5 | 20% |
| M03 | 1 | 6 | 2 | 1 | 9 | 2 | 18 | 18 | 28 | 10 | 35% |
| M04 | 2 | 12 | 4 | 2 | 20 | 5 | 45 | 34 | 55 | 18 | 60% |
| M05 | 3 | 18 | 6 | 4 | 34 | 10 | 90 | 52 | 90 | 26 | 85% |
| M06 | 3 | 18 | 6 | 4 | 34 | 10 | 90 | 52 | 90 | 26 | 100% |

Content/design, art/UI, and audio owners update this table weekly. Missing cumulative count is displaced milestone scope unless the change log names an accepted substitution.

Required scene kits outside the dungeon-room/landmark columns are still scheduled work: M01 Combat Laboratory; M03 graybox Lantern Halls and Core micro-realm; M04 personal Requiem arena and the production realm mask; M05 ship-quality Lantern Halls, Training Crypt, shared class-tutorial arena, Mastery Trial, and Bell Warden public arena. They may reuse tiles, but each needs collision, anchors, lighting/palette, navigation, readability, localization, and fixed-trace validation; none is hidden contingency scope.

---

## 6. GB-M00 — Scope lock and reproducible foundation

**Timebox:** Days 1–3  
**Objective:** Any developer or AI agent can build, test, run, and modify the project without inventing structure.

### Work packages

| ID | Deliverable | Dependencies | Primary owner |
|---|---|---|---|
| `GB-M00-01` | Minimum Rust workspace with `client_bevy`, `sim_core`, `sim_content`, `content_schema`, and `tools_content`; remaining target crates are milestone-scoped | None | Gameplay/backend |
| `GB-M00-02` | Pinned toolchain, `Cargo.lock`, format/lint rules, logging, environment templates | M00-01 | Backend |
| `GB-M00-03` | Documented build, test, local-lab, local-stack, and headless commands | M00-01 | Backend |
| `GB-M00-04` | CI for format, lint, unit tests, content validation, Windows release build | M00-02 | Backend |
| `GB-M00-05` | 30 Hz fixed-step simulation clock and deterministic RNG service | M00-01 | Gameplay |
| `GB-M00-06` | Stable entity/content IDs and feature registry schema | M00-01 | Gameplay/design |
| `GB-M00-07` | Minimum versioned schemas for class, ability, enemy, pattern, arena, item, and drop table; schema breadth grows only with consuming features | M00-06 | Design/tools |
| `GB-M00-08` | Fixed-input trace runner and selected-tick state-hash output | M00-05 | Gameplay/QA |

### Exit gate

- Clean machine can clone and produce runnable Windows build with documented commands.
- CI passes twice from clean caches.
- Invalid IDs, missing references, illegal M01 data combinations, and schema mismatches fail the build; later schemas add their own validators when introduced.
- Known simulation seed produces the same state hash in two clean runs.
- Every next task uses a stable feature ID and acceptance criteria.

---

## 7. GB-M01 — Ten-day combat laboratory

**Timebox:** Ten working days beginning on calendar day 4 and ending no earlier than calendar day 13  
**Objective:** Produce a genuinely enjoyable local build before accounts, infrastructure, or content scale.

### Daily target

| Working day | Required result |
|---:|---|
| 1 | Bevy window, orthographic camera, fixed-step player, debug grid, WASD movement |
| 2 | Mouse aim, held primary fire, projectile/hitbox debug, collision |
| 3 | Grave Arbalist abilities, cooldown/resource feedback, consumables |
| 4 | Chaser, fan shooter, area-denial enemy; spawn menu and time scale |
| 5 | Three-phase test boss with aimed fan, ring gap, lane, and break window |
| 6 | Damage bands, health/status HUD, fair death trace, restart under 3 seconds |
| 7 | Twelve item templates, drops, field equip, visible behavior changes |
| 8 | First audio/VFX/readability pass and accessibility effect controls |
| 9 | Performance fixture, fixed-input trace/state-hash playback, test feedback fixes |
| 10 | Blind playtest with at least 10 external-to-feature testers and gate decision |

### Work packages

| ID | Deliverable |
|---|---|
| `GB-M01-01` | Input actions, normalized movement, camera, rebind-ready action map |
| `GB-M01-02` | Grave Arbalist primary, Grave Mark, Slipstep, passive |
| `GB-M01-03` | Three enemy roles and authored arena |
| `GB-M01-04` | Three-phase benchmark boss |
| `GB-M01-05` | Combat contract: hitboxes, damage order, speed/gap/telegraph caps |
| `GB-M01-06` | Cause-of-death display and restart flow |
| `GB-M01-07` | Twelve test items with at least four behavior changes |
| `GB-M01-08` | Hitbox, threat, timeline, spawn, invulnerability, seed, and performance debug tools |
| `GB-M01-09` | Repeatable projectile-density benchmark |
| `GB-M01-10` | Playtest instrumentation and survey |
| `GB-M01-11` | `consumable.red_tonic`: belt input, restore 30% max health over 0.4 s, 2 s shared cooldown, stack-6/death/restart behavior, audio/UI feedback |

### Exit gate

- At least 8 of 10 testers correctly state what killed them.
- At least 7 of 10 voluntarily restart and want another attempt.
- Median movement/shooting/dodging score at least 4/5.
- Target PC maintains 60 FPS at 1080p with stress fixture.
- Twenty deterministic boss runs complete without crash, soft lock, unavoidable pattern, or inconsistent state hash.

**Failure response:** Continue M01 tuning. Do not add classes, realm scheduling, crafting, or account systems.

---

## 8. GB-M02 — Authoritative network loop

**Timebox:** Weeks 3–5  
**Objective:** Establish final authority boundaries before persistence or content expansion.

### Work packages

- `GB-M02-00`: Add `protocol`, `server_app`, and `bot_client` crates with CI targets and ownership boundaries.
- `GB-M02-01`: Versioned handshake, session, input frames, snapshots, reliable events, error codes.
- `GB-M02-02`: Server authority for movement, attacks, cooldowns, projectiles, collision, health, death, eligibility, pickup.
- `GB-M02-03`: Local movement prediction, remote interpolation, reconciliation, deterministic projectile presentation.
- `GB-M02-04`: Join, leave, timeout, LinkLost, reconnect, duplicate-session handling, clean shutdown.
- `GB-M02-05`: Latency/jitter/loss/duplication/reordering/outage harness.
- `GB-M02-06`: Reject teleport, speed, fire-rate, forged hit, duplicate pickup, stale/replayed input.
- `GB-M02-07`: Headless bot that moves, aims, fights, picks up, dies, Recalls, reconnects.
- `GB-M02-08`: Realm/arena instance lifecycle and diagnostics.

### Exit gate

- Four humans complete the combat test together.
- Sixteen bots run for two hours without crash, memory growth, invalid state, or simulation stall.
- At 100 ms RTT, 20 ms jitter, and 1% loss, control remains playable and accepted deaths match authoritative traces.
- Server tick p95 ≤20 ms and p99 ≤30 ms.
- All malicious/mutation tests pass.

---

## 9. GB-M03 — First complete private character life

**Timebox:** Weeks 6–8  
**Objective:** Deliver the smallest complete Gravebound loop to private testers.

### Work packages

| ID | Deliverable |
|---|---|
| `GB-M03-01` | Wipeable test identity, character creation/select, one class |
| `GB-M03-02` | PostgreSQL account, character, item, vault, memorial, ledger persistence |
| `GB-M03-03` | Graybox Lantern Hall, micro-realm, explicit instance transfers, character select -> Hall -> micro-realm -> six-room dungeon -> boss -> Hall |
| `GB-M03-04` | Levels 1–10, XP, 18 items, four slots, pending inventory, vault |
| `GB-M03-05` | First Arbalist oath shrine presenting both exact oath choices plus one three-choice Veil Bargain shrine |
| `GB-M03-06` | Atomic permadeath, deterministic destruction, memorial |
| `GB-M03-07` | Successor preset, starter kit, return to control |
| `GB-M03-08` | Normal extraction and Emergency Recall loss rules |
| `GB-M03-09` | Add `telemetry` crate; onboarding/death/loot/session telemetry and crash-redaction contract |
| `GB-M03-10` | Support/debug lookup for character, item, death |
| `GB-M03-11` | Add `persistence` crate, migrations, transactional repositories, and ephemeral PostgreSQL test stack |
| `GB-M03-12` | Minimal Ash wallet with idempotent earn/spend ledger and retry/duplicate tests |
| `GB-M03-13` | Atomically persist a qualifying Fallen Hero Echo and run oldest-first promotion (`Dormant -> Available` when none is Available); no encounter assembly yet |
| `GB-M03-14` | Steamworks partner/legal/platform account setup and hosting/IaC ADR; no Steam runtime dependency |

### Exit gate

- Tester completes every step without developer commands.
- Server restart preserves committed inventory, vault, death, and memorial state.
- Mutation retry cannot duplicate item/currency/character.
- A qualifying death transaction atomically writes the death, item destruction, memorial, and dormant Echo record or writes none of them.
- Median login-to-control under 30 seconds.
- Median death-to-successor control under 15 seconds.
- At least 70% of deaths produce successor combat within two minutes.
- At least 80% of testers explain their latest death correctly.
- Twenty-five scripted full-loop journeys pass.

---

## 10. GB-M04 — Distinctive Vertical Slice

**Timebox:** Weeks 9–13  
**Objective:** Prove the product has a reason to exist beyond baseline genre imitation.

### Content target

- Two classes.
- Mire of Bells realm slice.
- Two dungeons and two major bosses.
- Four minibosses and 12 normal enemies.
- 45 items and 12 affixes.
- Six Bargains and three dungeon modifiers.
- Personal Fallen Hero Echo/Requiem.
- Parties of 1–4; eight-player proof moves to M05.

### Work packages

- `GB-M04-01`: Ashen Vanguard plus balance fixtures for both Arbalist oaths already implemented in M03.
- `GB-M04-02`: Deterministic authored-room assembly and 10,000-seed validator.
- `GB-M04-03`: Assemble the M03 dormant Fallen Hero Echo record into a personal Requiem, resolve it, and grant the exact cosmetic/codex reward.
- `GB-M04-04`: Behavior-changing rare/Unique item pass.
- `GB-M04-05`: Parties capped at four, join codes, ready check, shared portal, reconnect, pings, personal loot.
- `GB-M04-06`: Coherent vertical-slice art and first audio pass; temporary assets remain only when labeled in the asset registry.
- `GB-M04-07`: Accessibility baseline and four-player readability controls.
- `GB-M04-08`: Remote test environment, live config, content pinning, telemetry dashboard.
- `GB-M04-09`: Mire realm slice, director/scaling, dungeon portals, and two exact realm events from the content specification.
- `GB-M04-10`: Read-only audited support lookup for account, character, item UID, death, Echo, and transaction IDs.
- `GB-M04-11`: Mute/block/report intake, evidence retention, and emergency content/encounter disable before external Alpha.
- `GB-M04-12`: Draft Steam store copy/art, privacy notice, Playtest recruitment flow, survey, and cohort scheduler; do not publish yet.
- `GB-M04-13`: Minimal idempotent Lantern Marks and Echo Ember wallets/material mutations, memorial-title/codex writes, appearance-entitlement ledger and exact two-class Echo appearance pool, weekly Requiem ordinals, duplicate-pair guard, and reward fixtures required by the Requiem prototype.
- `GB-M04-14`: Shared resolved-stat caps plus exact Glass Floor movement smoothing, Oathfire damage placement, and modifier-altered fairness fixtures; M05 performs broader balance, not first implementation.

### Exit gate

- 25–50 testers complete at least three scheduled sessions.
- At least 60% name Echoes or Bargains as distinctive without prompting.
- At least 70% reach first dungeon within 15 minutes.
- At least 70% reroll after meaningful death.
- At least 70% rate death fairness 4/5+.
- 10,000 dungeon seeds produce zero invalid shipped layouts.
- Solo/2/4-player variants work without body blocking, mandatory class, or readability breach.
- Eight concurrent players traverse and fight in the M04 public-realm cell mask while dungeon party size remains capped at four; server/readability budgets pass.
- Support lookup is read-only, authenticated, audited, and can locate every scripted M04 incident ID.
- Owner/helper Requiem rewards, weekly caps, duplicate-pair suppression, and retry idempotency match the content specification.

**Failure response:** Redesign Echo/Bargain identity or death loop. Do not hide the failure with additional content.

---

## 11. GB-M05 — Closed external Alpha

**Timebox:** Weeks 14–18  
**Objective:** Finish the lean Early Access gameplay scope and determine whether external users return voluntarily.

### Work packages

- `GB-M05-01`: Veil Witch, third dungeon, Bell Warden, Salt Confessor, the remaining six normal enemies, two minibosses, 45 item definitions, 17 affixes, six Bargains, and `modifier.fevered_veil`, `modifier.saints_debt`, and `modifier.restless_dead`; promote the complete Alpha manifest.
- `GB-M05-02`: Levels 1–20, oaths, mastery trial, codex, memorial browser.
- `GB-M05-03`: Vault/Overflow, salvage, Forge, Temper, Reforge, pity Fragments.
- `GB-M05-04`: Complete first-hour onboarding and first real death flow.
- `GB-M05-05`: Join-code/test-account invites, party controls, mute/block/report resolution, predefined communication; Steam friend invites remain M06.
- `GB-M05-06`: Solo/group balance, movement/healing caps, death-cause review, item budgets.
- `GB-M05-07`: Account/session/death/item support tools.
- `GB-M05-08`: 90-day economy simulations and dashboards.
- `GB-M05-09`: Client crash collection, incident IDs, backup automation.
- `GB-M05-10`: Material IDs/sources/sinks, currency reward tables, per-entrant contract transactions, Overflow expiry job, and economy tests.
- `GB-M05-11`: Party-gifting lifecycle, binding, provenance, retry, and abuse tests.
- `GB-M05-12`: Complete the 128×128 Mire macro map, all four authored realm events including Bell Tower Siege, Bell Warden access/climax, retirement, and realm matchmaker.
- `GB-M05-13`: Complete Hall contracts, practice rooms, Training Crypt, first-death tutorial, mastery access, and NPC interaction scripts.
- `GB-M05-14`: Validate parties of 1/2/4/8 in dungeons and 4/20/40 concurrent players in the public realm; these are separate fixtures.
- `GB-M05-15`: Publish Steam Coming Soon and Playtest pages, open cohort recruitment, and publish support/status contact paths; still accept no money.

### Exit gate

At least 100 eligible external participants; staff data excluded:

- Tutorial completion ≥70%.
- Median first session ≥35 minutes.
- D1 ≥30%; D7 ≥12%.
- At least 25% play three sessions in seven days.
- Reroll within two minutes ≥70%.
- Fair-death rating ≥70% at 4/5+.
- Would recommend current state ≥60%.
- Class adjusted death/clear rates remain within investigation bounds.
- Crash-free sessions ≥99.5%.
- No known duplication, impossible resurrection, or permanent state loss.
- Eight-player dungeon and 40-player realm fixtures meet the GDD readability and tick-budget limits.
- The promoted `alpha.1.0.0` bundle passes the complete Alpha row of Content CONT-VALID-001: 90 items, 29 affixes, 3 classes/6 oaths, 12 Bargains/6 modifiers, 18 normal enemies/6 minibosses/4 major bosses, 34 rooms, and 10 landmarks/4 events.

These retention readings are directional because the cohort is below the 500-account decisive threshold. They authorize only progression to the Public Playtest, not commercial assumptions.

If retention fails, allow at most two focused two-week iterations on combat, onboarding, recovery, or death fairness before reconsidering the product. Do not respond with content volume.

---

## 12. GB-M06 — Public Steam Playtest

**Timebox:** Weeks 19–26; extend until the required D30 cohort has matured  
**Objective:** Validate real audience behavior and production-shaped operations without accepting money.

### Work packages

- `GB-M06-01`: Steam authentication, depot packaging, branches, invites, explicit wipe notice.
- `GB-M06-02`: One deployment region, queue, rolling instance retirement, health checks, logs, metrics, alerts.
- `GB-M06-03`: Reconnect, maintenance messaging, backup/restore rehearsal, failed-write retry, account-lock recovery.
- `GB-M06-04`: Rate limits, protocol validation, secret rotation, least privilege, audit logs, suspicious-action flags.
- `GB-M06-05`: Privacy notice, terms, code of conduct, support intake, status/known-issues process.
- `GB-M06-06`: Fixed daily seed, one rotating legal modifier, no season/pass.
- `GB-M06-07`: Synthetic login, realm, combat, dungeon, death, inventory, and reconnect load.
- `GB-M06-08`: Creator-safe effect settings, screenshots, trailer-quality capture path.
- `GB-M06-09`: Steam friend invites, Playtest enrollment waves, entitlement/access revocation, depot rollback, and test-cohort communications.

### Exit gate

- At least 2,500 accounts and 500 D1-eligible users.
- Tutorial completion ≥75%.
- D1 ≥35%; D7 ≥15%; three-session rate ≥30%.
- At least 500 D30-eligible accounts with D30 ≥7%; extend the Playtest if necessary.
- At least 100 surveyed real permadeaths with cause identification ≥80% and fairness rating 4/5+ ≥75%.
- First dungeon within 15 minutes ≥70%.
- Reroll ≥70%.
- Crash-free sessions ≥99.7%.
- Served-region network latency p95 ≤120 ms.
- Server tick p95 ≤20 ms, p99 ≤30 ms at peak.
- 72-hour synthetic soak at three times forecast has no duplication, stuck character, database exhaustion, or unrecoverable instance.
- Backup restore: RPO ≤5 minutes, RTO ≤30 minutes.
- Unit-economics model includes real server/support/retention inputs.

**Hard rule:** Commerce work begins only after this gate passes.

---

## 13. GB-M07 — Early Access release candidate

**Timebox:** Weeks 27–32  
**Objective:** Add only what is required to accept money and preserve durable live accounts.

### Work packages

- `GB-M07-01`: Final account namespace, two wipe/migration rehearsals, schema freeze, permanence policy.
- `GB-M07-02`: Immutable purchase/entitlement ledger, idempotent grants, refund/reversal, reconciliation.
- `GB-M07-03`: Configure the Steam base app as `US$0` free-to-play; implement the one Founder Supporter Pack SKU and exactly 10 direct cosmetic SKUs in Content CONT-COS-001; enforce its entitlement/catalog allowlist validator.
- `GB-M07-04`: Staged rollout, queue limits, emergency disable, maintenance mode, build/content/database rollback.
- `GB-M07-05`: Final onboarding, audio, UI scaling, readability, settings persistence, credits/legal, and complete `manifest.localization.ea.en-US` copy/reference closure under Content CONT-LOC-001.
- `GB-M07-06`: Full regression, save/load, network-failure, payment sandbox, malicious-client suite.
- `GB-M07-07`: 72-hour production-shaped release-candidate soak.
- `GB-M07-08`: Incident, moderation, privacy, purchase, accounting, and customer-support runbooks/ownership.
- `GB-M07-09`: Hourly order reconciliation, daily settlement import/export, finance dashboard, aged-state alerts, and accounting verification.
- `GB-M07-10`: Implement the exact six earnable cosmetic sets in Content CONT-COS-001, Lantern Mark catalog, ownership rules, and refund-safe default fallbacks.

### Hard release criteria

- Seven-day feature/content freeze.
- Zero open P0/P1; accepted P2s have workaround/owner.
- Crash-free sessions ≥99.8%.
- 30 Hz server remains in budget at 3× forecast peak.
- Client/content/database rollback rehearsed.
- Backup and regional redeploy rehearsed.
- 100,000 randomized inventory/death/purchase sequences produce zero duplication/impossible resurrection.
- Every death has authoritative cause, latency, encounter version, and character trace.
- Delayed/duplicate/missing/refunded purchase tests pass.
- Store is cosmetic only and every item previewable.
- `ea.1.0.0` passes Content CONT-VALID-001, including exact cosmetic/SKU counts and complete `en-US` localization closure.
- Required operational owners are named.
- 72-hour soak has no P0.
- The signed, versioned TEL-013 business baseline passes every prelaunch observable threshold and is attached to the recorded commercial go/no-go. Its trailing-90-day contribution rule remains a post-launch guard on fixed-payroll expansion, not a metric fabricated before launch.

---

## 14. GB-M08 — Commercial Early Access

**Target:** Week 33 plus first 30 days  
**Objective:** Release cautiously and stabilize before resuming feature growth.

### Rollout

`GB-M08-00` is a blocking Day-0 cutover before any public admission: stop all test-namespace writes; archive a read-only audit snapshot; wipe all player-visible pre-Early-Access characters, items, materials, wallets, progression, memorials, Echoes, and test entitlements; initialize the durable live namespace; prove by scripted queries that no prohibited test state leaked; run account/character/purchase/death smoke tests; publish the permanence declaration; then enable the rollout below. Only non-player aggregate telemetry and legally required security/commerce audit records may survive, segregated from gameplay and unable to grant an entitlement.

Define `C_launch = min(purchased concurrent capacity, floor(M07_72h_soak_clients / 3))`. Admission is controlled by a server-side `active_session_cap`; all excess authenticated sessions enter a FIFO queue with visible position and estimated wait. Never admit above `C_launch`.

1. Set `active_session_cap = min(100, ceil(0.10 × C_launch))` for at least two hours.
2. After persistence, entitlement, database, crash, and tick checks remain green, set it to `min(250, ceil(0.25 × C_launch))`.
3. After one observed peak period remains green, set it to `min(500, ceil(0.50 × C_launch))`.
4. Raise in configured steps of at most `250` active sessions and at most `25% of C_launch` per step; observe one peak period between steps.
5. Open to `C_launch` only while all release indicators remain green; lower the cap immediately on circuit-breaker or incident thresholds.

### First 30 days

- Days 1–7: incidents/exploits only.
- Days 8–14: one balance patch based on death/class telemetry.
- Days 15–30: continue reliability, support, economy, and balance stabilization; ship no timed event or new foundation system.
- Publish wipe/permanence policy, known issues, and balance reasoning.
- Review acquisition, retention, cosmetic conversion, refund rate, server cost/player-hour, support cost, and content cost weekly.

### Stabilization gate

- Four consecutive weeks without P0 state-loss/commerce incident.
- Crash-free ≥99.8%.
- `grant_success_5m ≥99.99%`, no wrong/duplicate grants, and `unreconciled_24h <0.01%`.
- D7 remains ≥15%.
- At least 70% of deaths still produce reroll within two minutes; 65–69% is an alert/iteration band, not stable.

---

## 15. GB-M09 — Version 1.0

**Window:** Six–twelve months after Early Access; gate-based, not date-only.

`GB-M09-00` is a blocking specification gate: before any Version 1.0 gameplay implementation, approve `Gravebound Content Production Specification v2` enumerating every added class/oath, region cell, dungeon/room, enemy, boss, item, reward, source, cosmetic, asset, localization key, migration, stage manifest, and validator fixture. Until that document passes reference closure and count simulation, the additional 1.0 content below is planning scope only and no AI or developer may invent production records from the headline counts.

Required minimum:

- Five polished classes.
- Two regions.
- Six dungeons across three bands.
- Eight major bosses.
- Exactly 180 item templates and 30–40 validated behavior-changing Uniques.
- Two successful content updates delivered six to eight weeks apart.
- One nonmonetized shared-world Chapter proven without population split.
- One 12-week Chapter delivered on time and the next Chapter at least 80% complete.
- Controller support and full accessibility review.
- Economy stable for 90 days.
- Content authoring/validation pipeline sustains cadence.
- D30, operating margin, support load, and runway meet approved business model.
- No critical progression depends on trading, guild schedule, or high population.

The first six-to-eight-week live content update has a separate gate: it must ship without a schema/protocol emergency, duplication, rollback failure, or unavoidable-death regression. This criterion is intentionally outside the M08 first-30-day stabilization gate.

---

## 16. Test population ladder

| ID | Population | Purpose | Required before |
|---|---:|---|---|
| `GB-TP00` | Unit/integration/deterministic/bots | Correctness each change | Every merge |
| `GB-TP01` | 1–5 team members | Immediate feel/regression | M01 blind test |
| `GB-TP02` | 10 people not involved in the tested features | First Playable fun/readability | M02 |
| `GB-TP03` | 10–20 dogfood testers | Complete-loop usability | M04 |
| `GB-TP04` | 25–50 invited testers | Distinctiveness/group behavior | M05 |
| `GB-TP05` | 100–300 external accounts | Retention/fairness | M06 |
| `GB-TP06` | 2,500+ public accounts; 250 CCU cap | Audience/operations | M07 |
| `GB-TP07` | Synthetic 3× forecast plus recruitable real peak | Capacity/recovery | M08 |

Synthetic players validate capacity. Real players validate fun, fairness, readability, and retention.

## 17. Initial implementation ordering index

This is an ordering index, not an executable ticket set. Before work begins, expand every row with the GDD Section 29 task template, exact content-spec IDs, dependencies, tests, telemetry, and file ownership.

| Order | Ticket | Outcome |
|---:|---|---|
| 1 | `GB-M00-01` | Workspace/crates build |
| 2 | `GB-M00-02` | Toolchain and formatting baseline |
| 3 | `GB-M00-03` | Build/test/local-lab command documentation |
| 4 | `GB-M00-04` | CI and clean Windows release build |
| 5 | `GB-M00-05` | Fixed 30 Hz clock |
| 6 | `GB-M00-06` | Stable IDs/feature registry |
| 7 | `GB-M00-07` | Minimum class/ability/enemy/pattern/arena/item/drop schemas |
| 8 | `GB-M00-08` | Fixed-input trace/state-hash harness |
| 9 | `GB-M01-01A` | Bevy camera and world units |
| 10 | `GB-M01-01B` | Input map and player movement |
| 11 | `GB-M01-02A` | Mouse aim and held primary fire |
| 12 | `GB-M01-02B` | Projectile/hitbox collision |
| 13 | `GB-M01-02C` | Grave Mark |
| 14 | `GB-M01-02D` | Slipstep and Exhaustion |
| 15 | `GB-M01-02E` | Stillness passive and HUD state |
| 16 | `GB-M01-11` | Red Tonic belt use and feedback |
| 17 | `GB-M01-03A` | `enemy.drowned_pilgrim` prototype-bundle record |
| 18 | `GB-M01-03B` | `enemy.bell_reed` prototype-bundle record |
| 19 | `GB-M01-03C` | `enemy.chain_sentry` prototype-bundle record |
| 20 | `GB-M01-04A` | Pattern primitives and timeline |
| 21 | `GB-M01-04B` | `boss.prototype.bell_proctor` phase 1 |
| 22 | `GB-M01-04C` | `boss.prototype.bell_proctor` phases 2–3/breaks |
| 23 | `GB-M01-05A` | Health, damage order, damage bands |
| 24 | `GB-M01-05B` | Telegraph/threat validator |
| 25 | `GB-M01-06A` | Death transaction in local lab |
| 26 | `GB-M01-06B` | Cause-of-death UI and restart |
| 27 | `GB-M01-07A` | Item instance/schema and four slots |
| 28 | `GB-M01-07B` | `item.prototype.*` 12-item catalog and `reward.prototype.*` tables |
| 29 | `GB-M01-08A` | Hitbox/spawn/seed debug overlay |
| 30 | `GB-M01-08B` | Time-scale/invulnerability tools |
| 31 | `GB-M01-09` | Projectile/performance fixture |
| 32 | `GB-M01-10A` | Accessibility effect controls |
| 33 | `GB-M01-10B` | Playtest telemetry/survey build |
| 34 | `GB-M01-GATE` | Blind test, report, stop/go decision |

## 18. Critical-path controls

- M01 combat gate owns all feature priority until passed.
- M02 authority gate precedes persistent inventory implementation.
- M03 death/recovery gate precedes second dungeon/content breadth.
- M04 distinctiveness gate precedes completion of 90-item catalog.
- M05 retention gate precedes paid acquisition or broad promotional scale; Steam administration and controlled Playtest recruitment are allowed earlier.
- M06 public/business gate precedes store/backend commerce.
- M07 durability gate precedes launch.

When a critical-path ticket is blocked, work may proceed only on dependency-neutral tests, tools, art based on locked footprints, or documentation. Do not use a blocker to justify unrelated scope expansion.

## 19. Explicitly deferred backlog

| Feature | Earliest reconsideration |
|---|---|
| Browser/WASM public client | After native EA stabilization and business case |
| Linux/macOS/Steam Deck certification | After measured demand post-EA |
| Global market/direct unrestricted trade | After 90 stable economy/ledger days and population gates |
| Guilds/guild halls | Version 1.0 candidate after party engagement data |
| 20–40 player instanced raids | After 8-player readability/performance proof |
| Formal seasons/battle pass | After two ordinary Chapters ship on time |
| Public text chat | After moderation tooling/staffing |
| Voice chat | External platform solution only if justified |
| Deep crafting/sets | After 90 days of satisfying found-loot economy |
| Paid storage/slots | Not planned |
| Kubernetes/microservices | Only for a measured independent bottleneck |
| Multi-region failover | After audience distribution and sustainable operation are known |
| Mobile/console | No commitment |

## 20. Roadmap maintenance

- Update milestone status weekly.
- Record actual effort separately from target timebox.
- Never mark a milestone complete when its gate fails.
- New features require feature ID, dependency, milestone, owner, tests, telemetry, and explicit displaced work.
- Reforecast calendar after M01, M03, M05, and M06 using measured throughput.
- Preserve failed assumptions and decisions in `docs/decisions/`; do not rewrite history.
- The GDD controls behavior; this roadmap controls order. A roadmap edit cannot silently change game rules.

## 21. Immediate next action

Start `GB-M00-01` through `GB-M00-08`, expand the M01 ordering rows with the GDD Section 29 template and exact IDs from `Gravebound_Content_Production_Spec_v1.md`, then execute the ten-working-day M01 schedule. The first stakeholder/demo build is the local Combat Laboratory. No production account, store, market, guild, browser, season, or Kubernetes task belongs ahead of `GB-M01-GATE`.
