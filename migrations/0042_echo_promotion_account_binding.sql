-- GB-M03-06 / GB-M03-13 Echo-promotion account-authority correction.
--
-- Authorities: Gravebound_Production_GDD_v1_Canonical.md ECH-001..003 and TECH-021..022;
-- Gravebound_Content_Production_Spec_v1.md CONT-ECHO-009;
-- Gravebound_Development_Roadmap_v1.md GB-M03-06/13 and the atomic qualifying-death exit gate;
-- owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md decision 5.
--
-- Published migration 0037 bound a promotion trigger to a durable death, but the foreign key did
-- not prove that the trigger death and target Echo belong to the same account. Its outbox row also
-- repeated trigger_death_id without binding that value to the transition it publishes. Preserve the
-- existing death-time, legal-edge, and oldest-first closures and add the missing account identity.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM echo_state_transitions AS transition
        JOIN echo_records AS echo
          ON echo.namespace_id = transition.namespace_id
         AND echo.echo_id = transition.echo_id
        JOIN death_events AS trigger_death
          ON trigger_death.namespace_id = transition.namespace_id
         AND trigger_death.death_id = transition.trigger_death_id
        WHERE transition.previous_state = 0 AND transition.next_state = 1
          AND echo.account_id <> trigger_death.account_id
    ) THEN
        RAISE EXCEPTION '0042 found an Echo promotion triggered by another account';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM death_outbox_events AS outbox
        WHERE outbox.event_type = 'echo_promoted'
          AND NOT EXISTS (
              SELECT 1
              FROM echo_records AS echo
              JOIN echo_state_transitions AS transition
                ON transition.namespace_id = echo.namespace_id
               AND transition.echo_id = echo.echo_id
               AND transition.transition_ordinal = outbox.echo_transition_ordinal
              JOIN death_events AS trigger_death
                ON trigger_death.namespace_id = outbox.namespace_id
               AND trigger_death.death_id = outbox.trigger_death_id
              WHERE echo.namespace_id = outbox.namespace_id
                AND echo.echo_id = outbox.echo_id
                AND transition.trigger_death_id = outbox.trigger_death_id
                AND echo.account_id = trigger_death.account_id
          )
    ) THEN
        RAISE EXCEPTION '0042 found an Echo promotion outbox with mismatched trigger authority';
    END IF;
END
$$;

CREATE FUNCTION enforce_echo_promotion_trigger_account_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    echo_account_id BYTEA;
    trigger_account_id BYTEA;
BEGIN
    IF NEW.previous_state IS DISTINCT FROM 0 OR NEW.next_state IS DISTINCT FROM 1 THEN
        RETURN NULL;
    END IF;

    SELECT echo.account_id, trigger_death.account_id
    INTO echo_account_id, trigger_account_id
    FROM echo_records AS echo
    JOIN death_events AS trigger_death
      ON trigger_death.namespace_id = echo.namespace_id
     AND trigger_death.death_id = NEW.trigger_death_id
    WHERE echo.namespace_id = NEW.namespace_id AND echo.echo_id = NEW.echo_id;

    IF NOT FOUND OR echo_account_id IS DISTINCT FROM trigger_account_id THEN
        RAISE EXCEPTION 'Echo promotion trigger death must belong to the target Echo account';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER echo_promotion_trigger_account_exact
AFTER INSERT ON echo_state_transitions
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_echo_promotion_trigger_account_v1();

CREATE FUNCTION enforce_echo_promotion_outbox_trigger_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    echo_account_id BYTEA;
    trigger_account_id BYTEA;
    transition_trigger_death_id BYTEA;
BEGIN
    IF NEW.event_type <> 'echo_promoted' THEN RETURN NULL; END IF;

    SELECT echo.account_id, trigger_death.account_id, transition.trigger_death_id
    INTO echo_account_id, trigger_account_id, transition_trigger_death_id
    FROM echo_records AS echo
    JOIN echo_state_transitions AS transition
      ON transition.namespace_id = echo.namespace_id
     AND transition.echo_id = echo.echo_id
     AND transition.transition_ordinal = NEW.echo_transition_ordinal
    JOIN death_events AS trigger_death
      ON trigger_death.namespace_id = echo.namespace_id
     AND trigger_death.death_id = NEW.trigger_death_id
    WHERE echo.namespace_id = NEW.namespace_id AND echo.echo_id = NEW.echo_id;

    IF NOT FOUND
        OR NEW.trigger_death_id IS DISTINCT FROM transition_trigger_death_id
        OR echo_account_id IS DISTINCT FROM trigger_account_id
    THEN
        RAISE EXCEPTION
            'Echo promotion outbox trigger must match its transition and target Echo account';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER echo_promotion_outbox_trigger_exact
AFTER INSERT ON death_outbox_events
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_echo_promotion_outbox_trigger_v1();

COMMENT ON FUNCTION enforce_echo_promotion_trigger_account_v1() IS
    'Requires every Dormant-to-Available trigger death to belong to the target Echo account.';
COMMENT ON FUNCTION enforce_echo_promotion_outbox_trigger_v1() IS
    'Binds Echo promotion outbox trigger authority to its transition and target Echo account.';
