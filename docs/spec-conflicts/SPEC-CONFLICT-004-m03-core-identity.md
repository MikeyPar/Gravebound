# SPEC-CONFLICT-004 — Core identity and character-select contract

**Status:** Owner decision required  
**Raised:** 2026-07-12  
**Blocks:** `GB-M03-01`  
**Authorities reviewed:** canonical GDD, Content Production Specification v1, Development Roadmap v1

## Context

The roadmap authorizes a wipeable test identity, character creation/select, and one class in `GB-M03-01`. The GDD requires the `UI-007` and `UI-008` character surfaces. The content specification fixes the Core class allowlist to `class.grave_arbalist`, but does not supply every presentation or validation record those surfaces require.

No implementation may invent the missing production rules. The decisions below keep the Core test namespace honest and avoid pulling PostgreSQL, items, Lantern Halls, or Early Access cosmetics into the first M03 package.

## Decisions requested

### 1. Core appearance

**Conflict:** `UI-008` requires an appearance choice. The only exact `appearance.default.grave_arbalist` record is explicitly `release_stage=ea` and provisioned at `GB-M08-00`.

**Recommended resolution:** For Core only, display the existing `sprite.class.grave_arbalist` base silhouette as a locked, non-entitlement preview. Store no appearance entitlement or production appearance ID. Add the actual appearance field when its stage-legal catalog exists. This is a presentation placeholder, not a content record and cannot migrate.

### 2. Hero epithet validation

**Conflict:** `UI-008` requires a validated optional hero epithet, but no authority defines length, normalization, allowed characters, reserved terms, profanity data, or impersonation policy.

**Recommended resolution:** Defer editable epithets in Core. Derive the card name from the wipeable account display label and a deterministic roster ordinal (`Hero 1`, `Hero 2`) using localized Core UI keys. Keep the protocol field absent, not empty. Add authored name policy before enabling player-entered names.

### 3. Class card and preview clips

**Conflict:** `UI-008` requires difficulty, range, survivability, primary-verb labels and two 15-second preview clips, but supplies no exact categories, copy, scripts, or assets.

**Recommended resolution:** Show the one enabled class with mechanically sourced class/ability localization and existing base sprite. Mark the two preview cells `AVAILABLE IN A LATER TEST` using the content specification's exact closed-feature literal. Do not invent ratings or clips.

### 4. Deferred character-card fields and Play

**Conflict:** `UI-007` requires equipped item-power band and primary `Play`. Starter items belong to `GB-M03-04`; Hall/control routing belongs to `GB-M03-03`.

**Recommended resolution:** Show item power as `NOT EQUIPPED` and Play as disabled with `AVAILABLE IN A LATER TEST`. Permit authoritative selection without world transfer. `GB-M03-03` enables Play; `GB-M03-04` replaces the item placeholder with computed data.

### 5. Boot order

**Conflict:** `LOOP-001` places Accessibility Quick Setup before authentication. `UI-002` orders authentication before optional Accessibility Quick Setup.

**Recommended resolution:** Follow `UI-002`'s explicit screen-state contract for Core: `Boot → PatchCheck → Authentication → CharacterSelect`. Preserve the existing M01 accessibility settings and expose them from authentication/select; do not add a new mandatory setup screen in M03.

### 6. Core content cutover

**Conflict:** `core.1.0.0` is the exact promoted M03 bundle, but its full manifest spans later M03 work packages. Promoting incomplete bytes would violate `CONT-VALID-003`; continuing to mutate a promoted `core.1.0.0` would violate `CONT-002`.

**Recommended resolution:** Keep `fp.1.0.0` byte-for-byte immutable. `GB-M03-01` introduces a clearly unpromoted `core-dev` compilation target that accepts only stage-complete subsets for development and cannot produce a promotion record or release package. The exact `core.1.0.0` identifier is assigned once the complete Core manifest and promotion gates pass.

### 7. Focused M02 retest

**Conflict:** The M02 gate audit authorizes M03 through an explicitly labeled owner-assumed human pass, while README additionally asks for a focused corrected-package owner retest. Four packaged clients and the server launched on 2026-07-12, but Windows Graphics Capture failed with `0x80004002`, so no new visual/pickup/reconnect result can be claimed.

**Recommended resolution:** Allow engineering to begin `GB-M03-01` under the already-recorded owner-assumed M02 gate. Keep the focused manual retest open as an M02 follow-up and do not reuse its assumption as measured M03 evidence.

## Approval record

The owner may approve all recommended resolutions together or replace individual numbered decisions. Once approved, record the date and exact disposition here before implementation begins.

