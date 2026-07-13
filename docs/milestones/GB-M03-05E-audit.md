# GB-M03-05E completion audit

## Result

PASS. The three enabled Core Veil Bargains compile from one atomic durable character snapshot and apply their exact authored combat, health, cadence, consumable, and belt behavior. Optional-Oath level-5 construction, fresh-arena reconstruction, PostgreSQL restart, accessible native feedback, and the complete legal item-bound combination matrix are covered without enabling purge, life cleanup, Core promotion, or the normal player route.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | `BRG-001`-`005` govern the ordered active loadout, three-Bargain cap, Cinder Hunger `1.18` direct output and `0.88` health, Bell Debt `0.85` primary rate/fifth-release repeat, Lantern Ash `1.40` potion output and one active belt slot, and global resolved-stat caps. `COM-002` owns Cinder's attacker-stage placement. The authored Q/E two-slot input contract remains visible. |
| Content Production Specification | `CONT-014`, `CONT-HUB-002`, `CONT-FP-010`, and `CONT-CATALOG-003` bind the Core allowlist, immutable acquiring-offer revision, exact records, named item effects, and build-time combination validation. Runtime definitions are compiled from validated content rather than duplicated authoring constants. |
| Development Roadmap | This closes the executable mechanics portion of `GB-M03-05` and its restart/idempotency exit gate. Durable death/retirement/safe-transfer lifecycle wiring remains isolated to `05F`; purge, promotion, and normal-route activation remain closed. |

Approved `SPEC-CONFLICT-008` and `SPEC-CONFLICT-011` supply Bell's exact emission semantics, Lantern's composition/locking semantics, and Bell-rate-before-Nailkeeper-interval order.

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Typed immutable loadout | `CoreBargainLoadout` accepts zero to three unique typed definitions in acquisition order. Unknown, duplicate, disabled, reordered, revision-mismatched, and cap-violating state fails closed. Optional Oath is legal, including the level-5 no-Oath path. | PASS |
| Atomic durable construction | One PostgreSQL statement snapshots selection, class/life/security state, level/current health, Oath and aggregate version, ordered active Bargains with acquiring-offer revision, inventory/equipped weapon, and both belt stacks. The server compiler validates the snapshot before constructing authority. | PASS |
| Cinder Hunger | Level-adjusted maximum health composes at `0.88` with the Oath multiplier and respects the `0.70` floor. Its `1.18` output composes through the `COM-002` attacker stage and covers primary, Grave Mark, Nailkeeper traps, normal enemies, and bosses under the global `+50%` cap. | PASS |
| Bell Debt | Ordinary primary cadence resolves `0.85` rate before Nailkeeper's `1.08` interval. Legal misses count, multibolt emissions count once, and every fifth accepted release snapshots aim/resolved behavior. The nonrecursive repeat emits at exactly nine ticks from the live origin for `50%` post-resolution damage with distinct provenance and no cadence/resource spend. | PASS |
| Bell lifecycle seams | Validated checkpoint import/export preserves reconnect/room-change state. Acquisition, purge, death, retirement, safe transfer, and primary-illegality reset/cancel seams are explicit. Fresh arenas preserve immutable choices while clearing Bell progress, pending repeats, projectiles, traps, timers, and buffered input. | PASS |
| Lantern Ash | Base potion output and named item effects compose before `1.40`, with one final rounding and unchanged 12-tick delivery/60-tick cooldown. Both durable stacks remain visible; slot zero alone is active, slot one returns a typed locked rejection, and empty slot zero never falls through. | PASS |
| Native accessible feedback | Bell repeats have a named notched silhouette and dedicated chime; Lantern's second slot shows explicit `[LOCKED]` text and has a distinct rejection cue. Reduced motion removes only the optional Bell pulse and preserves name, shape, and sound identity. | PASS |
| Restart and combinations | The real-QUIC PostgreSQL journey stages all three active Bargains and two belt stacks, compiles before shutdown, recompiles after bound-server restart, and compares the full authoritative signature. The item-bound compiler covers all 96 no-Oath/Long Vigil/Nailkeeper × Bargain-subset × Core-crossbow combinations. | PASS |

## Verification

- [Authoritative run 29255591203](https://github.com/MikeyPar/Gravebound/actions/runs/29255591203) passes formatting, warnings-denied workspace Clippy, all workspace tests, content validation, repeated deterministic traces, generated-schema drift, the mandatory PostgreSQL suite, real-QUIC restart proof, and the Windows release build.
- Local closure passes `cargo fmt --all -- --check`, warnings-denied workspace Clippy, 595 tests, content validation, two byte-identical deterministic traces, generated-schema verification, and `git diff --check`.
- Targeted mechanics coverage passes 265 `sim_core`, 61 `sim_content`, 79 native-client, and the PostgreSQL-only identity/restart tests exercised by CI.

## Delivery history

The slice was delivered in granular commits for typed loadouts, atomic persistence, optional-Oath compilation, Cinder, Bell, Lantern, fresh-arena reconstruction, PostgreSQL restart proof, exhaustive item-bound combinations, and accessible native feedback.

## Deferred scope

`GB-M03-05F` owns durable Bell checkpoint storage/wiring across reconnect and room transfer; death, retirement, and safe-transfer cleanup; crash/adversarial connected-route evidence; and lifecycle telemetry. The 50-Ash purge transaction and confirmation surface, paid Oath changes, unresolved rolled-affix combat, Core promotion, and the normal player route remain fail closed under their owning packages.
