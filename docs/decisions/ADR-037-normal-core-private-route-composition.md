# ADR-037 - Normal Core private-route composition

**Status:** Accepted on 2026-07-17

**Owners:** Server/runtime, native client, persistence, simulation, content, QA

**Applies to:** Parent `GB-M03-03`, normal Core route admission, and the M03 complete-private-loop gate

## Authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: Core Prototype scope; `LOOP-001`-`003`; `DTH-001`, `DTH-010`-`011`, `DTH-020`-`021`; `UI-002`-`007`; `TECH-010`-`023`; and `QA-101`.
- `Gravebound_Content_Production_Spec_v1.md`: `CONT-WORLD-001`, `CONT-ROOM-007`, the Core encounter/boss manifests, `CONT-BOSS-001`-`002`, and `CONT-HUB-001`-`002` define the only legal Core route and later-stage closures.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03` and the M03 exit gate require an explicit no-developer-command private loop, restart/idempotency proof, login and successor timing, and 25 scripted journeys.
- Approved [`SPEC-CONFLICT-006`](../spec-conflicts/SPEC-CONFLICT-006-m03-world-flow-contract.md) already fixes the exact route, capacity-one ownership, unpromoted content boundary, and delayed normal enablement. This ADR chooses the final software composition; it does not change that product contract.

## Context

The required route is:

```text
Character Select
  -> hub.lantern_halls_01
  -> world.core_microrealm_01
  -> dungeon.bell_sepulcher / layout.core_private_life_01
  -> boss.sir_caldus
  -> extraction -> Lantern Halls
  -> death -> Death Summary -> successor -> Character Select -> Lantern Halls
