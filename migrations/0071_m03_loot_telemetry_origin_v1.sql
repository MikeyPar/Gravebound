-- GB-M03-09 immutable item-ledger telemetry origin binding.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md LOOT-010, ECO-002, TECH-123,
--   and TEL-001..005;
-- - Gravebound_Content_Production_Spec_v1.md CONT-REWARD-001..004 and the
--   exact Core stable IDs;
-- - Gravebound_Development_Roadmap_v1.md ADR-005 and GB-M03-09.
--
-- The item ledger remains the gameplay writer and lifecycle authority. This
-- sidecar is projected by the same transaction, stores the exact session and
-- immutable item/source facts that existed at ledger commit, and may advance
-- only its delivery marker after exporter acceptance. Existing ledger history
-- is deliberately not backfilled because its origin session cannot be known.

CREATE FUNCTION derive_m03_loot_telemetry_event_id_v1(
    loot_action SMALLINT,
    ledger_identity BYTEA
)
RETURNS BYTEA
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
    domain_id TEXT;
BEGIN
    domain_id := CASE loot_action
        WHEN 0 THEN 'gravebound.telemetry.item-created.v1'
        WHEN 1 THEN 'gravebound.telemetry.item-picked-up.v1'
        WHEN 2 THEN 'gravebound.telemetry.item-equipped.v1'
        WHEN 3 THEN 'gravebound.telemetry.item-extracted.v1'
        WHEN 4 THEN 'gravebound.telemetry.item-destroyed.v1'
        ELSE NULL
    END;
    IF domain_id IS NULL OR octet_length(ledger_identity) <> 16 THEN
        RAISE EXCEPTION 'invalid M03 loot telemetry identity material';
    END IF;
    RETURN derive_m03_telemetry_event_id_v1(domain_id, ledger_identity);
END
$$;

