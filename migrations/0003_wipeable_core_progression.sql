DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM characters
        WHERE namespace_id = 'test.core' AND level <> 1
    ) THEN
        RAISE EXCEPTION '0003 progression backfill accepts only the approved level-1 wipeable roster';
    END IF;
END $$;

CREATE TABLE character_progression (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    total_xp INTEGER NOT NULL,
    level SMALLINT NOT NULL,
    current_health INTEGER NOT NULL,
    progression_version BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT progression_total_xp_core CHECK (total_xp BETWEEN 0 AND 2700),
    CONSTRAINT progression_level_core CHECK (level BETWEEN 1 AND 10),
    CONSTRAINT progression_level_xp_shape CHECK (
        (level = 1 AND total_xp BETWEEN 0 AND 99)
        OR (level = 2 AND total_xp BETWEEN 100 AND 249)
        OR (level = 3 AND total_xp BETWEEN 250 AND 449)
        OR (level = 4 AND total_xp BETWEEN 450 AND 699)
        OR (level = 5 AND total_xp BETWEEN 700 AND 999)
        OR (level = 6 AND total_xp BETWEEN 1000 AND 1349)
        OR (level = 7 AND total_xp BETWEEN 1350 AND 1749)
        OR (level = 8 AND total_xp BETWEEN 1750 AND 2199)
        OR (level = 9 AND total_xp BETWEEN 2200 AND 2699)
        OR (level = 10 AND total_xp = 2700)
    ),
    CONSTRAINT progression_current_health_living CHECK (current_health >= 1),
    CONSTRAINT progression_version_positive CHECK (progression_version > 0)
);

INSERT INTO character_progression (
    namespace_id,
    account_id,
    character_id,
    total_xp,
    level,
    current_health,
    progression_version
)
SELECT namespace_id, account_id, character_id, 0, 1, 120, 1
FROM characters;

