-- GB-M03-02D / GB-M03-06A durable-death custody and oath/bargain closure.
--
-- Authorities: GDD DTH-001/DTH-020 and TECH-020..023, Content CONT-HUB-002 and
-- CONT-ECHO-009, Roadmap GB-M03-02/06/13, and accepted SPEC-CONFLICT-009.
-- Published migrations 0037 and 0038 remain immutable. Normal lethal routes are still blocked,
-- so this forward correction refuses to reinterpret any existing death-terminal authority.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM death_events LIMIT 1)
        OR EXISTS (SELECT 1 FROM echo_records LIMIT 1)
        OR EXISTS (
            SELECT 1 FROM character_entry_restore_points
            WHERE restore_state = 2 LIMIT 1
        )
        OR EXISTS (
            SELECT 1 FROM character_run_material_stacks
            WHERE terminal_reason = 'permadeath' LIMIT 1
        )
        OR EXISTS (
            SELECT 1 FROM item_instances
            WHERE destruction_reason = 'permadeath' LIMIT 1
        )
        OR EXISTS (
            SELECT 1 FROM item_ledger_events
            WHERE reason = 'permadeath' LIMIT 1
        )
        OR EXISTS (
            SELECT 1 FROM character_life_outbox
            WHERE event_type = 'bargains_cleared_death' LIMIT 1
        )
    THEN
        RAISE EXCEPTION
            '0039 requires no death/Echo rows, death-terminal roots, or permadeath custody; clear the wipeable Core namespace';
    END IF;
END
$$;

-- The terminal root now owns the exact oath/bargain state advance and cleanup outbox event.
-- Both references are deferred because the single transaction may stage its final state, outbox,
-- and death root in any order, but may not commit a partial closure.
ALTER TABLE character_oath_bargain_state
    ADD CONSTRAINT oath_bargain_state_version_identity UNIQUE (
        namespace_id, account_id, character_id, oath_bargain_version
    );

ALTER TABLE character_life_outbox
    ADD CONSTRAINT life_outbox_character_event_version_identity UNIQUE (
        namespace_id, account_id, character_id, event_id, aggregate_version
    );

