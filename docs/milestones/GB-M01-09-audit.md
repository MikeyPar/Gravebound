# GB-M01-09 completion audit

- **Status:** PASS
- **Audited:** 2026-07-11
- **Authorities reviewed together:** GDD `TECH-070`, `TECH-060`, `COM-009`, and QA stress/determinism requirements; Content Production Specification Bell Proctor `CONT-FP-005`; Development Roadmap M01 day 9, `GB-M01-09`, and the M01 exit gate; `docs/milestones/GB-M01-completion-plan.md`
- **Fixture seed:** `0x47424D30312D3039`
- **Content bundle:** `fp.1.0.0`
- **Immutable executable ID:** `release-3211217280767d1003b98269096dd1dfe3ed4a76128c84089b0d9e6624c82a9a`
- **Executable SHA-256:** `2F5518A56A55E0A49948433DCF02D94F8DAC09AB632D026DCF7445B418CA70B0`

## Final target-hardware evidence

The operator verified and recorded the actual Windows machine rather than substituting the minimum-reference description:

| Field | Recorded value |
|---|---|
| Operating system | Windows 10 Home |
| CPU | Intel Core i7-10700K at 3.80 GHz, 8 physical cores |
| Memory | 68,604,682,240 bytes |
| GPU | AMD Radeon RX 6700 XT |
| Render size | 1920×1080, fixed/non-resizable for the benchmark |
| Attestation | Target-class-or-better verified; actual hardware remains explicit in every report |

### Full effects — canonical acceptance run

Evidence:

- Report: [`docs/performance/GB-M01-09-full-final-30m.json`](../performance/GB-M01-09-full-final-30m.json)
- Screenshot: [`docs/evidence/GB-M01-09-full-final-30m.png`](../evidence/GB-M01-09-full-final-30m.png)

| Metric | Result | Required | Verdict |
|---|---:|---:|---|
| Real rendered duration | 1,800,001 ms | At least 1,800,000 ms for memory | PASS |
| Rendered frame samples | 282,225 | Nonempty real-frame series | PASS |
| Measured FPS | 156.802 | At least 60 | PASS |
| Frame time p95 | 14.092 ms | At most 16.7 ms | PASS |
| Frame time p99 | 18.162 ms | At most 33.3 ms | PASS |
| Hostile projectiles | 800 | Exactly 800 fixture load | PASS |
| Enemies | 40 | Exactly 40 fixture load | PASS |
| Hostile telegraphs | Retained | Never culled | PASS |
| Culling | None | Full mode permits none | PASS |
| Memory samples | 181 at 10-second cadence | Complete 30-minute series | PASS |
| First / last RSS | 305,901,568 / 309,800,960 bytes | No monotonic leak | PASS |
| Peak RSS | 309,800,960 bytes (295 MiB displayed) | At most 1,500,000,000 bytes | PASS |
| Memory assessment | `pass` | `pass` | PASS |
| Overall report acceptance | `pass` | `pass` | PASS |

Hashes:

- Canonical report BLAKE3: `c4e62ff6d7d13091dfd61dd53ad364c1cb813107ed5d246181fc0e92519f745a`
- Report file SHA-256: `E645346482C88A7A9F8EFA9AF9F95F850E259B7545C349EF7677AD3A950CB070`
- Screenshot SHA-256: `04D108F39624E8D65383047459137C1D54CD651F35EBFEA7AD85DB5B95A9332C`

### Reduced effects — documented fallback

Evidence:

- Report: [`docs/performance/GB-M01-09-reduced-final-60s.json`](../performance/GB-M01-09-reduced-final-60s.json)
- Screenshot: [`docs/evidence/GB-M01-09-reduced-final-60s.png`](../evidence/GB-M01-09-reduced-final-60s.png)

The same executable sustained 170.836 FPS with p95 12.415 ms and p99 14.775 ms over 60.009 seconds and 10,251 rendered frames. It retained all 800 hostile projectiles, 40 enemies, and hostile telegraphs while culling only priority 5 decorative ambience and then priority 4 remote-friendly effects (`[5,4]`). Report BLAKE3 is `36a9015598b8c35a21258a89f3b5a990f739ead0aedc1ca2d7f5bd21da31c410`; report SHA-256 is `88BF0629B1785E2CFD48407220018ABDE84795D5FA89F95BCD4E626071716A3A`; screenshot SHA-256 is `F9AAD609C3B27076558A15323B03961BB879ADFE72767AFE4C3EC80C15AEEA22`.

The reduced report intentionally returns `memory_failed` / `insufficient_duration` because it is a 60-second fallback capture. This is not the milestone memory verdict: the same binary's full-effects report supplies the required passing 30-minute memory series, and full effects already exceed every frame target without fallback.

## Determinism and reliability evidence

| Criterion | Evidence | Result |
|---|---|---|
| Exact density | `StressFixture` continuously maintains exactly 800 hostile projectiles and 40 enemies. | PASS |
| Deterministic 60-second fixture | Independent 1,800-tick runs match golden state hash `7dac4876dea54c1e12d5a86febf8f2f33206e4f2ffa6f78c27195442bd3b975a`. | PASS |
| Bell replay | Independent 1,800-tick Bell Proctor scheduler replays match `68558b94dfed325a84b0074ac16ac6a298d7fa063ce28a99607046d8ab643546`. | PASS |
| Twenty complete boss runs | All 20 fixed-input 2,700-tick runs enter the defeated state and share `534e36440d915778945ff42b1937041bf1a2ad8809430fda428c29815a34400f`. | PASS |
| Fail-closed provenance | Simulation timing cannot pass as rendered FPS; short memory, missing process memory, wrong resolution, unverified hardware, invalid culling, or missing telegraphs cannot produce acceptance. | PASS |
| Immutable identity | Diagnostics, opt-in telemetry, and performance reports consume the same executable-derived BLAKE3 build ID. | PASS |

## Visual review

Both 1920×1080 captures were inspected. The full frame visibly reports `Pass`, exact 800/40 load, full mode, no culling, complete 30-minute duration, frame percentiles, and peak RSS. The reduced frame visibly reports the permitted `[5,4]` culling and retains the hostile field and warning shapes. The diagnostics, benchmark panel, health/consumable HUD, and input legend remain outside the center/lower-middle aiming corridor. A misleading display artifact in the image viewer was ruled out by checking the opaque source PNGs and a sampled left-region pixel comparison (99.90% identical UI region); the saved evidence is intact.

## Final verification

- `tools\dev.cmd ci`: PASS.
- Workspace tests: 294/294 (`client_bevy` 44, `content_schema` 3, `sim_content` 30, `sim_core` 217).
- Warnings-denied all-target Clippy: PASS.
- Formatting: PASS.
- Strict `fp.1.0.0` content validation: PASS, 34 records.
- M00 deterministic trace executed twice with identical selected-tick hashes: PASS.
- Optimized workspace release build: PASS.
- Optimized 1280×720 release smoke after responsive diagnostics correction: PASS; screenshot SHA-256 `F2A6C81D84D622633B21DC1981EFF7D211AF503DBE91EF322D5B4777FD750F75`.
- GitHub operations: intentionally excluded by user direction.

Earlier 60-second and pre-responsive-layout reports remain noncanonical development evidence. Only the `*-final-*` report/capture pair above is accepted for this ticket.
