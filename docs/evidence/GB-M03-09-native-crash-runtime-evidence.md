# GB-M03-09 native crash runtime evidence

## Authority and scope

This bounded runtime slice follows all three design authorities:

1. `Gravebound_Production_GDD_v1_Canonical.md` (`TECH-123`, `TEL-001`-`005`) requires privacy-safe client/server crash facts without making telemetry a gameplay dependency.
2. `Gravebound_Content_Production_Spec_v1.md` (`CONT-002`) keeps exact Core content authority on the durable logical session; crash reporting does not invent content or gameplay facts.
3. `Gravebound_Development_Roadmap_v1.md` (`ADR-005`, `GB-M03-09`) requires versioned crash reporting, disabled/offline behavior, and restart-safe committed sourcing.

## Implemented claim

- Protocol generation 1.24 appends message kind 27 and feature flag `telemetry.native-crash.v1`. Existing discriminants remain unchanged. The client submits only after the server advertises the capability.
- The production `CorePrivateLife` client installs a panic hook and retains at most one next-launch marker in `%LOCALAPPDATA%/Gravebound/Telemetry`. Its fixed schema contains only crash UUID, typed kind, a one-way location signature, uptime, and occurrence time. It has no message, stack, raw path, account, ticket, IP, socket, arbitrary property, or network field.
- A marker is deleted only after an exact durable `Accepted` acknowledgement. Disabled, unavailable, conflict, lost-response, and full local reliable-queue outcomes retain it for a later launch and never affect authentication, bootstrap, or gameplay.
- The server derives account and logical-session authority from the authenticated generation-bound telemetry lease. A stale or missing lease fails nonblocking. The client cannot author account, character, session, source, or reporter.
- Next-launch recovery preserves the exact orphaned predecessor session as the crash origin, closes it, and opens a separate replacement session. The crash timestamp is therefore checked against and published with the crashed session rather than relabeled onto the healthy replacement session.
- The current connection boundary has no safe selected-character projection. `character_id` is consequently server-authored `None`; it is never guessed or accepted from the client. A later enrichment may bind a character only from an authenticated durable selection snapshot.
- Persistence uses the existing schema-70 `crash_outbox_events_v1` transaction. Exact replay returns the stored fact, changed payload conflicts, and publication still flows only from committed outbox rows.

## Focused production-blocking verification

- Protocol validation rejects zero IDs, zero signatures, zero occurrence time, and pre-1.24 encoding; append-only message kind 27 round-trips.
- Native marker tests prove the six-field redacted schema, oldest-pending retention, and retention after nonaccepted outcomes.
- Coordinator tests prove exact replay, changed-payload conflict, stale-generation and missing-lease rejection, and predecessor-session attribution after process restart.
- Focused commands:
  - `cargo test --locked -p protocol native_crash -- --nocapture`
  - `cargo test --locked -p protocol protocol_1_24_appends_bounded_native_crash_frames -- --nocapture`
  - `cargo test --locked -p client_bevy native_crash_report --lib -- --nocapture`
  - `cargo test --locked -p server_app core_private_telemetry_session::tests --lib -- --nocapture`

Hosted PostgreSQL and real-QUIC crash-marker acknowledgement remain part of the final GB-M03-09 hosted proof; this evidence does not claim those external gates.
