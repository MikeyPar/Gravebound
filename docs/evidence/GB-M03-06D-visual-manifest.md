# GB-M03-06D native death and Memorial visual evidence

**Status:** PASS on client implementation commit `094d70e`; captured from the optimized Windows release executable built from that exact tree.

## Three design authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-020`, `UI-001`, `UI-002`, `UI-009`-`UI-011`, `UI-030`, and `TECH-022` require a durable-acknowledged death summary in exact order, accessible native controls, bounded recovery states, and no commercial interruption. This visual package proves the presentation prerequisites for `DTH-021`; its two-second latency and human recovery metrics remain with `GB-M03-06E` and final M03 acceptance.
- `Gravebound_Content_Production_Spec_v1.md`: `CONT-HUB-001`, `CONT-HUB-002`, and `CONT-LOC-001` require the Core Memorial Wall, newest-first `(death_at DESC, death_id)` ordering, exact stored snapshots, and closed canonical `en-US` copy.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-06`, `GB-M03-07`, `GB-M03-13`, and the M03 explanation/recovery gates require readable committed death evidence while successor creation and the normal route remain closed.

## Build and content identity

- Source commit: `094d70e16debc22e3df137cb0e66eee2d3ad98e8`.
- Focused read-only Back hardening: `92994f958f48e215dd8f0fd6eba4dce5abf11ef8`; presentation output is unchanged from the exact evidence binary.
- Build command: `.\tools\dev.cmd release` (`cargo build --locked --release -p client_bevy -p server_app`).
- Executable: `target/release/client_bevy.exe`.
- Executable SHA-256: `7de637f11a89e41c63f2b8e14eaca7e09122ff007df8db1966eb9c8150670225`.
- Executable size: `72,672,768` bytes; completed `2026-07-16 04:45:00 UTC`.
- Core item content revision: `core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb`.
- Death records BLAKE3: `349730a1246857978d1412510ee23af46624ec80dbb3333be42aad2e47f1f8e0`.
- Death assets BLAKE3: `0160f06954c88aba61392f72af66031d6f7ff4a592beb24f7ebe9f1981cc7a68`.
- Death localization BLAKE3: `c10bcc96887aac7db8c855f19d991e6185f46d1df39f7a37d3a31cb4b9ca1b92`.
- Runtime portrait-atlas BLAKE3: `e750553346829f5d4c0b7944da9b27ca79cfba5612f9e36e36ef707618678dd3`.
- Alegreya Sans Regular/Bold BLAKE3: `6c435d633146e3d45a22a0543b590cddb6d161db81055a2e93f0f43cf2d5df2a` / `c1e4ccd1cf57b5fb428f04f50d3c2532a185dcfa9ddaa2475eac146a313238d5`.

## Artifact matrix

| Artifact | Dimensions | Mode/state | SHA-256 | Inspection |
|---|---:|---|---|---|
| [`GB-M03-06D-summary-standard-1280x720.png`](GB-M03-06D-summary-standard-1280x720.png) | 1280x720 | Standard / summary top | `34844ff7600972efda19f9dbdae3182557ede5990b111ef4a0a8ede92eaf9948` | PASS |
| [`GB-M03-06D-summary-reduced-1280x720.png`](GB-M03-06D-summary-reduced-1280x720.png) | 1280x720 | Reduced effects / summary top | `34844ff7600972efda19f9dbdae3182557ede5990b111ef4a0a8ede92eaf9948` | PASS |
| [`GB-M03-06D-summary-standard-1920x1080.png`](GB-M03-06D-summary-standard-1920x1080.png) | 1920x1080 | Standard / complete summary | `8317c79a9e06731ab6cdfc4e1bf8f6226b17b7a9ae90e132c8c38f3bceee1d66` | PASS |
| [`GB-M03-06D-summary-reduced-1920x1080.png`](GB-M03-06D-summary-reduced-1920x1080.png) | 1920x1080 | Reduced effects / complete summary | `9fa7ada8d9e3e3eac6ee762d246f8e3a13861e2c2991c22caaf1ec22b8e07b64` | PASS |
| [`GB-M03-06D-summary-actions-standard-1280x720.png`](GB-M03-06D-summary-actions-standard-1280x720.png) | 1280x720 | Standard / focused recovery actions | `f18d1e8738c59d9d57b9dbd2df25f4f5e5b36ea7595cb5c8f9a2e0e2e1f1a22f` | PASS |
| [`GB-M03-06D-summary-actions-standard-1920x1080.png`](GB-M03-06D-summary-actions-standard-1920x1080.png) | 1920x1080 | Standard / focused recovery actions | `b90b298383ce8c0c54a5567a6177bdc1ea298ba20638f15000c10a5d2c944514` | PASS |
| [`GB-M03-06D-summary-trace-standard-1920x1080.png`](GB-M03-06D-summary-trace-standard-1920x1080.png) | 1920x1080 | Standard / emphasized trace | `bf1a90f299a97fc600e37025b6f24f3c34ace80c71fd9a58c48e7625f0bc3e9f` | PASS |
| [`GB-M03-06D-memorial-list-standard-1280x720.png`](GB-M03-06D-memorial-list-standard-1280x720.png) | 1280x720 | Standard / Memorial list | `ea341d1b25ee32ef1201d38f020e2d1f40c3917b1e4e617f1862a3779040396f` | PASS |
| [`GB-M03-06D-memorial-list-reduced-1280x720.png`](GB-M03-06D-memorial-list-reduced-1280x720.png) | 1280x720 | Reduced effects / Memorial list | `aadb5d365126bab038f0b0df98c4cbac446137f637071769dce66198297994db` | PASS |
| [`GB-M03-06D-memorial-list-standard-1920x1080.png`](GB-M03-06D-memorial-list-standard-1920x1080.png) | 1920x1080 | Standard / Memorial list | `217387493cc1188585bc109ffc9d78857c655ae9c99bfba51d7bc5856ea26dfa` | PASS |
| [`GB-M03-06D-memorial-list-reduced-1920x1080.png`](GB-M03-06D-memorial-list-reduced-1920x1080.png) | 1920x1080 | Reduced effects / Memorial list | `d3f17c1a6bb743ad82782545f81ae398aaf55f1a6fb9d5b25a8ac0fa859a56a1` | PASS |
| [`GB-M03-06D-memorial-detail-standard-1280x720.png`](GB-M03-06D-memorial-detail-standard-1280x720.png) | 1280x720 | Standard / stored Memorial detail | `946c7eef3d54fc1378513edc2e66ffb4ec6d1ce9092ff00c189602a220a2f093` | PASS |
| [`GB-M03-06D-memorial-detail-reduced-1280x720.png`](GB-M03-06D-memorial-detail-reduced-1280x720.png) | 1280x720 | Reduced effects / stored Memorial detail | `946c7eef3d54fc1378513edc2e66ffb4ec6d1ce9092ff00c189602a220a2f093` | PASS |
| [`GB-M03-06D-memorial-detail-standard-1920x1080.png`](GB-M03-06D-memorial-detail-standard-1920x1080.png) | 1920x1080 | Standard / stored Memorial detail | `d2fd81eca362f888c3f279d91572cfb6fbeb3c3367533e124d7cd33a4c685bb4` | PASS |
| [`GB-M03-06D-memorial-detail-reduced-1920x1080.png`](GB-M03-06D-memorial-detail-reduced-1920x1080.png) | 1920x1080 | Reduced effects / stored Memorial detail | `809a1bb2e77039091ccd5f4498804e9edb692ac1e4478a2bb51645b668622e6d` | PASS |
| [`GB-M03-06D-awaiting-commit-standard-1280x720.png`](GB-M03-06D-awaiting-commit-standard-1280x720.png) | 1280x720 | Standard / awaiting durable acknowledgement | `88fc2c248d6ef234c91df3b8f3ae26685f7bab348f3c5c539f6d31b40874c6f8` | PASS |
| [`GB-M03-06D-recoverable-error-reduced-1280x720.png`](GB-M03-06D-recoverable-error-reduced-1280x720.png) | 1280x720 | Reduced effects / recoverable error | `cff98057b15fb4de714f62028e261af79c5d5d0863b6dffb47181a40edb4f79a` | PASS |

## Inspection record

- Every PNG decoded at its stated original dimensions. Captures were requested only after the pinned fonts, native root, and Bevy text layout had settled for 90 frames and were published through the atomic screenshot path.
- The 1920x1080 summary fits without scrolling and preserves exact `DTH-020` order: hero, cause, last five, network/Recall, `Lost`, `Preserved`, `Created`, primary successor, then the three secondary actions.
- At 1280x720 the same semantic projection uses a visible bounded scrollbar. The top capture shows hero/cause/trace; the action capture records offset `259/304`, showing that keyboard focus reveals `Inspect Damage Trace` without jumping past required content.
- The Created card shows the exact stored `Available Echo` outcome rather than a generic creation count. `Create Successor` is the largest action and remains visibly disabled; no store, offer, product, promotion, or paid surface appears.
- Trace emphasis retains the full summary and adds source portraits/non-color emphasis to the exact five ordered events. Unknown portrait mappings fail closed before replacing the last safe snapshot; `ExplicitlyAbsent` remains a distinct validated state.
- Memorial rows are visibly newest-first and include stored class, presentation, Echo outcome, and UTC timestamp. The evidence driver binds every actionable row cursor to its own stored summary/digest/trace; historical detail structurally disables successor, Memorial, and character-select actions.
- Standard and reduced-effects captures retain identical semantic signatures. Pixel differences at reference resolution are restricted to nonessential ambience; the 1280 summary/detail pairs are byte-identical where the static render tree is identical.
- The waiting state exposes no committed losses or recovery actions. The recoverable error shows only canonical service-unavailable copy and a focused `Retry` action.
- The renderer supports pointer, keyboard, and controller focus metadata; skips disabled actions; wraps enabled focus; keeps focused controls visible; and enforces UI scale `80`-`150` plus an effective 14 px text floor.

## Verification

- Focused death/Memorial/native tests on `92994f9`: 45 passed, including the Escape-to-read-only-Back boundary.
- Complete client target on `92994f9`: 152 passed.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy -p client_bevy --all-targets --all-features -- -D warnings`: PASS.
- `.\tools\dev.cmd release`: PASS for the exact local evidence binary.
- The local full workspace gate passed formatting, strict workspace Clippy, all workspace tests, content validation, and two deterministic traces; Windows antivirus blocked only the `tools/persistence-test.ps1` wrapper at parse time.
- Exact hosted CI [`29471819642`](https://github.com/MikeyPar/Gravebound/actions/runs/29471819642) supplies the non-bypassed mandatory PostgreSQL suite and Windows release proof for `094d70e`.

End-to-end durable-commit-to-interactive latency, canonical cross-process signatures, crash-boundary injection, and resource-cleanup measurements remain owned by `GB-M03-06E`; this visual package does not substitute for them.
