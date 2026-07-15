-- GB-M03-02D / GB-M03-06A crash-restore request authority and source binding.
--
-- Authorities: GDD TECH-015/020/021/023, Content CONT-014/CONT-HUB-002,
-- Roadmap GB-M03-02/06/08, and accepted SPEC-CONFLICT-027/028. Migration 0034 remains
-- immutable. This forward correction separates request replay/terminal precedence from the
-- normalized payload written only by a newly committed crash restoration.

CREATE TABLE danger_crash_restore_request_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    request_hash BYTEA NOT NULL,
    outcome_code SMALLINT NOT NULL,
    observed_restore_state SMALLINT NOT NULL,
    committed_mutation_id BYTEA,
    result_payload BYTEA NOT NULL,
    result_digest BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    UNIQUE (
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    ),
    UNIQUE (
        namespace_id, account_id, character_id, restore_point_id, mutation_id, request_hash
    ),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT danger_crash_request_result_ids_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND (committed_mutation_id IS NULL OR (
            octet_length(committed_mutation_id) = 16
            AND committed_mutation_id <> decode(repeat('00', 16), 'hex')
        ))
    ),
    CONSTRAINT danger_crash_request_result_hashes_exact CHECK (
        octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_digest) = 32
        AND result_digest <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT danger_crash_request_result_payload_bounded CHECK (
        octet_length(result_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT danger_crash_request_result_outcome_known CHECK (
        outcome_code BETWEEN 0 AND 4
    ),
    CONSTRAINT danger_crash_request_result_terminal_shape CHECK (
        (outcome_code = 0 AND observed_restore_state = 4
            AND committed_mutation_id = mutation_id)
        OR (outcome_code = 1 AND observed_restore_state = 1)
        OR (outcome_code = 2 AND observed_restore_state = 2)
        OR (outcome_code = 3 AND observed_restore_state = 3)
        OR (outcome_code = 4 AND observed_restore_state = 4
            AND committed_mutation_id IS NOT NULL)
    )
);

CREATE TABLE danger_crash_restore_conflict_audits (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    stored_request_hash BYTEA NOT NULL,
    attempted_request_hash BYTEA NOT NULL,
    audit_id BYTEA NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, audit_id),
    UNIQUE (namespace_id, account_id, mutation_id, attempted_request_hash),
    FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, mutation_id,
        stored_request_hash
    ) REFERENCES danger_crash_restore_request_results(
        namespace_id, account_id, character_id, restore_point_id, mutation_id, request_hash
    ) ON DELETE CASCADE,
    CONSTRAINT danger_crash_conflict_audit_ids_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(audit_id) = 16
        AND audit_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT danger_crash_conflict_audit_hashes_exact CHECK (
        octet_length(stored_request_hash) = 32
        AND stored_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_request_hash) = 32
        AND attempted_request_hash <> decode(repeat('00', 32), 'hex')
        AND attempted_request_hash <> stored_request_hash
    )
);

CREATE FUNCTION enforce_danger_crash_request_terminal_source_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    root_state SMALLINT;
    root_crash_mutation BYTEA;
BEGIN
    SELECT restore_state, crash_restore_mutation_id
    INTO root_state, root_crash_mutation
    FROM character_entry_restore_points
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND character_id = NEW.character_id
      AND restore_point_id = NEW.restore_point_id;
    IF NOT FOUND OR root_state <> NEW.observed_restore_state THEN
        RAISE EXCEPTION 'danger crash request result does not match terminal root';
    END IF;
    IF NEW.outcome_code = 0 AND (
        root_crash_mutation IS DISTINCT FROM NEW.mutation_id
        OR NOT EXISTS (
            SELECT 1 FROM danger_crash_restore_results AS result
            WHERE result.namespace_id = NEW.namespace_id
              AND result.account_id = NEW.account_id
              AND result.character_id = NEW.character_id
              AND result.restore_point_id = NEW.restore_point_id
              AND result.mutation_id = NEW.mutation_id
        )
    ) THEN
        RAISE EXCEPTION 'new crash restoration receipt lacks its normalized result';
    END IF;
    IF NEW.outcome_code = 4 AND
        root_crash_mutation IS DISTINCT FROM NEW.committed_mutation_id THEN
        RAISE EXCEPTION 'replayed crash restoration receipt names the wrong committed mutation';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER danger_crash_request_terminal_source_exact
AFTER INSERT OR UPDATE ON danger_crash_restore_request_results
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_request_terminal_source_v1();

CREATE FUNCTION reject_danger_crash_request_history_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
    RAISE EXCEPTION 'danger crash request history is immutable';
END
$$;

CREATE TRIGGER danger_crash_request_results_immutable
BEFORE UPDATE OR DELETE ON danger_crash_restore_request_results
FOR EACH ROW EXECUTE FUNCTION reject_danger_crash_request_history_mutation_v1();
CREATE TRIGGER danger_crash_conflict_audits_immutable
BEFORE UPDATE OR DELETE ON danger_crash_restore_conflict_audits
FOR EACH ROW EXECUTE FUNCTION reject_danger_crash_request_history_mutation_v1();

