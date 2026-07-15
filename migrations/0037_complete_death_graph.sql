-- GB-M03-02D / GB-M03-06A / GB-M03-13 complete durable-death graph.
--
-- Authorities: GDD DTH-001/DTH-020 and TECH-020..023, Content CONT-ECHO-009 and
-- CONT-HUB-002, Roadmap GB-M03-02/06/13, and owner-approved SPEC-CONFLICT-009.
-- Published migrations 0031-0036 remain immutable. The lethal route is still disabled, so this
-- forward correction refuses to reinterpret any pre-existing death or Echo history.

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
    THEN
        RAISE EXCEPTION
            '0037 requires no death/Echo rows, death-terminal roots, or permadeath custody; clear the wipeable Core namespace';
    END IF;
END
$$;

-- Dead identities remain durable for Memorial/Echo/support history, but no longer occupy one of
-- the two playable roster slots. The final-death transaction clears the selected character and
-- archives the dead row by setting its ordinal null; successor creation can then reuse that exact
-- ordinal without deleting or rewriting the dead identity.
ALTER TABLE characters
    ALTER COLUMN roster_ordinal DROP NOT NULL,
    DROP CONSTRAINT character_roster_ordinal_core,
    ADD CONSTRAINT character_roster_life_shape CHECK (
        (life_state = 0 AND roster_ordinal BETWEEN 1 AND 2)
        OR (life_state = 1 AND roster_ordinal IS NULL)
    );

