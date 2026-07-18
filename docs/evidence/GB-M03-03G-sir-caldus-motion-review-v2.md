# GB-M03-03G — Sir Caldus motion-strip review v2

## Decision

The unregistered review pack at [`assets/core/bosses/sir_caldus/review/v2`](../../assets/core/bosses/sir_caldus/review/v2/README.md) passes its static candidate review for two four-frame boss motion strips: Shield Arc and Charge Lane. It is not runtime evidence, does not amend a compiled asset record, and cannot alter the normal-route gate.

The evidence follows `ART-004`'s seed -> strip -> shared normalization -> preview inspection sequence, `ART-030`'s anchor/silhouette requirements, Sir Caldus's `CONT-BOSS-002` presentation contexts, and the M03 temporary-asset restriction in the roadmap.

## Inspection record

| Check | Result |
|---|---|
| Locked frame 01 | Pass: both strips use SHA-256 `da0c0b90…4ea5`, byte-identical to v1's guard seed. |
| Frame format / anchor | Pass: every accepted frame is 192 x 192 RGBA, transparent corners, one connected component, bottom-center anchor `(96,192)`. |
| Shield Arc readability | Pass at 192 px and nearest-neighbor 96 px: guard, raised shield, outward release, and recovery remain distinct. |
| Charge Lane readability | Pass at 192 px and nearest-neighbor 96 px: compact windup, down-screen travel, and braking recovery remain distinct. |
| Slot/gutter integrity | Pass for accepted strips; all nontransparent bounds remain inside the renderer frame. The first Charge Lane attempt was rejected because its travel frame touched the left edge. |
| Standard/reduced parity | Pass: each action frame is composited unchanged over the existing standard/reduced native evidence at 1280 x 720 and 1920 x 1080. |
| Review labeling | Pass: every camera mock is explicitly watermarked `REVIEW MOCK / UNREGISTERED / NOT RUNTIME`. |

The 1280 x 720 and 1920 x 1080 mocks use the existing static Caldus evidence as a backdrop, retain its symbolic center marker, and are not asserted to be native application captures. This prevents a review composite from being mistaken for gameplay evidence.

## Accepted artifact hashes

| Artifact | SHA-256 |
|---|---|
| Shield Arc raw chroma | `58ac656da4f53764653832a9cfbefd336f572186d1085bda8c90c5e7890a0955` |
| Shield Arc alpha source | `07e6d48810c9577fd39dce97e40f0ca554067845ca5bbcf10ee096bb36b80814` |
| Charge Lane raw chroma | `e499bb417dcbd0691165adba4907b8f12ccb0de1281115852bf0331289e3c43c` |
| Charge Lane alpha source | `1191076867a11f94f49ec2acbeb8fef2b95c0ba83acdf4c5355b9c831d1460a3` |
| Shield Arc 192 px contact sheet | `3279447928e5d1075af7d7046ff9d26371c4f6e8a49a3086b878c414bb4a92bf` |
| Charge Lane 192 px contact sheet | `b2684d5fe7b88e35a6dea3fed3fa19855dcaf8e077ce323a114df4ed5359c05f` |
| Shield Arc 96 px contact sheet | `6523ede7ae9810c42227b8c068b87e01321251f4bd9cdb3cadbddf6c11844bf5` |
| Charge Lane 96 px contact sheet | `00ba295156091524047dd96da0e92f8984ddd11c2d8dcb7fcca1956075448926` |

The provenance JSON records all accepted frame and source hashes; the pack retains the failed first Charge Lane candidate in a rejected path with its reason.

## Remaining gate

The next legitimate art action is a separate in-engine pass: import no content yet, exercise the candidate animation state machine against the authoritative Shield Arc/Charge timelines, inspect anchor drift and authored attack origin/hurtbox alignment in motion, then capture optimized native standard/reduced evidence. Only after that independent pass may the team propose a registry/content-hash change.
