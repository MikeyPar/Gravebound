-- GB-M03-08 atomic production Emergency Recall terminal.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-010, LOOT-002, LOOT-033,
--   LOOT-060, and TECH-015/021-023.
-- - Gravebound_Content_Production_Spec_v1.md CONT-HUB-001/002, the Core
--   microrealm/dungeon/boss route, and CONT-VALID-001.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-03 and GB-M03-08.
-- - Accepted SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md.
--
-- Append-only discriminants:
--   terminal kind 3 EmergencyRecall, 4 DisconnectRecovery;
--   restore state 3 RecallCommitted; item-ledger source 6 Recall.
--
-- Recovery/downgrade:
-- A pre-0057 binary may be restored only after proving the complete Recall graph is empty,
-- no restore root is state 3, and every terminal_recall_id/recall_terminal_id is null.
-- Published migration history must never be rewritten or down-migrated in place.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM character_entry_restore_points
        WHERE restore_state = 3
        LIMIT 1
    ) THEN
        RAISE EXCEPTION
            '0057 requires no pre-existing RecallCommitted roots in the wipeable Core namespace';
    END IF;
END
$$;

CREATE TABLE character_recall_terminal_results_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    contract_version SMALLINT NOT NULL,
    terminal_kind SMALLINT NOT NULL,
    trigger_kind SMALLINT NOT NULL,
    explicit_request_sequence BIGINT,
    canonical_request_hash BYTEA NOT NULL,
    canonical_plan_hash BYTEA NOT NULL,
    result_hash BYTEA NOT NULL,
    result_payload BYTEA NOT NULL,
    instance_lineage_id BYTEA NOT NULL,
    entry_restore_point_id BYTEA NOT NULL,
    source_content_id TEXT NOT NULL,
    destination_content_id TEXT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    trigger_started_tick BIGINT NOT NULL,
    completion_tick BIGINT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    pre_character_security_state SMALLINT NOT NULL,
    post_character_security_state SMALLINT NOT NULL,
    pre_account_version BIGINT NOT NULL,
    post_account_version BIGINT NOT NULL,
    pre_character_version BIGINT NOT NULL,
    post_character_version BIGINT NOT NULL,
    pre_world_version BIGINT NOT NULL,
    post_world_version BIGINT NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    pre_life_metrics_version BIGINT NOT NULL,
    post_life_metrics_version BIGINT NOT NULL,
    pre_lifetime_ticks BIGINT NOT NULL,
    post_lifetime_ticks BIGINT NOT NULL,
    pre_permadeath_combat_ticks BIGINT NOT NULL,
    post_permadeath_combat_ticks BIGINT NOT NULL,
    preserved_progression_version BIGINT NOT NULL,
    preserved_oath_bargain_version BIGINT NOT NULL,
    preserved_ash_wallet_version BIGINT NOT NULL,
    stabilized_item_count SMALLINT NOT NULL,
    destroyed_item_count INTEGER NOT NULL,
    destroyed_material_stack_count SMALLINT NOT NULL,
    result_code SMALLINT NOT NULL,
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    UNIQUE (namespace_id, terminal_id),
    UNIQUE (namespace_id, account_id, character_id, terminal_id),
    UNIQUE (
        namespace_id, account_id, character_id, terminal_id, mutation_id
    ),
    UNIQUE (
        namespace_id, account_id, character_id,
        entry_restore_point_id, instance_lineage_id
    ),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, instance_lineage_id)
        REFERENCES character_instance_lineages(namespace_id, account_id, character_id, lineage_id),
    FOREIGN KEY (namespace_id, account_id, character_id, entry_restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ),
    CONSTRAINT recall_terminal_ids_exact CHECK (
        octet_length(account_id) = 16
        AND octet_length(character_id) = 16
        AND octet_length(mutation_id) = 16
        AND octet_length(terminal_id) = 16
        AND octet_length(instance_lineage_id) = 16
        AND octet_length(entry_restore_point_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
        AND character_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND terminal_id <> decode(repeat('00', 16), 'hex')
        AND instance_lineage_id <> decode(repeat('00', 16), 'hex')
        AND entry_restore_point_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> terminal_id
    ),
    CONSTRAINT recall_terminal_hashes_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(canonical_plan_hash) = 32
        AND canonical_plan_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_payload) BETWEEN 1 AND 1048576
    ),
    CONSTRAINT recall_terminal_content_exact CHECK (
        contract_version = 1
        AND length(source_content_id) BETWEEN 3 AND 96
        AND source_content_id ~ '^[a-z0-9._-]+$'
        AND destination_content_id = 'hub.lantern_halls_01'
        AND records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT recall_terminal_trigger_exact CHECK (
        (
            terminal_kind = 3
            AND trigger_kind = 0
            AND explicit_request_sequence IS NOT NULL
            AND explicit_request_sequence BETWEEN 1 AND 4294967295
            AND completion_tick = trigger_started_tick + 12
        )
        OR (
            terminal_kind = 4
            AND trigger_kind = 1
            AND explicit_request_sequence IS NULL
            AND completion_tick = trigger_started_tick + 90
        )
    ),
    CONSTRAINT recall_terminal_time_order CHECK (
        issued_at <= committed_at
        AND trigger_started_tick > 0
        AND completion_tick > trigger_started_tick
    ),
    CONSTRAINT recall_terminal_security_exact CHECK (
        pre_character_security_state = 0
        AND post_character_security_state = 0
    ),
    CONSTRAINT recall_terminal_versions_exact CHECK (
        pre_account_version > 0
        AND post_account_version = pre_account_version
        AND pre_character_version > 0
        AND post_character_version = pre_character_version + 1
        AND pre_world_version = pre_character_version
        AND post_world_version = post_character_version
        AND pre_inventory_version > 0
        AND post_inventory_version = pre_inventory_version + 1
        AND pre_life_metrics_version > 0
        AND post_life_metrics_version = pre_life_metrics_version + 1
        AND preserved_progression_version > 0
        AND preserved_oath_bargain_version > 0
        AND preserved_ash_wallet_version > 0
    ),
    CONSTRAINT recall_terminal_clocks_monotonic CHECK (
        pre_lifetime_ticks >= 0
        AND post_lifetime_ticks >= pre_lifetime_ticks
        AND pre_permadeath_combat_ticks >= 0
        AND post_permadeath_combat_ticks >= pre_permadeath_combat_ticks
    ),
    CONSTRAINT recall_terminal_counts_bounded CHECK (
        stabilized_item_count BETWEEN 0 AND 16
        AND destroyed_item_count BETWEEN 0 AND 4096
        AND destroyed_material_stack_count BETWEEN 0 AND 4
        AND result_code = 1
    )
);

