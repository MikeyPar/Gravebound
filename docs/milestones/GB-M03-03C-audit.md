# GB-M03-03C completion audit

## Result

PASS. Exact compiled Hall/private-microrealm simulation, deterministic lifecycle trace, inspected optimized-client evidence, and cumulative local/hosted gates pass for commit `272df4d`.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical Production GDD | `WRLD-001` requires a noncombat Lantern Halls with Realm Gate, Vault, Overflow, Memorial Wall, and Oath Shrine; `UI-030`, `ART-001`-`002`, `ART-005`-`006`, and `ART-020` require readable native presentation, minimum/reference resolution review, restrained palette/effects, localization, and traceable assets. `ART-010`-`011` define the later audio hierarchy/loop boundary; `03C` does not claim the ship-quality M05 Hall audio. The compiled Hall prohibits hostile, damage, projectile, pickup, drop, and death creation; the native showcase preserves playfield priority at 1280x720 and 1920x1080 in standard/reduced-motion modes. |
| Content Production Specification | `CONT-001`-`003`, `CONT-WORLD-001`, and `CONT-HUB-001`-`002` define the exact 64x48 Hall, 48x48 capacity-one Core microrealm, collision shell/solids, stations, arrivals, road, landmarks, portal conditions, trigger timing, warning, empty reset, and disabled macro systems. Runtime geometry and copy are compiled from those strict `03A` records rather than duplicated client constants. |
| Development Roadmap | `GB-M03-03` requires the graybox Hall/private-microrealm route; the M03 cumulative target requires both scene kits with collision, anchors, palette/lighting intent, navigation, readability, localization, and fixed-trace validation at 35% presentation quality. Approved `SPEC-CONFLICT-006` assigns these foundations to `03C`, defers pack/dungeon content to `03D`-`03E`, and keeps normal admission fail closed. |

## Acceptance evidence

| Requirement | Evidence | Result |
|---|---|---|
| Exact compiled geometry | Strict `sim_content` compilation produces immutable renderer-independent scenes with exact dimensions, shell, five Hall solids, objects, roads, capacity, arrivals, independent integration gates, dynamic conditions, and stable BLAKE3 digests. Missing, extra, mismatched, overlapping, out-of-bounds, or semantically invalid records fail validation. | PASS |
| Authoritative safety | Hall creation policy explicitly prohibits hostile, damage, projectile, pickup, drop, and death creation. The microrealm remains private capacity one with RealmCycle, Siege, and Retirement disabled. The normal Character Select `Play` and production Realm Gate route are not wired. | PASS |
| Collision and movement | Fixed-point radius-aware occupancy, bounded cardinal/diagonal movement, wall sliding, shell/interior collision, scene identity, and overflow rejection are renderer independent. The native player consumes the same scene/player authority at 30 Hz. | PASS |
| Navigation | Grid-connected path proofs cover both Hall arrivals to every enabled Core station and the microrealm road route. Explicit creation policies reject blocked spawn sites and the intentionally road-conflicting anchor. | PASS |
| Interaction authority | Range-ordered projections, exact instant/15-tick holds, typed `StageDisabled`/`ConditionUnmet`, release/focus reset, one-panel-per-player exclusion, and Escape-close-without-mutation are server-owned seams. Empty integration gates keep every affected Hall station unavailable. | PASS |
| Microrealm lifecycle | First movement beyond one tile or primary release enters Waiting, tick 31 requests the exact 27-tick/900 ms warning, Active supports the disposable `03D` clear seam, five seconds empty resets to Dormant, and Cleared is terminal and satisfies the Bell-portal condition. No `pack.bell.01` entity is constructed in `03C`. | PASS |
| Native presentation | The optimized Bevy showcase renders compiled geometry/localization, muted wet-stone/brass/ash hierarchy, roads, solids, station/portal/safe markers, player silhouette, bounded camera, off-camera label culling, explicit state/prompt copy, HUD global layering, keyboard controls, and standard/reduced-motion modes. | PASS |
| Fixed trace | The pure input-driven runtime pins Hall `StageDisabled` plus microrealm Dormant -> Waiting -> warning -> Active -> Cleared at BLAKE3 `25403408dac36184b2166a8454adbf22a7bb8db66df3eebbca9fa3d920f41bf9`. Scene/state mismatches fail closed and evidence states freeze only after exact endpoints. | PASS |
| Route boundary | Character Select `Play`, production Realm Gate admission, real item/vault preflight, death, extraction, Recall, normal QUIC journey, and Core promotion remain disabled for their owning packages. | PASS |

