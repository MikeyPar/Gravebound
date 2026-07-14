# GB-M03-04E native equipment visual matrix

## Authority and method

This evidence applies the Canonical GDD `UI-001/003/006/011/030` and `LOOT-001/002`, the Content Production Spec `CONT-ITEM-001` through `CONT-ITEM-004` plus `manifest.items.core_18`, and Roadmap `GB-M03-04`. Captures came from the optimized native Bevy executable after 90 settled frames. The runtime sheet is mechanically derived from the approved SVG and startup rejects either artifact if its pinned BLAKE3 changes.

Command shape:

```text
target/release/client_bevy.exe core-equipment-showcase --content-root content --state <comparison|icon-matrix> [--reduced-effects]
```

## Matrix

| State | Effects | Resolution | SHA-256 |
|---|---|---:|---|
| Comparison | Standard | 1280x720 | `0e5feef4577cd19c6218e14ee5b4025cc9e0ff726de3774259e9a83250ef6622` |
| Comparison | Standard | 1920x1080 | `239c31cfac9b6bbf3f103f853a2d15179743f3ccb5da9bd331d139e4afc1fce7` |
| Comparison | Reduced | 1280x720 | `67ce8530b8cd0f9b2bea6bc2f2cbf1691f1798bfaa6004d0f705fa46cec4337d` |
| Comparison | Reduced | 1920x1080 | `b1da42a6ef87fb317f8befa4186cfd6d7129a1be267de8d7e50700f710770064` |
| 18-icon matrix | Standard | 1280x720 | `8b5aff76169b1b75e56bc4b8b250ca7a8127dd8e87df6877419aa971596741e0` |
| 18-icon matrix | Standard | 1920x1080 | `206bb5344586650125886ba2d74f12af7dbe4cfab4830eb51916f9a014b5e0d1` |
| 18-icon matrix | Reduced | 1280x720 | `03b00043e3b4207ed8ac23e3f970648dbb2e2082c90f9ac174f1ad5d18e8180e` |
| 18-icon matrix | Reduced | 1920x1080 | `391a56463ecdfed2ca2553be8856925f56253d3d34be3ee47033963fac9df5a7` |

## Review result

- All 18 strict Core catalog entries render once from their real 6x3, 64x64 source cells; localized names remain readable at the 720p floor.
- The comparison overlay occupies the right edge and leaves the center/lower-middle playfield clear. `I`/`Tab` explicitly leaves the online world running.
- Four equipped cells, eight pending cells, the exact pending-loss warning, behavior-first differences, explicit confirmation, in-flight lock copy, cancel affordance, and the exact replacement destination are visible without scrolling at 1280x720.
- Standard and reduced-effects modes preserve layout and information. The review surface is static in both modes, so no required state is motion-only or color-only.
- Source SVG BLAKE3: `19d49b684fd2b78c84b7aee67b0f94dcc9f8f061acff0ec9c81882bddd2cf9f5`.
- Runtime PNG BLAKE3: `c48daa7c1e7d7e054dd94480031e636a7a892af19d25c5b5091e0b03c55b8da7`.

Result: PASS for the 04E native presentation and icon-readability gate. This disposable review route is evidence only; normal-route admission remains closed.
