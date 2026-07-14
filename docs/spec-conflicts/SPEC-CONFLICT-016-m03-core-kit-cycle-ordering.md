# SPEC-CONFLICT-016 — M03 Core kit cycle ordering

**Status:** Owner-approved on 2026-07-13

**Raised:** 2026-07-13

**Blocks:** `GB-M03-03D` exact Core-authored kit schedulers

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The canonical GDD fixes the Sepulcher Knight's six-second charge cadence and 2.2-second Shield Fans between charges, but does not define the fan subcycle phase or equal-cycle priority. It also fixes the Choir Skull and Choir Abbot six-second rotor loops while the Content Production Specification adds first/repeated rotor previews. Serializing each repeated preview after the six-second boundary would extend the authored cadence and shorten its quiet recovery.

The Abbot additionally owns a 2.5-second recovery, a recovery-wide origin warning, a directional gap preview during the final 650 ms, a ring at recovery end, and a repeated 500 ms rotor-arm preview. Their overlap and equal-tick release order were not explicit.

## Approved resolution

1. Each Sepulcher Knight 180-tick loop starts the Charge Lane telegraph at offset `0`. Shield Fan telegraphs start at offsets `66` and `132`; the fan subcycle resets after every charge loop. Charge/stop-ring ownership remains parent-linked, and the fan's first/repeated warnings remain global to that actor life.
2. Choir Skull and Choir Abbot rotor releases remain exactly 180 ticks apart. Start repeated rotor previews 15 ticks before the next release boundary. The Skull's ten arm volleys use authored offsets `400..4000 ms`; the Abbot's ten use `350..3500 ms`, independently rounded to 30 Hz so 350 ms does not accumulate drift.
3. During the Abbot's final recovery, the 20-tick directional gap preview overlaps the final 15-tick repeated rotor-arm preview. At the boundary, emit the recovery ring before starting the next rotor; rotor volleys resume at the authored first 350 ms offset.

These decisions preserve authored damage, projectile geometry, six-second cadence, active duration, recovery duration, counterplay, and threat budgets.

## Approval record

The owner approved both recommendations without amendment on 2026-07-13.
