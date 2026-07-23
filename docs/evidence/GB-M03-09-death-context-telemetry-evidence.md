# GB-M03-09 death-context telemetry evidence

## Authority and outcome

This evidence reads all three design authorities together:

1. `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-010`, `SIM-012`, `SOC-010`,
   `TECH-123`, and `TEL-003` require the committed death to retain exact boss, party,
   contribution, and network-health facts without making analytics a gameplay dependency.
2. `Gravebound_Content_Production_Spec_v1.md`: the Core capacity-one private route and
   `CONT-BOSS-001`/`002` define the only M03 Caldus phase and encounter identities that may be
   projected.
3. `Gravebound_Development_Roadmap_v1.md`: `GB-M03-06` and `GB-M03-09` require atomic durable
   death evidence plus versioned, privacy-safe, optional telemetry.

Commit `1f1d0fd` implements the remaining `TEL-003` source facts. Additive migration
`0078_m03_death_telemetry_context_v1.sql` stores them in the same immutable `death_events`
transaction as the terminal outcome. Historical rows remain explicitly unavailable; they are
never backfilled with invented solo, healthy-network, boss-phase, or contribution values.

## Authority chain

- Protocol 1.25 negotiates a bounded analytics-only correction diagnostic. The current private-life
  client explicitly reports correction authority as unavailable because it does not own the M02
  reconciliation reducer; it does not fabricate a zero count.
- The server derives RTT, jitter, and loss from the authenticated QUIC connection. Samples carry
  the winning transport generation and server timestamp, remain monotonic, and enter the same
  retained-frame reducer used by terminal authority.
- Reattach publishes the replacement generation's initial server-observed sample under the reducer
  lock before the first lethal-capable frame. A death cannot inherit the prior connection's sample
  or observe an unlabelled reconnect gap.
- The Caldus runtime derives phase, capacity-one party size, exact direct-damage centi-units, and
  reference health from the committed encounter frame. Non-Caldus frames cannot carry Caldus facts;
  mismatched phase, contribution, or party authority fails closed.
- Fresh death commits require an `Observed` context. Exact retries retain the original stored
  observation, while pre-schema-78 rows load as `HistoricalUnavailable` without changing their
  frozen canonical plan/request hashes.
- The committed outbox adapter projects only the immutable death row. Event schema 2 makes the
  correction count optional and serializes no account ID, socket address, transport generation,
  sample timestamp, authentication material, or other reversible network identifier.

## Local production-blocking verification

- `cargo fmt --all`: passed.
- Focused strict Clippy for `protocol`, `telemetry`, `persistence`, `server_app`, and
  `client_bevy`, including libraries, binaries, and tests: passed.
- Protocol 1.25 request/result append-only framing and older-minor rejection: passed.
- Frozen pre-schema-78 canonical plan and request hashes: passed.
- Additive, bounded, non-destructive schema-78 static contract: passed.
- Current durable-death context rejects fabricated party, phase/contribution shape, loss, and
  historical context on fresh writes: passed.
- Reattach's first committed terminal frame owns the new-generation network sample: passed.
- A real phase-one hostile hit produces the lethal Caldus frame and retains exact phase,
  capacity-one party, zero contribution, and reference health: passed.
- Planner tests accept exact same-frame Caldus/transport facts and reject missing network,
  cross-scene encounter, and route/phase mismatch: passed.
- `postgres_durable_death` schema-78 legacy-upgrade target compiles with its redacted JSON
  projection checks: passed.

## Current Next Step

Hosted run [`30048509525`](https://github.com/MikeyPar/Gravebound/actions/runs/30048509525)
must apply schema 78 and pass both the legacy upgrade/restart/replay journey and the complete fresh
durable-death graph with every stored column and schema-2 redacted projection asserted. After that,
the remaining `GB-M03-09` work is bounded outbox-lag/restart observability plus the owner-approved
destination, processor-region, access, encryption, retention, deletion, backup-expiry, and privacy
review required by ADR-039. Telemetry export remains disabled until those operational gates pass.
