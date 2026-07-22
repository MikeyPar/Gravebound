-- GB-M03-09 durable telemetry-domain sources.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md TECH-123 and TEL-001..005;
-- - Gravebound_Content_Production_Spec_v1.md CONT-002 and the exact Core stable IDs;
-- - Gravebound_Development_Roadmap_v1.md ADR-005 and GB-M03-09.
--
-- Published migrations 0001-0069 remain immutable. These rows are committed domain facts, not
-- analytics decisions: onboarding is projected by triggers in the owning gameplay transaction,
-- while session/crash rows are written only by their typed persistence repository. Export may
-- advance `published_at` once, but can never rewrite or delete source history.

CREATE FUNCTION m03_telemetry_tags_are_canonical_v1(tags TEXT[])
RETURNS BOOLEAN
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
    previous TEXT := NULL;
    current_tag TEXT;
BEGIN
    IF cardinality(tags) > 16 THEN
        RETURN FALSE;
    END IF;
    FOREACH current_tag IN ARRAY tags LOOP
        IF current_tag IS NULL
            OR octet_length(current_tag) NOT BETWEEN 1 AND 96
            OR current_tag !~ '^[a-z][a-z0-9._-]*$'
            OR (previous IS NOT NULL AND previous >= current_tag) THEN
            RETURN FALSE;
        END IF;
        previous := current_tag;
    END LOOP;
    RETURN TRUE;
END
$$;

CREATE FUNCTION derive_m03_telemetry_event_id_v1(domain_id TEXT, source_identity BYTEA)
RETURNS BYTEA
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
    derived BYTEA;
BEGIN
    IF domain_id !~ '^gravebound[.]telemetry[.][a-z0-9._-]+[.]v1$'
        OR octet_length(source_identity) < 1 THEN
        RAISE EXCEPTION 'invalid M03 telemetry event identity material';
    END IF;
    derived := decode(md5(domain_id || ':' || encode(source_identity, 'hex')), 'hex');
    IF derived = decode(repeat('00', 16), 'hex') THEN
        RAISE EXCEPTION 'derived M03 telemetry event identity is zero';
    END IF;
    RETURN derived;
END
$$;