CREATE TABLE character_xp_award_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    reward_event_id BYTEA NOT NULL,
    payload_hash BYTEA NOT NULL,
    source_content_id TEXT NOT NULL,
    xp_profile_id TEXT NOT NULL,
    progression_content_revision TEXT NOT NULL,
    eligibility_kind SMALLINT NOT NULL,
    eligible BOOLEAN NOT NULL,
    normal_delta_x_milli_tiles INTEGER,
    normal_delta_y_milli_tiles INTEGER,
    normal_window_ticks INTEGER,
    normal_actual_damage BIGINT,
    normal_effective_support BOOLEAN,
    encounter_active_ticks BIGINT,
    encounter_present_ticks BIGINT,
    encounter_longest_inactivity_ticks BIGINT,
    encounter_reference_health BIGINT,
    encounter_direct_damage BIGINT,
    encounter_effective_healing BIGINT,
    encounter_damage_prevented BIGINT,
    encounter_objective_credits SMALLINT,
    encounter_life_state SMALLINT,
    encounter_recall_state SMALLINT,
    encounter_trust_state SMALLINT,
    first_clear_awarded BOOLEAN NOT NULL,
    base_xp INTEGER NOT NULL,
    bonus_xp INTEGER NOT NULL,
    requested_xp INTEGER NOT NULL,
    applied_xp INTEGER NOT NULL,
    discarded_xp INTEGER NOT NULL,
    pre_total_xp INTEGER NOT NULL,
    post_total_xp INTEGER NOT NULL,
    pre_level SMALLINT NOT NULL,
    post_level SMALLINT NOT NULL,
    pre_progression_version BIGINT NOT NULL,
    post_progression_version BIGINT NOT NULL,
    result_code SMALLINT NOT NULL,
    result_payload BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, reward_event_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_progression(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT xp_reward_event_id_exact CHECK (
        octet_length(reward_event_id) = 16
        AND reward_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT xp_payload_hash_exact CHECK (
        octet_length(payload_hash) = 32
        AND payload_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT xp_source_content_id_bounded CHECK (length(source_content_id) BETWEEN 3 AND 96),
    CONSTRAINT xp_profile_id_bounded CHECK (length(xp_profile_id) BETWEEN 3 AND 96),
    CONSTRAINT xp_content_revision_exact CHECK (
        length(progression_content_revision) = 64
        AND progression_content_revision ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT xp_eligibility_kind_known CHECK (eligibility_kind BETWEEN 0 AND 1),
    CONSTRAINT xp_normal_evidence_shape CHECK (
        (eligibility_kind = 0
            AND normal_delta_x_milli_tiles IS NOT NULL
            AND normal_delta_y_milli_tiles IS NOT NULL
            AND normal_window_ticks IS NOT NULL
            AND normal_window_ticks BETWEEN 0 AND 300
            AND normal_actual_damage IS NOT NULL
            AND normal_actual_damage >= 0
            AND normal_effective_support IS NOT NULL
            AND encounter_active_ticks IS NULL
            AND encounter_present_ticks IS NULL
            AND encounter_longest_inactivity_ticks IS NULL
            AND encounter_reference_health IS NULL
            AND encounter_direct_damage IS NULL
            AND encounter_effective_healing IS NULL
            AND encounter_damage_prevented IS NULL
            AND encounter_objective_credits IS NULL
            AND encounter_life_state IS NULL
            AND encounter_recall_state IS NULL
            AND encounter_trust_state IS NULL)
        OR
        (eligibility_kind = 1
            AND normal_delta_x_milli_tiles IS NULL
            AND normal_delta_y_milli_tiles IS NULL
            AND normal_window_ticks IS NULL
            AND normal_actual_damage IS NULL
            AND normal_effective_support IS NULL
            AND encounter_active_ticks IS NOT NULL
            AND encounter_active_ticks > 0
            AND encounter_present_ticks IS NOT NULL
            AND encounter_present_ticks BETWEEN 0 AND encounter_active_ticks
            AND encounter_longest_inactivity_ticks IS NOT NULL
            AND encounter_longest_inactivity_ticks >= 0
            AND encounter_reference_health IS NOT NULL
            AND encounter_reference_health > 0
            AND encounter_direct_damage IS NOT NULL
            AND encounter_direct_damage >= 0
            AND encounter_effective_healing IS NOT NULL
            AND encounter_effective_healing >= 0
            AND encounter_damage_prevented IS NOT NULL
            AND encounter_damage_prevented >= 0
            AND encounter_objective_credits IS NOT NULL
            AND encounter_objective_credits BETWEEN 0 AND 2
            AND encounter_life_state IS NOT NULL
            AND encounter_life_state BETWEEN 0 AND 1
            AND encounter_recall_state IS NOT NULL
            AND encounter_recall_state BETWEEN 0 AND 1
            AND encounter_trust_state IS NOT NULL
            AND encounter_trust_state BETWEEN 0 AND 2)
    ),
    CONSTRAINT xp_amounts_nonnegative CHECK (
        base_xp >= 0 AND bonus_xp >= 0 AND requested_xp >= 0
        AND applied_xp >= 0 AND discarded_xp >= 0
        AND requested_xp = base_xp + bonus_xp
        AND requested_xp = applied_xp + discarded_xp
    ),
    CONSTRAINT xp_totals_core CHECK (
        pre_total_xp BETWEEN 0 AND 2700
        AND post_total_xp BETWEEN pre_total_xp AND 2700
        AND post_total_xp = pre_total_xp + applied_xp
    ),
    CONSTRAINT xp_levels_core CHECK (
        pre_level BETWEEN 1 AND 10
        AND post_level BETWEEN pre_level AND 10
    ),
    CONSTRAINT xp_versions_positive CHECK (
        pre_progression_version > 0 AND post_progression_version > 0
        AND ((applied_xp = 0 AND post_progression_version = pre_progression_version)
            OR (applied_xp > 0 AND post_progression_version = pre_progression_version + 1))
    ),
    CONSTRAINT xp_result_code_known CHECK (result_code BETWEEN 0 AND 12),
    CONSTRAINT xp_result_payload_bounded CHECK (octet_length(result_payload) BETWEEN 1 AND 65536)
);

CREATE INDEX xp_awards_by_character
    ON character_xp_award_results (namespace_id, account_id, character_id, committed_at);

CREATE TABLE account_boss_first_clears (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    boss_id TEXT NOT NULL,
    reward_event_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, boss_id),
    FOREIGN KEY (namespace_id, account_id, reward_event_id)
        REFERENCES character_xp_award_results(namespace_id, account_id, reward_event_id)
        DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_progression(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT first_clear_boss_id_bounded CHECK (length(boss_id) BETWEEN 3 AND 96),
    CONSTRAINT first_clear_reward_event_id_exact CHECK (octet_length(reward_event_id) = 16),
    CONSTRAINT first_clear_character_id_exact CHECK (octet_length(character_id) = 16)
);

CREATE TABLE entry_restore_progression_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    level SMALLINT NOT NULL,
    total_xp INTEGER NOT NULL,
    current_health INTEGER NOT NULL,
    progression_version BIGINT NOT NULL,
    PRIMARY KEY (namespace_id, restore_point_id),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(namespace_id, account_id, character_id, restore_point_id)
        ON DELETE CASCADE,
    CONSTRAINT restore_progression_level_core CHECK (level BETWEEN 1 AND 10),
    CONSTRAINT restore_progression_xp_core CHECK (total_xp BETWEEN 0 AND 2700),
    CONSTRAINT restore_progression_current_health_living CHECK (current_health >= 1),
    CONSTRAINT restore_progression_version_positive CHECK (progression_version > 0)
);
