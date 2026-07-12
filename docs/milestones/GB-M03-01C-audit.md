# GB-M03-01C completion audit

## Result

PASS for native Core identity presentation and authoritative create/select runtime evidence.

## Three-authority review

| Authority | Implemented evidence |
|---|---|
| Canonical GDD | `UI-001`/`UI-002` state handling, `UI-007` two-slot roster, approved `UI-008` creation subset, `LOOT-050` capacity, and `TECH-020`/`TECH-021` safe authoritative transitions are represented without inventing deferred fields. |
| Content Production Specification | The client loads strict `core-dev` identity and `en-US` copy descriptors, mechanically renders the immutable FP Grave Arbalist/four-ability subset, rejects unresolved placeholders, and creates no promotion record. |
| Development Roadmap | Delivers only the native portion of `GB-M03-01`; PostgreSQL, Hall routing, progression, and permanent-death packages remain excluded. |

## Runtime and visual evidence

- Release client: `363ddb9f9dab8ddb746c12a5c3b19c9de8750d7543cf16e2ff5f58b003a62de3`.
- Release server: `4f856d9631022aa782010ed06fe2722775914df34e80226346bea135b6cc89a3`.
- 1280x720 frame: [`GB-M03-01C.png`](../evidence/GB-M03-01C.png), SHA-256 `790d7345fa48f232739502611db0ce8632137db01485352330be79ef2672cd32`.
- 1920x1080 frame: [`GB-M03-01C-1080p.png`](../evidence/GB-M03-01C-1080p.png), SHA-256 `4c013eefb8d1d2d86456fda64dfbcae7e994cec91705608a06249434956accbb`.
- Both frames came from the optimized executables completing real QUIC empty-roster -> create -> refresh -> select journeys with distinct wipeable credentials.
- Visual inspection found no missing glyphs, clipping, unsafe margins, or false persistence/control claims. The 1080p surface stays centered and bounded; enabled and disabled actions remain distinguishable by border, fill, and text treatment.

## Automated verification

- `tools/dev.cmd m03-identity-smoke`: protocol 19 passed; client focused 5 passed; server real-QUIC restart-wipe 1 passed.
- Strict client and server all-target Clippy passed with warnings denied.
- `tools_content validate` passed with strict Core identity/copy schemas and feature registry entries.
- `cargo build --locked --release -p client_bevy -p server_app` passed.
- State-model tests cover authoritative empty/ready/selected projections, safe error retention, disabled classification, mutation IDs, action availability, and validated placeholder-free copy.

## Honest manual boundary

The required resolutions were inspected from native release frames and keyboard/pointer actions share the same authoritative command path. A separate human-operated keyboard/focus feel pass was not fabricated; it remains useful playtest evidence for the eventual M03 exit gate, but does not alter this package's deterministic command, accessibility-copy, or layout results.
