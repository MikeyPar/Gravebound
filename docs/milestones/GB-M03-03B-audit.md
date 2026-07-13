# GB-M03-03B completion audit

## Result

PASS. The dormant world-flow foundation now provides bounded reliable protocol, exact content-bound requests, durable typed locations and return arrivals, replay-first safe transfers, and one atomic disposable danger-entry coordinator. A committed danger entry contains exactly one capacity-one lineage, three typed restore components, one complete restore root, the danger location, and one immutable receipt. Normal real-QUIC admission remains fail closed and allocates or persists nothing.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical Production GDD | `LOOP-001`, `DTH-001`, `DTH-010`-`011`, `TECH-004`, `TECH-010`-`011`, and `TECH-020`-`023` require server-authoritative explicit transfer, durable safe/danger location, complete pre-entry restore state, idempotency, and commit-before-handoff behavior. The coordinator captures progression, inventory/Belt, and Oath/Bargain state before publishing danger admission. |
| Content Production Specification | `CONT-001`-`003`, `CONT-WORLD-001`, `CONT-ROOM-007`, `CONT-HUB-001`-`002`, and validation contracts supply the exact `hub.lantern_halls_01` Realm Gate route to `world.core_microrealm_01` / `layout.core_private_life_01`, fixed capacity, arrival semantics, strict IDs, and independent records/assets/localization hashes used on wire and disk. |
| Development Roadmap | `GB-M03-03` requires explicit Character Select -> Hall -> microrealm transfers and the M03 exit gate requires restart/idempotency evidence. Approved `SPEC-CONFLICT-006` splits `03B` from simulation/presentation and keeps the parent route closed; approved `SPEC-CONFLICT-010` fixes the exact revision triple, return-arrival state, and typed payload mismatch behavior. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Reliable bounded protocol | Protocol 1.7 appends bounded world-flow request/result kind 11, retains pinned 1.5/1.6 compatibility fixtures, rejects malformed wire data, and moves canonical payload equality to the authenticated authority for a typed `PayloadHashMismatch`. | PASS |
| Exact content binding | Requests, receipts, lineages, restore roots, and checkpoints persist and compare the exact records/assets/localization BLAKE3 triple. Drift in any member rejects before mutation. | PASS |
| Durable safe location and arrival | Typed projections preserve Character Select, Hall, and danger state. Initial Hall entry uses the default arrival; Hall re-entry through Character Select consumes the exact return anchor after reconnect/restart. | PASS |
| Replay-first authority | Account-ordered locking resolves an identical stored receipt before current-state validation. Matching retries are read-only after later state changes; changed canonical material conflicts. | PASS |
| Atomic danger entry | One owned serializable session captures real progression plus transactional inventory/Belt and Oath/Bargain providers, stages distinct lineage/restore IDs, inserts the complete root, advances danger location, and stores the receipt before one commit. | PASS |
| Complete restore graph | The root digest binds all typed component versions/digests and exact content/destination data. Deferred root referential validation permits child-first staging but refuses a missing or inconsistent graph at commit. | PASS |
| Concurrency and rollback | Competing entry attempts publish exactly one accepted capacity-one lineage. A failure after progression staging rolls back components, lineage, root, receipt, and location together. | PASS |
| Restart and corruption | Disposable PostgreSQL proves accepted replay after pool restart and refuses stale, foreign, or corrupted stored projections instead of repairing or guessing. | PASS |
| Production route boundary | Real-QUIC production ingress continues to return `StageDisabled` before transaction/ID allocation, advertises no world-flow feature, and leaves lineage, restore, and receipt tables empty. | PASS |

## Verification

- [CI run 29284193972](https://github.com/MikeyPar/Gravebound/actions/runs/29284193972) is the authoritative hosted run for schema version 24, the mandatory PostgreSQL world-flow fixture, format, warnings-denied lint, workspace tests, strict content validation, deterministic trace/schema checks, and Windows release construction.
- Local closure passes `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`, `cargo run --locked -p tools_content -- validate`, and `git diff --check`.
- The mandatory PostgreSQL target proves exact root IDs/layout/revision/version/digest and location, restart replay, changed-material conflict, same-state concurrency, rollback after partial provider staging, and stale/foreign/corrupt fail-closed behavior.
- The living-only database constraint intentionally prevents fabricating a persisted dead character before `GB-M03-06A` widens the life-state schema under approved `SPEC-CONFLICT-009`. The coordinator unit gate proves typed `CharacterDead` rejection; persisted death/re-entry integration remains owned by `GB-M03-06` and is not claimed here.

## Granular delivery

- `3675c30` - bounded reliable world-transfer protocol.
- `2050a6d` - normalized transfer projections.
- `fbb6521` - durable world locations.
- `920cd95` - world-flow repository and transaction foundation.
- `d2392dd` - fail-closed normal server route.
- `61817e4` - exact revision binding and return arrival.
- `a6c6242` - replay-first dormant safe coordinator.
- `d42b728` - atomic composite danger-entry root.
- `1ba3f5f` - PostgreSQL replay/concurrency/rollback/restart/corruption proof.
- `f5d0fde` - pinned danger-root digest assertion.
- `30b822d` - schema-readiness advancement for migration 24.
- `1968a8e` - atomic danger-entry architecture decision.
- `fab59bf` - serialization-loser retry proof for the concurrency fixture.

## Remaining ownership

`GB-M03-03C` owns Lantern Hall and private-microrealm simulation/presentation. `03D`-`03F` own the fixed dungeon journey, encounters, Sir Caldus, committed extraction return, loading/reconnect UX, real-QUIC route evidence, and performance closure. `GB-M03-04`, `06`, and `08` still own production inventory preflight, death, extraction, and Recall semantics. The normal player route, Core promotion, and affected Hall stations remain fail closed until those packages pass.