CREATE TABLE recall_terminal_item_stabilizations_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    stabilization_ordinal SMALLINT NOT NULL,
    item_uid BYTEA NOT NULL,
    template_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    item_kind SMALLINT NOT NULL,
    source_kind SMALLINT NOT NULL,
    source_slot_index SMALLINT NOT NULL,
    pre_item_version BIGINT NOT NULL,
    post_item_version BIGINT NOT NULL,
    pre_security_state SMALLINT NOT NULL,
    post_security_state SMALLINT NOT NULL,
    destination_kind SMALLINT NOT NULL,
    ledger_event_id BYTEA NOT NULL,
    ledger_event_kind SMALLINT NOT NULL,
    ledger_source_kind SMALLINT NOT NULL,
    PRIMARY KEY (namespace_id, terminal_id, stabilization_ordinal),
    UNIQUE (namespace_id, terminal_id, item_uid),
    FOREIGN KEY (
        namespace_id, account_id, character_id, terminal_id, mutation_id
    ) REFERENCES character_recall_terminal_results_v1(
        namespace_id, account_id, character_id, terminal_id, mutation_id
    ) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid),
    CONSTRAINT recall_stabilization_ordinal_bounded CHECK (
        stabilization_ordinal BETWEEN 0 AND 15
    ),
    CONSTRAINT recall_stabilization_ids_exact CHECK (
        octet_length(item_uid) = 16
        AND item_uid <> decode(repeat('00', 16), 'hex')
        AND octet_length(ledger_event_id) = 16
        AND ledger_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT recall_stabilization_item_exact CHECK (
        length(template_id) BETWEEN 3 AND 96
        AND template_id ~ '^[a-z0-9._-]+$'
        AND length(content_revision) BETWEEN 3 AND 128
        AND content_revision ~ '^[a-z0-9._-]+$'
        AND (
            (item_kind = 0 AND source_kind = 0 AND source_slot_index BETWEEN 0 AND 3)
            OR (item_kind = 1 AND source_kind = 1 AND source_slot_index BETWEEN 0 AND 1)
        )
    ),
    CONSTRAINT recall_stabilization_mutation_exact CHECK (
        pre_item_version > 0
        AND post_item_version = pre_item_version + 1
        AND pre_security_state = 1
        AND post_security_state = 0
        AND destination_kind = source_kind
        AND ledger_event_kind = 1
        AND ledger_source_kind = 6
    )
);

CREATE TABLE recall_terminal_item_destructions_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    destruction_ordinal INTEGER NOT NULL,
    item_uid BYTEA NOT NULL,
    template_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    item_kind SMALLINT NOT NULL,
    source_kind SMALLINT NOT NULL,
    source_slot_index SMALLINT,
    source_instance_id BYTEA,
    source_pickup_id BYTEA,
    source_expires_at_tick BIGINT,
    pre_item_version BIGINT NOT NULL,
    post_item_version BIGINT NOT NULL,
    pre_security_state SMALLINT NOT NULL,
    post_security_state SMALLINT NOT NULL,
    destination_kind SMALLINT NOT NULL,
    destruction_reason TEXT NOT NULL,
    ledger_event_id BYTEA NOT NULL,
    ledger_event_kind SMALLINT NOT NULL,
    ledger_source_kind SMALLINT NOT NULL,
    PRIMARY KEY (namespace_id, terminal_id, destruction_ordinal),
    UNIQUE (namespace_id, terminal_id, item_uid),
    FOREIGN KEY (
        namespace_id, account_id, character_id, terminal_id, mutation_id
    ) REFERENCES character_recall_terminal_results_v1(
        namespace_id, account_id, character_id, terminal_id, mutation_id
    ) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid),
    CONSTRAINT recall_destruction_ordinal_bounded CHECK (
        destruction_ordinal BETWEEN 0 AND 4095
    ),
    CONSTRAINT recall_destruction_ids_exact CHECK (
        octet_length(item_uid) = 16
        AND item_uid <> decode(repeat('00', 16), 'hex')
        AND octet_length(ledger_event_id) = 16
        AND ledger_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT recall_destruction_item_exact CHECK (
        length(template_id) BETWEEN 3 AND 96
        AND template_id ~ '^[a-z0-9._-]+$'
        AND length(content_revision) BETWEEN 3 AND 128
        AND content_revision ~ '^[a-z0-9._-]+$'
        AND item_kind IN (0, 1)
    ),
    CONSTRAINT recall_destruction_source_exact CHECK (
        (
            source_kind = 2
            AND source_slot_index BETWEEN 0 AND 7
            AND source_instance_id IS NULL
            AND source_pickup_id IS NULL
            AND source_expires_at_tick IS NULL
        )
        OR (
            source_kind = 3
            AND source_slot_index IS NULL
            AND source_instance_id IS NOT NULL
            AND octet_length(source_instance_id) = 16
            AND source_instance_id <> decode(repeat('00', 16), 'hex')
            AND source_pickup_id IS NOT NULL
            AND octet_length(source_pickup_id) = 16
            AND source_pickup_id <> decode(repeat('00', 16), 'hex')
            AND source_expires_at_tick IS NOT NULL
            AND source_expires_at_tick > 0
        )
    ),
    CONSTRAINT recall_destruction_mutation_exact CHECK (
        pre_item_version > 0
        AND post_item_version = pre_item_version + 1
        AND pre_security_state = 2
        AND post_security_state = 3
        AND destination_kind = 4
        AND destruction_reason = 'recall'
        AND ledger_event_kind = 2
        AND ledger_source_kind = 6
    )
);

