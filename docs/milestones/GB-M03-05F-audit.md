# GB-M03-05F completion audit

## Result

PASS. The Core Oath/Bargain character-life aggregate now preserves exact Bell Debt state across reconnect, dangerous-room handoff, and recoverable server-process restart; journals dirty state on the authored 30-second danger cadence; and clears final-life state only after committed safe transfer, death, or retirement boundaries. Lifecycle outbox records are canonical, versioned, atomic, and replay-safe. The normal player route, purge, paid Oath changes, full death/retirement features, and Core promotion remain closed.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | `BRG-001`, `BRG-005`, `TECH-015`, and `TECH-023` govern character-life ownership, reconnect/transfer continuity, deterministic ordering, and 30-second danger checkpoints. `QA-005`/`006` govern stale, corrupt, concurrent, restart, and response-loss fixtures. `TEL-001`/`002` govern the typed `bargain_offered`, `bargain_selected`, and `bargain_declined` lifecycle records. |
| Content Production Specification | `CONT-014`, immutable Core content revisions, exact source/layout IDs, and the dormant route restriction bind checkpoint restoration and lifecycle payloads to `world.core_microrealm_01` content without admitting the normal route. |
| Development Roadmap | `GB-M03-05` and the M03 PostgreSQL restart/idempotency exit gates require one complete private-life Oath/Bargain loop. This closes its lifecycle package while leaving the parent milestone open for the remaining `05C` inspected audiovisual evidence. |

Approved `SPEC-CONFLICT-008` and `SPEC-CONFLICT-011` supply Bell's exact persistence, reset, pending-repeat, and cadence-order semantics.

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Versioned danger checkpoint | A bounded canonical Bell DTO and BLAKE3-bound storage envelope cover account, character, lineage, tick, three content hashes, schema, character/progression/inventory/Oath-Bargain versions, counter, and optional complete pending-repeat projectile snapshot. Semantic and byte corruption fail closed. | PASS |
| Authored write cadence | The live aggregate marks Bell mutations dirty, coalesces them, skips clean intervals, writes exactly every 900 simulation ticks in danger, and forces lifecycle-boundary persistence. Ordinary shots never write PostgreSQL. | PASS |
| Reconnect and handoff | One account/character/lineage aggregate rebinds transport without rebuilding combat. Dangerous room-to-room handoff retains the exact counter and pending delay; cross-account, character, lineage, and stale-room attempts fail closed. | PASS |
| Process resume | A fresh immutable combat package restores only a fully bound latest checkpoint. Binding drift, stale aggregate versions, malformed payloads, or content mismatch reject restoration. This seam never changes item security and does not replace danger-entry crash restore. | PASS |
| Safe-transfer ordering | Bell resets and its checkpoint is deleted only after the durable world location is committed safe and the lineage is closed. Failed commit preserves live and durable state; response-loss replay converges through `Deleted` then `Absent`. Dangerous handoff never invokes cleanup. | PASS |
| Death/retirement participant | A reusable transaction participant locks the Oath/Bargain version, snapshots active Bargains in acquisition order, deletes active rows and Bell checkpoint, advances exactly one version, and appends one canonical typed outbox result. Offer and decision history remain immutable. | PASS |
| Lifecycle telemetry | `bargain_offered` is atomic with the open offer, `bargain_selected` remains atomic with acquisition, and `bargain_declined` is atomic with terminal refusal. Offered/declined payloads bind exact source, layout, lineage, restore point, content hashes, aggregate version, and ordered candidates. | PASS |
| Adversarial timing and durability | Tests cover Bell release four/five disconnects, pending repeat at `+1` and `+8`, exact `+9` emission, concurrent/replayed/stale writes, corrupt storage, process restart, safe-transfer failure/replay, and real-QUIC PostgreSQL restart with exact pending-state reconstruction. | PASS |
| Dormant production route | The real Core identity endpoint continues returning stage-disabled world transfer, reports zero combat admissions across restart, and creates no normal-route scheduler. | PASS |

## Verification

- [Authoritative run 29265860539](https://github.com/MikeyPar/Gravebound/actions/runs/29265860539) passes formatting, warnings-denied workspace Clippy, all workspace tests, content validation, repeated deterministic traces, generated-schema drift, the PostgreSQL 17.10 migration/transaction suite, real-QUIC restart proof, and the Windows release build.
- Local closure passes `cargo fmt --all -- --check`, warnings-denied workspace Clippy, 608 tests, strict content validation, two byte-identical deterministic traces, generated-schema verification, the Windows release client build, and `git diff --check`.
- PostgreSQL evidence exercises migrations 0021-0023, checkpoint monotonicity/replay/finalization, offered/selected/declined outbox atomicity, death cleanup, restart reconstruction, and normal-route exclusion.

## Delivery history

The slice was delivered in granular commits for the checkpoint contract/repository, live aggregate, cadence scheduler, process resume, safe-transfer finalization, death/retirement cleanup, persistence-owned lifecycle events, real PostgreSQL/restart evidence, adversarial disconnect timing, and maintainable shared test fixtures.

## Deferred scope

The 50-Ash purge transaction and confirmations; later paid Oath changes; full death, memorial, Echo, successor, or retirement features; normal-route admission; Core promotion; and telemetry export/redaction remain under their owning roadmap packages. `GB-M03-05C` still owns the remaining inspected Nailkeeper/Frostbind audiovisual evidence before parent `GB-M03-05` can close.
