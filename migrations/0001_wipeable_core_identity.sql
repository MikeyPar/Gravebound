CREATE TABLE gravebound_namespaces (
    namespace_id TEXT PRIMARY KEY,
    wipeable BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    CONSTRAINT namespace_id_not_blank CHECK (length(namespace_id) BETWEEN 1 AND 64)
);

INSERT INTO gravebound_namespaces (namespace_id, wipeable)
VALUES ('test.core', TRUE);

CREATE TABLE accounts (
    namespace_id TEXT NOT NULL REFERENCES gravebound_namespaces(namespace_id),
    account_id BYTEA NOT NULL,
    state_version BIGINT NOT NULL,
    slot_capacity SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id),
    CONSTRAINT account_id_exact_length CHECK (octet_length(account_id) = 16),
    CONSTRAINT account_id_nonzero CHECK (account_id <> decode(repeat('00', 16), 'hex')),
    CONSTRAINT account_state_version_positive CHECK (state_version > 0),
    CONSTRAINT account_slot_capacity_core CHECK (slot_capacity = 2)
);

CREATE TABLE characters (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    roster_ordinal SMALLINT NOT NULL,
    class_id TEXT NOT NULL,
    level INTEGER NOT NULL,
    oath_id TEXT,
    life_state SMALLINT NOT NULL,
    security_state SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, character_id),
    UNIQUE (namespace_id, account_id, character_id),
    UNIQUE (namespace_id, account_id, roster_ordinal),
    FOREIGN KEY (namespace_id, account_id)
        REFERENCES accounts(namespace_id, account_id) ON DELETE CASCADE,
    CONSTRAINT character_account_id_exact_length CHECK (octet_length(account_id) = 16),
    CONSTRAINT character_id_exact_length CHECK (octet_length(character_id) = 16),
    CONSTRAINT character_id_nonzero CHECK (character_id <> decode(repeat('00', 16), 'hex')),
    CONSTRAINT character_roster_ordinal_core CHECK (roster_ordinal BETWEEN 1 AND 2),
    CONSTRAINT character_class_id_core CHECK (class_id = 'class.grave_arbalist'),
    CONSTRAINT character_level_positive CHECK (level > 0),
    CONSTRAINT character_oath_id_bounded CHECK (oath_id IS NULL OR length(oath_id) BETWEEN 1 AND 96),
    CONSTRAINT character_life_state_core CHECK (life_state = 0),
    CONSTRAINT character_security_state_core CHECK (security_state = 0)
);

ALTER TABLE accounts ADD COLUMN selected_character_id BYTEA;
ALTER TABLE accounts ADD CONSTRAINT selected_character_id_exact_length
    CHECK (selected_character_id IS NULL OR octet_length(selected_character_id) = 16);
ALTER TABLE accounts ADD CONSTRAINT selected_character_owned
    FOREIGN KEY (namespace_id, account_id, selected_character_id)
    REFERENCES characters(namespace_id, account_id, character_id)
    DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE account_mutation_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    payload_hash BYTEA NOT NULL,
    result_payload BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id)
        REFERENCES accounts(namespace_id, account_id) ON DELETE CASCADE,
    CONSTRAINT mutation_account_id_exact_length CHECK (octet_length(account_id) = 16),
    CONSTRAINT mutation_id_exact_length CHECK (octet_length(mutation_id) = 16),
    CONSTRAINT mutation_id_nonzero CHECK (mutation_id <> decode(repeat('00', 16), 'hex')),
    CONSTRAINT mutation_payload_hash_exact_length CHECK (octet_length(payload_hash) = 32),
    CONSTRAINT mutation_payload_hash_nonzero CHECK (payload_hash <> decode(repeat('00', 32), 'hex')),
    CONSTRAINT mutation_result_payload_bounded CHECK (octet_length(result_payload) BETWEEN 1 AND 65536)
);

CREATE INDEX characters_by_account
    ON characters (namespace_id, account_id, roster_ordinal);