CREATE FUNCTION enforce_selected_character_live_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.selected_character_id IS NULL THEN RETURN NEW; END IF;
    IF NOT EXISTS (
        SELECT 1 FROM characters AS selected
        WHERE selected.namespace_id = NEW.namespace_id
          AND selected.account_id = NEW.account_id
          AND selected.character_id = NEW.selected_character_id
          AND selected.life_state = 0
          AND selected.roster_ordinal IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'selected character must be a living active-roster identity';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER account_selected_character_live_insert
BEFORE INSERT ON accounts
FOR EACH ROW EXECUTE FUNCTION enforce_selected_character_live_v1();
CREATE TRIGGER account_selected_character_live_update
BEFORE UPDATE OF selected_character_id ON accounts
FOR EACH ROW EXECUTE FUNCTION enforce_selected_character_live_v1();

-- A terminal danger root and its death receipt are mutually authoritative. The FK cycle is
-- deferred deliberately: the single writer may stage either row first, but cannot commit one
-- without the other. Durable terminal discriminants remain append-only: 2 is DeathCommitted.
ALTER TABLE character_entry_restore_points
    ADD COLUMN death_mutation_id BYTEA,
    ADD CONSTRAINT restore_death_result_identity_shape CHECK (
        (restore_state = 2
            AND death_mutation_id IS NOT NULL
            AND octet_length(death_mutation_id) = 16
            AND death_mutation_id <> decode(repeat('00', 16), 'hex'))
        OR (restore_state <> 2 AND death_mutation_id IS NULL)
    ),
    ADD CONSTRAINT restore_death_terminal_identity UNIQUE (
        namespace_id, account_id, character_id, restore_point_id,
        lineage_id, records_blake3, assets_blake3, localization_blake3,
        death_mutation_id
    );

ALTER TABLE death_events
    ADD COLUMN former_roster_ordinal SMALLINT NOT NULL,
    ADD COLUMN echo_expected BOOLEAN NOT NULL,
    ADD COLUMN preexisting_available_echo_id BYTEA,
    ADD COLUMN promoted_echo_id BYTEA,
    ADD COLUMN world_records_blake3 TEXT NOT NULL,
    ADD COLUMN world_assets_blake3 TEXT NOT NULL,
    ADD COLUMN world_localization_blake3 TEXT NOT NULL,
    ADD CONSTRAINT death_world_revision_exact CHECK (
        world_records_blake3 ~ '^[0-9a-f]{64}$'
        AND world_assets_blake3 ~ '^[0-9a-f]{64}$'
        AND world_localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    ADD CONSTRAINT death_former_roster_ordinal_core CHECK (
        former_roster_ordinal BETWEEN 1 AND 2
    ),
    ADD CONSTRAINT death_echo_projector_decision_shape CHECK (
        (NOT echo_expected
            AND preexisting_available_echo_id IS NULL AND promoted_echo_id IS NULL)
        OR (echo_expected
            AND preexisting_available_echo_id IS NOT NULL AND promoted_echo_id IS NULL
            AND octet_length(preexisting_available_echo_id) = 16)
        OR (echo_expected
            AND preexisting_available_echo_id IS NULL AND promoted_echo_id IS NOT NULL
            AND octet_length(promoted_echo_id) = 16)
    ),
    ADD CONSTRAINT death_terminal_request_identity UNIQUE (
        namespace_id, account_id, character_id, mutation_id
    ),
    ADD CONSTRAINT death_restore_terminal_owned FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id,
        lineage_id, world_records_blake3, world_assets_blake3,
        world_localization_blake3, mutation_id
    ) REFERENCES character_entry_restore_points (
        namespace_id, account_id, character_id, restore_point_id,
        lineage_id, records_blake3, assets_blake3, localization_blake3,
        death_mutation_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE character_entry_restore_points
    ADD CONSTRAINT restore_death_result_owned FOREIGN KEY (
        namespace_id, account_id, character_id, death_mutation_id
    ) REFERENCES death_events (
        namespace_id, account_id, character_id, mutation_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE echo_records
    ADD CONSTRAINT echo_account_identity UNIQUE (namespace_id, account_id, echo_id);

ALTER TABLE death_events
    ADD CONSTRAINT death_preexisting_available_echo_owned FOREIGN KEY (
        namespace_id, account_id, preexisting_available_echo_id
    ) REFERENCES echo_records (namespace_id, account_id, echo_id)
      DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT death_promoted_echo_owned FOREIGN KEY (
        namespace_id, account_id, promoted_echo_id
    ) REFERENCES echo_records (namespace_id, account_id, echo_id)
      DEFERRABLE INITIALLY DEFERRED;

-- A terminal material stack names the exact death that consumed it, just as an item destruction
-- names its immutable ledger event. Crash revocation continues to name only its restore point.
ALTER TABLE character_run_material_stacks
    ADD COLUMN terminal_death_id BYTEA,
    DROP CONSTRAINT run_material_security_shape,
    ADD CONSTRAINT run_material_security_shape CHECK (
        (security_state = 2 AND quantity > 0
            AND terminal_reason IS NULL
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NULL)
        OR (security_state = 3 AND quantity = 0
            AND terminal_reason = 'permadeath'
            AND terminal_restore_point_id IS NULL
            AND terminal_death_id IS NOT NULL
            AND octet_length(terminal_death_id) = 16
            AND terminal_death_id <> decode(repeat('00', 16), 'hex'))
        OR (security_state = 3 AND quantity = 0
            AND terminal_reason = 'crash_revoked'
            AND terminal_restore_point_id IS NOT NULL
            AND octet_length(terminal_restore_point_id) = 16
            AND terminal_restore_point_id <> decode(repeat('00', 16), 'hex')
            AND terminal_death_id IS NULL)
    ),
    ADD CONSTRAINT run_material_terminal_death_owned FOREIGN KEY (
        namespace_id, account_id, character_id, terminal_death_id
    ) REFERENCES death_events (
        namespace_id, account_id, character_id, death_id
    ) DEFERRABLE INITIALLY DEFERRED;

-- Direct custody/history deletes are rejected by terminal triggers below. Cascading these
-- death-owned references is therefore reserved for the established wipeable account/namespace
-- ownership path and avoids immediate RESTRICT ordering failures during an authorized Core wipe.
ALTER TABLE death_destruction_entries
    DROP CONSTRAINT death_destruction_item_owned,
    DROP CONSTRAINT death_destruction_ledger_owned,
    DROP CONSTRAINT death_destruction_material_owned,
    ADD CONSTRAINT death_destruction_item_owned FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid
    ) REFERENCES item_instances (
        namespace_id, account_id, character_id, item_uid
    ) ON DELETE CASCADE,
    ADD CONSTRAINT death_destruction_ledger_owned FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid,
        ledger_event_id, post_item_version
    ) REFERENCES item_ledger_events (
        namespace_id, account_id, character_id, item_uid,
        ledger_event_id, post_item_version
    ) ON DELETE CASCADE,
    ADD CONSTRAINT death_destruction_material_owned FOREIGN KEY (
        namespace_id, account_id, character_id, material_id, post_material_version
    ) REFERENCES character_run_material_stacks (
        namespace_id, account_id, character_id, material_id, material_version
    ) ON DELETE CASCADE;

ALTER TABLE death_summary_projection_entries
    DROP CONSTRAINT death_summary_projection_item_owned,
    ADD CONSTRAINT death_summary_projection_item_owned FOREIGN KEY (
        namespace_id, item_uid
    ) REFERENCES item_instances (namespace_id, item_uid) ON DELETE CASCADE;

-- An accepted audit is singular. Exact replay/conflict audits remain appendable after commit, but
-- every audit timestamp is PostgreSQL transaction authority.
CREATE UNIQUE INDEX one_accepted_audit_per_death
    ON death_audit_events (namespace_id, death_id, event_kind)
    WHERE event_kind = 0;

CREATE FUNCTION enforce_death_audit_insert_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    death_time TIMESTAMPTZ;
BEGIN
    IF NEW.created_at IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION 'death audit time is PostgreSQL transaction authority';
    END IF;
    IF NEW.event_kind = 0 THEN
        SELECT committed_at INTO death_time
        FROM death_events
        WHERE namespace_id = NEW.namespace_id AND death_id = NEW.death_id;
        IF NOT FOUND OR death_time IS DISTINCT FROM transaction_timestamp() THEN
            RAISE EXCEPTION 'accepted death audit must be inserted with its death';
        END IF;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER death_audit_insert_contract BEFORE INSERT ON death_audit_events
FOR EACH ROW EXECUTE FUNCTION enforce_death_audit_insert_v1();

-- Echo outbox rows bind to the exact state transition they publish. This permits a later death to
-- promote the oldest Dormant Echo, whose immutable source death is necessarily older, without
-- permitting a fabricated or duplicate promotion.
DO $$
DECLARE
    constraint_name name;
BEGIN
    SELECT relation_constraint.conname INTO constraint_name
    FROM pg_constraint AS relation_constraint
    WHERE relation_constraint.conrelid = 'death_outbox_events'::regclass
      AND relation_constraint.contype = 'u'
      AND pg_get_constraintdef(relation_constraint.oid)
          LIKE 'UNIQUE (namespace_id, death_id, event_type, echo_id)%';
    IF constraint_name IS NOT NULL THEN
        EXECUTE format('ALTER TABLE death_outbox_events DROP CONSTRAINT %I', constraint_name);
    END IF;
END
$$;

DROP INDEX one_echo_outbox_event_per_transition_kind;

ALTER TABLE death_outbox_events
    DROP CONSTRAINT death_outbox_echo_shape,
    ADD COLUMN echo_transition_ordinal SMALLINT,
    ADD COLUMN trigger_death_id BYTEA,
    ADD CONSTRAINT death_outbox_echo_shape CHECK (
        (event_type = 'death_committed'
            AND echo_id IS NULL AND echo_transition_ordinal IS NULL
            AND trigger_death_id IS NULL)
        OR (event_type = 'echo_created'
            AND echo_id IS NOT NULL AND octet_length(echo_id) = 16
            AND echo_transition_ordinal = 0
            AND trigger_death_id IS NOT NULL AND trigger_death_id = death_id)
        OR (event_type = 'echo_promoted'
            AND echo_id IS NOT NULL AND octet_length(echo_id) = 16
            AND echo_transition_ordinal IS NOT NULL
            AND echo_transition_ordinal > 0
            AND (trigger_death_id IS NULL
                OR (octet_length(trigger_death_id) = 16
                    AND trigger_death_id <> decode(repeat('00', 16), 'hex'))))
    ),
    ADD CONSTRAINT death_outbox_transition_owned FOREIGN KEY (
        namespace_id, echo_id, echo_transition_ordinal
    ) REFERENCES echo_state_transitions (
        namespace_id, echo_id, transition_ordinal
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT death_outbox_trigger_death_owned FOREIGN KEY (
        namespace_id, trigger_death_id
    ) REFERENCES death_events (namespace_id, death_id)
      DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT death_outbox_transition_unique UNIQUE (
        namespace_id, echo_id, echo_transition_ordinal, event_type
    );

-- A zero entity identity is never a legible source. A nonlethal trace row may not leave health at
-- zero; exactly one matching final lethal row is additionally required by the deferred closure.
ALTER TABLE death_combat_trace_entries
    ADD CONSTRAINT death_trace_source_entity_nonzero CHECK (
        source_entity_id IS NULL
        OR source_entity_id <> decode(repeat('00', 16), 'hex')
    ),
    DROP CONSTRAINT death_trace_terminal_shape,
    ADD CONSTRAINT death_trace_terminal_shape CHECK (
        (lethal AND post_health = 0)
        OR (NOT lethal AND post_health > 0)
    );

ALTER TABLE death_events
    DROP CONSTRAINT death_cause_final_known,
    ADD CONSTRAINT death_cause_final_known CHECK (cause_kind BETWEEN 0 AND 3),
    ADD CONSTRAINT death_cause_ids_required CHECK (
        killer_content_id IS NOT NULL AND killer_attack_id IS NOT NULL
    ),
    DROP CONSTRAINT death_damage_shape,
    ADD CONSTRAINT death_damage_shape CHECK (
        raw_damage >= 0 AND final_damage > 0
        AND damage_type IN (0, 1) AND pre_hit_health > 0
        AND final_damage >= pre_hit_health
    );

ALTER TABLE death_combat_trace_entries
    DROP CONSTRAINT death_trace_damage_shape,
    ADD CONSTRAINT death_trace_damage_shape CHECK (
        raw_damage >= 0 AND final_damage >= 0
        AND damage_type IN (0, 1)
        AND pre_health > 0 AND post_health BETWEEN 0 AND pre_health
        AND post_health = GREATEST(0, pre_health - final_damage)
    );

ALTER TABLE death_summary_snapshots
    DROP CONSTRAINT death_summary_echo_outcome_known,
    ADD CONSTRAINT death_summary_echo_outcome_known CHECK (echo_outcome BETWEEN 0 AND 2);

ALTER TABLE echo_state_transitions
    ADD COLUMN trigger_death_id BYTEA,
    ADD CONSTRAINT echo_transition_trigger_shape CHECK (
        (transition_ordinal = 0
            AND trigger_death_id = source_death_id
            AND octet_length(trigger_death_id) = 16)
        OR (transition_ordinal > 0 AND previous_state = 0 AND next_state = 1
            AND trigger_death_id IS NOT NULL
            AND octet_length(trigger_death_id) = 16)
        OR (transition_ordinal > 0
            AND NOT (previous_state = 0 AND next_state = 1)
            AND trigger_death_id IS NULL)
    ),
    ADD CONSTRAINT echo_transition_trigger_death_owned FOREIGN KEY (
        namespace_id, trigger_death_id
    ) REFERENCES death_events (namespace_id, death_id) ON DELETE CASCADE,
    ADD CONSTRAINT echo_transition_reason_shape CHECK (
        (transition_ordinal = 0 AND previous_state IS NULL
            AND next_state = 0 AND reason_kind = 0)
        OR (transition_ordinal > 0 AND previous_state = 0
            AND next_state = 1 AND reason_kind = 1)
        OR (transition_ordinal > 0
            AND NOT (previous_state = 0 AND next_state = 1)
            AND reason_kind BETWEEN 2 AND 7)
    );

-- Every destruction entry is checked against the final item/material row and the exact death
-- ledger transition. This is deferred because all three records are written in one transaction.
CREATE FUNCTION enforce_death_destruction_source_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT := COALESCE(NEW.namespace_id, OLD.namespace_id);
    target_death_id BYTEA := COALESCE(NEW.death_id, OLD.death_id);
    target_ordinal SMALLINT := COALESCE(NEW.destruction_ordinal, OLD.destruction_ordinal);
    entry death_destruction_entries%ROWTYPE;
    death death_events%ROWTYPE;
BEGIN
    SELECT * INTO entry
    FROM death_destruction_entries
    WHERE namespace_id = target_namespace
      AND death_id = target_death_id
      AND destruction_ordinal = target_ordinal;
    IF NOT FOUND THEN RETURN NULL; END IF;

    SELECT * INTO death
    FROM death_events
    WHERE namespace_id = entry.namespace_id AND death_id = entry.death_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'death destruction entry has no owning death';
    END IF;

    IF entry.entry_kind = 0 THEN
        IF NOT EXISTS (
            SELECT 1
            FROM item_instances AS item
            JOIN item_ledger_events AS ledger
              ON ledger.namespace_id = item.namespace_id
             AND ledger.item_uid = item.item_uid
             AND ledger.ledger_event_id = entry.ledger_event_id
            WHERE item.namespace_id = entry.namespace_id
              AND item.account_id = entry.account_id
              AND item.character_id = entry.character_id
              AND item.item_uid = entry.item_uid
              AND item.item_version = entry.post_item_version
              AND item.security_state = 3
              AND item.location_kind = 4
              AND item.destruction_reason = 'permadeath'
              AND ledger.account_id = entry.account_id
              AND ledger.character_id = entry.character_id
              AND ledger.mutation_id = death.mutation_id
              AND ledger.event_kind = 2
              AND ledger.source_kind = 3
              AND ledger.pre_item_version = entry.pre_item_version
              AND ledger.post_item_version = entry.post_item_version
              AND ledger.pre_security_state IN (1, 2)
              AND ledger.post_security_state = 3
              AND ledger.pre_location_kind = entry.pre_location_kind
              AND ledger.post_location_kind = 4
              AND ledger.reason = 'permadeath'
        ) THEN
            RAISE EXCEPTION 'death item destruction is not bound to its exact permadeath transition';
        END IF;
    ELSIF entry.entry_kind = 1 THEN
        IF NOT EXISTS (
            SELECT 1
            FROM character_run_material_stacks AS material
            WHERE material.namespace_id = entry.namespace_id
              AND material.account_id = entry.account_id
              AND material.character_id = entry.character_id
              AND material.material_id = entry.material_id
              AND material.material_version = entry.post_material_version
              AND material.quantity = 0
              AND material.security_state = 3
              AND material.terminal_reason = 'permadeath'
              AND material.terminal_restore_point_id IS NULL
              AND material.terminal_death_id = death.death_id
        ) THEN
            RAISE EXCEPTION 'death material destruction is not bound to its exact terminal state';
        END IF;
    ELSE
        RAISE EXCEPTION 'death destruction entry kind is unknown';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER death_destruction_source_exact
AFTER INSERT OR UPDATE ON death_destruction_entries
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_death_destruction_source_v1();

-- The root closure proves that an accepted death is a complete immutable graph rather than a
-- collection of optional child rows. It also proves commit-time post versions, trace ordering,
-- canonical destruction order, and the mandatory in-transaction Echo projector seam.
CREATE FUNCTION enforce_complete_death_graph_v1()
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
            WHERE NOT EXISTS (
                SELECT 1 FROM death_summary_projection_entries AS projection
                WHERE projection.namespace_id = death.namespace_id
                  AND projection.death_id = death.death_id
                  AND projection.section_kind = section.section_kind
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
        (SELECT count(*) FROM item_ledger_events
         WHERE namespace_id = death.namespace_id
           AND account_id = death.account_id AND character_id = death.character_id
           AND mutation_id = death.mutation_id AND event_kind = 2
           AND source_kind = 3 AND reason = 'permadeath')
        +
        (SELECT count(*) FROM character_run_material_stacks
         WHERE namespace_id = death.namespace_id
           AND account_id = death.account_id AND character_id = death.character_id
           AND security_state = 3 AND quantity = 0 AND terminal_reason = 'permadeath'
           AND terminal_death_id = death.death_id)
    INTO expected_destruction_count;
    IF destruction_count <> expected_destruction_count
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

-- Attach the one closure function to every relation that can make the graph complete or corrupt
-- it. One deferred root execution sees the transaction's final state; child insert-window and
-- immutability triggers prevent post-commit extension without queuing a quadratic row-trigger
-- scan for every trace/status/destruction row.
CREATE CONSTRAINT TRIGGER complete_death_graph_root
AFTER INSERT ON death_events
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION enforce_complete_death_graph_v1();

-- Later Echo lifecycle transitions must not revalidate mutable character/account heads from the
-- historical death commit. They do retain an independent deferred history closure.
CREATE FUNCTION enforce_echo_history_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT := COALESCE(NEW.namespace_id, OLD.namespace_id);
    target_echo_id BYTEA := COALESCE(NEW.echo_id, OLD.echo_id);
    target_state SMALLINT;
    transition_count INTEGER;
    target_account_id BYTEA;
    target_created_at TIMESTAMPTZ;
    tail_ordinal SMALLINT;
    tail_previous_state SMALLINT;
    tail_next_state SMALLINT;
    tail_reason SMALLINT;
    tail_committed_at TIMESTAMPTZ;
    account_available_count INTEGER;
    account_dormant_count INTEGER;
BEGIN
    SELECT state, account_id, created_at
    INTO target_state, target_account_id, target_created_at
    FROM echo_records
    WHERE namespace_id = target_namespace AND echo_id = target_echo_id;
    IF NOT FOUND THEN RETURN NULL; END IF;
    SELECT count(*) INTO transition_count
    FROM echo_state_transitions
    WHERE namespace_id = target_namespace AND echo_id = target_echo_id;
    IF transition_count < 1
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT transition_ordinal, previous_state, next_state,
                    lag(next_state) OVER (ORDER BY transition_ordinal) AS prior_next,
                    row_number() OVER (ORDER BY transition_ordinal) - 1 AS expected_ordinal
                FROM echo_state_transitions
                WHERE namespace_id = target_namespace AND echo_id = target_echo_id
            ) AS transition
            WHERE transition.transition_ordinal <> transition.expected_ordinal
               OR (transition.transition_ordinal = 0
                   AND (transition.previous_state IS NOT NULL OR transition.next_state <> 0))
               OR (transition.transition_ordinal > 0
                   AND transition.previous_state IS DISTINCT FROM transition.prior_next)
        )
        OR NOT EXISTS (
            SELECT 1 FROM echo_state_transitions
            WHERE namespace_id = target_namespace AND echo_id = target_echo_id
              AND transition_ordinal = transition_count - 1
              AND next_state = target_state
        )
    THEN
        RAISE EXCEPTION 'Echo transition history is incomplete or disagrees with current state';
    END IF;
    SELECT transition_ordinal, previous_state, next_state, reason_kind, committed_at
    INTO tail_ordinal, tail_previous_state, tail_next_state, tail_reason, tail_committed_at
    FROM echo_state_transitions
    WHERE namespace_id = target_namespace AND echo_id = target_echo_id
    ORDER BY transition_ordinal DESC LIMIT 1;
    IF target_state = 1 THEN
        IF tail_previous_state <> 0 OR tail_next_state <> 1 OR tail_reason <> 1
            OR EXISTS (
                SELECT 1 FROM echo_records AS dormant
                WHERE dormant.namespace_id = target_namespace
                  AND dormant.account_id = target_account_id
                  AND (
                      dormant.state = 0
                      OR EXISTS (
                          SELECT 1
                          FROM echo_state_transitions AS prior_state
                          WHERE prior_state.namespace_id = dormant.namespace_id
                            AND prior_state.echo_id = dormant.echo_id
                            AND prior_state.previous_state = 0
                            AND prior_state.next_state = 1
                            AND prior_state.committed_at = tail_committed_at
                      )
                  )
                  AND (dormant.created_at, dormant.echo_id)
                      < (target_created_at, target_echo_id)
            )
            OR (SELECT count(*)
                FROM death_outbox_events AS outbox
                WHERE outbox.namespace_id = target_namespace
                  AND outbox.echo_id = target_echo_id
                  AND outbox.echo_transition_ordinal = tail_ordinal
                  AND outbox.event_type = 'echo_promoted'
                  AND outbox.created_at = tail_committed_at) <> 1
        THEN
            RAISE EXCEPTION 'Available Echo is not the transition-owned oldest Dormant promotion';
        END IF;
    END IF;
    SELECT count(*) FILTER (WHERE state = 1), count(*) FILTER (WHERE state = 0)
    INTO account_available_count, account_dormant_count
    FROM echo_records
    WHERE namespace_id = target_namespace AND account_id = target_account_id;
    IF account_available_count = 0 AND account_dormant_count > 0 THEN
        RAISE EXCEPTION 'Echo account with Dormant history must promote its oldest candidate';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER echo_record_history_complete
AFTER INSERT OR UPDATE ON echo_records
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION enforce_echo_history_v1();
CREATE CONSTRAINT TRIGGER echo_transition_history_complete
AFTER INSERT ON echo_state_transitions
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION enforce_echo_history_v1();

CREATE FUNCTION enforce_echo_transition_insert_time_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.committed_at IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION 'Echo transition time is PostgreSQL transaction authority';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER echo_transition_insert_time
BEFORE INSERT ON echo_state_transitions
FOR EACH ROW EXECUTE FUNCTION enforce_echo_transition_insert_time_v1();

-- Snapshot children may be appended only in the transaction that creates the death. This closes
-- the otherwise-valid possibility of adding a status/projection row after the immutable graph
-- was acknowledged. Conflict audits and later Echo state transitions intentionally remain
-- appendable through their separate contracts.
CREATE FUNCTION enforce_death_child_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_death_id BYTEA;
    death_time TIMESTAMPTZ;
BEGIN
    IF TG_TABLE_NAME = 'death_outbox_events' AND NEW.event_type = 'echo_promoted' THEN
        IF NOT EXISTS (
            SELECT 1
            FROM echo_state_transitions AS transition
            WHERE transition.namespace_id = NEW.namespace_id
              AND transition.echo_id = NEW.echo_id
              AND transition.transition_ordinal = NEW.echo_transition_ordinal
              AND transition.previous_state = 0 AND transition.next_state = 1
              AND transition.committed_at = transaction_timestamp()
        ) OR NEW.created_at IS DISTINCT FROM transaction_timestamp() THEN
            RAISE EXCEPTION 'Echo promotion outbox must match its current transaction transition';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_TABLE_NAME IN ('echo_bargain_snapshots', 'echo_deed_tags') THEN
        SELECT death_id INTO target_death_id
        FROM echo_records
        WHERE namespace_id = NEW.namespace_id AND echo_id = NEW.echo_id;
    ELSE
        target_death_id := NEW.death_id;
    END IF;
    SELECT committed_at INTO death_time
    FROM death_events
    WHERE namespace_id = NEW.namespace_id AND death_id = target_death_id;
    IF NOT FOUND OR death_time IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION '% may be inserted only with its owning death', TG_TABLE_NAME;
    END IF;
    RETURN NEW;
END
$$;

CREATE FUNCTION enforce_death_event_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.committed_at IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION 'death commit time is PostgreSQL transaction authority';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER death_event_insert_window BEFORE INSERT ON death_events
FOR EACH ROW EXECUTE FUNCTION enforce_death_event_insert_window_v1();
CREATE TRIGGER death_trace_insert_window BEFORE INSERT ON death_combat_trace_entries
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_status_insert_window BEFORE INSERT ON death_combat_trace_statuses
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_summary_insert_window BEFORE INSERT ON death_summary_snapshots
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_summary_bargain_insert_window BEFORE INSERT ON death_summary_bargains
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_summary_damage_insert_window BEFORE INSERT ON death_summary_damage_entries
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_projection_insert_window BEFORE INSERT ON death_summary_projection_entries
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER memorial_insert_window BEFORE INSERT ON memorial_records
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_destruction_insert_window BEFORE INSERT ON death_destruction_entries
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_result_insert_window BEFORE INSERT ON death_mutation_results
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_outbox_insert_window BEFORE INSERT ON death_outbox_events
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER echo_record_insert_window BEFORE INSERT ON echo_records
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER echo_bargain_insert_window BEFORE INSERT ON echo_bargain_snapshots
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER echo_deed_insert_window BEFORE INSERT ON echo_deed_tags
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();

-- Death snapshots and histories are append-only. Namespace/account cascades remain the only
-- deletion path in the explicitly wipeable Core namespace.
CREATE FUNCTION reject_death_history_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
    RAISE EXCEPTION '% history is immutable', TG_TABLE_NAME;
END
$$;

CREATE TRIGGER death_events_immutable BEFORE UPDATE OR DELETE ON death_events
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_trace_immutable BEFORE UPDATE OR DELETE ON death_combat_trace_entries
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_trace_status_immutable BEFORE UPDATE OR DELETE ON death_combat_trace_statuses
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_summary_immutable BEFORE UPDATE OR DELETE ON death_summary_snapshots
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_summary_bargain_immutable BEFORE UPDATE OR DELETE ON death_summary_bargains
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_summary_damage_immutable BEFORE UPDATE OR DELETE ON death_summary_damage_entries
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_summary_projection_immutable BEFORE UPDATE OR DELETE ON death_summary_projection_entries
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER memorial_records_immutable BEFORE UPDATE OR DELETE ON memorial_records
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_destruction_immutable BEFORE UPDATE OR DELETE ON death_destruction_entries
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_results_immutable BEFORE UPDATE OR DELETE ON death_mutation_results
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_audits_immutable BEFORE UPDATE OR DELETE ON death_audit_events
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER echo_bargains_immutable BEFORE UPDATE OR DELETE ON echo_bargain_snapshots
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER echo_deeds_immutable BEFORE UPDATE OR DELETE ON echo_deed_tags
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER echo_transitions_immutable BEFORE UPDATE OR DELETE ON echo_state_transitions
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();

-- Echo snapshots never change; only the state discriminant may advance, with deferred closure
-- requiring an appended CONT-ECHO-009 transition whose tail equals the new state.
CREATE FUNCTION enforce_echo_record_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'Echo history is immutable';
    END IF;
    IF ROW(
        NEW.namespace_id, NEW.echo_id, NEW.death_id, NEW.account_id,
        NEW.character_name_snapshot, NEW.class_id, NEW.oath_id, NEW.level,
        NEW.appearance_snapshot_id, NEW.appearance_theme_id,
        NEW.weapon_signature_tag, NEW.relic_signature_tag,
        NEW.killer_content_id, NEW.killer_pattern_id, NEW.death_region_id,
        NEW.power_band, NEW.content_revision, NEW.snapshot_digest, NEW.created_at
    ) IS DISTINCT FROM ROW(
        OLD.namespace_id, OLD.echo_id, OLD.death_id, OLD.account_id,
        OLD.character_name_snapshot, OLD.class_id, OLD.oath_id, OLD.level,
        OLD.appearance_snapshot_id, OLD.appearance_theme_id,
        OLD.weapon_signature_tag, OLD.relic_signature_tag,
        OLD.killer_content_id, OLD.killer_pattern_id, OLD.death_region_id,
        OLD.power_band, OLD.content_revision, OLD.snapshot_digest, OLD.created_at
    ) OR NEW.state = OLD.state THEN
        RAISE EXCEPTION 'Echo snapshot and no-op state history are immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER echo_records_transition_only
BEFORE UPDATE OR DELETE ON echo_records
FOR EACH ROW EXECUTE FUNCTION enforce_echo_record_mutation_v1();

-- Outbox payloads are immutable. The publisher may set `published_at` exactly once after commit.
CREATE FUNCTION enforce_death_outbox_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'death outbox history is immutable';
    END IF;
    IF OLD.published_at IS NOT NULL OR NEW.published_at IS NULL
        OR ROW(
            NEW.namespace_id, NEW.death_id, NEW.event_id, NEW.event_type,
            NEW.echo_id, NEW.echo_transition_ordinal, NEW.trigger_death_id,
            NEW.event_payload, NEW.created_at
        ) IS DISTINCT FROM ROW(
            OLD.namespace_id, OLD.death_id, OLD.event_id, OLD.event_type,
            OLD.echo_id, OLD.echo_transition_ordinal, OLD.trigger_death_id,
            OLD.event_payload, OLD.created_at
        ) THEN
        RAISE EXCEPTION 'death outbox permits only first publication';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER death_outbox_publish_only
BEFORE UPDATE OR DELETE ON death_outbox_events
FOR EACH ROW EXECUTE FUNCTION enforce_death_outbox_mutation_v1();

-- A valid death graph cannot later be rewritten into a resurrection. Final death has already
-- archived the identity out of the playable roster; successor creation writes a new character in
-- the freed ordinal and never mutates the dead identity or its aggregate history.
CREATE FUNCTION enforce_dead_character_terminal_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM death_events
        WHERE namespace_id = OLD.namespace_id
          AND account_id = OLD.account_id AND character_id = OLD.character_id
    ) THEN
        IF TG_OP = 'UPDATE'
            AND (NEW.life_state IS DISTINCT FROM OLD.life_state
                 OR NEW.roster_ordinal IS DISTINCT FROM OLD.roster_ordinal)
        THEN
            RAISE EXCEPTION 'character roster/life transition requires its current death root';
        END IF;
        IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'dead character may be deleted only by wipeable account/namespace cascade';
    END IF;
    IF OLD.life_state = 0 AND NEW.life_state = 1
        AND OLD.roster_ordinal BETWEEN 1 AND 2
        AND NEW.roster_ordinal IS NULL
        AND NEW.character_state_version = OLD.character_state_version + 1
        AND NEW.updated_at = transaction_timestamp()
        AND ROW(
            NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.class_id, NEW.level,
            NEW.oath_id, NEW.security_state, NEW.created_at
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.class_id, OLD.level,
            OLD.oath_id, OLD.security_state, OLD.created_at
        )
        AND EXISTS (
            SELECT 1 FROM death_events AS death
            WHERE death.namespace_id = OLD.namespace_id
              AND death.account_id = OLD.account_id
              AND death.character_id = OLD.character_id
              AND death.former_roster_ordinal = OLD.roster_ordinal
              AND death.pre_character_version = OLD.character_state_version
              AND death.post_character_version = NEW.character_state_version
              AND death.committed_at = transaction_timestamp()
        )
    THEN
        RETURN NEW;
    END IF;
    IF ROW(
        NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.roster_ordinal,
        NEW.class_id, NEW.level,
        NEW.oath_id, NEW.life_state, NEW.security_state,
        NEW.character_state_version, NEW.created_at, NEW.updated_at
    ) IS DISTINCT FROM ROW(
        OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.roster_ordinal,
        OLD.class_id, OLD.level,
        OLD.oath_id, OLD.life_state, OLD.security_state,
        OLD.character_state_version, OLD.created_at, OLD.updated_at
    ) THEN
        RAISE EXCEPTION 'dead character terminal authority is immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER dead_character_terminal
