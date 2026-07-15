-- GB-M03-06B authoritative 30 Hz life-clock receipt closure.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-001 and TECH-020..023;
-- - Gravebound_Content_Production_Spec_v1.md Core life/Echo content authority;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06/13 and restart/idempotency exits;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md.
--
-- Migration 0043 intentionally created the dormant normalized clock table and 0044 retained the
-- server issue time. No production writer existed. This forward-only migration completes the
-- TECH-021 command authority before enabling that writer: expected character version, explicit
-- result contract, nonempty intervals, and append-only changed-payload conflict evidence.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM character_life_clock_checkpoint_receipts_v1) THEN
        RAISE EXCEPTION '0047 requires the dormant life-clock receipt table';
    END IF;
END
$$;

ALTER TABLE character_life_clock_checkpoint_receipts_v1
    ADD COLUMN contract_version SMALLINT NOT NULL DEFAULT 1,
    ADD COLUMN expected_character_version BIGINT NOT NULL,
    DROP CONSTRAINT life_clock_checkpoint_interval_bounded,
    ADD CONSTRAINT life_clock_checkpoint_contract_exact CHECK (contract_version = 1),
    ADD CONSTRAINT life_clock_checkpoint_character_version_positive CHECK (
        expected_character_version > 0
    ),
    ADD CONSTRAINT life_clock_checkpoint_interval_bounded CHECK (
        authoritative_tick > 0
        AND advanced_ticks BETWEEN 1 AND 1800
        AND authoritative_tick >= advanced_ticks
    );

CREATE TABLE character_life_clock_conflict_audits_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    checkpoint_id BYTEA NOT NULL,
    attempted_character_id BYTEA NOT NULL,
    audit_id BYTEA NOT NULL,
    conflict_code SMALLINT NOT NULL,
    stored_request_hash BYTEA NOT NULL,
    attempted_request_hash BYTEA NOT NULL,
    observed_character_version BIGINT NOT NULL,
    observed_life_metrics_version BIGINT NOT NULL,
    attempted_issued_at TIMESTAMPTZ NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, audit_id),
    UNIQUE (namespace_id, account_id, checkpoint_id, attempted_request_hash),
    FOREIGN KEY (namespace_id, account_id, character_id, checkpoint_id)
        REFERENCES character_life_clock_checkpoint_receipts_v1(
            namespace_id, account_id, character_id, checkpoint_id
        ) ON DELETE CASCADE,
    CONSTRAINT life_clock_conflict_ids_exact CHECK (
        octet_length(attempted_character_id) = 16
        AND attempted_character_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(audit_id) = 16
        AND audit_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT life_clock_conflict_code_exact CHECK (conflict_code = 0),
    CONSTRAINT life_clock_conflict_hashes_exact CHECK (
        octet_length(stored_request_hash) = 32
        AND stored_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_request_hash) = 32
        AND attempted_request_hash <> decode(repeat('00', 32), 'hex')
        AND attempted_request_hash <> stored_request_hash
    ),
    CONSTRAINT life_clock_conflict_versions_positive CHECK (
        observed_character_version > 0 AND observed_life_metrics_version > 0
    )
);

CREATE TRIGGER life_clock_conflict_audit_append_only_v1
BEFORE UPDATE OR DELETE ON character_life_clock_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION reject_live_death_evidence_receipt_mutation_v1();

COMMENT ON TABLE character_life_clock_checkpoint_receipts_v1 IS
    'Append-only contract-1 30 Hz clock intervals with selected-character, content, version, replay, and restart authority.';
COMMENT ON TABLE character_life_clock_conflict_audits_v1 IS
    'Bounded TECH-021 clock changed-payload evidence; stores hashes and observed versions, never raw payloads or network secrets.';
COMMENT ON COLUMN character_life_clock_checkpoint_receipts_v1.expected_character_version IS
    'Selected living character version observed by the server-owned clock checkpoint command.';

-- Downgrade/recovery: Core remains wipeable. Restore a pre-0047 backup or wipe/reapply the Core
-- namespace. Do not drop the expected-version/conflict evidence from a namespace that has emitted
-- contract-1 receipts, and never rewrite migrations 0043/0044 in place.
