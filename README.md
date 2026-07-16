# Gravebound

Gravebound is a server-authoritative, permanent-death, 2D dark-fantasy bullet-hell dungeon crawler inspired by the immediacy and social danger of *Realm of the Mad God*.

Every character life is temporary. The account remembers what happened, and exceptional deaths can return as personalized Fallen Hero Echo encounters. The design emphasizes readable combat, rapid recovery, fair monetization, solo viability, and long-term replayability without permanent account-level combat power.

> **Project status:** M01 and M02 are closed under their recorded gates. M03 has closed identity, parent PostgreSQL persistence, exact world-flow content, atomic dormant transfers, Hall/private-microrealm simulation and native evidence, progression, the complete Core 18-item/equipment/CharacterSafe/Vault lifecycle, the first Oath/Bargain package, the minimal Ash wallet, and atomic death/destruction/Memorial/Echo persistence with hosted adverse, outage, response-loss, restart, soak, and native evidence. Parents `GB-M03-02`, `GB-M03-04`, and `GB-M03-06` are complete. `GB-M03-08` now has append-only protocol `1.16`: hosted-green extraction/Recall `1.15` bytes remain unchanged while bounded ResolutionHold query/mutation kinds `21`/`22` are appended under their own disabled capability. Terminal custody `0055`, atomic extraction graph `0056`, authenticated extraction dispatch, Recall graph `0057` with deterministic custody and immutable recovery, exact server-owned 12-tick explicit/90-tick LinkLost coordination, replay identity `0058`, a bounded per-character Recall mailbox, and the staged five-producer terminal driver are implemented. The driver freezes the complete tick/binding/version/content/clock snapshot across outages, preserves committed replay before current-state validation, proves explicit and LinkLost death precedence, and publishes a receipt-matched stored result plus Hall projection that can be reinstalled after restart. A bounded logical-session completion outbox proves server-initiated real-QUIC `Stored` delivery at tick `112`; abandoned delivery is replayed exactly on reconnect. The disposable active route now has a hosted PostgreSQL gate for Hall-to-danger Recall commit, lost completion, pool/actor reconstruction, and exact replay while the ordinary Core route remains disabled. ResolutionHold schema/repository/service/UI work, remaining adverse/cleanup evidence, successor recovery, complete-route admission, telemetry, support, platform evidence, and final private-loop/cohort gates remain disabled.

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

### GB-M03 private-loop world foundation

| Lantern Halls keeps Realm Gate admission fail closed | The capacity-one microrealm requests the exact 900 ms warning without constructing `03D` enemies |
|---|---|
| <img src="docs/evidence/GB-M03-03C-hall-stage-disabled-1920x1080.png" alt="Lantern Halls graybox with Realm Gate StageDisabled evidence" width="720"> | <img src="docs/evidence/GB-M03-03C-microrealm-warning-1920x1080.png" alt="Core microrealm exact warning request evidence" width="720"> |

<img src="docs/evidence/GB-M03-03C-microrealm-cleared-1920x1080.png" alt="Core microrealm terminal Cleared state at the Bell Sepulcher portal" width="960">

The native graybox is compiled from the exact Core world records and localization. Fixed-point collision/navigation, server-owned interaction projection, camera bounds, standard/reduced-motion presentation, and the Dormant -> Waiting -> Active -> Cleared lifecycle are deterministic; the normal player route remains disabled until its item, death, extraction, and Recall owners pass. See the [`03C`](docs/milestones/GB-M03-03C-audit.md) and [`03F`](docs/milestones/GB-M03-03F-audit.md) completion audits.

### GB-M03 native transition and recovery

| Server-owned LinkLost boundary | Committed extraction returns to Hall |
|---|---|
| <img src="docs/evidence/GB-M03-03F-link-lost-standard-1920x1080.png" alt="Native LinkLost state with the server-owned 90-tick boundary and retry focus" width="720"> | <img src="docs/evidence/GB-M03-03F-hall-resolution-standard-1920x1080.png" alt="Native committed extraction resolution returning the character to HallDefault" width="720"> |

The strict Core transition projection preserves the last authoritative state, safe origin, destination, exact retry policy, and committed terminal result without predicting server outcomes. Its optimized 33-frame standard/reduced-effects matrix covers all eight required states at 1280x720 and 1920x1080 plus an ultrawide reference; see the [visual evidence manifest](docs/evidence/GB-M03-03F-visual-manifest.md). Normal route admission and Core promotion remain disabled.

### GB-M03 durable item and Vault lifecycle

<img src="docs/evidence/GB-M03-04G-lifecycle-standard-1920x1080.png" alt="Read-only native item and Vault lifecycle signature beside the protected Lantern Halls corridor" width="960">

