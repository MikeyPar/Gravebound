# Content implementation contract

The checked-in `fp.1.0.0` package is the machine-readable First Playable contract derived from the canonical GDD (`CLS-020`) and Content Production Specification (`CONT-001` through `CONT-FP-010`). The roadmap controls implementation order through `content/features/registry.json`.

Runtime code must deserialize these records through `content_schema` and consume only a package accepted by `sim_content`. Do not parse display strings, infer omitted defaults, rename stable IDs, or load an unlisted record. Numeric gameplay values use the fixed-point units named by each field: milliseconds, milli-tiles, milli-tiles per second, or basis points.

Implementation order for each consuming feature is:

1. Select its stable `GB-*` entry and acceptance criteria from the feature registry.
2. Read the cited GDD, content-specification, and roadmap sections together.
3. Consume the checked-in typed record; do not duplicate its numeric values in presentation code.
4. Add deterministic unit or trace coverage before enabling the behavior.
5. Run `cargo run --locked -p tools_content -- validate`, the golden trace, and the full workspace tests.

`manifests/fp.1.0.0.json` is an exact allowlist. `assets.fp.json` and `localization/en-US.json` are reference manifests for foundation validation; placeholder presentation assets may satisfy these IDs during M01, but unresolved IDs may never ship.

## Unpromoted Core development

`core_dev/identity.json` is a strict internal compiler descriptor for `GB-M03-01A`. It reuses the immutable `fp.1.0.0` Grave Arbalist class, four abilities, and base sprite without copying or relabeling their records. It is not a release manifest, cannot name promotion metadata, and rejects item, arena, reward, or prototype IDs. Formal `core.1.0.0` packaging remains prohibited until the complete Core manifest passes `CONT-VALID-003`.

`core_dev/world_flow.json` is the independent `GB-M03-03A` descriptor. It pins exact BLAKE3 hashes for the typed Lantern Halls, private microrealm, child-record, graybox-asset, and localization sources. Validate it with `cargo run --locked -p tools_content -- validate-core-world-flow`. The compiled view contains geometry only: it has no route destination, encounter, room, release manifest, promotion record, or runtime activation API. The `core_world_flow_integration` gate remains closed until the owning M03 item, Oath/Bargain, death, extraction, and Recall packages pass.

`core_dev/items.json` is the independently hashed `GB-M03-04C` target. It binds the exact 18-item Core catalog, four reward tables, fixed Forged policy, canonical English copy, complete ART-020 icon manifest, and editable vector source sheet under one immutable `core-dev.blake3.*` revision. Validate it with `cargo run --locked -p tools_content -- validate-core-items`. It allocates no item UID, performs no inventory or persistence mutation, emits no production reward seed, and cannot promote or enable a player route.