CREATE TABLE item_ledger_telemetry_outbox_v1 (
    namespace_id TEXT NOT NULL,
    event_id BYTEA NOT NULL,
    ledger_event_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    session_id BYTEA NOT NULL,
    loot_action SMALLINT NOT NULL,
    item_uid BYTEA NOT NULL,
    template_id TEXT NOT NULL,
    source_content_id TEXT NOT NULL,
    item_version BIGINT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, ledger_event_id, loot_action),
    FOREIGN KEY (namespace_id, ledger_event_id)
        REFERENCES item_ledger_events(namespace_id, ledger_event_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, session_id)
        REFERENCES core_telemetry_sessions_v1(namespace_id, account_id, session_id)
        ON DELETE CASCADE,
    CONSTRAINT item_ledger_telemetry_event_id_exact_v1 CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
        AND event_id = derive_m03_loot_telemetry_event_id_v1(
            loot_action, ledger_event_id
        )
    ),
    CONSTRAINT item_ledger_telemetry_ledger_id_exact_v1 CHECK (
        octet_length(ledger_event_id) = 16
        AND ledger_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT item_ledger_telemetry_account_id_exact_v1 CHECK (
        octet_length(account_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT item_ledger_telemetry_character_id_exact_v1 CHECK (
        octet_length(character_id) = 16
        AND character_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT item_ledger_telemetry_session_id_exact_v1 CHECK (
        octet_length(session_id) = 16
        AND session_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT item_ledger_telemetry_action_known_v1 CHECK (
        loot_action BETWEEN 0 AND 4
    ),
    CONSTRAINT item_ledger_telemetry_item_id_exact_v1 CHECK (
        octet_length(item_uid) = 16
        AND item_uid <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT item_ledger_telemetry_template_bounded_v1 CHECK (
        octet_length(template_id) BETWEEN 3 AND 96
        AND template_id ~ '^[a-z][a-z0-9._-]*$'
    ),
    CONSTRAINT item_ledger_telemetry_source_bounded_v1 CHECK (
        octet_length(source_content_id) BETWEEN 3 AND 96
        AND source_content_id ~ '^[a-z][a-z0-9._-]*$'
    ),
    CONSTRAINT item_ledger_telemetry_version_positive_v1 CHECK (
        item_version > 0
    ),
    CONSTRAINT item_ledger_telemetry_publish_order_v1 CHECK (
        published_at IS NULL OR published_at >= created_at
    )
);

CREATE INDEX unpublished_item_ledger_telemetry_events_v1
    ON item_ledger_telemetry_outbox_v1(namespace_id, created_at, event_id)
    WHERE published_at IS NULL;

CREATE INDEX item_ledger_telemetry_session_origin_v1
    ON core_telemetry_sessions_v1(
        namespace_id, account_id, started_at, ended_at, session_id
    );

CREATE FUNCTION project_item_ledger_telemetry_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    active_session BYTEA;
    eligible_sessions BYTEA[];
    item_template TEXT;
    item_source TEXT;
BEGIN
    -- A plain bounded MVCC read never waits on session shutdown. The exact
    -- session whose durable interval covers the ledger transaction timestamp
    -- is required; absence or corrupt overlap cleanly disables this projection
    -- without affecting the item-ledger write. A later replacement session can
    -- therefore never relabel an older in-flight item mutation.
    SELECT array_agg(candidate.session_id ORDER BY candidate.session_id)
      INTO eligible_sessions
    FROM (
        SELECT session_id
        FROM core_telemetry_sessions_v1
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id
          AND started_at <= NEW.committed_at
          AND (ended_at IS NULL OR ended_at >= NEW.committed_at)
        ORDER BY session_id
        LIMIT 2
    ) AS candidate;
    IF cardinality(eligible_sessions) IS DISTINCT FROM 1 THEN
        RETURN NEW;
    END IF;
    active_session := eligible_sessions[1];

    SELECT item.template_id,
           CASE item.creation_kind
               WHEN 0 THEN 'starter.core-dev.v1'
               WHEN 1 THEN reward.reward_table_id
               ELSE NULL
           END
      INTO item_template, item_source
    FROM item_instances AS item
    LEFT JOIN reward_requests AS reward
      ON reward.namespace_id=item.namespace_id
     AND reward.reward_request_id=item.creation_request_id
     AND item.creation_kind=1
    WHERE item.namespace_id=NEW.namespace_id
      AND item.item_uid=NEW.item_uid;
    IF item_template IS NULL OR item_source IS NULL THEN
        RETURN NEW;
    END IF;

    IF NEW.event_kind = 0 THEN
        INSERT INTO item_ledger_telemetry_outbox_v1 (
            namespace_id,event_id,ledger_event_id,account_id,character_id,
            session_id,loot_action,item_uid,template_id,source_content_id,
            item_version,occurred_at
        ) VALUES (
            NEW.namespace_id,
            derive_m03_loot_telemetry_event_id_v1(0,NEW.ledger_event_id),
            NEW.ledger_event_id,NEW.account_id,NEW.character_id,active_session,
            0,NEW.item_uid,item_template,item_source,NEW.post_item_version,
            NEW.committed_at
        );
    END IF;

    IF NEW.event_kind = 1 AND NEW.pre_location_kind = 3 THEN
        INSERT INTO item_ledger_telemetry_outbox_v1 (
            namespace_id,event_id,ledger_event_id,account_id,character_id,
            session_id,loot_action,item_uid,template_id,source_content_id,
            item_version,occurred_at
        ) VALUES (
            NEW.namespace_id,
            derive_m03_loot_telemetry_event_id_v1(1,NEW.ledger_event_id),
            NEW.ledger_event_id,NEW.account_id,NEW.character_id,active_session,
            1,NEW.item_uid,item_template,item_source,NEW.post_item_version,
            NEW.committed_at
        );
    END IF;

    IF NEW.event_kind = 1
       AND NEW.post_location_kind = 0
       AND NEW.pre_location_kind <> 0 THEN
        INSERT INTO item_ledger_telemetry_outbox_v1 (
            namespace_id,event_id,ledger_event_id,account_id,character_id,
            session_id,loot_action,item_uid,template_id,source_content_id,
            item_version,occurred_at
        ) VALUES (
            NEW.namespace_id,
            derive_m03_loot_telemetry_event_id_v1(2,NEW.ledger_event_id),
            NEW.ledger_event_id,NEW.account_id,NEW.character_id,active_session,
            2,NEW.item_uid,item_template,item_source,NEW.post_item_version,
            NEW.committed_at
        );
    END IF;

    IF NEW.event_kind = 1 AND NEW.source_kind = 5 THEN
        INSERT INTO item_ledger_telemetry_outbox_v1 (
            namespace_id,event_id,ledger_event_id,account_id,character_id,
            session_id,loot_action,item_uid,template_id,source_content_id,
            item_version,occurred_at
        ) VALUES (
            NEW.namespace_id,
            derive_m03_loot_telemetry_event_id_v1(3,NEW.ledger_event_id),
            NEW.ledger_event_id,NEW.account_id,NEW.character_id,active_session,
            3,NEW.item_uid,item_template,item_source,NEW.post_item_version,
            NEW.committed_at
        );
    END IF;

    IF NEW.event_kind IN (2,3)
       OR (NEW.event_kind = 4 AND NEW.reason = 'crash_revoked') THEN
        INSERT INTO item_ledger_telemetry_outbox_v1 (
            namespace_id,event_id,ledger_event_id,account_id,character_id,
            session_id,loot_action,item_uid,template_id,source_content_id,
            item_version,occurred_at
        ) VALUES (
            NEW.namespace_id,
            derive_m03_loot_telemetry_event_id_v1(4,NEW.ledger_event_id),
            NEW.ledger_event_id,NEW.account_id,NEW.character_id,active_session,
            4,NEW.item_uid,item_template,item_source,NEW.post_item_version,
            NEW.committed_at
        );
    END IF;
    RETURN NEW;
EXCEPTION
    WHEN OTHERS THEN
        -- Telemetry is optional and can never reject the owning gameplay
        -- transaction. PostgreSQL rolls back only this trigger's side effects.
        RETURN NEW;
END
$$;

CREATE TRIGGER item_ledger_telemetry_projection_v1
AFTER INSERT ON item_ledger_events
FOR EACH ROW EXECUTE FUNCTION project_item_ledger_telemetry_v1();

CREATE FUNCTION enforce_item_ledger_telemetry_immutability_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    accepted_at TIMESTAMPTZ;
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'item-ledger telemetry source history is immutable';
    END IF;
    IF OLD.published_at IS NOT NULL
       OR NEW.published_at IS NULL
       OR NEW.published_at < OLD.created_at THEN
        RAISE EXCEPTION 'item-ledger telemetry publication may advance exactly once';
    END IF;
    accepted_at := NEW.published_at;
    NEW.published_at := OLD.published_at;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'item-ledger telemetry source payload is immutable';
    END IF;
    NEW.published_at := accepted_at;
    RETURN NEW;
END
$$;

CREATE TRIGGER item_ledger_telemetry_immutable_v1
BEFORE UPDATE OR DELETE ON item_ledger_telemetry_outbox_v1
FOR EACH ROW EXECUTE FUNCTION enforce_item_ledger_telemetry_immutability_v1();

COMMENT ON TABLE item_ledger_telemetry_outbox_v1 IS
    'GB-M03-09 immutable item-ledger facts with exact durable origin-session context; delivery marker only.';
COMMENT ON FUNCTION project_item_ledger_telemetry_v1() IS
    'Projects canonical item lifecycle facts in the owning gameplay transaction without authoring gameplay.';

-- Recovery/downgrade: disable telemetry polling first. Core is wipeable, so
-- reset test data before removing schema 0071. Never backfill origin_session_id
-- from a later live session or rewrite item-ledger history.
