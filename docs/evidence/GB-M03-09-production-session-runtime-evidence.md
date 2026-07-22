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

## Integrated production-root gate

The ignored hosted journey now opts into `environment=test` and `region=local-playtest` for its command only. Around the actual real-QUIC handshake, account creation, character creation, Hall traversal, and committed microrealm admission, it requires exactly five unpublished schema-70 facts with one immutable account/session context: SessionStarted, AccountCreated, CharacterCreated, CharacterEnteredCombat, and CleanExit. It asserts Windows attribution from `ClientHello`, UUIDv7 session identity, exact Core build/content, `cohort.private` only, no crash event, no remaining open session, and post-reset cleanup.

## Claim boundary and Current Next Step

Hosted run [`29900131501`](https://github.com/MikeyPar/Gravebound/actions/runs/29900131501) at exact source `fbb0c01` passed the schema-70 PostgreSQL source test, including the committed session/onboarding rows. The combined production-root journey then stopped before transport admission on a fail-closed bootstrap-holder count mismatch; the required foundation/terminal-reconciler/world-flow-coordinator count is corrected locally and awaits hosted rerun. Protocol 1.24 next-launch panic collection and the committed-domain export adapter are also implemented locally under their focused evidence. This slice still does not claim item-ledger loot/session binding, schema-70 worker operation, destination privacy approval, or a passing integrated production-route result. The Current Next Step is the corrected hosted B1/session rerun, immutable loot origin binding, and disabled-by-default worker integration.
