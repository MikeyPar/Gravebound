-- GB-M03-06B crash-time deed revocation closure.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md ECH-001 and TECH-021/023;
-- - Gravebound_Content_Production_Spec_v1.md Core reward/XP bindings;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06/13 replay, restart, and atomicity gates;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md.
--
-- Migration 0045 introduced the revocation graph before the crash writer existed. This forward-only
-- closure makes contract 1 mandatory for every new normalized crash result, binds a canonical
-- aggregate revocation digest, requires every active v2 deed from the restored danger root to be
-- revoked, and binds all revocations to the crash transaction timestamp and rebuilt projection.
-- Downgrade requires proving no contract-1 result or v2 revocation exists before restoring schema
-- 45; migration history is never rewritten.

ALTER TABLE danger_crash_restore_results
    ADD COLUMN life_deed_revocation_digest BYTEA;

ALTER TABLE danger_crash_restore_results
    DROP CONSTRAINT danger_crash_life_deed_v2_shape,
    ADD CONSTRAINT danger_crash_life_deed_v2_shape CHECK (
        (
            life_deed_contract_version = 0
            AND revoked_life_deed_count = 0
            AND life_deed_projection_digest IS NULL
            AND life_deed_revocation_digest IS NULL
        ) OR (
            life_deed_contract_version = 1
            AND revoked_life_deed_count BETWEEN 0 AND 4095
            AND octet_length(life_deed_projection_digest) = 32
            AND life_deed_projection_digest <> decode(repeat('00', 32), 'hex')
            AND octet_length(life_deed_revocation_digest) = 32
            AND life_deed_revocation_digest <> decode(repeat('00', 32), 'hex')
        )
    ),
    ALTER COLUMN life_deed_contract_version SET DEFAULT 1;

CREATE FUNCTION require_new_crash_life_deed_contract_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.life_deed_contract_version <> 1 THEN
        RAISE EXCEPTION 'new crash result requires life deed contract 1';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER new_crash_life_deed_contract_v1
BEFORE INSERT ON danger_crash_restore_results
FOR EACH ROW EXECUTE FUNCTION require_new_crash_life_deed_contract_v1();

CREATE OR REPLACE FUNCTION enforce_danger_crash_life_deed_graph_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT;
    target_account BYTEA;
    target_mutation BYTEA;
    stored_character BYTEA;
    stored_restore BYTEA;
    stored_contract SMALLINT;
    stored_count INTEGER;
    stored_projection_digest BYTEA;
    stored_revocation_digest BYTEA;
    stored_committed_at TIMESTAMPTZ;
    actual_count BIGINT;
BEGIN
    IF TG_TABLE_NAME = 'danger_crash_restore_results' THEN
        IF TG_OP = 'DELETE' THEN
            target_namespace := OLD.namespace_id;
            target_account := OLD.account_id;
            target_mutation := OLD.mutation_id;
        ELSE
            target_namespace := NEW.namespace_id;
            target_account := NEW.account_id;
            target_mutation := NEW.mutation_id;
        END IF;
    ELSIF TG_OP = 'DELETE' THEN
        target_namespace := OLD.namespace_id;
        target_account := OLD.account_id;
        target_mutation := OLD.crash_mutation_id;
    ELSE
        target_namespace := NEW.namespace_id;
        target_account := NEW.account_id;
        target_mutation := NEW.crash_mutation_id;
    END IF;

    SELECT character_id, restore_point_id, life_deed_contract_version,
           revoked_life_deed_count, life_deed_projection_digest,
           life_deed_revocation_digest, committed_at
    INTO stored_character, stored_restore, stored_contract, stored_count,
         stored_projection_digest, stored_revocation_digest, stored_committed_at
    FROM danger_crash_restore_results
    WHERE namespace_id = target_namespace
      AND account_id = target_account
      AND mutation_id = target_mutation;
    IF NOT FOUND THEN RETURN NULL; END IF;

    SELECT count(*) INTO actual_count
    FROM character_life_deed_revocations_v2
    WHERE namespace_id = target_namespace
      AND account_id = target_account
      AND crash_mutation_id = target_mutation;

    IF stored_contract = 0 THEN
        IF actual_count <> 0 OR stored_count <> 0
            OR stored_projection_digest IS NOT NULL
            OR stored_revocation_digest IS NOT NULL
        THEN
            RAISE EXCEPTION 'legacy crash result cannot carry live deed revocations';
        END IF;
        RETURN NULL;
    END IF;

    IF stored_contract <> 1
        OR stored_projection_digest IS NULL
        OR stored_revocation_digest IS NULL
        OR actual_count <> stored_count
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT change_ordinal,
                    row_number() OVER (ORDER BY completion_id) - 1 AS expected_ordinal,
                    restore_point_id, post_projection_digest, revoked_at
                FROM character_life_deed_revocations_v2
                WHERE namespace_id = target_namespace
                  AND account_id = target_account
                  AND crash_mutation_id = target_mutation
            ) AS ordered
            WHERE ordered.change_ordinal <> ordered.expected_ordinal
               OR ordered.restore_point_id <> stored_restore
               OR ordered.post_projection_digest <> stored_projection_digest
               OR ordered.revoked_at <> stored_committed_at
        )
        OR EXISTS (
            SELECT 1
            FROM character_life_deed_completion_receipts_v2 AS receipt
            LEFT JOIN character_life_deed_revocations_v2 AS revocation
              ON revocation.namespace_id = receipt.namespace_id
             AND revocation.account_id = receipt.account_id
             AND revocation.character_id = receipt.character_id
             AND revocation.completion_id = receipt.completion_id
            WHERE receipt.namespace_id = target_namespace
              AND receipt.account_id = target_account
              AND receipt.character_id = stored_character
              AND receipt.restore_point_id = stored_restore
              AND revocation.completion_id IS NULL
        )
    THEN
        RAISE EXCEPTION 'crash result live deed revocation graph is incomplete or noncanonical';
    END IF;
    RETURN NULL;
END
$$;

COMMENT ON COLUMN danger_crash_restore_results.life_deed_revocation_digest IS
    'Canonical root-bound v2 receipt revocation set; contract 1 is mandatory even when the set is empty.';
COMMENT ON FUNCTION require_new_crash_life_deed_contract_v1() IS
    'Prevents new crash restores from silently using the migration-0045 legacy default-zero sidecar.';
COMMENT ON FUNCTION enforce_danger_crash_life_deed_graph_v1() IS
    'Requires every active v2 deed from the restored danger root to be revoked and the projection rebuilt atomically.';
