# GB-M03-03E completion audit

## Result

PASS. Strict unpromoted Sir Caldus content, deterministic participant and combat authorities, reward-gated victory presentation, committed wipeable extraction, idempotent `HallDefault` transfer, inspected native evidence, and cumulative local gates pass for commit `c4c592d`. Hosted CI confirmation is recorded below.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical Production GDD | `COM-001`-`006`, `DTH-001`, `DTH-010`-`011`, `WRLD-006`, `DNG-006`, `ENC-005`, `ENC-010`, `ENC-020`, `SOC-010`, `TECH-004`, `TECH-015`, `TECH-021`, and `TECH-023` are represented by fixed-step combat, immutable participant locks, exact health scaling, deterministic targeting and identity, durable personal rewards, a receipt-gated exit, transactional Hall return, restart-safe replay, and reduced-effects parity. The normal player route and production inventory extraction remain unavailable. |
| Content Production Specification | The strict Core compiler binds exactly `boss.sir_caldus`, `arena.boss.caldus_01`, `reward.boss_caldus`, the four authored hostile patterns, the B6 overlay, the reward-gated Bell Sepulcher exit, Hall destination, assets, localization, warning language, scheduler values, participant policy, and stable hashes. Approved `SPEC-CONFLICT-022`-`024` resolve only missing deterministic Caldus mechanics and transfer ordering. |
| Development Roadmap | The approved `GB-M03-03E` split owns the Core major boss, boss participant lock, personal reward closure, committed extraction seam, and authoritative Hall return. Restart, rollback, retry, replay, presentation, and Windows construction gates pass without importing `GB-M03-08` inventory conversion or opening production ingress. `03F` retains loading, failure, reconnect, real-QUIC, visual, and performance journey closure. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Strict content closure | Four checked-in schemas and independent source pins prove exact boss, arena, pattern, reward, XP, exit, B6, asset, localization, and hash closure. Mutation tests reject scalar, timeline, binding, metadata, and promotion drift. | PASS |
| Participant authority | Clone-stage-commit ownership pins all-loaded/timeout staging, visible countdown and introduction, immutable `(party_slot, entity_id)` locks, `N=1..8` round-half-up health scaling, no late entry or rescale, legal Recall, zero-living reset, cancellation, expiry, and fresh attempt identities. | PASS |
| Caldus scheduler and movement | Golden fixtures pin all Phase 1-3 and low-health loops, warnings, target locks, group rotation, persistent ring gaps, thresholds, cleanup, exact four-second breaks, charge axis/tie selection, 17 movement segments, solid truncation, contact order, return, and cadence-only soft enrage. | PASS |
| Integrated combat | The aggregate uses real armor, hurtboxes, friendly intent, projectiles, lanes, charge contact, phase cleanup, rollback, and terminal hostile cleanup. The complete solo fixture defeats Caldus at tick 5,400 (180 seconds), within the authored 150-210 second target. Duplicate runs produce byte-identical traces. | PASS |
| Personal victory and exit | Domain-separated stable encounter, reward, exit, extraction request, and receipt identities cannot collide or be reused across attempts. SOC-010 eligibility, exact Core rewards, 450 XP plus 225 first-clear XP, schema-25 terminality, retry recovery, and the reward-gated stable exit are exercised against PostgreSQL. | PASS |
| Extraction and Hall transfer | Protocol 1.11 carries distinct committed request and receipt identities. Schema 26 and the dedicated wipeable coordinator bind the exact presented exit, character, lineage, restore point, content revision, and danger location. One serializable transaction consumes the receipt, records command kind 3, moves to `HallDefault`, and removes the matching checkpoint. | PASS |
| Adversarial persistence | Wrong receipt, payload drift, duplicate request, accepted replay, database-pool restart, item-only partial completion, crash-restore supersession, forced checkpoint mismatch, late transaction rollback, repair-and-retry, and exact checkpoint cleanup pass. Inventory version remains unchanged. The Caldus PostgreSQL target is mandatory in CI. | PASS |
| Production boundary | Ordinary portal coordination rejects committed extraction. The disposable Caldus authority has no normal ingress. No item conversion, Overflow, ResolutionHold, wallet, storage, or production namespace mutation exists in the 03E boundary. | PASS |
| Native presentation | An isolated optimized Bevy showcase renders staging, introduction, phase learning, charge pressure, final rings, victory/exit, committed extraction, and Hall arrival from compiled Caldus content. Standard and reduced-effects modes preserve the same information hierarchy at 1920x1080 and 1280x720. | PASS |

