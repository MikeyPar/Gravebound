CREATE TABLE field_equipment_mutations (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    command_id BYTEA NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    preview_hash BYTEA NOT NULL,
    result_hash BYTEA NOT NULL,
    content_revision TEXT NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    incoming_item_uid BYTEA NOT NULL,
    replaced_item_uid BYTEA,
    source_kind SMALLINT NOT NULL,
    source_slot_index SMALLINT,
    source_instance_id BYTEA,
    source_pickup_id BYTEA,
    replacement_slot_index SMALLINT,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id, command_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id) ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, incoming_item_uid)
        REFERENCES item_instances(namespace_id, item_uid) ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, replaced_item_uid)
        REFERENCES item_instances(namespace_id, item_uid) ON DELETE RESTRICT,
    CONSTRAINT field_equipment_command_exact CHECK (
        octet_length(command_id) = 16 AND command_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT field_equipment_hashes_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(preview_hash) = 32
        AND preview_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT field_equipment_content_revision_exact CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT field_equipment_versions_exact CHECK (
        pre_inventory_version > 0 AND post_inventory_version = pre_inventory_version + 1
    ),
    CONSTRAINT field_equipment_incoming_exact CHECK (
        octet_length(incoming_item_uid) = 16
        AND incoming_item_uid <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT field_equipment_replaced_exact CHECK (
        replaced_item_uid IS NULL
        OR (octet_length(replaced_item_uid) = 16
            AND replaced_item_uid <> decode(repeat('00', 16), 'hex')
            AND replaced_item_uid <> incoming_item_uid)
    ),
    CONSTRAINT field_equipment_source_shape CHECK (
        (source_kind = 0 AND source_slot_index BETWEEN 0 AND 7
            AND source_instance_id IS NULL AND source_pickup_id IS NULL)
        OR (source_kind = 1 AND source_slot_index IS NULL
            AND octet_length(source_instance_id) = 16
            AND source_instance_id <> decode(repeat('00', 16), 'hex')
            AND octet_length(source_pickup_id) = 16
            AND source_pickup_id <> decode(repeat('00', 16), 'hex'))
    ),
    CONSTRAINT field_equipment_replacement_shape CHECK (
        (replaced_item_uid IS NULL AND replacement_slot_index IS NULL)
        OR (replaced_item_uid IS NOT NULL AND replacement_slot_index BETWEEN 0 AND 7)
    )
);

CREATE INDEX field_equipment_by_character_version
    ON field_equipment_mutations (
        namespace_id, account_id, character_id, post_inventory_version, command_id
    );
