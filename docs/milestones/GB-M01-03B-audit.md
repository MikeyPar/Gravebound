# GB-M01-03B completion audit

- **Status:** `PASS` (local gate; GitHub intentionally excluded)
- **Audited:** 2026-07-10
- **Authorities reviewed together:** GDD `SIM-010/011`, `COM-001/003/005/006/009`, `ENC-003/004`; Content `CONT-010/011/013`, `CONT-FP-003/004/008/009`; Roadmap M01 day four, work package `GB-M01-03`, ordering row 18
- **Content:** `enemy.bell_reed`, `pattern.enemy.bell_reed.gap_ring`, `reward.prototype.normal_enemy`
- **Decision:** `ADR-010`

## Evidence matrix

| Criterion | Current evidence | Result |
|---|---|---|
| Exact FP compilation | Unique exact enemy/pattern/reward/manifest records; exact first/repeat telegraphs, state order, pattern kind/payload, cues, damage metadata, cap, and sim-definition equality. Same-rounded `3001 ms` lifetime drift fails. | Passed |
| Deterministic scheduler | Spawn/dormant/telegraph/fire/recover uses cycle-start anchoring. Golden warnings/fire occur at `42/56` and `132/141`; omitted gaps progress `[0,1]` to `[3,4]`. | Passed |
| Fixed trace | BLAKE3 `fa33c935fe16283c2366ee1d2456143367d3d4c49a4ba2a58cff0b3d27685182`; includes explicit `pierces_players=false`. | Passed |
| Cumulative automated gate | 220 tests pass with strict lint/content validation and identical repeated foundation traces. | Passed |
| LocalLab presentation | Accepted optimized frame shows the distinct Reed, six purple diamond projectiles around an omitted corridor, event/hit counter, and textual damage state. | Passed |
| Hostile collision/damage | Swept Veil Chip contact reaches shared vitals through canonical damage with stable ring source/cast identity. | Passed |
| Death/drop readability | Death showcase records `85/0/300`, `DEATHS 1`, `DROPS 1`; the Reed disappears and its gold reward seam appears after eight ticks. | Passed |
| Runtime evidence | Release build and both inspected frames are warning/error/panic clean. | Passed |

## Adversarial evidence

- Wrong kind, band/type, projectile/lane optional fields, cue, disposition, memory, threat, cap, state order, reward, manifest, or duplicate record fails closed.
- First and repeated warning durations cannot collapse into one hidden default.
- Gap index progression is integer/stable; unordered collections do not select emitted indices.
- Simulation owns no renderer, health mutation, death, reward roll, or persistence.

## Accepted runtime evidence

- Shared combat frame: [`GB-M01-03A-03C.png`](../evidence/GB-M01-03A-03C.png), SHA-256 `5B87769E6379CE4BAFF9BFBCC3878A5CCFC7394AA9E76BDB9E8057B935F66408`.
- Death/drop frame: [`GB-M01-03-death-drop.png`](../evidence/GB-M01-03-death-drop.png), SHA-256 `EE4400F1A7E4BC82EB485E1A2B7D3A2DEC85E4D542AF061EF1D770622AF42B0B`.
- Reward resolution/pickup remains owned by `GB-M01-07A/B`; player death UI remains `06A/B`.
