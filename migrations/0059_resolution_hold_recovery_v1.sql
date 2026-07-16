-- GB-M03-08 minimum ResolutionHold recovery.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-011, LOOT-002/050/060,
--   and TECH-021-023.
-- - Gravebound_Content_Production_Spec_v1.md CONT-HUB-001/002.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-03/08.
-- - Accepted SPEC-CONFLICT-029 and SPEC-CONFLICT-030.
--
-- Append-only discriminants:
--   Hold action 0 Move, 1 DestroyConfirmed.
--   Hold disposition 0 Moved, 1 Destroyed.
--   Item-ledger source 7 ResolutionHold.
--
-- Recovery/downgrade:
-- A pre-0059 binary may be restored only after proving these result/projection/audit/outbox
-- tables are empty and no item ledger has source_kind=7 or reason
-- 'resolution_hold_destroyed'. Published migration history is never rewritten in place.

CREATE TABLE resolution_hold_mutation_results_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    extraction_id BYTEA NOT NULL,
    stack_index SMALLINT NOT NULL,
    contract_version SMALLINT NOT NULL,
    action_kind SMALLINT NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    expected_stack_digest BYTEA NOT NULL,
    result_hash BYTEA NOT NULL,
    result_payload BYTEA NOT NULL,
    content_revision TEXT NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    pre_account_version BIGINT NOT NULL,
    post_account_version BIGINT NOT NULL,
    pre_character_version BIGINT NOT NULL,
    post_character_version BIGINT NOT NULL,
    pre_world_version BIGINT NOT NULL,
    post_world_version BIGINT NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    destination_kind SMALLINT,
    destination_slot_index SMALLINT,
    transition_count SMALLINT NOT NULL,
    remaining_hold_stack_count SMALLINT NOT NULL,
    storage_resolution_required BOOLEAN NOT NULL,
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    UNIQUE (namespace_id, account_id, character_id, mutation_id),
    UNIQUE (
        namespace_id, account_id, mutation_id, canonical_request_hash
    ),
    UNIQUE (
        namespace_id, account_id, character_id, extraction_id, stack_index
    ),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, extraction_id)
        REFERENCES character_extraction_terminal_results_v1(
            namespace_id, account_id, character_id, terminal_id
        ),
    CONSTRAINT resolution_hold_result_ids_exact CHECK (
        octet_length(account_id) = 16
        AND octet_length(character_id) = 16
        AND octet_length(mutation_id) = 16
        AND octet_length(extraction_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
        AND character_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND extraction_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> extraction_id
    ),
    CONSTRAINT resolution_hold_result_hashes_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(expected_stack_digest) = 32
        AND expected_stack_digest <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT resolution_hold_result_content_exact CHECK (
        contract_version = 1
        AND action_kind IN (0, 1)
        AND stack_index BETWEEN 0 AND 7
        AND length(content_revision) BETWEEN 3 AND 96
        AND content_revision !~ '[[:cntrl:]]'
        AND issued_at <= committed_at
    ),
    CONSTRAINT resolution_hold_result_versions_exact CHECK (
        pre_account_version > 0
        AND post_account_version IN (pre_account_version, pre_account_version + 1)
        AND pre_character_version > 0
        AND pre_world_version = pre_character_version
        AND pre_inventory_version > 0
        AND post_inventory_version = pre_inventory_version + 1
        AND (
            (
                storage_resolution_required
                AND post_character_version = pre_character_version
                AND post_world_version = pre_world_version
            )
            OR (
                NOT storage_resolution_required
                AND post_character_version = pre_character_version + 1
                AND post_world_version = pre_world_version + 1
            )
        )
    ),
    CONSTRAINT resolution_hold_result_action_exact CHECK (
        transition_count BETWEEN 1 AND 64
        AND remaining_hold_stack_count BETWEEN 0 AND 8
        AND storage_resolution_required = (remaining_hold_stack_count > 0)
        AND (
            (
                action_kind = 0
                AND destination_kind IN (5, 6, 8)
                AND destination_slot_index IS NOT NULL
                AND (
                    (destination_kind = 5 AND destination_slot_index BETWEEN 0 AND 7)
                    OR (destination_kind = 6 AND destination_slot_index BETWEEN 0 AND 159)
                    OR (destination_kind = 8 AND destination_slot_index BETWEEN 0 AND 19)
                )
                AND (
                    (
                        destination_kind = 5
                        AND post_account_version = pre_account_version
                    )
                    OR (
                        destination_kind IN (6, 8)
                        AND post_account_version = pre_account_version + 1
                    )
                )
            )
            OR (
                action_kind = 1
                AND destination_kind IS NULL
                AND destination_slot_index IS NULL
                AND post_account_version = pre_account_version
            )
        )
    )
);

