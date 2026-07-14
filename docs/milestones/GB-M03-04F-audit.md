# GB-M03-04F completion audit

**Status:** PASS on main implementation commit `5f10460`; hosted CI [`29355855048`](https://github.com/MikeyPar/Gravebound/actions/runs/29355855048) is green.

## Three-authority closure

| Authority | Closure |
|---|---|
| `Gravebound_Production_GDD_v1_Canonical.md` | `LOOP-005`, `LOOT-002`, `LOOT-050`, `LOOT-060`, `TECH-020`, `TECH-023`, and the Lantern Halls Vault contract are represented by exact CharacterSafe 8 and Vault 160 capacities, lowest-index deterministic placement, safe-only custody, immutable ledgers, and an all-or-nothing danger-entry preflight. |
| `Gravebound_Content_Production_Spec_v1.md` | `CONT-HUB-001/002` supplies the compiled Lantern Halls, Vault, and Realm Gate identities and the storage-resolution behavior. The implementation consumes those identities without enabling the normal station or Realm Gate route. |
| `Gravebound_Development_Roadmap_v1.md` | The `GB-M03-02`/`GB-M03-04` safe-storage slice now has additive PostgreSQL durability, bounded server authority, restart/replay proof, deterministic concurrency, and atomic danger-entry participation while the final real-QUIC parent closure remains with `04G`. |

Approved [`SPEC-CONFLICT-005`](../spec-conflicts/SPEC-CONFLICT-005-m03-persistence-order.md) supplies the additive domain-migration and real-PostgreSQL boundary. Approved [`SPEC-CONFLICT-007`](../spec-conflicts/SPEC-CONFLICT-007-m03-progression-items.md) supplies the exact capacities, consumable-unit ordering, wipeable revision contract, and `04F`/`04G` ownership split.

## Acceptance evidence

| Criterion | Evidence | Result |
|---|---|---|
| Exact durable model | Schema 28 appends only `CharacterSafe=5` and `Vault=6`, preserves every prior discriminant, enforces legal capacities/security/custody, preserves existing rows through forward migration, and rejects unsafe rollback while either new location is live. | PASS |
| Deterministic pure planner | One typed planner validates exact 8/160/8 snapshots, places equipment at the lowest empty index, merges consumables into ascending matching nonfull stacks, opens lowest empty indices, and orders units by ascending unsigned UID. | PASS |
| Atomic manual transfers | Schema 29/30 stores one normalized receipt plus bounded placement rows. Accepted transfers update every item and ledger once, advance account/inventory aggregates once, and commit in one serializable transaction. | PASS |
| Bounded server/protocol boundary | Clients bind identity, command, source slot/item, and expected aggregate versions; they cannot author destinations, security, item versions, placements, or result hashes. Exact 8/160 source and six-placement response bounds reject malformed frames before mutation. | PASS |
| Safe Hall authority | CharacterSafe-to-Vault, Vault-to-selected-CharacterSafe, and explicit CharacterSafe-to-RunBackpack require one owned living selected character at the exact Hall location with no unresolved mutation. Foreign, dead, stale, danger, and cross-character bindings fail closed. | PASS |
| Danger-entry preflight | The existing caller-owned world-flow serializable transaction validates replay/source/content/versions, plans the complete CharacterSafe deposit, and performs it before lineage, restore, instance identity, or location mutation. Successful roots capture post-mutation account/inventory versions. | PASS |
| Capacity and no-op behavior | Empty CharacterSafe is a version-stable no-op. A full Vault returns typed `StorageResolutionRequired` without item, ledger, aggregate, ID, root, lineage, or location mutation. Prior deliberate-risk placement remains `AtRiskPending` and permits the dormant entry path. | PASS |
| Replay, restart, and rollback | Exact retries return stored results; altered payloads conflict. Process restart preserves placements and receipts. Injected ledger/provider failures roll back every item, ledger, version, placement, receipt, restore root, and world-flow write. | PASS |
| Concurrency | Final-slot claims serialize to one winner. Concurrent manual transfer and danger entry converge to one legal serial order; bounded retries absorb PostgreSQL serialization/deadlock victims without exposing an untyped transport failure. | PASS |
| Existing safety readers | Reward, combat-loadout, field-equipment, Oath/Bargain, and world-flow readers recognize the two safe locations only where owned and do not broaden danger or equipment admission. | PASS |
| Scope remains closed | Extraction placement, Overflow, ResolutionHold, death destruction, successor recovery, salvage/crafting, normal station interaction, Realm Gate admission, and Core promotion remain unavailable. | PASS |

## Cumulative verification

- Local: `cargo fmt --all -- --check`.
- Local: `cargo test --workspace --locked` (all workspace unit, integration, deterministic, performance, and documentation tests passed; PostgreSQL-required tests remain explicitly ignored locally).
- Local: `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Local: `cargo run --locked -p tools_content -- validate`.
- Local: `cargo run --locked -p tools_content -- validate-core-caldus`.
- Local: `cargo run --locked -p tools_content -- generate-schemas`, followed by a clean schema diff.
- Hosted: main CI `29355855048` passed the Linux quality/content/schema job, mandatory shared-database PostgreSQL suite, and Windows release build on `5f10460`.

## Deferred ownership

`GB-M03-04G` owns the real-QUIC full item/Vault lifecycle, parent adversarial/performance evidence, and closure of `GB-M03-04` plus `GB-M03-02C`. Extraction/Recall conversion, death, successor recovery, Overflow, ResolutionHold, salvage/crafting, party allocation, later rarity/affixes, station and normal-route activation, production namespace cutover, and Core promotion remain with their named roadmap owners.

## Handoff

Proceed to `GB-M03-04G`: expose the already bounded item/Vault lifecycle only through the disposable real-QUIC integration harness, prove PostgreSQL restart/replay/adversarial/performance behavior at the production protocol boundary, and publish the parent `GB-M03-04`/`GB-M03-02C` closure audits. Do not add gameplay rules or open the normal player route.
