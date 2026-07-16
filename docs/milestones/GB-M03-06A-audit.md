# GB-M03-06A death records, repository, and reliable protocol completion audit

**Status:** PASS on main implementation commit `18dcbad`; hosted CI [`29506273492`](https://github.com/MikeyPar/Gravebound/actions/runs/29506273492) is green.

## Three-authority closure

| Authority | Closure |
|---|---|
| `Gravebound_Production_GDD_v1_Canonical.md` | `DTH-001`, `DTH-020`, `ECH-002`, and `TECH-020`-`022` are represented by immutable records, bounded identifiers/collections, durable acknowledgement, exact historical reads, and an append-only reliable protocol. |
| `Gravebound_Content_Production_Spec_v1.md` | `CONT-ECHO-009` and `CONT-HUB-002` bind stored Echo transitions and Memorial reads to exact Core content and the committed snapshot. |
| `Gravebound_Development_Roadmap_v1.md` | `GB-M03-02D`, `GB-M03-06`, and `GB-M03-13` receive real PostgreSQL and transport-visible replay/restart proof without enabling later gameplay routes. |

## Acceptance evidence

| Criterion | Evidence | Result |
|---|---|---|
| Typed record family | Strict DTOs cover death, summary, Memorial, combat trace/statuses, destruction, result, audit, outbox, Echo snapshot/transitions, and retained live-trace provenance. | PASS |
| Server builders | `server_app` constructs and validates records from sealed simulation/live-trace authority without coupling combat rules into `persistence`. | PASS |
| Danger custody prerequisite | Accepted danger entry snapshots Equipped/Belt and deliberate-risk RunBackpack identities, performs exact security transitions, and restores them atomically after an uncommitted crash. | PASS |
| Append-only protocol | Current protocol is `1.14`; message-kind bytes `1`-`17` remain unchanged and death view remains byte `18`, with immutable legacy `1.13` and older compatibility fixtures. | PASS |
| Authenticated read surface | Latest death, owned historical summary, bounded Memorial pages, and bounded trace pages require authenticated account binding and never expose a lethal mutation command. | PASS |
| Bounds and fail-closed behavior | Empty/maximum collections, ordinals, IDs, UTF-8 byte limits, zero UUIDs, stale versions, foreign ownership, malformed/oversized frames, disabled capability, incompatible content, and corrupt rows are rejected. | PASS |
| Replay/restart/outage | Real QUIC plus PostgreSQL proves response-loss replay, fresh-process reconstruction, unavailable database, serialization retry, corruption rejection, and rollback. | PASS |

## Cumulative verification

- Joint persistence evidence: [`GB-M03-02D-audit.md`](GB-M03-02D-audit.md).
- Integrated source/native evidence: [`GB-M03-06E-integrated-evidence.md`](../evidence/GB-M03-06E-integrated-evidence.md).
- Exact cumulative CI: [`29506273492`](https://github.com/MikeyPar/Gravebound/actions/runs/29506273492), PASS.

## Deferred ownership

Clock/deed/cause/trace selection, final destruction planning, Echo encounter assembly, successor, extraction/Recall, telemetry, support, and route admission remain outside `06A`.

## Handoff

`GB-M03-06A` and joint persistence slice `GB-M03-02D` are closed. Their records and protocol are final authority for `06B`-`06E`; no normal player route is enabled by this audit.
