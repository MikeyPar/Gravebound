# SPEC-CONFLICT-006 — M03 world-flow content and enablement contract

**Status:** Awaiting owner decision

**Raised:** 2026-07-12

**Blocks:** Full implementation and closure of `GB-M03-03`

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The three authorities agree on the Core route:

```text
Character Select
  -> hub.lantern_halls_01
  -> world.core_microrealm_01
  -> dungeon.bell_sepulcher / layout.core_private_life_01
  -> boss.sir_caldus
  -> committed extraction
  -> Lantern Halls
```

They also agree that the server owns instance allocation and transfer, `sim_core` owns deterministic world/dungeon state machines, `sim_content` owns immutable definitions, `persistence` owns location/restore/idempotency transactions, and `client_bevy` presents only authoritative state. The Content Production Specification fixes the Hall geometry, 48×48 micro-realm, six counted Bell rooms plus safe vestibule, exact Core encounter manifest, Sir Caldus arena, and return spawn.

Five details remain non-executable without inventing content or product behavior. The recommendations below keep the route unpromoted and fail closed while preserving the roadmap's later item, oath, death, and Recall ownership.

## Decisions requested

### 1. World-flow child IDs, assets, and tags

**Conflict:** `CONT-001` requires complete records and `CONT-003` permits only documented asset/tag expansion. It defines derivation for items, actors, rooms, abilities, patterns, and modifiers, but not hubs, worlds, landmarks, stations, portals, or spawn anchors. `CONT-WORLD-001` describes a Bell portal and Realm Gate rectangle without stable child IDs. `CONT-HUB-001` names a character-select return coordinate without an object/behavior contract.

**Recommended resolution:** Authorize these exact Core expansion rules:

- Hub/world records use `asset_ids=["tilemap." + id]` and tags `[hub,safe,noncombat]` or `[world,danger,core_microrealm]` respectively.
- Landmark, station, and portal records use `asset_ids=["sprite." + id]` and include their exact category tag plus the behavioral tags below.
- The micro-realm Bell portal is `portal.dungeon.bell_sepulcher` with tags `[portal,dungeon_entry,requires_microrealm_cleared]`.
- The micro-realm Realm Gate return is `portal.return.lantern_halls` with tags `[portal,hall_return,safe_transfer]`.
- The existing `station.realm_gate` uses tags `[station,realm_entry,instant_interaction]`; `landmark.realm_gate` uses `[landmark,realm_entry]`; `landmark.lantern_fork` uses `[landmark,safe_zone]`.
- `(32,44)` is the non-interactive `spawn.hub.character_select_return` child record with `asset_ids=[]` and tags `[spawn_anchor,nonvisual,character_select_return]`. It is used when a living safe character re-enters Hall after returning to character select. Initial entry, normal extraction, and Recall continue to use `(32,42)` as specified.

All IDs remain children of their earliest enabled parent and do not change landmark or room headline counts. No undocumented fallback asset or inferred gameplay tag is allowed.

### 2. Core secret-room validation

**Conflict:** `CONT-ROOM-001` puts all nine Bell room/arena rows in `manifest.rooms.core`, including `room.bell.secret_01`. `CONT-ROOM-008` makes `encounter.secret.bell_01` Slice-only, while `CONT-VALID-001` currently requires every enabled Secret template to bind one stage-legal encounter. Pulling the encounter and its AtRiskPending rewards into Core would expand M03 scope.

**Recommended resolution:** Core compiles and counts `room.bell.secret_01` geometry as an unreachable template, but Secret encounter-binding validation applies only when the template is stage-reachable. `layout.core_private_life_01` keeps all branches disabled. Slice enables reachability and binds `room.bell.secret_01` to `encounter.secret.bell_01`. Core validation must prove there is no graph edge, generation profile, portal, or encounter reference that can instantiate the secret room.

### 3. Core micro-realm admission

**Conflict:** The micro-realm rules refer to plural participants and Hall routing chooses lowest population, but M04 is the first package that defines the eight-player realm and party scope. No M03 cap, join policy, or cross-account admission rule is authored.

**Recommended resolution:** M03 uses one private micro-realm lineage per selected character with capacity one. No party, public matchmaking, cross-account join, or population-based coalescing is enabled. The lowest-population rule begins with the first multiplayer stage in M04. Content and deterministic encounter validators still exercise `N=1..8` so later stages do not inherit untested scaling, but the M03 runtime admits only `N=1`.

### 4. Unpromoted Core world-flow target

**Conflict:** `SPEC-CONFLICT-004` authorized an identity-only `core_dev` compilation target. `GB-M03-03` needs many new Core domains, while `CONT-002` and `CONT-VALID-003` forbid assigning or mutating promoted `core.1.0.0` bytes before complete Core closure.

**Recommended resolution:** Extend the same explicitly unpromoted `content/core_dev` boundary with independently reviewed world-flow data and localization files. The compiler exposes a named world-flow development target, validates exact domain allowlists and hashes, rejects any release/promotion metadata, and permits reviewed additions as later M03 packages land. `fp.1.0.0` remains byte-identical. The exact `core.1.0.0` identifier and promotion record remain unavailable until every Core domain and the M03 exit gate pass.

### 5. Route enablement before later M03 state machines

**Conflict:** `GB-M03-03` precedes items/vault (`GB-M03-04`), oath/Bargain (`GB-M03-05`), atomic death/memorial (`GB-M03-06`), and extraction/Recall loss (`GB-M03-08`). Yet Core Hall marks Vault, Overflow, Memorial, and Oath stations On, and entering danger with a durable selected character is unsafe before death and loss transactions exist.

**Recommended resolution:** Split `GB-M03-03` into independently testable subpackages, but keep its parent player-route gate open:

- `03A`: strict unpromoted world-flow content, schemas, graybox assets, and localization.
- `03B`: reliable protocol plus durable location, restore-point, and idempotent transfer coordinator.
- `03C`: Hall and private micro-realm simulation/presentation.
- `03D`: fixed Bell rooms, Core normal enemies, and minibosses.
- `03E`: production Sir Caldus, committed extraction exit, and Hall return.
- `03F`: native loading/error/reconnect UX, real-QUIC journey, failure, visual, and performance evidence.

Before the owning packages pass, the normal Core runtime keeps Character Select `Play`, Realm Gate entry, and the affected Hall stations behind `core_world_flow_integration`, presenting the exact `AVAILABLE IN A LATER TEST` copy and typed `stage_disabled` result. Disposable integration fixtures may exercise the full route only in the wipeable test namespace; they cannot migrate or be presented as durable player progress. A lethal fixture terminates and is reset—it may never resurrect a dead durable character. `GB-M03-03` closes only after `GB-M03-04`, `05`, `06`, and `08` make the player-visible route and every Core-On station honest.

## Approval requested

Approve all five recommended resolutions, or provide amendments. Once approved, implementation can begin with `GB-M03-03A` and `03B` while the parent remains visibly open through the later M03 integrations.
