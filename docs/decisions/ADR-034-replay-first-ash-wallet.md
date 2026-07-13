# ADR-034 - Replay-first Ash wallet authority

Status: Accepted

Implementation package: `GB-M03-12` / `GB-M03-02`

## Context

The canonical GDD defines Ash Shards as a nontradeable account currency capped at 99,999. Every mutation must carry reason, source, content revision, idempotency identity, and auditable before/delta/after values; insufficient spends and cap overflow must reject before their gameplay source resolves. The Content Production Specification pins the Hall's 40-Ash Oath change and 50-Ash Bargain purge. The Development Roadmap requires a minimal idempotent earn/spend wallet in M03, while the normal route remains closed until the complete private-life gate passes.

Network retry, process restart, and concurrent gameplay sources make a read-then-write balance update unsafe. An accepted result can be lost in transit, the same mutation ID can be reused with different content, or two individually legal commands can race from the same observed balance.

## Decision

1. Ash is one durable account-owned wallet in the wipeable Core namespace. Its balance is bounded to 0-99,999 and its positive wallet version advances exactly once per accepted mutation.
2. A command contains mutation ID, expected wallet version, canonical payload hash, issuance time, earn/spend kind, typed reason, exact amount, source ID, and immutable Core development content revision.
3. The server owns the complete reason contract and exact GDD values. Callers cannot submit free-form reasons or prices; achievement awards are the only variable amount and are bounded to 1-100.
4. The repository locks the account before checking replay and locks the wallet before resolution. A matching retry returns the stored outcome even when the wallet has since changed. Reusing a mutation ID with a different payload hash is an idempotency conflict.
5. Expected-version mismatch, insufficient balance, and cap overflow are durable replayable rejections. They do not alter the wallet version and do not append a currency ledger event.
6. An accepted mutation updates the wallet, stores its result, and appends one immutable ledger event in the same serializable PostgreSQL transaction. The ledger records currency, reason, source, content revision, before balance, signed delta, after balance, and resulting wallet version.
7. The initial wallet is projected lazily as balance 0/version 1. No transfer API exists, and neither arbitrary SQL balance correction nor caller-selected delta is part of the authority surface.
8. The service is restricted to authenticated wipeable-test accounts and is not bound to a protocol or Hall interaction. Source-specific gameplay integration occurs only in the packages that own source resolution and rollback.
9. The destructive PostgreSQL fixture proves replay, conflict, same-version concurrency, spend, insufficient balance, cap rejection, conservation, and restart durability. Local and CI PostgreSQL runners must invoke it explicitly because ignored tests are not acceptance evidence by compilation alone.

## Consequences

- A lost response can be retried without duplicating Ash, and a stale or conflicting command cannot spend or mint against an unintended wallet state.
- Support can reconstruct every accepted balance transition from an immutable, content-versioned ledger while retaining rejected command receipts for deterministic replay.
- Cap and insufficient-funds rejection occur inside the same authority boundary that would commit the source integration.
- Wallet mechanics are reusable by salvage, rewards, contracts, crafting, Oath changes, and Bargain purge without allowing those systems to redefine economy values.
- Live disposable-PostgreSQL execution remains a closure artifact. The normal route and Hall economy interactions remain disabled until their owning M03 packages pass.