CREATE TABLE resolution_hold_item_transitions_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    extraction_id BYTEA NOT NULL,
    stack_index SMALLINT NOT NULL,
    transition_ordinal SMALLINT NOT NULL,
    item_uid BYTEA NOT NULL,
    template_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    item_kind SMALLINT NOT NULL,
    disposition_kind SMALLINT NOT NULL,
    source_kind SMALLINT NOT NULL,
    source_slot_index SMALLINT NOT NULL,
    destination_kind SMALLINT NOT NULL,
    destination_slot_index SMALLINT,
    pre_item_version BIGINT NOT NULL,
    post_item_version BIGINT NOT NULL,
    pre_security_state SMALLINT NOT NULL,
    post_security_state SMALLINT NOT NULL,
    destruction_reason TEXT,
    ledger_event_id BYTEA NOT NULL,
    ledger_event_kind SMALLINT NOT NULL,
    ledger_source_kind SMALLINT NOT NULL,
    PRIMARY KEY (namespace_id, account_id, mutation_id, transition_ordinal),
    UNIQUE (namespace_id, account_id, mutation_id, item_uid),
    FOREIGN KEY (namespace_id, account_id, character_id, mutation_id)
        REFERENCES resolution_hold_mutation_results_v1(
            namespace_id, account_id, character_id, mutation_id
        ) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, extraction_id, item_uid)
        REFERENCES extraction_terminal_item_placements_v1(
            namespace_id, terminal_id, item_uid
        ),
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid),
    CONSTRAINT resolution_hold_transition_ids_exact CHECK (
        octet_length(account_id) = 16
        AND octet_length(character_id) = 16
        AND octet_length(mutation_id) = 16
        AND octet_length(extraction_id) = 16
        AND octet_length(item_uid) = 16
        AND octet_length(ledger_event_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
        AND character_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND extraction_id <> decode(repeat('00', 16), 'hex')
        AND item_uid <> decode(repeat('00', 16), 'hex')
        AND ledger_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT resolution_hold_transition_shape_exact CHECK (
        stack_index BETWEEN 0 AND 7
        AND transition_ordinal BETWEEN 0 AND 63
        AND length(template_id) BETWEEN 3 AND 96
        AND length(content_revision) BETWEEN 3 AND 96
        AND item_kind IN (0, 1)
        AND disposition_kind IN (0, 1)
        AND source_kind = 9
        AND source_slot_index = stack_index
        AND pre_item_version > 0
        AND post_item_version = pre_item_version + 1
        AND pre_security_state = 0
        AND ledger_source_kind = 7
        AND (
            (
                disposition_kind = 0
                AND destination_kind IN (5, 6, 8)
                AND destination_slot_index IS NOT NULL
                AND post_security_state = 0
                AND destruction_reason IS NULL
                AND ledger_event_kind = 1
                AND (
                    (destination_kind = 5 AND destination_slot_index BETWEEN 0 AND 7)
                    OR (destination_kind = 6 AND destination_slot_index BETWEEN 0 AND 159)
                    OR (destination_kind = 8 AND destination_slot_index BETWEEN 0 AND 19)
                )
            )
            OR (
                disposition_kind = 1
                AND destination_kind = 4
                AND destination_slot_index IS NULL
                AND post_security_state = 3
                AND destruction_reason = 'resolution_hold_destroyed'
                AND ledger_event_kind = 2
            )
        )
    )
);

CREATE TABLE resolution_hold_mutation_audit_events_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    event_id BYTEA NOT NULL,
    event_type SMALLINT NOT NULL,
    event_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, account_id, mutation_id, event_type),
    FOREIGN KEY (namespace_id, account_id, character_id, mutation_id)
        REFERENCES resolution_hold_mutation_results_v1(
            namespace_id, account_id, character_id, mutation_id
        ) ON DELETE CASCADE,
    CONSTRAINT resolution_hold_audit_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
        AND event_type = 1
        AND octet_length(event_digest) = 32
        AND event_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE resolution_hold_mutation_conflict_audits_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    stored_request_hash BYTEA NOT NULL,
    incoming_request_hash BYTEA NOT NULL,
    conflict_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id, conflict_digest),
    UNIQUE (namespace_id, account_id, mutation_id, incoming_request_hash),
    FOREIGN KEY (
        namespace_id, account_id, mutation_id, stored_request_hash
    )
        REFERENCES resolution_hold_mutation_results_v1(
            namespace_id, account_id, mutation_id, canonical_request_hash
        ) ON DELETE CASCADE,
    CONSTRAINT resolution_hold_conflict_exact CHECK (
        octet_length(stored_request_hash) = 32
        AND octet_length(incoming_request_hash) = 32
        AND octet_length(conflict_digest) = 32
        AND stored_request_hash <> decode(repeat('00', 32), 'hex')
        AND incoming_request_hash <> decode(repeat('00', 32), 'hex')
        AND conflict_digest <> decode(repeat('00', 32), 'hex')
        AND stored_request_hash <> incoming_request_hash
    )
);

