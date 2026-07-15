-- GB-M03-06B retained live damage-trace ingestion authority.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-001/DTH-020 and TECH-020..023;
-- - Gravebound_Content_Production_Spec_v1.md Core encounter/content authority;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06/13 restart and exact-replay gates;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md decision 4.
--
-- Migration 0043 owns the normalized, prunable 300-tick payload. This forward-only migration
-- retains the server command/result receipt independently, so pruning a complete old tick never
-- destroys exact-replay evidence. No client-authored destination or gameplay authority is stored.

CREATE TABLE character_live_damage_trace_ingest_receipts_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    trace_tick_id BYTEA NOT NULL,
    contract_version SMALLINT NOT NULL,
    expected_character_version BIGINT NOT NULL,
    lineage_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    checkpoint_tick BIGINT NOT NULL,
    event_tick BIGINT NOT NULL,
    entry_count SMALLINT NOT NULL,
    status_count SMALLINT NOT NULL,
    lethal_count SMALLINT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    tick_digest BYTEA NOT NULL,
    result_digest BYTEA NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, trace_tick_id),
    UNIQUE (namespace_id, account_id, character_id, trace_tick_id),
    UNIQUE (namespace_id, account_id, character_id, lineage_id, event_tick),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) REFERENCES character_entry_restore_points (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT live_trace_ingest_contract_exact CHECK (contract_version = 1),
    CONSTRAINT live_trace_ingest_ids_exact CHECK (
        octet_length(trace_tick_id) = 16
        AND trace_tick_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(lineage_id) = 16
        AND lineage_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(restore_point_id) = 16
        AND restore_point_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT live_trace_ingest_versions_ticks_positive CHECK (
        expected_character_version > 0 AND checkpoint_tick >= 0 AND event_tick > 0
        AND event_tick >= checkpoint_tick
    ),
    CONSTRAINT live_trace_ingest_counts_bounded CHECK (
        entry_count BETWEEN 1 AND 4096
        AND status_count BETWEEN 0 AND 4096
        AND lethal_count BETWEEN 0 AND 1
    ),
    CONSTRAINT live_trace_ingest_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT live_trace_ingest_hashes_exact CHECK (
        octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(tick_digest) = 32
        AND tick_digest <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_digest) = 32
        AND result_digest <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT live_trace_ingest_issue_order CHECK (committed_at >= issued_at),
    CONSTRAINT live_trace_ingest_payload_authority_unique UNIQUE (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        event_tick, trace_tick_id, records_blake3, assets_blake3, localization_blake3,
        request_hash, tick_digest
    )
);

ALTER TABLE character_live_damage_trace_entries_v1
    ADD COLUMN source_sim_entity_id BYTEA,
    ADD CONSTRAINT live_trace_source_identity_parity CHECK (
        (source_entity_id IS NULL AND source_sim_entity_id IS NULL)
        OR (source_entity_id IS NOT NULL
            AND source_sim_entity_id IS NOT NULL
            AND octet_length(source_sim_entity_id) = 8
            AND source_sim_entity_id <> decode(repeat('00', 8), 'hex'))
    );

ALTER TABLE character_live_damage_trace_ticks_v1
    ADD CONSTRAINT live_trace_payload_retained_receipt_owned_v1 FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        event_tick, trace_tick_id, records_blake3, assets_blake3, localization_blake3,
        request_hash, tick_digest
    ) REFERENCES character_live_damage_trace_ingest_receipts_v1 (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        event_tick, trace_tick_id, records_blake3, assets_blake3, localization_blake3,
        request_hash, tick_digest
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;

CREATE INDEX live_trace_ingest_receipts_ordered_v1
    ON character_live_damage_trace_ingest_receipts_v1 (
        namespace_id, account_id, character_id, lineage_id, event_tick DESC
    );

CREATE TABLE character_live_damage_trace_conflict_audits_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    trace_tick_id BYTEA NOT NULL,
    attempted_character_id BYTEA NOT NULL,
    audit_id BYTEA NOT NULL,
    conflict_code SMALLINT NOT NULL,
    stored_request_hash BYTEA NOT NULL,
    attempted_request_hash BYTEA NOT NULL,
    observed_character_version BIGINT NOT NULL,
    attempted_issued_at TIMESTAMPTZ NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, audit_id),
    UNIQUE (namespace_id, account_id, trace_tick_id, attempted_request_hash),
    FOREIGN KEY (namespace_id, account_id, character_id, trace_tick_id)
        REFERENCES character_live_damage_trace_ingest_receipts_v1(
            namespace_id, account_id, character_id, trace_tick_id
        ) ON DELETE CASCADE,
    CONSTRAINT live_trace_conflict_ids_exact CHECK (
        octet_length(attempted_character_id) = 16
        AND attempted_character_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(audit_id) = 16
        AND audit_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT live_trace_conflict_code_exact CHECK (conflict_code = 0),
    CONSTRAINT live_trace_conflict_hashes_exact CHECK (
        octet_length(stored_request_hash) = 32
        AND stored_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_request_hash) = 32
        AND attempted_request_hash <> decode(repeat('00', 32), 'hex')
        AND attempted_request_hash <> stored_request_hash
    ),
    CONSTRAINT live_trace_conflict_character_version_positive CHECK (
        observed_character_version > 0
    )
);

CREATE TRIGGER live_trace_ingest_receipt_append_only_v1
BEFORE UPDATE OR DELETE ON character_live_damage_trace_ingest_receipts_v1
FOR EACH ROW EXECUTE FUNCTION reject_live_death_evidence_receipt_mutation_v1();

CREATE TRIGGER live_trace_conflict_audit_append_only_v1
BEFORE UPDATE OR DELETE ON character_live_damage_trace_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION reject_live_death_evidence_receipt_mutation_v1();

COMMENT ON TABLE character_live_damage_trace_ingest_receipts_v1 IS
    'Append-only contract-1 server trace-ingest receipts retained after complete live tick pruning.';
COMMENT ON TABLE character_live_damage_trace_conflict_audits_v1 IS
    'TECH-021 changed-payload evidence containing hashes and bounded authority, never raw payloads or secrets.';
COMMENT ON COLUMN character_live_damage_trace_entries_v1.source_sim_entity_id IS
    'Exact little-endian sim_core EntityId u64 paired with the durable source entity identity.';

-- Downgrade/recovery: Core remains wipeable. Restore a pre-0048 backup or wipe/reapply the Core
-- namespace. Never remove retained ingest/conflict evidence from a namespace that accepted
-- contract-1 commands, and never rewrite the normalized migration 0043 in place.
