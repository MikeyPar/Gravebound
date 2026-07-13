# GB-M03-05B completion audit

## Result

PASS. The free initial Grave Arbalist Oath choice is server-authoritative, explicitly confirmed, durable, replay-safe, inventory-safe, and preserved across a real QUIC server restart. Later paid changes and the remaining Bargain/lifecycle slices remain disabled.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | Level-10 specialization, exact permanent-life choice, Hall-only eligibility, mutation exclusion, and irreversible-action confirmation are enforced by authoritative locked state. |
| Content Production Specification | The Core allowlist contains exactly Long Vigil and Nailkeeper; the shrine projection and native review UI use exact immutable content/localization and the permanent-life warning from `CONT-HUB-002`. |
| Development Roadmap | This closes the durable initial-choice portion of `GB-M03-05`, including exact replay and restart behavior, while later paid changes, Bargain offers/mechanics, and life closure retain their own work packages. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Bounded reliable contract | Protocol 1.9 binds character, exact Oath, three-part content revision, confirmation, character-life version, mutation identity, time, and canonical payload hash. | PASS |
| Authoritative eligibility | The selected owned living character must be level 10, in exact Lantern Halls state, version-aligned, unresolved-mutation-free, and backed by a complete safe inventory aggregate. | PASS |
| Atomic life mutation | Selection, character/location version advance, immutable result, and one `oath_selected` outbox event commit together under serializable account/character/inventory locks. | PASS |
| Exact replay | Identical mutation retry returns the stored result after state changes; changed material under the same ID conflicts. A later different choice remains typed `stage_disabled`. | PASS |
| Accessible confirmation | Choosing a card enters a separate review state; only the explicit confirm action mutates. Text, names, descriptions, warning, action labels, keyboard controls, and rejection states do not rely on color. | PASS |
| Restart durability | The production reliable QUIC route selects Long Vigil, replays exactly, restarts the bound server, bootstraps the progressed/Oathed roster, and reads the same Oath projection. | PASS |

## Verification

- [CI run 29236047011](https://github.com/MikeyPar/Gravebound/actions/runs/29236047011): the combined PostgreSQL repository and real-QUIC Oath restart fixture passes with all workspace gates.
- The live audit exercises the actual persistent server/bot transport boundary, not a direct service-only substitute.
- Protocol, content, server authority, native UI, and combat-factory regression suites pass. Formatting, warnings-denied Clippy, and `git diff --check` pass locally.
- `8353c9f` corrects the shared roster validator to accept GDD-legal levels 1-20 and only the exact Core Oaths at level 10+, eliminating the restart-only `ServiceUnavailable` mask.

## Deferred parent scope

Later 40-Ash Oath changes, `GB-M03-05D` Bargain offers/shrine UI, `05E` Bargain mechanics, `05F` life/crash closure, non-Arbalist Oaths, Core promotion, and normal-route activation remain closed.
