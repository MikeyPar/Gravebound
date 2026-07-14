-- Forward-only integrity corrections for the schema-31 durable death foundation.
-- The normal lethal route remains disabled, so every schema-31 death-owned table must still be
-- empty while relational authority is tightened. Existing characters and item history survive.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM death_events)
        OR EXISTS (SELECT 1 FROM death_destruction_entries)
        OR EXISTS (SELECT 1 FROM death_mutation_results)
        OR EXISTS (SELECT 1 FROM echo_records)
        OR EXISTS (SELECT 1 FROM entry_restore_inventory_items_v1)
    THEN
        RAISE EXCEPTION '0032 requires dormant pre-route death and Echo tables';
    END IF;
END
$$;

-- Final damage is the authoritative post-mitigation/post-amplification value and may exceed the
-- raw base damage. Health arithmetic, positivity, and the typed damage pipeline remain bounded.
ALTER TABLE death_events
    DROP CONSTRAINT death_damage_shape,
    ADD CONSTRAINT death_damage_shape CHECK (
        raw_damage >= 0 AND final_damage > 0
        AND damage_type BETWEEN 0 AND 6 AND pre_hit_health > 0
        AND final_damage >= pre_hit_health
    );

ALTER TABLE death_combat_trace_entries
    DROP CONSTRAINT death_trace_damage_shape,
    ADD CONSTRAINT death_trace_damage_shape CHECK (
        raw_damage >= 0 AND final_damage >= 0
        AND damage_type BETWEEN 0 AND 6
        AND pre_health > 0 AND post_health BETWEEN 0 AND pre_health
        AND post_health = GREATEST(0, pre_health - final_damage)
    );

-- Pending run materials are a real versioned aggregate. Core currently authors no run-material
-- grant, but death cannot claim atomic pouch destruction without an authoritative source state.
CREATE TABLE character_run_material_stacks (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    material_id TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    material_version BIGINT NOT NULL,
    security_state SMALLINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id, material_id),
    UNIQUE (
        namespace_id, account_id, character_id, material_id, material_version
    ),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id)
        ON DELETE CASCADE,
    CONSTRAINT run_material_id_bounded CHECK (length(material_id) BETWEEN 3 AND 96),
    CONSTRAINT run_material_version_positive CHECK (material_version > 0),
    CONSTRAINT run_material_security_shape CHECK (
        (security_state = 2 AND quantity > 0)
        OR (security_state = 3 AND quantity = 0)
    )
);

ALTER TABLE item_instances
    ADD CONSTRAINT item_death_custody_identity UNIQUE (
        namespace_id, account_id, character_id, item_uid
    );

ALTER TABLE item_ledger_events
    ADD CONSTRAINT item_ledger_death_identity UNIQUE (
        namespace_id, account_id, character_id, item_uid,
        ledger_event_id, post_item_version
    );

DO $$
DECLARE
    constraint_name name;
BEGIN
    FOR constraint_name IN
        SELECT relation_constraint.conname
        FROM pg_constraint AS relation_constraint
        WHERE relation_constraint.conrelid = 'death_destruction_entries'::regclass
          AND relation_constraint.confrelid IN (
              'item_instances'::regclass,
              'item_ledger_events'::regclass,
              'death_events'::regclass
          )
          AND relation_constraint.contype = 'f'
    LOOP
        EXECUTE format(
            'ALTER TABLE death_destruction_entries DROP CONSTRAINT %I',
            constraint_name
        );
    END LOOP;
END
$$;

