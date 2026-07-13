# GB-M03-04D completion audit

## Result

PASS. The exact starter kit, opaque item identities, replay-first reward placement, immutable provenance, and atomic personal-ground expiry are durable and restart-safe. This closes only `GB-M03-04D`; later item resolution, inventory/vault integration, and the parent `GB-M03-04` package remain open.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | Starter loadout, distinct consumable units, RunBackpack-first reward placement, recipient-only ground fallback, 60-second expiry, immutable provenance, idempotency, and `TECH-023` durability follow the approved item lifecycle. |
| Content Production Specification | Starter and reward plans bind the exact checked-in 18-item Core catalog revision, exact Pine Crossbow/Cracked Mark Lens/Red Tonic records, normalized unit semantics, and stage source profiles. |
| Development Roadmap | This supplies the durable starter/reward lifecycle required by `GB-M03-04` and the M03 retry/restart gates without activating unfinished inventory, death, extraction, or normal-route systems. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Exact starter reconciliation | Every character atomically receives one Worn Pine Crossbow, one Worn Cracked Mark Lens, and two distinct Red Tonic units. Bootstrap recreates a missing initializer result with the same opaque UIDs. | PASS |
| Stable item identity | Domain-separated 16-byte BLAKE3 UIDs bind character/request, exact full content revision, roll, and unit ordinal. Zero/collision/cross-unit ambiguity fails closed. | PASS |
| Secret reward planning | A redacted secret-backed epoch derives ChaCha8 plans and audit digests. Replay is independent of secret rotation because the complete accepted result is stored before current planning. | PASS |
| Deterministic placement | Equipment and consumables enter RunBackpack by exact lowest-index rules; units merge before empty slots and overflow becomes recipient-only PersonalGround. | PASS |
| Replay and concurrency | A pre-plan reservation prevents duplicate planning. Identical requests replay the stored normalized result; changed canonical material conflicts; immutable ledger rows conserve every accepted transition. | PASS |
| Exact expiry | Due ground units expire atomically at the authored tick, write deterministic destroyed events, and advance the owning inventory version once. Repeated expiry is nonmutating. | PASS |
| Real PostgreSQL behavior | The disposable lifecycle fixture proves starter creation/backfill, reward replay after epoch rotation, full-backpack ground placement, exact-boundary expiry, immutable ledger counts, corruption refusal, restart durability, and concurrent identity writers. | PASS |

## Verification

- [CI run 29236047011](https://github.com/MikeyPar/Gravebound/actions/runs/29236047011): full hosted gates and the PostgreSQL identity/item lifecycle fixture pass.
- The checked-in starter content revision is the exact `core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb` catalog revision.
- Server library tests, focused protocol tests, warnings-denied Clippy, formatting, and `git diff --check` pass locally; hosted CI supplies the authorized destructive database.
- Reward stream audit vectors are pinned to that exact revision, including the corrected full-revision deterministic draw fixture.

## Granular delivery

The package was separated into schema/repository, UID/placement, reward authority, expiry, PostgreSQL fixture, and architecture commits. Live-audit corrections include `f221db7` (full starter content revision) and `78a7883` (matching reward stream audit vector).

## Deferred parent scope

Resolved affix storage, broader inventory/vault behavior, death destruction, extraction/Recall, Oath/Bargain lifecycle closure, Core promotion, and normal-route activation remain gated by their assigned M03 packages.
