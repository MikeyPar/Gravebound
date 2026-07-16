# GB-M03-06E integrated death, Echo, and Memorial evidence

**Status:** PASS for source commit `18dcbad6ee2a4b79f954c90418ec6842bce79050` on hosted CI [`29506273492`](https://github.com/MikeyPar/Gravebound/actions/runs/29506273492).

## Three design authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: `DTH-001`, `DTH-002`, `DTH-020`, `DTH-021`, `ECH-001`/`002`, `TECH-015`, `TECH-020`-`023`, `TECH-070`, and `QA-005`-`007` require atomic death finality, exact stored evidence, durable acknowledgement, restart/replay safety, adverse-network coverage, and measured native presentation.
- `Gravebound_Content_Production_Spec_v1.md`: immutable Core content, `CONT-ECHO-009`, `CONT-HUB-001`, and `CONT-HUB-002` require exact Echo eligibility/state projection, oldest-first promotion, and Memorial presentation over the committed snapshot.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-02D`, `GB-M03-06`, and `GB-M03-13` require restart preservation, no duplicate terminal records, and qualifying death/destruction/memorial/Echo atomicity. The final 25 death-to-successor loops and human comprehension metric remain outside this subsystem evidence.

## Hosted run identity

- Source commit: `18dcbad6ee2a4b79f954c90418ec6842bce79050`.
- Authoritative cumulative run: [`29506273492`](https://github.com/MikeyPar/Gravebound/actions/runs/29506273492), PASS.
- Source artifact archive: `gb-m03-06e-death-source-29506273492-1`, GitHub artifact SHA-256 `44e8cb2bf0a980b7e7343d285fa0e5b9bf35d091e5392c12cd1df7a10b04698f`.
- Native artifact archive: `gb-m03-06e-native-death-frame-29506273492-1`, GitHub artifact SHA-256 `ba0b75495f735b02086b20a8348cf2fd12bb3efe03b9a76d26854919179380be`.
- Client executable BLAKE3: `8ec7e6b488edd116498f6a06caeb01853382179c2ff8802605cd666a43380b8f`.
- Core item revision: `core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb`.
- Death view records/assets/localization BLAKE3: `349730a1246857978d1412510ee23af46624ec80dbb3333be42aad2e47f1f8e0` / `0160f06954c88aba61392f72af66031d6f7ff4a592beb24f7ebe9f1981cc7a68` / `c10bcc96887aac7db8c855f19d991e6185f46d1df39f7a37d3a31cb4b9ca1b92`.
- Native fixture BLAKE3: `2f205db621b1b039d804d2b4ed6dfa2a000e2e09620c123f35c368c4a0043f73`.

## Durable and transport evidence

The mandatory PostgreSQL job passed migrations through `0054`, the complete death repository suite, retained live-trace promotion, danger entry/crash restoration, post-death production-service rejection, and the authenticated real-QUIC route. It includes:

- exact replay, altered-payload conflict, stale/foreign authority, response loss, process restart, database outage, serialization retry, corruption rejection, and injected rollback;
- 17 immutable terminal-history families with 34 rejected `UPDATE`/`DELETE` attempts;
- zero/ordinary/full custody, every destruction location family, empty/nonempty run pouch, and nonvacuous CharacterSafe/Vault preservation;
- a qualifying zero-custody death with one complete death/summary/memorial/Echo/receipt/outbox graph and zero item, material, checkpoint, ledger, or destruction rows;
- exact post-death item, progression, Bargain, and world-command rejection with no appended mutation state;
- normal-route and unsupported nonpermadeath paths remaining fail closed.

The archived [six-branch matrix](GB-M03-06E-death-branch-matrix.json) is accepted with the exact required branch set:

| Branch | Echo outcome | Echo rows/transitions | Target outbox rows |
|---|---|---:|---:|
| Level below 10 | Not eligible | 0 / 0 | 1 |
| Combat below 18,000 ticks | Not eligible | 0 / 0 | 1 |
| Missing qualifying deed | Not eligible | 0 / 0 | 1 |
| Verified server incident | Not eligible | 0 / 0 | 1 |
| Eligible self-promotion | Available | 1 / 2 | 3 |
| Eligible with existing Available | Dormant | 1 / 1 | 2 |

Every branch preserves its canonical signature through exact replay, leaves zero database/runtime residue, and reaches the client model in under two seconds. Across the six branches, terminal commit maximum is `152.894 ms`, exact replay maximum is `7.396 ms`, canonical-signature query maximum is `33.439 ms`, and commit-to-client-model-ready maximum is `13.437 ms`.

The archived [concurrent eligible-death report](GB-M03-13-concurrent-eligible-deaths.json) is accepted: two distinct accounts commit and replay concurrently with exact per-account graphs, four unchanged signature checks, zero cross-account rows, `362.242 ms` combined commit, `4.474 ms` combined replay, and zero transaction/lock residue. Same-account duplicate writers serialize through the durable final-death identity; the repository's pure locked-state selector separately proves multiple Dormant candidates use exact `(created_at, echo_id)` ordering, including equal timestamps.

## Performance and soak

- Checked-in [latency report](../performance/GB-M03-06E-death-latency.json): 10 measured death journeys; terminal commit `87.065/133.060/133.060 ms` median/p95/max; exact replay `3.061/7.704/7.704 ms`; acknowledgement-to-interactive `7.454/12.868/12.868 ms`; zero transport/session/transaction/lock residue.
- Explicit release-profile soak run [`29489909161`](https://github.com/MikeyPar/Gravebound/actions/runs/29489909161): [30-minute report](../performance/GB-M03-06E-death-memory-soak.json) accepted after `1,800,029 ms`, 8,509 query journeys, 34,036 death-view queries, 425 reconnects/replays/signature checks, stable resident memory with `561,152` bytes post-warmup growth, unchanged canonical signature, and zero final residue.
- These subsystem journeys are not mislabeled as the roadmap's final 25 death-to-successor private loops.

## Native artifact matrix

All reports declare `accepted=true`, exact content/build identity, `destination=death_final`, focused `Inspect Damage Trace`, and the requested viewport/effects mode.

| PNG | Mode | SHA-256 | Report SHA-256 | Inspection |
|---|---|---|---|---|
| [1280x720 standard](GB-M03-06E-native-standard-1280x720.png) | Standard | `c0e23f8903ac4b6a3fffdaaeeff2c7bb8d3bc8a5942ece980dc8c97ffeec31e27` | `2a8e32b2857666aef60937748d419a3c85e87869cadcfffb37407e749ba515b0` | PASS |
| [1280x720 reduced](GB-M03-06E-native-reduced-1280x720.png) | Reduced effects | `c0e23f8903ac4b6a3fffdaaeeff2c7bb8d3bc8a5942ece980dc8c97ffeec31e27` | `c0235ec3827ea7ab45b0df5b8b40fe6c08a7e1ac644682b4af007529184b04fe` | PASS |
| [1920x1080 standard](GB-M03-06E-native-standard-1920x1080.png) | Standard | `01d34e569b97c281366a1bb01100e67ccacd3b8bd15c8f413151ebbb5e899d44` | `0d32ac5266f981ca4c80031b0aba4dec14fef9f5013b809fc229bf98787da658` | PASS |
| [1920x1080 reduced](GB-M03-06E-native-reduced-1920x1080.png) | Reduced effects | `b82365ad940658ea18c0471a592117f49d5ef0d74b9ff8dca14a5cbe337972b7` | `a77a5f34fa4866ff225d5dc2b74560bf7465d91ef60c707e556c698b41054859` | PASS |

Source fixture and report hashes:

- [Branch matrix](GB-M03-06E-death-branch-matrix.json): SHA-256 `735c5a3bcfc8ca429f500822c29f0867a04022f878cc2c4fe5423c12461c12ef`.
- [Native frame fixture](GB-M03-06E-native-death-frame-fixture.json): SHA-256 `c99ff702d1f84ac5e26286a49acd98ac9b3316d120525d664f310d9aa09c04cb`.
- [Concurrent Echo report](GB-M03-13-concurrent-eligible-deaths.json): SHA-256 `c63ac219f73d2f941b40773632096d1499b6cb4d0a123c624c146e032141b718`.

## Inspection record

- All four PNGs decode at the requested dimensions as opaque RGBA with full RGB extrema and at least `99.9661%` nonblack framebuffer coverage.
- Original-resolution standard captures pass hierarchy, exact cause/trace order, readable loss/preserved/created cards, explicit Available Echo result, disabled primary successor action, enabled read-only secondary actions, and absence of commerce.
- Standard and reduced 1280x720 captures are byte-identical. At 1920x1080, only 330 pixels differ, the maximum channel delta is 10, and zero differing pixels reach intensity 20. Semantic content and layout are identical.
- The desktop image preview may display reduced captures as black; numeric RGB/alpha inspection and direct file decoding prove the archived images contain the complete frame. This is an inspection-tool defect, not a game capture defect.
- Native probe time covers model-ready through focus, activation, and screenshot capture and is not the `DTH-021` acknowledgement-to-interactive SLA. The dedicated durable/QUIC latency report supplies that SLA and remains below two seconds.

## Scope boundary

This evidence closes durable death, deterministic destruction, Memorial, Echo projection, exact replay/restart, and native summary/Memorial integration. It does not enable successor creation, extraction/Recall, normal route admission, Requiem encounters, telemetry, support tooling, Steam/platform actions, human comprehension measurement, or the final complete-loop cohort.
