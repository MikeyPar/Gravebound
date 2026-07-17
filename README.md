# Gravebound

Gravebound is a server-authoritative, permanent-death, 2D dark-fantasy bullet-hell dungeon crawler inspired by the immediacy and social danger of *Realm of the Mad God*.

Every character life is temporary. The account remembers what happened, and exceptional deaths can return as personalized Fallen Hero Echo encounters. The design emphasizes readable combat, rapid recovery, fair monetization, solo viability, and long-term replayability without permanent account-level combat power.

> **Project status:** M01 and M02 are closed. GB-M03 is approximately **78% complete**: identity, PostgreSQL persistence, Hall/private-world foundations, progression, the complete Core item/Vault lifecycle, Oath/Bargain, Ash, atomic death/Memorial/Echo persistence, successful extraction/Emergency Recall/ResolutionHold, and successor recovery are closed under hosted evidence. [`GB-M03-08`](docs/milestones/GB-M03-08-audit.md) passed at commit `2535e8c` on hosted CI [`29554811453`](https://github.com/MikeyPar/Gravebound/actions/runs/29554811453). Successor recovery has append-only protocol `1.17`, reliable kind `23`, `core_successor_v1`, pinned `1.16` compatibility, migrations `0060`/`0061`, universal death presets/reservations, the shared transaction-local exact `04D` starter seam, a replay-first atomic writer, authenticated fail-closed service, and durable response-loss/reconnect/pool/OS-process replay. Its native authority gates Create Successor on the opaque durable terminal summary, retains exact retry, preselects the legal level-one successor, and treats Play as confirmation two behind matching Hall readiness. The optimized 32-frame resolution/effects/scale matrix passes. Hosted CI [`29608793943`](https://github.com/MikeyPar/Gravebound/actions/runs/29608793943) passes the cumulative Linux/Windows/PostgreSQL/native matrix and the hashed 25-journey route: median death-to-control `118.676 ms`, p95 `129.126 ms`, 25/25 checked danger returns, 25 unique successors, 100 unique starter items, exact durable graphs, and zero operational residue. [`GB-M03-07`](docs/milestones/GB-M03-07-audit.md) is closed. Normal parent-route admission, telemetry, support, hosting/platform evidence, and final private-loop/cohort gates remain disabled.

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

### GB-M03 successor recovery

| Durable `Create Successor` focus | Exact preselected successor |
|---|---|
| <img src="docs/evidence/GB-M03-07-successor/GB-M03-07-successor-death-summary-standard-1280x720.png" alt="Durably recorded death summary scrolled to the focused Create Successor primary action" width="720"> | <img src="docs/evidence/GB-M03-07-successor/GB-M03-07-successor-character-select-standard-1920x1080.png" alt="Preselected level-one Grave Arbalist successor with no Oath and Play as confirmation two" width="720"> |

The evidence-only native coordinator joins the durable terminal proof, exact stored successor result, preselected Character Select, accepted Hall transfer, and matching scene readiness without enabling the shipped normal route. The [32-frame optimized visual matrix](docs/evidence/GB-M03-07-successor-visual-manifest.md) covers every recovery phase, both certified resolutions and effects modes, and 150% Character Select scale. The [hosted 25-journey report](docs/evidence/GB-M03-07-successor-recovery-manifest.md) passes timing, unique identity/grant, danger-return, and zero-residue gates; the [three-authority audit](docs/milestones/GB-M03-07-audit.md) closes the package. Normal Play and production Realm Gate admission remain fail closed until parent `GB-M03-03` integration passes.

### GB-M03 Resolution Hold recovery

| Full storage keeps Move disabled | Permanent destruction defaults to Cancel |
|---|---|
| <img src="docs/evidence/GB-M03-08-hold/GB-M03-08-hold-storage-full-standard-1280x720.png" alt="Resolution Hold full-storage state with Move disabled and permanent destruction separately available" width="720"> | <img src="docs/evidence/GB-M03-08-hold/GB-M03-08-hold-confirm-destroy-standard-1920x1080.png" alt="Resolution Hold permanent-destruction review with Cancel Keep Item focused by default" width="720"> |

The blocking native surface consumes only negotiated server authority and the compiled Core item catalog. It shows exact whole-stack quantities, durable identities, one-based server-planned destinations, retained Overflow deadlines, typed retry state, and final-refresh acknowledgement without exposing a route-to-play escape. The optimized [24-frame visual manifest](docs/evidence/GB-M03-08-hold-visual-manifest.md) covers six states in standard and reduced effects at both target resolutions; the separate PostgreSQL/real-QUIC precedence and cleanup gates are closed in the [integrated evidence](docs/evidence/GB-M03-08-integrated-evidence.md).

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

[`GB-M03-08`](docs/tasks/GB-M03-08.md) is complete under its [three-authority audit](docs/milestones/GB-M03-08-audit.md) and [integrated evidence](docs/evidence/GB-M03-08-integrated-evidence.md). Hosted CI [`29554811453`](https://github.com/MikeyPar/Gravebound/actions/runs/29554811453) is green for the exact audited source, including PostgreSQL, real QUIC, strict lint/tests/content validation, and optimized Windows construction.

The three-authority parent audit identified the missing normal composition root and accepted [`ADR-037`](docs/decisions/ADR-037-normal-core-private-route-composition.md) under the route contract already approved by [`SPEC-CONFLICT-006`](docs/spec-conflicts/SPEC-CONFLICT-006-m03-world-flow-contract.md). [`GB-M03-03G`](docs/tasks/GB-M03-03G.md) slice 1 is now implemented locally: production read/mutation routing, restart-stable authority-scoped IDs, exact Hall/danger entry, two-phase generation/version-bound Bell permits outside PostgreSQL locks, same-root fixed-dungeon commit, typed replay/abort/reconcile, and fail-closed later destinations. Run its guarded hosted PostgreSQL suite and cumulative Linux/Windows CI next; after green evidence, append `CorePrivateRouteStateV1` and construct the capacity-one actor. Keep normal capability advertisement, Character Select `Play`, and Realm Gate interaction disabled until the live actor, terminal owners, ordinary native route, and cumulative evidence are attached; keep Core promotion, M04+ content, telemetry, support, hosting/platform surfaces, and unrelated Hall stations fail closed.

## Resolved prior handoff

The owner approved the in-place `fp.1.0.0` correction. The subsequent full reference-loadout audit corrected the earlier omitted-armor premise, retained the raw-12 fan as Chip, and closed both Bell specification conflicts. The resulting Bell, combat, summary, and debug tickets pass locally; this paragraph is retained only as the resolved decision record.
