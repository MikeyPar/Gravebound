# SPEC-CONFLICT-025 - M03 native transition and reconnect UX contract

**Status:** Approved under the owner's standing authorization on 2026-07-13

**Raised:** 2026-07-13

**Blocks:** `GB-M03-03F` implementation and parent `GB-M03-03` presentation closure

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The three authorities require native `DungeonLoading`, transition `loading/success/recoverable_error/fatal_error` states, exact `LinkLost`/reconnect authority, adverse-network journeys, visual review, and performance evidence. Approved `SPEC-CONFLICT-006` assigns those responsibilities to `03F` while keeping normal route admission closed.

The authorities do not define the exact Core transition projection, localized copy, retry mapping, reconnect-result presentation, disposable journey boundary, capture matrix, or how M03 performance evidence may be reported without falsely claiming the reference hardware. Implementing those details ad hoc would create undocumented runtime defaults.

## Approved resolution

### 1. One typed transition projection

Add a renderer-independent native transition view model with these closed phases:

```text
safe_origin
requesting_transfer
loading_content
awaiting_authoritative_state
ready
recoverable_error
fatal_error
link_lost
reconnecting
resolved_to_hall
resolved_to_character_select
```

The transfer mutation/result, transport event, session-control result, durable location snapshot, and compiled scene readiness are the only inputs. The client may animate presentation time but cannot advance authority, invent a destination, predict extraction/death finality, or display `ready` before both authoritative state and local content are ready. No fabricated percentage is shown.

### 2. Exact retry and safety behavior

- `recoverable_error` preserves and visibly names the prior safe origin. Retry reuses the same canonical mutation identity and payload unless the server explicitly returned a fresh-request requirement.
- `fatal_error` is used only for nonretryable handshake/protocol/content/authentication rejection or invalid authoritative state. It offers no mutation retry.
- Transport loss enters `link_lost` immediately. The displayed three-second countdown is advisory; only the server's 90-tick `TECH-015` result resolves the character.
- Reconnect before terminal resolution rebinds the same authoritative state. A committed extraction/Recall resolves to Hall, a committed death resolves to Character Select, and crash restore resolves to Hall from the entry restore point. Presentation never resurrects a dead character.
- Duplicate-session handoff invalidates the older transport only after the new authoritative binding succeeds.

### 3. Strict Core localization

Extend the unpromoted `core_dev` world-flow localization boundary with exact en-US transition headings, status lines, retry/return actions, prior-safe-state language, three-second vulnerability warning, and the nine existing `TECH-010` handshake rejection messages. Runtime hard-coded transition copy is rejected by tests. Other locales and ship-quality narrative treatment remain later-stage work.

### 4. Disposable real-QUIC journey

The integrated route fixture uses only the wipeable test namespace and an explicit test-only admission capability. It traverses the real reliable protocol and QUIC adapters through Character Select, Hall, private microrealm, fixed Bell rooms, Sir Caldus terminality, committed extraction, and Hall arrival. It may prepare exact fixture state through guarded repositories, but every route transition itself must use the production protocol/coordinator boundary. It cannot advertise or enable normal Character Select `Play`, Realm Gate admission, production inventory extraction, seeded branches, or Core promotion.

The journey matrix covers baseline, response loss/retry, disconnect before and after authoritative transfer, reconnect before tick 90, committed extraction before reconnect, committed death before reconnect, duplicate session, content mismatch, allocation failure, and server crash restore. Each case proves the prior safe state or exact committed terminal wins without duplication.

### 5. Visual and performance evidence

- Capture inspected optimized-native evidence at 1920x1080 and 1280x720 for Hall loading, dungeon loading, recoverable error, fatal error, LinkLost vulnerability, reconnecting, same-state recovery, and committed Hall resolution. Include standard and reduced-motion/effects coverage without changing information or layout priority, plus one ultrawide reference for the transition shell.
- Preserve the GDD's center-60% and lower-middle combat corridor. Keyboard focus, pointer state, retry availability, non-color status identity, and long-copy wrapping are mandatory.
- Report route load/reconnect timings, frame-time p95/p99, peak/steady memory, and a 30-minute route soak from the measured machine with build/content hashes. Apply the exact `TECH-070` thresholds, but label results as measured evidence rather than claiming reference-hardware certification when the hardware differs.
- The M03 login-to-control median remains below 30 seconds. Final reference-hardware certification, broad device coverage, and ship optimization remain later release gates.

## Scope preservation

This decision authors presentation and evidence behavior only. It does not implement `GB-M03-04` inventory/vault truth, `GB-M03-06` death/memorial, `GB-M03-08` production extraction/Recall loss, party reconnect, public allocation, telemetry export, release packaging, or Core promotion. Parent `GB-M03-03` and the normal player route remain open until their owning packages pass.

## Decision

The owner directed that future recommendations be applied without further approval requests. All five recommendations above are therefore approved without amendment on 2026-07-13.
