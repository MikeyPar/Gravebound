-- GB-M03-08 atomic production extraction terminal.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-011, LOOT-002, LOOT-033,
--   LOOT-050, LOOT-060, and TECH-015/021-023.
-- - Gravebound_Content_Production_Spec_v1.md CONT-HUB-001/002, the Core
--   Bell Sepulcher/Caldus route, and CONT-VALID-001.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-03 and GB-M03-08.
-- - Accepted SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md.
--
-- Schema 26 remains an explicitly disposable evidence seam. Production extraction writes the
-- normalized v1 graph below in one serializable transaction and never commits schema 26 first.
-- The terminal result owns exact replay bytes while normalized placements, material credits,
-- ledgers, audit, and outbox rows keep support and integrity queries bounded.
--
-- Recovery/downgrade:
-- A pre-0056 binary may be restored only after proving this complete graph is empty, every
-- extraction-terminal foreign key is null, no run material has terminal_reason='extraction',
-- and all material-wallet balances are zero at version 1. Published migration history must never
-- be rewritten or down-migrated in place.

ALTER TABLE characters
    DROP CONSTRAINT character_security_state_core,
    ADD CONSTRAINT character_security_state_core CHECK (
        (life_state = 0 AND security_state IN (0, 1))
        OR (life_state = 1 AND security_state = 0)
    );