BEFORE UPDATE OR DELETE ON characters
FOR EACH ROW EXECUTE FUNCTION enforce_dead_character_terminal_v1();

CREATE FUNCTION reject_dead_aggregate_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT;
    target_account BYTEA;
    target_character BYTEA;
BEGIN
    IF TG_OP = 'DELETE' THEN
        target_namespace := OLD.namespace_id;
        target_account := OLD.account_id;
        target_character := OLD.character_id;
    ELSE
        target_namespace := NEW.namespace_id;
        target_account := NEW.account_id;
        target_character := NEW.character_id;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM death_events
        WHERE namespace_id = target_namespace
          AND account_id = target_account AND character_id = target_character
    ) THEN
        IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
    RAISE EXCEPTION '% is immutable after final death', TG_TABLE_NAME;
END
$$;

CREATE TRIGGER dead_progression_immutable
BEFORE UPDATE OR DELETE ON character_progression
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_inventory_immutable
BEFORE UPDATE OR DELETE ON character_inventories
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_life_metrics_immutable
BEFORE UPDATE OR DELETE ON character_life_metrics
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_world_location_immutable
BEFORE UPDATE OR DELETE ON character_world_locations
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_lineage_immutable
BEFORE UPDATE OR DELETE ON character_instance_lineages
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_oath_bargain_state_immutable
BEFORE UPDATE OR DELETE ON character_oath_bargain_state
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_bargain_offer_immutable
BEFORE UPDATE OR DELETE ON bargain_offers
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_active_bargain_immutable
BEFORE UPDATE OR DELETE ON character_active_bargains
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_life_deed_immutable
BEFORE UPDATE OR DELETE ON character_life_deeds
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();
CREATE TRIGGER dead_reward_request_immutable
BEFORE UPDATE OR DELETE ON reward_requests
FOR EACH ROW EXECUTE FUNCTION reject_dead_aggregate_mutation_v1();

