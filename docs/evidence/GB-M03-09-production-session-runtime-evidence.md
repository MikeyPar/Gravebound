# GB-M03-09 production logical-session runtime evidence

**Status:** SOURCE COMPLETE; HOSTED POSTGRESQL INTEGRATION PENDING

## Three design authorities

1. `Gravebound_Production_GDD_v1_Canonical.md` — `TECH-123` and `TEL-001`–`005` require privacy-safe pseudonymous context, truthful session start/end/disconnect/reconnect facts, explicit platform/region/environment attribution, and gameplay independence.
2. `Gravebound_Content_Production_Spec_v1.md` — the exact Core content revision and stable IDs remain the only content attribution; telemetry cannot invent or localize gameplay state.
3. `Gravebound_Development_Roadmap_v1.md` — `GB-M03-09` requires onboarding/session telemetry, crash redaction, bounded batching, and disabled/offline operation.

## Runtime contract

The persistent production-root server now owns an optional schema-70 logical-session coordinator:

- It begins or recovers telemetry only after the handshake accepts the client and before account/bootstrap transactions can project onboarding facts.
- It receives only the derived account ID and the accepted `ClientHello` platform; authentication ticket bytes never enter the coordinator.
- `GRAVEBOUND_TELEMETRY_ENVIRONMENT` is an explicit opt-in. Missing or invalid configuration disables the coordinator without affecting gameplay.
- Local/test operation defaults to the canonical local-playtest region. Staging/production additionally requires a valid `GRAVEBOUND_TELEMETRY_REGION_ID`; missing, unstable, or secret-looking region text disables telemetry rather than mislabeling data.
- The private cohort tag is explicit; testers are never implicitly labeled staff.
- A disconnected durable head reconnects in place. A process-orphaned Started/Reconnected head closes as `TransportClosed` and receives a new session. No invalid transition is treated as success.
- Generation leases make stale connection retirement harmless. A failed reconnect observation cannot claim a connected telemetry lease.
- Exact native client shutdown ends as `CleanExit`; ordinary transport loss records `Disconnected`; graceful server shutdown ends tracked roots as `ServerShutdown`.
- Every persistence operation is bounded to one second, errors are logged without raw identity material, and no telemetry failure can reject handshake, bootstrap, route admission, or gameplay.

## Production-blocking local checks

- Focused coordinator tests cover handoff, reconnect, orphan replacement, clean exit, failed reconnect, repository failure, UUIDv7 identity, platform attribution, cohort attribution, and region validation.
- Strict `server_app` library/test Clippy: PASS.
- Strict persistence library Clippy for durable-head inspection: PASS.
- Formatting and scoped diff validation: PASS.

## Claim boundary and Current Next Step

This slice wires logical sessions only. It does not claim crash collection, item-ledger loot/session binding, schema-70 worker publication, destination privacy approval, or hosted PostgreSQL/restart evidence. The Current Next Step is to enable `test` attribution in the disposable production-root journey, assert the committed session/onboarding/first-combat/end sequence around real QUIC, then add redacted crash collection and immutable loot origin binding.