CREATE TABLE resolution_hold_mutation_outbox_events_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    event_id BYTEA NOT NULL,
    event_type SMALLINT NOT NULL,
    event_payload BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, account_id, mutation_id, event_type),
    FOREIGN KEY (namespace_id, account_id, character_id, mutation_id)
        REFERENCES resolution_hold_mutation_results_v1(
            namespace_id, account_id, character_id, mutation_id
        ) ON DELETE CASCADE,
    CONSTRAINT resolution_hold_outbox_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
        AND event_type = 1
        AND octet_length(event_payload) BETWEEN 1 AND 65536
        AND (published_at IS NULL OR published_at >= created_at)
    )
);

ALTER TABLE item_instances
    DROP CONSTRAINT item_location_shape,
    ADD CONSTRAINT item_location_shape CHECK (
        (location_kind = 0 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 3 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state IN (0, 1) AND item_kind = 0)
        OR (location_kind = 1 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 1 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state IN (0, 1) AND item_kind = 1)
        OR (location_kind = 2 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state = 2)
        OR (location_kind = 3 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NOT NULL AND octet_length(instance_id) = 16
            AND instance_id <> decode(repeat('00', 16), 'hex')
            AND pickup_id IS NOT NULL AND octet_length(pickup_id) = 16
            AND pickup_id <> decode(repeat('00', 16), 'hex')
            AND expires_at_tick > 0 AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state = 2)
        OR (location_kind = 4 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NOT NULL AND security_state = 3
            AND overflow_expires_at IS NULL
            AND (
                (destruction_reason = 'permadeath'
                    AND terminal_death_id IS NOT NULL
                    AND octet_length(terminal_death_id) = 16
                    AND terminal_death_id <> decode(repeat('00', 16), 'hex'))
                OR (destruction_reason IN ('ground_expired', 'crash_revoked')
                    AND terminal_death_id IS NULL)
                OR (destruction_reason = 'recall'
                    AND terminal_death_id IS NULL
                    AND terminal_recall_id IS NOT NULL
                    AND recalled_at IS NOT NULL)
                OR (destruction_reason = 'resolution_hold_destroyed'
                    AND terminal_death_id IS NULL
                    AND terminal_recall_id IS NULL
                    AND recalled_at IS NULL
                    AND terminal_extraction_id IS NOT NULL
                    AND extracted_at IS NOT NULL)
            ))
        OR (location_kind = 5 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state = 0)
        OR (location_kind = 6 AND character_id IS NULL
            AND slot_index BETWEEN 0 AND 159 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state = 0)
        OR (location_kind = 7 AND character_id IS NOT NULL AND item_kind = 1
            AND slot_index BETWEEN 0 AND 1 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason = 'consumed'
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state = 4)
        OR (location_kind = 8 AND character_id IS NULL
            AND slot_index BETWEEN 0 AND 19 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND terminal_extraction_id IS NOT NULL
            AND extracted_at IS NOT NULL
            AND overflow_expires_at = extracted_at + INTERVAL '72 hours'
            AND security_state = 0)
        OR (location_kind = 9 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND terminal_extraction_id IS NOT NULL
            AND extracted_at IS NOT NULL AND overflow_expires_at IS NULL
            AND security_state = 0)
    );

