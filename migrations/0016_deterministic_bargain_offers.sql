ALTER TABLE characters
    DROP CONSTRAINT character_initial_oath_level,
    ADD CONSTRAINT character_oath_level_core CHECK (
        oath_id IS NULL OR level BETWEEN 10 AND 20
    );

CREATE TABLE character_oath_bargain_state (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    earned_bargain_slots SMALLINT NOT NULL DEFAULT 0,
    oath_bargain_version BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT oath_bargain_state_account_id_exact CHECK (octet_length(account_id) = 16),
    CONSTRAINT oath_bargain_state_character_id_exact CHECK (octet_length(character_id) = 16),
    CONSTRAINT oath_bargain_state_earned_slots_bounded CHECK (
        earned_bargain_slots BETWEEN 0 AND 3
    ),
    CONSTRAINT oath_bargain_state_version_positive CHECK (oath_bargain_version > 0)
);

INSERT INTO character_oath_bargain_state (namespace_id, account_id, character_id)
SELECT namespace_id, account_id, character_id FROM characters;

CREATE TABLE bargain_offers (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    offer_id BYTEA NOT NULL,
    source_reward_event_id BYTEA NOT NULL,
    content_version TEXT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    offer_state SMALLINT NOT NULL,
    selected_bargain_id TEXT,
    created_oath_bargain_version BIGINT NOT NULL,
    resolved_oath_bargain_version BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    resolved_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, account_id, offer_id),
    UNIQUE (namespace_id, account_id, character_id, offer_id),
    UNIQUE (namespace_id, account_id, source_reward_event_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_oath_bargain_state(namespace_id, account_id, character_id)
        ON DELETE CASCADE,
    CONSTRAINT bargain_offer_account_id_exact CHECK (octet_length(account_id) = 16),
    CONSTRAINT bargain_offer_character_id_exact CHECK (octet_length(character_id) = 16),
    CONSTRAINT bargain_offer_id_exact CHECK (
        octet_length(offer_id) = 16
        AND offer_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT bargain_offer_source_id_exact CHECK (
        octet_length(source_reward_event_id) = 16
        AND source_reward_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT bargain_offer_content_version_bounded CHECK (
        length(content_version) BETWEEN 1 AND 96
    ),
    CONSTRAINT bargain_offer_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT bargain_offer_state_known CHECK (offer_state BETWEEN 0 AND 3),
    CONSTRAINT bargain_offer_resolution_shape CHECK (
        (offer_state = 0 AND selected_bargain_id IS NULL
            AND resolved_oath_bargain_version IS NULL AND resolved_at IS NULL)
        OR (offer_state = 1 AND selected_bargain_id IS NOT NULL
            AND selected_bargain_id IN (
                'bargain.bell_debt',
                'bargain.cinder_hunger',
                'bargain.lantern_ash'
            ) AND resolved_oath_bargain_version IS NOT NULL
            AND resolved_oath_bargain_version = created_oath_bargain_version + 1
            AND resolved_at IS NOT NULL)
        OR (offer_state IN (2, 3) AND selected_bargain_id IS NULL
            AND resolved_oath_bargain_version IS NOT NULL
            AND resolved_oath_bargain_version = created_oath_bargain_version
            AND resolved_at IS NOT NULL)
    ),
    CONSTRAINT bargain_offer_versions_positive CHECK (
        created_oath_bargain_version > 0
        AND (resolved_oath_bargain_version IS NULL OR resolved_oath_bargain_version > 0)
    )
);

CREATE TABLE bargain_offer_candidates (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    offer_id BYTEA NOT NULL,
    candidate_ordinal SMALLINT NOT NULL,
    bargain_id TEXT NOT NULL,
    score BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, account_id, offer_id, candidate_ordinal),
    UNIQUE (namespace_id, account_id, offer_id, bargain_id),
    FOREIGN KEY (namespace_id, account_id, offer_id)
        REFERENCES bargain_offers(namespace_id, account_id, offer_id) ON DELETE CASCADE,
    CONSTRAINT bargain_candidate_ordinal_bounded CHECK (candidate_ordinal BETWEEN 0 AND 2),
    CONSTRAINT bargain_candidate_id_core CHECK (bargain_id IN (
        'bargain.bell_debt',
        'bargain.cinder_hunger',
        'bargain.lantern_ash'
    )),
    CONSTRAINT bargain_candidate_score_exact CHECK (
        octet_length(score) = 32
        AND score <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE character_active_bargains (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    bargain_id TEXT NOT NULL,
    acquisition_ordinal SMALLINT NOT NULL,
    acquired_by_offer_id BYTEA NOT NULL,
    acquired_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id, bargain_id),
    UNIQUE (namespace_id, account_id, character_id, acquisition_ordinal),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_oath_bargain_state(namespace_id, account_id, character_id)
        ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, acquired_by_offer_id)
        REFERENCES bargain_offers(namespace_id, account_id, character_id, offer_id),
    CONSTRAINT active_bargain_id_core CHECK (bargain_id IN (
        'bargain.bell_debt',
        'bargain.cinder_hunger',
        'bargain.lantern_ash'
    )),
    CONSTRAINT active_bargain_ordinal_bounded CHECK (acquisition_ordinal BETWEEN 1 AND 3)
);

CREATE TABLE bargain_milestone_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    source_reward_event_id BYTEA NOT NULL,
    payload_hash BYTEA NOT NULL,
    result_code SMALLINT NOT NULL,
    pre_oath_bargain_version BIGINT NOT NULL,
    post_oath_bargain_version BIGINT NOT NULL,
    offer_id BYTEA,
    ash_mutation_id BYTEA,
    result_payload BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, source_reward_event_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_oath_bargain_state(namespace_id, account_id, character_id)
        ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, offer_id)
        REFERENCES bargain_offers(namespace_id, account_id, offer_id),
    CONSTRAINT bargain_milestone_source_id_exact CHECK (
        octet_length(source_reward_event_id) = 16
        AND source_reward_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT bargain_milestone_payload_hash_exact CHECK (
        octet_length(payload_hash) = 32
        AND payload_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT bargain_milestone_result_code_known CHECK (result_code BETWEEN 0 AND 2),
    CONSTRAINT bargain_milestone_version_shape CHECK (
        pre_oath_bargain_version > 0
        AND (
            (result_code = 0 AND post_oath_bargain_version = pre_oath_bargain_version + 1
                AND offer_id IS NOT NULL AND ash_mutation_id IS NULL)
            OR (result_code = 1
                AND post_oath_bargain_version = pre_oath_bargain_version + 1
                AND offer_id IS NOT NULL AND ash_mutation_id IS NOT NULL
                AND octet_length(ash_mutation_id) = 16
                AND ash_mutation_id <> decode(repeat('00', 16), 'hex'))
            OR (result_code = 2
                AND post_oath_bargain_version = pre_oath_bargain_version
                AND offer_id IS NULL AND ash_mutation_id IS NOT NULL
                AND octet_length(ash_mutation_id) = 16
                AND ash_mutation_id <> decode(repeat('00', 16), 'hex'))
        )
    ),
    CONSTRAINT bargain_milestone_result_payload_bounded CHECK (
        octet_length(result_payload) BETWEEN 1 AND 65536
    )
);

CREATE TABLE bargain_decision_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    offer_id BYTEA NOT NULL,
    payload_hash BYTEA NOT NULL,
    decision_kind SMALLINT NOT NULL,
    bargain_id TEXT,
    pre_oath_bargain_version BIGINT NOT NULL,
    post_oath_bargain_version BIGINT NOT NULL,
    result_code SMALLINT NOT NULL,
    result_payload BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_oath_bargain_state(namespace_id, account_id, character_id)
        ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, offer_id)
        REFERENCES bargain_offers(namespace_id, account_id, character_id, offer_id),
    CONSTRAINT bargain_decision_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT bargain_decision_payload_hash_exact CHECK (
        octet_length(payload_hash) = 32
        AND payload_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT bargain_decision_kind_known CHECK (decision_kind IN (0, 1)),
    CONSTRAINT bargain_decision_payload_shape CHECK (
        (decision_kind = 0 AND bargain_id IS NOT NULL AND bargain_id IN (
            'bargain.bell_debt',
            'bargain.cinder_hunger',
            'bargain.lantern_ash'
        )) OR (decision_kind = 1 AND bargain_id IS NULL)
    ),
    CONSTRAINT bargain_decision_result_code_known CHECK (result_code BETWEEN 0 AND 15),
    CONSTRAINT bargain_decision_version_shape CHECK (
        pre_oath_bargain_version > 0
        AND (
            (result_code = 0 AND decision_kind = 0
                AND post_oath_bargain_version = pre_oath_bargain_version + 1)
            OR (result_code <> 0 AND post_oath_bargain_version = pre_oath_bargain_version)
        )
    ),
    CONSTRAINT bargain_decision_result_payload_bounded CHECK (
        octet_length(result_payload) BETWEEN 1 AND 65536
    )
);

ALTER TABLE character_life_outbox
    DROP CONSTRAINT life_outbox_event_type_known,
    ADD CONSTRAINT life_outbox_event_type_known CHECK (
        event_type IN ('oath_selected', 'bargain_selected')
    );

DROP INDEX one_oath_selected_event_per_character;
CREATE UNIQUE INDEX one_oath_selected_event_per_character
    ON character_life_outbox (namespace_id, account_id, character_id)
    WHERE event_type = 'oath_selected';

CREATE INDEX bargain_life_events_by_character
    ON character_life_outbox (namespace_id, account_id, character_id, aggregate_version)
    WHERE event_type = 'bargain_selected';

CREATE INDEX open_bargain_offers_by_character
    ON bargain_offers (namespace_id, account_id, character_id, created_at, offer_id)
    WHERE offer_state = 0;
