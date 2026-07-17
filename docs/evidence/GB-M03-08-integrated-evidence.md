# GB-M03-08 integrated extraction, Recall, and Resolution Hold evidence

**Status:** PASS for source commit `2535e8c3ac5a3e51bf4b1e45323ad14951b9f430` on hosted CI [`29554811453`](https://github.com/MikeyPar/Gravebound/actions/runs/29554811453).

## Three design authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-010`, `DTH-011`, `LOOT-002`, `LOOT-033`, `LOOT-050`, `LOOT-060`, `TECH-015`, and `TECH-021`-`023` require exact 400 ms Recall, lethal-first terminal ordering, deterministic extraction custody, crash-safe replay, and no accepted-item loss.
- `Gravebound_Content_Production_Spec_v1.md`: `CONT-HUB-001`/`002`, the Core danger-area contracts, and `CONT-VALID-001` require Recall availability in every Core danger space, committed terminal state before Hall arrival, exact Vault/Overflow behavior, and material conservation.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-08`, and the M03 restart/idempotency/no-duplication gates assign extraction, Emergency Recall, terminal recovery, and their closed production-route boundary to this package.

Accepted [`SPEC-CONFLICT-029`](../spec-conflicts/SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md) fixes durable terminal identity, server-owned placement/destruction, Overflow, ResolutionHold, reward ordering, and shared-writer rules. Accepted [`SPEC-CONFLICT-030`](../spec-conflicts/SPEC-CONFLICT-030-m03-resolution-hold-recovery.md) fixes whole-stack recovery, server-planned destinations, retained deadlines, explicit destruction, exact replay identity, and final-unlock versioning.

## Hosted run identity

- Source commit: `2535e8c3ac5a3e51bf4b1e45323ad14951b9f430`.
- Authoritative cumulative run: [`29554811453`](https://github.com/MikeyPar/Gravebound/actions/runs/29554811453), PASS.
- Windows release build: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS.
- `cargo test --workspace --locked` plus content, schema, and deterministic-trace validation: PASS.
- Mandatory PostgreSQL migrations and transactions, including extraction, Recall, death competition, and authenticated real-QUIC routes: PASS on PostgreSQL 17.
- Local all-target verification also passed; `cargo test --workspace --all-targets -- --list` enumerated 1,367 tests, with destructive PostgreSQL cases compiled locally and executed by the hosted gate.

## Durable terminal authority

Migrations `0055`-`0059` append extraction/Recall custody, immutable terminal results, exact replay identity, Overflow, and ResolutionHold without rewriting prior history. The production writers provide:

- one server-planned, serializable extraction transaction that secures equipment/Belt, credits pouch materials, places accepted pending items through CharacterSafe, Vault, Overflow, then ResolutionHold, and stores the complete placement map before Hall transfer;
- one shared explicit/LinkLost Recall writer that preserves equipped/Belt state, destroys all remaining pending custody and pouch state in canonical order, retains auditable provenance, and commits before Hall transfer;
- replay-before-current-state behavior, canonical request/result hashes, selected-character/account/content/version binding, typed stale/foreign/corrupt/unavailable outcomes, and durable outbox/audit publication;
- a blocking ResolutionHold reader plus replay-first Move/DestroyConfirmed writer using only server-authored legal destinations, whole-stack identity, retained Overflow deadlines, and an authoritative final empty refresh;
- normal endpoint capability omission and rejection until the integrated private route earns admission.

The hosted PostgreSQL suites cover exact replay, altered payload, response loss, restart, concurrent writers, stale/foreign authority, full storage, rollback, serialization retry, corrupt stored state, database outage behavior, and material/item conservation. Exact retries return the stored result; no accepted extraction item is deleted and no Recall loss is applied twice.

## Recall coordination and transport evidence

The server owns the exact 12-tick explicit channel and 90-tick LinkLost deadline. One five-producer terminal coordinator seals lethal death, extraction, explicit Recall, LinkLost recovery, and verified fault restoration for the same authoritative tick; committed lethal death wins every conflict.

Commits `b7bba81`, `a0ac058`, `aeb8971`, `264de41`, and `2535e8c` close the production runtime boundary:

- monotonic transport generations bind one authenticated connection to one selected-character actor; duplicate handoff retires the old authority and stale detach cannot begin LinkLost;
- one bounded actor inbox serializes Start/Cancel, samples the authoritative tick after dequeue, and fails stale authority closed;
- committed completion publications use the same ordered writer sequence as request responses and survive abandoned delivery for exact reconnect replay;
- planned shutdown closes inboxes, joins actor/connection/delivery tasks, and reports queued, undelivered, abandoned, and remaining work explicitly.

The ignored hosted matrix `postgres_quic_link_lost_terminal_matrix_is_lethal_first_and_residue_free` proves two real-QUIC/PostgreSQL branches:

| Branch | Required result | Hosted result |
|---|---|---|
| LinkLost recovery | No result at tick 89; PostgreSQL `DisconnectRecovery` at tick 90; server push after reconnect; byte-identical recovery after pool/process restart. | PASS |
| Same-tick lethal competition | Duplicate handoff and stale detach rejected; real durable lethal candidate wins at exact tick 90; Recall result/outbox remain untouched. | PASS |

Both branches finish with zero actor, transport, delivery-task, transaction, lock, and death-database residue. Earlier real-QUIC cases also prove response abandonment followed by exact reconnect replay and normal-route capability rejection.

## Native Resolution Hold evidence

The accepted [24-frame optimized manifest](GB-M03-08-hold-visual-manifest.md) covers mixed destinations, storage full, destruction confirmation, mutation pending, final clear, and recoverable error in standard and reduced-effects modes at 1280x720 and 1920x1080. Every frame records exact build/content identity and SHA-256 integrity.

The native blocking surface consumes negotiated server authority and the compiled Core item catalog. It preserves exact quantity and durable UID, shows one-based server destinations and Overflow deadlines, skips disabled actions, defaults permanent destruction to Cancel, retains exact retry identity, and exposes no route-to-play escape before the final authoritative refresh.

## Scope boundary

This evidence closes `GB-M03-08`: successful extraction, explicit Emergency Recall, automatic LinkLost Recall, terminal precedence, Overflow/ResolutionHold recovery, exact replay/restart, cleanup, and native Hold presentation. It does not enable successor creation, normal Character Select-to-danger admission, parent `GB-M03-03`, telemetry, support tooling, hosting/platform work, the final 25 full-loop journeys, or the human private-cohort gates.