ALTER TABLE item_ledger_events
    DROP CONSTRAINT ledger_source_kind_known,
    DROP CONSTRAINT ledger_terminal_source_shape,
    DROP CONSTRAINT ledger_creation_shape,
    ADD CONSTRAINT ledger_source_kind_known CHECK (source_kind BETWEEN 0 AND 7),
    ADD CONSTRAINT ledger_terminal_source_shape CHECK (
        (
            source_kind = 5
            AND event_kind = 1
            AND terminal_extraction_id IS NOT NULL
            AND terminal_recall_id IS NULL
        )
        OR (
            source_kind = 6
            AND terminal_extraction_id IS NULL
            AND terminal_recall_id IS NOT NULL
            AND (
                (event_kind = 1 AND reason IS NULL)
                OR (event_kind = 2 AND reason = 'recall')
            )
        )
        OR (
            source_kind = 7
            AND terminal_extraction_id IS NOT NULL
            AND terminal_recall_id IS NULL
            AND (
                (event_kind = 1 AND reason IS NULL)
                OR (event_kind = 2 AND reason = 'resolution_hold_destroyed')
            )
        )
        OR (
            source_kind NOT IN (5, 6, 7)
            AND terminal_extraction_id IS NULL
            AND terminal_recall_id IS NULL
        )
    ),
    ADD CONSTRAINT ledger_creation_shape CHECK (
        (event_kind = 0 AND pre_item_version = 0
            AND pre_security_state IS NULL AND pre_location_kind IS NULL
            AND reason IS NULL AND terminal_death_id IS NULL)
        OR (event_kind = 1 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND reason IS NULL AND terminal_death_id IS NULL)
        OR (event_kind = 2 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND post_security_state = 3 AND post_location_kind = 4
            AND reason IS NOT NULL
            AND (
                (reason = 'permadeath'
                    AND terminal_death_id IS NOT NULL
                    AND octet_length(terminal_death_id) = 16
                    AND terminal_death_id <> decode(repeat('00', 16), 'hex'))
                OR (reason IN (
                        'ground_expired', 'recall', 'resolution_hold_destroyed'
                    ) AND terminal_death_id IS NULL)
            ))
        OR (event_kind = 3 AND pre_item_version > 0 AND source_kind <> 4
            AND pre_security_state = 1 AND pre_location_kind = 1
            AND post_security_state = 4 AND post_location_kind = 7
            AND reason = 'consumed' AND terminal_death_id IS NULL)
        OR (event_kind = 4 AND pre_item_version > 0 AND source_kind = 4
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND reason IS NOT NULL AND terminal_death_id IS NULL
            AND (
                (reason = 'crash_restored'
                    AND ((post_security_state = 0 AND post_location_kind IN (0, 1))
                        OR (post_security_state = 2 AND post_location_kind = 2)))
                OR (reason = 'crash_revoked'
                    AND post_security_state = 3 AND post_location_kind = 4)
            ))
    ),
    ADD CONSTRAINT item_ledger_resolution_hold_identity_v1 UNIQUE (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, event_kind, source_kind, pre_item_version,
        post_item_version, pre_security_state, post_security_state,
        pre_location_kind, post_location_kind, terminal_extraction_id
    );

ALTER TABLE resolution_hold_item_transitions_v1
    ADD CONSTRAINT resolution_hold_transition_ledger_owned FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, ledger_event_kind, ledger_source_kind,
        pre_item_version, post_item_version, pre_security_state,
        post_security_state, source_kind, destination_kind, extraction_id
    ) REFERENCES item_ledger_events (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, event_kind, source_kind,
        pre_item_version, post_item_version, pre_security_state,
        post_security_state, pre_location_kind, post_location_kind,
        terminal_extraction_id
    ) DEFERRABLE INITIALLY DEFERRED;

CREATE INDEX unpublished_resolution_hold_events_v1
    ON resolution_hold_mutation_outbox_events_v1 (
        namespace_id, created_at, event_id
    )
    WHERE published_at IS NULL;

CREATE FUNCTION enforce_resolution_hold_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    mutation_time TIMESTAMPTZ;
BEGIN
    IF TG_TABLE_NAME = 'resolution_hold_mutation_results_v1' THEN
        IF NEW.committed_at IS DISTINCT FROM transaction_timestamp() THEN
            RAISE EXCEPTION
                'ResolutionHold commit time is PostgreSQL transaction authority';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_TABLE_NAME = 'resolution_hold_mutation_outbox_events_v1' THEN
        IF NEW.published_at IS NOT NULL THEN
            RAISE EXCEPTION
                'ResolutionHold outbox must be inserted unpublished';
        END IF;
    END IF;
    SELECT committed_at INTO mutation_time
    FROM resolution_hold_mutation_results_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND mutation_id = NEW.mutation_id;
    IF NOT FOUND OR mutation_time IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION
            '% may be inserted only with its owning ResolutionHold mutation',
            TG_TABLE_NAME;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER resolution_hold_result_insert_window_v1
