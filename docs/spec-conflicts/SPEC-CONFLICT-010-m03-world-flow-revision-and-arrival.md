# SPEC-CONFLICT-010 — M03 world-flow revision and return-arrival contract

**Status:** Owner-approved on 2026-07-12

**Raised:** 2026-07-12

**Blocks:** dormant `GB-M03-03B` transfer-coordinator completion; the normal route remains disabled

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The three authorities and approved `SPEC-CONFLICT-006` require an idempotent, content-bound transfer coordinator while keeping the normal Core route fail-closed until the item, Oath/Bargain, death, extraction, and Recall owners pass. The implemented 03B scaffolding now provides protocol 1.7, typed durable roots, typed restore-provider seams, read-only location projection, and real-QUIC `StageDisabled` behavior.

The closeout audit found three exact rules that cannot be completed safely from the current documents. None changes the approved route gate.

## Decisions requested

### 1. Exact Core world-flow revision on wire and disk

**Conflict:** `GB-M03-03A` deliberately pins records, assets, and localization with three independent BLAKE3 hashes. No authority defines one composite hash. The current 03B draft reuses the immutable FP package hash, which does not cover any 03A world-flow byte and therefore cannot detect world-flow drift.

**Recommended resolution:** Replace the transfer contract's single `content_manifest_hash` with a typed `WorldFlowContentRevisionV1 { records_blake3, assets_blake3, localization_blake3 }`. Each member is the exact validated 03A hash. Persist and compare the triple without concatenating or rehashing it. The normal handshake keeps its existing FP source hash and unpromoted Core target label; the separately typed world-flow revision binds only world-flow requests, receipts, lineages, restore points, and checkpoints. Any member mismatch returns `ContentMismatch` before allocation or mutation.

### 2. Character Select re-entry arrival

**Conflict:** approved `SPEC-CONFLICT-006` distinguishes initial Character Select → Hall arrival at Hall default `(32,42)` from Hall → Character Select → Hall arrival at `spawn.hub.character_select_return` `(32,44)`. Current `CharacterSelect` location state carries no next-arrival provenance, so the coordinator cannot reproduce both rules after reconnect or restart.

**Recommended resolution:** Make Character Select carry a durable `next_hall_arrival: SafeArrival`. New characters use `HallDefault`; an accepted Hall → Character Select transfer stores `SpawnAnchor("spawn.hub.character_select_return")`. Entering Hall consumes that value as the committed safe arrival. Preserve it in location projections and receipts so replay and restart are exact. No client may choose or override the arrival.

### 3. Typed payload-hash mismatch behavior

**Conflict:** protocol validation currently rejects a canonical payload-hash mismatch during decode, so the authority never gets the request and cannot return the approved typed `PayloadHashMismatch` result. The gate collapses other frame-validation failures into `ServiceUnavailable`, which also obscures the exact reason.

**Recommended resolution:** Keep codec validation responsible for frame bounds, nonzero identifiers, known variants, and syntactic content values. Move canonical payload-hash equality to the authenticated world-flow authority. A well-shaped request with a mismatched hash returns `PayloadHashMismatch` with no snapshot, allocation, ID generation, or durable write. Malformed/oversized/unknown wire data still fails at decode and closes only that request stream.

## Fixed implementation scope after approval

- A dormant disposable coordinator locks account → character/status → location → receipt.
- Exact replay precedes current-state validation and is read-only; changed binding/hash returns `IdempotencyConflict`.
- Safe transfer commits location, one character-version increment, and one immutable receipt atomically.
- Danger fixture transfer captures all three typed providers and commits the capacity-one lineage, complete restore root, danger location, and receipt in one serializable transaction.
- Normal runtime continues to return `StageDisabled` before allocation, ID generation, or transaction and never advertises `core_world_flow_integration`.
- Real progression/item/Oath storage remains owned by `GB-M03-04`/`05`; range/allocation/presentation by `03C`; death/extraction/Recall by `06`/`08`.

## Approval record

The owner approved all three recommended resolutions without amendment on 2026-07-12. The dormant coordinator and disposable PostgreSQL proof may proceed exactly as recorded above without inventing a composite revision or losing approved arrival semantics.
