DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM characters
        WHERE oath_id IS NOT NULL
          AND oath_id NOT IN ('oath.arbalist.long_vigil', 'oath.arbalist.nailkeeper')
    ) THEN
        RAISE EXCEPTION '0006 rejects unknown pre-existing Oath IDs';
    END IF;
END $$;

ALTER TABLE characters
    DROP CONSTRAINT character_oath_id_bounded,
    ADD CONSTRAINT character_oath_id_core CHECK (
        oath_id IS NULL
        OR oath_id IN ('oath.arbalist.long_vigil', 'oath.arbalist.nailkeeper')
    ),
    ADD CONSTRAINT character_initial_oath_level CHECK (oath_id IS NULL OR level = 10);

CREATE TABLE character_oath_mutation_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    payload_hash BYTEA NOT NULL,
    oath_id TEXT NOT NULL,
    pre_character_state_version BIGINT NOT NULL,
    post_character_state_version BIGINT NOT NULL,
    result_code SMALLINT NOT NULL,
    result_payload BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT oath_result_account_id_exact CHECK (octet_length(account_id) = 16),
    CONSTRAINT oath_result_character_id_exact CHECK (octet_length(character_id) = 16),
    CONSTRAINT oath_result_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT oath_result_payload_hash_exact CHECK (
        octet_length(payload_hash) = 32
        AND payload_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT oath_result_oath_id_core CHECK (
        oath_id IN ('oath.arbalist.long_vigil', 'oath.arbalist.nailkeeper')
    ),
    CONSTRAINT oath_result_versions_shape CHECK (
        pre_character_state_version > 0
        AND (
            (result_code = 1 AND post_character_state_version = pre_character_state_version + 1)
            OR (result_code <> 1 AND post_character_state_version = pre_character_state_version)
        )
    ),
    CONSTRAINT oath_result_code_known CHECK (result_code BETWEEN 0 AND 18),
    CONSTRAINT oath_result_payload_bounded CHECK (octet_length(result_payload) BETWEEN 1 AND 65536)
);

CREATE TABLE character_life_outbox (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    event_id BYTEA NOT NULL,
    event_type TEXT NOT NULL,
    aggregate_version BIGINT NOT NULL,
    event_payload BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT life_outbox_account_id_exact CHECK (octet_length(account_id) = 16),
    CONSTRAINT life_outbox_character_id_exact CHECK (octet_length(character_id) = 16),
    CONSTRAINT life_outbox_event_id_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT life_outbox_event_type_known CHECK (event_type = 'oath_selected'),
    CONSTRAINT life_outbox_aggregate_version_positive CHECK (aggregate_version > 0),
    CONSTRAINT life_outbox_payload_bounded CHECK (octet_length(event_payload) BETWEEN 1 AND 65536),
    CONSTRAINT life_outbox_publish_order CHECK (published_at IS NULL OR published_at >= created_at)
);

CREATE UNIQUE INDEX one_oath_selected_event_per_character
    ON character_life_outbox (namespace_id, account_id, character_id, event_type);

CREATE INDEX unpublished_character_life_events
    ON character_life_outbox (namespace_id, created_at, event_id)
    WHERE published_at IS NULL;
