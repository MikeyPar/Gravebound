-- GB-M03-02D / GB-M03-06A durable-death child insertion-window correction.
--
-- Authorities: Gravebound_Production_GDD_v1_Canonical.md DTH-001/DTH-020 and
-- TECH-020..023; Gravebound_Content_Production_Spec_v1.md CONT-HUB-002 and
-- CONT-ECHO-009; Gravebound_Development_Roadmap_v1.md GB-M03-02/06/13.
--
-- PostgreSQL does not promise short-circuit evaluation for boolean expressions. Migration 0037
-- therefore cannot guard a relation-specific NEW.event_type field with a boolean conjunction:
-- the field may still be resolved for another trigger relation. Preserve the published history and
-- replace only the function body with an explicit procedural relation guard.

CREATE OR REPLACE FUNCTION enforce_death_child_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_death_id BYTEA;
    death_time TIMESTAMPTZ;
BEGIN
    IF TG_TABLE_NAME = 'death_outbox_events' THEN
        IF NEW.event_type = 'echo_promoted' THEN
            IF NOT EXISTS (
                SELECT 1
                FROM echo_state_transitions AS transition
                WHERE transition.namespace_id = NEW.namespace_id
                  AND transition.echo_id = NEW.echo_id
                  AND transition.transition_ordinal = NEW.echo_transition_ordinal
                  AND transition.previous_state = 0 AND transition.next_state = 1
                  AND transition.committed_at = transaction_timestamp()
            ) OR NEW.created_at IS DISTINCT FROM transaction_timestamp() THEN
                RAISE EXCEPTION
                    'Echo promotion outbox must match its current transaction transition';
            END IF;
            RETURN NEW;
        END IF;
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

COMMENT ON FUNCTION enforce_death_child_insert_window_v1() IS
    'Relation-safe immutable death-child insertion window; 0041 forward correction.';