BEFORE INSERT ON resolution_hold_mutation_results_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_insert_window_v1();

CREATE TRIGGER dead_resolution_hold_result_insert_v1
BEFORE INSERT ON resolution_hold_mutation_results_v1
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();

CREATE TRIGGER resolution_hold_transition_insert_window_v1
BEFORE INSERT ON resolution_hold_item_transitions_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_insert_window_v1();

CREATE TRIGGER resolution_hold_audit_insert_window_v1
BEFORE INSERT ON resolution_hold_mutation_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_insert_window_v1();

CREATE TRIGGER resolution_hold_outbox_insert_window_v1
BEFORE INSERT ON resolution_hold_mutation_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_insert_window_v1();

CREATE FUNCTION enforce_resolution_hold_conflict_time_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.created_at IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION
            'ResolutionHold conflict time is PostgreSQL transaction authority';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER resolution_hold_conflict_insert_time_v1
BEFORE INSERT ON resolution_hold_mutation_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_conflict_time_v1();

CREATE FUNCTION enforce_resolution_hold_ledger_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    mutation_time TIMESTAMPTZ;
BEGIN
    IF NEW.source_kind <> 7 THEN
        RETURN NEW;
    END IF;
    SELECT committed_at INTO mutation_time
    FROM resolution_hold_mutation_results_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND character_id = NEW.character_id
      AND mutation_id = NEW.mutation_id
      AND extraction_id = NEW.terminal_extraction_id;
    IF NOT FOUND OR mutation_time IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION
            'ResolutionHold ledger may be inserted only with its owning mutation';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER resolution_hold_ledger_insert_window_v1
BEFORE INSERT ON item_ledger_events
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_ledger_insert_window_v1();

CREATE FUNCTION enforce_resolution_hold_history_immutable_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'ResolutionHold mutation history is immutable';
END
$$;

CREATE TRIGGER resolution_hold_result_immutable_v1
BEFORE UPDATE OR DELETE ON resolution_hold_mutation_results_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_history_immutable_v1();

CREATE TRIGGER resolution_hold_transition_immutable_v1
BEFORE UPDATE OR DELETE ON resolution_hold_item_transitions_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_history_immutable_v1();

CREATE TRIGGER resolution_hold_audit_immutable_v1
BEFORE UPDATE OR DELETE ON resolution_hold_mutation_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_history_immutable_v1();

CREATE TRIGGER resolution_hold_conflict_immutable_v1
BEFORE UPDATE OR DELETE ON resolution_hold_mutation_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_history_immutable_v1();

CREATE FUNCTION enforce_resolution_hold_outbox_publish_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'ResolutionHold outbox history is immutable';
    END IF;
    IF OLD.published_at IS NULL
        AND NEW.published_at IS NOT NULL
        AND NEW.namespace_id = OLD.namespace_id
        AND NEW.account_id = OLD.account_id
        AND NEW.character_id = OLD.character_id
        AND NEW.mutation_id = OLD.mutation_id
        AND NEW.event_id = OLD.event_id
        AND NEW.event_type = OLD.event_type
        AND NEW.event_payload = OLD.event_payload
        AND NEW.created_at = OLD.created_at
    THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'ResolutionHold outbox permits only first publication';
END
$$;

CREATE TRIGGER resolution_hold_outbox_publish_only_v1
BEFORE UPDATE OR DELETE ON resolution_hold_mutation_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_resolution_hold_outbox_publish_v1();

CREATE FUNCTION reject_resolution_hold_custody_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.location_kind = 4
        AND OLD.security_state = 3
        AND OLD.destruction_reason = 'resolution_hold_destroyed'
        AND OLD.terminal_extraction_id IS NOT NULL
    THEN
        IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'ResolutionHold-destroyed item custody is immutable';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER resolution_hold_destroyed_item_immutable_v1
BEFORE UPDATE OR DELETE ON item_instances
FOR EACH ROW EXECUTE FUNCTION reject_resolution_hold_custody_mutation_v1();

CREATE FUNCTION reject_resolution_hold_ledger_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.source_kind = 7 THEN
        IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'ResolutionHold item ledger is immutable';
    END IF;
    IF TG_OP = 'UPDATE' AND NEW.source_kind = 7 THEN
        RAISE EXCEPTION 'ResolutionHold item ledger is immutable';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER resolution_hold_item_ledger_immutable_v1
