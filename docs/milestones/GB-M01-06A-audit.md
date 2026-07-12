# GB-M01-06A completion audit

- **Status:** PASS
- **Date:** 2026-07-11
- **Scope:** Local-only death transaction, cleanup, and fresh-run reconstruction
- **Decision:** [ADR-013](../decisions/ADR-013-local-death-freeze-and-fresh-run.md)

## Acceptance evidence

| Criterion | Authoritative evidence | Result |
|---|---|---|
| Same-tick lethal order | Existing 05A integration rejects later held-primary/consumable work at health zero; 06A consumes only its exact pending lethal observation. | PASS |
| Atomic death | Clone-before-commit lifecycle validates provenance/trace/counts, allocates death ID, freezes encounter, clears inventory/belt, and commits once. | PASS |
| Complete local cleanup | Typed census covers enemies, hostile projectiles/hazards, friendly projectiles, pickups, rewards, effects, equipment, backpack, and belt. Client visuals despawn only after authoritative commit. | PASS |
| Fresh successor | Run Again reconstructs full movement/combat/vitals/consumables/enemies/allocators from validated definitions with default seed, exact starter state, and run-qualified IDs. | PASS |
| Retry safety | Duplicate death, restart while alive, invalid observation, count overflow, and restart-tick mismatch are nonmutating typed failures. | PASS |
| Under-three-second control | Fixed seam enforces a 90-tick bound; client measures the complete reconstruction action and semantic evidence requires `<3000 ms` plus active control. | PASS |
| Local quality/evidence gate | 240 tests, strict all-target Clippy, strict content validation, identical repeated traces, optimized build, clean runtime logs, and directly inspected complete frame. | PASS |

## Boundary proof

- This ticket writes no account, memorial, Echo, durable death, item-ledger, telemetry, or server state.
- `DeathFrozen` preserves the dead run for presentation but cannot accept combat activity.
- The old runtime is discarded, not healed or rewound. Successor player, enemies, hostile allocator, item instances, and death IDs occupy a new run-qualified namespace.
- Evidence-only automatic restart waits three fixed ticks and invokes the same explicit restart transaction as `R`; it is unavailable without a screenshot request.

## Verification record

- Focused `sim_core` death selection: 7 tests passed.
- `.\tools\dev.cmd ci`: passed 240 tests: `client_bevy` 30, `content_schema` 3, `sim_content` 23, `sim_core` 184.
- Strict workspace all-target Clippy with warnings denied: passed.
- Strict content validation: passed.
- Repeated M00 deterministic traces: identical at ticks 1, 30, 60, 90, and 120.
- `cargo build -p client_bevy --release --locked`: passed.
- Accepted optimized runtime logs: zero warning/error/panic matches.
- Accepted evidence: [`GB-M01-06A.png`](../evidence/GB-M01-06A.png), SHA-256 `649D1DC9C63FCFD6F98E6F0C718F40EB927F3F1C9B619B5E6FA948DB428D7DC1`.

![Accepted local death and fresh-run frame](../evidence/GB-M01-06A.png)

The inspected frame shows run 2 at the exact `(4,12)` spawn with full `128/128` health, active control, default seed `B311A501`, three starter equipment items, two Tonics, a retained lethal trace/death ID, three frozen ticks, 14 logical/12 visual cleanup records, and old-run IDs retired. Rejected artifacts included an intentionally superseded later-combat frame and two incomplete GPU composites; none were accepted as evidence.
