-- Append the GB-M03-04F safe-storage locations without renumbering any durable state.
-- A schema-27 rollback is safe only when no item uses location_kind 5 or 6. There is no
-- automatic down migration because silently relocating live safe-storage items is prohibited.

ALTER TABLE item_instances
    DROP CONSTRAINT item_instances_namespace_id_account_id_character_id_fkey,
    DROP CONSTRAINT item_location_known,
    DROP CONSTRAINT item_location_shape,
    ALTER COLUMN character_id DROP NOT NULL;

ALTER TABLE item_instances
    ADD CONSTRAINT item_account_owned
        FOREIGN KEY (namespace_id, account_id)
        REFERENCES accounts(namespace_id, account_id) ON DELETE CASCADE,
    ADD CONSTRAINT item_character_custody_owned
        FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id)
        ON DELETE CASCADE,
    ADD CONSTRAINT item_location_known CHECK (location_kind BETWEEN 0 AND 6),
    ADD CONSTRAINT item_location_shape CHECK (
        (location_kind = 0 AND character_id IS NOT NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 3
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state IN (0, 1) AND item_kind = 0)
        OR (location_kind = 1 AND character_id IS NOT NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 1
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state IN (0, 1) AND item_kind = 1)
        OR (location_kind = 2 AND character_id IS NOT NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 7
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state = 2)
        OR (location_kind = 3 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NOT NULL AND octet_length(instance_id) = 16
            AND instance_id <> decode(repeat('00', 16), 'hex')
            AND pickup_id IS NOT NULL AND octet_length(pickup_id) = 16
            AND pickup_id <> decode(repeat('00', 16), 'hex')
            AND expires_at_tick IS NOT NULL AND expires_at_tick > 0
            AND destruction_reason IS NULL AND security_state = 2)
        OR (location_kind = 4 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason = 'ground_expired' AND security_state = 3)
        OR (location_kind = 5 AND character_id IS NOT NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 7
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state = 0)
        OR (location_kind = 6 AND character_id IS NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 159
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state = 0)
    );

ALTER TABLE item_ledger_events
    DROP CONSTRAINT ledger_location_known,
    ADD CONSTRAINT ledger_location_known CHECK (
        (pre_location_kind IS NULL OR pre_location_kind BETWEEN 0 AND 6)
        AND post_location_kind BETWEEN 0 AND 6
    );

CREATE UNIQUE INDEX one_character_safe_equipment_per_slot
    ON item_instances (namespace_id, account_id, character_id, slot_index)
    WHERE location_kind = 5 AND item_kind = 0;

CREATE UNIQUE INDEX one_vault_equipment_per_slot
    ON item_instances (namespace_id, account_id, slot_index)
    WHERE location_kind = 6 AND item_kind = 0;

CREATE INDEX character_safe_units_by_projected_stack
    ON item_instances (
        namespace_id, account_id, character_id, slot_index, template_id, item_uid
    )
    WHERE location_kind = 5;

CREATE INDEX vault_units_by_projected_stack
    ON item_instances (namespace_id, account_id, slot_index, template_id, item_uid)
    WHERE location_kind = 6;

COMMENT ON COLUMN item_instances.location_kind IS
    '0 Equipped, 1 Belt, 2 RunBackpack, 3 PersonalGround, 4 Destroyed, '
    '5 CharacterSafe, 6 Vault. Schema-27 rollback requires zero rows in 5 or 6.';