ALTER TABLE death_destruction_entries
    DROP CONSTRAINT death_destruction_entry_shape,
    DROP CONSTRAINT death_destruction_location_shape,
    ADD COLUMN account_id BYTEA NOT NULL,
    ADD COLUMN character_id BYTEA NOT NULL,
    ADD COLUMN pre_material_version BIGINT,
    ADD COLUMN post_material_version BIGINT,
    ADD COLUMN pre_material_quantity INTEGER,
    ADD CONSTRAINT death_destruction_owned FOREIGN KEY (
        namespace_id, account_id, character_id, death_id
    ) REFERENCES death_events(
        namespace_id, account_id, character_id, death_id
    ) ON DELETE CASCADE,
    ADD CONSTRAINT death_destruction_item_owned FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid
    ) REFERENCES item_instances(
        namespace_id, account_id, character_id, item_uid
    ) ON DELETE RESTRICT,
    ADD CONSTRAINT death_destruction_ledger_owned FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid,
        ledger_event_id, post_item_version
    ) REFERENCES item_ledger_events(
        namespace_id, account_id, character_id, item_uid,
        ledger_event_id, post_item_version
    ) ON DELETE RESTRICT,
    ADD CONSTRAINT death_destruction_material_owned FOREIGN KEY (
        namespace_id, account_id, character_id, material_id,
        post_material_version
    ) REFERENCES character_run_material_stacks(
        namespace_id, account_id, character_id, material_id,
        material_version
    ) ON DELETE RESTRICT,
    ADD CONSTRAINT death_destruction_entry_shape CHECK (
        (entry_kind = 0
            AND item_uid IS NOT NULL AND octet_length(item_uid) = 16
            AND material_id IS NULL AND quantity = 1
            AND pre_location_kind IS NOT NULL AND pre_location_kind BETWEEN 0 AND 3
            AND pre_item_version IS NOT NULL AND pre_item_version > 0
            AND post_item_version IS NOT NULL
            AND post_item_version = pre_item_version + 1
            AND ledger_event_id IS NOT NULL AND octet_length(ledger_event_id) = 16
            AND pre_material_version IS NULL AND post_material_version IS NULL
            AND pre_material_quantity IS NULL)
        OR (entry_kind = 1
            AND item_uid IS NULL AND ledger_event_id IS NULL
            AND material_id IS NOT NULL AND length(material_id) BETWEEN 3 AND 96
            AND quantity > 0
            AND pre_location_kind IS NULL AND pre_slot_index IS NULL
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL
            AND pre_item_version IS NULL AND post_item_version IS NULL
            AND pre_material_version IS NOT NULL AND pre_material_version > 0
            AND post_material_version IS NOT NULL
            AND post_material_version = pre_material_version + 1
            AND pre_material_quantity IS NOT NULL
            AND pre_material_quantity = quantity)
    ),
    ADD CONSTRAINT death_destruction_location_shape CHECK (
        (entry_kind = 1
            AND pre_location_kind IS NULL AND pre_slot_index IS NULL
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL)
        OR (entry_kind = 0 AND pre_location_kind = 0
            AND pre_slot_index IS NOT NULL AND pre_slot_index BETWEEN 0 AND 3
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL)
        OR (entry_kind = 0 AND pre_location_kind = 1
            AND pre_slot_index IS NOT NULL AND pre_slot_index BETWEEN 0 AND 1
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL)
        OR (entry_kind = 0 AND pre_location_kind = 2
            AND pre_slot_index IS NOT NULL AND pre_slot_index BETWEEN 0 AND 7
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL)
        OR (entry_kind = 0 AND pre_location_kind = 3
            AND pre_slot_index IS NULL
            AND pre_instance_id IS NOT NULL AND octet_length(pre_instance_id) = 16
            AND pre_pickup_id IS NOT NULL AND octet_length(pre_pickup_id) = 16)
    );

-- A receipt is a projection of exactly one immutable request, never a second authority that may
-- point at the same death with different mutation/hash/contract material.
ALTER TABLE death_events
    ADD CONSTRAINT death_request_identity UNIQUE (
        namespace_id, account_id, character_id, death_id,
        mutation_id, contract_kind, canonical_request_hash
    );

DO $$
DECLARE
    constraint_name name;
BEGIN
    FOR constraint_name IN
        SELECT relation_constraint.conname
        FROM pg_constraint AS relation_constraint
        WHERE relation_constraint.conrelid = 'death_mutation_results'::regclass
          AND relation_constraint.confrelid = 'death_events'::regclass
          AND relation_constraint.contype = 'f'
    LOOP
        EXECUTE format(
            'ALTER TABLE death_mutation_results DROP CONSTRAINT %I',
            constraint_name
        );
    END LOOP;
