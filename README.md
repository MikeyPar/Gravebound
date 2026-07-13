# Gravebound

Gravebound is a server-authoritative, permanent-death, 2D dark-fantasy bullet-hell dungeon crawler inspired by the immediacy and social danger of *Realm of the Mad God*.

Every character life is temporary. The account remembers what happened, and exceptional deaths can return as personalized Fallen Hero Echo encounters. The design emphasizes readable combat, rapid recovery, fair monetization, solo viability, and long-term replayability without permanent account-level combat power.

> **Project status:** M01 and M02 are closed under their recorded gates. `GB-M03-01`, `GB-M03-02A`, `GB-M03-02B`, `GB-M03-03A`, and `GB-M03-11` now pass: the Core Grave Arbalist identity is native, server-authoritative, and durable, and exact Hall/private-microrealm content compiles in the explicitly wipeable, unpromoted namespace. Parent `GB-M03-02` remains open for item/vault, memorial/death, and ledger aggregates; world routing, progression, permanent death, recovery, telemetry, and operations packages also remain open.

![Bell Sepulcher gameplay concept](Concept%20Art/01-bell-sepulcher-gameplay.png)

## Design package

| Document | Purpose |
|---|---|
| [Canonical Production GDD](Gravebound_Production_GDD_v1_Canonical.md) | Product contract, gameplay systems, architecture, economy, monetization, UI, art direction, QA, and release gates |
| [Content Production Specification](Gravebound_Content_Production_Spec_v1.md) | Exact IDs, formulas, encounters, rooms, loot tables, boss schedules, manifests, cosmetics, localization, and validation rules |
| [Development Roadmap](Gravebound_Development_Roadmap_v1.md) | Gate-based delivery plan from First Playable through Early Access and Version 1.0 |
| [Original Ashen Veil GDD](Gravebound_Ashen_Veil_GDD.html) | Preserved source design used to produce the canonical package |
| [M00 Completion Audit](docs/milestones/GB-M00-audit.md) | Reproducibility, validation, deterministic trace, clean CI, and Windows release evidence |

The canonical GDD defines intent and product rules. The content specification is the executable authority for exact gameplay data. The roadmap controls sequencing and promotion gates.

## Core experience

- Top-down orthographic movement with independently aimed weapons.
- Dense but strongly telegraphed projectile combat.
- Permanent character death with fast successor creation.
- Four equipment slots: Weapon, Relic, Armor, and Charm.
- Optional Veil Bargains that pair a meaningful boon with a meaningful curse.
- Personal Fallen Hero Echoes assembled from notable dead characters.
- Public realm events, authored-room dungeons, minibosses, and major bosses.
- Solo-completable progression with parties and public encounters as optional advantages.
- Cosmetics-only commercial model: no paid power, storage, slots, access, or death protection.

## Early Access target

| Category | Scope |
|---|---|
| Classes | Ashen Vanguard, Grave Arbalist, Veil Witch; two oaths each |
| World | Lantern Halls nexus and the Mire of Bells public realm |
| Dungeons | Bell Sepulcher, Root Chapel, Drowned Reliquary |
| Encounters | 18 normal enemies, 6 minibosses, 3 dungeon bosses, Bell Warden world climax |
| Items | 90 templates, 29 affixes, 12 Black Uniques |
| Replay systems | 12 Veil Bargains, 6 dungeon modifiers, personal Requiem encounters |
| Groups | Solo to 8-player dungeons; 40-player realm cap |
| Platform | Native Windows 10/11 release through Steam |

## Fastest playable path

The first milestone intentionally excludes accounts, networking, the public realm, crafting, and commerce. It proves the feel of the game before expensive infrastructure work begins.

The 10-day First Playable contains:

- Grave Arbalist.
- One fixed combat arena.
- Drowned Pilgrim, Bell Reed, and Chain Sentry.
- Bell Proctor benchmark boss.
- Twelve prototype equipment templates and Red Tonic.
- Local movement, aiming, shooting, abilities, loot, death, and immediate restart.