CREATE TABLE core_telemetry_sessions_v1 (
    namespace_id TEXT NOT NULL REFERENCES gravebound_namespaces(namespace_id),
    session_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    build_id TEXT NOT NULL,
    content_bundle_version TEXT NOT NULL,
    platform SMALLINT NOT NULL,
    region_id TEXT NOT NULL,
    environment SMALLINT NOT NULL,
    cohort_tags TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    end_reason SMALLINT,
    PRIMARY KEY (namespace_id, session_id),
    UNIQUE (namespace_id, account_id, session_id),
    CONSTRAINT telemetry_session_id_exact_v1 CHECK (
        octet_length(session_id) = 16
        AND session_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT telemetry_session_account_exact_v1 CHECK (
        octet_length(account_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT telemetry_session_build_bounded_v1 CHECK (
        octet_length(build_id) BETWEEN 1 AND 96
        AND build_id ~ '^[a-z][a-z0-9._-]*$'
    ),
    CONSTRAINT telemetry_session_content_bounded_v1 CHECK (
        octet_length(content_bundle_version) BETWEEN 1 AND 128
        AND content_bundle_version ~ '^[a-z][a-z0-9._-]*$'
    ),
    CONSTRAINT telemetry_session_platform_known_v1 CHECK (platform BETWEEN 0 AND 3),
    CONSTRAINT telemetry_session_region_bounded_v1 CHECK (
        octet_length(region_id) BETWEEN 1 AND 96
        AND region_id ~ '^[a-z][a-z0-9._-]*$'
    ),
    CONSTRAINT telemetry_session_environment_known_v1 CHECK (environment BETWEEN 0 AND 3),
    CONSTRAINT telemetry_session_cohorts_canonical_v1 CHECK (
        m03_telemetry_tags_are_canonical_v1(cohort_tags)
    ),
    CONSTRAINT telemetry_session_end_shape_v1 CHECK (
        (ended_at IS NULL AND end_reason IS NULL)
        OR (ended_at >= started_at AND end_reason BETWEEN 0 AND 4)
    )
);

CREATE UNIQUE INDEX one_open_core_telemetry_session_per_account_v1
    ON core_telemetry_sessions_v1(namespace_id, account_id)
    WHERE ended_at IS NULL;

CREATE TABLE onboarding_outbox_events_v1 (
    namespace_id TEXT NOT NULL,
    event_id BYTEA NOT NULL,
    event_kind SMALLINT NOT NULL,
    source_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA,
    session_id BYTEA NOT NULL,
    class_id TEXT,
    source_content_id TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    FOREIGN KEY (namespace_id, account_id)
        REFERENCES accounts(namespace_id, account_id) ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, account_id, session_id)
        REFERENCES core_telemetry_sessions_v1(namespace_id, account_id, session_id)
        ON DELETE RESTRICT,
    CONSTRAINT onboarding_event_id_exact_v1 CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT onboarding_source_id_exact_v1 CHECK (
        octet_length(source_id) = 16
        AND source_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT onboarding_account_id_exact_v1 CHECK (octet_length(account_id) = 16),
    CONSTRAINT onboarding_character_id_exact_v1 CHECK (
        character_id IS NULL OR octet_length(character_id) = 16
    ),
    CONSTRAINT onboarding_event_kind_known_v1 CHECK (event_kind BETWEEN 0 AND 2),
    CONSTRAINT onboarding_class_bounded_v1 CHECK (
        class_id IS NULL OR (
            octet_length(class_id) BETWEEN 3 AND 96
            AND class_id ~ '^[a-z][a-z0-9._-]*$'
        )
    ),
    CONSTRAINT onboarding_source_content_bounded_v1 CHECK (
        source_content_id IS NULL OR (
            octet_length(source_content_id) BETWEEN 3 AND 96
            AND source_content_id ~ '^[a-z][a-z0-9._-]*$'
        )
    ),
    CONSTRAINT onboarding_event_shape_v1 CHECK (
        (event_kind = 0 AND character_id IS NULL AND source_id = account_id
            AND class_id IS NULL AND source_content_id IS NULL)
        OR (event_kind = 1 AND character_id IS NOT NULL AND source_id = character_id
            AND class_id IS NOT NULL AND source_content_id IS NULL)
        OR (event_kind = 2 AND character_id IS NOT NULL AND source_id = character_id
            AND class_id IS NOT NULL AND source_content_id IS NOT NULL)
    ),
    CONSTRAINT onboarding_publish_order_v1 CHECK (
        published_at IS NULL OR published_at >= created_at
    )
);

CREATE UNIQUE INDEX one_account_created_telemetry_event_v1
    ON onboarding_outbox_events_v1(namespace_id, account_id, event_kind)
    WHERE event_kind = 0;
CREATE UNIQUE INDEX one_character_created_telemetry_event_v1
    ON onboarding_outbox_events_v1(namespace_id, account_id, character_id, event_kind)
    WHERE event_kind = 1;
CREATE UNIQUE INDEX one_character_combat_telemetry_event_v1
    ON onboarding_outbox_events_v1(namespace_id, account_id, character_id, event_kind)
    WHERE event_kind = 2;
CREATE INDEX unpublished_onboarding_telemetry_events_v1
    ON onboarding_outbox_events_v1(namespace_id, created_at, event_id)
    WHERE published_at IS NULL;

CREATE TABLE session_outbox_events_v1 (
    namespace_id TEXT NOT NULL,
    event_id BYTEA NOT NULL,
    source_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    session_id BYTEA NOT NULL,
    event_sequence BIGINT NOT NULL,
    event_kind SMALLINT NOT NULL,
    duration_millis BIGINT,
    end_reason SMALLINT,
    link_lost_millis BIGINT,
    occurred_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, session_id, event_sequence),
    UNIQUE (namespace_id, session_id, source_id),
    FOREIGN KEY (namespace_id, account_id, session_id)
        REFERENCES core_telemetry_sessions_v1(namespace_id, account_id, session_id)
        ON DELETE RESTRICT,
    CONSTRAINT session_outbox_event_id_exact_v1 CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT session_outbox_source_id_exact_v1 CHECK (
        octet_length(source_id) = 16
        AND source_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT session_outbox_account_id_exact_v1 CHECK (octet_length(account_id) = 16),
    CONSTRAINT session_outbox_sequence_positive_v1 CHECK (event_sequence > 0),
    CONSTRAINT session_outbox_kind_known_v1 CHECK (event_kind BETWEEN 0 AND 3),
    CONSTRAINT session_outbox_shape_v1 CHECK (
        (event_kind = 0 AND event_sequence = 1
            AND duration_millis IS NULL AND end_reason IS NULL AND link_lost_millis IS NULL)
        OR (event_kind = 1 AND duration_millis IS NOT NULL AND duration_millis >= 0
            AND end_reason IS NOT NULL AND end_reason BETWEEN 0 AND 4
            AND link_lost_millis IS NULL)
        OR (event_kind = 2 AND duration_millis IS NULL
            AND end_reason IS NULL AND link_lost_millis IS NULL)
        OR (event_kind = 3 AND duration_millis IS NULL
            AND end_reason IS NULL AND link_lost_millis IS NOT NULL
            AND link_lost_millis >= 0)
    ),
    CONSTRAINT session_outbox_publish_order_v1 CHECK (
        published_at IS NULL OR published_at >= created_at
    )
);

CREATE UNIQUE INDEX one_session_started_telemetry_event_v1
    ON session_outbox_events_v1(namespace_id, session_id, event_kind)
    WHERE event_kind = 0;
CREATE UNIQUE INDEX one_session_ended_telemetry_event_v1
    ON session_outbox_events_v1(namespace_id, session_id, event_kind)
    WHERE event_kind = 1;
CREATE INDEX unpublished_session_telemetry_events_v1
    ON session_outbox_events_v1(namespace_id, created_at, event_id)
    WHERE published_at IS NULL;

CREATE TABLE crash_outbox_events_v1 (
    namespace_id TEXT NOT NULL,
    event_id BYTEA NOT NULL,
    crash_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA,
    session_id BYTEA NOT NULL,
    crash_source SMALLINT NOT NULL,
    crash_kind SMALLINT NOT NULL,
    reporter_kind SMALLINT NOT NULL,
    signature BYTEA NOT NULL,
    uptime_millis BIGINT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, crash_id),
    FOREIGN KEY (namespace_id, account_id, session_id)
        REFERENCES core_telemetry_sessions_v1(namespace_id, account_id, session_id)
        ON DELETE RESTRICT,
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE RESTRICT,
    CONSTRAINT crash_event_id_exact_v1 CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT crash_id_exact_v1 CHECK (
        octet_length(crash_id) = 16
        AND crash_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT crash_account_id_exact_v1 CHECK (octet_length(account_id) = 16),
    CONSTRAINT crash_character_id_exact_v1 CHECK (
        character_id IS NULL OR octet_length(character_id) = 16
    ),
    CONSTRAINT crash_source_known_v1 CHECK (crash_source BETWEEN 0 AND 1),
    CONSTRAINT crash_kind_known_v1 CHECK (crash_kind BETWEEN 0 AND 4),
    CONSTRAINT crash_reporter_known_v1 CHECK (reporter_kind BETWEEN 0 AND 2),
    CONSTRAINT crash_reporter_source_shape_v1 CHECK (
        (crash_source = 0 AND reporter_kind IN (1, 2))
        OR (crash_source = 1 AND reporter_kind IN (0, 2))
    ),
    CONSTRAINT crash_signature_exact_v1 CHECK (
        octet_length(signature) = 32
        AND signature <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT crash_uptime_nonnegative_v1 CHECK (uptime_millis >= 0),
    CONSTRAINT crash_publish_order_v1 CHECK (
        published_at IS NULL OR published_at >= created_at
    )
);

CREATE INDEX unpublished_crash_telemetry_events_v1
    ON crash_outbox_events_v1(namespace_id, created_at, event_id)
    WHERE published_at IS NULL;

CREATE FUNCTION project_account_created_telemetry_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    active_session BYTEA;
BEGIN
    SELECT session_id INTO active_session
    FROM core_telemetry_sessions_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND ended_at IS NULL;
    IF active_session IS NULL THEN
        RETURN NEW;
    END IF;
    INSERT INTO onboarding_outbox_events_v1 (
        namespace_id, event_id, event_kind, source_id, account_id, session_id, occurred_at
    ) VALUES (
        NEW.namespace_id,
        derive_m03_telemetry_event_id_v1(
            'gravebound.telemetry.account-created.v1', NEW.account_id
        ),
        0, NEW.account_id, NEW.account_id, active_session, NEW.created_at
    ) ON CONFLICT DO NOTHING;
    RETURN NEW;
END
$$;

CREATE TRIGGER account_created_telemetry_projection_v1
AFTER INSERT ON accounts
FOR EACH ROW EXECUTE FUNCTION project_account_created_telemetry_v1();

CREATE FUNCTION project_character_created_telemetry_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    active_session BYTEA;
BEGIN
    SELECT session_id INTO active_session
    FROM core_telemetry_sessions_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND ended_at IS NULL;
    IF active_session IS NULL THEN
        RETURN NEW;
    END IF;
    INSERT INTO onboarding_outbox_events_v1 (
        namespace_id, event_id, event_kind, source_id, account_id, character_id,
        session_id, class_id, occurred_at
    ) VALUES (
        NEW.namespace_id,
        derive_m03_telemetry_event_id_v1(
            'gravebound.telemetry.character-created.v1', NEW.character_id
        ),
        1, NEW.character_id, NEW.account_id, NEW.character_id,
        active_session, NEW.class_id, NEW.created_at
    ) ON CONFLICT DO NOTHING;
    RETURN NEW;
END
$$;

CREATE TRIGGER character_created_telemetry_projection_v1
AFTER INSERT ON characters
FOR EACH ROW EXECUTE FUNCTION project_character_created_telemetry_v1();

CREATE FUNCTION project_character_entered_combat_telemetry_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    active_session BYTEA;
    character_class TEXT;
BEGIN
    IF NEW.location_kind <> 2 THEN
        RETURN NEW;
    END IF;
    IF TG_OP = 'UPDATE' AND OLD.location_kind = 2 THEN
        RETURN NEW;
    END IF;
    SELECT session_id INTO active_session
    FROM core_telemetry_sessions_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND ended_at IS NULL;
    IF active_session IS NULL THEN
        RETURN NEW;
    END IF;
    SELECT class_id INTO STRICT character_class
    FROM characters
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND character_id = NEW.character_id;
    INSERT INTO onboarding_outbox_events_v1 (
        namespace_id, event_id, event_kind, source_id, account_id, character_id,
        session_id, class_id, source_content_id, occurred_at
    ) VALUES (
        NEW.namespace_id,
        derive_m03_telemetry_event_id_v1(
            'gravebound.telemetry.character-entered-combat.v1', NEW.character_id
        ),
        2, NEW.character_id, NEW.account_id, NEW.character_id,
        active_session, character_class, NEW.location_content_id, NEW.updated_at
    ) ON CONFLICT DO NOTHING;
    RETURN NEW;
END
$$;

CREATE TRIGGER character_entered_combat_telemetry_projection_v1
AFTER INSERT OR UPDATE OF location_kind ON character_world_locations
FOR EACH ROW EXECUTE FUNCTION project_character_entered_combat_telemetry_v1();

CREATE FUNCTION enforce_m03_telemetry_outbox_immutability_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    accepted_at TIMESTAMPTZ;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'M03 telemetry source history is immutable';
    END IF;
    IF OLD.published_at IS NOT NULL
        OR NEW.published_at IS NULL
        OR NEW.published_at < OLD.created_at THEN
        RAISE EXCEPTION 'M03 telemetry publication may advance exactly once';
    END IF;
    accepted_at := NEW.published_at;
    NEW.published_at := OLD.published_at;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'M03 telemetry source payload is immutable';
    END IF;
    NEW.published_at := accepted_at;
    RETURN NEW;
END
$$;

CREATE TRIGGER onboarding_outbox_immutable_v1
BEFORE UPDATE OR DELETE ON onboarding_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_m03_telemetry_outbox_immutability_v1();
CREATE TRIGGER session_outbox_immutable_v1
BEFORE UPDATE OR DELETE ON session_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_m03_telemetry_outbox_immutability_v1();
CREATE TRIGGER crash_outbox_immutable_v1
BEFORE UPDATE OR DELETE ON crash_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_m03_telemetry_outbox_immutability_v1();

CREATE FUNCTION enforce_core_telemetry_session_transition_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    accepted_end TIMESTAMPTZ;
    accepted_reason SMALLINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Core telemetry session history is immutable';
    END IF;
    IF OLD.ended_at IS NOT NULL
        OR NEW.ended_at IS NULL
        OR NEW.end_reason IS NULL
        OR NEW.ended_at < OLD.started_at THEN
        RAISE EXCEPTION 'Core telemetry session may end exactly once';
    END IF;
    accepted_end := NEW.ended_at;
    accepted_reason := NEW.end_reason;
    NEW.ended_at := OLD.ended_at;
    NEW.end_reason := OLD.end_reason;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Core telemetry session context is immutable';
    END IF;
    NEW.ended_at := accepted_end;
    NEW.end_reason := accepted_reason;
    RETURN NEW;
END
$$;

CREATE TRIGGER core_telemetry_session_transition_v1
BEFORE UPDATE OR DELETE ON core_telemetry_sessions_v1
FOR EACH ROW EXECUTE FUNCTION enforce_core_telemetry_session_transition_v1();

COMMENT ON TABLE core_telemetry_sessions_v1 IS
    'GB-M03-09 durable logical session and immutable TEL-001 origin context; no analytics decisions.';
COMMENT ON TABLE onboarding_outbox_events_v1 IS
    'GB-M03-09 account/character/first-combat facts projected inside owning gameplay transactions.';
COMMENT ON TABLE session_outbox_events_v1 IS
    'GB-M03-09 append-only started/ended/disconnected/reconnected session facts.';
COMMENT ON TABLE crash_outbox_events_v1 IS
    'GB-M03-09 typed redacted crash observations; raw diagnostics are structurally absent.';
COMMENT ON FUNCTION derive_m03_telemetry_event_id_v1(TEXT, BYTEA) IS
    'Deterministic domain-separated idempotency identity only; never a security or gameplay hash.';

-- Recovery/downgrade: export and runtime integration must be disabled first. Because Core is a
-- wipeable namespace, reset its test data before removing schema 0070. Never reinterpret these
-- typed rows, reuse an event ID for changed material, or backfill origin context by guessing.
