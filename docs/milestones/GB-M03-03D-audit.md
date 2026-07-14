# GB-M03-03D completion audit

## Result

PASS. Strict unpromoted Core encounter content, the exact six-normal/two-miniboss roster, nine Bell templates, fixed-room and private-microrealm combat owners, deterministic full-combat fixtures, inspected optimized-client evidence, and cumulative local/hosted gates pass for commit `97f8b60`.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical Production GDD | `COM-001`-`006`, `DNG-001`, `DNG-003`-`006`, `DNG-040`, `ENC-001`-`005`, `ENC-014`, `ENC-020`, `UI-030`, `ART-005`-`011`, `TECH-004`, and `TECH-020`-`023` are represented by renderer-independent combat ownership, deterministic target/cast locks, room lifecycle safety, readable hostile-priority presentation, reduced-effects parity, and fixed-step replay. The native adapter is disposable and cannot admit or mutate a production run. |
| Content Production Specification | Strict records compile exactly six normal enemies, Sepulcher Knight, Choir Abbot, `pack.bell.01`, nine Bell templates, the fixed B0→B6 chain, B1/B2/B3/B5 encounters, 900 ms warnings, reward/XP bindings, assets, localization, and stable hashes. Approved `SPEC-CONFLICT-014` through `021` resolve only underspecified deterministic mechanics; none change authored payload, cadence, or route scope. |
| Development Roadmap | `GB-M03-03` requires Hall → micro-realm → six-room dungeon → boss → Hall. Approved `SPEC-CONFLICT-006` assigns fixed rooms, Core normal enemies, and minibosses to `03D`; Sir Caldus and committed extraction remain `03E`, while normal ingress remains closed until all parent dependencies pass. The M03 cumulative 6/2 and nine-room content targets are now met without promoting Core content. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Strict content closure | The unpromoted compiler proves exact roster order, immutable First Playable reuse, authored behavior/pattern closure, reward/XP references, symbolic assets, localization, all nine room templates, `pack.bell.01`, fixed-layout membership, budgets, warnings, and stable digests. Mutations fail closed. | PASS |
| Room geometry and route | Exact northwest-origin doors, volumes, hazards, anchors, rotations, four-tile corridors, safe navigation, role-compatible spawn capacity, and the placed B0→B6 layout are validated. BB1/BS1 are authored but absent from the compiled route; B6 is geometry-only. | PASS |
| Micro-realm pack | The existing `03C` lifecycle constructs exactly six Pilgrims and two Bell Reeds on the first eight sorted legal anchors after the 27-tick warning. Authoritative defeat alone clears; empty reset removes output and advances identities; terminal Cleared cannot respawn. | PASS |
| Shared normal contract | All six normal roles have exact health/armor/hurtbox, target ordering, warning/aim locks, locomotion, attacks, damage/band, aggro/leash/reset, reward/XP, and deterministic replay coverage. Reused Pilgrim/Reed/Sentry behavior remains on the immutable First Playable owners. | PASS |
| B1/B5 ownership | Immutable wave owners compose exact room lifecycle, combat, hostile projectiles, rewards, cleanup, quiet/reset boundaries, and disjoint retry identities for `6 Pilgrim + 2 Reed` and `6 Pilgrim + 1 Sentry`. | PASS |
| B2 ownership | One transaction owns six immutable Pilgrims, two authored Acolytes, and one Choir Skull. The sustained fixture pins alternating fans, the complete two-arm rotor, locomotion, projectile IDs, nine-actor clear, reward/drop handoff, and reset without softlock. | PASS |
| Sepulcher Knight | The B3 owner pins three-second introduction, pursuit, immutable five-tile/17-segment charge, one swept contact, solid truncation, dynamic home, 8-of-10 stop ring, five-shot shield fan, shared hostile pipeline, Elite rewards/XP, quiet, and reset over a replay-identical 35-second fixture. | PASS |
| Choir Abbot | The disabled-branch fixture pins stationary targeting, three-second introduction, ten independently rounded rotor volleys, recovery warnings, immutable target-facing 12-of-16 gap ring, equal-tick ring-before-rotor ordering, shared hostile pipeline, Elite rewards/XP, quiet, and reset over a replay-identical 35-second fixture. | PASS |
| Mire Leech | The disabled-route fixture pins its harmless spawn warning, approach, exact 15-segment charge, stable swept contact, solid truncation, 45-tick retreat, reward/XP, and reset without inserting Mire into the main chain. | PASS |
| Room lifecycle safety | Capacity one, hurtbox-safe doors, exact warning/quiet/empty-reset boundaries, monotonic objective progress, one completion handoff, projectile/drop cleanup, immutable death/Recall input, adversarial ordering, and atomic rollback are executable. Validators separately exercise `N=1..8` scaling/counterplay without enabling party admission. | PASS |
| Native presentation | The isolated optimized Bevy surface loads only compiled encounter content, derives four real SpawnWarning states from room authority, presents every Core 6/2 identity, exact B0→B6 room state/budget/warning metadata, and explicit B6/BB1/BS1/seeded/ingress disabled states. Standard and reduced-effects frames preserve identical hostile-priority information at both required resolutions. | PASS |
| Route boundary | Character Select `Play`, production Realm Gate admission, Sir Caldus behavior/rewards, BB1/BS1 traversal, seeded selection, normal route, durable extraction/death/Recall arbitration, and Core promotion remain unavailable. | PASS |

