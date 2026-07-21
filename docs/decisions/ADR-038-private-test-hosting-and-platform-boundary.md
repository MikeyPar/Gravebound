# ADR-038 - Private-test hosting, infrastructure, and platform boundary

**Status:** Accepted on 2026-07-21

**Owners:** Runtime, persistence, operations, security, release engineering, product owner

**Applies to:** `GB-M03-14`, the private M03 cohort, and the hosting foundation inherited by later milestones

## Authorities

- `Gravebound_Production_GDD_v1_Canonical.md`: `PRD-020`-`022` establish the native Windows-first release order, private Core scope, and the later Steam Playtest target; `TECH-010`-`023` require an authoritative, observable, fail-closed service.
- `Gravebound_Content_Production_Spec_v1.md`: the Core stage manifest, `CONT-HUB-001`-`002`, and Core content gates define the only content permitted on the M03 private service. Closed or later-stage content is not promoted by deployment configuration.
- `Gravebound_Development_Roadmap_v1.md`: `GB-M03-14` requires Steamworks partner/legal/platform setup and a hosting/IaC ADR without introducing a Steam runtime dependency. The M03 exit gate still requires real private-test evidence.

## Context

M03 needs a small, recoverable private service, not a premature production estate. The current Core namespace is explicitly wipeable, the player route is capacity-one per account, and public realm scheduling is out of scope. Hosting must nevertheless establish professional controls for encrypted transport, durable PostgreSQL, repeatable deployment, backup recovery, observability, and spend.

Steamworks onboarding is an owner-controlled legal and financial action. Repository code cannot accept agreements, submit identity/tax/bank data, pay an application fee, or create private partner records. Those actions must be evidenced without committing secrets or personal information. Steam runtime authentication, overlay, achievements, cloud saves, matchmaking, and SDK linkage remain outside M03.

## Decision

### Environment topology

Three isolated environments are defined:

| Environment | Purpose | Data policy | Admission |
|---|---|---|---|
| `local` | Developer and disposable PostgreSQL work | Synthetic, wipeable | Loopback only |
| `private-test` | M03 owner-approved cohort | Wipeable Core records with encrypted backup | Explicit allowlist |
| `production` | Reserved for the later namespace cutover | No M03 deployment | Disabled |

`private-test` uses one regional application service and one regional managed PostgreSQL service. A second application instance may be added for rolling replacement, but the private-life actor and database remain authoritative; a load balancer never creates an alternate writer. Environment identifiers, database roles, certificates, backup stores, telemetry destinations, and DNS names are not shared.

### Infrastructure as code

- Reproducible infrastructure is declared with provider-pinned Terraform/OpenTofu modules before a hosted cohort is admitted. The committed configuration contains no credentials, partner IDs, account numbers, private DNS zone identifiers, or state files.
- Remote state must be encrypted, versioned, access logged, and locked. A bootstrap administrator is separate from the least-privilege deployment identity.
- Immutable application artifacts are addressed by content hash and build identity. Deployment never compiles on the host.
- Configuration is schema-checked at startup. Missing secrets, unknown stage/content revision, unavailable PostgreSQL, or an unapplied migration fails startup before route admission.
- Core content promotion and database namespace promotion are separate explicit changes. Infrastructure cannot enable M04+ features or turn the wipeable Core namespace into production data.

Provider-specific modules are intentionally deferred until the owner selects an account and region. The provider selection must satisfy this ADR without revising its security, recovery, or cost gates.

### Network, TLS, and DNS

- Gameplay QUIC is exposed only on UDP `443`; administrative access is private-network or identity-aware proxy only. PostgreSQL is never publicly reachable.
- QUIC uses TLS 1.3 with a publicly trusted certificate for the private-test hostname. Certificate issuance and renewal use a least-privilege DNS challenge identity; private keys live in the platform secret store and are never committed or included in tester packages.
- DNS uses a dedicated `private-test` record with a 60-second cutover TTL. Environment hostnames do not alias production. Health checks validate both transport establishment and route readiness before traffic changes.
- Network policy defaults to deny. Explicit egress is limited to DNS/time, the selected secret/certificate services, PostgreSQL, backup storage, and the approved telemetry destination.
- Account tokens, operator tokens, database credentials, certificate material, and Steamworks credentials must never appear in URLs, process arguments, logs, crash payloads, telemetry, support results, evidence manifests, or repository history.

### PostgreSQL durability and recovery

