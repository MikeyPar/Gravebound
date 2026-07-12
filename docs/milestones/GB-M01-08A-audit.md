# GB-M01-08A completion audit

- **Status:** PASS (local gate; GitHub intentionally excluded)
- **Audited:** 2026-07-11
- **Decision:** `ADR-015`, extended by the Bell composite runtime

## Acceptance evidence

- Versioned BLAKE3 debug hashing covers authoritative player, normal-wave, and Bell state; presentation toggles are excluded and mutation fixtures change the hash.
- Overlay shows fixed seed `B311A501`, hash, simulation/encounter/boss clocks, anchors, hurtboxes, entity/projectile/lane counts, pattern family, threat/cap, and rolling FPS/p95/p99.
- Normal journey is exact `4 -> 6 -> 6`; Bell stage consumes the persistent handoff and reports exact health, phase/break/defeat state, local tick, live `P/L` hazards, and threat `41`.
- F3 remains presentation-only and is explicitly labeled debug-only; ordinary play metrics exclude developer state.

## Verification and evidence

- Normal overview: [`GB-M01-08A-wave.png`](../evidence/GB-M01-08A-wave.png), SHA-256 `9361091F6E2224E6B896A5F179FBA6402DED6C6CB5B9AE4289EEB623FC7325E8`.
- Boss overview: [`GB-M01-04B-04C.png`](../evidence/GB-M01-04B-04C.png), SHA-256 `25604EEDBF9528FBA483DFF45CD4740616D4BB11667A7B59E40DEFBF0017D71F`.
- Workspace tests, strict content, deterministic traces, all-target warnings-denied Clippy, and optimized evidence build pass.

The Bell-dependent prerequisite is closed, so `GB-M01-08B` may be promoted.
