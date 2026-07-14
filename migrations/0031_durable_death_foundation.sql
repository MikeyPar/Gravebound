-- GB-M03-02D / GB-M03-06A / GB-M03-13 durable death foundation.
--
-- This is a forward-only wipeable-Core migration. Returning to schema 30 is safe only after
-- proving there are no dead characters, no death/memorial/Echo rows, no entry inventory
-- components, and no item or ledger row whose destruction reason is `permadeath`.

ALTER TABLE characters
    DROP CONSTRAINT character_life_state_core,
    ADD CONSTRAINT character_life_state_core CHECK (life_state IN (0, 1));

ALTER TABLE character_progression
    DROP CONSTRAINT progression_current_health_living,
    ADD CONSTRAINT progression_current_health_terminal CHECK (current_health >= 0);

ALTER TABLE item_instances
    DROP CONSTRAINT item_location_shape,
    ADD CONSTRAINT item_location_shape CHECK (
        (location_kind = 0 AND character_id IS NOT NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 3
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state IN (0, 1) AND item_kind = 0)
        OR (location_kind = 1 AND character_id IS NOT NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 1
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state IN (0, 1) AND item_kind = 1)
        OR (location_kind = 2 AND character_id IS NOT NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 7
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state = 2)
        OR (location_kind = 3 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NOT NULL AND octet_length(instance_id) = 16
            AND instance_id <> decode(repeat('00', 16), 'hex')
            AND pickup_id IS NOT NULL AND octet_length(pickup_id) = 16
            AND pickup_id <> decode(repeat('00', 16), 'hex')
            AND expires_at_tick IS NOT NULL AND expires_at_tick > 0
            AND destruction_reason IS NULL AND security_state = 2)
        OR (location_kind = 4 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IN ('ground_expired', 'permadeath')
            AND security_state = 3)
        OR (location_kind = 5 AND character_id IS NOT NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 7
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state = 0)
        OR (location_kind = 6 AND character_id IS NULL
            AND slot_index IS NOT NULL AND slot_index BETWEEN 0 AND 159
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NULL AND security_state = 0)
    );

ALTER TABLE item_ledger_events
    DROP CONSTRAINT ledger_event_kind_known,
    DROP CONSTRAINT ledger_source_kind_known,
    DROP CONSTRAINT ledger_creation_shape,
    ADD CONSTRAINT ledger_event_kind_known CHECK (event_kind BETWEEN 0 AND 2),
    ADD CONSTRAINT ledger_source_kind_known CHECK (source_kind BETWEEN 0 AND 3),
    ADD CONSTRAINT ledger_creation_shape CHECK (
        (event_kind = 0 AND pre_item_version = 0
            AND pre_security_state IS NULL AND pre_location_kind IS NULL AND reason IS NULL)
        OR (event_kind = 1 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL AND reason IS NULL)
        OR (event_kind = 2 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND post_security_state = 3 AND post_location_kind = 4
            AND reason IN ('ground_expired', 'permadeath'))
    );