## Visual evidence

- Hall standard 1280x720: [`GB-M03-03C-hall-1280x720.png`](../evidence/GB-M03-03C-hall-1280x720.png), SHA-256 `A4A66A1A5041FD49DFE5E1214C2E860CFFAAB92C8EB7388B4F83E268BD5C87CC`.
- Microrealm reduced-motion 1920x1080: [`GB-M03-03C-microrealm-reduced-1920x1080.png`](../evidence/GB-M03-03C-microrealm-reduced-1920x1080.png), SHA-256 `368442EA6C20D2D7B4C0692ED225FBBB14CE17A8EC1CFB10EE633CA55F0264C3`.
- Hall Realm Gate fail-closed 1920x1080: [`GB-M03-03C-hall-stage-disabled-1920x1080.png`](../evidence/GB-M03-03C-hall-stage-disabled-1920x1080.png), SHA-256 `4335B057E91985E3558F3A9A4AFEE0BCD047B665E66A3D469D16F570E1BF19AB`.
- Microrealm exact warning 1920x1080: [`GB-M03-03C-microrealm-warning-1920x1080.png`](../evidence/GB-M03-03C-microrealm-warning-1920x1080.png), SHA-256 `0A9DE1E321F524D0EEE51FA0CB5BCFB0278EF736A0D1C287CE2901A0F624C36E`.
- Microrealm terminal Cleared/Bell portal 1920x1080: [`GB-M03-03C-microrealm-cleared-1920x1080.png`](../evidence/GB-M03-03C-microrealm-cleared-1920x1080.png), SHA-256 `BC58C3F610F11AFF115AEAF474C833CC83354C0AAD5DC547E58FD64BBE8EA5A7`.

Every frame was captured atomically from the optimized Windows client and inspected at original resolution. Rejected frames with unclamped cameras, overlapping/off-camera labels, overwritten transition copy, or world-over-HUD layering were replaced rather than accepted.

## Verification

- Hosted CI: [run `29288946461`](https://github.com/MikeyPar/Gravebound/actions/runs/29288946461) PASS for exact commit `272df4d`, including Windows release, PostgreSQL transactions, format, warnings-denied lint, workspace tests, strict content validation, deterministic trace, and generated-schema verification.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `cargo test --workspace --locked`: PASS, including seven native-showcase tests plus exact content, scene, lifecycle, navigation, and interaction tests.
- `cargo run --locked -p tools_content -- validate`: PASS.
- Duplicate `cargo run --locked -p tools_content -- trace tests/deterministic/m00_smoke.json`: byte-identical PASS.
- `cargo run --locked -p tools_content -- generate-schemas` plus clean schema diff: PASS.
- `cargo build --release --locked -p client_bevy`: PASS.
- `git diff --check`: PASS.

## Granular delivery

- `6a0120f` - exact Core scene compilation.
- `a5e744b` - deterministic capacity-one microrealm lifecycle.
- `25d2495` - authoritative collision/navigation and safe creation policy.
- `eda140c` - authoritative scene interactions.
- `b9029e2` - native standard/reduced-motion world showcase and baseline evidence.
- `488f75c` - pinned scripted lifecycle trace and exact runtime evidence states.
- `272df4d` - GitHub README status and accepted screenshots.

## Remaining ownership

`GB-M03-03D` owns `pack.bell.01`, fixed Bell dungeon rooms, Core normal enemies, and minibosses. `03E` owns Sir Caldus, committed extraction, and Hall return; `03F` owns loading/error/reconnect UX plus real-QUIC journey, failure, visual, and performance closure. `GB-M03-04`, `06`, and `08` still own production item/vault preflight, death, extraction, and Recall semantics. The normal route and Core promotion remain fail closed.