-- Append-only receipts are still gameplay mutations. Once a death exists, no later transaction
-- may add a lineage, reward, item, deed, extraction, or other character-owned result and thereby
-- manufacture posthumous authority. The death writer stages its final ledger rows before the
-- death root; all other routes fail closed against the committed terminal identity.
CREATE FUNCTION reject_dead_character_insert_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.character_id IS NULL THEN RETURN NEW; END IF;
    IF EXISTS (
        SELECT 1 FROM death_events
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id AND character_id = NEW.character_id
    ) THEN
        RAISE EXCEPTION '% cannot append authority after final death', TG_TABLE_NAME;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER dead_lineage_insert
BEFORE INSERT ON character_instance_lineages
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_restore_point_insert
BEFORE INSERT ON character_entry_restore_points
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_world_transfer_insert
BEFORE INSERT ON character_world_transfer_results
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_danger_checkpoint_insert
BEFORE INSERT ON character_danger_checkpoints
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_xp_award_insert
BEFORE INSERT ON character_xp_award_results
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_boss_first_clear_insert
BEFORE INSERT ON account_boss_first_clears
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_oath_result_insert
BEFORE INSERT ON character_oath_mutation_results
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_life_outbox_insert
BEFORE INSERT ON character_life_outbox
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_starter_result_insert
BEFORE INSERT ON starter_initializer_results
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_reward_request_insert
BEFORE INSERT ON reward_requests
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_item_insert
BEFORE INSERT ON item_instances
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_item_ledger_insert
BEFORE INSERT ON item_ledger_events
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_bargain_offer_insert
BEFORE INSERT ON bargain_offers
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_active_bargain_insert
BEFORE INSERT ON character_active_bargains
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_bargain_milestone_insert
BEFORE INSERT ON bargain_milestone_results
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_bargain_decision_insert
BEFORE INSERT ON bargain_decision_results
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_caldus_exit_owner_insert
BEFORE INSERT ON caldus_victory_exit_owners
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_extraction_result_insert
BEFORE INSERT ON character_extraction_results
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_field_equipment_insert
BEFORE INSERT ON field_equipment_mutations
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_safe_inventory_insert
BEFORE INSERT ON safe_inventory_mutations
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_life_deed_insert
BEFORE INSERT ON character_life_deeds
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_run_material_insert
BEFORE INSERT ON character_run_material_stacks
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_crash_restore_result_insert
BEFORE INSERT ON danger_crash_restore_results
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();