The disposable native inspection surface composes the completed `04A`-`04F` authorities into one content-bound signature: selected character, progression, exact storage capacities and occupancy, durable item identities and provenance, security/location state, aggregate versions, receipts, and the ordered mutation ledger. It remains read-only and preserves 49% of the viewport for the Hall corridor; Realm Gate, Vault station, and the normal route remain disabled. See the [visual evidence manifest](docs/evidence/GB-M03-04G-visual-manifest.md).

### GB-M03 durable death and Memorial presentation

| Durable death summary | Read-only Memorial Wall |
|---|---|
| <img src="docs/evidence/GB-M03-06E-native-standard-1920x1080.png" alt="Source-driven native durable death summary with exact lethal cause, damage trace, losses, preserved state, Echo result, and disabled successor action" width="720"> | <img src="docs/evidence/GB-M03-06E-native-reduced-1920x1080.png" alt="Source-driven reduced-effects durable death summary with equivalent semantic content and focus" width="720"> |

The native Bevy surfaces consume only the durable, content-revision-bound client projection. The summary preserves exact `DTH-020` order, shows the stored Echo outcome, supports bounded focus-follow scrolling at 1280x720, and keeps `Create Successor` disabled until `GB-M03-07`. Memorial rows retain raw cursor authority and open their own immutable stored snapshot without a gameplay mutation. The optimized [presentation matrix](docs/evidence/GB-M03-06D-visual-manifest.md), final [source-driven integration evidence](docs/evidence/GB-M03-06E-integrated-evidence.md), and [parent completion audit](docs/milestones/GB-M03-06-audit.md) cover standard/reduced effects, both target resolutions, exact replay/restart, adverse PostgreSQL/QUIC behavior, latency, and soak.

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

Parents [`GB-M03-02`](docs/milestones/GB-M03-02-audit.md), [`GB-M03-04`](docs/milestones/GB-M03-04-audit.md), and [`GB-M03-06`](docs/milestones/GB-M03-06-audit.md), plus atomic Echo package [`GB-M03-13`](docs/milestones/GB-M03-13-audit.md), are complete. Hosted CI [`29506273492`](https://github.com/MikeyPar/Gravebound/actions/runs/29506273492) is the final death/Echo acceptance. [`GB-M03-08`](docs/tasks/GB-M03-08.md) preserves old protocol bytes while appending extraction/Recall kinds `19`/`20`; extraction custody/writer/recovery/dispatch are hosted green through [`29521716146`](https://github.com/MikeyPar/Gravebound/actions/runs/29521716146); durable Recall replay identity is hosted green through [`29531220086`](https://github.com/MikeyPar/Gravebound/actions/runs/29531220086); the bounded actor mailbox is hosted green through [`29531939971`](https://github.com/MikeyPar/Gravebound/actions/runs/29531939971); and the immutable full-snapshot driver is hosted green through [`29534531389`](https://github.com/MikeyPar/Gravebound/actions/runs/29534531389). Commits `1529bfc` and `31b3569` add server-initiated completion delivery plus abandoned-stream reconnect replay. Commit `32ed9c6` composes the real Hall-to-danger route, Recall actor, five-producer driver, PostgreSQL writer, lost post-commit delivery, pool/actor reconstruction, exact real-QUIC replay, altered-client-tick conflict, and single-result assertion; hosted run [`29536980393`](https://github.com/MikeyPar/Gravebound/actions/runs/29536980393) is validating that slice. Accepted [`SPEC-CONFLICT-030`](docs/spec-conflicts/SPEC-CONFLICT-030-m03-resolution-hold-recovery.md) is now implemented at the wire boundary by commit `265d50e`: protocol `1.16` preserves kinds `1`-`20` and appends bounded Hold query/mutation kinds `21`/`22`; hosted run [`29537777576`](https://github.com/MikeyPar/Gravebound/actions/runs/29537777576) is validating it. Next, add schema `0059` plus read, whole-stack server-planned move, confirmed explicit destroy, exact replay, and final Hall unlock. In parallel, close remaining Recall adverse/cleanup evidence. After `GB-M03-08`, complete `GB-M03-07` successor recovery and parent `GB-M03-03`; keep normal route admission and Core promotion fail closed.

## Resolved prior handoff

The owner approved the in-place `fp.1.0.0` correction. The subsequent full reference-loadout audit corrected the earlier omitted-armor premise, retained the raw-12 fan as Chip, and closed both Bell specification conflicts. The resulting Bell, combat, summary, and debug tickets pass locally; this paragraph is retained only as the resolved decision record.
