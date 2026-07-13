# SPEC-CONFLICT-013 — M03 item UID and consumable placement contract

**Status:** Approved by owner on 2026-07-12

**Raised:** 2026-07-12

**Blocks:** Durable starter initialization and production reward placement in `GB-M03-04D`

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1, approved `SPEC-CONFLICT-007`, and approved `SPEC-CONFLICT-012`

## Context

The authorities require immutable item identity, deterministic reward replay, distinct provenance for every consumable unit, two starter Red Tonics in the first Belt slot, and pending-inventory placement before a 60-second personal ground fallback. They do not specify the durable UID width or byte derivation. They also do not explicitly say whether a reward Tonic may enter a Belt slot before pending inventory.

## Approved contract

1. Every durable equipment instance and consumable unit has one opaque unsigned 16-byte UID. UIDs have no timestamp, shard, account, character, template, or sequence semantics.
2. Reward UIDs use BLAKE3 key derivation with the exact context string `gravebound.item-uid.v1`. Input fields are, in order, reward request ID, unsigned little-endian 16-bit reward-roll index, and unsigned little-endian 16-bit unit ordinal. Every field is encoded as its canonical bytes preceded by an unsigned little-endian 32-bit byte length.
3. Starter UIDs use the distinct exact BLAKE3 context string `gravebound.starter-init.v1`. Input fields are, in order, character ID, immutable starter-schema revision, item template ID, and unsigned little-endian 16-bit unit ordinal, using the same length-delimited encoding.
4. The UID is the first 16 bytes of the 32-byte derived output. An all-zero result or collision fails the containing transaction closed. No retry substitutes entropy, increments an ordinal, or rewrites an existing UID.
5. Starter initialization places its two distinct Red Tonic units together in Belt index `0`; Belt index `1` remains empty. This is the zero-based database/runtime representation of the authorities' “Belt slot 1” and “Belt slot 2” language.
6. A reward Tonic never auto-fills the Belt. It first merges into matching non-full `RunBackpack` stacks in ascending slot order, then uses the lowest empty `RunBackpack` slot. Units within every projected stack remain ordered by ascending unsigned UID.
7. When no legal `RunBackpack` capacity remains, the complete unplaced reward quantity becomes `PersonalGround` for the recipient with the approved 60-second expiry. Placement never silently truncates a quantity or splits overflow into the Belt.

## Rationale

Domain-separated derivation makes starter and reward identities reproducible without leaking creation metadata into the UID. Length-delimited fields prevent ambiguous encodings. Pending-inventory-first reward placement preserves the GDD security model and keeps Belt contents under explicit player control, while the starter exception satisfies the exact initial loadout.

## Decision

The owner approved the complete contract on 2026-07-12 without amendment.