CREATE TABLE character_extraction_terminal_results_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    extraction_request_id BYTEA NOT NULL,
    extraction_receipt_id BYTEA NOT NULL,
    contract_version SMALLINT NOT NULL,
    terminal_kind SMALLINT NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    canonical_plan_hash BYTEA NOT NULL,
    result_hash BYTEA NOT NULL,
    result_payload BYTEA NOT NULL,
    encounter_id BYTEA NOT NULL,
    instance_lineage_id BYTEA NOT NULL,
    entry_restore_point_id BYTEA NOT NULL,
    exit_instance_id BYTEA NOT NULL,
    source_content_id TEXT NOT NULL,
    destination_content_id TEXT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    observed_tick BIGINT NOT NULL,
    committed_tick BIGINT NOT NULL,
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
    placement_count SMALLINT NOT NULL,
    material_credit_count SMALLINT NOT NULL,
    storage_resolution_required BOOLEAN NOT NULL,
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    UNIQUE (namespace_id, terminal_id),
    UNIQUE (namespace_id, extraction_request_id),
    UNIQUE (namespace_id, extraction_receipt_id),
    UNIQUE (namespace_id, account_id, character_id, terminal_id),
    UNIQUE (namespace_id, account_id, character_id, terminal_id, mutation_id),
    UNIQUE (namespace_id, account_id, character_id, extraction_request_id),
    UNIQUE (
        namespace_id, account_id, character_id,
        entry_restore_point_id, instance_lineage_id
    ),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, encounter_id, exit_instance_id)
        REFERENCES caldus_victory_exits(namespace_id, encounter_id, exit_instance_id),
    FOREIGN KEY (namespace_id, account_id, character_id, instance_lineage_id)
        REFERENCES character_instance_lineages(namespace_id, account_id, character_id, lineage_id),
    FOREIGN KEY (namespace_id, account_id, character_id, entry_restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ),
    CONSTRAINT extraction_terminal_ids_exact CHECK (
        octet_length(account_id) = 16
        AND octet_length(character_id) = 16
        AND octet_length(mutation_id) = 16
        AND octet_length(terminal_id) = 16
        AND octet_length(extraction_request_id) = 16
        AND octet_length(extraction_receipt_id) = 16
        AND octet_length(encounter_id) = 16
        AND octet_length(instance_lineage_id) = 16
        AND octet_length(entry_restore_point_id) = 16
        AND octet_length(exit_instance_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
        AND character_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND terminal_id <> decode(repeat('00', 16), 'hex')
        AND extraction_request_id <> decode(repeat('00', 16), 'hex')
        AND extraction_receipt_id <> decode(repeat('00', 16), 'hex')
        AND encounter_id <> decode(repeat('00', 16), 'hex')
        AND instance_lineage_id <> decode(repeat('00', 16), 'hex')
        AND entry_restore_point_id <> decode(repeat('00', 16), 'hex')
        AND exit_instance_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> terminal_id
        AND mutation_id <> extraction_request_id
        AND mutation_id <> extraction_receipt_id
        AND terminal_id <> extraction_request_id
        AND terminal_id <> extraction_receipt_id
        AND extraction_request_id <> extraction_receipt_id
    ),
    CONSTRAINT extraction_terminal_hashes_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(canonical_plan_hash) = 32
        AND canonical_plan_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT extraction_terminal_content_exact CHECK (
        contract_version = 1
        AND terminal_kind = 2
        AND
        source_content_id = 'portal.exit.dungeon.bell_sepulcher'
        AND destination_content_id = 'hub.lantern_halls_01'
        AND records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT extraction_terminal_time_order CHECK (
        issued_at <= committed_at
        AND observed_tick > 0
        AND committed_tick = observed_tick
    ),
    CONSTRAINT extraction_terminal_security_exact CHECK (
        pre_character_security_state = 0
        AND post_character_security_state IN (0, 1)
        AND storage_resolution_required = (post_character_security_state = 1)
    ),
    CONSTRAINT extraction_terminal_versions_exact CHECK (
        pre_account_version > 0
        AND post_account_version IN (pre_account_version, pre_account_version + 1)
        AND pre_character_version > 0
        AND post_character_version = pre_character_version + 1
        AND pre_world_version = pre_character_version
        AND post_world_version = post_character_version
        AND pre_inventory_version > 0
        AND post_inventory_version = pre_inventory_version + 1
        AND pre_life_metrics_version > 0
        AND post_life_metrics_version = pre_life_metrics_version + 1
    ),
    CONSTRAINT extraction_terminal_counts_bounded CHECK (
        placement_count BETWEEN 0 AND 64
        AND material_credit_count BETWEEN 0 AND 4
    )
);

ALTER TABLE character_extraction_results
    ADD COLUMN production_mutation_id BYTEA,
    DROP CONSTRAINT extraction_state_shape,
    ADD CONSTRAINT extraction_production_mutation_shape CHECK (
        production_mutation_id IS NULL
        OR (
            octet_length(production_mutation_id) = 16
            AND production_mutation_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT extraction_state_shape CHECK (
        (extraction_state = 0
            AND extraction_receipt_id IS NULL
            AND receipt_payload_hash IS NULL
            AND authority_kind IS NULL
            AND destination_content_id IS NULL
            AND safe_arrival_kind IS NULL
            AND committed_at IS NULL
            AND transfer_mutation_id IS NULL
            AND post_character_version IS NULL
            AND transferred_at IS NULL
            AND production_mutation_id IS NULL)
        OR
        (extraction_state = 1
            AND extraction_receipt_id IS NOT NULL
            AND receipt_payload_hash IS NOT NULL
            AND authority_kind = 0
            AND destination_content_id = 'hub.lantern_halls_01'
            AND safe_arrival_kind = 0
            AND committed_at IS NOT NULL
            AND production_mutation_id IS NULL
            AND ((transfer_mutation_id IS NULL
                    AND post_character_version IS NULL
                    AND transferred_at IS NULL)
                OR (octet_length(transfer_mutation_id) = 16
                    AND transfer_mutation_id <> decode(repeat('00', 16), 'hex')
                    AND post_character_version > expected_character_version
                    AND transferred_at IS NOT NULL)))
        OR
        (extraction_state = 1
            AND extraction_receipt_id IS NOT NULL
            AND receipt_payload_hash IS NOT NULL
            AND authority_kind = 1
            AND destination_content_id = 'hub.lantern_halls_01'
            AND safe_arrival_kind = 0
            AND committed_at IS NOT NULL
            AND production_mutation_id IS NOT NULL
            AND transfer_mutation_id = production_mutation_id
            AND post_character_version = expected_character_version + 1
            AND transferred_at = committed_at)
    ),
    ADD CONSTRAINT extraction_production_result_owned FOREIGN KEY (
        namespace_id, account_id, production_mutation_id
    ) REFERENCES character_extraction_terminal_results_v1(
        namespace_id, account_id, mutation_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE character_extraction_terminal_results_v1
    ADD CONSTRAINT extraction_terminal_requested_seam_owned FOREIGN KEY (
        namespace_id, extraction_request_id
    ) REFERENCES character_extraction_results(
        namespace_id, extraction_request_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE character_entry_restore_points
    ADD COLUMN extraction_terminal_id BYTEA,
    ADD CONSTRAINT restore_extraction_terminal_shape CHECK (
        (restore_state = 1 AND (
            extraction_terminal_id IS NULL
            OR (
                octet_length(extraction_terminal_id) = 16
                AND extraction_terminal_id <> decode(repeat('00', 16), 'hex')
            )
        ))
        OR (restore_state <> 1 AND extraction_terminal_id IS NULL)
    ),
    ADD CONSTRAINT restore_extraction_terminal_owned FOREIGN KEY (
        namespace_id, account_id, character_id, extraction_terminal_id
    ) REFERENCES character_extraction_terminal_results_v1(
        namespace_id, account_id, character_id, terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

COMMENT ON COLUMN character_entry_restore_points.extraction_terminal_id IS
    'Production restore_state 1 terminal identity. Null state-1 rows are legacy schema-26 evidence.';

CREATE TABLE account_material_wallet_balances_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    material_id TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    wallet_cap INTEGER NOT NULL,
    material_version BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, material_id),
    FOREIGN KEY (namespace_id, account_id)
        REFERENCES accounts(namespace_id, account_id) ON DELETE CASCADE,
    CONSTRAINT material_wallet_account_exact CHECK (
        octet_length(account_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT material_wallet_core_definition CHECK (
        (material_id = 'material.bell_brass' AND wallet_cap = 999)
        OR (material_id = 'material.funeral_root' AND wallet_cap = 999)
        OR (material_id = 'material.saltglass_shard' AND wallet_cap = 999)
        OR (material_id = 'material.echo_ember' AND wallet_cap = 99)
    ),
    CONSTRAINT material_wallet_quantity_bounded CHECK (
        quantity BETWEEN 0 AND wallet_cap
    ),
    CONSTRAINT material_wallet_version_positive CHECK (material_version > 0)
);

INSERT INTO account_material_wallet_balances_v1 (
    namespace_id, account_id, material_id, quantity, wallet_cap, material_version
)
SELECT account.namespace_id, account.account_id, material.material_id, 0, material.wallet_cap, 1
FROM accounts AS account
CROSS JOIN (
    VALUES
        ('material.bell_brass', 999),
        ('material.funeral_root', 999),
        ('material.saltglass_shard', 999),
        ('material.echo_ember', 99)
) AS material(material_id, wallet_cap);

CREATE FUNCTION initialize_core_material_wallet_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO account_material_wallet_balances_v1 (
        namespace_id, account_id, material_id, quantity, wallet_cap, material_version
    )
    VALUES
        (NEW.namespace_id, NEW.account_id, 'material.bell_brass', 0, 999, 1),
        (NEW.namespace_id, NEW.account_id, 'material.funeral_root', 0, 999, 1),
        (NEW.namespace_id, NEW.account_id, 'material.saltglass_shard', 0, 999, 1),
        (NEW.namespace_id, NEW.account_id, 'material.echo_ember', 0, 99, 1);
    RETURN NEW;
END
$$;

CREATE TRIGGER account_material_wallet_initialize_v1
AFTER INSERT ON accounts
FOR EACH ROW EXECUTE FUNCTION initialize_core_material_wallet_v1();

ALTER TABLE character_run_material_stacks
    ADD COLUMN terminal_extraction_id BYTEA,
    ADD COLUMN extracted_at TIMESTAMPTZ,
    DROP CONSTRAINT run_material_security_shape,
    ADD CONSTRAINT run_material_security_shape CHECK (
        (security_state = 2 AND quantity > 0
            AND terminal_reason IS NULL
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NULL
            AND terminal_extraction_id IS NULL
            AND extracted_at IS NULL)
        OR (security_state = 3 AND quantity = 0
            AND terminal_reason = 'permadeath'
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NOT NULL
            AND octet_length(terminal_death_id) = 16
            AND terminal_death_id <> decode(repeat('00', 16), 'hex')
            AND terminal_extraction_id IS NULL
            AND extracted_at IS NULL)
        OR (security_state = 3 AND quantity = 0
            AND terminal_reason = 'crash_revoked'
            AND terminal_restore_point_id IS NOT NULL
            AND octet_length(terminal_restore_point_id) = 16
            AND terminal_restore_point_id <> decode(repeat('00', 16), 'hex')
            AND terminal_death_id IS NULL
            AND terminal_extraction_id IS NULL
            AND extracted_at IS NULL)
        OR (security_state = 4 AND quantity = 0
            AND terminal_reason = 'extraction'
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NULL
            AND terminal_extraction_id IS NOT NULL
            AND octet_length(terminal_extraction_id) = 16
            AND terminal_extraction_id <> decode(repeat('00', 16), 'hex')
            AND extracted_at IS NOT NULL)
    ),
    ADD CONSTRAINT run_material_extraction_owned FOREIGN KEY (
        namespace_id, terminal_extraction_id
    ) REFERENCES character_extraction_terminal_results_v1(
        namespace_id, terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

-- terminal_extraction_id is durable provenance, not merely current location state. It survives
-- later safe transfers, deliberate re-risking, consumption, expiry, and a later death. Overflow
-- and ResolutionHold still require a current extraction identity and exact expiry/ownership.
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
    ADD CONSTRAINT item_extraction_terminal_owned FOREIGN KEY (
        namespace_id, terminal_extraction_id
    ) REFERENCES character_extraction_terminal_results_v1(
        namespace_id, terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE item_ledger_events
    ADD CONSTRAINT item_ledger_extraction_identity_v1 UNIQUE (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, event_kind, source_kind, pre_item_version,
        post_item_version, pre_security_state, post_security_state,
        pre_location_kind, post_location_kind, terminal_extraction_id
    ),
    ADD CONSTRAINT item_ledger_extraction_terminal_owned FOREIGN KEY (
        namespace_id, terminal_extraction_id
    ) REFERENCES character_extraction_terminal_results_v1(
        namespace_id, terminal_id
    ) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE extraction_terminal_item_placements_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    placement_ordinal SMALLINT NOT NULL,
    item_uid BYTEA NOT NULL,
    template_id TEXT NOT NULL,
    item_kind SMALLINT NOT NULL,
    source_kind SMALLINT NOT NULL,
    source_slot_index SMALLINT NOT NULL,
    destination_kind SMALLINT NOT NULL,
    destination_slot_index SMALLINT NOT NULL,
    pre_item_version BIGINT NOT NULL,
    post_item_version BIGINT NOT NULL,
    pre_security_state SMALLINT NOT NULL,
    post_security_state SMALLINT NOT NULL,
    ledger_event_id BYTEA NOT NULL,
    ledger_event_kind SMALLINT NOT NULL,
    ledger_source_kind SMALLINT NOT NULL,
    PRIMARY KEY (namespace_id, terminal_id, placement_ordinal),
    UNIQUE (namespace_id, terminal_id, item_uid),
    FOREIGN KEY (
        namespace_id, account_id, character_id, terminal_id, mutation_id
    )
        REFERENCES character_extraction_terminal_results_v1(
            namespace_id, account_id, character_id, terminal_id, mutation_id
        ) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid),
    FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, ledger_event_kind, ledger_source_kind,
        pre_item_version, post_item_version, pre_security_state,
        post_security_state, source_kind, destination_kind, terminal_id
    ) REFERENCES item_ledger_events(
        namespace_id, account_id, character_id, item_uid, mutation_id,
        ledger_event_id, event_kind, source_kind, pre_item_version,
        post_item_version, pre_security_state, post_security_state,
        pre_location_kind, post_location_kind, terminal_extraction_id
    ) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT extraction_placement_ordinal_bounded CHECK (
        placement_ordinal BETWEEN 0 AND 63
    ),
    CONSTRAINT extraction_placement_ids_exact CHECK (
        octet_length(item_uid) = 16
        AND item_uid <> decode(repeat('00', 16), 'hex')
        AND octet_length(ledger_event_id) = 16
        AND ledger_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT extraction_placement_item_shape CHECK (
        length(template_id) BETWEEN 3 AND 96
        AND item_kind IN (0, 1)
        AND (
            (item_kind = 0 AND source_kind IN (0, 2) AND destination_kind <> 1)
            OR (item_kind = 1 AND source_kind IN (1, 2) AND destination_kind <> 0)
        )
    ),
    CONSTRAINT extraction_placement_source_shape CHECK (
        (source_kind = 0 AND source_slot_index BETWEEN 0 AND 3
            AND pre_security_state = 1)
        OR (source_kind = 1 AND source_slot_index BETWEEN 0 AND 1
            AND pre_security_state = 1)
        OR (source_kind = 2 AND source_slot_index BETWEEN 0 AND 7
            AND pre_security_state = 2)
    ),
    CONSTRAINT extraction_placement_destination_shape CHECK (
        (destination_kind = 0 AND destination_slot_index BETWEEN 0 AND 3
            AND source_kind = 0 AND destination_slot_index = source_slot_index)
        OR (destination_kind = 1 AND destination_slot_index BETWEEN 0 AND 1
            AND (source_kind = 2
                OR (source_kind = 1 AND destination_slot_index = source_slot_index)))
        OR (destination_kind = 5 AND destination_slot_index BETWEEN 0 AND 7
            AND source_kind = 2)
        OR (destination_kind = 6 AND destination_slot_index BETWEEN 0 AND 159
            AND source_kind = 2)
        OR (destination_kind = 8 AND destination_slot_index BETWEEN 0 AND 19
            AND source_kind = 2)
        OR (destination_kind = 9 AND destination_slot_index BETWEEN 0 AND 7
            AND source_kind = 2)
    ),
    CONSTRAINT extraction_placement_versions_exact CHECK (
        pre_item_version > 0 AND post_item_version = pre_item_version + 1
    ),
    CONSTRAINT extraction_placement_security_exact CHECK (
        post_security_state = 0
        AND ledger_event_kind = 1
        AND ledger_source_kind = 5
    )
);

CREATE TABLE account_material_ledger_events_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    material_id TEXT NOT NULL,
    ledger_event_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    event_kind SMALLINT NOT NULL,
    delta INTEGER NOT NULL,
    pre_balance INTEGER NOT NULL,
    post_balance INTEGER NOT NULL,
    pre_wallet_version BIGINT NOT NULL,
    post_wallet_version BIGINT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, ledger_event_id),
    UNIQUE (namespace_id, account_id, material_id, post_wallet_version),
    UNIQUE (namespace_id, terminal_id, material_id),
    UNIQUE (
        namespace_id, account_id, material_id, terminal_id, ledger_event_id,
        mutation_id, event_kind, delta, pre_balance, post_balance,
        pre_wallet_version, post_wallet_version
    ),
    UNIQUE (
        namespace_id, account_id, material_id, terminal_id, ledger_event_id,
        mutation_id, delta, pre_balance, post_balance,
        pre_wallet_version, post_wallet_version
    ),
    FOREIGN KEY (namespace_id, account_id, material_id)
        REFERENCES account_material_wallet_balances_v1(
            namespace_id, account_id, material_id
        ),
    FOREIGN KEY (namespace_id, account_id, mutation_id)
        REFERENCES character_extraction_terminal_results_v1(
            namespace_id, account_id, mutation_id
        ) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, terminal_id)
        REFERENCES character_extraction_terminal_results_v1(
            namespace_id, terminal_id
        ) ON DELETE CASCADE,
    CONSTRAINT material_ledger_ids_exact CHECK (
        octet_length(ledger_event_id) = 16
        AND ledger_event_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(terminal_id) = 16
        AND terminal_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT material_ledger_extraction_exact CHECK (
        event_kind = 0
        AND delta BETWEEN 1 AND 99
        AND pre_balance >= 0
        AND post_balance = pre_balance + delta
        AND pre_wallet_version > 0
        AND post_wallet_version = pre_wallet_version + 1
    )
);

CREATE TABLE extraction_terminal_material_credits_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    credit_ordinal SMALLINT NOT NULL,
    material_id TEXT NOT NULL,
    credited_quantity INTEGER NOT NULL,
    wallet_cap INTEGER NOT NULL,
    pre_wallet_quantity INTEGER NOT NULL,
    post_wallet_quantity INTEGER NOT NULL,
    pre_wallet_version BIGINT NOT NULL,
    post_wallet_version BIGINT NOT NULL,
    pre_pouch_quantity INTEGER NOT NULL,
    post_pouch_quantity INTEGER NOT NULL,
    pre_pouch_version BIGINT NOT NULL,
    post_pouch_version BIGINT NOT NULL,
    wallet_ledger_event_id BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, terminal_id, credit_ordinal),
    UNIQUE (namespace_id, terminal_id, material_id),
    FOREIGN KEY (
        namespace_id, account_id, character_id, terminal_id, mutation_id
    )
        REFERENCES character_extraction_terminal_results_v1(
            namespace_id, account_id, character_id, terminal_id, mutation_id
        ) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, material_id)
        REFERENCES account_material_wallet_balances_v1(
            namespace_id, account_id, material_id
        ),
    FOREIGN KEY (namespace_id, account_id, character_id, material_id)
        REFERENCES character_run_material_stacks(
            namespace_id, account_id, character_id, material_id
        ),
    FOREIGN KEY (
        namespace_id, account_id, material_id, terminal_id,
        wallet_ledger_event_id, mutation_id, credited_quantity,
        pre_wallet_quantity, post_wallet_quantity,
        pre_wallet_version, post_wallet_version
    ) REFERENCES account_material_ledger_events_v1(
        namespace_id, account_id, material_id, terminal_id,
        ledger_event_id, mutation_id, delta, pre_balance, post_balance,
        pre_wallet_version, post_wallet_version
    ) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT extraction_credit_ordinal_bounded CHECK (
        credit_ordinal BETWEEN 0 AND 3
    ),
    CONSTRAINT extraction_credit_quantity_exact CHECK (
        credited_quantity BETWEEN 1 AND 99
        AND pre_pouch_quantity = credited_quantity
        AND post_pouch_quantity = 0
        AND pre_wallet_quantity >= 0
        AND post_wallet_quantity = pre_wallet_quantity + credited_quantity
        AND post_wallet_quantity <= wallet_cap
    ),
    CONSTRAINT extraction_credit_versions_exact CHECK (
        pre_wallet_version > 0
        AND post_wallet_version = pre_wallet_version + 1
        AND pre_pouch_version > 0
        AND post_pouch_version = pre_pouch_version + 1
    ),
    CONSTRAINT extraction_credit_ledger_id_exact CHECK (
        octet_length(wallet_ledger_event_id) = 16
        AND wallet_ledger_event_id <> decode(repeat('00', 16), 'hex')
    )
);

CREATE TABLE extraction_terminal_audit_events_v1 (
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
        REFERENCES character_extraction_terminal_results_v1(
            namespace_id, account_id, character_id, terminal_id
        ) ON DELETE CASCADE,
    CONSTRAINT extraction_audit_id_exact CHECK (
        octet_length(audit_event_id) = 16
        AND audit_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT extraction_audit_type_exact CHECK (
        event_type = 'extraction_committed'
    ),
    CONSTRAINT extraction_audit_digest_exact CHECK (
        octet_length(event_digest) = 32
        AND event_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE extraction_terminal_conflict_audits_v1 (
    namespace_id TEXT NOT NULL,
    extraction_request_id BYTEA NOT NULL,
    conflict_audit_id BYTEA NOT NULL,
    attempted_account_id BYTEA NOT NULL,
    attempted_character_id BYTEA NOT NULL,
    attempted_mutation_id BYTEA NOT NULL,
    stored_request_hash BYTEA NOT NULL,
    attempted_request_hash BYTEA NOT NULL,
    attempted_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, conflict_audit_id),
    UNIQUE (namespace_id, extraction_request_id, attempted_request_hash),
    FOREIGN KEY (namespace_id, extraction_request_id)
        REFERENCES character_extraction_terminal_results_v1(
            namespace_id, extraction_request_id
        ) ON DELETE CASCADE,
    CONSTRAINT extraction_conflict_ids_exact CHECK (
        octet_length(conflict_audit_id) = 16
        AND conflict_audit_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(attempted_account_id) = 16
        AND attempted_account_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(attempted_character_id) = 16
        AND attempted_character_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(attempted_mutation_id) = 16
        AND attempted_mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT extraction_conflict_hashes_exact CHECK (
        octet_length(stored_request_hash) = 32
        AND stored_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_request_hash) = 32
        AND attempted_request_hash <> decode(repeat('00', 32), 'hex')
        AND stored_request_hash <> attempted_request_hash
    )
);

CREATE TABLE extraction_terminal_outbox_events_v1 (
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
        REFERENCES character_extraction_terminal_results_v1(
            namespace_id, account_id, character_id, terminal_id
        ) ON DELETE CASCADE,
    CONSTRAINT extraction_outbox_event_id_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT extraction_outbox_event_type_exact CHECK (
        event_type = 'extraction_committed'
    ),
    CONSTRAINT extraction_outbox_payload_bounded CHECK (
        octet_length(event_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT extraction_outbox_publish_order CHECK (
        published_at IS NULL OR published_at >= created_at
    )
);

CREATE INDEX extraction_terminals_by_character_time_v1
    ON character_extraction_terminal_results_v1 (
        namespace_id, account_id, character_id, committed_at DESC, terminal_id
    );

CREATE INDEX extraction_hold_by_terminal_v1
    ON extraction_terminal_item_placements_v1 (
        namespace_id, account_id, character_id, terminal_id,
        destination_slot_index, placement_ordinal
    )
    WHERE destination_kind = 9;

CREATE INDEX unpublished_extraction_terminal_events_v1
    ON extraction_terminal_outbox_events_v1 (
        namespace_id, created_at, event_id
    )
    WHERE published_at IS NULL;

CREATE FUNCTION enforce_extraction_terminal_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_terminal_id BYTEA;
    terminal_time TIMESTAMPTZ;
BEGIN
    IF TG_TABLE_NAME = 'character_extraction_terminal_results_v1' THEN
        IF NEW.committed_at IS DISTINCT FROM transaction_timestamp() THEN
            RAISE EXCEPTION 'extraction terminal commit time is PostgreSQL transaction authority';
        END IF;
        RETURN NEW;
    END IF;
    target_terminal_id := NEW.terminal_id;
    SELECT committed_at INTO terminal_time
    FROM character_extraction_terminal_results_v1
    WHERE namespace_id = NEW.namespace_id AND terminal_id = target_terminal_id;
    IF NOT FOUND OR terminal_time IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION '% may be inserted only with its owning extraction terminal', TG_TABLE_NAME;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER extraction_terminal_result_insert_window_v1
BEFORE INSERT ON character_extraction_terminal_results_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_insert_window_v1();

CREATE TRIGGER extraction_terminal_placement_insert_window_v1
BEFORE INSERT ON extraction_terminal_item_placements_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_insert_window_v1();

CREATE TRIGGER extraction_terminal_credit_insert_window_v1
BEFORE INSERT ON extraction_terminal_material_credits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_insert_window_v1();

CREATE TRIGGER extraction_terminal_material_ledger_insert_window_v1
BEFORE INSERT ON account_material_ledger_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_insert_window_v1();

CREATE TRIGGER extraction_terminal_audit_insert_window_v1
BEFORE INSERT ON extraction_terminal_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_insert_window_v1();

CREATE TRIGGER extraction_terminal_outbox_insert_window_v1
BEFORE INSERT ON extraction_terminal_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_insert_window_v1();

CREATE FUNCTION enforce_extraction_terminal_history_immutable_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'extraction terminal history is immutable';
END
$$;

CREATE TRIGGER extraction_terminal_result_immutable_v1
BEFORE UPDATE OR DELETE ON character_extraction_terminal_results_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_history_immutable_v1();

CREATE TRIGGER extraction_terminal_placement_immutable_v1
BEFORE UPDATE OR DELETE ON extraction_terminal_item_placements_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_history_immutable_v1();

CREATE TRIGGER extraction_terminal_credit_immutable_v1
BEFORE UPDATE OR DELETE ON extraction_terminal_material_credits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_history_immutable_v1();

CREATE TRIGGER extraction_terminal_material_ledger_immutable_v1
BEFORE UPDATE OR DELETE ON account_material_ledger_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_history_immutable_v1();

CREATE TRIGGER extraction_terminal_audit_immutable_v1
BEFORE UPDATE OR DELETE ON extraction_terminal_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_history_immutable_v1();

CREATE TRIGGER extraction_terminal_conflict_immutable_v1
BEFORE UPDATE OR DELETE ON extraction_terminal_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_history_immutable_v1();

CREATE FUNCTION enforce_extraction_terminal_outbox_publish_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'extraction terminal outbox history is immutable';
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
    RAISE EXCEPTION 'extraction terminal outbox permits only first publication';
END
$$;

CREATE TRIGGER extraction_terminal_outbox_publish_only_v1
BEFORE UPDATE OR DELETE ON extraction_terminal_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_extraction_terminal_outbox_publish_v1();

CREATE TRIGGER dead_extraction_terminal_result_insert_v1
BEFORE INSERT ON character_extraction_terminal_results_v1
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();

CREATE FUNCTION enforce_complete_extraction_terminal_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    actual_placements BIGINT;
    actual_credits BIGINT;
    account_changed BOOLEAN;
BEGIN
    SELECT count(*) INTO actual_placements
    FROM extraction_terminal_item_placements_v1
    WHERE namespace_id = NEW.namespace_id AND terminal_id = NEW.terminal_id;

    SELECT count(*) INTO actual_credits
    FROM extraction_terminal_material_credits_v1
    WHERE namespace_id = NEW.namespace_id AND terminal_id = NEW.terminal_id;

    IF actual_placements <> NEW.placement_count
        OR actual_credits <> NEW.material_credit_count
        OR EXISTS (
            SELECT 1
            FROM generate_series(0, NEW.placement_count - 1) AS expected(ordinal)
            LEFT JOIN extraction_terminal_item_placements_v1 AS placement
              ON placement.namespace_id = NEW.namespace_id
             AND placement.terminal_id = NEW.terminal_id
             AND placement.placement_ordinal = expected.ordinal
            WHERE placement.placement_ordinal IS NULL
        )
        OR EXISTS (
            SELECT 1
            FROM generate_series(0, NEW.material_credit_count - 1) AS expected(ordinal)
            LEFT JOIN extraction_terminal_material_credits_v1 AS credit
              ON credit.namespace_id = NEW.namespace_id
             AND credit.terminal_id = NEW.terminal_id
             AND credit.credit_ordinal = expected.ordinal
            WHERE credit.credit_ordinal IS NULL
        )
        OR NEW.storage_resolution_required <> EXISTS (
            SELECT 1 FROM extraction_terminal_item_placements_v1
            WHERE namespace_id = NEW.namespace_id
              AND terminal_id = NEW.terminal_id
              AND destination_kind = 9
        )
    THEN
        RAISE EXCEPTION 'extraction terminal projection count or ordering is incomplete';
    END IF;

    SELECT (
        actual_credits > 0
        OR EXISTS (
            SELECT 1 FROM extraction_terminal_item_placements_v1
            WHERE namespace_id = NEW.namespace_id
              AND terminal_id = NEW.terminal_id
              AND source_kind = 2
              AND destination_kind IN (6, 8)
        )
    ) INTO account_changed;

    IF NEW.post_account_version
            <> NEW.pre_account_version + CASE WHEN account_changed THEN 1 ELSE 0 END
        OR NOT EXISTS (
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
              AND security_state = NEW.post_character_security_state
              AND character_state_version = NEW.post_character_version
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_world_locations
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND character_version = NEW.post_world_version
              AND location_kind = 1
              AND location_content_id = 'hub.lantern_halls_01'
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
              AND life_metrics_version = NEW.post_life_metrics_version
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_entry_restore_points
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND restore_point_id = NEW.entry_restore_point_id
              AND lineage_id = NEW.instance_lineage_id
              AND restore_state = 1
              AND extraction_terminal_id = NEW.terminal_id
              AND consumed_at = NEW.committed_at
              AND records_blake3 = NEW.records_blake3
              AND assets_blake3 = NEW.assets_blake3
              AND localization_blake3 = NEW.localization_blake3
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_extraction_results
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND extraction_request_id = NEW.extraction_request_id
              AND extraction_receipt_id = NEW.extraction_receipt_id
              AND extraction_state = 1
              AND authority_kind = 1
              AND destination_content_id = NEW.destination_content_id
              AND safe_arrival_kind = 0
              AND committed_at = NEW.committed_at
              AND transfer_mutation_id = NEW.mutation_id
              AND production_mutation_id = NEW.mutation_id
              AND post_character_version = NEW.post_character_version
              AND transferred_at = NEW.committed_at
        )
        OR NOT EXISTS (
            SELECT 1 FROM character_instance_lineages
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND lineage_id = NEW.instance_lineage_id
              AND lineage_state = 2
              AND closed_at = NEW.committed_at
              AND records_blake3 = NEW.records_blake3
              AND assets_blake3 = NEW.assets_blake3
              AND localization_blake3 = NEW.localization_blake3
        )
    THEN
        RAISE EXCEPTION 'extraction terminal aggregate or danger-root closure is incomplete';
    END IF;

    IF (SELECT count(*) FROM item_instances
        WHERE namespace_id = NEW.namespace_id
          AND terminal_extraction_id = NEW.terminal_id) <> NEW.placement_count
        OR (SELECT count(*) FROM item_ledger_events
            WHERE namespace_id = NEW.namespace_id
              AND terminal_extraction_id = NEW.terminal_id) <> NEW.placement_count
        OR EXISTS (
            SELECT 1
            FROM extraction_terminal_item_placements_v1 AS placement
            LEFT JOIN item_instances AS item
              ON item.namespace_id = placement.namespace_id
             AND item.item_uid = placement.item_uid
            LEFT JOIN item_ledger_events AS ledger
              ON ledger.namespace_id = placement.namespace_id
             AND ledger.ledger_event_id = placement.ledger_event_id
            WHERE placement.namespace_id = NEW.namespace_id
              AND placement.terminal_id = NEW.terminal_id
              AND (
                  item.account_id IS DISTINCT FROM NEW.account_id
                  OR item.template_id IS DISTINCT FROM placement.template_id
                  OR item.item_kind IS DISTINCT FROM placement.item_kind
                  OR item.item_version IS DISTINCT FROM placement.post_item_version
                  OR item.security_state IS DISTINCT FROM placement.post_security_state
                  OR item.location_kind IS DISTINCT FROM placement.destination_kind
                  OR item.slot_index IS DISTINCT FROM placement.destination_slot_index
                  OR item.terminal_extraction_id IS DISTINCT FROM NEW.terminal_id
                  OR item.extracted_at IS DISTINCT FROM NEW.committed_at
                  OR (
                      placement.destination_kind IN (6, 8)
                      AND item.character_id IS NOT NULL
                  )
                  OR (
                      placement.destination_kind NOT IN (6, 8)
                      AND item.character_id IS DISTINCT FROM NEW.character_id
                  )
                  OR ledger.item_uid IS DISTINCT FROM placement.item_uid
                  OR ledger.account_id IS DISTINCT FROM NEW.account_id
                  OR ledger.character_id IS DISTINCT FROM NEW.character_id
                  OR ledger.mutation_id IS DISTINCT FROM NEW.mutation_id
                  OR ledger.event_kind IS DISTINCT FROM 1
                  OR ledger.source_kind IS DISTINCT FROM 5
                  OR ledger.pre_item_version IS DISTINCT FROM placement.pre_item_version
                  OR ledger.post_item_version IS DISTINCT FROM placement.post_item_version
                  OR ledger.pre_security_state IS DISTINCT FROM placement.pre_security_state
                  OR ledger.post_security_state IS DISTINCT FROM placement.post_security_state
                  OR ledger.pre_location_kind IS DISTINCT FROM placement.source_kind
                  OR ledger.post_location_kind IS DISTINCT FROM placement.destination_kind
                  OR ledger.reason IS NOT NULL
                  OR ledger.terminal_extraction_id IS DISTINCT FROM NEW.terminal_id
                  OR ledger.committed_at IS DISTINCT FROM NEW.committed_at
              )
        )
        OR EXISTS (
            SELECT 1 FROM item_instances
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.character_id
              AND (
                  location_kind = 2
                  OR (location_kind IN (0, 1)
                      AND (security_state <> 0
                          OR terminal_extraction_id IS DISTINCT FROM NEW.terminal_id))
              )
        )
    THEN
        RAISE EXCEPTION 'extraction terminal item custody or ledger is incomplete';
    END IF;

    IF (SELECT count(*) FROM character_run_material_stacks
        WHERE namespace_id = NEW.namespace_id
          AND terminal_extraction_id = NEW.terminal_id) <> NEW.material_credit_count
        OR (SELECT count(*) FROM account_material_ledger_events_v1
            WHERE namespace_id = NEW.namespace_id
              AND terminal_id = NEW.terminal_id) <> NEW.material_credit_count
        OR EXISTS (
            SELECT 1
            FROM extraction_terminal_material_credits_v1 AS credit
            LEFT JOIN account_material_wallet_balances_v1 AS wallet
              ON wallet.namespace_id = credit.namespace_id
             AND wallet.account_id = credit.account_id
             AND wallet.material_id = credit.material_id
            LEFT JOIN character_run_material_stacks AS pouch
              ON pouch.namespace_id = credit.namespace_id
             AND pouch.account_id = credit.account_id
             AND pouch.character_id = credit.character_id
             AND pouch.material_id = credit.material_id
            LEFT JOIN account_material_ledger_events_v1 AS ledger
              ON ledger.namespace_id = credit.namespace_id
             AND ledger.ledger_event_id = credit.wallet_ledger_event_id
            WHERE credit.namespace_id = NEW.namespace_id
              AND credit.terminal_id = NEW.terminal_id
              AND (
                  wallet.quantity IS DISTINCT FROM credit.post_wallet_quantity
                  OR wallet.wallet_cap IS DISTINCT FROM credit.wallet_cap
                  OR wallet.material_version IS DISTINCT FROM credit.post_wallet_version
                  OR pouch.quantity IS DISTINCT FROM credit.post_pouch_quantity
                  OR pouch.material_version IS DISTINCT FROM credit.post_pouch_version
                  OR pouch.security_state IS DISTINCT FROM 4
                  OR pouch.terminal_reason IS DISTINCT FROM 'extraction'
                  OR pouch.terminal_restore_point_id IS NOT NULL
                  OR pouch.terminal_death_id IS NOT NULL
                  OR pouch.terminal_extraction_id IS DISTINCT FROM NEW.terminal_id
                  OR pouch.extracted_at IS DISTINCT FROM NEW.committed_at
                  OR ledger.account_id IS DISTINCT FROM NEW.account_id
                  OR ledger.material_id IS DISTINCT FROM credit.material_id
                  OR ledger.mutation_id IS DISTINCT FROM NEW.mutation_id
                  OR ledger.terminal_id IS DISTINCT FROM NEW.terminal_id
                  OR ledger.event_kind IS DISTINCT FROM 0
                  OR ledger.delta IS DISTINCT FROM credit.credited_quantity
                  OR ledger.pre_balance IS DISTINCT FROM credit.pre_wallet_quantity
                  OR ledger.post_balance IS DISTINCT FROM credit.post_wallet_quantity
                  OR ledger.pre_wallet_version IS DISTINCT FROM credit.pre_wallet_version
                  OR ledger.post_wallet_version IS DISTINCT FROM credit.post_wallet_version
                  OR ledger.committed_at IS DISTINCT FROM NEW.committed_at
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
        RAISE EXCEPTION 'extraction terminal material conversion is incomplete';
    END IF;

    IF (SELECT count(*) FROM extraction_terminal_audit_events_v1
        WHERE namespace_id = NEW.namespace_id
          AND terminal_id = NEW.terminal_id
          AND event_type = 'extraction_committed'
          AND event_digest = NEW.result_hash
          AND created_at = NEW.committed_at) <> 1
        OR (SELECT count(*) FROM extraction_terminal_outbox_events_v1
            WHERE namespace_id = NEW.namespace_id
              AND terminal_id = NEW.terminal_id
              AND event_type = 'extraction_committed'
              AND event_payload = NEW.result_payload
              AND created_at = NEW.committed_at) <> 1
    THEN
        RAISE EXCEPTION 'extraction terminal audit or outbox is incomplete';
    END IF;

    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER complete_extraction_terminal_v1
AFTER INSERT ON character_extraction_terminal_results_v1
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_complete_extraction_terminal_v1();

COMMENT ON TABLE character_extraction_terminal_results_v1 IS
    'GB-M03-08 production extraction result; exact replay bytes plus normalized atomic authority.';

COMMENT ON COLUMN item_instances.terminal_extraction_id IS
    'Most recent successful extraction provenance; retained through later custody transitions.';

COMMENT ON COLUMN character_run_material_stacks.security_state IS
    '2 AtRiskPending, 3 Destroyed, 4 Extracted-to-safe-wallet terminal tombstone.';

COMMENT ON COLUMN characters.security_state IS
    '0 Normal, 1 StorageResolutionRequired. State 1 is living-only and blocks normal admission.';
