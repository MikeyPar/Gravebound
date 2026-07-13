# ADR-033 — Persisted Oath combat construction

Status: Accepted

Implementation package: `GB-M03-05C` / `GB-M03-04D`

## Context

The canonical GDD makes the selected character-life Oath authoritative for Grave Arbalist combat, while item rarity, item level, affixes, and global caps participate in the same resolved-stat pipeline. The Content Production Specification fixes the exact Long Vigil and Nailkeeper mechanics and requires deterministic units and rounding. The Development Roadmap requires both exact Oaths in M03 but keeps the normal route closed until the complete private-life gate passes.

Identity, progression, Oath, and equipped weapon data live in different normalized tables. Independently timed reads could construct combat from a character/Oath pair and a weapon projection that never coexisted. The current durable item stage stores starter and reward rarity but does not yet persist resolved affix identities and values.

## Decision

1. Persistence exposes one combat-loadout read model populated by a single `PostgreSQL` statement snapshot. It joins selected character identity, class, level, life/security state, character version, Oath, inventory version, and the live equipped weapon.
2. Persistence validates storage shape only. Selection, class, life, security, Oath, inventory, content revision, and combat-stage eligibility remain server-owned decisions.
3. The server factory rejects unselected, non-Arbalist, dead, unresolved, Oathless, inventory-less, weapon-less, revision-mismatched, and unknown-content projections. It never substitutes a default Oath or weapon.
4. The exact persisted Oath ID is passed into the immutable Oath compiler. The resulting weapon cadence, Grave Mark, Slipstep, Stillness, maximum-health multiplier, and `PlayerCombatState` are constructed together.
5. Item rarity and weapon-W affix contribution are explicit compiler inputs. The existing baseline compiler remains a Forged/zero-affix compatibility wrapper for exhaustive content fixtures.
6. Until `GB-M03-04F` persists resolved affix identities and values, the server factory accepts only the exact Worn starter weapon. Rolled reward weapons return typed `rolled weapon stage disabled`; guessing zero affixes for them is forbidden.
7. The persistent real-QUIC fixture selects Long Vigil and asks the production combat factory to construct the character both before and after server restart. It asserts the exact Oath, level, and 0.90 maximum-health multiplier.
8. Combat feedback remains presentation-only. Trap arm, trigger, and Frostbind-immunity cues are distinct deterministic audio signals; immunity also has textual feedback. Reduced motion preserves the exact trap radius and armed/arming shape distinction while removing transient scaling.

## Consequences

- Combat cannot combine mutually inconsistent durable projections or silently run without the selected Oath.
- Long Vigil and Nailkeeper enter the same authoritative construction path; client presentation never selects mechanics.
- Rolled items remain safely gated until their complete resolved-stat persistence contract exists.
- The normal route remains disabled; constructing a tested authority object does not admit a combat session.
- Live `PostgreSQL` execution and inspected audiovisual evidence remain closure gates in their task records.
