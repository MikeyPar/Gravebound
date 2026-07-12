# GB-M02 runtime implementation audit

## Result

SHARED IMPLEMENTATION AND PACKAGE PASS; HUMAN EVIDENCE PENDING. The native client and local QUIC server run one authoritative combat world for up to four players. `SPEC-CONFLICT-003` and ADR-027 resolve “together” as shared combat while preserving the Content Specification's manual-Recall prohibition.

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
- The client binds prediction to `controlled_entity_id`; owner-qualified projectile provenance prevents remote/local confirmation collisions.
- Ordinary server tests contain no quarantined fixtures. The only ignored test is the explicit release-profile two-hour soak, which passes when invoked by the gate command.
- Networking CI (68 active tests), strict workspace Clippy, full workspace CI (390 active tests), content validation, deterministic traces, impairment, abuse, retirement, teardown, and release soak pass.
- Optimized Windows executables were packaged with clean-destination enforcement, exact hashes, a shared runbook, and an all-client launcher. One server and four packaged clients remained alive concurrently in process smoke.

## Remaining gate

Run the checked-in four-human shared-combat runbook. M03 remains blocked until that human row is accepted.