END
$$;

ALTER TABLE death_mutation_results
    ADD CONSTRAINT death_result_request_owned FOREIGN KEY (
        namespace_id, account_id, character_id, death_id,
        mutation_id, contract_kind, canonical_request_hash
    ) REFERENCES death_events(
        namespace_id, account_id, character_id, death_id,
        mutation_id, contract_kind, canonical_request_hash
    ) ON DELETE CASCADE;

-- The final-five timeline is a child of both the immutable summary and its source trace.
ALTER TABLE death_summary_damage_entries
    ADD CONSTRAINT death_summary_damage_parent FOREIGN KEY (
        namespace_id, death_id
    ) REFERENCES death_summary_snapshots(namespace_id, death_id) ON DELETE CASCADE;

ALTER TABLE death_summary_projection_entries
    ADD CONSTRAINT death_summary_projection_item_owned FOREIGN KEY (
        namespace_id, item_uid
    ) REFERENCES item_instances(namespace_id, item_uid) ON DELETE RESTRICT;

-- Echo creation and state history are death-bound and follow only CONT-ECHO-009 legal edges.
ALTER TABLE echo_records
    ADD CONSTRAINT echo_death_identity UNIQUE (namespace_id, echo_id, death_id),
    ADD CONSTRAINT echo_death_outbox_identity UNIQUE (namespace_id, death_id, echo_id);

ALTER TABLE echo_state_transitions
    DROP CONSTRAINT echo_transition_creation_shape,
    ADD CONSTRAINT echo_transition_creation_shape CHECK (
        (transition_ordinal = 0 AND previous_state IS NULL AND next_state = 0
            AND source_death_id IS NOT NULL AND octet_length(source_death_id) = 16)
        OR (transition_ordinal > 0 AND source_death_id IS NULL
            AND (
                (previous_state = 0 AND next_state IN (1, 4))
                OR (previous_state = 1 AND next_state IN (2, 3, 4))
                OR (previous_state = 4 AND next_state = 0)
            ))
    ),
    ADD CONSTRAINT echo_transition_creation_death_owned FOREIGN KEY (
        namespace_id, echo_id, source_death_id
    ) REFERENCES echo_records(namespace_id, echo_id, death_id) ON DELETE CASCADE;

DO $$
DECLARE
    constraint_name name;
BEGIN
    FOR constraint_name IN
        SELECT relation_constraint.conname
        FROM pg_constraint AS relation_constraint
        WHERE relation_constraint.conrelid = 'death_outbox_events'::regclass
          AND relation_constraint.confrelid = 'echo_records'::regclass
          AND relation_constraint.contype = 'f'
    LOOP
        EXECUTE format(
            'ALTER TABLE death_outbox_events DROP CONSTRAINT %I',
            constraint_name
        );
    END LOOP;
END
$$;

ALTER TABLE death_outbox_events
    ADD CONSTRAINT death_outbox_echo_owned FOREIGN KEY (
        namespace_id, death_id, echo_id
    ) REFERENCES echo_records(namespace_id, death_id, echo_id) ON DELETE CASCADE;

ALTER TABLE death_audit_events
    DROP CONSTRAINT death_audit_mutation_id_exact,
    ADD CONSTRAINT death_audit_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    ADD CONSTRAINT death_audit_event_shape CHECK (
        (event_kind IN (0, 1) AND death_id IS NOT NULL)
        OR (event_kind IN (2, 3) AND death_id IS NULL)
    );

ALTER TABLE entry_restore_inventory_items_v1
    ADD COLUMN pre_security_state SMALLINT NOT NULL,
    ADD COLUMN post_security_state SMALLINT NOT NULL,
    ADD CONSTRAINT restore_inventory_item_security_transition CHECK (
        pre_security_state = 0 AND post_security_state = 1
    );