ALTER TABLE death_events
    ADD COLUMN pre_oath_bargain_version BIGINT NOT NULL,
    ADD COLUMN post_oath_bargain_version BIGINT NOT NULL,
    ADD COLUMN bargain_cleanup_event_id BYTEA NOT NULL,
    ADD CONSTRAINT death_oath_bargain_versions_exact CHECK (
        pre_oath_bargain_version > 0
        AND post_oath_bargain_version = pre_oath_bargain_version + 1
    ),
    ADD CONSTRAINT death_bargain_cleanup_event_id_exact CHECK (
        octet_length(bargain_cleanup_event_id) = 16
        AND bargain_cleanup_event_id <> decode(repeat('00', 16), 'hex')
    ),
    ADD CONSTRAINT death_ledger_terminal_identity UNIQUE (
        namespace_id, account_id, character_id, mutation_id, death_id
    ),
    ADD CONSTRAINT death_oath_bargain_state_owned FOREIGN KEY (
        namespace_id, account_id, character_id, post_oath_bargain_version
    ) REFERENCES character_oath_bargain_state (
        namespace_id, account_id, character_id, oath_bargain_version
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT death_bargain_cleanup_outbox_owned FOREIGN KEY (
        namespace_id, account_id, character_id,
        bargain_cleanup_event_id, post_oath_bargain_version
    ) REFERENCES character_life_outbox (
        namespace_id, account_id, character_id, event_id, aggregate_version
    ) DEFERRABLE INITIALLY DEFERRED;

-- A death may destroy the full bounded item graph, so the Lost projection ordinal must match the
-- destruction ledger's 0..4095 range.
ALTER TABLE death_summary_projection_entries
    DROP CONSTRAINT death_summary_projection_ordinal_bounded,
    ADD CONSTRAINT death_summary_projection_ordinal_bounded CHECK (
        entry_ordinal BETWEEN 0 AND 4095
    );

-- Terminal item custody is bidirectional: every permadeath item and ledger row names its exact
-- owner death. Ground expiry and crash/consumption outcomes retain null terminal-death authority.
ALTER TABLE item_instances
    ADD COLUMN terminal_death_id BYTEA,
    DROP CONSTRAINT item_location_shape,
    ADD CONSTRAINT item_location_shape CHECK (
        (location_kind = 0 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 3 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL
            AND security_state IN (0, 1) AND item_kind = 0)
        OR (location_kind = 1 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 1 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL
            AND security_state IN (0, 1) AND item_kind = 1)
        OR (location_kind = 2 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND security_state = 2)
        OR (location_kind = 3 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NOT NULL AND octet_length(instance_id) = 16
            AND instance_id <> decode(repeat('00', 16), 'hex')
            AND pickup_id IS NOT NULL AND octet_length(pickup_id) = 16
            AND pickup_id <> decode(repeat('00', 16), 'hex')
            AND expires_at_tick > 0 AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND security_state = 2)
        OR (location_kind = 4 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NOT NULL AND security_state = 3
            AND (
                (destruction_reason = 'permadeath'
                    AND terminal_death_id IS NOT NULL
                    AND octet_length(terminal_death_id) = 16
                    AND terminal_death_id <> decode(repeat('00', 16), 'hex'))
                OR (destruction_reason IN ('ground_expired', 'crash_revoked')
                    AND terminal_death_id IS NULL)
            ))
        OR (location_kind = 5 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND security_state = 0)
        OR (location_kind = 6 AND character_id IS NULL
            AND slot_index BETWEEN 0 AND 159 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND security_state = 0)
        OR (location_kind = 7 AND character_id IS NOT NULL AND item_kind = 1
            AND slot_index BETWEEN 0 AND 1 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason = 'consumed'
            AND terminal_death_id IS NULL AND security_state = 4)
    ),
    ADD CONSTRAINT item_terminal_death_owned FOREIGN KEY (
        namespace_id, account_id, character_id, terminal_death_id
    ) REFERENCES death_events (
        namespace_id, account_id, character_id, death_id
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE item_ledger_events
    ADD COLUMN terminal_death_id BYTEA,
    DROP CONSTRAINT ledger_creation_shape,
    ADD CONSTRAINT ledger_creation_shape CHECK (
        (event_kind = 0 AND pre_item_version = 0
            AND pre_security_state IS NULL AND pre_location_kind IS NULL
            AND reason IS NULL AND terminal_death_id IS NULL)
        OR (event_kind = 1 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND reason IS NULL AND terminal_death_id IS NULL)
        OR (event_kind = 2 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND post_security_state = 3 AND post_location_kind = 4
            AND reason IS NOT NULL
            AND (
                (reason = 'permadeath'
                    AND terminal_death_id IS NOT NULL
                    AND octet_length(terminal_death_id) = 16
                    AND terminal_death_id <> decode(repeat('00', 16), 'hex'))
                OR (reason = 'ground_expired' AND terminal_death_id IS NULL)
            ))
        OR (event_kind = 3 AND pre_item_version > 0 AND source_kind <> 4
            AND pre_security_state = 1 AND pre_location_kind = 1
            AND post_security_state = 4 AND post_location_kind = 7
            AND reason IS NOT NULL AND reason = 'consumed'
            AND terminal_death_id IS NULL)
        OR (event_kind = 4 AND pre_item_version > 0 AND source_kind = 4
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND reason IS NOT NULL AND terminal_death_id IS NULL
            AND (
                (reason = 'crash_restored'
                    AND ((post_security_state = 0 AND post_location_kind IN (0, 1))
                        OR (post_security_state = 2 AND post_location_kind = 2)))
                OR (reason = 'crash_revoked'
                    AND post_security_state = 3 AND post_location_kind = 4)
            ))
    ),
    ADD CONSTRAINT item_ledger_terminal_death_owned FOREIGN KEY (
        namespace_id, account_id, character_id, mutation_id, terminal_death_id
    ) REFERENCES death_events (
        namespace_id, account_id, character_id, mutation_id, death_id
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;

-- Replace the deferred root closure without changing its trigger identity. Lost may be empty only
-- when the destruction graph is empty; Preserved and Created remain mandatory exact projections.
CREATE OR REPLACE FUNCTION enforce_complete_death_graph_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT;
    target_death_id BYTEA;
    death death_events%ROWTYPE;
    trace_count INTEGER;
    summary_damage_count INTEGER;
    destruction_count INTEGER;
    expected_destruction_count INTEGER;
    echo_count INTEGER;
    target_echo_id BYTEA;
    target_echo_state SMALLINT;
    transition_count INTEGER;
    computed_echo_expected BOOLEAN;
    computed_echo_power_band SMALLINT;
    available_count INTEGER;
    promoted_echo_id BYTEA;
    promoted_echo_created_at TIMESTAMPTZ;
    promoted_transition_ordinal SMALLINT;
BEGIN
    IF TG_TABLE_NAME = 'death_events' THEN
        target_namespace := COALESCE(NEW.namespace_id, OLD.namespace_id);
        target_death_id := COALESCE(NEW.death_id, OLD.death_id);
    ELSIF TG_TABLE_NAME IN (
        'death_combat_trace_entries', 'death_combat_trace_statuses',
        'death_summary_snapshots', 'death_summary_bargains',
        'death_summary_damage_entries', 'death_summary_projection_entries',
        'memorial_records', 'death_destruction_entries', 'death_outbox_events'
    ) THEN
        target_namespace := COALESCE(NEW.namespace_id, OLD.namespace_id);
        target_death_id := COALESCE(NEW.death_id, OLD.death_id);
    ELSIF TG_TABLE_NAME = 'death_mutation_results' THEN
        target_namespace := COALESCE(NEW.namespace_id, OLD.namespace_id);
        target_death_id := COALESCE(NEW.death_id, OLD.death_id);
    ELSIF TG_TABLE_NAME = 'death_audit_events' THEN
        target_namespace := COALESCE(NEW.namespace_id, OLD.namespace_id);
        target_death_id := COALESCE(NEW.death_id, OLD.death_id);
        IF target_death_id IS NULL THEN RETURN NULL; END IF;
    ELSIF TG_TABLE_NAME = 'echo_records' THEN
        target_namespace := COALESCE(NEW.namespace_id, OLD.namespace_id);
        target_death_id := COALESCE(NEW.death_id, OLD.death_id);
    ELSIF TG_TABLE_NAME = 'echo_state_transitions' THEN
        target_namespace := COALESCE(NEW.namespace_id, OLD.namespace_id);
        SELECT death_id INTO target_death_id
        FROM echo_records
        WHERE namespace_id = target_namespace
          AND echo_id = COALESCE(NEW.echo_id, OLD.echo_id);
        IF NOT FOUND THEN RETURN NULL; END IF;
    ELSE
        RAISE EXCEPTION 'complete death graph trigger attached to unsupported relation';
    END IF;

    SELECT * INTO death
    FROM death_events
    WHERE namespace_id = target_namespace AND death_id = target_death_id;
    IF NOT FOUND THEN RETURN NULL; END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM character_entry_restore_points AS root
        JOIN character_instance_lineages AS lineage
          ON lineage.namespace_id = root.namespace_id
         AND lineage.account_id = root.account_id
         AND lineage.character_id = root.character_id
         AND lineage.lineage_id = root.lineage_id
        WHERE root.namespace_id = death.namespace_id
          AND root.account_id = death.account_id
          AND root.character_id = death.character_id
          AND root.restore_point_id = death.restore_point_id
          AND root.lineage_id = death.lineage_id
          AND root.records_blake3 = death.world_records_blake3
          AND root.assets_blake3 = death.world_assets_blake3
          AND root.localization_blake3 = death.world_localization_blake3
          AND root.restore_state = 2
          AND root.death_mutation_id = death.mutation_id
          AND root.consumed_at = death.committed_at
          AND lineage.records_blake3 = death.world_records_blake3
          AND lineage.assets_blake3 = death.world_assets_blake3
          AND lineage.localization_blake3 = death.world_localization_blake3
          AND lineage.lineage_state = 3
          AND lineage.closed_at = death.committed_at
    ) THEN
        RAISE EXCEPTION 'death is not bound to the exact committed terminal danger root';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM accounts AS account
        JOIN characters AS character
          ON character.namespace_id = account.namespace_id
         AND character.account_id = account.account_id
         AND character.character_id = death.character_id
        JOIN character_world_locations AS world
          ON world.namespace_id = character.namespace_id
         AND world.account_id = character.account_id
         AND world.character_id = character.character_id
        JOIN character_progression AS progression
          ON progression.namespace_id = character.namespace_id
         AND progression.account_id = character.account_id
         AND progression.character_id = character.character_id
        JOIN character_inventories AS inventory
          ON inventory.namespace_id = character.namespace_id
         AND inventory.account_id = character.account_id
         AND inventory.character_id = character.character_id
        JOIN character_life_metrics AS life
          ON life.namespace_id = character.namespace_id
         AND life.account_id = character.account_id
         AND life.character_id = character.character_id
        JOIN character_oath_bargain_state AS oath_bargain
          ON oath_bargain.namespace_id = character.namespace_id
         AND oath_bargain.account_id = character.account_id
         AND oath_bargain.character_id = character.character_id
        JOIN death_summary_snapshots AS summary
          ON summary.namespace_id = death.namespace_id
         AND summary.death_id = death.death_id
        WHERE account.namespace_id = death.namespace_id
          AND account.account_id = death.account_id
          AND account.state_version = death.post_account_version
          AND account.selected_character_id IS NULL
          AND character.life_state = 1
          AND character.roster_ordinal IS NULL
          AND NOT EXISTS (
              SELECT 1 FROM characters AS active_roster
              WHERE active_roster.namespace_id = death.namespace_id
                AND active_roster.account_id = death.account_id
                AND active_roster.life_state = 0
                AND active_roster.roster_ordinal = death.former_roster_ordinal
          )
          AND character.character_state_version = death.post_character_version
          AND world.character_version = death.post_character_version
          AND progression.current_health = 0
          AND progression.progression_version = death.post_progression_version
          AND summary.class_id = character.class_id
          AND summary.level = progression.level
          AND summary.level = character.level
          AND summary.oath_id IS NOT DISTINCT FROM character.oath_id
          AND inventory.inventory_version = death.post_inventory_version
          AND life.life_metrics_version = death.post_life_metrics_version
          AND life.lifetime_ticks = death.lifetime_ticks
          AND life.permadeath_combat_ticks = death.permadeath_combat_ticks
          AND oath_bargain.oath_bargain_version = death.post_oath_bargain_version
          AND NOT EXISTS (
              SELECT 1 FROM character_active_bargains AS active_bargain
              WHERE active_bargain.namespace_id = death.namespace_id
                AND active_bargain.account_id = death.account_id
                AND active_bargain.character_id = death.character_id
          )
    ) THEN
        RAISE EXCEPTION 'death post versions or terminal aggregate state are not authoritative';
    END IF;

    IF (SELECT count(*) FROM death_summary_snapshots
        WHERE namespace_id = death.namespace_id AND death_id = death.death_id
          AND content_revision = death.content_revision
          AND lifetime_ms = floor(death.lifetime_ticks::numeric * 1000 / 30)::bigint) <> 1
        OR (SELECT count(*) FROM memorial_records
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND account_id = death.account_id AND death_at = death.committed_at) <> 1
        OR (SELECT count(*) FROM death_mutation_results
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND account_id = death.account_id AND character_id = death.character_id
              AND mutation_id = death.mutation_id
              AND contract_kind = death.contract_kind
              AND canonical_request_hash = death.canonical_request_hash
              AND committed_at = death.committed_at) <> 1
        OR (SELECT count(*) FROM death_audit_events
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND account_id = death.account_id AND character_id = death.character_id
              AND mutation_id = death.mutation_id AND event_kind = 0
              AND created_at = death.committed_at) <> 1
        OR (SELECT count(*) FROM death_outbox_events
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND event_type = 'death_committed' AND echo_id IS NULL
              AND created_at = death.committed_at) <> 1
        OR (SELECT count(*) FROM character_life_outbox
            WHERE namespace_id = death.namespace_id
              AND account_id = death.account_id
              AND character_id = death.character_id
              AND event_id = death.bargain_cleanup_event_id
              AND event_type = 'bargains_cleared_death'
              AND aggregate_version = death.post_oath_bargain_version
              AND created_at = death.committed_at) <> 1
    THEN
        RAISE EXCEPTION 'accepted death is missing its summary, memorial, receipt, audit, or outbox';
    END IF;

    SELECT count(*) INTO trace_count
    FROM death_combat_trace_entries
    WHERE namespace_id = death.namespace_id AND death_id = death.death_id;
    IF trace_count < 1 OR trace_count > 4096
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT trace_ordinal,
                    row_number() OVER (ORDER BY event_tick, event_ordinal) - 1 AS expected_ordinal
                FROM death_combat_trace_entries
                WHERE namespace_id = death.namespace_id AND death_id = death.death_id
            ) AS ordered
            WHERE trace_ordinal <> expected_ordinal
        )
        OR EXISTS (
            SELECT 1 FROM death_combat_trace_entries
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND (event_tick > death.death_tick
                   OR event_tick < GREATEST(1, death.death_tick - 300))
        )
        OR (SELECT count(*) FROM death_combat_trace_entries
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND lethal) <> 1
        OR NOT EXISTS (
            SELECT 1 FROM death_combat_trace_entries AS lethal
            WHERE lethal.namespace_id = death.namespace_id
              AND lethal.death_id = death.death_id
              AND lethal.trace_ordinal = trace_count - 1
              AND lethal.lethal
              AND lethal.event_tick = death.death_tick
              AND lethal.source_content_id IS NOT DISTINCT FROM death.killer_content_id
              AND lethal.pattern_id IS NOT DISTINCT FROM death.killer_pattern_id
              AND lethal.attack_id IS NOT DISTINCT FROM death.killer_attack_id
              AND lethal.raw_damage = death.raw_damage
              AND lethal.final_damage = death.final_damage
              AND lethal.damage_type = death.damage_type
              AND lethal.pre_health = death.pre_hit_health
              AND lethal.post_health = 0
              AND lethal.source_x_milli_tiles = death.source_x_milli_tiles
              AND lethal.source_y_milli_tiles = death.source_y_milli_tiles
              AND lethal.network_state = death.network_state
              AND lethal.recall_state = death.recall_state
        )
        OR EXISTS (
            SELECT 1
            FROM death_combat_trace_entries AS trace
            WHERE trace.namespace_id = death.namespace_id AND trace.death_id = death.death_id
              AND EXISTS (
                  SELECT 1
                  FROM (
                      SELECT status_ordinal,
                          row_number() OVER (ORDER BY status_ordinal) - 1 AS expected_ordinal
                      FROM death_combat_trace_statuses AS status
                      WHERE status.namespace_id = trace.namespace_id
                        AND status.death_id = trace.death_id
                        AND status.trace_ordinal = trace.trace_ordinal
                  ) AS ordered_status
                  WHERE ordered_status.status_ordinal <> ordered_status.expected_ordinal
              )
        )
    THEN
        RAISE EXCEPTION 'death combat trace is incomplete or noncanonical';
    END IF;

    SELECT count(*) INTO summary_damage_count
    FROM death_summary_damage_entries
    WHERE namespace_id = death.namespace_id AND death_id = death.death_id;
    IF summary_damage_count <> LEAST(5, trace_count)
        OR EXISTS (
            SELECT 1
            FROM death_summary_damage_entries
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND (summary_ordinal <> trace_ordinal - (trace_count - summary_damage_count))
        )
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT bargain_ordinal,
                    row_number() OVER (ORDER BY bargain_ordinal) - 1 AS expected_ordinal
                FROM death_summary_bargains
                WHERE namespace_id = death.namespace_id AND death_id = death.death_id
            ) AS bargains
            WHERE bargain_ordinal <> expected_ordinal
        )
        OR EXISTS (
            SELECT 1
            FROM generate_series(0, 2) AS section(section_kind)
            WHERE (
                section.section_kind IN (1, 2)
                AND NOT EXISTS (
                    SELECT 1 FROM death_summary_projection_entries AS projection
                    WHERE projection.namespace_id = death.namespace_id
                      AND projection.death_id = death.death_id
                      AND projection.section_kind = section.section_kind
                )
            )
            OR EXISTS (
                SELECT 1
                FROM (
                    SELECT entry_ordinal,
                        row_number() OVER (ORDER BY entry_ordinal) - 1 AS expected_ordinal
                    FROM death_summary_projection_entries AS projection
                    WHERE projection.namespace_id = death.namespace_id
                      AND projection.death_id = death.death_id
                      AND projection.section_kind = section.section_kind
                ) AS ordered_projection
                WHERE ordered_projection.entry_ordinal <> ordered_projection.expected_ordinal
            )
        )
        OR (SELECT count(*) FROM death_summary_projection_entries
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND section_kind = 0) <> (
            SELECT count(*) FROM death_destruction_entries
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
        )
        OR EXISTS (
            SELECT 1
            FROM death_destruction_entries AS destroyed
            LEFT JOIN death_summary_projection_entries AS projection
              ON projection.namespace_id = destroyed.namespace_id
             AND projection.death_id = destroyed.death_id
             AND projection.section_kind = 0
             AND projection.entry_ordinal = destroyed.destruction_ordinal
            LEFT JOIN item_instances AS item
              ON item.namespace_id = destroyed.namespace_id
             AND item.item_uid = destroyed.item_uid
            WHERE destroyed.namespace_id = death.namespace_id
              AND destroyed.death_id = death.death_id
              AND (
                  projection.entry_ordinal IS NULL
                  OR (destroyed.entry_kind = 0 AND NOT (
                      projection.projection_kind = 0
                      AND projection.content_id = item.template_id
                      AND projection.quantity = 1
                      AND projection.item_uid = destroyed.item_uid
                  ))
                  OR (destroyed.entry_kind = 1 AND NOT (
                      projection.projection_kind = 1
                      AND projection.content_id = destroyed.material_id
                      AND projection.quantity = destroyed.quantity
                      AND projection.item_uid IS NULL
                  ))
              )
        )
        OR (SELECT count(*) FROM death_summary_projection_entries
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND section_kind = 1) <> 5
        OR EXISTS (
            SELECT 1
            FROM (VALUES
                (0, 2, 'projection.preserved.account_records'),
                (1, 3, 'projection.preserved.currency'),
                (2, 4, 'projection.preserved.vault'),
                (3, 5, 'projection.preserved.cosmetics'),
                (4, 6, 'projection.preserved.recipes')
            ) AS expected(entry_ordinal, projection_kind, content_id)
            WHERE NOT EXISTS (
                SELECT 1 FROM death_summary_projection_entries AS projection
                WHERE projection.namespace_id = death.namespace_id
                  AND projection.death_id = death.death_id
                  AND projection.section_kind = 1
                  AND projection.entry_ordinal = expected.entry_ordinal
                  AND projection.projection_kind = expected.projection_kind
                  AND projection.content_id = expected.content_id
                  AND projection.quantity = 1
                  AND projection.item_uid IS NULL
            )
        )
        OR (SELECT count(*) FROM death_summary_projection_entries
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND section_kind = 2) <> 2
        OR EXISTS (
            SELECT 1
            FROM (VALUES
                (0, 7, 'projection.created.memorial'),
                (1, 8, 'projection.created.echo')
            ) AS expected(entry_ordinal, projection_kind, content_id)
            WHERE NOT EXISTS (
                SELECT 1 FROM death_summary_projection_entries AS projection
                WHERE projection.namespace_id = death.namespace_id
                  AND projection.death_id = death.death_id
                  AND projection.section_kind = 2
                  AND projection.entry_ordinal = expected.entry_ordinal
                  AND projection.projection_kind = expected.projection_kind
                  AND projection.content_id = expected.content_id
                  AND projection.quantity = 1
                  AND projection.item_uid IS NULL
            )
        )
    THEN
        RAISE EXCEPTION 'death summary children are incomplete or noncanonical';
    END IF;

    SELECT count(*) INTO destruction_count
    FROM death_destruction_entries
    WHERE namespace_id = death.namespace_id AND death_id = death.death_id;
    SELECT
        (SELECT count(*) FROM item_instances
         WHERE namespace_id = death.namespace_id
           AND account_id = death.account_id AND character_id = death.character_id
           AND location_kind = 4 AND security_state = 3
           AND destruction_reason = 'permadeath'
           AND terminal_death_id = death.death_id)
        +
        (SELECT count(*) FROM character_run_material_stacks
         WHERE namespace_id = death.namespace_id
           AND account_id = death.account_id AND character_id = death.character_id
           AND security_state = 3 AND quantity = 0 AND terminal_reason = 'permadeath'
           AND terminal_death_id = death.death_id)
    INTO expected_destruction_count;
    IF destruction_count <> expected_destruction_count
        OR (SELECT count(*) FROM item_ledger_events
            WHERE namespace_id = death.namespace_id
              AND account_id = death.account_id AND character_id = death.character_id
              AND mutation_id = death.mutation_id AND event_kind = 2
              AND source_kind = 3 AND reason = 'permadeath'
              AND terminal_death_id = death.death_id) <> (
            SELECT count(*) FROM item_instances
            WHERE namespace_id = death.namespace_id
              AND account_id = death.account_id AND character_id = death.character_id
              AND location_kind = 4 AND security_state = 3
              AND destruction_reason = 'permadeath'
              AND terminal_death_id = death.death_id
        )
        OR EXISTS (
            SELECT 1 FROM item_instances
            WHERE namespace_id = death.namespace_id
              AND account_id = death.account_id AND character_id = death.character_id
              AND security_state IN (1, 2)
        )
        OR EXISTS (
            SELECT 1 FROM character_run_material_stacks
            WHERE namespace_id = death.namespace_id
              AND account_id = death.account_id AND character_id = death.character_id
              AND security_state = 2 AND quantity > 0
        )
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT destruction_ordinal,
                    row_number() OVER (
                        ORDER BY
                            CASE WHEN entry_kind = 0 THEN pre_location_kind ELSE 4 END,
                            CASE WHEN entry_kind = 0 AND pre_location_kind < 3
                                 THEN pre_slot_index END,
                            CASE WHEN entry_kind = 0 AND pre_location_kind = 3
                                 THEN encode(pre_instance_id, 'hex') END COLLATE "C",
                            CASE WHEN entry_kind = 0 AND pre_location_kind = 3
                                 THEN encode(pre_pickup_id, 'hex') END COLLATE "C",
                            CASE WHEN entry_kind = 0 THEN encode(item_uid, 'hex') END COLLATE "C",
                            CASE WHEN entry_kind = 1 THEN material_id END COLLATE "C"
                    ) - 1 AS expected_ordinal
                FROM death_destruction_entries
                WHERE namespace_id = death.namespace_id AND death_id = death.death_id
            ) AS ordered_destruction
            WHERE destruction_ordinal <> expected_ordinal
        )
    THEN
        RAISE EXCEPTION 'death destruction graph is incomplete or noncanonical';
    END IF;

    SELECT summary.level = 10
        AND death.permadeath_combat_ticks >= 18000
        AND (
            (SELECT count(*) FROM character_life_deeds AS deed
             WHERE deed.namespace_id = death.namespace_id
               AND deed.account_id = death.account_id
               AND deed.character_id = death.character_id
               AND deed.deed_kind = 0
               AND deed.achieved_tick <= death.death_tick
               AND deed.content_revision = death.content_revision) >= 1
            OR
            (SELECT count(DISTINCT deed.deed_id) FROM character_life_deeds AS deed
             WHERE deed.namespace_id = death.namespace_id
               AND deed.account_id = death.account_id
               AND deed.character_id = death.character_id
               AND deed.deed_kind = 1
               AND deed.achieved_tick <= death.death_tick
               AND deed.content_revision = death.content_revision) >= 2
        )
    INTO computed_echo_expected
    FROM death_summary_snapshots AS summary
    WHERE summary.namespace_id = death.namespace_id AND summary.death_id = death.death_id;
    IF computed_echo_expected IS DISTINCT FROM death.echo_expected THEN
        RAISE EXCEPTION 'death Echo eligibility does not match level, combat time, and deeds';
    END IF;

    SELECT count(*) INTO echo_count
    FROM echo_records
    WHERE namespace_id = death.namespace_id AND death_id = death.death_id;
    IF (death.echo_expected AND echo_count <> 1)
        OR (NOT death.echo_expected AND echo_count <> 0) THEN
        RAISE EXCEPTION 'death Echo presence does not match the authoritative projector outcome';
    END IF;
    IF NOT death.echo_expected AND NOT EXISTS (
        SELECT 1 FROM death_summary_snapshots
        WHERE namespace_id = death.namespace_id AND death_id = death.death_id
          AND echo_outcome = 0
    ) THEN
        RAISE EXCEPTION 'ineligible death summary has an impossible Echo outcome';
    END IF;
    IF death.echo_expected THEN
        SELECT CASE
            WHEN power_index < 90 THEN 1
            WHEN power_index < 120 THEN 2
            WHEN power_index < 150 THEN 3
            WHEN power_index < 180 THEN 4
            ELSE 5
        END INTO computed_echo_power_band
        FROM (
            SELECT (summary.level * 10 + (
                35 * COALESCE(max(
                    item.item_level * 10 + CASE item.rarity
                        WHEN 0 THEN 0 WHEN 1 THEN 5 WHEN 2 THEN 10
                        WHEN 3 THEN 20 WHEN 4 THEN 30 END
                ) FILTER (WHERE destroyed.pre_slot_index = 0), 0)
                + 25 * COALESCE(max(
                    item.item_level * 10 + CASE item.rarity
                        WHEN 0 THEN 0 WHEN 1 THEN 5 WHEN 2 THEN 10
                        WHEN 3 THEN 20 WHEN 4 THEN 30 END
                ) FILTER (WHERE destroyed.pre_slot_index = 1), 0)
                + 25 * COALESCE(max(
                    item.item_level * 10 + CASE item.rarity
                        WHEN 0 THEN 0 WHEN 1 THEN 5 WHEN 2 THEN 10
                        WHEN 3 THEN 20 WHEN 4 THEN 30 END
                ) FILTER (WHERE destroyed.pre_slot_index = 2), 0)
                + 15 * COALESCE(max(
                    item.item_level * 10 + CASE item.rarity
                        WHEN 0 THEN 0 WHEN 1 THEN 5 WHEN 2 THEN 10
                        WHEN 3 THEN 20 WHEN 4 THEN 30 END
                ) FILTER (WHERE destroyed.pre_slot_index = 3), 0)
                + 50
            ) / 100 + 1) / 2 AS power_index
            FROM death_summary_snapshots AS summary
            LEFT JOIN death_destruction_entries AS destroyed
              ON destroyed.namespace_id = summary.namespace_id
             AND destroyed.death_id = summary.death_id
             AND destroyed.entry_kind = 0
             AND destroyed.pre_location_kind = 0
            LEFT JOIN item_instances AS item
              ON item.namespace_id = destroyed.namespace_id
             AND item.item_uid = destroyed.item_uid
            WHERE summary.namespace_id = death.namespace_id
              AND summary.death_id = death.death_id
            GROUP BY summary.level
        ) AS computed;
        SELECT echo_id, state INTO target_echo_id, target_echo_state
        FROM echo_records
        WHERE namespace_id = death.namespace_id AND death_id = death.death_id
          AND account_id = death.account_id
          AND content_revision = death.content_revision
          AND created_at = death.committed_at;
        IF NOT FOUND OR target_echo_state NOT IN (0, 1) THEN
            RAISE EXCEPTION 'new death Echo snapshot is not canonical';
        END IF;
        IF NOT EXISTS (
            SELECT 1
            FROM echo_records AS echo
            JOIN death_summary_snapshots AS summary
              ON summary.namespace_id = echo.namespace_id
             AND summary.death_id = echo.death_id
            WHERE echo.namespace_id = death.namespace_id
              AND echo.echo_id = target_echo_id
              AND echo.account_id = death.account_id
              AND echo.character_name_snapshot = summary.character_name_snapshot
              AND echo.class_id = summary.class_id
              AND echo.oath_id IS NOT DISTINCT FROM summary.oath_id
              AND echo.level = summary.level
              AND echo.appearance_snapshot_id = 'appearance.default.grave_arbalist'
              AND echo.appearance_theme_id = 'theme.echo.arbalist_ash'
              AND echo.power_band = computed_echo_power_band
              AND echo.killer_content_id IS NOT DISTINCT FROM death.killer_content_id
              AND echo.killer_pattern_id IS NOT DISTINCT FROM death.killer_pattern_id
              AND echo.death_region_id = death.region_id
        ) THEN
            RAISE EXCEPTION 'death Echo snapshot disagrees with its death summary or cause';
        END IF;
        IF NOT EXISTS (
            SELECT 1 FROM death_summary_snapshots
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id
              AND echo_outcome = CASE WHEN target_echo_state = 0 THEN 1 ELSE 2 END
        ) THEN
            RAISE EXCEPTION 'eligible death summary has an impossible Echo outcome';
        END IF;
        SELECT count(*) INTO transition_count
        FROM echo_state_transitions
        WHERE namespace_id = death.namespace_id AND echo_id = target_echo_id;
        IF transition_count <> (CASE WHEN target_echo_state = 0 THEN 1 ELSE 2 END)
            OR EXISTS (
                SELECT 1
                FROM (
                    SELECT transition_ordinal, previous_state, next_state,
                        lag(next_state) OVER (ORDER BY transition_ordinal) AS prior_next,
                        row_number() OVER (ORDER BY transition_ordinal) - 1 AS expected_ordinal
                    FROM echo_state_transitions
                    WHERE namespace_id = death.namespace_id AND echo_id = target_echo_id
                ) AS transition
                WHERE transition.transition_ordinal <> transition.expected_ordinal
                   OR (transition.transition_ordinal = 0
                       AND (transition.previous_state IS NOT NULL OR transition.next_state <> 0))
                   OR (transition.transition_ordinal > 0
                       AND transition.previous_state IS DISTINCT FROM transition.prior_next)
            )
            OR NOT EXISTS (
                SELECT 1 FROM echo_state_transitions
                WHERE namespace_id = death.namespace_id AND echo_id = target_echo_id
                  AND transition_ordinal = transition_count - 1
                  AND next_state = target_echo_state
            )
            OR NOT EXISTS (
                SELECT 1 FROM echo_state_transitions
                WHERE namespace_id = death.namespace_id AND echo_id = target_echo_id
                  AND transition_ordinal = 0 AND previous_state IS NULL AND next_state = 0
                  AND reason_kind = 0
                  AND source_death_id = death.death_id
                  AND trigger_death_id = death.death_id
                  AND committed_at = death.committed_at
            )
            OR (target_echo_state = 1 AND NOT EXISTS (
                SELECT 1 FROM echo_state_transitions
                WHERE namespace_id = death.namespace_id AND echo_id = target_echo_id
                  AND transition_ordinal = 1 AND previous_state = 0 AND next_state = 1
                  AND reason_kind = 1
                  AND trigger_death_id = death.death_id
                  AND committed_at = death.committed_at
            ))
            OR NOT EXISTS (
                SELECT 1 FROM echo_deed_tags
                WHERE namespace_id = death.namespace_id AND echo_id = target_echo_id
            )
            OR EXISTS (
                SELECT 1
                FROM (
                    SELECT deed_ordinal,
                        row_number() OVER (ORDER BY deed_ordinal) - 1 AS expected_ordinal
                    FROM echo_deed_tags
                    WHERE namespace_id = death.namespace_id AND echo_id = target_echo_id
                ) AS deed
                WHERE deed.deed_ordinal <> deed.expected_ordinal
            )
            OR EXISTS (
                SELECT 1
                FROM (
                    SELECT bargain_ordinal,
                        row_number() OVER (ORDER BY bargain_ordinal) - 1 AS expected_ordinal
                    FROM echo_bargain_snapshots
                    WHERE namespace_id = death.namespace_id AND echo_id = target_echo_id
                ) AS bargain
                WHERE bargain.bargain_ordinal <> bargain.expected_ordinal
            )
            OR (SELECT count(*) FROM death_outbox_events
                WHERE namespace_id = death.namespace_id AND death_id = death.death_id
                  AND echo_id = target_echo_id AND event_type = 'echo_created'
                  AND echo_transition_ordinal = 0
                  AND trigger_death_id = death.death_id
                  AND created_at = death.committed_at) <> 1
            OR ((target_echo_state = 1) <> ((SELECT count(*) FROM death_outbox_events
                WHERE namespace_id = death.namespace_id AND death_id = death.death_id
                  AND echo_id = target_echo_id AND event_type = 'echo_promoted'
                  AND echo_transition_ordinal = 1
                  AND trigger_death_id = death.death_id
                  AND created_at = death.committed_at) = 1))
        THEN
            RAISE EXCEPTION 'death Echo history or outbox is incomplete';
        END IF;

        SELECT count(*) INTO available_count
        FROM echo_records
        WHERE namespace_id = death.namespace_id
          AND account_id = death.account_id AND state = 1;
        SELECT count(*) INTO transition_count
        FROM echo_state_transitions AS transition
        JOIN echo_records AS echo
          ON echo.namespace_id = transition.namespace_id
         AND echo.echo_id = transition.echo_id
        WHERE echo.namespace_id = death.namespace_id
          AND echo.account_id = death.account_id
          AND transition.trigger_death_id = death.death_id;
        IF transition_count <> (CASE
            WHEN death.preexisting_available_echo_id IS NOT NULL THEN 1 ELSE 2 END)
        THEN
            RAISE EXCEPTION 'death Echo projector transition set is not exact';
        END IF;
        IF death.preexisting_available_echo_id IS NOT NULL THEN
            IF available_count <> 1 OR target_echo_state <> 0
                OR NOT EXISTS (
                    SELECT 1 FROM echo_records AS available
                    WHERE available.namespace_id = death.namespace_id
                      AND available.account_id = death.account_id
                      AND available.echo_id = death.preexisting_available_echo_id
                      AND available.state = 1
                )
                OR EXISTS (
                    SELECT 1
                    FROM echo_state_transitions AS transition
                    JOIN echo_records AS echo
                      ON echo.namespace_id = transition.namespace_id
                     AND echo.echo_id = transition.echo_id
                    WHERE echo.namespace_id = death.namespace_id
                      AND echo.account_id = death.account_id
                      AND transition.previous_state = 0 AND transition.next_state = 1
                      AND transition.reason_kind = 1
                      AND transition.trigger_death_id = death.death_id
                      AND transition.committed_at = death.committed_at
                )
                OR EXISTS (
                    SELECT 1
                    FROM echo_state_transitions AS transition
                    JOIN echo_records AS echo
                      ON echo.namespace_id = transition.namespace_id
                     AND echo.echo_id = transition.echo_id
                    WHERE echo.namespace_id = death.namespace_id
                      AND echo.account_id = death.account_id
                      AND transition.committed_at = death.committed_at
                      AND NOT (
                          transition.echo_id = target_echo_id
                          AND transition.transition_ordinal = 0
                          AND transition.previous_state IS NULL
                          AND transition.next_state = 0
                          AND transition.reason_kind = 0
                          AND transition.trigger_death_id = death.death_id
                      )
                )
            THEN
                RAISE EXCEPTION 'death Echo projector changed an account with an Available Echo';
            END IF;
        ELSIF death.promoted_echo_id IS NOT NULL THEN
            IF available_count <> 1 THEN
                RAISE EXCEPTION 'death Echo projector did not produce one Available Echo';
            END IF;
            SELECT echo.echo_id, echo.created_at, transition.transition_ordinal
            INTO promoted_echo_id, promoted_echo_created_at, promoted_transition_ordinal
            FROM echo_records AS echo
            JOIN echo_state_transitions AS transition
              ON transition.namespace_id = echo.namespace_id
             AND transition.echo_id = echo.echo_id
            WHERE echo.namespace_id = death.namespace_id
              AND echo.account_id = death.account_id
              AND echo.echo_id = death.promoted_echo_id
              AND echo.state = 1
              AND transition.previous_state = 0 AND transition.next_state = 1
              AND transition.reason_kind = 1
              AND transition.trigger_death_id = death.death_id
              AND transition.committed_at = death.committed_at
              AND transition.transition_ordinal = (
                  SELECT max(tail.transition_ordinal)
                  FROM echo_state_transitions AS tail
                  WHERE tail.namespace_id = echo.namespace_id
                    AND tail.echo_id = echo.echo_id
              );
            IF NOT FOUND
                OR EXISTS (
                    SELECT 1 FROM echo_records AS dormant
                    WHERE dormant.namespace_id = death.namespace_id
                      AND dormant.account_id = death.account_id
                      AND (
                          dormant.state = 0
                          OR EXISTS (
                              SELECT 1
                              FROM echo_state_transitions AS prior_state
                              WHERE prior_state.namespace_id = dormant.namespace_id
                                AND prior_state.echo_id = dormant.echo_id
                                AND prior_state.previous_state = 0
                                AND prior_state.committed_at = death.committed_at
                          )
                      )
                      AND (dormant.created_at, dormant.echo_id)
                          < (promoted_echo_created_at, promoted_echo_id)
                )
                OR EXISTS (
                    SELECT 1
                    FROM echo_state_transitions AS transition
                    JOIN echo_records AS echo
                      ON echo.namespace_id = transition.namespace_id
                     AND echo.echo_id = transition.echo_id
                    WHERE echo.namespace_id = death.namespace_id
                      AND echo.account_id = death.account_id
                      AND transition.committed_at = death.committed_at
                      AND NOT (
                          transition.echo_id = target_echo_id
                          AND transition.transition_ordinal = 0
                          AND transition.previous_state IS NULL
                          AND transition.next_state = 0
                          AND transition.reason_kind = 0
                          AND transition.trigger_death_id = death.death_id
                      )
                      AND NOT (
                          transition.echo_id = promoted_echo_id
                          AND transition.transition_ordinal = promoted_transition_ordinal
                          AND transition.previous_state = 0
                          AND transition.next_state = 1
                          AND transition.reason_kind = 1
                          AND transition.trigger_death_id = death.death_id
                      )
                )
                OR NOT EXISTS (
                    SELECT 1 FROM death_outbox_events AS outbox
                    JOIN echo_records AS promoted
                      ON promoted.namespace_id = outbox.namespace_id
                     AND promoted.death_id = outbox.death_id
                     AND promoted.echo_id = outbox.echo_id
                    WHERE outbox.namespace_id = death.namespace_id
                      AND outbox.echo_id = promoted_echo_id
                      AND outbox.event_type = 'echo_promoted'
                      AND outbox.echo_transition_ordinal = promoted_transition_ordinal
                      AND outbox.trigger_death_id = death.death_id
                      AND outbox.created_at = death.committed_at
                )
            THEN
                RAISE EXCEPTION 'death Echo projector did not promote the oldest Dormant Echo';
            END IF;
        ELSE
            RAISE EXCEPTION 'death Echo projector decision is incomplete';
        END IF;
    END IF;
    RETURN NULL;
END
$$;
