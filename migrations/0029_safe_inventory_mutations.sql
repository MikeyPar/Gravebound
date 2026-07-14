CREATE TABLE safe_inventory_mutations (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    command_kind SMALLINT NOT NULL,
    source_slot_index SMALLINT NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    pre_account_version BIGINT NOT NULL,
    post_account_version BIGINT NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    placement_count SMALLINT NOT NULL,
    result_hash BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id)
        REFERENCES accounts(namespace_id, account_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id)
        ON DELETE CASCADE,
    CONSTRAINT safe_inventory_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT safe_inventory_command_known CHECK (command_kind BETWEEN 0 AND 2),
    CONSTRAINT safe_inventory_source_shape CHECK (
        (command_kind IN (0, 2) AND source_slot_index BETWEEN 0 AND 7)
        OR (command_kind = 1 AND source_slot_index BETWEEN 0 AND 159)
    ),
    CONSTRAINT safe_inventory_request_hash_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT safe_inventory_versions_exact CHECK (
        pre_account_version > 0
        AND pre_inventory_version > 0
        AND post_inventory_version = pre_inventory_version + 1
        AND (
            (command_kind IN (0, 1) AND post_account_version = pre_account_version + 1)
            OR (command_kind = 2 AND post_account_version = pre_account_version)
        )
    ),
    CONSTRAINT safe_inventory_placement_count_bounded CHECK (
        placement_count BETWEEN 1 AND 6
    ),
    CONSTRAINT safe_inventory_result_hash_exact CHECK (
        octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE safe_inventory_placements (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    placement_ordinal SMALLINT NOT NULL,
    item_uid BYTEA NOT NULL,
    destination_kind SMALLINT NOT NULL,
    destination_slot_index SMALLINT NOT NULL,
    pre_item_version BIGINT NOT NULL,
    post_item_version BIGINT NOT NULL,
    PRIMARY KEY (namespace_id, account_id, mutation_id, placement_ordinal),
    UNIQUE (namespace_id, account_id, mutation_id, item_uid),
    FOREIGN KEY (namespace_id, account_id, mutation_id)
        REFERENCES safe_inventory_mutations(namespace_id, account_id, mutation_id)
        ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid) ON DELETE CASCADE,
    CONSTRAINT safe_inventory_placement_ordinal_bounded CHECK (
        placement_ordinal BETWEEN 0 AND 5
    ),
    CONSTRAINT safe_inventory_destination_shape CHECK (
        (destination_kind = 2 AND destination_slot_index BETWEEN 0 AND 7)
        OR (destination_kind = 5 AND destination_slot_index BETWEEN 0 AND 7)
        OR (destination_kind = 6 AND destination_slot_index BETWEEN 0 AND 159)
    ),
    CONSTRAINT safe_inventory_item_versions_exact CHECK (
        pre_item_version > 0 AND post_item_version = pre_item_version + 1
    )
);

CREATE INDEX safe_inventory_mutations_by_character
    ON safe_inventory_mutations (
        namespace_id, account_id, character_id, committed_at, mutation_id
    );