- `private-test` PostgreSQL uses encrypted storage, encryption in transit, automated daily full backups, continuous write-ahead-log retention when the provider supports it, and seven-day backup retention.
- M03 targets an RPO of 15 minutes and an RTO of 60 minutes. A restore rehearsal into an isolated database must prove schema version, content revision, row counts, death/memorial/Echo atomicity, terminal replay, and application admission before GB-M03 can close.
- Restores never overwrite the source service. DNS and route admission remain closed until the restored service passes integrity checks.
- Migrations are additive and applied by a single migration job before application rollout. A failed migration stops deployment. Application rollback must remain compatible with the current schema and the immediately preceding schema; database history is never rewritten.
- Backup credentials are restore-only outside the scheduled backup identity. Deletion requires an owner and an operations approver, with provider retention or object lock enabled where available.

### Deployment and rollback

1. Build and sign/hash the Windows client and server artifacts in CI.
2. Validate the exact Core content revision and migration plan.
3. Take or verify a recoverable pre-deploy database point.
4. Apply additive migrations once.
5. Start the candidate application with admission disabled and run readiness checks.
6. Move the private-test DNS/load-balancer target to the candidate.
7. Observe transport, database, terminal, and crash signals for 15 minutes before retiring the prior application.

Rollback changes traffic to the prior immutable application artifact. If a newly written row is unknown to the prior binary, route admission stays closed and recovery uses a forward-fix; destructive schema rollback is prohibited. A failed health check or error-budget breach automatically stops rollout and preserves the prior target.

### Observability and incident controls

- Minimum signals are process availability, QUIC handshake/session counts, simulation tick p95/p99, route actor count/residue, PostgreSQL connection saturation and transaction failures, terminal outcome counts, outbox lag, queue drops, crash fingerprints, and deployment/build/content identities.
- Logs are structured, UTC timestamped, bounded, and redacted. Durable domain IDs may be logged only where needed to correlate an incident; account credentials, network addresses, free-form player text, and full payload dumps are prohibited.
- Alerts cover sustained admission failure, database unavailability, terminal transaction failures, backup failure, certificate expiry, actor residue, crash-rate increase, and cost threshold breach. A designated owner receives each alert.
- The telemetry and support packages consume committed records through their approved read-only boundaries. Neither is a health dependency for gameplay and neither can mutate domain state.
- Incident response is: close new admission, preserve durable state and evidence, identify build/content/schema identity, restore or roll back, verify terminal replay, then reopen. Support cannot reverse permadeath.

### Cost ceilings

The private-test baseline ceiling is **USD 150 per calendar month** and the hard ceiling is **USD 300 per calendar month**. Budgets include compute, managed PostgreSQL, backup/object storage, traffic, DNS/certificates, and observability.

- Alerts fire at 50%, 80%, and 100% of the baseline ceiling.
- Crossing USD 150 requires a written owner decision with the cause and expiry date.
- Crossing USD 300 closes new cohort admission unless an incident would make shutdown unsafe; expansion requires a revised ADR.
- Idle nonlocal environments scale to the smallest safe footprint. Log retention, high-cardinality labels, unrestricted egress, and orphaned artifacts are budget-audited weekly during a cohort.

### Steamworks boundary

- The owner completes partner identity, NDA/distribution agreement, bank/tax verification, product fee, application creation, and access-role setup in the Steamworks partner site.
- Redacted evidence records only completion state, date, accountable owner role, application type, and non-secret identifiers explicitly safe to publish. Legal names, addresses, tax/bank details, credentials, recovery codes, and agreement screenshots are not committed.
- A later Windows Steam depot may package the same reviewed native build. M03 adds no Steam SDK, runtime library, Steam identity, overlay, lobby, achievements, cloud, inventory, or commerce dependency.
- Steam Playtest configuration and build/store review remain owner-controlled follow-up actions. They do not replace the private M03 cohort and cannot bypass server capacity or privacy gates.

## Acceptance evidence

`GB-M03-14` cannot close until all of the following exist:

- provider/region selection and reviewed IaC plan;
- private-test TLS/DNS, secret-store, least-privilege, budget, and alert evidence;
- successful deployment rollback and PostgreSQL restore rehearsal;
- redacted Steamworks onboarding/application evidence from the owner;
- confirmation that the M03 client and server contain no Steam runtime dependency; and
- links from [`GB-M03-14`](../tasks/GB-M03-14.md) to the evidence without exposing secrets.

## Consequences

This architecture is intentionally small but operationally complete. It allows a private cohort to run on recoverable infrastructure, provides a controlled path to later scale, and keeps platform/legal authority outside game code. Provider choice, production namespace cutover, public Steam Playtest release, and all Steam runtime integrations remain separate future decisions.
