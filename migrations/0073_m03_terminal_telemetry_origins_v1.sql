-- GB-M03-09 immutable terminal telemetry origin binding.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-001/010/011, TECH-021..023,
--   TECH-123, and TEL-001..005;
-- - Gravebound_Content_Production_Spec_v1.md CONT-CATALOG-003,
--   CONT-ROOM-007, CONT-BOSS-001, and the exact Core stable IDs;
-- - Gravebound_Development_Roadmap_v1.md ADR-005 and GB-M03-06..09.
--
-- Terminal gameplay outboxes remain the only outcome authority. This additive
-- migration binds only newly inserted committed death, extraction, Recall, and
-- successor facts to the one durable PostgreSQL-authored session-outbox
-- interval covering their transaction. Application-authored observation times
-- never participate in origin selection. Missing, overlapping, or unavailable
-- telemetry context leaves the origin NULL and must never reject the gameplay
-- transaction. Pre-0073 history is deliberately not backfilled because its
-- session cannot be known.

ALTER TABLE death_outbox_events
    ADD COLUMN origin_account_id BYTEA,
    ADD COLUMN origin_session_id BYTEA,
    ADD CONSTRAINT death_telemetry_origin_shape_v1 CHECK (
        (origin_account_id IS NULL AND origin_session_id IS NULL)
        OR (
            octet_length(origin_account_id) = 16
            AND origin_account_id <> decode(repeat('00', 16), 'hex')
            AND octet_length(origin_session_id) = 16
            AND origin_session_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT death_telemetry_origin_session_owned_v1 FOREIGN KEY (
        namespace_id, origin_account_id, origin_session_id
    ) REFERENCES core_telemetry_sessions_v1(
        namespace_id, account_id, session_id
    ) ON DELETE RESTRICT;

ALTER TABLE extraction_terminal_outbox_events_v1
    ADD COLUMN origin_session_id BYTEA,
    ADD CONSTRAINT extraction_telemetry_origin_session_exact_v1 CHECK (
        origin_session_id IS NULL
        OR (
            octet_length(origin_session_id) = 16
            AND origin_session_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT extraction_telemetry_origin_session_owned_v1 FOREIGN KEY (
        namespace_id, account_id, origin_session_id
    ) REFERENCES core_telemetry_sessions_v1(
        namespace_id, account_id, session_id
    ) ON DELETE RESTRICT;

ALTER TABLE recall_terminal_outbox_events_v1
    ADD COLUMN origin_session_id BYTEA,
    ADD CONSTRAINT recall_telemetry_origin_session_exact_v1 CHECK (
        origin_session_id IS NULL
        OR (
            octet_length(origin_session_id) = 16
            AND origin_session_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT recall_telemetry_origin_session_owned_v1 FOREIGN KEY (
        namespace_id, account_id, origin_session_id
    ) REFERENCES core_telemetry_sessions_v1(
        namespace_id, account_id, session_id
    ) ON DELETE RESTRICT;

ALTER TABLE successor_mutation_outbox_events_v1
    ADD COLUMN origin_session_id BYTEA,
    ADD CONSTRAINT successor_telemetry_origin_session_exact_v1 CHECK (
        origin_session_id IS NULL
        OR (
            octet_length(origin_session_id) = 16
            AND origin_session_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT successor_telemetry_origin_session_owned_v1 FOREIGN KEY (
        namespace_id, account_id, origin_session_id
    ) REFERENCES core_telemetry_sessions_v1(
        namespace_id, account_id, session_id
    ) ON DELETE RESTRICT;

CREATE FUNCTION m03_death_item_power_band_v1(
    target_namespace_id TEXT,
    target_death_id BYTEA
)
RETURNS SMALLINT
LANGUAGE sql
STABLE
STRICT
AS $$
    SELECT CASE
        WHEN computed.power_index < 90 THEN 1
        WHEN computed.power_index < 120 THEN 2
        WHEN computed.power_index < 150 THEN 3
        WHEN computed.power_index < 180 THEN 4
        ELSE 5
    END::SMALLINT
    FROM (
        SELECT (summary.level * 10 + (
            35 * COALESCE(max(
                item.item_level * 10 + CASE item.rarity
                    WHEN 0 THEN 0 WHEN 1 THEN 5 WHEN 2 THEN 10
                    WHEN 3 THEN 20 WHEN 4 THEN 30 WHEN 5 THEN 30
                END
            ) FILTER (WHERE destroyed.pre_slot_index = 0), 0)
            + 25 * COALESCE(max(
                item.item_level * 10 + CASE item.rarity
                    WHEN 0 THEN 0 WHEN 1 THEN 5 WHEN 2 THEN 10
                    WHEN 3 THEN 20 WHEN 4 THEN 30 WHEN 5 THEN 30
                END
            ) FILTER (WHERE destroyed.pre_slot_index = 1), 0)
            + 25 * COALESCE(max(
                item.item_level * 10 + CASE item.rarity
                    WHEN 0 THEN 0 WHEN 1 THEN 5 WHEN 2 THEN 10
                    WHEN 3 THEN 20 WHEN 4 THEN 30 WHEN 5 THEN 30
                END
            ) FILTER (WHERE destroyed.pre_slot_index = 2), 0)
            + 15 * COALESCE(max(
                item.item_level * 10 + CASE item.rarity
                    WHEN 0 THEN 0 WHEN 1 THEN 5 WHEN 2 THEN 10
                    WHEN 3 THEN 20 WHEN 4 THEN 30 WHEN 5 THEN 30
                END
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
        WHERE summary.namespace_id = target_namespace_id
          AND summary.death_id = target_death_id
        GROUP BY summary.level
    ) AS computed
$$;

CREATE FUNCTION resolve_m03_terminal_telemetry_session_v1(
    target_namespace_id TEXT,
    target_account_id BYTEA,
    target_occurred_at TIMESTAMPTZ
)
RETURNS BYTEA
LANGUAGE plpgsql
STABLE
STRICT
AS $$
DECLARE
    eligible_sessions BYTEA[];
BEGIN
    -- This bounded MVCC read takes no row lock and does not wait for session
    -- shutdown. Session interval boundaries and target_occurred_at all come
    -- from PostgreSQL transaction_timestamp(), so application clock skew cannot
    -- relabel a terminal. Exact single-interval coverage is required. An overlap
    -- or gap is telemetry ambiguity, never a gameplay failure.
    SELECT array_agg(candidate.session_id ORDER BY candidate.session_id)
      INTO eligible_sessions
    FROM (
        SELECT session.session_id
        FROM core_telemetry_sessions_v1 AS session
        JOIN session_outbox_events_v1 AS started
          ON started.namespace_id = session.namespace_id
         AND started.account_id = session.account_id
         AND started.session_id = session.session_id
         AND started.event_kind = 0
        LEFT JOIN session_outbox_events_v1 AS ended
          ON ended.namespace_id = session.namespace_id
         AND ended.account_id = session.account_id
         AND ended.session_id = session.session_id
         AND ended.event_kind = 1
        WHERE session.namespace_id = target_namespace_id
          AND session.account_id = target_account_id
          AND started.created_at <= target_occurred_at
          AND (ended.created_at IS NULL OR target_occurred_at < ended.created_at)
        ORDER BY session.session_id
        LIMIT 2
    ) AS candidate;
    IF cardinality(eligible_sessions) IS DISTINCT FROM 1 THEN
        RETURN NULL;
    END IF;
    RETURN eligible_sessions[1];
EXCEPTION
    WHEN OTHERS THEN
        RETURN NULL;
END
$$;

CREATE FUNCTION bind_death_terminal_telemetry_origin_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    source_account_id BYTEA;
    source_committed_at TIMESTAMPTZ;
BEGIN
    NEW.origin_account_id := NULL;
    NEW.origin_session_id := NULL;
    IF NEW.event_type <> 'death_committed' THEN
        RETURN NEW;
    END IF;
    SELECT account_id, committed_at
      INTO source_account_id, source_committed_at
    FROM death_events
    WHERE namespace_id = NEW.namespace_id
      AND death_id = NEW.death_id;
    IF source_account_id IS NULL OR source_committed_at IS NULL THEN
        RETURN NEW;
    END IF;
    NEW.origin_session_id := resolve_m03_terminal_telemetry_session_v1(
        NEW.namespace_id, source_account_id, source_committed_at
    );
    IF NEW.origin_session_id IS NOT NULL THEN
        NEW.origin_account_id := source_account_id;
    END IF;
    RETURN NEW;
EXCEPTION
    WHEN OTHERS THEN
        NEW.origin_account_id := NULL;
        NEW.origin_session_id := NULL;
        RETURN NEW;
END
$$;

CREATE FUNCTION bind_owned_terminal_telemetry_origin_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.origin_session_id := resolve_m03_terminal_telemetry_session_v1(
        NEW.namespace_id, NEW.account_id, NEW.created_at
    );
    RETURN NEW;
EXCEPTION
    WHEN OTHERS THEN
        NEW.origin_session_id := NULL;
        RETURN NEW;
END
$$;

CREATE TRIGGER death_terminal_telemetry_origin_v1
BEFORE INSERT ON death_outbox_events
FOR EACH ROW EXECUTE FUNCTION bind_death_terminal_telemetry_origin_v1();

CREATE TRIGGER extraction_terminal_telemetry_origin_v1
BEFORE INSERT ON extraction_terminal_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION bind_owned_terminal_telemetry_origin_v1();

CREATE TRIGGER recall_terminal_telemetry_origin_v1
BEFORE INSERT ON recall_terminal_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION bind_owned_terminal_telemetry_origin_v1();

CREATE TRIGGER successor_terminal_telemetry_origin_v1
BEFORE INSERT ON successor_mutation_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION bind_owned_terminal_telemetry_origin_v1();

CREATE FUNCTION enforce_death_telemetry_origin_immutable_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.origin_session_id IS DISTINCT FROM OLD.origin_session_id
        OR NEW.origin_account_id IS DISTINCT FROM OLD.origin_account_id
    THEN
        RAISE EXCEPTION 'terminal telemetry origin is immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE FUNCTION enforce_owned_terminal_telemetry_origin_immutable_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.origin_session_id IS DISTINCT FROM OLD.origin_session_id THEN
        RAISE EXCEPTION 'terminal telemetry origin is immutable';
    END IF;
    RETURN NEW;
END
$$;

-- PostgreSQL fires same-kind triggers by name. The `a_` prefix ensures this
-- origin-specific guard identifies an origin rewrite before the older generic
-- publish-only guard; legitimate first publication then continues to that
-- owning guard unchanged.
CREATE TRIGGER a_death_telemetry_origin_immutable_v1
BEFORE UPDATE ON death_outbox_events
FOR EACH ROW EXECUTE FUNCTION enforce_death_telemetry_origin_immutable_v1();

CREATE TRIGGER a_extraction_telemetry_origin_immutable_v1
BEFORE UPDATE ON extraction_terminal_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_owned_terminal_telemetry_origin_immutable_v1();

CREATE TRIGGER a_recall_telemetry_origin_immutable_v1
BEFORE UPDATE ON recall_terminal_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_owned_terminal_telemetry_origin_immutable_v1();

CREATE TRIGGER a_successor_telemetry_origin_immutable_v1
BEFORE UPDATE ON successor_mutation_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_owned_terminal_telemetry_origin_immutable_v1();

CREATE INDEX unpublished_death_telemetry_origins_v1
    ON death_outbox_events(namespace_id, created_at, event_id)
    WHERE published_at IS NULL AND origin_session_id IS NOT NULL;

CREATE INDEX unpublished_extraction_telemetry_origins_v1
    ON extraction_terminal_outbox_events_v1(namespace_id, created_at, event_id)
    WHERE published_at IS NULL AND origin_session_id IS NOT NULL;

CREATE INDEX unpublished_recall_telemetry_origins_v1
    ON recall_terminal_outbox_events_v1(namespace_id, created_at, event_id)
    WHERE published_at IS NULL AND origin_session_id IS NOT NULL;

CREATE INDEX unpublished_successor_telemetry_origins_v1
    ON successor_mutation_outbox_events_v1(namespace_id, created_at, event_id)
    WHERE published_at IS NULL AND origin_session_id IS NOT NULL;

COMMENT ON COLUMN death_outbox_events.origin_session_id IS
    'GB-M03-09 immutable origin session; NULL means telemetry attribution was unavailable or ambiguous.';
COMMENT ON COLUMN death_outbox_events.origin_account_id IS
    'GB-M03-09 immutable account side of the captured origin-session foreign key; never exported raw.';
COMMENT ON COLUMN extraction_terminal_outbox_events_v1.origin_session_id IS
    'GB-M03-09 immutable origin session; NULL means telemetry attribution was unavailable or ambiguous.';
COMMENT ON COLUMN recall_terminal_outbox_events_v1.origin_session_id IS
    'GB-M03-09 immutable origin session; NULL means telemetry attribution was unavailable or ambiguous.';
COMMENT ON COLUMN successor_mutation_outbox_events_v1.origin_session_id IS
    'GB-M03-09 immutable origin session; NULL means telemetry attribution was unavailable or ambiguous.';

-- Recovery/downgrade: disable terminal telemetry polling before removing schema
-- 0073. Core remains wipeable; reset test data first. Never populate a NULL
-- origin from a later live session, rewrite terminal history, or reinterpret a
-- pre-0073 terminal result as if its original session were known.