```

Completed `GB-M03-03A`-`03F`, `04`, `05`, `06`, `07`, `08`, `12`, and `13` prove their individual content, simulation, persistence, terminal, client-projection, and evidence-harness boundaries. They intentionally do not expose that route. The normal persistent server still constructs `WorldFlowGateService`, advertises no `core_world_flow_integration`, injects disabled safe-inventory, ResolutionHold, successor, extraction, and Recall authorities, and reports zero combat sessions. The native Core identity surface can create/select a character but has no `Play` action or world-flow dispatch. The only full-route adapter is explicitly disposable, skips live scene ownership, uses the older Caldus evidence-extraction handoff, and cannot honestly become production admission.

**Implementation-audit clarification (2026-07-17):** those completed packages do not by themselves prove one common per-transport reliable sequence, a normal live extraction-intent owner, a terminal-first composite bootstrap snapshot, or complete movement/combat/reward/pending-inventory ownership inside the private-life actor. Those are composition prerequisites, not evidence that can be inferred from the disposable route or isolated package tests. Capability advertisement remains false until the dedicated root constructs and verifies them together.

**Implementation checkpoint (2026-07-17):** exact source `037efe6` is green under hosted CI [`29630076008`](https://github.com/MikeyPar/Gravebound/actions/runs/29630076008). It closes the shared per-transport writer, terminal-first serializable bootstrap, schema-63 durable extraction-intent acceptance/conflict audit, route-owned `TerminalPending` permit, paired route/world content authority, live Caldus intent actor/planner, response-loss/reconnect replay, PostgreSQL transaction/restart coverage, strict Linux checks, Windows release construction, and optimized native evidence. Commit `26c420b` then adds the locally green dormant persistent foundation, constructing exact route content and reusable identity, world-flow, progression, death, Oath/Bargain, storage, successor, terminal-execution, and route-directory owners before socket binding. It deliberately retains the regression endpoint's disabled normal authorities. The per-account session owner, dynamic extraction/Recall binding, transition/bootstrap reconciliation, complete live actor, and dedicated ordinary root remain open, so capability advertisement remains false.

The final implementation therefore needs a composition root and live actor ownership, not a feature-flag edit.

## Decision

### Separate regression and normal roots

- `BoundCoreIdentityServer` remains an explicit identity/regression endpoint. It continues to omit the normal-route capability and reject route mutations without side effects.
- Add a persistent-only `BoundCorePrivateLifeServer` for the wipeable Core namespace. It advertises a capability only after the corresponding concrete authority and cleanup owner construct successfully.
- The normal root owns identity, progression, death views, Oath/Bargain, safe inventory, ResolutionHold, successor recovery, world flow, live scene actors, terminal coordination, Recall channel state, connection ownership, and reliable completion delivery. Partial construction fails startup.
- "Normal" means the ordinary player-facing Core route, not permanent commercial data. Production namespace cutover and Steam authentication remain outside M03.

### One private-life actor graph

- One selected living character has at most one active private-life lineage and one live actor generation.
- Server-owned explicit transitions are `CharacterSelect -> Hall`, `Hall -> core microrealm`, cleared Bell portal -> fixed Bell dungeon, fixed room boundaries -> Sir Caldus, and stored terminal outcome -> Hall or Character Select.
- The actor owns exact compiled Hall/microrealm movement and interaction, microrealm trigger/clear state, fixed `B0 -> B6` room lifecycle, Core combat, reward handoff, Sir Caldus, and terminal coordination. Clients submit bounded input and interaction intent only.
- The Bell portal rejects until the authoritative microrealm is `Cleared`. BB1, BS1, seeded layouts, parties, public allocation, and all M04+ content remain unavailable.
- Location reads and accepted transitions retain authenticated account/selected-character binding, exact content revisions, aggregate versions, durable receipts, capacity-one lineage, and one entry restore root.

### Append one private-route projection

Existing location and terminal messages do not expose authoritative microrealm, room, portal, or boss phase. Append a negotiated, bounded `CorePrivateRouteStateV1` reliable event after existing discriminants. It carries lineage, scene/room identity, authoritative phase, clear/portal/boss/extraction readiness, and state version. Existing message numbers and pinned protocol fixtures remain unchanged.

### Terminal outcomes replace the evidence shortcut

- Successful extraction, Emergency Recall, lethal death, disconnect recovery, and server-fault restoration use the completed five-producer terminal coordinator. A committed death wins a same-tick conflict.
- Extraction and Recall publish the exact stored terminal result and Hall projection from `GB-M03-08`; normal admission does not use the older disposable Caldus evidence receipt.
- Death publishes the durable summary only after `GB-M03-06` commits. Successor creation and confirmation-two `Play` use `GB-M03-07`.
- ResolutionHold blocks control and danger entry until an authoritative empty refresh. No terminal path deletes an accepted extraction item or resurrects a dead character.

### Compose one ordinary native state machine

- Add a `core-private-life` mode that owns the GDD `UI-002` sequence and composes completed view models rather than launching showcase applications.
- Character Select exposes `Play` only with the negotiated normal-route capability, a selected living character, resolved storage, and no mutation in flight. Play emits `EnterHallFromCharacterSelect`.
- Matching authoritative location plus matching `CoreSceneReadiness` is required before control. Hall readiness never implies danger/combat readiness.
- The same application traverses Hall, RealmLoading, microrealm, DungeonLoading, dungeon/boss, Recall/extraction Hall return, DeathSummary, Memorial, successor recovery, and Character Select without developer commands.
- Evidence/showcase commands remain focused inspection surfaces and are never called by the normal route.

### Fail closed and cleanly

- Capability advertisement derives from the fully constructed authority set.
- Response loss replays the same canonical mutation. Reconnect first reads stored terminal/location authority and cannot choose a destination locally.
- Database outage, content drift, stale/foreign authority, actor-generation mismatch, actor crash, malformed/oversized input, and partial startup fail closed with bounded cleanup.
- Core promotion, M04+ systems, Requiem encounters, parties, public scheduling, Forge/salvage, paid Oath changes, retirement, purge, wardrobe, commerce, and Steam runtime integration remain disabled.

## Acceptance evidence

`GB-M03-03G` and parent `GB-M03-03` require:

- focused routing, actor-generation, portal/room, client-phase, and capability-truth tests;
- PostgreSQL and real-QUIC restart/replay/adverse coverage;
- 25 scripted ordinary private-life journeys without repository shortcuts or developer commands, covering extraction and death/successor branches;
- exact durable graphs, nonduplication, terminal precedence, no stale control, cleanup, login/death timing, and crash-restore assertions;
- optimized native evidence at 1280x720 and 1920x1080 in standard and reduced-effects modes for every major route phase;
- updated tester/README evidence; and
- a final three-authority `GB-M03-03` audit before normal admission is called complete.

The human cohort/comprehension gate, telemetry, support lookup, Steamworks evidence, hosting/IaC, backup/restore rehearsal, and Core promotion remain separate M03 completion owners.

## Consequences

This adds a dedicated runtime and one append-only projection rather than weakening the proven identity server or presenting the disposable harness as production. It costs more integration work, but makes feature advertisement truthful, preserves completed evidence boundaries, and leaves later content structurally closed.
