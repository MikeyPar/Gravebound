# GB-M03-01B completion audit

## Result

PASS for the ephemeral protocol and server-authority slice. This does not close `GB-M03-01`; native UI and combined package evidence remain separate.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | Two slots follow `LOOT-050`; safe character projections follow `UI-007`/`UI-008` within approved deferrals; `TECH-021` identity, version, hash, issue time, retry, and safe-state mismatch rules are authoritative. |
| Content Production Specification | Creation allows only `class.grave_arbalist`; runtime advertises unpromoted `core-dev`; no item, oath, appearance entitlement, or promoted Core record enters the aggregate. |
| Development Roadmap | Supplies only the wipeable identity/creation/select authority in `GB-M03-01`; PostgreSQL remains deferred to `GB-M03-02`. |

## Requirement evidence

| Requirement | Evidence |
|---|---|
| Append-only protocol | Existing kind bytes 1–8 remain fixed; bootstrap/mutation append 9/10; reliable result variants append after all M02 variants. |
| M02 byte boundary | `encode_m02_compatibility_frame` retains canonical hash `643b0c2d1746c2e697e2c5cb3b4fc0e352019903a951004326e808e00b5cd7ec`. |
| Bounds | Wire text uses bounded borrowed-string deserialization; roster deserialization caps allocation at two; reliable frame limit remains 64 KiB. |
| Authentication/ownership | Runtime derives redacted opaque account identity from BLAKE3(ticket); client supplies no account ID; global owner lookup rejects a forged character as `character_not_owned`. |
| Exact aggregate | Version starts at 1; slot capacity is 2; creation yields deterministic ordinal, Grave Arbalist, level 1, no oath, living, safe character select, with no name/appearance/item/last-played fields. |
| Optimistic/idempotent mutation | One writer lock protects aggregate and a 128-result non-evicting ledger; duplicate retry returns original result, changed payload conflicts, stale version returns current snapshot. |
| Wipe boundary | Same ticket reconnects to the same aggregate in-process; a newly bound server returns an empty version-1 account. |
| No premature play | `BoundCoreIdentityServer` has no `InstanceScheduler`; its report asserts zero combat sessions and persistence disabled. |
| Privacy seam | Account `Debug` is redacted; identity events carry only kind/error/ordinal; credential/name/platform fields are absent. |

## Automated verification

- `cargo test -p protocol`: 19 passed after the appended-kind and pre-allocation bound fixtures were added.
- `cargo test -p server_app identity --lib`: 7 identity-domain tests passed; the filter also ran one existing instance identity test and the matching Core runtime test (9 total).
- `cargo test -p server_app core_identity_real_quic_reconnects_and_server_restart_wipes --lib`: 1 passed.
- `cargo test -p server_app --lib`: all 43 server library tests passed, including the M02 real-QUIC regressions.
- Strict Clippy passed for the `protocol`, `bot_client`, and `server_app` production libraries with warnings denied.
- Real-QUIC test covers handshake, empty bootstrap, create, reconnect restore, zero combat admission, restart, and empty bootstrap after wipe.

## Remaining parent-package gates

Native authentication/select/create presentation, keyboard/focus/accessibility review, both required resolutions, screenshots/hashes, optimized runtime review, full workspace gates, and combined `GB-M03-01` audit remain open. No human or visual result is inferred from this backend pass.
