# SPEC-CONFLICT-011 — M03 Oath/Bargain content closure

**Status:** Approved by owner on 2026-07-12

**Raised:** 2026-07-12

**Blocks:** Exact `GB-M03-05A` records/compiler closure and the affected `05C/05E` cadence composition; strict schemas may proceed independently

**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1, and approved `SPEC-CONFLICT-008`

## Context

The authorities exactly define the two Core Grave Arbalist Oaths, three Core Bargains, their numeric behavior, derived icon IDs, and initial warning value. A record-by-record implementation audit found three remaining authoring details that are not named precisely enough to claim exact compiler closure.

## Decisions requested

### 1. Exact capability/effect tag vocabulary

**Gap:** Approved `SPEC-CONFLICT-008` requires exact tags but does not name their lexemes.

**Recommended resolution:** Approve these sorted tag sets:

- `oath.arbalist.long_vigil`: `ability.mark`, `class.grave_arbalist`, `max_health_mod`, `oath`, `passive.stillness`.
- `oath.arbalist.nailkeeper`: `ability.mark`, `class.grave_arbalist`, `oath`, `outgoing.status`, `primary_cadence`, `trap`.
- `bargain.cinder_hunger`: `bargain`, `direct_output`, `max_health_mod`, `voluntary_risk`.
- `bargain.bell_debt`: `bargain`, `primary.repeat`, `primary_cadence`, `voluntary_risk`.
- `bargain.lantern_ash`: `bargain`, `belt_constraint`, `potion_output`, `voluntary_risk`.

The compiler rejects any missing, extra, duplicate, or reordered tag. These tags describe capabilities/effects only and grant no undocumented behavior.

### 2. Nailkeeper and Bell Debt cadence composition

**Gap:** Nailkeeper multiplies primary interval by `1.08`; Bell Debt reduces primary attack rate by `15%`. Both occupy the Oath/Bargain resolution step, but their intra-step order is unspecified.

**Recommended resolution:** Resolve all ordinary attack-rate bonuses/penalties into the legal rate denominator first, including Bell Debt's `×0.85` rate. Divide the authored weapon interval by that resolved rate, then multiply the resulting interval by Nailkeeper `×1.08`; carry fixed-point precision and round only at the normal interval/tick boundary. This preserves the distinct authored semantics of “rate” and “interval” and is deterministic for future combinations.

### 3. Source-document feature IDs

**Gap:** `CONT-003` requests the nearest `CONT-*` heading, while the actual executable Oath/Bargain mechanics are authored in GDD `CLS-020` and `BRG-003`. Existing unpromoted Core records cite their true GDD owner when that is the normative source.

**Recommended resolution:** Use `CLS-020` for the two Oath records and `BRG-003` for the three Bargain records. The 05A task and compiler continue to cite the Content specification for stage allowlists, derivation, localization, and validation. This records semantic ownership without inventing a nonexistent Content heading.

## Approval requested

Approve all three recommendations, or provide amendments. Approval permits exact 05A records/assets/localization/compiler closure and fixes the later 05C/05E cadence-order fixture without enabling persistence, interaction, Ash, or the normal route.

## Decision

The owner approved all three recommendations on 2026-07-12 without amendment.
