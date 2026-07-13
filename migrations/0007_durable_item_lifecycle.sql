CREATE TABLE character_inventories (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    inventory_version BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT inventory_account_id_exact CHECK (octet_length(account_id) = 16),
    CONSTRAINT inventory_character_id_exact CHECK (octet_length(character_id) = 16),
    CONSTRAINT inventory_version_positive CHECK (inventory_version > 0)
);

INSERT INTO character_inventories (
    namespace_id, account_id, character_id, inventory_version
)
SELECT namespace_id, account_id, character_id, 1
FROM characters;

CREATE TABLE starter_initializer_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    initializer_revision TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    result_hash BYTEA NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id, initializer_revision),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT starter_revision_exact CHECK (initializer_revision = 'starter.core-dev.v1'),
    CONSTRAINT starter_request_hash_exact CHECK (
        octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT starter_result_hash_exact CHECK (
        octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT starter_versions_advance_once CHECK (
        pre_inventory_version > 0
        AND post_inventory_version = pre_inventory_version + 1
    )
);

CREATE TABLE reward_requests (
    namespace_id TEXT NOT NULL,
    reward_request_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    source_instance_id BYTEA NOT NULL,
    reward_table_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    epoch_id TEXT NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    plan_hash BYTEA NOT NULL,
    result_hash BYTEA NOT NULL,
    audit_digest BYTEA NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, reward_request_id),
    UNIQUE (namespace_id, account_id, character_id, reward_request_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT reward_request_id_exact CHECK (
        octet_length(reward_request_id) = 16
        AND reward_request_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT reward_source_instance_exact CHECK (
        octet_length(source_instance_id) = 16
        AND source_instance_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT reward_table_id_bounded CHECK (length(reward_table_id) BETWEEN 3 AND 96),
    CONSTRAINT reward_content_revision_dev CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT reward_epoch_id_bounded CHECK (length(epoch_id) BETWEEN 1 AND 64),
    CONSTRAINT reward_hashes_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(plan_hash) = 32
        AND plan_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(audit_digest) = 32
        AND audit_digest <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT reward_versions_advance_once CHECK (
        pre_inventory_version > 0
        AND post_inventory_version = pre_inventory_version + 1
    )
);

CREATE TABLE reward_result_entries (
    namespace_id TEXT NOT NULL,
    reward_request_id BYTEA NOT NULL,
    roll_index INTEGER NOT NULL,
    template_id TEXT NOT NULL,
    item_kind SMALLINT NOT NULL,
    quantity SMALLINT NOT NULL,
    item_level SMALLINT,
    rarity SMALLINT,
    PRIMARY KEY (namespace_id, reward_request_id, roll_index),
    FOREIGN KEY (namespace_id, reward_request_id)
        REFERENCES reward_requests(namespace_id, reward_request_id) ON DELETE CASCADE,
    CONSTRAINT reward_roll_index_u16 CHECK (roll_index BETWEEN 0 AND 65535),
    CONSTRAINT reward_entry_template_bounded CHECK (length(template_id) BETWEEN 3 AND 96),
    CONSTRAINT reward_entry_kind_known CHECK (item_kind IN (0, 1)),
    CONSTRAINT reward_entry_quantity_bounded CHECK (quantity BETWEEN 1 AND 6),
    CONSTRAINT reward_entry_shape CHECK (
        (item_kind = 0 AND quantity = 1
            AND item_level IS NOT NULL AND item_level BETWEEN 1 AND 10
            AND rarity IS NOT NULL AND rarity BETWEEN 0 AND 4)
        OR (item_kind = 1 AND item_level IS NULL AND rarity IS NULL)
    )
);

CREATE TABLE item_instances (
    namespace_id TEXT NOT NULL,
    item_uid BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    template_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    item_kind SMALLINT NOT NULL,
    item_level SMALLINT,
    rarity SMALLINT,
    creation_kind SMALLINT NOT NULL,
    creation_request_id BYTEA NOT NULL,
    roll_index INTEGER NOT NULL,
    unit_ordinal INTEGER NOT NULL,
    item_version BIGINT NOT NULL,
    security_state SMALLINT NOT NULL,
    location_kind SMALLINT NOT NULL,
    slot_index SMALLINT,
    instance_id BYTEA,
    pickup_id BYTEA,
    expires_at_tick BIGINT,
    destruction_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, item_uid),
    UNIQUE (namespace_id, creation_kind, creation_request_id, roll_index, unit_ordinal),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT item_uid_exact CHECK (
        octet_length(item_uid) = 16
        AND item_uid <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT item_template_bounded CHECK (length(template_id) BETWEEN 3 AND 96),
    CONSTRAINT item_content_revision_dev CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT item_kind_known CHECK (item_kind IN (0, 1)),
    CONSTRAINT item_shape CHECK (
        (item_kind = 0
            AND item_level IS NOT NULL AND item_level BETWEEN 1 AND 10
            AND rarity IS NOT NULL AND rarity BETWEEN 0 AND 4)
        OR (item_kind = 1 AND item_level IS NULL AND rarity IS NULL)
    ),
    CONSTRAINT item_creation_kind_known CHECK (creation_kind IN (0, 1)),
    CONSTRAINT item_creation_request_exact CHECK (
        octet_length(creation_request_id) = 16
        AND creation_request_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT item_ordinals_u16 CHECK (
        roll_index BETWEEN 0 AND 65535 AND unit_ordinal BETWEEN 0 AND 65535
    ),
    CONSTRAINT item_version_positive CHECK (item_version > 0),
    CONSTRAINT item_security_known CHECK (security_state BETWEEN 0 AND 3),
    CONSTRAINT item_location_known CHECK (location_kind BETWEEN 0 AND 4),
    CONSTRAINT item_location_shape CHECK (
        (location_kind = 0 AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 3
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state IN (0, 1) AND item_kind = 0)
        OR (location_kind = 1 AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 1
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state IN (0, 1) AND item_kind = 1)
        OR (location_kind = 2 AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 7
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state = 2)
        OR (location_kind = 3 AND slot_index IS NULL
            AND instance_id IS NOT NULL AND octet_length(instance_id) = 16
            AND instance_id <> decode(repeat('00', 16), 'hex')
            AND pickup_id IS NOT NULL AND octet_length(pickup_id) = 16
            AND pickup_id <> decode(repeat('00', 16), 'hex')
            AND expires_at_tick IS NOT NULL AND expires_at_tick > 0
            AND destruction_reason IS NULL AND security_state = 2)
        OR (location_kind = 4 AND slot_index IS NULL
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason = 'ground_expired' AND security_state = 3)
    )
);

CREATE UNIQUE INDEX one_equipment_per_slot
    ON item_instances (namespace_id, account_id, character_id, slot_index)
    WHERE location_kind = 0;

CREATE INDEX item_units_by_projected_stack
    ON item_instances (
        namespace_id, account_id, character_id, location_kind, slot_index, template_id, item_uid
    )
    WHERE location_kind IN (1, 2);

CREATE INDEX personal_ground_by_owner_pickup
    ON item_instances (namespace_id, account_id, character_id, instance_id, pickup_id, item_uid)
    WHERE location_kind = 3;

CREATE INDEX personal_ground_by_expiry
    ON item_instances (namespace_id, instance_id, expires_at_tick, pickup_id, item_uid)
    WHERE location_kind = 3;

CREATE TABLE item_ledger_events (
    namespace_id TEXT NOT NULL,
    ledger_event_id BYTEA NOT NULL,
    item_uid BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    event_kind SMALLINT NOT NULL,
    source_kind SMALLINT NOT NULL,
    pre_item_version BIGINT NOT NULL,
    post_item_version BIGINT NOT NULL,
    pre_security_state SMALLINT,
    post_security_state SMALLINT NOT NULL,
    pre_location_kind SMALLINT,
    post_location_kind SMALLINT NOT NULL,
    reason TEXT,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, ledger_event_id),
    UNIQUE (namespace_id, item_uid, post_item_version),
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid) ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id) ON DELETE RESTRICT,
    CONSTRAINT ledger_event_id_exact CHECK (
        octet_length(ledger_event_id) = 16
        AND ledger_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT ledger_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT ledger_event_kind_known CHECK (event_kind BETWEEN 0 AND 2),
    CONSTRAINT ledger_source_kind_known CHECK (source_kind IN (0, 1, 2)),
    CONSTRAINT ledger_versions_exact CHECK (
        pre_item_version >= 0 AND post_item_version = pre_item_version + 1
    ),
    CONSTRAINT ledger_security_known CHECK (
        (pre_security_state IS NULL OR pre_security_state BETWEEN 0 AND 3)
        AND post_security_state BETWEEN 0 AND 3
    ),
    CONSTRAINT ledger_location_known CHECK (
        (pre_location_kind IS NULL OR pre_location_kind BETWEEN 0 AND 4)
        AND post_location_kind BETWEEN 0 AND 4
    ),
    CONSTRAINT ledger_creation_shape CHECK (
        (event_kind = 0 AND pre_item_version = 0
            AND pre_security_state IS NULL AND pre_location_kind IS NULL AND reason IS NULL)
        OR (event_kind = 1 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL AND reason IS NULL)
        OR (event_kind = 2 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND post_security_state = 3 AND post_location_kind = 4
            AND reason = 'ground_expired')
    )
);

CREATE INDEX item_ledger_by_owner_time
    ON item_ledger_events (namespace_id, account_id, character_id, committed_at, ledger_event_id);
