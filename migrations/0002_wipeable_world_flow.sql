ALTER TABLE characters
    ADD COLUMN character_state_version BIGINT NOT NULL DEFAULT 1,
    ADD CONSTRAINT character_state_version_positive CHECK (character_state_version > 0);

CREATE TABLE character_world_locations (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    character_version BIGINT NOT NULL,
    location_kind SMALLINT NOT NULL,
    location_content_id TEXT,
    safe_arrival_kind SMALLINT,
    safe_spawn_id TEXT,
    instance_lineage_id BYTEA,
    entry_restore_point_id BYTEA,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT world_location_character_version_positive CHECK (character_version > 0),
    CONSTRAINT world_location_kind_known CHECK (location_kind BETWEEN 0 AND 2),
    CONSTRAINT world_location_content_id_bounded CHECK (
        location_content_id IS NULL OR length(location_content_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT world_location_safe_arrival_known CHECK (
        safe_arrival_kind IS NULL OR safe_arrival_kind BETWEEN 0 AND 1
    ),
    CONSTRAINT world_location_safe_spawn_bounded CHECK (
        safe_spawn_id IS NULL OR length(safe_spawn_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT world_location_lineage_exact CHECK (
        instance_lineage_id IS NULL OR (
            octet_length(instance_lineage_id) = 16
            AND instance_lineage_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    CONSTRAINT world_location_restore_exact CHECK (
        entry_restore_point_id IS NULL OR (
            octet_length(entry_restore_point_id) = 16
            AND entry_restore_point_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    CONSTRAINT world_location_shape CHECK (
        (location_kind = 0
            AND location_content_id IS NULL
            AND safe_arrival_kind IS NULL
            AND safe_spawn_id IS NULL
            AND instance_lineage_id IS NULL
            AND entry_restore_point_id IS NULL)
        OR
        (location_kind = 1
            AND location_content_id IS NOT NULL
            AND safe_arrival_kind IS NOT NULL
            AND ((safe_arrival_kind = 0 AND safe_spawn_id IS NULL)
                OR (safe_arrival_kind = 1 AND safe_spawn_id IS NOT NULL))
            AND instance_lineage_id IS NULL
            AND entry_restore_point_id IS NULL)
        OR
        (location_kind = 2
            AND location_content_id IS NOT NULL
            AND safe_arrival_kind IS NULL
            AND safe_spawn_id IS NULL
            AND instance_lineage_id IS NOT NULL
            AND entry_restore_point_id IS NOT NULL)
    )
);

INSERT INTO character_world_locations (
    namespace_id,
    account_id,
    character_id,
    character_version,
    location_kind
)
SELECT namespace_id, account_id, character_id, character_state_version, 0
FROM characters;

CREATE TABLE character_instance_lineages (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    content_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    lineage_state SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    closed_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, lineage_id),
    UNIQUE (namespace_id, account_id, character_id, lineage_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT lineage_id_exact CHECK (
        octet_length(lineage_id) = 16
        AND lineage_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT lineage_content_id_bounded CHECK (length(content_id) BETWEEN 3 AND 96),
    CONSTRAINT lineage_content_revision_bounded CHECK (length(content_revision) BETWEEN 1 AND 128),
    CONSTRAINT lineage_state_known CHECK (lineage_state BETWEEN 0 AND 3),
    CONSTRAINT lineage_closed_shape CHECK (
        (lineage_state IN (0, 1) AND closed_at IS NULL)
        OR (lineage_state IN (2, 3) AND closed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX one_open_lineage_per_character
    ON character_instance_lineages (namespace_id, account_id, character_id)
    WHERE lineage_state IN (0, 1);

CREATE TABLE character_entry_restore_points (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    source_location_id TEXT NOT NULL,
    restore_location_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    snapshot_contract_version SMALLINT NOT NULL,
    account_version BIGINT NOT NULL,
    character_version BIGINT NOT NULL,
    progression_version BIGINT NOT NULL,
    inventory_version BIGINT NOT NULL,
    oath_bargain_version BIGINT NOT NULL,
    component_mask SMALLINT NOT NULL,
    composite_digest BYTEA NOT NULL,
    restore_state SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    consumed_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, restore_point_id),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id),
    FOREIGN KEY (namespace_id, account_id, character_id, lineage_id)
        REFERENCES character_instance_lineages(namespace_id, account_id, character_id, lineage_id)
        ON DELETE CASCADE,
    CONSTRAINT restore_point_id_exact CHECK (
        octet_length(restore_point_id) = 16
        AND restore_point_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT restore_source_bounded CHECK (length(source_location_id) BETWEEN 3 AND 96),
    CONSTRAINT restore_destination_hall CHECK (restore_location_id = 'hub.lantern_halls_01'),
    CONSTRAINT restore_content_revision_bounded CHECK (length(content_revision) BETWEEN 1 AND 128),
    CONSTRAINT restore_contract_v1 CHECK (snapshot_contract_version = 1),
    CONSTRAINT restore_versions_positive CHECK (
        account_version > 0
        AND character_version > 0
        AND progression_version > 0
        AND inventory_version > 0
        AND oath_bargain_version > 0
    ),
    CONSTRAINT restore_components_complete CHECK (component_mask = 7),
    CONSTRAINT restore_digest_exact CHECK (
        octet_length(composite_digest) = 32
        AND composite_digest <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT restore_state_known CHECK (restore_state BETWEEN 0 AND 4),
    CONSTRAINT restore_consumed_shape CHECK (
        (restore_state = 0 AND consumed_at IS NULL)
        OR (restore_state BETWEEN 1 AND 4 AND consumed_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX one_active_restore_point_per_character
    ON character_entry_restore_points (namespace_id, account_id, character_id)
    WHERE restore_state = 0;

ALTER TABLE character_world_locations
    ADD CONSTRAINT world_location_lineage_owned
        FOREIGN KEY (namespace_id, account_id, character_id, instance_lineage_id)
        REFERENCES character_instance_lineages(namespace_id, account_id, character_id, lineage_id)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT world_location_restore_owned
        FOREIGN KEY (namespace_id, account_id, character_id, entry_restore_point_id)
        REFERENCES character_entry_restore_points(namespace_id, account_id, character_id, restore_point_id)
        DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE character_world_transfer_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    payload_hash BYTEA NOT NULL,
    expected_character_version BIGINT NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    command_kind SMALLINT NOT NULL,
    transfer_id BYTEA,
    pre_character_version BIGINT NOT NULL,
    post_character_version BIGINT NOT NULL,
    result_code SMALLINT NOT NULL,
    result_payload BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT transfer_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT transfer_payload_hash_exact CHECK (
        octet_length(payload_hash) = 32
        AND payload_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT transfer_expected_version_positive CHECK (expected_character_version > 0),
    CONSTRAINT transfer_command_known CHECK (command_kind BETWEEN 0 AND 2),
    CONSTRAINT transfer_id_exact CHECK (
        transfer_id IS NULL OR (
            octet_length(transfer_id) = 16
            AND transfer_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    CONSTRAINT transfer_versions_positive CHECK (
        pre_character_version > 0 AND post_character_version > 0
    ),
    CONSTRAINT transfer_result_known CHECK (result_code BETWEEN 0 AND 20),
    CONSTRAINT transfer_acceptance_shape CHECK ((result_code = 0) = (transfer_id IS NOT NULL)),
    CONSTRAINT transfer_result_payload_bounded CHECK (
        octet_length(result_payload) BETWEEN 1 AND 65536
    )
);

CREATE TABLE character_danger_checkpoints (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    checkpoint_tick BIGINT NOT NULL,
    content_revision TEXT NOT NULL,
    component_mask SMALLINT NOT NULL,
    composite_digest BYTEA NOT NULL,
    character_version BIGINT NOT NULL,
    progression_version BIGINT NOT NULL,
    inventory_version BIGINT NOT NULL,
    oath_bargain_version BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id),
    FOREIGN KEY (namespace_id, account_id, character_id, lineage_id)
        REFERENCES character_instance_lineages(namespace_id, account_id, character_id, lineage_id)
        ON DELETE CASCADE,
    CONSTRAINT checkpoint_tick_nonnegative CHECK (checkpoint_tick >= 0),
    CONSTRAINT checkpoint_content_revision_bounded CHECK (length(content_revision) BETWEEN 1 AND 128),
    CONSTRAINT checkpoint_components_complete CHECK (component_mask = 7),
    CONSTRAINT checkpoint_digest_exact CHECK (
        octet_length(composite_digest) = 32
        AND composite_digest <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT checkpoint_versions_positive CHECK (
        character_version > 0
        AND progression_version > 0
        AND inventory_version > 0
        AND oath_bargain_version > 0
    )
);