## Visual evidence

The retained matrix contains 32 atomically captured PNGs: eight journey states, standard and reduced effects, and both required resolutions. Every image was checked for exact dimensions and nonblank output, and representative frames across the journey were inspected at original resolution for clipping, glyph support, telegraph priority, and playfield obstruction.

- Phase learning, standard 1920x1080: [`GB-M03-03E-phase-one-standard-1920x1080.png`](../evidence/GB-M03-03E-phase-one-standard-1920x1080.png), SHA-256 `EF315BC4DD520E057061953A759587053046B96D94D971594BDF6D332C127FCF`.
- Charge pressure, reduced effects 1280x720: [`GB-M03-03E-charge-pressure-reduced-1280x720.png`](../evidence/GB-M03-03E-charge-pressure-reduced-1280x720.png), SHA-256 `17C59333229E92DA3764BD1C0EE4FF875F40BB49B05E5D586F89825D9EEBA76E`.
- Victory-gated exit, standard 1920x1080: [`GB-M03-03E-victory-exit-standard-1920x1080.png`](../evidence/GB-M03-03E-victory-exit-standard-1920x1080.png), SHA-256 `72C7444E96C9F9ADFADDA0C6CB44DFBF4E21CAFCA05BAB14789D260031D7FCF2`.
- Authoritative Hall arrival, reduced effects 1920x1080: [`GB-M03-03E-hall-arrival-reduced-1920x1080.png`](../evidence/GB-M03-03E-hall-arrival-reduced-1920x1080.png), SHA-256 `DFF6650A1E8EDE7879DFD5FC16FF7310BB48F0197B717B486C34F3E97A9F509B`.

## Verification

- Hosted CI: [run `29320392826`](https://github.com/MikeyPar/Gravebound/actions/runs/29320392826) for exact commit `c4c592d`, covering Windows release construction, mandatory PostgreSQL transactions including Caldus victory/extraction/Hall recovery, format, warnings-denied lint, workspace tests, strict content validation, deterministic trace, and generated-schema verification.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `cargo test --workspace --locked --no-fail-fast`: PASS.
- `cargo run --locked -p tools_content -- validate`: PASS.
- `cargo run --locked -p tools_content -- validate-core-caldus`: PASS.
- Duplicate deterministic traces: byte-identical PASS.
- `cargo run --locked -p tools_content -- generate-schemas` plus clean schema diff: PASS.
- `cargo build --release --locked -p client_bevy`: PASS.
- Automated 32-image dimension/nonblank audit and representative original-resolution inspection: PASS.
- `git diff --check`: PASS.

## Granular delivery

The final delivery remained split by authority and failure domain:

- `b346009` - bind committed Caldus extraction in protocol 1.11.
- `24f3f7a` - commit the transactional Caldus Hall transfer.
- `4cc0415` - cover extraction recovery races.
- `98b8535` - prove late-failure rollback and exact retry.
- `2ba5a90` - pin fail-closed Hall preflight.
- `f6c20d6` - add the disposable native Caldus journey.
- `a2da08c` - retain the inspected 32-frame evidence matrix.
- `016fe61` - pin the complete Caldus schema table inventory.
- `c19cdbb` - make Caldus persistence transactions mandatory in CI.
- `6b15994` - correct byte encoding and valid payload-conflict PostgreSQL fixtures exposed by the mandatory gate.
- `c4c592d` - reset schema-25/26 cross-links in dependency order for shared destructive fixtures.

## Remaining ownership

This closes `GB-M03-03E`, not the parent milestone. `GB-M03-03F` still owns loading/error/reconnect UX, real-QUIC journey and failure evidence, and final visual/performance closure. `GB-M03-08` owns production pending-inventory conversion, Overflow, ResolutionHold, extraction loss semantics, and the normal extraction route. Parent M03 work also retains inventory/vault preflight, shrine truth, death/memorial and successor recovery, Recall, telemetry/support, Echo, platform, and final private-loop gates. Character Select `Play`, Realm Gate admission, BB1/BS1 traversal, seeded selection, affected Hall stations, production namespaces, and Core promotion remain fail closed.
