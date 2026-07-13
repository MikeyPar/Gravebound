# GB-M03-12 completion audit

## Result

PASS. Gravebound now has one account-owned, nontradeable, 99,999-cap Ash wallet with canonical source/sink authority, replay-first serializable mutation handling, immutable accepted-mutation evidence, and restart/concurrency proof. Player-facing earn/spend routes remain disabled until their owning packages are complete.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | `ECO-003`-`005`, `LOOT-031`, and `LOOT-034` are represented by the exact cap, nonnegative balance, reason/source/content/idempotency metadata, before/delta/after evidence, and exact Ash source/sink amounts. |
| Content Production Specification | `CONT-HUB-002` exact 40-Ash later Oath change and 50-Ash Bargain purge are typed contracts; neither unfinished Hall mutation route is enabled. |
| Development Roadmap | `GB-M03-12` delivers the minimal idempotent earn/spend wallet with duplicate, stale-version, concurrency, insufficient-funds, cap, ledger, and restart tests. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Strict durable schema | Forward migrations add the account wallet, replay results, expected-version binding, and immutable accepted ledger with database-enforced cap, balance, version, arithmetic, and result-shape constraints. | PASS |
| Canonical authority | The server requires nonzero identity, expected wallet version, exact kind/reason/amount/source, immutable Core content revision, issuance time, and canonical payload hash. Unknown or drifted material fails closed. | PASS |
| Exact economy values | Salvage, events, Core bosses, Bargain replacement, Unique fallback, achievements, Hall contract, Oath/Bargain sinks, Forge, Temper, and Reforge values are pinned exhaustively. | PASS |
| Replay-first serializability | Replay is checked before current wallet validation. Identical retries return the exact stored result; conflicting reuse is rejected; bounded PostgreSQL serialization retry preserves one committed outcome. | PASS |
| Conservation and bounds | Accepted mutations write exactly one immutable before/delta/after event. Insufficient spends, cap overflow, stale versions, malformed storage, and invalid achievements do not change balance or ledger. | PASS |
| Real PostgreSQL behavior | The disposable fixture proves earn, exact replay, conflicting reuse, concurrent same-version resolution, spend, insufficient balance, cap overflow, ledger conservation, and process-restart durability. | PASS |
| Safe staging | Storage exists only in the explicitly wipeable namespace. No normal protocol, reward, salvage, crafting, contract, Oath-change, Bargain-purge, or production route is enabled by this package. | PASS |

## Verification

- [CI run 29236047011](https://github.com/MikeyPar/Gravebound/actions/runs/29236047011): full hosted gates and the mandatory PostgreSQL Ash fixture pass.
- PostgreSQL Ash audit: 1 comprehensive test passed against PostgreSQL 17.10 with all replay/concurrency/restart assertions.
- Workspace tests and warnings-denied Clippy pass; local destructive PostgreSQL execution was unavailable and was not claimed as evidence.
- ADR-034 records the replay-first wallet boundary, exact version policy, and fail-closed staging decision.

## Granular delivery commits

- `0f2c7fd` - replay-first Ash wallet schema and repository.
- `505aefe` - expected-version binding.
- `0d04352` - canonical server source/sink authority.
- `c879b3c` - destructive PostgreSQL transaction/restart audit.
- `ecc3eef`, `9ceef78` - mandatory ordered local/CI execution.
- `e19599c` - bounded serializable race retry.
- `500db5a` - task and ADR architecture record.

## Deferred parent scope

The normal earn/spend sources and Hall stations remain owned by reward, salvage, crafting, contracts, paid Oath changes, and Bargain purge packages. Trading remains prohibited. `GB-M03-05D` is the next approved slice.
