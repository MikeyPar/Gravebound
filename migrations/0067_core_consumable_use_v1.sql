-- GB-M03 durable Q/E Belt consumable authority.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md INP-001, LOOT-032, TECH-040.
-- - Gravebound_Content_Production_Spec_v1.md CONT-FP-007, CONT-CATALOG-003.
-- - Gravebound_Development_Roadmap_v1.md GB-M03 item/death/Recall persistence gates.
--
-- One immutable receipt binds mutation identity to character, actor generation, lineage, content,
-- inventory version, slot, selected lowest-ID unit, ledger row, and resulting Belt quantities.
-- Existing item discriminants are reused: event 3 / security 4 / location 7 means Consumed.
--
-- Recovery: keep 0067 applied. A schema-66 binary must not run against schema 67. Core remains
-- wipeable; rollback requires a pre-0067 restore or a namespace wipe and intended migration set.
-- Published migration history must never be rewritten or down-migrated in place.

CREATE TABLE core_consumable_use_receipts_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    payload_hash BYTEA NOT NULL,
    actor_generation BIGINT NOT NULL,
    instance_lineage_id BYTEA NOT NULL,
    content_revision TEXT NOT NULL,
    slot_index SMALLINT NOT NULL,
    result_code SMALLINT NOT NULL,
    consumed_item_uid BYTEA,
    ledger_event_id BYTEA,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    belt_one_quantity SMALLINT NOT NULL,
    belt_two_quantity SMALLINT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, actor_generation)
        REFERENCES private_route_generation_allocations_v1(
            namespace_id, account_id, character_id, actor_generation
        ) ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, consumed_item_uid)
        REFERENCES item_instances(namespace_id, item_uid) ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, ledger_event_id)
        REFERENCES item_ledger_events(namespace_id, ledger_event_id) ON DELETE RESTRICT,
    CONSTRAINT core_consumable_account_id_exact CHECK (
        octet_length(account_id) = 16 AND account_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT core_consumable_character_id_exact CHECK (
        octet_length(character_id) = 16 AND character_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT core_consumable_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16 AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT core_consumable_payload_hash_exact CHECK (
        octet_length(payload_hash) = 32 AND payload_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT core_consumable_generation_positive CHECK (actor_generation > 0),
    CONSTRAINT core_consumable_lineage_id_exact CHECK (
        octet_length(instance_lineage_id) = 16
        AND instance_lineage_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT core_consumable_content_revision_exact CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT core_consumable_slot_known CHECK (slot_index BETWEEN 0 AND 1),
    CONSTRAINT core_consumable_result_known CHECK (result_code BETWEEN 0 AND 4),
    CONSTRAINT core_consumable_versions_shape CHECK (
        pre_inventory_version > 0
        AND (
            (result_code = 0 AND post_inventory_version = pre_inventory_version + 1)
            OR (result_code BETWEEN 1 AND 4 AND post_inventory_version = pre_inventory_version)
        )
    ),
    CONSTRAINT core_consumable_quantities_bounded CHECK (
        belt_one_quantity BETWEEN 0 AND 6 AND belt_two_quantity BETWEEN 0 AND 6
    ),
    CONSTRAINT core_consumable_result_shape CHECK (
        (result_code = 0
            AND consumed_item_uid IS NOT NULL
            AND octet_length(consumed_item_uid) = 16
            AND consumed_item_uid <> decode(repeat('00', 16), 'hex')
            AND ledger_event_id IS NOT NULL
            AND octet_length(ledger_event_id) = 16
            AND ledger_event_id <> decode(repeat('00', 16), 'hex'))
        OR (result_code BETWEEN 1 AND 4
            AND consumed_item_uid IS NULL AND ledger_event_id IS NULL)
    )
);

CREATE UNIQUE INDEX core_consumable_consumed_unit_once_v1
    ON core_consumable_use_receipts_v1(namespace_id, consumed_item_uid)
    WHERE consumed_item_uid IS NOT NULL;

CREATE FUNCTION prevent_core_consumable_receipt_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM characters
        WHERE namespace_id = OLD.namespace_id
          AND account_id = OLD.account_id
          AND character_id = OLD.character_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'Core consumable receipts are immutable';
END
$$;

CREATE TRIGGER core_consumable_receipt_immutable_v1
BEFORE UPDATE OR DELETE ON core_consumable_use_receipts_v1
FOR EACH ROW EXECUTE FUNCTION prevent_core_consumable_receipt_mutation_v1();

COMMENT ON TABLE core_consumable_use_receipts_v1 IS
    'GB-M03 immutable exact-replay receipt for server-selected lowest-ID Belt consumption.';
COMMENT ON FUNCTION prevent_core_consumable_receipt_mutation_v1() IS
    'Preserves consumable receipts while permitting explicit wipeable parent aggregate cascade.';
