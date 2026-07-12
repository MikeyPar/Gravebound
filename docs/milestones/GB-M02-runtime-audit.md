# GB-M02 runtime implementation audit

## Result

PASS. The native client and local QUIC server run one authoritative combat world for up to four players. `SPEC-CONFLICT-003` and ADR-027 resolve “together” as shared combat while preserving the Content Specification's manual-Recall prohibition. Automated runtime/package evidence is measured; the human row is closed separately by the explicitly labeled owner assumption.

## Three-authority review

| Authority | Applied contract |
|---|---|
| Canonical GDD | Modular-monolith ownership, exact channels/cadence, client movement prediction, server combat authority, reconnect, bounded state, and human gate evidence remain intact. |
| Content Production Specification | Both executables validate immutable `fp.1.0.0`; manual Recall is typed unavailable; no Core/M03 content or persistence is enabled. |
| Development Roadmap | M02 supplies native client, authoritative server, bots, shared four-player combat, performance evidence, and a four-human gate without advancing into M03. |

## Implemented evidence

- Each `HostedInstance` owns one `SharedAuthoritativeArena` with one to four stable player bindings, shared enemies/hostiles/lanes, and player-local inventory, pickups, health, death, and automatic Recall.
- Real QUIC proves four credentials receive all four players and identical enemy facts while independent inputs move east/west/north/south.
- Shared response clocks cover snapshots, reconnect, leave, actions, mutations, and exact LinkLost deadlines.
- Individually launched clients share the authored eight-second participant-lock window; a fresh process using the same owner credential reattaches to the same entity and world when it returns inside the exact three-second LinkLost deadline.
- Personal reward copies remain recipient-local and independently collectible; one player's pickup cannot remove or consume another player's copy.
- Enemy movement validates its integer-committed hurtbox against arena solids, including a 10,000-tick pillar-contact regression.
- The client binds prediction to `controlled_entity_id`; owner-qualified projectile provenance prevents remote/local confirmation collisions.
- Ordinary server tests contain no quarantined fixtures. The only ignored test is the explicit release-profile two-hour soak, which passes when invoked by the gate command.
- Networking CI (71 active tests), strict workspace Clippy, full workspace CI (399 active tests), content validation, deterministic traces, impairment, abuse, retirement, teardown, and release soak pass.
- Optimized Windows executables were packaged with clean-destination enforcement, exact hashes, a shared runbook, and an all-client launcher. The actual packaged server/all-client `.cmd` launchers produced exactly one server and four concurrently live clients.

## Gate disposition

[`GB-M02-gate-audit.md`](GB-M02-gate-audit.md) records the prior overall M02 PASS and links the owner-assumed human record. Automated correction gates pass; the Current Next Step is the focused owner retest of participant formation, personal pickups, reconnect, and enemy-solid safety before `GB-M03-01` begins.
