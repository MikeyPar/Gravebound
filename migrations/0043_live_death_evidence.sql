-- GB-M03-06B live death-evidence persistence foundation.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-001/DTH-020 and TECH-020..023;
-- - Gravebound_Content_Production_Spec_v1.md Core encounter/reward authority and CONT-ECHO-009;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06/13 and the restart/atomic-death gates;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md decision 4.
--
-- This forward-only Core migration records committed clock intervals and reward-qualified deed
-- completions append-only. Live damage evidence remains a bounded, normalized danger journal: its
-- rows are immutable, but complete tick roots may be pruned or removed by the owning danger-root
-- cascade. The 4 KiB Bell Debt checkpoint payload remains unchanged and owns no death authority.

ALTER TABLE character_entry_restore_points
    ADD CONSTRAINT restore_point_live_evidence_authority_unique UNIQUE (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    );

ALTER TABLE character_danger_checkpoints
    ADD CONSTRAINT danger_checkpoint_live_evidence_authority_unique UNIQUE (
        namespace_id, account_id, character_id, lineage_id,
        records_blake3, assets_blake3, localization_blake3
    );

CREATE TABLE character_life_clock_checkpoint_receipts_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    checkpoint_id BYTEA NOT NULL,
    authoritative_tick BIGINT NOT NULL,
    clock_state SMALLINT NOT NULL,
    advanced_ticks INTEGER NOT NULL,
    lineage_id BYTEA,
    restore_point_id BYTEA,
    danger_entry_life_metrics_version BIGINT,
    danger_entry_permadeath_combat_ticks BIGINT,
    pre_lifetime_ticks BIGINT NOT NULL,
    post_lifetime_ticks BIGINT NOT NULL,
    pre_permadeath_combat_ticks BIGINT NOT NULL,
    post_permadeath_combat_ticks BIGINT NOT NULL,
    pre_link_lost_ticks SMALLINT NOT NULL,
    post_link_lost_ticks SMALLINT NOT NULL,
    pre_life_metrics_version BIGINT NOT NULL,
    post_life_metrics_version BIGINT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    result_digest BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, checkpoint_id),
    UNIQUE (namespace_id, account_id, character_id, checkpoint_id),
    UNIQUE (namespace_id, account_id, character_id, authoritative_tick),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) REFERENCES character_entry_restore_points (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id,
        danger_entry_life_metrics_version
    ) REFERENCES entry_restore_life_metrics_v3 (
        namespace_id, account_id, character_id, restore_point_id, life_metrics_version
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT life_clock_checkpoint_id_exact CHECK (
        octet_length(checkpoint_id) = 16
        AND checkpoint_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT life_clock_checkpoint_state_known CHECK (clock_state BETWEEN 0 AND 7),
    CONSTRAINT life_clock_checkpoint_interval_bounded CHECK (
        authoritative_tick >= 0 AND advanced_ticks BETWEEN 0 AND 1800
    ),
    CONSTRAINT life_clock_checkpoint_versions_exact CHECK (
        pre_life_metrics_version > 0
        AND post_life_metrics_version = pre_life_metrics_version + 1
    ),
    CONSTRAINT life_clock_checkpoint_ticks_exact CHECK (
        pre_lifetime_ticks >= 0
        AND pre_permadeath_combat_ticks >= 0
        AND post_lifetime_ticks = pre_lifetime_ticks
            + CASE WHEN clock_state IN (3, 6, 7) THEN advanced_ticks ELSE 0 END
        AND post_permadeath_combat_ticks = pre_permadeath_combat_ticks
            + CASE WHEN clock_state BETWEEN 4 AND 7 THEN advanced_ticks ELSE 0 END
    ),
    CONSTRAINT life_clock_checkpoint_link_lost_exact CHECK (
        pre_link_lost_ticks BETWEEN 0 AND 90
        AND post_link_lost_ticks BETWEEN 0 AND 90
        AND (
            (clock_state = 7
                AND post_link_lost_ticks = pre_link_lost_ticks + advanced_ticks)
            OR (clock_state <> 7 AND post_link_lost_ticks = 0)
        )
    ),
    CONSTRAINT life_clock_checkpoint_danger_shape CHECK (
        (
            clock_state BETWEEN 4 AND 7
            AND lineage_id IS NOT NULL
            AND restore_point_id IS NOT NULL
            AND danger_entry_life_metrics_version IS NOT NULL
            AND danger_entry_life_metrics_version > 0
            AND danger_entry_life_metrics_version <= pre_life_metrics_version
            AND danger_entry_permadeath_combat_ticks IS NOT NULL
            AND danger_entry_permadeath_combat_ticks >= 0
            AND danger_entry_permadeath_combat_ticks <= pre_permadeath_combat_ticks
        ) OR (
            clock_state BETWEEN 0 AND 3
            AND lineage_id IS NULL
            AND restore_point_id IS NULL
            AND danger_entry_life_metrics_version IS NULL
            AND danger_entry_permadeath_combat_ticks IS NULL
        )
    ),
    CONSTRAINT life_clock_checkpoint_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT life_clock_checkpoint_hashes_exact CHECK (
        octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_digest) = 32
        AND result_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE INDEX life_clock_checkpoints_newest_first_v1
    ON character_life_clock_checkpoint_receipts_v1 (
        namespace_id, account_id, character_id, authoritative_tick DESC, committed_at DESC
    );

CREATE TABLE character_life_deed_completion_receipts_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    completion_id BYTEA NOT NULL,
    deed_id TEXT NOT NULL,
    source_content_id TEXT NOT NULL,
    deed_kind SMALLINT NOT NULL,
    achieved_tick BIGINT NOT NULL,
    content_revision TEXT NOT NULL,
    projection_outcome SMALLINT NOT NULL,
    request_hash BYTEA NOT NULL,
    result_digest BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, completion_id),
    UNIQUE (namespace_id, account_id, character_id, completion_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT life_deed_completion_id_exact CHECK (
        octet_length(completion_id) = 16
        AND completion_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT life_deed_completion_ids_bounded CHECK (
        length(deed_id) BETWEEN 3 AND 96
        AND length(source_content_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT life_deed_completion_kind_known CHECK (deed_kind IN (0, 1)),
    CONSTRAINT life_deed_completion_tick_positive CHECK (achieved_tick > 0),
    CONSTRAINT life_deed_completion_revision_exact CHECK (
        content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT life_deed_completion_projection_known CHECK (projection_outcome BETWEEN 0 AND 2),
    CONSTRAINT life_deed_completion_hashes_exact CHECK (
        octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_digest) = 32
        AND result_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE INDEX life_deed_completions_latest_v1
    ON character_life_deed_completion_receipts_v1 (
        namespace_id, account_id, character_id, achieved_tick DESC, deed_id COLLATE "C" DESC
    );

CREATE TABLE character_live_damage_trace_ticks_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    trace_tick_id BYTEA NOT NULL,
    event_tick BIGINT NOT NULL,
    entry_count SMALLINT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    tick_digest BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, trace_tick_id),
    UNIQUE (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        event_tick, trace_tick_id
    ),
    UNIQUE (namespace_id, account_id, character_id, lineage_id, event_tick),
    FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id,
        records_blake3, assets_blake3, localization_blake3
    ) REFERENCES character_danger_checkpoints (
        namespace_id, account_id, character_id, lineage_id,
        records_blake3, assets_blake3, localization_blake3
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) REFERENCES character_entry_restore_points (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT live_trace_tick_id_exact CHECK (
        octet_length(trace_tick_id) = 16
        AND trace_tick_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT live_trace_tick_shape CHECK (event_tick > 0 AND entry_count BETWEEN 1 AND 4096),
    CONSTRAINT live_trace_tick_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT live_trace_tick_hashes_exact CHECK (
        octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(tick_digest) = 32
        AND tick_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE INDEX live_trace_ticks_ordered_v1
    ON character_live_damage_trace_ticks_v1 (
        namespace_id, account_id, character_id, lineage_id, event_tick, trace_tick_id
    );

CREATE TABLE character_live_damage_trace_entries_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    trace_tick_id BYTEA NOT NULL,
    event_tick BIGINT NOT NULL,
    event_ordinal INTEGER NOT NULL,
    cause_kind SMALLINT NOT NULL,
    source_content_id TEXT NOT NULL,
    source_entity_id BYTEA,
    pattern_id TEXT,
    attack_id TEXT NOT NULL,
    raw_damage INTEGER NOT NULL,
    final_damage INTEGER NOT NULL,
    damage_type SMALLINT NOT NULL,
    pre_health INTEGER NOT NULL,
    post_health INTEGER NOT NULL,
    source_x_milli_tiles INTEGER NOT NULL,
    source_y_milli_tiles INTEGER NOT NULL,
    status_count SMALLINT NOT NULL,
    network_state SMALLINT NOT NULL,
    recall_state SMALLINT NOT NULL,
    lethal BOOLEAN NOT NULL,
    entry_digest BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, account_id, trace_tick_id, event_ordinal),
    UNIQUE (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        event_tick, event_ordinal
    ),
    UNIQUE (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        trace_tick_id, event_tick, event_ordinal
    ),
    FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        event_tick, trace_tick_id
    ) REFERENCES character_live_damage_trace_ticks_v1 (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        event_tick, trace_tick_id
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT live_trace_entry_ordinal_bounded CHECK (event_ordinal >= 0),
    CONSTRAINT live_trace_entry_cause_known CHECK (cause_kind BETWEEN 0 AND 3),
    CONSTRAINT live_trace_entry_source_shape CHECK (
        length(source_content_id) BETWEEN 3 AND 96
        AND (source_entity_id IS NULL OR (
            octet_length(source_entity_id) = 16
            AND source_entity_id <> decode(repeat('00', 16), 'hex')
        ))
        AND (pattern_id IS NULL OR length(pattern_id) BETWEEN 3 AND 96)
        AND length(attack_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT live_trace_entry_damage_shape CHECK (
        raw_damage >= 0 AND final_damage >= 0
        AND damage_type IN (0, 1)
        AND pre_health > 0 AND post_health BETWEEN 0 AND pre_health
        AND post_health = GREATEST(0, pre_health - final_damage)
    ),
    CONSTRAINT live_trace_entry_terminal_shape CHECK (
        (lethal AND post_health = 0) OR (NOT lethal AND post_health > 0)
    ),
    CONSTRAINT live_trace_entry_status_count_bounded CHECK (status_count BETWEEN 0 AND 32),
    CONSTRAINT live_trace_entry_state_known CHECK (
        network_state BETWEEN 0 AND 3 AND recall_state BETWEEN 0 AND 2
    ),
    CONSTRAINT live_trace_entry_digest_exact CHECK (
        octet_length(entry_digest) = 32
        AND entry_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE character_live_damage_trace_statuses_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    trace_tick_id BYTEA NOT NULL,
    event_tick BIGINT NOT NULL,
    event_ordinal INTEGER NOT NULL,
    status_ordinal SMALLINT NOT NULL,
    status_id TEXT COLLATE "C" NOT NULL,
    remaining_ticks INTEGER NOT NULL,
    stack_count SMALLINT NOT NULL,
    PRIMARY KEY (
        namespace_id, account_id, trace_tick_id, event_ordinal, status_ordinal
    ),
    UNIQUE (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        trace_tick_id, event_tick, event_ordinal, status_id
    ),
    FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        trace_tick_id, event_tick, event_ordinal
    ) REFERENCES character_live_damage_trace_entries_v1 (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        trace_tick_id, event_tick, event_ordinal
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT live_trace_status_ordinal_bounded CHECK (status_ordinal BETWEEN 0 AND 31),
    CONSTRAINT live_trace_status_id_bounded CHECK (length(status_id) BETWEEN 3 AND 96),
    CONSTRAINT live_trace_status_values_bounded CHECK (
        remaining_ticks BETWEEN 0 AND 108000 AND stack_count BETWEEN 1 AND 255
    )
);

CREATE FUNCTION reject_live_death_evidence_receipt_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
    RAISE EXCEPTION '% is append-only live death evidence', TG_TABLE_NAME;
END
$$;

CREATE TRIGGER life_clock_checkpoint_receipt_append_only_v1
BEFORE UPDATE OR DELETE ON character_life_clock_checkpoint_receipts_v1
FOR EACH ROW EXECUTE FUNCTION reject_live_death_evidence_receipt_mutation_v1();
CREATE TRIGGER life_deed_completion_receipt_append_only_v1
BEFORE UPDATE OR DELETE ON character_life_deed_completion_receipts_v1
FOR EACH ROW EXECUTE FUNCTION reject_live_death_evidence_receipt_mutation_v1();

CREATE FUNCTION reject_live_damage_trace_update_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% rows are immutable; prune the complete trace tick instead', TG_TABLE_NAME;
END
$$;

CREATE TRIGGER live_trace_tick_immutable_v1
BEFORE UPDATE ON character_live_damage_trace_ticks_v1
FOR EACH ROW EXECUTE FUNCTION reject_live_damage_trace_update_v1();
CREATE TRIGGER live_trace_entry_immutable_v1
BEFORE UPDATE ON character_live_damage_trace_entries_v1
FOR EACH ROW EXECUTE FUNCTION reject_live_damage_trace_update_v1();
CREATE TRIGGER live_trace_status_immutable_v1
BEFORE UPDATE ON character_live_damage_trace_statuses_v1
FOR EACH ROW EXECUTE FUNCTION reject_live_damage_trace_update_v1();

CREATE TRIGGER dead_life_clock_checkpoint_insert_v1
BEFORE INSERT ON character_life_clock_checkpoint_receipts_v1
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_life_deed_completion_insert_v1
BEFORE INSERT ON character_life_deed_completion_receipts_v1
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_live_trace_tick_insert_v1
BEFORE INSERT ON character_live_damage_trace_ticks_v1
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_live_trace_entry_insert_v1
BEFORE INSERT ON character_live_damage_trace_entries_v1
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_live_trace_status_insert_v1
BEFORE INSERT ON character_live_damage_trace_statuses_v1
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();

CREATE FUNCTION enforce_life_clock_checkpoint_entry_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    stored_rollback_ticks BIGINT;
BEGIN
    IF NEW.clock_state BETWEEN 0 AND 3 THEN RETURN NULL; END IF;
    SELECT rollback_permadeath_combat_ticks
    INTO stored_rollback_ticks
    FROM entry_restore_life_metrics_v3
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND character_id = NEW.character_id
      AND restore_point_id = NEW.restore_point_id
      AND life_metrics_version = NEW.danger_entry_life_metrics_version;
    IF NOT FOUND
        OR stored_rollback_ticks IS DISTINCT FROM NEW.danger_entry_permadeath_combat_ticks
    THEN
        RAISE EXCEPTION 'life clock checkpoint does not match its immutable danger-entry clock';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER life_clock_checkpoint_entry_exact_v1
AFTER INSERT ON character_life_clock_checkpoint_receipts_v1
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_life_clock_checkpoint_entry_v1();

CREATE FUNCTION enforce_live_damage_trace_graph_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT;
    target_account BYTEA;
    target_character BYTEA;
    target_lineage BYTEA;
    target_restore BYTEA;
    target_tick_id BYTEA;
    target_tick BIGINT;
    expected_entry_count SMALLINT;
    actual_entry_count BIGINT;
    window_entry_count BIGINT;
    minimum_tick BIGINT;
    maximum_tick BIGINT;
    lethal_count BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        target_namespace := OLD.namespace_id;
        target_account := OLD.account_id;
        target_character := OLD.character_id;
        target_lineage := OLD.lineage_id;
        target_restore := OLD.restore_point_id;
        target_tick_id := OLD.trace_tick_id;
        target_tick := OLD.event_tick;
    ELSE
        target_namespace := NEW.namespace_id;
        target_account := NEW.account_id;
        target_character := NEW.character_id;
        target_lineage := NEW.lineage_id;
        target_restore := NEW.restore_point_id;
        target_tick_id := NEW.trace_tick_id;
        target_tick := NEW.event_tick;
    END IF;

    SELECT entry_count INTO expected_entry_count
    FROM character_live_damage_trace_ticks_v1
    WHERE namespace_id = target_namespace
      AND account_id = target_account
      AND character_id = target_character
      AND lineage_id = target_lineage
      AND restore_point_id = target_restore
      AND trace_tick_id = target_tick_id
      AND event_tick = target_tick;
    IF NOT FOUND THEN RETURN NULL; END IF;

    SELECT count(*) INTO actual_entry_count
    FROM character_live_damage_trace_entries_v1
    WHERE namespace_id = target_namespace
      AND account_id = target_account
      AND character_id = target_character
      AND lineage_id = target_lineage
      AND restore_point_id = target_restore
      AND trace_tick_id = target_tick_id
      AND event_tick = target_tick;
    IF actual_entry_count <> expected_entry_count THEN
        RAISE EXCEPTION 'live damage trace tick entry count is incomplete';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM character_live_damage_trace_entries_v1 AS entry
        WHERE entry.namespace_id = target_namespace
          AND entry.account_id = target_account
          AND entry.character_id = target_character
          AND entry.lineage_id = target_lineage
          AND entry.restore_point_id = target_restore
          AND entry.trace_tick_id = target_tick_id
          AND entry.event_tick = target_tick
          AND (
              entry.status_count <> (
                  SELECT count(*)
                  FROM character_live_damage_trace_statuses_v1 AS status
                  WHERE status.namespace_id = entry.namespace_id
                    AND status.account_id = entry.account_id
                    AND status.trace_tick_id = entry.trace_tick_id
                    AND status.event_ordinal = entry.event_ordinal
              )
              OR EXISTS (
                  SELECT 1
                  FROM (
                      SELECT status.status_ordinal,
                          row_number() OVER (ORDER BY status.status_ordinal) - 1
                              AS expected_ordinal,
                          status.status_id,
                          lag(status.status_id) OVER (ORDER BY status.status_ordinal)
                              AS previous_status_id
                      FROM character_live_damage_trace_statuses_v1 AS status
                      WHERE status.namespace_id = entry.namespace_id
                        AND status.account_id = entry.account_id
                        AND status.trace_tick_id = entry.trace_tick_id
                        AND status.event_ordinal = entry.event_ordinal
                  ) AS ordered_status
                  WHERE ordered_status.status_ordinal <> ordered_status.expected_ordinal
                     OR (
                         ordered_status.previous_status_id IS NOT NULL
                         AND ordered_status.previous_status_id COLLATE "C"
                             >= ordered_status.status_id COLLATE "C"
                     )
              )
          )
    ) THEN
        RAISE EXCEPTION 'live damage trace statuses are incomplete or noncanonical';
    END IF;

    SELECT count(*), min(event_tick), max(event_tick),
           count(*) FILTER (WHERE lethal)
    INTO window_entry_count, minimum_tick, maximum_tick, lethal_count
    FROM character_live_damage_trace_entries_v1
    WHERE namespace_id = target_namespace
      AND account_id = target_account
      AND character_id = target_character
      AND lineage_id = target_lineage
      AND restore_point_id = target_restore;
    IF window_entry_count > 4096 OR maximum_tick - minimum_tick > 300 THEN
        RAISE EXCEPTION 'live damage trace exceeds its bounded ten-second window';
    END IF;
    IF lethal_count > 1 OR EXISTS (
        SELECT 1
        FROM character_live_damage_trace_entries_v1 AS lethal
        WHERE lethal.namespace_id = target_namespace
          AND lethal.account_id = target_account
          AND lethal.character_id = target_character
          AND lethal.lineage_id = target_lineage
          AND lethal.restore_point_id = target_restore
          AND lethal.lethal
          AND EXISTS (
              SELECT 1
              FROM character_live_damage_trace_entries_v1 AS later
              WHERE later.namespace_id = lethal.namespace_id
                AND later.account_id = lethal.account_id
                AND later.character_id = lethal.character_id
                AND later.lineage_id = lethal.lineage_id
                AND later.restore_point_id = lethal.restore_point_id
                AND (later.event_tick, later.event_ordinal)
                    > (lethal.event_tick, lethal.event_ordinal)
          )
    ) THEN
        RAISE EXCEPTION 'live damage trace lethal evidence is noncanonical';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER live_trace_tick_graph_complete_v1
AFTER INSERT OR DELETE ON character_live_damage_trace_ticks_v1
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_live_damage_trace_graph_v1();
CREATE CONSTRAINT TRIGGER live_trace_entry_graph_complete_v1
AFTER INSERT OR DELETE ON character_live_damage_trace_entries_v1
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_live_damage_trace_graph_v1();
CREATE CONSTRAINT TRIGGER live_trace_status_graph_complete_v1
AFTER INSERT OR DELETE ON character_live_damage_trace_statuses_v1
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_live_damage_trace_graph_v1();

CREATE FUNCTION enforce_live_damage_trace_not_posthumous_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM character_live_damage_trace_ticks_v1 AS trace
        JOIN death_events AS death
          ON death.namespace_id = trace.namespace_id
         AND death.account_id = trace.account_id
         AND death.character_id = trace.character_id
        WHERE trace.namespace_id = NEW.namespace_id
          AND trace.account_id = NEW.account_id
          AND trace.trace_tick_id = NEW.trace_tick_id
    ) THEN
        RAISE EXCEPTION 'live damage trace cannot survive final death';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER live_trace_absent_after_death_v1
AFTER INSERT ON character_live_damage_trace_ticks_v1
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_live_damage_trace_not_posthumous_v1();

CREATE FUNCTION enforce_death_has_no_live_damage_trace_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM character_live_damage_trace_ticks_v1
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id
          AND character_id = NEW.character_id
    ) THEN
        RAISE EXCEPTION 'final death cannot commit while live damage trace remains';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER death_requires_live_trace_cleanup_v1
AFTER INSERT ON death_events
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_death_has_no_live_damage_trace_v1();

COMMENT ON TABLE character_life_clock_checkpoint_receipts_v1 IS
    'Append-only exact 30 Hz clock interval receipts for GB-M03-06B restart and replay.';
COMMENT ON TABLE character_life_deed_completion_receipts_v1 IS
    'Append-only reward-qualified completion receipts; character_life_deeds remains the latest-per-deed projection.';
COMMENT ON TABLE character_live_damage_trace_ticks_v1 IS
    'Prunable normalized live ten-second trace roots bound to one danger checkpoint and restore point.';
COMMENT ON FUNCTION enforce_live_damage_trace_graph_v1() IS
    'Closes live trace tick counts, ordered statuses, the 300-tick/4096-entry window, and terminal ordering.';
COMMENT ON FUNCTION enforce_death_has_no_live_damage_trace_v1() IS
    'Requires terminal death to remove every pre-existing live trace row in the same transaction.';