## Visual evidence

- Standard 1920×1080: [`GB-M03-03D-standard-1920x1080.png`](../evidence/GB-M03-03D-standard-1920x1080.png), SHA-256 `103FABA19844DDA38EE1450F7BAA5891AB685844B7D02AB80D0A57FD4B133457`.
- Reduced effects 1920×1080: [`GB-M03-03D-reduced-1920x1080.png`](../evidence/GB-M03-03D-reduced-1920x1080.png), SHA-256 `F97790B4032AA9EE00013EBE91CD72DCED3110365A68663030B37230ED415024`.
- Standard 1280×720: [`GB-M03-03D-standard-1280x720.png`](../evidence/GB-M03-03D-standard-1280x720.png), SHA-256 `D4BE260EDCC2E4D18496B04C4A38C2BA6DA1B9D6EB8F960CC17BF26EB95FBD26`.
- Reduced effects 1280×720: [`GB-M03-03D-reduced-1280x720.png`](../evidence/GB-M03-03D-reduced-1280x720.png), SHA-256 `F433C3C899F5A8240250769E7ECF34719EC80B83F584A84D29CC5CD4266D3231`.

All four frames were captured atomically from the optimized Windows client and inspected at original resolution. The first reduced-effects captures were rejected because a layout-affecting border change displaced persistent UI; the retained frames keep identical geometry and warning priority while reducing only nonessential marker intensity.

## Verification

- Hosted CI: [run `29306504400`](https://github.com/MikeyPar/Gravebound/actions/runs/29306504400) PASS for exact commit `97f8b60`, including Windows release construction, mandatory PostgreSQL transactions, format, warnings-denied lint, workspace tests, strict content validation, deterministic trace, and generated-schema verification.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `cargo test --workspace --locked`: PASS, including 88 native-client, 99 `sim_content`, and 303 `sim_core` tests plus server, persistence, protocol, and integration suites.
- `cargo run --locked -p tools_content -- validate`: PASS.
- Duplicate `cargo run --locked -p tools_content -- trace tests/deterministic/m00_smoke.json`: byte-identical PASS.
- `cargo run --locked -p tools_content -- generate-schemas` plus clean schema diff: PASS.
- `cargo build --release --locked -p client_bevy`: PASS.
- `git diff --check`: PASS.

## Granular delivery

Delivery was split across strict schema/content, compiler/geometry, shared AI and kit scheduling, micro-realm construction, fixed-room lifecycle and encounter owners, B2 mixed combat, Sepulcher Knight, Choir Abbot, Mire Leech, native projectile compatibility, the disposable evidence surface, and retained inspected evidence. The final presentation commits are:

- `6b8239f` - rotating-arm native hostile presentation.
- `36fbe13` - isolated compiled Core encounter evidence surface.
- `97f8b60` - inspected standard/reduced-effects evidence at both required resolutions.

## Remaining ownership

`GB-M03-03E` owns production Sir Caldus, the boss participant lock, committed extraction exit, and authoritative Hall return. `03F` owns loading/error/reconnect UX plus real-QUIC journey, failure, visual, and performance closure. `GB-M03-04`, `05`, `06`, and `08` still own the parent route's inventory/vault preflight, shrine truth, death/memorial, extraction, and Recall semantics. Normal ingress, affected Hall stations, and Core promotion remain fail closed.
