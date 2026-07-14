CREATE TABLE caldus_victory_exits (
    namespace_id TEXT NOT NULL,
    encounter_id BYTEA NOT NULL,
    instance_lineage_id BYTEA NOT NULL,
    attempt_ordinal INTEGER NOT NULL,
    exit_instance_id BYTEA NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    eligible_owner_count SMALLINT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, encounter_id),
    UNIQUE (namespace_id, exit_instance_id),
    FOREIGN KEY (namespace_id)
        REFERENCES gravebound_namespaces(namespace_id) ON DELETE CASCADE,
    CONSTRAINT caldus_victory_encounter_id_exact CHECK (
        octet_length(encounter_id) = 16
        AND encounter_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT caldus_victory_lineage_id_exact CHECK (
        octet_length(instance_lineage_id) = 16
        AND instance_lineage_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT caldus_victory_exit_id_exact CHECK (
        octet_length(exit_instance_id) = 16
        AND exit_instance_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT caldus_victory_request_hash_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT caldus_victory_attempt_positive CHECK (attempt_ordinal > 0),
    CONSTRAINT caldus_victory_owner_count_bounded CHECK (eligible_owner_count BETWEEN 1 AND 8)
);

CREATE TABLE caldus_victory_exit_owners (
    namespace_id TEXT NOT NULL,
    encounter_id BYTEA NOT NULL,
    party_slot SMALLINT NOT NULL,
    participant_entity_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    reward_request_id BYTEA NOT NULL,
    reward_result_hash BYTEA NOT NULL,
    progression_payload_hash BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, encounter_id, party_slot),
    UNIQUE (namespace_id, encounter_id, participant_entity_id),
    UNIQUE (namespace_id, encounter_id, reward_request_id),
    FOREIGN KEY (namespace_id, encounter_id)
        REFERENCES caldus_victory_exits(namespace_id, encounter_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, reward_request_id)
        REFERENCES reward_requests(namespace_id, reward_request_id),
    FOREIGN KEY (namespace_id, account_id, reward_request_id)
        REFERENCES character_xp_award_results(namespace_id, account_id, reward_event_id),
    CONSTRAINT caldus_victory_party_slot_bounded CHECK (party_slot BETWEEN 0 AND 7),
    CONSTRAINT caldus_victory_participant_entity_exact CHECK (
        octet_length(participant_entity_id) = 8
        AND participant_entity_id <> decode(repeat('00', 8), 'hex')
    ),
    CONSTRAINT caldus_victory_account_id_exact CHECK (
        octet_length(account_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT caldus_victory_character_id_exact CHECK (
        octet_length(character_id) = 16
        AND character_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT caldus_victory_reward_request_id_exact CHECK (
        octet_length(reward_request_id) = 16
        AND reward_request_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT caldus_victory_terminal_hashes_exact CHECK (
        octet_length(reward_result_hash) = 32
        AND reward_result_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(progression_payload_hash) = 32
        AND progression_payload_hash <> decode(repeat('00', 32), 'hex')
    )
);

CREATE INDEX caldus_victory_exits_by_lineage
    ON caldus_victory_exits (namespace_id, instance_lineage_id, attempt_ordinal);