CREATE TABLE recall_terminal_material_destructions_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    destruction_ordinal SMALLINT NOT NULL,
    material_id TEXT NOT NULL,
    destroyed_quantity INTEGER NOT NULL,
    pre_pouch_version BIGINT NOT NULL,
    post_pouch_version BIGINT NOT NULL,
    destruction_event_id BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, terminal_id, destruction_ordinal),
    UNIQUE (namespace_id, terminal_id, material_id),
    FOREIGN KEY (
        namespace_id, account_id, character_id, terminal_id, mutation_id
    ) REFERENCES character_recall_terminal_results_v1(
        namespace_id, account_id, character_id, terminal_id, mutation_id
    ) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, material_id)
        REFERENCES character_run_material_stacks(
            namespace_id, account_id, character_id, material_id
        ),
    CONSTRAINT recall_material_ordinal_bounded CHECK (
        destruction_ordinal BETWEEN 0 AND 3
    ),
    CONSTRAINT recall_material_identity_exact CHECK (
        length(material_id) BETWEEN 3 AND 96
        AND material_id ~ '^[a-z0-9._-]+$'
        AND octet_length(destruction_event_id) = 16
        AND destruction_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT recall_material_mutation_exact CHECK (
        destroyed_quantity BETWEEN 1 AND 99
        AND pre_pouch_version > 0
        AND post_pouch_version = pre_pouch_version + 1
    )
);

CREATE TABLE recall_terminal_audit_events_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    audit_event_id BYTEA NOT NULL,
    event_type TEXT NOT NULL,
    event_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, audit_event_id),
    UNIQUE (namespace_id, terminal_id, event_type),
    FOREIGN KEY (namespace_id, account_id, character_id, terminal_id)
        REFERENCES character_recall_terminal_results_v1(
            namespace_id, account_id, character_id, terminal_id
        ) ON DELETE CASCADE,
    CONSTRAINT recall_audit_id_exact CHECK (
        octet_length(audit_event_id) = 16
        AND audit_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT recall_audit_type_exact CHECK (
        event_type IN ('emergency_recall_committed', 'disconnect_recovery_committed')
    ),
    CONSTRAINT recall_audit_digest_exact CHECK (
        octet_length(event_digest) = 32
        AND event_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE recall_terminal_conflict_audits_v1 (
    namespace_id TEXT NOT NULL,
    stored_terminal_id BYTEA NOT NULL,
    conflict_audit_id BYTEA NOT NULL,
    attempted_account_id BYTEA NOT NULL,
    attempted_character_id BYTEA NOT NULL,
    attempted_mutation_id BYTEA NOT NULL,
    attempted_terminal_id BYTEA NOT NULL,
    attempted_trigger_kind SMALLINT NOT NULL,
    stored_request_hash BYTEA NOT NULL,
    attempted_request_hash BYTEA NOT NULL,
    attempted_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, conflict_audit_id),
    UNIQUE (namespace_id, stored_terminal_id, attempted_request_hash),
    FOREIGN KEY (namespace_id, stored_terminal_id)
        REFERENCES character_recall_terminal_results_v1(namespace_id, terminal_id)
        ON DELETE CASCADE,
    CONSTRAINT recall_conflict_ids_exact CHECK (
        octet_length(stored_terminal_id) = 16
        AND octet_length(conflict_audit_id) = 16
        AND octet_length(attempted_account_id) = 16
        AND octet_length(attempted_character_id) = 16
        AND octet_length(attempted_mutation_id) = 16
        AND octet_length(attempted_terminal_id) = 16
        AND stored_terminal_id <> decode(repeat('00', 16), 'hex')
        AND conflict_audit_id <> decode(repeat('00', 16), 'hex')
        AND attempted_account_id <> decode(repeat('00', 16), 'hex')
        AND attempted_character_id <> decode(repeat('00', 16), 'hex')
        AND attempted_mutation_id <> decode(repeat('00', 16), 'hex')
        AND attempted_terminal_id <> decode(repeat('00', 16), 'hex')
        AND attempted_trigger_kind IN (0, 1)
    ),
    CONSTRAINT recall_conflict_hashes_exact CHECK (
        octet_length(stored_request_hash) = 32
        AND stored_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_request_hash) = 32
        AND attempted_request_hash <> decode(repeat('00', 32), 'hex')
        AND stored_request_hash <> attempted_request_hash
    )
);

CREATE TABLE recall_terminal_outbox_events_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    event_id BYTEA NOT NULL,
    event_type TEXT NOT NULL,
    event_payload BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, terminal_id, event_type),
    FOREIGN KEY (namespace_id, account_id, character_id, terminal_id)
        REFERENCES character_recall_terminal_results_v1(
            namespace_id, account_id, character_id, terminal_id
        ) ON DELETE CASCADE,
    CONSTRAINT recall_outbox_event_id_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT recall_outbox_event_type_exact CHECK (
        event_type IN ('emergency_recall_committed', 'disconnect_recovery_committed')
    ),
    CONSTRAINT recall_outbox_payload_bounded CHECK (
        octet_length(event_payload) BETWEEN 1 AND 1048576
    ),
    CONSTRAINT recall_outbox_publish_order CHECK (
        published_at IS NULL OR published_at >= created_at
    )
);

