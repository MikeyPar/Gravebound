# GB-M03-03G Shared Terminal Writer Evidence

Date: 2026-07-18
Implementation commit: `93dde0a`
Status: local contract gates pass; hosted PostgreSQL/Windows verification pending

## Three design authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-010`, `DTH-011`, and `TECH-015`/`021`-`023` require committed terminal authority before Hall control, exact replay, generation-safe reconnect, and crash restoration that never reconstructs danger.
- `Gravebound_Content_Production_Spec_v1.md`: `CONT-HUB-001`/`002` and the fixed Core route require Hall as the terminal destination, Recall throughout danger, and the authored Realm Gate → Core micro-realm → Bell Sepulcher route.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-03`, `GB-M03-08`, and the M03 exit gates require one ordinary private-life transport, response-loss/restart safety, no duplicate terminal result, and zero operational residue.

## Implemented contract

- Recall and extraction expose symmetric prepare/commit/abort writer handoffs. Prepare is non-visible, exact retries recover the same reservation, changed writers advance generation, stale tokens fail closed, and exact committed retries are idempotent.
- `CorePrivateLifeSessionDirectory` is the only shared-writer lifecycle owner. It prepares both dynamic bindings before committing either, publishes the new session generation only after both commits, restores or fails closed after a partial handoff, and retires the superseded writer once.
- Extraction provides a non-retiring shared detach. Recall already detached without retiring the writer. Current disconnect removes both bindings before the session closes the connection; stale detach cannot affect the winning generation.
- Exact extraction Hall acknowledgment is correlated to the session's current extraction lease. Replay authority is consumed only after the matching Hall projection has been installed.
- Process-restart Recall delivery is bound to the authenticated wipeable account, selected character, current Hall ownership, stored trigger, and stored optional request sequence. Explicit retains `Some(sequence)`; LinkLost retains `None`.
- Process-restart extraction delivery requires the immutable accepted intent and matching committed terminal. Only the retry frame's current request sequence may differ; account, character, mutation, request/receipt/terminal/route identities, issued time, payload hash, content revision, and all five pre-versions remain exact.
- Both recovery paths use the current authoritative outer server tick, preserve historical inner terminal timing, emit through the session writer, and return opaque delivery proofs only after a successful write.
- Shutdown clears pending handoff reservations, retires actors, aggregates Recall/extraction reports, and requires zero session/runtime residue.

## Verification

- Formatting and `git diff --check`: pass.
- `cargo clippy -p server_app --all-targets -- -D warnings`: pass.
- `cargo test -p server_app --all-targets --no-fail-fast`: pass; `301/301` library tests plus every enabled integration target are green.
- Focused handoff tests: `2/2` pass.
- Focused restart-delivery tests: `5/5` pass, including authenticated Recall rejection and real-QUIC Recall/extraction delivery.
- Combined real-QUIC composition: pass. Session, Recall, and extraction hold the same `Arc<CoreReliableWriter>`; each transport emits contiguous sequences `1/2/3`; reconnect rebinds both dynamic leases before return; old leases and stale detach are harmless; current detach leaves central retirement to the session; session, Recall, extraction, and route shutdown report zero residue.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace --locked`: pass.
- First Playable, Core Caldus, Core death-view, and Core successor-recovery content validators: pass.
- Schema regeneration and `git diff --exit-code -- schemas`: pass.

## Remaining gate

Normal admission remains disabled. Compose live movement, combat, rewards, pending-inventory custody, and all five terminal producers through the persistent private-life session; then close hosted PostgreSQL/restart evidence, 25 ordinary full-loop journeys, optimized native visual review, and the parent `GB-M03-03` audit before enabling Character Select `Play`, the production Realm Gate, or normal extraction/Recall capability flags.
