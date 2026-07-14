# GB-M03-04E completion audit

**Status:** PASS on main implementation commit `f5bdce6`; hosted CI [`29342811347`](https://github.com/MikeyPar/Gravebound/actions/runs/29342811347) is green.

## Three-authority closure

| Authority | Closure |
|---|---|
| `Gravebound_Production_GDD_v1_Canonical.md` | `SIM-003`, `SIM-010`-`014`, `LOOT-001`-`005`, `LOOT-010/060`, `UI-001/003/006/011/030`, `TECH-023`, and `QA-003` are represented by typed four-slot equipment, server-owned preview/confirmation, exact replacement placement, immutable item ledgers, health-safe rebuilds, and native accessible review. |
| `Gravebound_Content_Production_Spec_v1.md` | `CONT-ITEM-001`-`004`, `CONT-AFFIX-005`, `CONT-CATALOG-002/003/020/021/040/050`, `CONT-LOC-001`, and `manifest.items.core_18` compile into the exact Worn/Forged Core behavior and approved 18-icon source/runtime closure. |
| `Gravebound_Development_Roadmap_v1.md` | The `GB-M03-04` field-equipment slice now has deterministic four-slot swaps, schema-27 PostgreSQL durability, bounded server projection, authoritative combat construction, native comparison UI, restart/replay proof, and optimized visual evidence while the later storage and full-lifecycle gates remain closed. |

Approved [`SPEC-CONFLICT-007`](../spec-conflicts/SPEC-CONFLICT-007-m03-progression-items.md) supplies the absolute-health and full-backpack source rules. Accepted [`SPEC-CONFLICT-026`](../spec-conflicts/SPEC-CONFLICT-026-m03-core-equipment-composition.md) supplies only the previously missing composition order; it does not add later rarity, affixes, storage, extraction, or route admission.

## Acceptance evidence

| Criterion | Evidence | Result |
|---|---|---|
| Pure authoritative planning | Typed four-slot/eight-index snapshots validate ownership, legal slot, source location, item power/version, inventory version, content revision, preview hash, and exact replacement destination. RunBackpack sources reuse the vacated index; PersonalGround sources require the lowest empty index. | PASS |
| Atomic durable mutation | Schema 27 and the PostgreSQL repository lock the aggregate, move both item rows, advance each changed item and the inventory exactly once, append immutable ledger transitions, and store the bounded replay result in one serializable transaction. | PASS |
| Replay and corruption safety | Exact retry returns the stored result. Changed payload, stale version, cross-owner source, altered preview, malformed location, wrong slot, full PersonalGround swap, and injected transaction failure reject without partial movement. | PASS |
| Bounded server/protocol boundary | Clients submit only source identity and confirmation binding. The authority derives slot, destination, comparison, hashes, versions, and result; bounded wire validation rejects caller-authored or oversized state. | PASS |
| Four-slot combat construction | One atomic loadout query projects canonical Weapon/Relic/Armor/Charm slots. Immutable composition applies relic replacement before Oath, flat Armor health before multipliers, additive same-family basis points, separate healing families, and exact Worn/Forged admission. | PASS |
| Exact runtime definitions | Weapon and Relic overrides feed the actual ability definitions; Bell Locket composes with Lantern Ash in the authoritative tonic policy; Armor, resistance, movement, healing, status, barrier, rested-primary, and resonance policies are carried in the authoritative combat aggregate for the owning runtime tick systems. | PASS |
| Health-safe rebuild | The canonical rebuild preserves absolute current health on maximum increase, clamps on decrease, and retains a living floor of one. Forged four-slot construction proves `150/153`, not percentage healing. | PASS |
| Native accessible review | The optimized [eight-case visual matrix](../evidence/GB-M03-04E-visual-matrix.md) covers comparison and all 18 icons at 1280x720 and 1920x1080 in standard/reduced-effects modes. It preserves the playfield, 14 px floor, non-color warnings, focus metadata, explicit confirmation, in-flight lock, and destination copy. | PASS |
| Optimized/adverse construction | The optimized Windows client builds; workspace deterministic/performance tests and exhaustive planner/server boundary tests pass. The disposable showcase is evidence-only and cannot admit the normal route. | PASS |
| Scope remains closed | CharacterSafe, Vault, extraction placement, death/Recall, Overflow, ResolutionHold, later rarity/affixes, normal Character Select `Play`, Realm Gate admission, and Core promotion remain unavailable. | PASS |

## Immutable presentation identity

- Approved source SVG BLAKE3: `19d49b684fd2b78c84b7aee67b0f94dcc9f8f061acff0ec9c81882bddd2cf9f5`.
- Runtime PNG BLAKE3: `c48daa7c1e7d7e054dd94480031e636a7a892af19d25c5b5091e0b03c55b8da7`.
- Every accepted capture and SHA-256 identity is recorded in the [visual matrix](../evidence/GB-M03-04E-visual-matrix.md).

## Cumulative verification

- Local: `cargo fmt --all -- --check`.
- Local: `cargo test --workspace --locked` (all workspace unit, integration, deterministic, performance, and documentation tests passed; PostgreSQL-required tests remain explicitly ignored locally).
- Local: `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Local: `cargo run --locked -p tools_content -- validate`.
- Local: `cargo run --locked -p tools_content -- validate-core-caldus`.
- Local: `cargo run --locked -p tools_content -- generate-schemas`, followed by a clean schema diff.
- Hosted: main CI `29342811347` passed the Linux quality/content/schema job, mandatory shared-database PostgreSQL suite, and Windows release build on `f5bdce6`.

## Deferred ownership

`GB-M03-04F` owns CharacterSafe 8, Vault 160, safe-Hall transfers, the danger-entry storage preflight hook, and their PostgreSQL lifecycle. `GB-M03-04G` owns real-QUIC item/Vault lifecycle, parent adversarial/performance evidence, and closure of `GB-M03-04` plus `GB-M03-02C`. Extraction/Recall conversion, death, successor recovery, Overflow, ResolutionHold, salvage/crafting, party allocation, later rarity/affixes, route activation, and Core promotion remain with their named roadmap owners.

## Handoff

Proceed to `GB-M03-04F`: add only CharacterSafe and Vault to the durable item locations, implement deterministic safe-Hall transfers, and embed all-or-nothing CharacterSafe preflight inside the dormant serializable danger-entry transaction before identity allocation or location mutation. Keep 04G and every later-owned surface fail closed.