BEFORE UPDATE OR DELETE ON item_ledger_events
FOR EACH ROW EXECUTE FUNCTION reject_resolution_hold_ledger_mutation_v1();

CREATE FUNCTION enforce_complete_resolution_hold_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (SELECT count(*) FROM resolution_hold_item_transitions_v1
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id
          AND mutation_id = NEW.mutation_id) <> NEW.transition_count
        OR (SELECT min(transition_ordinal)
            FROM resolution_hold_item_transitions_v1
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND mutation_id = NEW.mutation_id) <> 0
        OR (SELECT max(transition_ordinal)
            FROM resolution_hold_item_transitions_v1
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND mutation_id = NEW.mutation_id) <> NEW.transition_count - 1
        OR (SELECT count(*) FROM extraction_terminal_item_placements_v1
            WHERE namespace_id = NEW.namespace_id
              AND terminal_id = NEW.extraction_id
              AND destination_kind = 9
              AND destination_slot_index = NEW.stack_index) <> NEW.transition_count
        OR NOT EXISTS (
            SELECT 1
            FROM resolution_hold_item_transitions_v1
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND mutation_id = NEW.mutation_id
            GROUP BY namespace_id, account_id, mutation_id
            HAVING count(*) = NEW.transition_count
               AND min(template_id) = max(template_id)
               AND min(content_revision) = max(content_revision)
               AND min(content_revision) = NEW.content_revision
               AND min(item_kind) = max(item_kind)
               AND (
                   (min(item_kind) = 0 AND count(*) = 1)
                   OR (min(item_kind) = 1 AND count(*) BETWEEN 1 AND 6)
               )
        )
        OR EXISTS (
            SELECT 1
            FROM resolution_hold_item_transitions_v1 AS current_transition
            JOIN resolution_hold_item_transitions_v1 AS previous_transition
              ON previous_transition.namespace_id = current_transition.namespace_id
             AND previous_transition.account_id = current_transition.account_id
             AND previous_transition.mutation_id = current_transition.mutation_id
             AND previous_transition.transition_ordinal
                 = current_transition.transition_ordinal - 1
            WHERE current_transition.namespace_id = NEW.namespace_id
              AND current_transition.account_id = NEW.account_id
              AND current_transition.mutation_id = NEW.mutation_id
              AND previous_transition.item_uid >= current_transition.item_uid
        )
        OR (SELECT count(*) FROM resolution_hold_mutation_audit_events_v1
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND mutation_id = NEW.mutation_id
              AND event_type = 1) <> 1
        OR (SELECT count(*) FROM resolution_hold_mutation_outbox_events_v1
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND mutation_id = NEW.mutation_id
              AND event_type = 1) <> 1
        OR NOT EXISTS (
            SELECT 1
            FROM resolution_hold_mutation_audit_events_v1 AS audit
            JOIN resolution_hold_mutation_outbox_events_v1 AS outbox
              ON outbox.namespace_id = audit.namespace_id
             AND outbox.account_id = audit.account_id
             AND outbox.character_id = audit.character_id
             AND outbox.mutation_id = audit.mutation_id
             AND outbox.event_type = audit.event_type
            WHERE audit.namespace_id = NEW.namespace_id
              AND audit.account_id = NEW.account_id
              AND audit.character_id = NEW.character_id
              AND audit.mutation_id = NEW.mutation_id
              AND audit.event_type = 1
              AND audit.event_digest = NEW.result_hash
              AND audit.created_at = NEW.committed_at
              AND outbox.event_payload = NEW.result_payload
              AND outbox.created_at = NEW.committed_at
              AND outbox.published_at IS NULL
        )
        OR EXISTS (
            SELECT 1
            FROM item_instances
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND terminal_extraction_id = NEW.extraction_id
              AND location_kind = 9
              AND slot_index = NEW.stack_index
        )
        OR (SELECT count(DISTINCT (terminal_extraction_id, slot_index))
            FROM item_instances
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND location_kind = 9) <> NEW.remaining_hold_stack_count
        OR EXISTS (
            SELECT 1
            FROM resolution_hold_item_transitions_v1 AS transition
            LEFT JOIN extraction_terminal_item_placements_v1 AS placement
              ON placement.namespace_id = transition.namespace_id
             AND placement.terminal_id = transition.extraction_id
             AND placement.item_uid = transition.item_uid
            LEFT JOIN character_extraction_terminal_results_v1 AS extraction
              ON extraction.namespace_id = transition.namespace_id
             AND extraction.account_id = transition.account_id
             AND extraction.character_id = transition.character_id
             AND extraction.terminal_id = transition.extraction_id
            LEFT JOIN item_instances AS item
              ON item.namespace_id = transition.namespace_id
             AND item.item_uid = transition.item_uid
            LEFT JOIN item_ledger_events AS ledger
              ON ledger.namespace_id = transition.namespace_id
             AND ledger.ledger_event_id = transition.ledger_event_id
            WHERE transition.namespace_id = NEW.namespace_id
              AND transition.account_id = NEW.account_id
              AND transition.mutation_id = NEW.mutation_id
              AND (
                  transition.character_id IS DISTINCT FROM NEW.character_id
                  OR transition.extraction_id IS DISTINCT FROM NEW.extraction_id
                  OR transition.stack_index IS DISTINCT FROM NEW.stack_index
                  OR transition.content_revision IS DISTINCT FROM NEW.content_revision
                  OR transition.disposition_kind IS DISTINCT FROM NEW.action_kind
                  OR (
                      NEW.action_kind = 0
                      AND (
                          transition.destination_kind IS DISTINCT FROM NEW.destination_kind
                          OR transition.destination_slot_index
                              IS DISTINCT FROM NEW.destination_slot_index
                      )
                  )
                  OR (
                      NEW.action_kind = 1
                      AND (
                          transition.destination_kind IS DISTINCT FROM 4
                          OR transition.destination_slot_index IS NOT NULL
                      )
                  )
                  OR placement.item_uid IS NULL
                  OR placement.account_id IS DISTINCT FROM NEW.account_id
                  OR placement.character_id IS DISTINCT FROM NEW.character_id
                  OR placement.template_id IS DISTINCT FROM transition.template_id
                  OR placement.item_kind IS DISTINCT FROM transition.item_kind
                  OR placement.destination_kind IS DISTINCT FROM 9
                  OR placement.destination_slot_index IS DISTINCT FROM NEW.stack_index
                  OR placement.post_item_version IS DISTINCT FROM transition.pre_item_version
                  OR placement.post_security_state IS DISTINCT FROM transition.pre_security_state
                  OR extraction.terminal_id IS NULL
                  OR item.item_uid IS NULL
                  OR item.account_id IS DISTINCT FROM NEW.account_id
                  OR (
                      transition.destination_kind IN (5, 4)
                      AND item.character_id IS DISTINCT FROM NEW.character_id
                  )
                  OR (
                      transition.destination_kind IN (6, 8)
                      AND item.character_id IS NOT NULL
                  )
                  OR item.template_id IS DISTINCT FROM transition.template_id
                  OR item.content_revision IS DISTINCT FROM transition.content_revision
                  OR item.item_kind IS DISTINCT FROM transition.item_kind
                  OR item.item_version IS DISTINCT FROM transition.post_item_version
                  OR item.security_state IS DISTINCT FROM transition.post_security_state
                  OR item.location_kind IS DISTINCT FROM transition.destination_kind
                  OR item.slot_index IS DISTINCT FROM transition.destination_slot_index
                  OR item.instance_id IS NOT NULL
                  OR item.pickup_id IS NOT NULL
                  OR item.expires_at_tick IS NOT NULL
                  OR item.destruction_reason IS DISTINCT FROM transition.destruction_reason
                  OR item.terminal_extraction_id IS DISTINCT FROM NEW.extraction_id
                  OR item.extracted_at IS DISTINCT FROM extraction.committed_at
                  OR item.terminal_recall_id IS NOT NULL
                  OR item.recalled_at IS NOT NULL
                  OR (
                      transition.destination_kind = 8
                      AND (
                          item.overflow_expires_at IS DISTINCT FROM
                              item.extracted_at + INTERVAL '72 hours'
                          OR item.overflow_expires_at <= NEW.committed_at
                      )
                  )
                  OR (
                      transition.destination_kind <> 8
                      AND item.overflow_expires_at IS NOT NULL
                  )
                  OR ledger.item_uid IS DISTINCT FROM transition.item_uid
                  OR ledger.account_id IS DISTINCT FROM NEW.account_id
                  OR ledger.character_id IS DISTINCT FROM NEW.character_id
                  OR ledger.mutation_id IS DISTINCT FROM NEW.mutation_id
                  OR ledger.event_kind IS DISTINCT FROM transition.ledger_event_kind
                  OR ledger.source_kind IS DISTINCT FROM 7
                  OR ledger.pre_item_version IS DISTINCT FROM transition.pre_item_version
                  OR ledger.post_item_version IS DISTINCT FROM transition.post_item_version
                  OR ledger.pre_security_state IS DISTINCT FROM 0
                  OR ledger.post_security_state IS DISTINCT FROM transition.post_security_state
                  OR ledger.pre_location_kind IS DISTINCT FROM 9
                  OR ledger.post_location_kind IS DISTINCT FROM transition.destination_kind
                  OR ledger.reason IS DISTINCT FROM transition.destruction_reason
                  OR ledger.terminal_death_id IS NOT NULL
                  OR ledger.terminal_extraction_id IS DISTINCT FROM NEW.extraction_id
                  OR ledger.terminal_recall_id IS NOT NULL
                  OR ledger.committed_at IS DISTINCT FROM NEW.committed_at
              )
        )
        OR (
            NEW.action_kind = 0
            AND NOT EXISTS (
                SELECT 1
                FROM item_instances AS destination_item
                WHERE destination_item.namespace_id = NEW.namespace_id
                  AND destination_item.account_id = NEW.account_id
                  AND destination_item.location_kind = NEW.destination_kind
                  AND destination_item.slot_index = NEW.destination_slot_index
                  AND (
                      (
                          NEW.destination_kind = 5
                          AND destination_item.character_id = NEW.character_id
                      )
                      OR (
                          NEW.destination_kind IN (6, 8)
                          AND destination_item.character_id IS NULL
                      )
                  )
                GROUP BY destination_item.namespace_id,
                    destination_item.account_id,
                    destination_item.location_kind,
                    destination_item.slot_index,
                    destination_item.character_id
                HAVING min(destination_item.item_kind) = max(destination_item.item_kind)
                   AND min(destination_item.template_id) = max(destination_item.template_id)
                   AND min(destination_item.content_revision)
                       = max(destination_item.content_revision)
                   AND (
                       (
                           min(destination_item.item_kind) = 0
                           AND count(*) = 1
                       )
                       OR (
                           min(destination_item.item_kind) = 1
                           AND count(*) BETWEEN 1 AND 6
                       )
                   )
                   AND (
                       NEW.destination_kind <> 8
                       OR count(*) = NEW.transition_count
                   )
            )
        )
        OR NOT EXISTS (
            SELECT 1
            FROM accounts AS account
            JOIN characters AS character
              ON character.namespace_id = account.namespace_id
             AND character.account_id = account.account_id
            JOIN character_world_locations AS world
              ON world.namespace_id = character.namespace_id
             AND world.account_id = character.account_id
             AND world.character_id = character.character_id
            JOIN character_inventories AS inventory
              ON inventory.namespace_id = character.namespace_id
             AND inventory.account_id = character.account_id
             AND inventory.character_id = character.character_id
            WHERE account.namespace_id = NEW.namespace_id
              AND account.account_id = NEW.account_id
              AND account.selected_character_id = NEW.character_id
              AND account.state_version = NEW.post_account_version
              AND character.character_id = NEW.character_id
              AND character.life_state = 0
              AND character.security_state =
                  CASE WHEN NEW.storage_resolution_required THEN 1 ELSE 0 END
              AND character.character_state_version = NEW.post_character_version
              AND world.character_version = NEW.post_world_version
              AND world.location_kind = 1
              AND world.location_content_id = 'hub.lantern_halls_01'
              AND inventory.inventory_version = NEW.post_inventory_version
        )
    THEN
        RAISE EXCEPTION 'ResolutionHold mutation graph or aggregate publication is incomplete';
    END IF;
    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER complete_resolution_hold_mutation_v1
AFTER INSERT ON resolution_hold_mutation_results_v1
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_complete_resolution_hold_mutation_v1();

COMMENT ON TABLE resolution_hold_mutation_results_v1 IS
    'GB-M03-08 exact-replay whole-stack Hold move or confirmed destruction result.';

COMMENT ON TABLE resolution_hold_item_transitions_v1 IS
    'Normalized immutable per-UID transition for one whole logical Hold stack.';

COMMENT ON COLUMN item_ledger_events.source_kind IS
    '0 Starter, 1 Reward, 2 Field, 3 Death, 4 CrashRestore, 5 Extraction, '
    '6 Recall, 7 ResolutionHold.';
