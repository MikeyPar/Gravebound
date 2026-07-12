# ADR-017 — In-place First Playable Bell content correction

- **Status:** Accepted owner exception
- **Date:** 2026-07-11
- **Owner:** Product
- **Scope:** `fp.1.0.0`, `CONT-FP-001`, `CONT-FP-005`, `GB-M01-04B/04C`

## Authorities reviewed together

- `Gravebound_Production_GDD_v1_Canonical.md`: `TECH-001`, `COM-002` through `COM-006`, `PRD-123`.
- `Gravebound_Content_Production_Spec_v1.md`: `CONT-001` through `CONT-013`, `CONT-FP-001`, `CONT-FP-005`, `CONT-FP-009`.
- `Gravebound_Development_Roadmap_v1.md`: M01 packages 04, 05, 08, and implementation order 21–23.

## Context

The checked-in `fp.1.0.0` package was validated and treated as immutable while omitting the Bell Proctor record and its three required patterns, even though the authoritative First Playable scope explicitly includes them. A new bundle ID would preserve ordinary promotion immutability but would make the named First Playable contract and its existing local fixtures disagree.

## Decision

The owner explicitly authorizes one in-place correction to `fp.1.0.0`: add the missing strict Bell Proctor domain record and its three pattern records, extend the exact manifest/assets/localization/schema closure, and refresh derived hashes/evidence. No unrelated balance or content changes are included.

The Bell fan remains raw `12`, Veil, `Chip`. The earlier Pressure recommendation omitted the Arbalist's 2 armor. Exact `COM-002` resolution is final damage `10`; `10/128=7.8125%`, which is `Chip` under `COM-003`. Its overlapping casts require maximum active instances `10`.

## Consequences

- Enabled record count becomes 34: class 1, abilities 4, normal enemies 3, boss 1, patterns 6, arena 1, items 13, reward tables 5.
- Old package hashes and captures that display 30 records remain historical evidence, not the corrected package identity.
- All current validation, release, deterministic, and milestone evidence must use the corrected hash before promotion.
- Future promoted-bundle bytes remain immutable; this exception does not establish a general in-place migration policy.

## Validation

- Strict boss schema and exact lossless compiler into `BellProctorDefinition`.
- Exact authored-millisecond, identity, reference, pattern, threshold, timeline, and cap drift rejection.
- Full-reference six-attack damage-band fixture including health and armor.
- Cumulative content validation, deterministic traces, tests, optimized build, and refreshed evidence.
