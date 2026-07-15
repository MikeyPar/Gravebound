-- GB-M03-02D / GB-M03-06B..06C diagnosable live-to-durable provenance validation.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-001/DTH-020 and TECH-020..023;
-- - Gravebound_Content_Production_Spec_v1.md CONT-ECHO-001/009 and CONT-HUB-002;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-02/06/13 and the atomic-death gate;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md decisions 4/5.
--
-- Migration 0049 introduced one immediate all-in-one predicate. Keep its table and trigger history
-- intact, but replace the function body with staged checks so a rejected promotion identifies the
-- exact authority layer without exposing player or network data.

CREATE OR REPLACE FUNCTION enforce_death_live_trace_provenance_source_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    live_entry RECORD;
    durable_entry RECORD;
BEGIN
    SELECT live.*
      INTO live_entry
      FROM death_live_trace_receipt_links_v1 AS receipt
      JOIN character_live_damage_trace_entries_v1 AS live
        ON live.namespace_id = receipt.namespace_id
       AND live.account_id = receipt.account_id
       AND live.character_id = receipt.character_id
       AND live.lineage_id = receipt.lineage_id
       AND live.restore_point_id = receipt.restore_point_id
       AND live.trace_tick_id = receipt.trace_tick_id
       AND live.event_tick = receipt.event_tick
       AND live.event_ordinal = NEW.event_ordinal
     WHERE receipt.namespace_id = NEW.namespace_id
       AND receipt.death_id = NEW.death_id
       AND receipt.receipt_ordinal = NEW.receipt_ordinal
       AND receipt.trace_tick_id = NEW.trace_tick_id
       AND receipt.event_tick = NEW.event_tick;

    IF NOT FOUND THEN
        RAISE EXCEPTION
            'death live-trace provenance % has no exact retained live entry',
            NEW.trace_ordinal;
    END IF;

    IF live_entry.cause_kind IS DISTINCT FROM NEW.cause_kind
        OR live_entry.source_entity_id IS DISTINCT FROM NEW.source_entity_id
        OR live_entry.source_sim_entity_id IS DISTINCT FROM NEW.source_sim_entity_id
        OR live_entry.status_count IS DISTINCT FROM NEW.status_count
        OR live_entry.entry_digest IS DISTINCT FROM NEW.live_entry_digest
    THEN
        RAISE EXCEPTION
            'death live-trace provenance % differs from retained live authority',
            NEW.trace_ordinal;
    END IF;

    SELECT durable.*
      INTO durable_entry
      FROM death_combat_trace_entries AS durable
     WHERE durable.namespace_id = NEW.namespace_id
       AND durable.death_id = NEW.death_id
       AND durable.trace_ordinal = NEW.trace_ordinal;

    IF NOT FOUND THEN
        RAISE EXCEPTION
            'death live-trace provenance % has no durable trace entry',
            NEW.trace_ordinal;
    END IF;

    IF durable_entry.event_tick IS DISTINCT FROM live_entry.event_tick
        OR durable_entry.event_ordinal IS DISTINCT FROM live_entry.event_ordinal
        OR durable_entry.source_content_id IS DISTINCT FROM live_entry.source_content_id
        OR durable_entry.source_entity_id IS DISTINCT FROM live_entry.source_entity_id
        OR durable_entry.pattern_id IS DISTINCT FROM live_entry.pattern_id
        OR durable_entry.attack_id IS DISTINCT FROM live_entry.attack_id
        OR durable_entry.raw_damage IS DISTINCT FROM live_entry.raw_damage
        OR durable_entry.final_damage IS DISTINCT FROM live_entry.final_damage
        OR durable_entry.damage_type IS DISTINCT FROM live_entry.damage_type
        OR durable_entry.pre_health IS DISTINCT FROM live_entry.pre_health
        OR durable_entry.post_health IS DISTINCT FROM live_entry.post_health
        OR durable_entry.source_x_milli_tiles IS DISTINCT FROM live_entry.source_x_milli_tiles
        OR durable_entry.source_y_milli_tiles IS DISTINCT FROM live_entry.source_y_milli_tiles
        OR durable_entry.network_state IS DISTINCT FROM live_entry.network_state
        OR durable_entry.recall_state IS DISTINCT FROM live_entry.recall_state
        OR durable_entry.lethal IS DISTINCT FROM live_entry.lethal
    THEN
        RAISE EXCEPTION
            'death live-trace provenance % differs from durable trace authority',
            NEW.trace_ordinal;
    END IF;

    IF EXISTS (
        (SELECT status_ordinal, status_id COLLATE "C", remaining_ticks, stack_count
           FROM character_live_damage_trace_statuses_v1 AS live_status
          WHERE live_status.namespace_id = live_entry.namespace_id
            AND live_status.account_id = live_entry.account_id
            AND live_status.character_id = live_entry.character_id
            AND live_status.lineage_id = live_entry.lineage_id
            AND live_status.restore_point_id = live_entry.restore_point_id
            AND live_status.trace_tick_id = live_entry.trace_tick_id
            AND live_status.event_tick = live_entry.event_tick
            AND live_status.event_ordinal = live_entry.event_ordinal)
        EXCEPT
        (SELECT status_ordinal, status_id COLLATE "C", remaining_ticks, stack_count
           FROM death_combat_trace_statuses AS durable_status
          WHERE durable_status.namespace_id = NEW.namespace_id
            AND durable_status.death_id = NEW.death_id
            AND durable_status.trace_ordinal = NEW.trace_ordinal)
    ) OR EXISTS (
        (SELECT status_ordinal, status_id COLLATE "C", remaining_ticks, stack_count
           FROM death_combat_trace_statuses AS durable_status
          WHERE durable_status.namespace_id = NEW.namespace_id
            AND durable_status.death_id = NEW.death_id
            AND durable_status.trace_ordinal = NEW.trace_ordinal)
        EXCEPT
        (SELECT status_ordinal, status_id COLLATE "C", remaining_ticks, stack_count
           FROM character_live_damage_trace_statuses_v1 AS live_status
          WHERE live_status.namespace_id = live_entry.namespace_id
            AND live_status.account_id = live_entry.account_id
            AND live_status.character_id = live_entry.character_id
            AND live_status.lineage_id = live_entry.lineage_id
            AND live_status.restore_point_id = live_entry.restore_point_id
            AND live_status.trace_tick_id = live_entry.trace_tick_id
            AND live_status.event_tick = live_entry.event_tick
            AND live_status.event_ordinal = live_entry.event_ordinal)
    ) THEN
        RAISE EXCEPTION
            'death live-trace provenance % has divergent durable statuses',
            NEW.trace_ordinal;
    END IF;

    RETURN NEW;
END
$$;

-- Downgrade/recovery: Core remains wipeable. Restore a pre-0050 backup or wipe/reapply the Core
-- namespace. Never rewrite migration 0049 or bypass its immediate provenance trigger.
