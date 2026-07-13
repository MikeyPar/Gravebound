# ADR-031 — Progression danger binding and crash restoration

Status: Accepted

Implementation package: `GB-M03-04A` / `GB-M03-03B`

## Context

The canonical GDD's `TECH-023` requires an unrecoverable danger-instance crash to restore entry XP, level, and health exactly while revoking post-entry unsecured gains; a committed death or extraction must win. `PROG-001`–`003` make XP character-owned while first-clear ownership is account-wide. The Content Production Specification applies the same restore rule to realm and Echo attempts and forbids rewards from an aborted attempt. The Development Roadmap assigns durable progression, crash participation, concurrency evidence, and restart proof to M03. The owner-approved restore contract prohibits timestamp inference because another character can legitimately earn the same account-wide first clear during an overlapping interval.

## Decision

1. Every XP result resolved while the authoritative world location is `Danger` stores that location's exact `entry_restore_point_id`. Results resolved in Character Select or a safe location store no restore identity. The caller cannot provide or override this binding.
2. Fresh awards use one serializable account → character → location → progression lock order. Replay is checked immediately after the account lock and before current character/location validation, preserving exact retry behavior after a later restore or return to Hall.
3. Entry capture stores exact level, total XP, current health, and progression version beneath the restore point. The transfer identity is the restore-point identity for the v1 composite snapshot.
4. Crash restoration locks the account and character, the exact restore root/snapshot, the authoritative location, progression, bound award receipts in reward-event order, and exact first-clear ownership. It proceeds only for a living character whose active danger location still references an active restore root.
5. Restored level, total XP, and current health equal the entry snapshot. The live progression version becomes the pre-restore live version plus one; it never moves backward to the captured version. The character's normalized level cache is updated in the same transaction.
6. Bound XP receipts are immutable evidence. Restoration marks them with the exact restore point, restore time, and post-restore progression version instead of deleting or rewriting their original award payload.
7. Retry of a revoked receipt returns the exact typed `revoked_by_crash_restore` outcome with no stale projection. A request that reuses the reward identity with different canonical material remains an idempotency conflict.
8. An account-wide boss first-clear marker is removed only when its foreign-keyed reward event is one of the exact receipts revoked by this restore. No timestamp, character-wide sweep, or boss-ID guess is legal.
9. The progression component records its restored version for transactional idempotency. If the restore root has already been consumed, the character is dead, or the active location no longer references the restore point, the provider returns `RestoreSuperseded`; it performs no progression or receipt mutation.
10. Migration corrections are forward-only. Schema 13 explicitly rejects PostgreSQL null/unknown edge cases in the revocation shape rather than rewriting a previously published migration.

## Consequences

- Support retains the original award evidence and can trace both grant and revocation to one restore point.
- Concurrent awards, crash restore, extraction, and death serialize on the account/character authority; no timestamp race can revoke another character's first clear.
- A crash can restore old XP values without violating monotonic consumer/version assumptions.
- The compiled disposable-PostgreSQL fixture covers concurrent exactly-once danger awards, exact restoration, first-clear revocation, typed replay, safe nonbinding, and committed-resolution precedence.
- Live PostgreSQL execution remains required closure evidence; compiled tests do not substitute for it.
- Inventory and Oath/Bargain providers, the complete crash coordinator, and the normal player route remain gated by their assigned M03 packages.