-- Reward children do not repeat character identity, so resolve it through their immutable parent.
CREATE FUNCTION reject_dead_reward_entry_insert_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM reward_requests AS reward
        JOIN death_events AS death
          ON death.namespace_id = reward.namespace_id
         AND death.account_id = reward.account_id
         AND death.character_id = reward.character_id
        WHERE reward.namespace_id = NEW.namespace_id
          AND reward.reward_request_id = NEW.reward_request_id
    ) THEN
        RAISE EXCEPTION 'reward result cannot be extended after final death';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER dead_reward_entry_insert
BEFORE INSERT ON reward_result_entries
FOR EACH ROW EXECUTE FUNCTION reject_dead_reward_entry_insert_v1();

CREATE FUNCTION reject_permadeath_custody_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    terminal BOOLEAN := FALSE;
BEGIN
    IF TG_TABLE_NAME = 'item_instances' THEN
        SELECT EXISTS (
            SELECT 1 FROM death_destruction_entries
            WHERE namespace_id = OLD.namespace_id AND item_uid = OLD.item_uid
        ) INTO terminal;
    ELSIF TG_TABLE_NAME = 'item_ledger_events' THEN
        SELECT EXISTS (
            SELECT 1 FROM death_destruction_entries
            WHERE namespace_id = OLD.namespace_id AND item_uid = OLD.item_uid
        ) INTO terminal;
    ELSIF TG_TABLE_NAME = 'character_run_material_stacks' THEN
        terminal := OLD.terminal_death_id IS NOT NULL;
    END IF;
    IF NOT terminal THEN
        IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
    RAISE EXCEPTION 'permadeath item/material custody is immutable';
END
$$;

CREATE TRIGGER permadeath_item_immutable
BEFORE UPDATE OR DELETE ON item_instances
FOR EACH ROW EXECUTE FUNCTION reject_permadeath_custody_mutation_v1();
CREATE TRIGGER permadeath_item_ledger_immutable
BEFORE UPDATE OR DELETE ON item_ledger_events
FOR EACH ROW EXECUTE FUNCTION reject_permadeath_custody_mutation_v1();
CREATE TRIGGER permadeath_material_immutable
BEFORE UPDATE OR DELETE ON character_run_material_stacks
FOR EACH ROW EXECUTE FUNCTION reject_permadeath_custody_mutation_v1();

COMMENT ON COLUMN character_entry_restore_points.death_mutation_id IS
    'Exact final-death mutation for terminal restore_state 2; null for every other state.';
COMMENT ON COLUMN death_events.echo_expected IS
    'Server-authored eligibility outcome; true requires same-transaction Echo projection.';
