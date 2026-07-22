# GB-M03-10 completion audit

## Result

PASS. Gravebound now has an authenticated, read-only, least-privilege support boundary for exact character, item, and death lookup. Results are bounded, audit-before-disclosure, durable-ID based, and structurally unable to mutate gameplay state or expose credentials, platform identities, network addresses, unrestricted scans, or localized reconstruction.

## Three-authority review

| Authority | Implemented evidence |
| --- | --- |
| `Gravebound_Production_GDD_v1_Canonical.md` | `TECH-050`, `TECH-120`, `TECH-122`, `TECH-124`, and `TECH-125` are satisfied by staff authentication, exact-ID queries, least-privilege database roles, immutable access audits, redacted credentials, item provenance, and death-trace reconstruction without direct database edits. |
| `Gravebound_Content_Production_Spec_v1.md` | `CONT-LOC-001` remains the only localization authority. Support returns stored stable IDs, versions, numeric states, content revisions, and digests; it never invents or reconstructs localized player copy. |
| `Gravebound_Development_Roadmap_v1.md` | `GB-M03-10` delivers support/debug lookup for character, item, and death. Broader account, Echo, transaction, party, entitlement, moderation, and write-command scope remains deferred to its later roadmap owners. |

## Acceptance evidence

| Requirement | Evidence | Result |
| --- | --- | --- |
| Authentication and secret hygiene | Opaque bounded operator tokens are domain-hashed, constant-time compared, redacted from `Debug`, and wiped on drop. Disabled, unknown, or wrongly authorized operators fail closed. | PASS |
| Exact bounded lookup | Requests accept one typed target and one exact nonzero ID. Character, item, and death results expose a maximum of 64 transitions plus a nondisclosed truncation probe; wildcard, substring, account scan, free text, and caller-selected limits do not exist. | PASS |
| Audit before disclosure | A result is returned only after the append-only audit insert commits. Duplicate request IDs, audit update/delete, and missing audit authority fail closed. | PASS |
| Least-privilege PostgreSQL surface | Security-barrier views and exact-ID `SECURITY DEFINER` functions expose only approved fields. Public execution is revoked; the support role can execute the bounded functions and insert audits, but cannot select protected views/base tables or mutate gameplay/audit state. | PASS |
| Safe durable reconstruction | Character history, item custody/provenance, and death terminal/trace authority use stored IDs, versions, digests, and transitions. Authentication, platform, network, raw mutation payload, and localized fields are absent. | PASS |
| Hosted database proof | The schema-correct PostgreSQL 17.10 journey creates canonical Oath/content digest fixtures, verifies exact hit/miss/truncation behavior, audit durability, duplicate rejection, append-only enforcement, allowed grants, prohibited direct access, and cleanup. | PASS |

## Verification

- [Hosted CI run 29897225480](https://github.com/MikeyPar/Gravebound/actions/runs/29897225480), exact source `efaae92`: `postgres_least_privilege` passed 1/1 against PostgreSQL 17.10 in 1.22 seconds.
- The same exact source passed formatting, warnings-denied lint, and optimized Windows release construction. The workflow's eventual red result came from later unrelated server tests; no support lookup check failed.
- Focused local authorization/bounds/redaction tests passed 6/6; strict package Clippy and PostgreSQL target compilation passed.
- Granular commits: `d273db5` (bounded support package), `cb91582` (database isolation gate), `14aa194` (canonical Oath fixture), and `efaae92` (byte-accurate content digests).

## Outcome and Current Next Step

`GB-M03-10` is complete. Keep the surface read-only and limited to character, item, and death while M03 closes. The milestone Current Next Step is the schema-70 telemetry/runtime gate and the production-root B0–B6/Caldus/extraction/death/successor journey, followed by the 25-loop matrix and external cohort/operations evidence. All remaining work continues under the same three design authorities.