ALTER TABLE character_entry_restore_points
    ADD COLUMN recall_terminal_id BYTEA,
    ADD CONSTRAINT restore_recall_terminal_shape CHECK (
        (
            restore_state = 3
            AND recall_terminal_id IS NOT NULL
            AND octet_length(recall_terminal_id) = 16
            AND recall_terminal_id <> decode(repeat('00', 16), 'hex')
        )
        OR (restore_state <> 3 AND recall_terminal_id IS NULL)
    ),
    ADD CONSTRAINT restore_recall_terminal_identity UNIQUE (
        namespace_id, account_id, character_id, restore_point_id,
        lineage_id, records_blake3, assets_blake3, localization_blake3,
        recall_terminal_id
    ),
    ADD CONSTRAINT restore_recall_terminal_owned FOREIGN KEY (
        namespace_id, account_id, character_id, recall_terminal_id
    ) REFERENCES character_recall_terminal_results_v1(
        namespace_id, account_id, character_id, terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE character_recall_terminal_results_v1
    ADD CONSTRAINT recall_restore_terminal_owned FOREIGN KEY (
        namespace_id, account_id, character_id, entry_restore_point_id,
        instance_lineage_id, records_blake3, assets_blake3,
        localization_blake3, terminal_id
    ) REFERENCES character_entry_restore_points (
        namespace_id, account_id, character_id, restore_point_id,
        lineage_id, records_blake3, assets_blake3,
        localization_blake3, recall_terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE item_instances
    ADD COLUMN terminal_recall_id BYTEA,
    ADD COLUMN recalled_at TIMESTAMPTZ,
    ADD CONSTRAINT item_recall_identity_shape CHECK (
        (
            terminal_recall_id IS NULL
            AND recalled_at IS NULL
        )
        OR (
            terminal_recall_id IS NOT NULL
            AND octet_length(terminal_recall_id) = 16
            AND terminal_recall_id <> decode(repeat('00', 16), 'hex')
            AND recalled_at IS NOT NULL
        )
    ),
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
    ),
    ADD CONSTRAINT item_recall_terminal_owned FOREIGN KEY (
        namespace_id, terminal_recall_id
    ) REFERENCES character_recall_terminal_results_v1(
        namespace_id, terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE item_ledger_events
    ADD COLUMN terminal_recall_id BYTEA,
    DROP CONSTRAINT ledger_source_kind_known,
    DROP CONSTRAINT ledger_extraction_source_shape,
    DROP CONSTRAINT ledger_creation_shape,
    ADD CONSTRAINT ledger_source_kind_known CHECK (source_kind BETWEEN 0 AND 6),
    ADD CONSTRAINT ledger_recall_identity_shape CHECK (
        terminal_recall_id IS NULL
        OR (
            octet_length(terminal_recall_id) = 16
            AND terminal_recall_id <> decode(repeat('00', 16), 'hex')
        )
    ),
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
            source_kind NOT IN (5, 6)
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
                OR (reason IN ('ground_expired', 'recall')
                    AND terminal_death_id IS NULL)
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
    ADD CONSTRAINT item_ledger_recall_identity_v1 UNIQUE (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, event_kind, source_kind, pre_item_version,
        post_item_version, pre_security_state, post_security_state,
        pre_location_kind, post_location_kind, reason, terminal_recall_id
    ),
    ADD CONSTRAINT item_ledger_recall_identity_without_reason_v1 UNIQUE (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, event_kind, source_kind, pre_item_version,
        post_item_version, pre_security_state, post_security_state,
        pre_location_kind, post_location_kind, terminal_recall_id
    ),
    ADD CONSTRAINT item_ledger_recall_terminal_owned FOREIGN KEY (
        namespace_id, terminal_recall_id
    ) REFERENCES character_recall_terminal_results_v1(
        namespace_id, terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE recall_terminal_item_stabilizations_v1
    ADD CONSTRAINT recall_stabilization_ledger_owned FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, ledger_event_kind, ledger_source_kind,
        pre_item_version, post_item_version, pre_security_state,
        post_security_state, source_kind, destination_kind,
        terminal_id
    ) REFERENCES item_ledger_events (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, event_kind, source_kind,
        pre_item_version, post_item_version, pre_security_state,
        post_security_state, pre_location_kind, post_location_kind,
        terminal_recall_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE recall_terminal_item_destructions_v1
    ADD CONSTRAINT recall_destruction_ledger_owned FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, ledger_event_kind, ledger_source_kind,
        pre_item_version, post_item_version, pre_security_state,
        post_security_state, source_kind, destination_kind,
        destruction_reason, terminal_id
    ) REFERENCES item_ledger_events (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, event_kind, source_kind,
        pre_item_version, post_item_version, pre_security_state,
        post_security_state, pre_location_kind, post_location_kind,
        reason, terminal_recall_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE character_run_material_stacks
    ADD COLUMN terminal_recall_id BYTEA,
    ADD COLUMN recalled_at TIMESTAMPTZ,
    DROP CONSTRAINT run_material_security_shape,
    ADD CONSTRAINT run_material_security_shape CHECK (
        (security_state = 2 AND quantity > 0
            AND terminal_reason IS NULL
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NULL
            AND terminal_extraction_id IS NULL
            AND terminal_recall_id IS NULL
            AND extracted_at IS NULL
            AND recalled_at IS NULL)
        OR (security_state = 3 AND quantity = 0
            AND terminal_reason = 'permadeath'
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NOT NULL
            AND octet_length(terminal_death_id) = 16
            AND terminal_death_id <> decode(repeat('00', 16), 'hex')
            AND terminal_extraction_id IS NULL
            AND terminal_recall_id IS NULL
            AND extracted_at IS NULL
            AND recalled_at IS NULL)
        OR (security_state = 3 AND quantity = 0
            AND terminal_reason = 'crash_revoked'
            AND terminal_restore_point_id IS NOT NULL
            AND octet_length(terminal_restore_point_id) = 16
            AND terminal_restore_point_id <> decode(repeat('00', 16), 'hex')
            AND terminal_death_id IS NULL
            AND terminal_extraction_id IS NULL
            AND terminal_recall_id IS NULL
            AND extracted_at IS NULL
            AND recalled_at IS NULL)
        OR (security_state = 3 AND quantity = 0
            AND terminal_reason = 'recall'
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NULL
            AND terminal_extraction_id IS NULL
            AND terminal_recall_id IS NOT NULL
            AND octet_length(terminal_recall_id) = 16
            AND terminal_recall_id <> decode(repeat('00', 16), 'hex')
            AND extracted_at IS NULL
            AND recalled_at IS NOT NULL)
        OR (security_state = 4 AND quantity = 0
            AND terminal_reason = 'extraction'
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NULL
            AND terminal_extraction_id IS NOT NULL
            AND octet_length(terminal_extraction_id) = 16
            AND terminal_extraction_id <> decode(repeat('00', 16), 'hex')
            AND terminal_recall_id IS NULL
            AND extracted_at IS NOT NULL
            AND recalled_at IS NULL)
    ),
    ADD CONSTRAINT run_material_recall_owned FOREIGN KEY (
        namespace_id, terminal_recall_id
    ) REFERENCES character_recall_terminal_results_v1(
        namespace_id, terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

CREATE INDEX recall_terminals_by_character_time_v1
    ON character_recall_terminal_results_v1 (
        namespace_id, account_id, character_id, committed_at DESC, terminal_id
    );

CREATE INDEX recall_item_provenance_v1
    ON item_instances (
        namespace_id, terminal_recall_id, item_uid
    )
    WHERE terminal_recall_id IS NOT NULL;

CREATE INDEX recall_item_ledger_provenance_v1
    ON item_ledger_events (
        namespace_id, terminal_recall_id, ledger_event_id
    )
    WHERE terminal_recall_id IS NOT NULL;

CREATE INDEX recall_material_provenance_v1
    ON character_run_material_stacks (
        namespace_id, terminal_recall_id, material_id
    )
    WHERE terminal_recall_id IS NOT NULL;

CREATE INDEX unpublished_recall_terminal_events_v1
    ON recall_terminal_outbox_events_v1 (
        namespace_id, created_at, event_id
    )
    WHERE published_at IS NULL;

CREATE FUNCTION enforce_recall_terminal_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    terminal_time TIMESTAMPTZ;
BEGIN
    IF TG_TABLE_NAME = 'character_recall_terminal_results_v1' THEN
        IF NEW.committed_at IS DISTINCT FROM transaction_timestamp() THEN
            RAISE EXCEPTION 'Recall terminal commit time is PostgreSQL transaction authority';
        END IF;
        RETURN NEW;
    END IF;
    SELECT committed_at INTO terminal_time
    FROM character_recall_terminal_results_v1
    WHERE namespace_id = NEW.namespace_id
      AND terminal_id = NEW.terminal_id;
    IF NOT FOUND OR terminal_time IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION '% may be inserted only with its owning Recall terminal', TG_TABLE_NAME;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER recall_terminal_result_insert_window_v1
BEFORE INSERT ON character_recall_terminal_results_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_insert_window_v1();

CREATE TRIGGER dead_recall_terminal_result_insert_v1
BEFORE INSERT ON character_recall_terminal_results_v1
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();

CREATE TRIGGER recall_terminal_stabilization_insert_window_v1
BEFORE INSERT ON recall_terminal_item_stabilizations_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_insert_window_v1();

CREATE TRIGGER recall_terminal_destruction_insert_window_v1
BEFORE INSERT ON recall_terminal_item_destructions_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_insert_window_v1();

CREATE TRIGGER recall_terminal_material_insert_window_v1
BEFORE INSERT ON recall_terminal_material_destructions_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_insert_window_v1();

CREATE TRIGGER recall_terminal_audit_insert_window_v1
BEFORE INSERT ON recall_terminal_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_insert_window_v1();

CREATE TRIGGER recall_terminal_outbox_insert_window_v1
BEFORE INSERT ON recall_terminal_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_insert_window_v1();

CREATE FUNCTION enforce_recall_terminal_history_immutable_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'Recall terminal history is immutable';
END
$$;

CREATE TRIGGER recall_terminal_result_immutable_v1
BEFORE UPDATE OR DELETE ON character_recall_terminal_results_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_history_immutable_v1();

CREATE TRIGGER recall_terminal_stabilization_immutable_v1
BEFORE UPDATE OR DELETE ON recall_terminal_item_stabilizations_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_history_immutable_v1();

CREATE TRIGGER recall_terminal_destruction_immutable_v1
BEFORE UPDATE OR DELETE ON recall_terminal_item_destructions_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_history_immutable_v1();

CREATE TRIGGER recall_terminal_material_immutable_v1
BEFORE UPDATE OR DELETE ON recall_terminal_material_destructions_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_history_immutable_v1();

CREATE TRIGGER recall_terminal_audit_immutable_v1
BEFORE UPDATE OR DELETE ON recall_terminal_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_history_immutable_v1();

CREATE TRIGGER recall_terminal_conflict_immutable_v1
BEFORE UPDATE OR DELETE ON recall_terminal_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_history_immutable_v1();

CREATE FUNCTION enforce_recall_terminal_outbox_publish_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'Recall terminal outbox history is immutable';
    END IF;
    IF OLD.published_at IS NULL
        AND NEW.published_at IS NOT NULL
        AND NEW.namespace_id = OLD.namespace_id
        AND NEW.account_id = OLD.account_id
        AND NEW.character_id = OLD.character_id
        AND NEW.terminal_id = OLD.terminal_id
        AND NEW.event_id = OLD.event_id
        AND NEW.event_type = OLD.event_type
        AND NEW.event_payload = OLD.event_payload
        AND NEW.created_at = OLD.created_at
    THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'Recall terminal outbox permits only first publication';
END
$$;

CREATE TRIGGER recall_terminal_outbox_publish_only_v1
BEFORE UPDATE OR DELETE ON recall_terminal_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_recall_terminal_outbox_publish_v1();

CREATE FUNCTION reject_recall_destroyed_custody_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.location_kind = 4
        AND OLD.security_state = 3
        AND OLD.destruction_reason = 'recall'
        AND OLD.terminal_recall_id IS NOT NULL
    THEN
        RAISE EXCEPTION 'Recall-destroyed item custody is immutable';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER recall_destroyed_item_immutable_v1
BEFORE UPDATE OR DELETE ON item_instances
FOR EACH ROW EXECUTE FUNCTION reject_recall_destroyed_custody_mutation_v1();

CREATE FUNCTION reject_recall_destroyed_material_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.security_state = 3
        AND OLD.quantity = 0
        AND OLD.terminal_reason = 'recall'
        AND OLD.terminal_recall_id IS NOT NULL
    THEN
        RAISE EXCEPTION 'Recall-destroyed material custody is immutable';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER recall_destroyed_material_immutable_v1
BEFORE UPDATE OR DELETE ON character_run_material_stacks
FOR EACH ROW EXECUTE FUNCTION reject_recall_destroyed_material_mutation_v1();

CREATE OR REPLACE FUNCTION enforce_entry_restore_v3_root_immutability()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    valid_terminal_transition BOOLEAN;
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.restore_state = 0
            AND NEW.consumed_at IS NULL
            AND NEW.crash_restore_mutation_id IS NULL
            AND NEW.death_mutation_id IS NULL
            AND NEW.extraction_terminal_id IS NULL
            AND NEW.recall_terminal_id IS NULL
        THEN
            RETURN NEW;
        END IF;
        RAISE EXCEPTION 'danger-entry v3 root must begin Active';
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'danger-entry v3 root history is immutable';
    END IF;
    valid_terminal_transition := OLD.restore_state = 0
        AND NEW.restore_state BETWEEN 1 AND 4
        AND OLD.consumed_at IS NULL
        AND NEW.consumed_at IS NOT NULL
        AND ROW(
            NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.restore_point_id,
            NEW.lineage_id, NEW.source_location_id, NEW.restore_location_id,
            NEW.records_blake3, NEW.assets_blake3, NEW.localization_blake3,
            NEW.snapshot_contract_version, NEW.account_version,
            NEW.character_version, NEW.progression_version, NEW.inventory_version,
            NEW.oath_bargain_version, NEW.life_metrics_version, NEW.ash_wallet_version,
            NEW.component_mask, NEW.composite_digest, NEW.created_at
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.restore_point_id,
            OLD.lineage_id, OLD.source_location_id, OLD.restore_location_id,
            OLD.records_blake3, OLD.assets_blake3, OLD.localization_blake3,
            OLD.snapshot_contract_version, OLD.account_version,
            OLD.character_version, OLD.progression_version, OLD.inventory_version,
            OLD.oath_bargain_version, OLD.life_metrics_version, OLD.ash_wallet_version,
            OLD.component_mask, OLD.composite_digest, OLD.created_at
        );
    IF NOT valid_terminal_transition THEN
        RAISE EXCEPTION 'danger-entry v3 root history is immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE FUNCTION enforce_complete_recall_terminal_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    expected_event_type TEXT;
BEGIN
    expected_event_type := CASE NEW.trigger_kind
        WHEN 0 THEN 'emergency_recall_committed'
        WHEN 1 THEN 'disconnect_recovery_committed'
        ELSE NULL
    END;

    IF (SELECT count(*) FROM recall_terminal_item_stabilizations_v1
        WHERE namespace_id = NEW.namespace_id
          AND terminal_id = NEW.terminal_id) <> NEW.stabilized_item_count
        OR (SELECT count(*) FROM recall_terminal_item_destructions_v1
            WHERE namespace_id = NEW.namespace_id
              AND terminal_id = NEW.terminal_id) <> NEW.destroyed_item_count
        OR (SELECT count(*) FROM recall_terminal_material_destructions_v1
            WHERE namespace_id = NEW.namespace_id
              AND terminal_id = NEW.terminal_id) <> NEW.destroyed_material_stack_count
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT stabilization_ordinal,
                    row_number() OVER (
                        ORDER BY source_kind, source_slot_index, item_uid
                    ) - 1 AS expected_ordinal
                FROM recall_terminal_item_stabilizations_v1
                WHERE namespace_id = NEW.namespace_id
                  AND terminal_id = NEW.terminal_id
            ) AS ordered
            WHERE ordered.stabilization_ordinal <> ordered.expected_ordinal
        )
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT destruction_ordinal,
                    row_number() OVER (
                        ORDER BY source_kind, source_slot_index NULLS LAST,
                            source_instance_id NULLS LAST,
                            source_pickup_id NULLS LAST, item_uid
                    ) - 1 AS expected_ordinal
                FROM recall_terminal_item_destructions_v1
                WHERE namespace_id = NEW.namespace_id
                  AND terminal_id = NEW.terminal_id
            ) AS ordered
            WHERE ordered.destruction_ordinal <> ordered.expected_ordinal
        )
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT destruction_ordinal,
                    row_number() OVER (
                        ORDER BY material_id COLLATE "C"
                    ) - 1 AS expected_ordinal
                FROM recall_terminal_material_destructions_v1
                WHERE namespace_id = NEW.namespace_id
                  AND terminal_id = NEW.terminal_id
            ) AS ordered
            WHERE ordered.destruction_ordinal <> ordered.expected_ordinal
        )
    THEN
        RAISE EXCEPTION 'Recall terminal projection count or canonical ordering is incomplete';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM accounts
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id
          AND selected_character_id = NEW.character_id
          AND state_version = NEW.post_account_version
    )
        OR NOT EXISTS (
            SELECT 1 FROM characters
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND life_state = 0
              AND security_state = 0
              AND character_state_version = NEW.post_character_version
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_world_locations
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND character_version = NEW.post_world_version
              AND location_kind = 1
              AND location_content_id = NEW.destination_content_id
              AND safe_arrival_kind = 0
              AND safe_spawn_id IS NULL
              AND instance_lineage_id IS NULL
              AND entry_restore_point_id IS NULL
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_inventories
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND inventory_version = NEW.post_inventory_version
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_life_metrics
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND lifetime_ticks = NEW.post_lifetime_ticks
              AND permadeath_combat_ticks = NEW.post_permadeath_combat_ticks
              AND life_metrics_version = NEW.post_life_metrics_version
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_progression
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND progression_version = NEW.preserved_progression_version
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_oath_bargain_state
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND oath_bargain_version = NEW.preserved_oath_bargain_version
        )
        OR NOT EXISTS (
            SELECT 1 FROM ash_wallets
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND wallet_version = NEW.preserved_ash_wallet_version
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_entry_restore_points
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND restore_point_id = NEW.entry_restore_point_id
              AND lineage_id = NEW.instance_lineage_id
              AND restore_state = 3
              AND recall_terminal_id = NEW.terminal_id
              AND consumed_at = NEW.committed_at
              AND records_blake3 = NEW.records_blake3
              AND assets_blake3 = NEW.assets_blake3
              AND localization_blake3 = NEW.localization_blake3
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_instance_lineages
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND lineage_id = NEW.instance_lineage_id
              AND content_id = NEW.source_content_id
              AND lineage_state = 2
              AND closed_at = NEW.committed_at
              AND records_blake3 = NEW.records_blake3
              AND assets_blake3 = NEW.assets_blake3
              AND localization_blake3 = NEW.localization_blake3
        )
        OR EXISTS (
            SELECT 1 FROM character_danger_checkpoints
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
        )
    THEN
        RAISE EXCEPTION 'Recall terminal aggregate, clock, or danger-root closure is incomplete';
    END IF;

    IF (SELECT count(*) FROM item_instances
        WHERE namespace_id = NEW.namespace_id
          AND terminal_recall_id = NEW.terminal_id)
        <> NEW.stabilized_item_count + NEW.destroyed_item_count
        OR (SELECT count(*) FROM item_ledger_events
        WHERE namespace_id = NEW.namespace_id
          AND terminal_recall_id = NEW.terminal_id)
        <> NEW.stabilized_item_count + NEW.destroyed_item_count
        OR EXISTS (
            SELECT 1
            FROM recall_terminal_item_stabilizations_v1 AS projection
            LEFT JOIN item_instances AS item
              ON item.namespace_id = projection.namespace_id
             AND item.item_uid = projection.item_uid
            LEFT JOIN item_ledger_events AS ledger
              ON ledger.namespace_id = projection.namespace_id
             AND ledger.ledger_event_id = projection.ledger_event_id
            WHERE projection.namespace_id = NEW.namespace_id
              AND projection.terminal_id = NEW.terminal_id
              AND (
                  item.account_id IS DISTINCT FROM NEW.account_id
                  OR item.character_id IS DISTINCT FROM NEW.character_id
                  OR item.template_id IS DISTINCT FROM projection.template_id
                  OR item.content_revision IS DISTINCT FROM projection.content_revision
                  OR item.item_kind IS DISTINCT FROM projection.item_kind
                  OR item.item_version IS DISTINCT FROM projection.post_item_version
                  OR item.security_state IS DISTINCT FROM 0
                  OR item.location_kind IS DISTINCT FROM projection.source_kind
                  OR item.slot_index IS DISTINCT FROM projection.source_slot_index
                  OR item.terminal_recall_id IS DISTINCT FROM NEW.terminal_id
                  OR item.recalled_at IS DISTINCT FROM NEW.committed_at
                  OR ledger.item_uid IS DISTINCT FROM projection.item_uid
                  OR ledger.account_id IS DISTINCT FROM NEW.account_id
                  OR ledger.character_id IS DISTINCT FROM NEW.character_id
                  OR ledger.mutation_id IS DISTINCT FROM NEW.mutation_id
                  OR ledger.event_kind IS DISTINCT FROM 1
                  OR ledger.source_kind IS DISTINCT FROM 6
                  OR ledger.pre_item_version IS DISTINCT FROM projection.pre_item_version
                  OR ledger.post_item_version IS DISTINCT FROM projection.post_item_version
                  OR ledger.pre_security_state IS DISTINCT FROM 1
                  OR ledger.post_security_state IS DISTINCT FROM 0
                  OR ledger.pre_location_kind IS DISTINCT FROM projection.source_kind
                  OR ledger.post_location_kind IS DISTINCT FROM projection.source_kind
                  OR ledger.reason IS NOT NULL
                  OR ledger.terminal_recall_id IS DISTINCT FROM NEW.terminal_id
                  OR ledger.committed_at IS DISTINCT FROM NEW.committed_at
              )
        )
        OR EXISTS (
            SELECT 1
            FROM recall_terminal_item_destructions_v1 AS projection
            LEFT JOIN item_instances AS item
              ON item.namespace_id = projection.namespace_id
             AND item.item_uid = projection.item_uid
            LEFT JOIN item_ledger_events AS ledger
              ON ledger.namespace_id = projection.namespace_id
             AND ledger.ledger_event_id = projection.ledger_event_id
            WHERE projection.namespace_id = NEW.namespace_id
              AND projection.terminal_id = NEW.terminal_id
              AND (
                  item.account_id IS DISTINCT FROM NEW.account_id
                  OR item.character_id IS DISTINCT FROM NEW.character_id
                  OR item.template_id IS DISTINCT FROM projection.template_id
                  OR item.content_revision IS DISTINCT FROM projection.content_revision
                  OR item.item_kind IS DISTINCT FROM projection.item_kind
                  OR item.item_version IS DISTINCT FROM projection.post_item_version
                  OR item.security_state IS DISTINCT FROM 3
                  OR item.location_kind IS DISTINCT FROM 4
                  OR item.destruction_reason IS DISTINCT FROM 'recall'
                  OR item.terminal_recall_id IS DISTINCT FROM NEW.terminal_id
                  OR item.recalled_at IS DISTINCT FROM NEW.committed_at
                  OR ledger.item_uid IS DISTINCT FROM projection.item_uid
                  OR ledger.account_id IS DISTINCT FROM NEW.account_id
                  OR ledger.character_id IS DISTINCT FROM NEW.character_id
                  OR ledger.mutation_id IS DISTINCT FROM NEW.mutation_id
                  OR ledger.event_kind IS DISTINCT FROM 2
                  OR ledger.source_kind IS DISTINCT FROM 6
                  OR ledger.pre_item_version IS DISTINCT FROM projection.pre_item_version
                  OR ledger.post_item_version IS DISTINCT FROM projection.post_item_version
                  OR ledger.pre_security_state IS DISTINCT FROM 2
                  OR ledger.post_security_state IS DISTINCT FROM 3
                  OR ledger.pre_location_kind IS DISTINCT FROM projection.source_kind
                  OR ledger.post_location_kind IS DISTINCT FROM 4
                  OR ledger.reason IS DISTINCT FROM 'recall'
                  OR ledger.terminal_recall_id IS DISTINCT FROM NEW.terminal_id
                  OR ledger.committed_at IS DISTINCT FROM NEW.committed_at
              )
        )
        OR EXISTS (
            SELECT 1 FROM item_instances
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND security_state IN (1, 2)
        )
    THEN
        RAISE EXCEPTION 'Recall terminal item custody or ledger is incomplete';
    END IF;

    IF (SELECT count(*) FROM character_run_material_stacks
        WHERE namespace_id = NEW.namespace_id
          AND terminal_recall_id = NEW.terminal_id)
        <> NEW.destroyed_material_stack_count
        OR EXISTS (
            SELECT 1
            FROM recall_terminal_material_destructions_v1 AS projection
            LEFT JOIN character_run_material_stacks AS pouch
              ON pouch.namespace_id = projection.namespace_id
             AND pouch.account_id = projection.account_id
             AND pouch.character_id = projection.character_id
             AND pouch.material_id = projection.material_id
            WHERE projection.namespace_id = NEW.namespace_id
              AND projection.terminal_id = NEW.terminal_id
              AND (
                  pouch.quantity IS DISTINCT FROM 0
                  OR pouch.material_version IS DISTINCT FROM projection.post_pouch_version
                  OR pouch.security_state IS DISTINCT FROM 3
                  OR pouch.terminal_reason IS DISTINCT FROM 'recall'
                  OR pouch.terminal_restore_point_id IS NOT NULL
                  OR pouch.terminal_death_id IS NOT NULL
                  OR pouch.terminal_extraction_id IS NOT NULL
                  OR pouch.terminal_recall_id IS DISTINCT FROM NEW.terminal_id
                  OR pouch.recalled_at IS DISTINCT FROM NEW.committed_at
              )
        )
        OR EXISTS (
            SELECT 1 FROM character_run_material_stacks
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND security_state = 2
              AND quantity > 0
        )
    THEN
        RAISE EXCEPTION 'Recall terminal material destruction is incomplete';
    END IF;

    IF (SELECT count(*) FROM recall_terminal_audit_events_v1
        WHERE namespace_id = NEW.namespace_id
          AND terminal_id = NEW.terminal_id
          AND event_type = expected_event_type
          AND event_digest = NEW.result_hash
          AND created_at = NEW.committed_at) <> 1
        OR (SELECT count(*) FROM recall_terminal_outbox_events_v1
            WHERE namespace_id = NEW.namespace_id
              AND terminal_id = NEW.terminal_id
              AND event_type = expected_event_type
              AND event_payload = NEW.result_payload
              AND created_at = NEW.committed_at) <> 1
    THEN
        RAISE EXCEPTION 'Recall terminal audit or outbox is incomplete';
    END IF;

    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER complete_recall_terminal_v1
AFTER INSERT ON character_recall_terminal_results_v1
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_complete_recall_terminal_v1();

COMMENT ON TABLE character_recall_terminal_results_v1 IS
    'GB-M03-08 explicit/LinkLost Recall result and exact replay authority.';

COMMENT ON COLUMN item_instances.terminal_recall_id IS
    'Most recent Recall provenance. Stabilized items may move later; Recall-destroyed rows do not.';

COMMENT ON COLUMN character_entry_restore_points.recall_terminal_id IS
    'Mandatory production identity for reserved restore_state 3 RecallCommitted.';

COMMENT ON COLUMN item_ledger_events.source_kind IS
    '0 Starter, 1 Reward, 2 Field, 3 Death, 4 CrashRestore, 5 Extraction, 6 Recall.';

COMMENT ON COLUMN character_run_material_stacks.security_state IS
    '2 AtRiskPending, 3 Destroyed by death/crash/Recall, 4 Extracted-to-safe-wallet tombstone.';
