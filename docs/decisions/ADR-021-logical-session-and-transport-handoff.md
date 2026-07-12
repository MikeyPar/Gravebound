# ADR-021 — Logical session and transport handoff

Status: Accepted

Implementation package: `GB-M02-04`

## Context

The existing QUIC connection and authoritative combat aggregate have the same practical lifetime. `TECH-015` requires the opposite: transient transport loss must preserve a vulnerable server-owned character for three seconds, while reconnect and duplicate login must attach exactly one transport to exactly the same logical state.

## Decision

1. `server_app` owns a deterministic logical-session directory keyed by an opaque authenticated owner ID. Authentication supplies that ID out of band; auth-ticket bytes never become a session key or diagnostic value.
2. A managed logical session owns one `AuthoritativeSession` and one lifecycle state. Replaceable transport IDs authorize ingress but never own gameplay state.
3. Join, reconnect, and leave use protocol Control requests and typed Control results. The wire contract advances to protocol `1.3`; exact-minor matching remains mandatory.
4. Loss and voluntary client leave both enter the same 90-tick LinkLost window. This prevents process exit from becoming an immediate safe extraction. Latest movement and held-primary input are neutralized once; the server continues ordinary simulation.
5. Tick ordering is authoritative simulation, death observation, then LinkLost deadline. Therefore a lethal outcome on the deadline tick wins over automatic Recall exactly as `TECH-015` requires.
6. Reconnect and duplicate login stage a complete replacement binding, validate owner/session/sequence first, then atomically publish it. Only after commit may the caller close the former QUIC transport.
7. Reconnect results carry server tick and monotonic-time facts for immediate time-sync recalculation. A resolved session returns only its authoritative destination: combat instance, Lantern Halls, or final death.
8. Client LinkLost state is presentation and request orchestration only. Its local deadline changes to Awaiting Resolution; it cannot infer Recall, restore health, or suppress a later death result.
9. Graceful shutdown is distinct from transport loss and crash recovery. It stops admission, emits a shutdown control result, and drains logical sessions without committing gameplay death.

## Consequences

- QUIC reconnection can replace sockets without cloning combat or inventory state.
- The M02 impairment harness can advance the lifecycle with an explicit tick clock rather than wall-clock sleeps.
- Old connections are harmless after handoff even if packets arrive late.
- The live aggregate commits Recall inventory disposition now; durable transfer, danger-entry restoration, and Lantern Halls records remain persistence work without inventing a database in M02.
