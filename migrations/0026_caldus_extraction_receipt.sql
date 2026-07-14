-- GB-M03-03E wipeable extraction receipt seam. GB-M03-08 owns item stabilization.
ALTER TABLE caldus_victory_exits
    ADD CONSTRAINT caldus_victory_exit_pair_unique
    UNIQUE (namespace_id, encounter_id, exit_instance_id);

ALTER TABLE character_world_transfer_results
    DROP CONSTRAINT transfer_command_known,
    ADD CONSTRAINT transfer_command_known CHECK (command_kind BETWEEN 0 AND 3);

CREATE TABLE character_extraction_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    extraction_request_id BYTEA NOT NULL,
    extraction_receipt_id BYTEA,
    request_payload_hash BYTEA NOT NULL,
    receipt_payload_hash BYTEA,
    encounter_id BYTEA NOT NULL,
    instance_lineage_id BYTEA NOT NULL,
    entry_restore_point_id BYTEA NOT NULL,
    exit_instance_id BYTEA NOT NULL,
    exit_content_id TEXT NOT NULL,
    attempt_ordinal INTEGER NOT NULL,
    party_slot SMALLINT NOT NULL,
    participant_entity_id BYTEA NOT NULL,
    expected_character_version BIGINT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    extraction_state SMALLINT NOT NULL,
    authority_kind SMALLINT,
    destination_content_id TEXT,
    safe_arrival_kind SMALLINT,
    requested_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    committed_at TIMESTAMPTZ,
    transfer_mutation_id BYTEA,
    post_character_version BIGINT,
    transferred_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, extraction_request_id),
    UNIQUE (namespace_id, extraction_receipt_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, encounter_id, exit_instance_id)
        REFERENCES caldus_victory_exits(namespace_id, encounter_id, exit_instance_id),
    FOREIGN KEY (namespace_id, account_id, character_id, instance_lineage_id)
        REFERENCES character_instance_lineages(namespace_id, account_id, character_id, lineage_id),
    FOREIGN KEY (namespace_id, account_id, character_id, entry_restore_point_id)
        REFERENCES character_entry_restore_points(namespace_id, account_id, character_id, restore_point_id),
    CONSTRAINT extraction_ids_exact CHECK (
        octet_length(extraction_request_id) = 16
        AND extraction_request_id <> decode(repeat('00', 16), 'hex')
        AND (extraction_receipt_id IS NULL OR (
            octet_length(extraction_receipt_id) = 16
            AND extraction_receipt_id <> decode(repeat('00', 16), 'hex')
        ))
        AND octet_length(encounter_id) = 16
        AND octet_length(instance_lineage_id) = 16
        AND octet_length(entry_restore_point_id) = 16
        AND octet_length(exit_instance_id) = 16
        AND octet_length(participant_entity_id) = 8
    ),
    CONSTRAINT extraction_hashes_exact CHECK (
        octet_length(request_payload_hash) = 32
        AND request_payload_hash <> decode(repeat('00', 32), 'hex')
        AND (receipt_payload_hash IS NULL OR (
            octet_length(receipt_payload_hash) = 32
            AND receipt_payload_hash <> decode(repeat('00', 32), 'hex')
        ))
        AND records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT extraction_binding_exact CHECK (
        exit_content_id = 'portal.exit.dungeon.bell_sepulcher'
        AND attempt_ordinal > 0
        AND party_slot BETWEEN 0 AND 7
        AND participant_entity_id <> decode(repeat('00', 8), 'hex')
        AND expected_character_version > 0
    ),
    CONSTRAINT extraction_state_known CHECK (extraction_state IN (0, 1)),
    CONSTRAINT extraction_state_shape CHECK (
        (extraction_state = 0
            AND extraction_receipt_id IS NULL
            AND receipt_payload_hash IS NULL
            AND authority_kind IS NULL
            AND destination_content_id IS NULL
            AND safe_arrival_kind IS NULL
            AND committed_at IS NULL
            AND transfer_mutation_id IS NULL
            AND post_character_version IS NULL
            AND transferred_at IS NULL)
        OR
        (extraction_state = 1
            AND extraction_receipt_id IS NOT NULL
            AND receipt_payload_hash IS NOT NULL
            AND authority_kind = 0
            AND destination_content_id = 'hub.lantern_halls_01'
            AND safe_arrival_kind = 0
            AND committed_at IS NOT NULL
            AND ((transfer_mutation_id IS NULL
                    AND post_character_version IS NULL
                    AND transferred_at IS NULL)
                OR (octet_length(transfer_mutation_id) = 16
                    AND transfer_mutation_id <> decode(repeat('00', 16), 'hex')
                    AND post_character_version > expected_character_version
                    AND transferred_at IS NOT NULL)))
    )
);

CREATE UNIQUE INDEX one_caldus_extraction_per_character_attempt
    ON character_extraction_results
        (namespace_id, account_id, character_id, instance_lineage_id, attempt_ordinal);

COMMENT ON TABLE character_extraction_results IS
    'GB-M03-03E receipt/transfer seam only; contains no DTH-011 inventory conversion state';
