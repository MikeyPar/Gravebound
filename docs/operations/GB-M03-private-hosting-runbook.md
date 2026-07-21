# GB-M03 private-test hosting runbook

This runbook implements the operating contract in [`ADR-038`](../decisions/ADR-038-private-test-hosting-and-platform-boundary.md). It is intentionally provider-neutral until an owner selects an account and region. It does not authorize a production deployment or promote Core content.

## Authority check

Before each rehearsal or cohort deployment, record the reviewed revisions of:

1. `Gravebound_Production_GDD_v1_Canonical.md`
2. `Gravebound_Content_Production_Spec_v1.md`
3. `Gravebound_Development_Roadmap_v1.md`

Reject deployment when the built Core content revision, database migration head, or route capability set differs from the reviewed release manifest.

## Required owner inputs

- Cloud/provider account and billing owner
- Region selected for the private cohort
- Private-test DNS zone and hostname
- Encrypted remote IaC state backend
- Secret and certificate stores
- Managed PostgreSQL plan supporting the ADR recovery targets
- Backup/object store and retention controls
- Metrics/logs/alerts destination
- On-call owner and private tester allowlist

Never place values for these inputs in a committed variable file. Commit only non-secret schemas and examples after provider selection.

## Pre-deploy gate

- [ ] IaC provider/module versions and artifact hashes are pinned.
- [ ] The plan changes only the `private-test` environment.
- [ ] UDP 443 is the only public application ingress; PostgreSQL has no public route.
- [ ] TLS 1.3 certificate is valid beyond the cohort window and renewal is monitored.
- [ ] Application, migration, backup, and operator identities are separate and least-privilege.
- [ ] Database backup is current and a point-in-time target is recorded.
- [ ] Build identity, content revision, schema head, and tester-package SHA-256 are recorded.
- [ ] M04+ capabilities, Steam runtime integration, production namespace, and public admission are disabled.
- [ ] Budget alerts exist at USD 75, 120, and 150; the USD 300 admission stop is documented.
- [ ] Telemetry can be disabled or unavailable without blocking gameplay.
- [ ] Support lookup has an authenticated operator and cannot mutate gameplay state.

## Deployment

1. Close new admission and verify no unresolved terminal transaction is being deliberately interrupted.
2. Verify the pre-deploy backup/restore point.
3. Run the single migration job and record its immutable execution ID.
4. Start the candidate artifact with route admission disabled.
5. Check QUIC/TLS, PostgreSQL, content revision, schema head, route readiness, and actor-residue baseline.
6. Enable the tester allowlist, then move the private-test traffic target.
7. Observe for 15 minutes. Stop rollout on admission failures, transaction errors, crash increase, actor residue, or database saturation.
8. Retain the prior immutable application until the observation window passes.

## Rollback

1. Close new admission.
2. Move traffic to the prior schema-compatible artifact.
3. Do not reverse or edit migration history.
4. If the prior application cannot safely read newly committed rows, keep admission closed and deploy a forward-fix.
5. Confirm stored extraction, Recall, death, Memorial, Echo, and successor outcomes replay before reopening.
6. Record trigger, build/content/schema identities, timeline, impact, and follow-up owner without player secrets.

## Restore rehearsal

1. Create an isolated restore target from a selected point no older than 15 minutes before the rehearsal start.
2. Deny public ingress and use a dedicated restore-only identity.
3. Verify the migration head and Core content revision.
4. Compare bounded aggregate counts and immutable ledger continuity with the source checkpoint.
5. Verify one qualifying death has death, destruction, Memorial, and Echo state atomically; verify an exact terminal retry returns its stored result.
6. Start the application with admission disabled and confirm the restored route reaches ready state.
7. Record achieved RPO/RTO, provider backup ID, restore target ID, build identity, schema head, verification hashes, and reviewer.
8. Destroy the isolated target through the provider-native reviewed workflow after evidence retention.

## Evidence record

Evidence must be redacted and contain no account token, operator token, database URL, IP address, private hostname, certificate private material, cloud account number, or Steamworks confidential data. Record:

- UTC start/end
- accountable owner roles
- reviewed commit and authority revisions
- provider and region at a non-secret level
- artifact/content/schema identities
- IaC plan/apply IDs
- certificate expiry status
- backup and restore IDs
- achieved RPO/RTO
- rollback result
- observability and budget-alert result
- issues, disposition, and reviewer sign-off

## Current Next Step

Select the provider and region, author the provider-pinned IaC module against `ADR-038`, then run deployment rollback and isolated PostgreSQL restore rehearsals. Keep private cohort admission closed until those artifacts and the owner-supplied Steamworks evidence are attached to `GB-M03-14`.
