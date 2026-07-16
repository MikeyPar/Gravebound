# GB-M03-06C single-writer terminal death completion audit

**Status:** PASS on main implementation commit `18dcbad`; hosted CI [`29506273492`](https://github.com/MikeyPar/Gravebound/actions/runs/29506273492) is green.

## Three-authority closure

| Authority | Closure |
|---|---|
| `Gravebound_Production_GDD_v1_Canonical.md` | `DTH-001`, `TECH-015`, and `TECH-021`-`023` are represented by one server-owned terminal arbiter, canonical destruction plan, serializable final transaction, exact replay, and crash-safe precedence. |
| `Gravebound_Content_Production_Spec_v1.md` | `CONT-ECHO-009` participates synchronously in the death transaction; no qualifying death commits without its exact Echo decision and immutable transitions. |
| `Gravebound_Development_Roadmap_v1.md` | `GB-M03-06`/`13` atomicity and nonduplication gates pass while live extraction/Recall repository competition remains correctly assigned to `GB-M03-08`. |

## Transaction acceptance

| Criterion | Evidence | Result |
|---|---|---|
| Shared terminal seam | Five terminal kinds use stable append-only discriminants; all producers evaluate one tick before sealing; lethal death has same-tick precedence; unresolved terminal state blocks later mutation. | PASS |
| Canonical destruction | The pure planner sorts Equipment, Belt durable UIDs, RunBackpack stacks/UIDs, PersonalGround tuples, then material IDs; it derives ordinals, versions, and ledger IDs and rejects duplicates/overflow. | PASS |
| Complete custody matrix | Hosted zero, ordinary, and full custody covers every location family, Belt UID ordering, empty/nonempty pouch, and CharacterSafe/Vault preservation. | PASS |
| Single transaction | Character death, aggregate versions, item/material destruction, Bargain/Bell cleanup, trace/death/summary/Memorial/receipt/audit/outbox rows, and Echo projection publish together or roll back together. | PASS |
| Entry/crash authority | Safe Equipped/Belt in danger is rejected; accepted entry conversion, CharacterSafe preflight, exact restore, malformed custody, and rollback leave no partial root/lineage/item/ledger state. | PASS |
| Replay/adverse behavior | Exact/altered retry, stale/foreign authority, concurrent lethal writers, response loss, restart, outage, serialization retry, representative participant failures, deferred-constraint failure, and corruption are covered. | PASS |
| Post-death seal | Item, progression, Bargain, and world commands reject the dead aggregate; no new rows or signature drift appears; exact terminal replay remains available. | PASS |
| Presentation gate | Completed `06D` consumes only durable acknowledgement; qualifying normal-route death remains disabled until later route owners pass. | PASS |

## Cumulative verification

- Joint persistence/protocol: [`GB-M03-02D-audit.md`](GB-M03-02D-audit.md) and [`GB-M03-06A-audit.md`](GB-M03-06A-audit.md).
- Deterministic terminal inputs: [`GB-M03-06B-audit.md`](GB-M03-06B-audit.md).
- Atomic Echo participant: [`GB-M03-13-audit.md`](GB-M03-13-audit.md).
- Integrated hosted/native evidence: [`GB-M03-06E-integrated-evidence.md`](../evidence/GB-M03-06E-integrated-evidence.md).

## Deferred ownership

`GB-M03-08` owns production extraction/Recall writers and their live repository competition. Successor, Requiem encounters, telemetry, support, platform, and normal route admission remain open.

## Handoff

`GB-M03-06C` is closed. Its terminal journal/arbiter remains the shared seam for `GB-M03-08` without reopening death authority.
