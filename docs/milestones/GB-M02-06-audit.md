# GB-M02-06 completion audit

## Result

PASS. Encoded hostile, malformed, stale, replayed, and flood traffic cannot create movement, attacks, hits, rewards, item grants, or mutation outcomes outside the same bounded server-authoritative rules used by an ordinary client. Protocol `1.4` exposes typed nonmutating results where a valid request deserves a response and rejects malformed authority claims at the codec boundary.

## Authority review

| Authority | Evidence |
|---|---|
| Canonical GDD | `SIM-004`, `SIM-010`-`SIM-012`, `TECH-011`-`TECH-015`, and `TECH-120`-`TECH-125` require server-owned movement, cooldown, collision, reward, sequencing, idempotency, typed rejection, bounded diagnostics, and separation between poor networks and suspicious traffic. |
| Content Production Specification | All speed, cooldown, collision, reach, reward, and inventory facts continue to come from immutable `fp.1.0.0`; the ingress layer introduces no alternate gameplay values. |
| Development Roadmap | `GB-M02-06` names teleport, speed, fire-rate, forged-hit, duplicate-pickup, stale/replayed-input, and mutation rejection. Each category crosses the canonical encoded protocol or is proven structurally unrepresentable on that wire. |

## Ingress and authority evidence

| Requirement | Evidence |
|---|---|
| Intent-only wire | Input carries bounded direction, aim, held-fire intent, and sequencing only. It cannot encode position, velocity, collision, hit, damage, health, death, eligibility, drop, reward, or item-grant authority. Forged header/payload kinds and malformed vectors fail canonical codec validation before gameplay ingress. |
| Datagram disorder | Stale or duplicate Input datagrams are benign `Superseded` outcomes and counted separately from suspicious anomalies. A held-primary source sequence below the observed maximum is a typed rejection; equal sequence remains legal held fire. |
| Ability channel | Ability sequence fields on Input must be zero. Reliable Action is the sole press seam; stale actions return typed `StaleSequence`, and more than one press per ability per authority tick returns `RateLimited` without replacing the first press. |
| Fire-rate authority | Encoded tests rotate primary identities every tick and still cannot exceed the ordinary server-owned cooldown/projectile cadence. Projectile collision and damage remain simulation results, never client claims. |
| Mutation idempotency | A mutation ID is bound to its complete request. Exact retry returns the cached result; the same ID with a different payload returns `IdempotencyConflict`; a second pickup identity returns `AlreadyResolved`. Rejections do not mutate inventory, pickup state, or accepted sequencing. |
| Resource bounds | At most eight new mutations are processed per authority tick, at most 1024 mutation identities are cached per logical session, and at most 64 recent anomaly records are retained. Counters saturate and the reviewed evidence score never bans or changes gameplay. |
| Determinism | Replaying the same encoded abuse script produces identical typed outcomes, diagnostics, snapshots, and terminal authority state. |

## Defect found and corrected

The speed-abuse fixture exposed that the ordinary authority path advanced player movement once directly and then a second time inside combat. `AuthoritativeArena` now receives the exact `MovementStep` produced by the single combat-owned simulation step. Slipstep supplies the same typed movement outcome without creating another integration path. The encoded flood test pins the configured per-tick displacement ceiling and prevents this regression.

## Protocol history

Protocol minor `1.4` requires zero legacy ability sequences on Input and adds typed `IdempotencyConflict` and `RateLimited` mutation results. Exact minor matching remains mandatory, no compatibility adapter is claimed, and the canonical codec fixture and BLAKE3 golden were intentionally repinned.

## Verification

| Gate | Result |
|---|---|
| Encoded abuse suite | PASS - malformed/forged intent, movement and primary-sequence flood, fire-cadence identity churn, and deterministic evidence replay |
| Networking CI | PASS - 50 protocol/harness/server/bot tests, strict warnings-denied Clippy, real QUIC tests, and both doctor commands |
| Full workspace CI | PASS - 359 tests, format, strict all-target warnings-denied Clippy, content validation, and two byte-identical deterministic traces |
| Worktree diff check | PASS before commit |

## Deferred without waiver

- Full real-protocol journey bot and the two-hour sixteen-bot soak: `GB-M02-07` and the M02 exit gate.
- Realm/arena scheduler lifecycle, server tick percentiles, and clean multi-instance teardown were completed by `GB-M02-08`; this audit retains their original deferral as historical scope evidence.
- Edge DDoS/connection quotas, durable security evidence, platform anti-tamper, staff review, and sanctions: M06.
- Four-human impaired playability approval remains a conjunctive full M02 exit gate and is not replaced by automated abuse coverage.

## Handoff

`GB-M02-07` subsequently drove movement, aim, combat, pickup, death, manual Recall, and reconnect only through protocol `1.4` and the managed-session boundary. Its policy depends on protocol-visible state only and does not call simulation, inventory, lifecycle, or authority internals to advance its journey. `GB-M02-08` now owns instance scheduling and the complete population/performance gate.