Development proceeds only when the milestone's playability, fairness, reliability, and retention gates pass. See the [Development Roadmap](Gravebound_Development_Roadmap_v1.md) for the complete sequence.

### Current implementation

![GB-M01-06A local death and fresh-run evidence](docs/evidence/GB-M01-06A.png)

`GB-M01-06A` makes local death a one-shot transaction: health zero freezes the old run, retains the lethal trace, destroys all run-owned entities/items/stacks, and rejects later actions. Explicit Run Again reconstructs a full-health successor from validated content with the default seed, exact starter loadout, two Tonics, and new run-qualified identities; measured control return is below three seconds. See the [completion audit](docs/milestones/GB-M01-06A-audit.md).

![GB-M01 Bell Proctor Phase 1 evidence](docs/evidence/GB-M01-04B-04C.png)

The complete local journey now advances through the three authored waves into the real Bell Proctor composite. Its content-authored scheduler drives live fan, rotating-gap ring, Cross lanes, phase breaks, damage, defeat, boss reward, completion summary, and atomic Run Again flow. See the [`04B`](docs/milestones/GB-M01-04B-audit.md), [`04C`](docs/milestones/GB-M01-04C-audit.md), and [`06B`](docs/milestones/GB-M01-06B-audit.md) audits.

## Technical direction

- Rust stable and Bevy 0.19.
- Native Windows client.
- Fixed 30 Hz authoritative simulation.
- Server-authoritative modular monolith before service decomposition.
- PostgreSQL persistence with idempotent item, death, extraction, and purchase transactions.
- Immutable, versioned content bundles with deterministic RNG and golden fixtures.
- Generated JSON checked into the future implementation repository; undocumented runtime defaults are prohibited.

## Visual direction

Dark-fantasy pixel art uses wet stone, tarnished brass, ash, salt, bone, moss, candlelight, and restrained stained glass. Environments remain muted so hostile projectiles, telegraphs, exits, safe zones, and player silhouettes retain priority.

| Lantern Halls nexus | Characters, enemies, weapons, and projectiles |
|---|---|
| <img src="Concept%20Art/02-lantern-halls-nexus.png" alt="Lantern Halls nexus concept" width="720"> | <img src="Concept%20Art/03-character-enemy-arsenal-sheet.png" alt="Character, enemy, weapon and projectile concept sheet" width="720"> |

![Mire and dungeon environment concepts](Concept%20Art/04-world-dungeon-environment-sheet.png)

Concept images establish mood, hierarchy, and visual language. They are not final production sprites or promises of exact layout.

## Repository policy

- The canonical GDD and content specification require review together when gameplay data changes.
- Stable content IDs are never silently repurposed.
- No implementation may invent missing production rules; ambiguity becomes a specification change.
- Version 1.0 content implementation remains blocked until an exact Content Production Specification v2 is approved.
- Test progress is wipeable until the documented Early Access live-namespace cutover.

## Current Next Step

`GB-M03-04A`, `GB-M03-04D`, `GB-M03-05B`, [`GB-M03-05C`](docs/tasks/GB-M03-05C.md), `GB-M03-05D`, `GB-M03-05E`, [`GB-M03-05F`](docs/tasks/GB-M03-05F.md), and `GB-M03-12` are complete with authorized PostgreSQL concurrency/restart evidence and completion audits. Audit parent `GB-M03-05` across completed slices `05A`–`05F`, including tester-completable shrine-flow traceability. Finish `GB-M03-03B` with the remaining composite danger-entry providers and PostgreSQL closure evidence. Continue `GB-M03-04E`/`04F` and the approved `SPEC-CONFLICT-009` death foundation as their item/location dependencies become ready. Keep 50-Ash purge, paid Oath change, Core promotion, the normal player route, and affected Hall stations fail closed until their owning gates pass.

## Resolved prior handoff

The owner approved the in-place `fp.1.0.0` correction. The subsequent full reference-loadout audit corrected the earlier omitted-armor premise, retained the raw-12 fan as Chip, and closed both Bell specification conflicts. The resulting Bell, combat, summary, and debug tickets pass locally; this paragraph is retained only as the resolved decision record.
