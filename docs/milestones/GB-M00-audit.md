# GB-M00 completion audit

- **Status:** Passed
- **Audited:** 2026-07-10
- **Authority reviewed together:** `Gravebound_Production_GDD_v1_Canonical.md`, `Gravebound_Content_Production_Spec_v1.md`, and `Gravebound_Development_Roadmap_v1.md`
- **Final implementation commit audited:** `e4b49cdcfb85611bb9932a1e8ce0d12dee38f261`
- **Next feature:** `GB-M01-01A`

## Work-package evidence

| Work package | Evidence |
|---|---|
| `GB-M00-01` | Pinned Cargo workspace contains `client_bevy`, `sim_core`, `sim_content`, `content_schema`, and `tools_content`. |
| `GB-M00-02` | Rust `1.95.0`, exact dependency versions, committed `Cargo.lock`, rustfmt/Clippy policy, tracing, `.env.example`, and line-ending policy are checked in. |
| `GB-M00-03` | `docs/development.md` and `tools/dev.cmd` document and execute bootstrap, format, lint, test, validation, headless replay, LocalLab, local-stack boundary, CI, and Windows release commands. |
| `GB-M00-04` | GitHub Actions gates formatting, full-workspace lint/tests, exact content validation, two replay processes, schema drift, and an optimized Windows build. |
| `GB-M00-05` | Renderer-independent 30 Hz rational accumulator, integer ticks, pinned `ChaCha8Rng`, named BLAKE3-derived streams, and unbiased bounded sampling have unit coverage. |
| `GB-M00-06` | Nonzero monotonic entity IDs, strict content IDs, strict feature IDs, and a 31-entry M00/M01 feature registry are validated. |
| `GB-M00-07` | Ten generated JSON Schemas and typed records cover class, ability, enemy, pattern, arena, item, drop table, release manifest, feature registry, and asset manifest. |
| `GB-M00-08` | The 120-tick fixed-input fixture verifies five selected BLAKE3 state hashes in two separate processes. Entity-order independence is unit tested. |

## Exit-gate evidence

1. **Clean setup and runnable Windows build**
   - A new clone under an empty target directory completed `tools\dev.cmd bootstrap` and `tools\dev.cmd ci`.
   - `tools\dev.cmd release` produced `target\release\client_bevy.exe` locally in 6 minutes 54 seconds.
   - The release executable remained running and responsive during a five-second hidden smoke test and was then closed cleanly.
   - Hosted Windows release jobs also built from empty GitHub-hosted runners.

2. **Two clean-cache CI passes**
   - [Run 29128794033](https://github.com/MikeyPar/Gravebound/actions/runs/29128794033), commit `2e9a8d8`: success.
   - [Run 29128864928](https://github.com/MikeyPar/Gravebound/actions/runs/29128864928), commit `e4b49cd`: success.
   - The workflow defines no dependency or build cache, so both used clean hosted environments.

3. **Fail-closed content contract**
   - Tests reject invalid/case-unstable IDs, unknown schema fields, missing cross-references, illegal M01 ID substitutions, duplicate IDs, unresolved localization/assets, and reward tables whose authored multi-outcome weights do not total 100.
   - The exact `fp.1.0.0` manifest contains 30 records. Its validated package hash is `6b9513d7edf5f52480c4efe372a2e769a1dc5fdc57a1773780c36a6c60212ccb`.

4. **Repeatable known-seed result**
   - Seed `6840227782638526189`, content `fp.1.0.0`, selected ticks `1/30/60/90/120`.
   - Final tick-120 state hash: `9ce920644c159188524bfd8b3d64b0499cdef35ef6b184168701039554797ffa`.
   - Local CI and both successful hosted quality jobs executed the trace twice and matched the same checked-in report.

5. **Stable next-task contract**
   - `content/features/registry.json` assigns stable IDs, dependencies, source-document IDs, and explicit acceptance criteria through `GB-M01-GATE`.
   - README hands execution to `GB-M01-01A`; no account, network, commerce, public-realm, or persistence scope was pulled forward.

## Intentional boundary

`local-stack` remains explicitly unavailable until `server_app` in M02 and PostgreSQL in M03. The command fails with that milestone explanation rather than presenting a false substitute. This is the roadmap-defined M00/M01 boundary, not an incomplete M00 deliverable.
