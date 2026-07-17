-- GB-M03-07 successor insert-window relation guard.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-020/021 and TECH-021-023 require
--   atomic, replay-safe successor publication.
-- - Gravebound_Content_Production_Spec_v1.md CONT-CATALOG-003 requires the exact
--   successor starter grant to close with the stored result.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-07 requires retry-safe successor
--   creation with no duplicate character or grant.
-- - Accepted SPEC-CONFLICT-031 fixes result, audit, and outbox closure in one transaction.
--
-- Hosted run 29581539059 exposed the same PostgreSQL record-shape rule addressed by
-- migration 0041: a shared trigger function cannot safely reference an outbox-only NEW field
-- while executing for an audit row. This forward-only migration splits those relation-specific
-- insert windows. It changes no stored row, table, protocol byte, or gameplay authority.
--
-- Recovery: keep 0061 applied. Schema-60 binaries must not run against schema 61. Core remains
-- wipeable; rollback requires a pre-0061 database restore or wipe followed by the intended
-- migration set. Published migration 0060 is never rewritten.

CREATE OR REPLACE FUNCTION enforce_successor_result_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'INSERT' OR TG_TABLE_NAME <> 'successor_mutation_results_v1' THEN
        RAISE EXCEPTION 'successor result insert guard attached to unsupported operation or table';
    END IF;
    IF NEW.committed_at IS DISTINCT FROM transaction_timestamp()
        OR NOT EXISTS (
            SELECT 1 FROM successor_roster_reservations_v1
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND death_id = NEW.death_id
              AND former_roster_ordinal = NEW.former_roster_ordinal
              AND reservation_state = 0
        )
    THEN
        RAISE EXCEPTION 'successor result requires the current Active reservation';
    END IF;
    RETURN NEW;
END
$$;

CREATE FUNCTION enforce_successor_audit_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    result_time TIMESTAMPTZ;
BEGIN
    IF TG_OP <> 'INSERT' OR TG_TABLE_NAME <> 'successor_mutation_audit_events_v1' THEN
        RAISE EXCEPTION 'successor audit insert guard attached to unsupported operation or table';
    END IF;
    SELECT committed_at INTO result_time
    FROM successor_mutation_results_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND mutation_id = NEW.mutation_id;
    IF NOT FOUND
        OR result_time IS DISTINCT FROM transaction_timestamp()
        OR NEW.created_at IS DISTINCT FROM result_time
    THEN
        RAISE EXCEPTION 'successor audit may be inserted only with its successor result';
    END IF;
    RETURN NEW;
END
$$;

CREATE FUNCTION enforce_successor_outbox_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    result_time TIMESTAMPTZ;
BEGIN
    IF TG_OP <> 'INSERT' OR TG_TABLE_NAME <> 'successor_mutation_outbox_events_v1' THEN
        RAISE EXCEPTION 'successor outbox insert guard attached to unsupported operation or table';
    END IF;
    IF NEW.published_at IS NOT NULL THEN
        RAISE EXCEPTION 'successor outbox must be inserted unpublished';
    END IF;
    SELECT committed_at INTO result_time
    FROM successor_mutation_results_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND mutation_id = NEW.mutation_id;
    IF NOT FOUND
        OR result_time IS DISTINCT FROM transaction_timestamp()
        OR NEW.created_at IS DISTINCT FROM result_time
    THEN
        RAISE EXCEPTION 'successor outbox may be inserted only with its successor result';
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER successor_audit_insert_window_v1 ON successor_mutation_audit_events_v1;
CREATE TRIGGER successor_audit_insert_window_v1
BEFORE INSERT ON successor_mutation_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_audit_insert_window_v1();

DROP TRIGGER successor_outbox_insert_window_v1 ON successor_mutation_outbox_events_v1;
CREATE TRIGGER successor_outbox_insert_window_v1
BEFORE INSERT ON successor_mutation_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_outbox_insert_window_v1();

COMMENT ON FUNCTION enforce_successor_result_insert_window_v1() IS
    'GB-M03-07 result-only Active-reservation and PostgreSQL-time insert authority.';
COMMENT ON FUNCTION enforce_successor_audit_insert_window_v1() IS
    'GB-M03-07 audit-only same-transaction stored-result insert authority.';
COMMENT ON FUNCTION enforce_successor_outbox_insert_window_v1() IS
    'GB-M03-07 outbox-only unpublished same-transaction stored-result insert authority.';
