# GB-M03-01A completion audit

- **Status:** PASS
- **Audited:** 2026-07-12
- **Authorities:** GDD `CLS-001`, `CLS-002`, `CLS-020`, `TECH-002`, `TECH-004`, and `TECH-100` through `TECH-105`; Content Production Specification `CONT-000` through `CONT-003`, `CONT-FP-001`, `CONT-VALID-001`, and `CONT-VALID-003`; Development Roadmap M03 and `GB-M03-01`; approved `SPEC-CONFLICT-004` decisions 1, 3, and 6
- **Task contract:** `docs/tasks/GB-M03-01A.md`

## Acceptance evidence

| Criterion | Evidence | Result |
|---|---|---|
| Immutable FP bytes | Eleven reviewed per-file BLAKE3 locks cover all `content/fp/*.json`, FP localization, `assets.fp.json`, and `fp.1.0.0.json`. The focused test passes and `git diff` is empty for every locked path. | PASS |
| Strict development target | `content/core_dev/identity.json` uses `CoreDevelopmentTarget` with unknown-field denial and exact class, ability, source-version, and presentation-asset allowlists. | PASS |
| Stable class subset | Compilation resolves exactly `class.grave_arbalist`, validates the complete `CLS-020` base payload/metadata, and rejects source drift. | PASS |
| Stable ability subset | Compilation resolves primary, Grave Mark, Slipstep, and Stillness in canonical class order. Existing exact FP ability compilers still validate every gameplay value; Core additionally validates stable metadata/assets. | PASS |
| Approved base silhouette | The only presentation asset is `sprite.class.grave_arbalist`; it resolves through the validated FP asset manifest and is exposed only as a locked preview ID, not an appearance or entitlement. | PASS |
| Leakage prevention | Missing, extra, reordered, duplicate, item, arena, reward, and any `.prototype.` IDs fail closed. Dedicated tests exercise all three prohibited prototype domains. | PASS |
| No premature release | The descriptor type has no bundle, release-stage, promotion, packaging, or output fields. Its compiled view is non-serializable. Strict decoding rejects attempted `bundle_id`, and no Core release/promotion/package artifact exists. | PASS |
| Schema integrity | `schemas/core_development_target.schema.json` is checked in and a test compares it structurally with the schema generated from the Rust contract. | PASS |
| FP rollback remains independent | Existing `load_and_validate` continues to load `fp.1.0.0` without depending on Core development content. The Core loader composes on top of the independently validated FP package. | PASS |

## Verification

- `cargo fmt -p content_schema -p sim_content -- --check`: PASS.
- `cargo test --locked -p content_schema -p sim_content`: PASS; 42/42 unit tests plus both crate doc-test targets.
- `cargo clippy --locked -p content_schema -p sim_content --all-targets -- -D warnings`: PASS.
- `RUST_LOG=info cargo run --locked -p tools_content -- validate`: PASS; `fp.1.0.0`, 34 records, 34 features, content-tree BLAKE3 `f109aac92389c491bd980e71ec2959e100f91b0f972d4b2c948cc39491b42daa`.
- Core compiler focused tests: 6/6 PASS, including byte lock, leakage rejection, stable-record drift, allowlist drift, and no-release-artifact assertions.
- `git diff --check` over every owned implementation/documentation path: PASS.
- `git diff -- content/fp content/manifests/fp.1.0.0.json content/manifests/assets.fp.json content/localization/en-US.json`: empty.

The content-tree hash changes when any new JSON descriptor is added; it is not used as proof that frozen FP bytes changed. The eleven per-file BLAKE3 locks and empty targeted Git diff are the authoritative immutability evidence.

## Outcome

`GB-M03-01A` is complete. It provides a narrow, reusable compiler boundary for `GB-M03-01B` and `GB-M03-01C` without claiming a Core bundle or allowing prototype combat content into the Core identity aggregate. Formal `core.1.0.0` promotion remains blocked until the complete M03 manifest passes `CONT-VALID-003`.
