# GB-M02-GATE implementation audit

## Result

IMPLEMENTATION PASS; HUMAN/SPEC PENDING. The previously test-only M02 network stack now runs as one local authoritative QUIC server and a distinct native Bevy network client. Automated real-QUIC one-client and four-client routing tests pass. Overall M02 does not pass until the runbook is executed by four humans and `SPEC-CONFLICT-003` is resolved.

## Three-authority review

| Authority | Applied contract |
|---|---|
| Canonical GDD | `TECH-001`–`TECH-006` preserve modular-monolith ownership and runtime boundaries; `TECH-010`–`TECH-015` define exact handshake, channel, cadence, prediction, and reconnect rules; `TECH-080` and `QA-008` require real human evidence. |
| Content Production Specification | Both executables validate immutable `fp.1.0.0`; no Core/M03 content or persistence is enabled. The client shows the required nonpersistent and Recall-unavailable state. |
| Development Roadmap | M02 gets a native client, authoritative server, headless bot, and four-human gate without advancing into M03. The ambiguous word “together” remains visible rather than being weakened to concurrent isolated sessions. |

## Server evidence

- `server_app serve` binds QUIC, writes a per-launch certificate, requires exact build/protocol/content hash, hashes local tickets without logging raw bytes, and maps stable ephemeral owners across reconnect.
- One wall-clock loop owns 30 Hz scheduler ticks, skips missed bursts, dispatches owner-specific 15 Hz snapshots, retains terminal reconnect tombstones while routed, retires them after route release, and drains to zero residue.
- Connection workers handle datagram input and reliable control/action/mutation streams. Replacement closes the prior transport only after the new route commits.
- Malformed or stale traffic fails closed through the existing codec, session, and instance owners.

## Client evidence

- The executable keeps the existing default/`local-lab` mode and adds explicit `network` mode.
- Network mode installs no LocalLab combat authority plugins. A dedicated Tokio thread owns Quinn; Bevy owns input, movement prediction, reconciliation, rendering, lifecycle presentation, and HUD only.
- Input is a coalesced latest-state watch. Snapshots use a fixed 16-chunk rolling queue retaining newest state. Reliable commands/events use fixed 64-entry boundaries and fail visibly on saturation.
- Certificate trust is explicit. No insecure verifier or production identity claim exists.
- HUD exposes authoritative health/death/completion, connection/reconnect state, corrections, nonpersistent scope, and Recall-unavailable copy.

## Automated verification

| Gate | Evidence |
|---|---|
| One-client runnable server | Real QUIC handshake -> Join -> input -> acknowledged snapshot -> graceful shutdown -> zero residue. |
| Four-client routing | Four distinct credentials are active concurrently; east/west/north/south inputs produce four corresponding owner-specific snapshot positions; admitted sessions=4; teardown residue=0. |
| Bounded client transport | A 32-snapshot pressure test retains exactly newest sequences 17–32 in the 16-entry queue. |
| Prediction/lifecycle | Existing correction, interpolation, death-finality, LinkLost, and server-result-only routing tests remain green. |
| Packaging | `tools/dev.cmd m02-package` builds both Windows release executables and stages content, four launchers, and the runbook. Exact sizes/hashes and gate counts are recorded in [`GB-M02-runtime-package.json`](../evidence/GB-M02-runtime-package.json). |
| Networking CI | PASS — 69 tests, one explicit long soak ignored in ordinary CI, strict warnings-denied Clippy, real QUIC, and both doctors. |
| Full workspace CI | PASS — 381 tests, one explicit long soak ignored, content validation, and two byte-identical deterministic traces. |
| Native process smoke | Packaged server and client processes launched; Windows registered `Gravebound - M02 Network Playtest`. Window capture was unavailable with `0x80004002`, so no screenshot is claimed. |
| Windows release | PASS — optimized `server_app.exe` and `client_bevy.exe` built and staged. |

## Honest limitation

Each `ManagedSession` still owns an independent `AuthoritativeSession`; clients do not see one another or share enemies. The implementation proves simultaneous native network authority and route isolation. It does not prove shared party combat. That distinction and the First Playable manual-Recall contradiction are recorded in `SPEC-CONFLICT-003`.

## Remaining gate

Run four human clients using the checked-in runbook and complete the session record. Resolve `SPEC-CONFLICT-003` before changing the exit row to PASS or beginning M03.
