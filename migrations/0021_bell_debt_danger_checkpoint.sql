DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM character_danger_checkpoints) THEN
        RAISE EXCEPTION '0021 requires dormant danger checkpoint rows';
    END IF;
END
$$;

ALTER TABLE character_danger_checkpoints
    DROP CONSTRAINT checkpoint_components_complete,
    ADD COLUMN checkpoint_schema_version SMALLINT NOT NULL,
    ADD COLUMN checkpoint_payload BYTEA NOT NULL,
    ADD COLUMN checkpoint_payload_digest BYTEA NOT NULL,
    ADD CONSTRAINT checkpoint_components_complete CHECK (component_mask = 15),
    ADD CONSTRAINT checkpoint_schema_v1 CHECK (checkpoint_schema_version = 1),
    ADD CONSTRAINT checkpoint_payload_bounded CHECK (
        octet_length(checkpoint_payload) BETWEEN 1 AND 4096
    ),
    ADD CONSTRAINT checkpoint_payload_digest_exact CHECK (
        octet_length(checkpoint_payload_digest) = 32
        AND checkpoint_payload_digest <> decode(repeat('00', 32), 'hex')
    );