-- The entry writer snapshots exact Equipped/Belt units and commits Safe -> AtRiskEquipped
-- without changing their location before a dangerous lineage becomes observable.
CREATE TABLE entry_restore_inventory_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    risk_item_count SMALLINT NOT NULL,
    safe_placement_count SMALLINT NOT NULL,
    component_digest BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, restore_point_id),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT restore_inventory_versions_exact CHECK (
        pre_inventory_version > 0
        AND post_inventory_version = pre_inventory_version
            + CASE WHEN risk_item_count + safe_placement_count > 0 THEN 1 ELSE 0 END
    ),
    CONSTRAINT restore_inventory_counts_bounded CHECK (
        risk_item_count BETWEEN 0 AND 16
        AND safe_placement_count BETWEEN 0 AND 8
    ),
    CONSTRAINT restore_inventory_digest_exact CHECK (
        octet_length(component_digest) = 32
        AND component_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE entry_restore_inventory_items_v1 (
    namespace_id TEXT NOT NULL,
    restore_point_id BYTEA NOT NULL,
    item_ordinal SMALLINT NOT NULL,
    item_uid BYTEA NOT NULL,
    location_kind SMALLINT NOT NULL,
    slot_index SMALLINT NOT NULL,
    pre_item_version BIGINT NOT NULL,
    post_item_version BIGINT NOT NULL,
    PRIMARY KEY (namespace_id, restore_point_id, item_ordinal),
    UNIQUE (namespace_id, restore_point_id, item_uid),
    FOREIGN KEY (namespace_id, restore_point_id)
        REFERENCES entry_restore_inventory_v1(namespace_id, restore_point_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid) ON DELETE RESTRICT,
    CONSTRAINT restore_inventory_item_ordinal_bounded CHECK (item_ordinal BETWEEN 0 AND 15),
    CONSTRAINT restore_inventory_item_location_shape CHECK (
        (location_kind = 0 AND slot_index BETWEEN 0 AND 3)
        OR (location_kind = 1 AND slot_index BETWEEN 0 AND 1)
    ),
    CONSTRAINT restore_inventory_item_versions_exact CHECK (
        pre_item_version > 0 AND post_item_version = pre_item_version + 1
    )
);

CREATE TABLE character_life_metrics (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    lifetime_ticks BIGINT NOT NULL,
    permadeath_combat_ticks BIGINT NOT NULL,
    life_metrics_version BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT life_metrics_ticks_nonnegative CHECK (
        lifetime_ticks >= 0 AND permadeath_combat_ticks >= 0
    ),
    CONSTRAINT life_metrics_version_positive CHECK (life_metrics_version > 0)
);

INSERT INTO character_life_metrics (
    namespace_id, account_id, character_id,
    lifetime_ticks, permadeath_combat_ticks, life_metrics_version
)
SELECT namespace_id, account_id, character_id, 0, 0, 1
FROM characters;

CREATE TABLE character_life_deeds (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    deed_id TEXT NOT NULL,
    reward_event_id BYTEA NOT NULL,
    source_content_id TEXT NOT NULL,
    deed_kind SMALLINT NOT NULL,
    achieved_tick BIGINT NOT NULL,
    content_revision TEXT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id, deed_id),
    UNIQUE (namespace_id, account_id, reward_event_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT life_deed_id_bounded CHECK (length(deed_id) BETWEEN 3 AND 96),
    CONSTRAINT life_deed_reward_event_exact CHECK (
        octet_length(reward_event_id) = 16
        AND reward_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT life_deed_source_bounded CHECK (length(source_content_id) BETWEEN 3 AND 96),
    CONSTRAINT life_deed_kind_known CHECK (deed_kind IN (0, 1)),
    CONSTRAINT life_deed_tick_nonnegative CHECK (achieved_tick >= 0),
    CONSTRAINT life_deed_content_revision_exact CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    )
);

CREATE TABLE death_events (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    contract_kind TEXT NOT NULL,
    mutation_id BYTEA NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    content_revision TEXT NOT NULL,
    instance_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    region_id TEXT NOT NULL,
    room_id TEXT NOT NULL,
    death_tick BIGINT NOT NULL,
    cause_kind SMALLINT NOT NULL,
    killer_content_id TEXT,
    killer_pattern_id TEXT,
    killer_attack_id TEXT,
    raw_damage INTEGER NOT NULL,
    final_damage INTEGER NOT NULL,
    damage_type SMALLINT NOT NULL,
    pre_hit_health INTEGER NOT NULL,
    source_x_milli_tiles INTEGER NOT NULL,
    source_y_milli_tiles INTEGER NOT NULL,
    network_state SMALLINT NOT NULL,
    recall_state SMALLINT NOT NULL,
    lifetime_ticks BIGINT NOT NULL,
    permadeath_combat_ticks BIGINT NOT NULL,
    pre_account_version BIGINT NOT NULL,
    post_account_version BIGINT NOT NULL,
    pre_character_version BIGINT NOT NULL,
    post_character_version BIGINT NOT NULL,
    pre_progression_version BIGINT NOT NULL,
    post_progression_version BIGINT NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    pre_life_metrics_version BIGINT NOT NULL,
    post_life_metrics_version BIGINT NOT NULL,
    trace_digest BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, death_id),
    UNIQUE (namespace_id, account_id, character_id, contract_kind),
    UNIQUE (namespace_id, account_id, mutation_id),
    UNIQUE (namespace_id, account_id, death_id),
    UNIQUE (namespace_id, account_id, character_id, death_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT death_id_uuid_v7 CHECK (
        octet_length(death_id) = 16
        AND death_id <> decode(repeat('00', 16), 'hex')
        AND (get_byte(death_id, 6) >> 4) = 7
        AND (get_byte(death_id, 8) & 192) = 128
    ),
    CONSTRAINT death_contract_exact CHECK (contract_kind = 'permadeath-v1'),
    CONSTRAINT death_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT death_request_hash_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT death_content_revision_exact CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT death_instance_ids_exact CHECK (
        octet_length(instance_id) = 16
        AND instance_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(lineage_id) = 16
        AND lineage_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(restore_point_id) = 16
        AND restore_point_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT death_location_ids_bounded CHECK (
        length(region_id) BETWEEN 3 AND 96 AND length(room_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT death_tick_positive CHECK (death_tick > 0),
    CONSTRAINT death_cause_final_known CHECK (cause_kind BETWEEN 0 AND 3),
    CONSTRAINT death_killer_ids_bounded CHECK (
        (killer_content_id IS NULL OR length(killer_content_id) BETWEEN 3 AND 96)
        AND (killer_pattern_id IS NULL OR length(killer_pattern_id) BETWEEN 3 AND 96)
        AND (killer_attack_id IS NULL OR length(killer_attack_id) BETWEEN 3 AND 96)
    ),
    CONSTRAINT death_damage_shape CHECK (
        raw_damage >= 0 AND final_damage > 0 AND final_damage <= raw_damage
        AND damage_type BETWEEN 0 AND 6 AND pre_hit_health > 0
        AND final_damage >= pre_hit_health
    ),
    CONSTRAINT death_terminal_state_known CHECK (
        network_state BETWEEN 0 AND 3 AND recall_state BETWEEN 0 AND 2
    ),
    CONSTRAINT death_clocks_nonnegative CHECK (
        lifetime_ticks >= 0 AND permadeath_combat_ticks >= 0
    ),
    CONSTRAINT death_versions_exact CHECK (
        pre_account_version > 0 AND post_account_version = pre_account_version + 1
        AND pre_character_version > 0 AND post_character_version = pre_character_version + 1
        AND pre_progression_version > 0
        AND post_progression_version = pre_progression_version + 1
        AND pre_inventory_version > 0 AND post_inventory_version = pre_inventory_version + 1
        AND pre_life_metrics_version > 0
        AND post_life_metrics_version = pre_life_metrics_version + 1
    ),
    CONSTRAINT death_trace_digest_exact CHECK (
        octet_length(trace_digest) = 32
        AND trace_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE death_combat_trace_entries (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    trace_ordinal SMALLINT NOT NULL,
    event_tick BIGINT NOT NULL,
    event_ordinal INTEGER NOT NULL,
    source_content_id TEXT,
    source_entity_id BYTEA,
    pattern_id TEXT,
    attack_id TEXT,
    raw_damage INTEGER NOT NULL,
    final_damage INTEGER NOT NULL,
    damage_type SMALLINT NOT NULL,
    pre_health INTEGER NOT NULL,
    post_health INTEGER NOT NULL,
    source_x_milli_tiles INTEGER NOT NULL,
    source_y_milli_tiles INTEGER NOT NULL,
    network_state SMALLINT NOT NULL,
    recall_state SMALLINT NOT NULL,
    lethal BOOLEAN NOT NULL,
    PRIMARY KEY (namespace_id, death_id, trace_ordinal),
    UNIQUE (namespace_id, death_id, event_tick, event_ordinal),
    FOREIGN KEY (namespace_id, death_id)
        REFERENCES death_events(namespace_id, death_id) ON DELETE CASCADE,
    CONSTRAINT death_trace_ordinal_bounded CHECK (trace_ordinal BETWEEN 0 AND 4095),
    CONSTRAINT death_trace_tick_positive CHECK (event_tick > 0 AND event_ordinal >= 0),
    CONSTRAINT death_trace_source_shape CHECK (
        (source_content_id IS NULL OR length(source_content_id) BETWEEN 3 AND 96)
        AND (source_entity_id IS NULL OR octet_length(source_entity_id) = 16)
        AND (pattern_id IS NULL OR length(pattern_id) BETWEEN 3 AND 96)
        AND (attack_id IS NULL OR length(attack_id) BETWEEN 3 AND 96)
    ),
    CONSTRAINT death_trace_damage_shape CHECK (
        raw_damage >= 0 AND final_damage >= 0 AND final_damage <= raw_damage
        AND damage_type BETWEEN 0 AND 6
        AND pre_health > 0 AND post_health BETWEEN 0 AND pre_health
        AND post_health = GREATEST(0, pre_health - final_damage)
    ),
    CONSTRAINT death_trace_terminal_shape CHECK ((lethal AND post_health = 0) OR NOT lethal),
    CONSTRAINT death_trace_state_known CHECK (
        network_state BETWEEN 0 AND 3 AND recall_state BETWEEN 0 AND 2
    )
);

CREATE TABLE death_combat_trace_statuses (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    trace_ordinal SMALLINT NOT NULL,
    status_ordinal SMALLINT NOT NULL,
    status_id TEXT NOT NULL,
    remaining_ticks INTEGER NOT NULL,
    stack_count SMALLINT NOT NULL,
    PRIMARY KEY (namespace_id, death_id, trace_ordinal, status_ordinal),
    FOREIGN KEY (namespace_id, death_id, trace_ordinal)
        REFERENCES death_combat_trace_entries(namespace_id, death_id, trace_ordinal)
        ON DELETE CASCADE,
    CONSTRAINT death_trace_status_ordinal_bounded CHECK (status_ordinal BETWEEN 0 AND 31),
    CONSTRAINT death_trace_status_id_bounded CHECK (length(status_id) BETWEEN 3 AND 96),
    CONSTRAINT death_trace_status_values_bounded CHECK (
        remaining_ticks BETWEEN 0 AND 108000 AND stack_count BETWEEN 1 AND 255
    )
);

CREATE TABLE death_summary_snapshots (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    summary_revision SMALLINT NOT NULL,
    hero_label_key TEXT NOT NULL,
    character_name_snapshot TEXT NOT NULL,
    class_id TEXT NOT NULL,
    level SMALLINT NOT NULL,
    oath_id TEXT,
    lifetime_ms BIGINT NOT NULL,
    final_deed_id TEXT NOT NULL,
    echo_outcome SMALLINT NOT NULL,
    content_revision TEXT NOT NULL,
    snapshot_digest BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, death_id),
    FOREIGN KEY (namespace_id, death_id)
        REFERENCES death_events(namespace_id, death_id) ON DELETE CASCADE,
    CONSTRAINT death_summary_revision_v1 CHECK (summary_revision = 1),
    CONSTRAINT death_summary_labels_bounded CHECK (
        length(hero_label_key) BETWEEN 3 AND 96
        AND length(character_name_snapshot) BETWEEN 1 AND 24
        AND length(class_id) BETWEEN 3 AND 96
        AND (oath_id IS NULL OR length(oath_id) BETWEEN 3 AND 96)
        AND length(final_deed_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT death_summary_level_core CHECK (level BETWEEN 1 AND 10),
    CONSTRAINT death_summary_lifetime_nonnegative CHECK (lifetime_ms >= 0),
    CONSTRAINT death_summary_echo_outcome_known CHECK (echo_outcome BETWEEN 0 AND 5),
    CONSTRAINT death_summary_content_revision_exact CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT death_summary_digest_exact CHECK (
        octet_length(snapshot_digest) = 32
        AND snapshot_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE death_summary_bargains (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    bargain_ordinal SMALLINT NOT NULL,
    bargain_id TEXT NOT NULL,
    PRIMARY KEY (namespace_id, death_id, bargain_ordinal),
    UNIQUE (namespace_id, death_id, bargain_id),
    FOREIGN KEY (namespace_id, death_id)
        REFERENCES death_summary_snapshots(namespace_id, death_id) ON DELETE CASCADE,
    CONSTRAINT death_summary_bargain_ordinal_bounded CHECK (bargain_ordinal BETWEEN 0 AND 2),
    CONSTRAINT death_summary_bargain_id_bounded CHECK (length(bargain_id) BETWEEN 3 AND 96)
);

CREATE TABLE death_summary_damage_entries (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    summary_ordinal SMALLINT NOT NULL,
    trace_ordinal SMALLINT NOT NULL,
    PRIMARY KEY (namespace_id, death_id, summary_ordinal),
    UNIQUE (namespace_id, death_id, trace_ordinal),
    FOREIGN KEY (namespace_id, death_id, trace_ordinal)
        REFERENCES death_combat_trace_entries(namespace_id, death_id, trace_ordinal)
        ON DELETE CASCADE,
    CONSTRAINT death_summary_damage_ordinal_bounded CHECK (summary_ordinal BETWEEN 0 AND 4)
);

CREATE TABLE death_summary_projection_entries (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    section_kind SMALLINT NOT NULL,
    entry_ordinal SMALLINT NOT NULL,
    projection_kind SMALLINT NOT NULL,
    content_id TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    item_uid BYTEA,
    PRIMARY KEY (namespace_id, death_id, section_kind, entry_ordinal),
    FOREIGN KEY (namespace_id, death_id)
        REFERENCES death_summary_snapshots(namespace_id, death_id) ON DELETE CASCADE,
    CONSTRAINT death_summary_section_known CHECK (section_kind BETWEEN 0 AND 2),
    CONSTRAINT death_summary_projection_ordinal_bounded CHECK (entry_ordinal BETWEEN 0 AND 255),
    CONSTRAINT death_summary_projection_kind_known CHECK (projection_kind BETWEEN 0 AND 15),
    CONSTRAINT death_summary_projection_content_bounded CHECK (length(content_id) BETWEEN 3 AND 96),
    CONSTRAINT death_summary_projection_quantity_positive CHECK (quantity > 0),
    CONSTRAINT death_summary_projection_item_exact CHECK (
        item_uid IS NULL OR octet_length(item_uid) = 16
    )
);

CREATE TABLE memorial_records (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    death_at TIMESTAMPTZ NOT NULL,
    summary_revision SMALLINT NOT NULL,
    presentation_key TEXT NOT NULL,
    presentation_digest BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, death_id),
    FOREIGN KEY (namespace_id, death_id)
        REFERENCES death_summary_snapshots(namespace_id, death_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, death_id)
        REFERENCES death_events(namespace_id, account_id, death_id) ON DELETE CASCADE,
    CONSTRAINT memorial_summary_revision_v1 CHECK (summary_revision = 1),
    CONSTRAINT memorial_presentation_key_bounded CHECK (
        length(presentation_key) BETWEEN 3 AND 96
    ),
    CONSTRAINT memorial_presentation_digest_exact CHECK (
        octet_length(presentation_digest) = 32
        AND presentation_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE INDEX memorial_records_newest_first
    ON memorial_records (namespace_id, account_id, death_at DESC, death_id);

CREATE TABLE echo_records (
    namespace_id TEXT NOT NULL,
    echo_id BYTEA NOT NULL,
    death_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_name_snapshot TEXT NOT NULL,
    class_id TEXT NOT NULL,
    oath_id TEXT,
    level SMALLINT NOT NULL,
    appearance_snapshot_id TEXT NOT NULL,
    appearance_theme_id TEXT NOT NULL,
    weapon_signature_tag TEXT,
    relic_signature_tag TEXT,
    killer_content_id TEXT,
    killer_pattern_id TEXT,
    death_region_id TEXT NOT NULL,
    power_band SMALLINT NOT NULL,
    state SMALLINT NOT NULL,
    content_revision TEXT NOT NULL,
    snapshot_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, echo_id),
    UNIQUE (namespace_id, death_id),
    FOREIGN KEY (namespace_id, account_id, death_id)
        REFERENCES death_events(namespace_id, account_id, death_id) ON DELETE CASCADE,
    CONSTRAINT echo_id_uuid_v7 CHECK (
        octet_length(echo_id) = 16
        AND echo_id <> decode(repeat('00', 16), 'hex')
        AND (get_byte(echo_id, 6) >> 4) = 7
        AND (get_byte(echo_id, 8) & 192) = 128
    ),
    CONSTRAINT echo_snapshot_labels_bounded CHECK (
        length(character_name_snapshot) BETWEEN 1 AND 24
        AND length(class_id) BETWEEN 3 AND 96
        AND (oath_id IS NULL OR length(oath_id) BETWEEN 3 AND 96)
        AND length(appearance_snapshot_id) BETWEEN 3 AND 96
        AND length(appearance_theme_id) BETWEEN 3 AND 96
        AND (weapon_signature_tag IS NULL OR length(weapon_signature_tag) BETWEEN 3 AND 96)
        AND (relic_signature_tag IS NULL OR length(relic_signature_tag) BETWEEN 3 AND 96)
        AND (killer_content_id IS NULL OR length(killer_content_id) BETWEEN 3 AND 96)
        AND (killer_pattern_id IS NULL OR length(killer_pattern_id) BETWEEN 3 AND 96)
        AND length(death_region_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT echo_level_core CHECK (level = 10),
    CONSTRAINT echo_power_band_known CHECK (power_band BETWEEN 1 AND 5),
    CONSTRAINT echo_state_known CHECK (state BETWEEN 0 AND 4),
    CONSTRAINT echo_content_revision_exact CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT echo_snapshot_digest_exact CHECK (
        octet_length(snapshot_digest) = 32
        AND snapshot_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE UNIQUE INDEX one_available_echo_per_account
    ON echo_records (namespace_id, account_id)
    WHERE state = 1;

CREATE INDEX dormant_echoes_oldest_first
    ON echo_records (namespace_id, account_id, created_at, echo_id)
    WHERE state = 0;

CREATE TABLE echo_bargain_snapshots (
    namespace_id TEXT NOT NULL,
    echo_id BYTEA NOT NULL,
    bargain_ordinal SMALLINT NOT NULL,
    bargain_id TEXT NOT NULL,
    PRIMARY KEY (namespace_id, echo_id, bargain_ordinal),
    UNIQUE (namespace_id, echo_id, bargain_id),
    FOREIGN KEY (namespace_id, echo_id)
        REFERENCES echo_records(namespace_id, echo_id) ON DELETE CASCADE,
    CONSTRAINT echo_bargain_ordinal_bounded CHECK (bargain_ordinal BETWEEN 0 AND 2),
    CONSTRAINT echo_bargain_id_bounded CHECK (length(bargain_id) BETWEEN 3 AND 96)
);

CREATE TABLE echo_deed_tags (
    namespace_id TEXT NOT NULL,
    echo_id BYTEA NOT NULL,
    deed_ordinal SMALLINT NOT NULL,
    deed_tag TEXT NOT NULL,
    PRIMARY KEY (namespace_id, echo_id, deed_ordinal),
    UNIQUE (namespace_id, echo_id, deed_tag),
    FOREIGN KEY (namespace_id, echo_id)
        REFERENCES echo_records(namespace_id, echo_id) ON DELETE CASCADE,
    CONSTRAINT echo_deed_ordinal_bounded CHECK (deed_ordinal BETWEEN 0 AND 31),
    CONSTRAINT echo_deed_tag_bounded CHECK (length(deed_tag) BETWEEN 3 AND 96)
);

CREATE TABLE echo_state_transitions (
    namespace_id TEXT NOT NULL,
    echo_id BYTEA NOT NULL,
    transition_ordinal SMALLINT NOT NULL,
    previous_state SMALLINT,
    next_state SMALLINT NOT NULL,
    reason_kind SMALLINT NOT NULL,
    source_death_id BYTEA,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, echo_id, transition_ordinal),
    FOREIGN KEY (namespace_id, echo_id)
        REFERENCES echo_records(namespace_id, echo_id) ON DELETE CASCADE,
    CONSTRAINT echo_transition_ordinal_bounded CHECK (transition_ordinal BETWEEN 0 AND 32767),
    CONSTRAINT echo_transition_states_known CHECK (
        (previous_state IS NULL OR previous_state BETWEEN 0 AND 4)
        AND next_state BETWEEN 0 AND 4
    ),
    CONSTRAINT echo_transition_reason_known CHECK (reason_kind BETWEEN 0 AND 7),
    CONSTRAINT echo_transition_creation_shape CHECK (
        (transition_ordinal = 0 AND previous_state IS NULL AND next_state = 0
            AND source_death_id IS NOT NULL AND octet_length(source_death_id) = 16)
        OR (transition_ordinal > 0 AND previous_state IS NOT NULL)
    )
);

CREATE TABLE death_destruction_entries (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    destruction_ordinal SMALLINT NOT NULL,
    entry_kind SMALLINT NOT NULL,
    item_uid BYTEA,
    material_id TEXT,
    quantity INTEGER NOT NULL,
    pre_location_kind SMALLINT,
    pre_slot_index SMALLINT,
    pre_instance_id BYTEA,
    pre_pickup_id BYTEA,
    pre_item_version BIGINT,
    post_item_version BIGINT,
    ledger_event_id BYTEA,
    PRIMARY KEY (namespace_id, death_id, destruction_ordinal),
    UNIQUE (namespace_id, death_id, item_uid),
    UNIQUE (namespace_id, death_id, material_id),
    FOREIGN KEY (namespace_id, death_id)
        REFERENCES death_events(namespace_id, death_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid) ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, ledger_event_id)
        REFERENCES item_ledger_events(namespace_id, ledger_event_id) ON DELETE RESTRICT,
    CONSTRAINT death_destruction_ordinal_bounded CHECK (
        destruction_ordinal BETWEEN 0 AND 4095
    ),
    CONSTRAINT death_destruction_entry_shape CHECK (
        (entry_kind = 0
            AND item_uid IS NOT NULL AND octet_length(item_uid) = 16
            AND material_id IS NULL AND quantity = 1
            AND pre_location_kind BETWEEN 0 AND 3
            AND pre_item_version > 0 AND post_item_version = pre_item_version + 1
            AND ledger_event_id IS NOT NULL AND octet_length(ledger_event_id) = 16)
        OR (entry_kind = 1
            AND item_uid IS NULL AND material_id IS NOT NULL
            AND length(material_id) BETWEEN 3 AND 96 AND quantity > 0
            AND pre_location_kind IS NULL AND pre_slot_index IS NULL
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL
            AND pre_item_version IS NULL AND post_item_version IS NULL
            AND ledger_event_id IS NULL)
    ),
    CONSTRAINT death_destruction_location_shape CHECK (
        entry_kind = 1
        OR (pre_location_kind = 0 AND pre_slot_index BETWEEN 0 AND 3
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL)
        OR (pre_location_kind = 1 AND pre_slot_index BETWEEN 0 AND 1
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL)
        OR (pre_location_kind = 2 AND pre_slot_index BETWEEN 0 AND 7
            AND pre_instance_id IS NULL AND pre_pickup_id IS NULL)
        OR (pre_location_kind = 3 AND pre_slot_index IS NULL
            AND octet_length(pre_instance_id) = 16 AND octet_length(pre_pickup_id) = 16)
    )
);

CREATE TABLE death_mutation_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    contract_kind TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    result_code SMALLINT NOT NULL,
    result_payload BYTEA NOT NULL,
    result_hash BYTEA NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    UNIQUE (namespace_id, account_id, character_id, contract_kind),
    FOREIGN KEY (namespace_id, account_id, character_id, death_id)
        REFERENCES death_events(namespace_id, account_id, character_id, death_id)
        ON DELETE CASCADE,
    CONSTRAINT death_result_mutation_id_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT death_result_contract_exact CHECK (contract_kind = 'permadeath-v1'),
    CONSTRAINT death_result_request_hash_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT death_result_code_committed CHECK (result_code = 1),
    CONSTRAINT death_result_payload_bounded CHECK (
        octet_length(result_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT death_result_hash_exact CHECK (
        octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT death_result_issue_order CHECK (committed_at >= issued_at)
);

CREATE TABLE death_audit_events (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    audit_event_id BYTEA NOT NULL,
    death_id BYTEA,
    mutation_id BYTEA NOT NULL,
    event_kind SMALLINT NOT NULL,
    event_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, audit_event_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, death_id)
        REFERENCES death_events(namespace_id, account_id, character_id, death_id)
        ON DELETE CASCADE,
    CONSTRAINT death_audit_event_id_exact CHECK (
        octet_length(audit_event_id) = 16
        AND audit_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT death_audit_mutation_id_exact CHECK (octet_length(mutation_id) = 16),
    CONSTRAINT death_audit_event_kind_known CHECK (event_kind BETWEEN 0 AND 3),
    CONSTRAINT death_audit_digest_exact CHECK (
        octet_length(event_digest) = 32
        AND event_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE death_outbox_events (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    event_id BYTEA NOT NULL,
    event_type TEXT NOT NULL,
    echo_id BYTEA,
    event_payload BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, death_id, event_type, echo_id),
    FOREIGN KEY (namespace_id, death_id)
        REFERENCES death_events(namespace_id, death_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, echo_id)
        REFERENCES echo_records(namespace_id, echo_id) ON DELETE CASCADE,
    CONSTRAINT death_outbox_event_id_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT death_outbox_event_type_known CHECK (
        event_type IN ('death_committed', 'echo_created', 'echo_promoted')
    ),
    CONSTRAINT death_outbox_echo_shape CHECK (
        (event_type = 'death_committed' AND echo_id IS NULL)
        OR (event_type IN ('echo_created', 'echo_promoted')
            AND echo_id IS NOT NULL AND octet_length(echo_id) = 16)
    ),
    CONSTRAINT death_outbox_payload_bounded CHECK (
        octet_length(event_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT death_outbox_publish_order CHECK (
        published_at IS NULL OR published_at >= created_at
    )
);

CREATE INDEX death_events_by_account_time
    ON death_events (namespace_id, account_id, committed_at DESC, death_id);

CREATE INDEX death_audit_events_by_character_time
    ON death_audit_events (namespace_id, account_id, character_id, created_at, audit_event_id);

CREATE INDEX unpublished_death_outbox_events
    ON death_outbox_events (namespace_id, created_at, event_id)
    WHERE published_at IS NULL;

CREATE UNIQUE INDEX one_death_committed_outbox_event
    ON death_outbox_events (namespace_id, death_id, event_type)
    WHERE event_type = 'death_committed';

CREATE UNIQUE INDEX one_echo_outbox_event_per_transition_kind
    ON death_outbox_events (namespace_id, death_id, echo_id, event_type)
    WHERE echo_id IS NOT NULL;

COMMENT ON COLUMN characters.life_state IS '0 Living, 1 Dead.';
COMMENT ON COLUMN item_instances.destruction_reason IS
    'Null for live custody; ground_expired or permadeath for Destroyed rows.';