-- A restored child must be an entry-baseline UID at its exact entry location. A revoked child
-- must not be an entry-baseline UID. The schema-34 post-item and crash-ledger checks still apply.
CREATE OR REPLACE FUNCTION enforce_danger_crash_item_change_source_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM item_instances AS item
        WHERE item.namespace_id = NEW.namespace_id
          AND item.account_id = NEW.account_id
          AND item.character_id = NEW.character_id
          AND item.item_uid = NEW.item_uid
          AND item.item_version = NEW.post_item_version
          AND item.security_state = NEW.post_security_state
          AND item.location_kind = NEW.post_location_kind
          AND item.slot_index IS NOT DISTINCT FROM NEW.post_slot_index
          AND (
              (NEW.change_kind = 0 AND item.destruction_reason IS NULL)
              OR (NEW.change_kind = 1 AND item.destruction_reason = 'crash_revoked')
          )
    ) OR (
        NEW.change_kind = 0 AND NOT EXISTS (
            SELECT 1
            FROM entry_restore_inventory_items_v3 AS baseline
            WHERE baseline.namespace_id = NEW.namespace_id
              AND baseline.account_id = NEW.account_id
              AND baseline.character_id = NEW.character_id
              AND baseline.restore_point_id = NEW.restore_point_id
              AND baseline.item_uid = NEW.item_uid
              AND baseline.location_kind = NEW.post_location_kind
              AND baseline.slot_index = NEW.post_slot_index
              AND NEW.post_security_state = CASE baseline.location_kind
                  WHEN 0 THEN 0
                  WHEN 1 THEN 0
                  WHEN 2 THEN 2
              END
        )
    ) OR (
        NEW.change_kind = 1 AND EXISTS (
            SELECT 1
            FROM entry_restore_inventory_items_v3 AS baseline
            WHERE baseline.namespace_id = NEW.namespace_id
              AND baseline.account_id = NEW.account_id
              AND baseline.character_id = NEW.character_id
              AND baseline.restore_point_id = NEW.restore_point_id
              AND baseline.item_uid = NEW.item_uid
        )
    ) THEN
        RAISE EXCEPTION 'danger crash item change is not bound to exact baseline and item state';
    END IF;
    RETURN NULL;
END
$$;

ALTER TABLE bargain_offers
    ADD CONSTRAINT bargain_offer_crash_restore_same_entry CHECK (
        revoked_by_restore_point_id IS NULL
        OR revoked_by_restore_point_id = entry_restore_point_id
    );

CREATE OR REPLACE FUNCTION enforce_danger_crash_bargain_change_source_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    source_exists BOOLEAN;
BEGIN
    IF NEW.record_kind = 0 THEN
        SELECT EXISTS (
            SELECT 1 FROM bargain_offers AS offer
            WHERE offer.namespace_id = NEW.namespace_id
              AND offer.account_id = NEW.account_id
              AND offer.character_id = NEW.character_id
              AND offer.offer_id = NEW.record_id
              AND offer.entry_restore_point_id = NEW.restore_point_id
              AND offer.revoked_by_restore_point_id = NEW.restore_point_id
              AND offer.revoked_at IS NOT NULL
        ) INTO source_exists;
    ELSIF NEW.record_kind = 1 THEN
        SELECT EXISTS (
            SELECT 1 FROM bargain_milestone_results AS milestone
            WHERE milestone.namespace_id = NEW.namespace_id
              AND milestone.account_id = NEW.account_id
              AND milestone.character_id = NEW.character_id
              AND milestone.source_reward_event_id = NEW.record_id
              AND milestone.entry_restore_point_id = NEW.restore_point_id
              AND milestone.revoked_by_restore_point_id = NEW.restore_point_id
              AND milestone.revoked_at IS NOT NULL
        ) INTO source_exists;
    ELSE
        SELECT EXISTS (
            SELECT 1
            FROM bargain_decision_results AS decision
            JOIN bargain_offers AS offer
              ON offer.namespace_id = decision.namespace_id
             AND offer.account_id = decision.account_id
             AND offer.character_id = decision.character_id
             AND offer.offer_id = decision.offer_id
            WHERE decision.namespace_id = NEW.namespace_id
              AND decision.account_id = NEW.account_id
              AND decision.character_id = NEW.character_id
              AND decision.mutation_id = NEW.record_id
              AND offer.entry_restore_point_id = NEW.restore_point_id
              AND decision.revoked_by_restore_point_id = NEW.restore_point_id
              AND decision.revoked_at IS NOT NULL
        ) INTO source_exists;
    END IF;
    IF NOT source_exists THEN
        RAISE EXCEPTION 'danger crash Bargain change is not bound to its entry root';
    END IF;
    RETURN NULL;
END
$$;
