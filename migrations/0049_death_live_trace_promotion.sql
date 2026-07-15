-- GB-M03-02D / GB-M03-06B..06C retained live-trace promotion closure.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-001/DTH-020 and TECH-020..023;
-- - Gravebound_Content_Production_Spec_v1.md CONT-ECHO-001/009 and CONT-HUB-002;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-02/06/13 and the atomic-death gate;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md decisions 4/5.
--
-- Migrations 0043 and 0048 deliberately separate the prunable live payload from its retained
-- command/result receipts. This additive migration seals the exact retained receipt window and
-- per-entry durable/simulation provenance into the immutable death graph before the live payload
-- is removed. The production route is still disabled, so pre-existing deaths cannot be safely
-- reinterpreted as having passed this stronger authority boundary.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM death_events LIMIT 1) THEN
        RAISE EXCEPTION
            '0049 requires no existing deaths; clear the wipeable Core namespace before promotion closure';
    END IF;
END
$$;

CREATE TABLE death_live_trace_sets_v1 (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    contract_version SMALLINT NOT NULL,
    first_event_tick BIGINT NOT NULL,
    death_tick BIGINT NOT NULL,
    receipt_count SMALLINT NOT NULL,
    entry_count INTEGER NOT NULL,
    status_count INTEGER NOT NULL,
    lethal_trace_tick_id BYTEA NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    receipt_window_digest BYTEA NOT NULL,
    promotion_digest BYTEA NOT NULL,
    terminal_payload_hash BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, death_id),
    UNIQUE (namespace_id, death_id, account_id, character_id),
    UNIQUE (
        namespace_id, death_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ),
    FOREIGN KEY (namespace_id, account_id, character_id, death_id)
        REFERENCES death_events(namespace_id, account_id, character_id, death_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT death_live_trace_set_contract_exact CHECK (contract_version = 1),
    CONSTRAINT death_live_trace_set_ids_exact CHECK (
        octet_length(lineage_id) = 16
        AND lineage_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(restore_point_id) = 16
        AND restore_point_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(lethal_trace_tick_id) = 16
        AND lethal_trace_tick_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT death_live_trace_set_window_bounded CHECK (
        first_event_tick > 0 AND death_tick >= first_event_tick
        AND death_tick - first_event_tick <= 300
        AND receipt_count BETWEEN 1 AND 301
        AND entry_count BETWEEN 1 AND 4096
        AND status_count BETWEEN 0 AND 131072
    ),
    CONSTRAINT death_live_trace_set_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT death_live_trace_set_hashes_exact CHECK (
        octet_length(receipt_window_digest) = 32
        AND receipt_window_digest <> decode(repeat('00', 32), 'hex')
        AND octet_length(promotion_digest) = 32
        AND promotion_digest <> decode(repeat('00', 32), 'hex')
        AND octet_length(terminal_payload_hash) = 32
        AND terminal_payload_hash <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE death_live_trace_receipt_links_v1 (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    receipt_ordinal SMALLINT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    trace_tick_id BYTEA NOT NULL,
    expected_character_version BIGINT NOT NULL,
    lineage_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    checkpoint_tick BIGINT NOT NULL,
    event_tick BIGINT NOT NULL,
    entry_count SMALLINT NOT NULL,
    status_count SMALLINT NOT NULL,
    lethal_count SMALLINT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    tick_digest BYTEA NOT NULL,
    result_digest BYTEA NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    receipt_committed_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (namespace_id, death_id, receipt_ordinal),
    UNIQUE (namespace_id, death_id, trace_tick_id),
    UNIQUE (namespace_id, death_id, event_tick),
    UNIQUE (namespace_id, death_id, receipt_ordinal, trace_tick_id, event_tick),
    FOREIGN KEY (
        namespace_id, death_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) REFERENCES death_live_trace_sets_v1 (
        namespace_id, death_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id, trace_tick_id)
        REFERENCES character_live_damage_trace_ingest_receipts_v1 (
            namespace_id, account_id, character_id, trace_tick_id
        ) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT death_live_trace_receipt_ordinal_bounded CHECK (
        receipt_ordinal BETWEEN 0 AND 300
    ),
    CONSTRAINT death_live_trace_receipt_authority_bounded CHECK (
        expected_character_version > 0 AND checkpoint_tick >= 0 AND event_tick > 0
        AND entry_count BETWEEN 1 AND 4096
        AND status_count BETWEEN 0 AND 4096
        AND lethal_count BETWEEN 0 AND 1
        AND receipt_committed_at >= issued_at
    ),
    CONSTRAINT death_live_trace_receipt_hashes_exact CHECK (
        octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(tick_digest) = 32
        AND tick_digest <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_digest) = 32
        AND result_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE death_live_trace_entry_provenance_v1 (
    namespace_id TEXT NOT NULL,
    death_id BYTEA NOT NULL,
    trace_ordinal SMALLINT NOT NULL,
    receipt_ordinal SMALLINT NOT NULL,
    trace_tick_id BYTEA NOT NULL,
    event_tick BIGINT NOT NULL,
    event_ordinal INTEGER NOT NULL,
    cause_kind SMALLINT NOT NULL,
    source_entity_id BYTEA,
    source_sim_entity_id BYTEA,
    status_count SMALLINT NOT NULL,
    live_entry_digest BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, death_id, trace_ordinal),
    UNIQUE (namespace_id, death_id, event_tick, event_ordinal),
    FOREIGN KEY (namespace_id, death_id, trace_ordinal)
        REFERENCES death_combat_trace_entries(namespace_id, death_id, trace_ordinal)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (
        namespace_id, death_id, receipt_ordinal, trace_tick_id, event_tick
    ) REFERENCES death_live_trace_receipt_links_v1 (
        namespace_id, death_id, receipt_ordinal, trace_tick_id, event_tick
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT death_live_trace_provenance_ordinals_bounded CHECK (
        trace_ordinal BETWEEN 0 AND 4095
        AND receipt_ordinal BETWEEN 0 AND 300
        AND event_tick > 0 AND event_ordinal >= 0
    ),
    CONSTRAINT death_live_trace_provenance_cause_known CHECK (cause_kind BETWEEN 0 AND 3),
    CONSTRAINT death_live_trace_provenance_identity_parity CHECK (
        (source_entity_id IS NULL AND source_sim_entity_id IS NULL)
        OR (source_entity_id IS NOT NULL
            AND octet_length(source_entity_id) = 16
            AND source_entity_id <> decode(repeat('00', 16), 'hex')
            AND source_sim_entity_id IS NOT NULL
            AND octet_length(source_sim_entity_id) = 8
            AND source_sim_entity_id <> decode(repeat('00', 8), 'hex'))
    ),
    CONSTRAINT death_live_trace_provenance_status_bounded CHECK (
        status_count BETWEEN 0 AND 32
    ),
    CONSTRAINT death_live_trace_provenance_digest_exact CHECK (
        octet_length(live_entry_digest) = 32
        AND live_entry_digest <> decode(repeat('00', 32), 'hex')
    )
);

-- Changed promotion payloads are audit evidence, not death children. They intentionally remain
-- appendable after the accepted death transaction and contain hashes rather than raw trace data.
CREATE TABLE death_live_trace_promotion_conflict_audits_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    death_id BYTEA NOT NULL,
    audit_id BYTEA NOT NULL,
    conflict_code SMALLINT NOT NULL,
    stored_promotion_digest BYTEA NOT NULL,
    attempted_promotion_digest BYTEA NOT NULL,
    stored_terminal_payload_hash BYTEA NOT NULL,
    attempted_terminal_payload_hash BYTEA NOT NULL,
    attempted_issued_at TIMESTAMPTZ NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, audit_id),
    UNIQUE (namespace_id, death_id, attempted_promotion_digest),
    FOREIGN KEY (namespace_id, death_id, account_id, character_id)
        REFERENCES death_live_trace_sets_v1(namespace_id, death_id, account_id, character_id)
        ON DELETE CASCADE,
    CONSTRAINT death_live_trace_conflict_id_exact CHECK (
        octet_length(audit_id) = 16
        AND audit_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT death_live_trace_conflict_code_exact CHECK (conflict_code = 0),
    CONSTRAINT death_live_trace_conflict_hashes_exact CHECK (
        octet_length(stored_promotion_digest) = 32
        AND stored_promotion_digest <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_promotion_digest) = 32
        AND attempted_promotion_digest <> decode(repeat('00', 32), 'hex')
        AND attempted_promotion_digest <> stored_promotion_digest
        AND octet_length(stored_terminal_payload_hash) = 32
        AND stored_terminal_payload_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_terminal_payload_hash) = 32
        AND attempted_terminal_payload_hash <> decode(repeat('00', 32), 'hex')
    )
);

CREATE FUNCTION enforce_death_live_trace_promotion_graph_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT := COALESCE(NEW.namespace_id, OLD.namespace_id);
    target_death_id BYTEA := COALESCE(NEW.death_id, OLD.death_id);
    death death_events%ROWTYPE;
    promotion death_live_trace_sets_v1%ROWTYPE;
    actual_receipt_count BIGINT;
    actual_entry_count BIGINT;
    actual_status_count BIGINT;
BEGIN
    SELECT * INTO death
    FROM death_events
    WHERE namespace_id = target_namespace AND death_id = target_death_id;
    IF NOT FOUND THEN RETURN NULL; END IF;

    SELECT * INTO promotion
    FROM death_live_trace_sets_v1
    WHERE namespace_id = death.namespace_id AND death_id = death.death_id;
    IF NOT FOUND
        OR promotion.account_id IS DISTINCT FROM death.account_id
        OR promotion.character_id IS DISTINCT FROM death.character_id
        OR promotion.lineage_id IS DISTINCT FROM death.lineage_id
        OR promotion.restore_point_id IS DISTINCT FROM death.restore_point_id
        OR promotion.death_tick IS DISTINCT FROM death.death_tick
        OR promotion.records_blake3 IS DISTINCT FROM death.world_records_blake3
        OR promotion.assets_blake3 IS DISTINCT FROM death.world_assets_blake3
        OR promotion.localization_blake3 IS DISTINCT FROM death.world_localization_blake3
    THEN
        RAISE EXCEPTION 'death is missing its exact live-trace promotion root';
    END IF;

    SELECT count(*), COALESCE(sum(entry_count), 0), COALESCE(sum(status_count), 0)
    INTO actual_receipt_count, actual_entry_count, actual_status_count
    FROM death_live_trace_receipt_links_v1
    WHERE namespace_id = death.namespace_id AND death_id = death.death_id;
    IF actual_receipt_count <> promotion.receipt_count
        OR actual_entry_count <> promotion.entry_count
        OR actual_status_count <> promotion.status_count
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT receipt_ordinal,
                    event_tick,
                    checkpoint_tick,
                    row_number() OVER (ORDER BY event_tick, trace_tick_id) - 1 AS expected_ordinal,
                    lag(event_tick) OVER (ORDER BY event_tick, trace_tick_id) AS previous_tick,
                    lag(checkpoint_tick) OVER (ORDER BY event_tick, trace_tick_id)
                        AS previous_checkpoint
                FROM death_live_trace_receipt_links_v1
                WHERE namespace_id = death.namespace_id AND death_id = death.death_id
            ) AS ordered
            WHERE ordered.receipt_ordinal <> ordered.expected_ordinal
               OR ordered.event_tick IS NOT NULL AND ordered.previous_tick >= ordered.event_tick
               OR ordered.previous_checkpoint > ordered.checkpoint_tick
        )
        OR NOT EXISTS (
            SELECT 1
            FROM death_live_trace_receipt_links_v1 AS first_receipt
            WHERE first_receipt.namespace_id = death.namespace_id
              AND first_receipt.death_id = death.death_id
              AND first_receipt.receipt_ordinal = 0
              AND first_receipt.event_tick = promotion.first_event_tick
        )
        OR NOT EXISTS (
            SELECT 1
            FROM death_live_trace_receipt_links_v1 AS lethal_receipt
            WHERE lethal_receipt.namespace_id = death.namespace_id
              AND lethal_receipt.death_id = death.death_id
              AND lethal_receipt.receipt_ordinal = promotion.receipt_count - 1
              AND lethal_receipt.trace_tick_id = promotion.lethal_trace_tick_id
              AND lethal_receipt.event_tick = death.death_tick
              AND lethal_receipt.lethal_count = 1
              AND lethal_receipt.expected_character_version = death.pre_character_version
        )
        OR EXISTS (
            SELECT 1
            FROM death_live_trace_receipt_links_v1 AS receipt
            JOIN character_live_damage_trace_ingest_receipts_v1 AS retained
              ON retained.namespace_id = receipt.namespace_id
             AND retained.account_id = receipt.account_id
             AND retained.character_id = receipt.character_id
             AND retained.trace_tick_id = receipt.trace_tick_id
            WHERE receipt.namespace_id = death.namespace_id
              AND receipt.death_id = death.death_id
              AND (
                  receipt.expected_character_version IS DISTINCT FROM retained.expected_character_version
                  OR receipt.lineage_id IS DISTINCT FROM retained.lineage_id
                  OR receipt.restore_point_id IS DISTINCT FROM retained.restore_point_id
                  OR receipt.checkpoint_tick IS DISTINCT FROM retained.checkpoint_tick
                  OR receipt.event_tick IS DISTINCT FROM retained.event_tick
                  OR receipt.entry_count IS DISTINCT FROM retained.entry_count
                  OR receipt.status_count IS DISTINCT FROM retained.status_count
                  OR receipt.lethal_count IS DISTINCT FROM retained.lethal_count
                  OR receipt.records_blake3 IS DISTINCT FROM retained.records_blake3
                  OR receipt.assets_blake3 IS DISTINCT FROM retained.assets_blake3
                  OR receipt.localization_blake3 IS DISTINCT FROM retained.localization_blake3
                  OR receipt.request_hash IS DISTINCT FROM retained.request_hash
                  OR receipt.tick_digest IS DISTINCT FROM retained.tick_digest
                  OR receipt.result_digest IS DISTINCT FROM retained.result_digest
                  OR receipt.issued_at IS DISTINCT FROM retained.issued_at
                  OR receipt.receipt_committed_at IS DISTINCT FROM retained.committed_at
              )
        )
        OR EXISTS (
            SELECT 1 FROM death_live_trace_receipt_links_v1 AS receipt
            WHERE receipt.namespace_id = death.namespace_id
              AND receipt.death_id = death.death_id
              AND receipt.receipt_ordinal < promotion.receipt_count - 1
              AND receipt.lethal_count <> 0
        )
        OR EXISTS (
            SELECT 1
            FROM death_live_trace_receipt_links_v1 AS receipt
            WHERE receipt.namespace_id = death.namespace_id
              AND receipt.death_id = death.death_id
              AND (
                  receipt.entry_count <> (
                      SELECT count(*)
                      FROM death_live_trace_entry_provenance_v1 AS provenance
                      WHERE provenance.namespace_id = receipt.namespace_id
                        AND provenance.death_id = receipt.death_id
                        AND provenance.receipt_ordinal = receipt.receipt_ordinal
                  )
                  OR receipt.status_count <> (
                      SELECT COALESCE(sum(provenance.status_count), 0)
                      FROM death_live_trace_entry_provenance_v1 AS provenance
                      WHERE provenance.namespace_id = receipt.namespace_id
                        AND provenance.death_id = receipt.death_id
                        AND provenance.receipt_ordinal = receipt.receipt_ordinal
                  )
                  OR receipt.lethal_count <> (
                      SELECT count(*)
                      FROM death_live_trace_entry_provenance_v1 AS provenance
                      JOIN death_combat_trace_entries AS trace
                        ON trace.namespace_id = provenance.namespace_id
                       AND trace.death_id = provenance.death_id
                       AND trace.trace_ordinal = provenance.trace_ordinal
                      WHERE provenance.namespace_id = receipt.namespace_id
                        AND provenance.death_id = receipt.death_id
                        AND provenance.receipt_ordinal = receipt.receipt_ordinal
                        AND trace.lethal
                  )
              )
        )
        OR (SELECT count(*)
            FROM character_live_damage_trace_ingest_receipts_v1 AS retained
            WHERE retained.namespace_id = death.namespace_id
              AND retained.account_id = death.account_id
              AND retained.character_id = death.character_id
              AND retained.lineage_id = death.lineage_id
              AND retained.restore_point_id = death.restore_point_id
              AND retained.records_blake3 = promotion.records_blake3
              AND retained.assets_blake3 = promotion.assets_blake3
              AND retained.localization_blake3 = promotion.localization_blake3
              AND retained.event_tick BETWEEN GREATEST(1, death.death_tick - 300)
                                          AND death.death_tick) <> promotion.receipt_count
        OR EXISTS (
            SELECT 1
            FROM character_live_damage_trace_ingest_receipts_v1 AS retained
            WHERE retained.namespace_id = death.namespace_id
              AND retained.account_id = death.account_id
              AND retained.character_id = death.character_id
              AND retained.lineage_id = death.lineage_id
              AND retained.restore_point_id = death.restore_point_id
              AND retained.records_blake3 = promotion.records_blake3
              AND retained.assets_blake3 = promotion.assets_blake3
              AND retained.localization_blake3 = promotion.localization_blake3
              AND retained.event_tick BETWEEN GREATEST(1, death.death_tick - 300)
                                          AND death.death_tick
              AND NOT EXISTS (
                  SELECT 1
                  FROM death_live_trace_receipt_links_v1 AS linked
                  WHERE linked.namespace_id = retained.namespace_id
                    AND linked.death_id = death.death_id
                    AND linked.account_id = retained.account_id
                    AND linked.character_id = retained.character_id
                    AND linked.trace_tick_id = retained.trace_tick_id
              )
        )
    THEN
        RAISE EXCEPTION 'death live-trace receipt window is incomplete or unauthoritative';
    END IF;

    SELECT count(*) INTO actual_entry_count
    FROM death_live_trace_entry_provenance_v1
    WHERE namespace_id = death.namespace_id AND death_id = death.death_id;
    SELECT count(*) INTO actual_status_count
    FROM death_combat_trace_statuses
    WHERE namespace_id = death.namespace_id AND death_id = death.death_id;
    IF actual_entry_count <> promotion.entry_count
        OR actual_status_count <> promotion.status_count
        OR (SELECT count(*) FROM death_combat_trace_entries
            WHERE namespace_id = death.namespace_id AND death_id = death.death_id)
            <> promotion.entry_count
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT trace_ordinal,
                    row_number() OVER (ORDER BY event_tick, event_ordinal) - 1 AS expected_ordinal
                FROM death_live_trace_entry_provenance_v1
                WHERE namespace_id = death.namespace_id AND death_id = death.death_id
            ) AS ordered
            WHERE ordered.trace_ordinal <> ordered.expected_ordinal
        )
        OR EXISTS (
            SELECT 1
            FROM death_live_trace_entry_provenance_v1 AS provenance
            JOIN death_combat_trace_entries AS trace
              ON trace.namespace_id = provenance.namespace_id
             AND trace.death_id = provenance.death_id
             AND trace.trace_ordinal = provenance.trace_ordinal
            JOIN death_live_trace_receipt_links_v1 AS receipt
              ON receipt.namespace_id = provenance.namespace_id
             AND receipt.death_id = provenance.death_id
             AND receipt.receipt_ordinal = provenance.receipt_ordinal
             AND receipt.trace_tick_id = provenance.trace_tick_id
             AND receipt.event_tick = provenance.event_tick
            WHERE provenance.namespace_id = death.namespace_id
              AND provenance.death_id = death.death_id
              AND (
                  trace.event_tick IS DISTINCT FROM provenance.event_tick
                  OR trace.event_ordinal IS DISTINCT FROM provenance.event_ordinal
                  OR trace.source_entity_id IS DISTINCT FROM provenance.source_entity_id
                  OR provenance.status_count IS DISTINCT FROM (
                      SELECT count(*)
                      FROM death_combat_trace_statuses AS status
                      WHERE status.namespace_id = trace.namespace_id
                        AND status.death_id = trace.death_id
                        AND status.trace_ordinal = trace.trace_ordinal
                  )
              )
        )
        OR EXISTS (
            SELECT 1
            FROM death_live_trace_entry_provenance_v1 AS left_entry
            JOIN death_live_trace_entry_provenance_v1 AS right_entry
              ON right_entry.namespace_id = left_entry.namespace_id
             AND right_entry.death_id = left_entry.death_id
             AND right_entry.trace_ordinal > left_entry.trace_ordinal
            WHERE left_entry.namespace_id = death.namespace_id
              AND left_entry.death_id = death.death_id
              AND (
                  (left_entry.source_sim_entity_id = right_entry.source_sim_entity_id
                      AND left_entry.source_entity_id <> right_entry.source_entity_id)
                  OR (left_entry.source_entity_id = right_entry.source_entity_id
                      AND left_entry.source_sim_entity_id <> right_entry.source_sim_entity_id)
              )
        )
        OR NOT EXISTS (
            SELECT 1
            FROM death_live_trace_entry_provenance_v1 AS provenance
            JOIN death_combat_trace_entries AS trace
              ON trace.namespace_id = provenance.namespace_id
             AND trace.death_id = provenance.death_id
             AND trace.trace_ordinal = provenance.trace_ordinal
            WHERE provenance.namespace_id = death.namespace_id
              AND provenance.death_id = death.death_id
              AND provenance.trace_ordinal = promotion.entry_count - 1
              AND provenance.cause_kind = death.cause_kind
              AND trace.lethal
              AND trace.event_tick = death.death_tick
        )
    THEN
        RAISE EXCEPTION 'death live-trace provenance is incomplete or noncanonical';
    END IF;
    RETURN NULL;
END
$$;

-- This immediate validator runs while the normalized live row still exists. The later terminal
-- checkpoint cleanup may prune that row only after its complete field/status payload is sealed
-- into the immutable durable trace and provenance graph.
CREATE FUNCTION enforce_death_live_trace_provenance_source_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
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
        JOIN death_combat_trace_entries AS durable
          ON durable.namespace_id = NEW.namespace_id
         AND durable.death_id = NEW.death_id
         AND durable.trace_ordinal = NEW.trace_ordinal
        WHERE receipt.namespace_id = NEW.namespace_id
          AND receipt.death_id = NEW.death_id
          AND receipt.receipt_ordinal = NEW.receipt_ordinal
          AND receipt.trace_tick_id = NEW.trace_tick_id
          AND receipt.event_tick = NEW.event_tick
          AND live.cause_kind = NEW.cause_kind
          AND live.source_entity_id IS NOT DISTINCT FROM NEW.source_entity_id
          AND live.source_sim_entity_id IS NOT DISTINCT FROM NEW.source_sim_entity_id
          AND live.status_count = NEW.status_count
          AND live.entry_digest = NEW.live_entry_digest
          AND durable.event_tick = live.event_tick
          AND durable.event_ordinal = live.event_ordinal
          AND durable.source_content_id IS NOT DISTINCT FROM live.source_content_id
          AND durable.source_entity_id IS NOT DISTINCT FROM live.source_entity_id
          AND durable.pattern_id IS NOT DISTINCT FROM live.pattern_id
          AND durable.attack_id IS NOT DISTINCT FROM live.attack_id
          AND durable.raw_damage = live.raw_damage
          AND durable.final_damage = live.final_damage
          AND durable.damage_type = live.damage_type
          AND durable.pre_health = live.pre_health
          AND durable.post_health = live.post_health
          AND durable.source_x_milli_tiles = live.source_x_milli_tiles
          AND durable.source_y_milli_tiles = live.source_y_milli_tiles
          AND durable.network_state = live.network_state
          AND durable.recall_state = live.recall_state
          AND durable.lethal = live.lethal
          AND NOT EXISTS (
              (SELECT status_ordinal, status_id COLLATE "C", remaining_ticks, stack_count
               FROM character_live_damage_trace_statuses_v1 AS live_status
               WHERE live_status.namespace_id = live.namespace_id
                 AND live_status.account_id = live.account_id
                 AND live_status.trace_tick_id = live.trace_tick_id
                 AND live_status.event_ordinal = live.event_ordinal)
              EXCEPT
              (SELECT status_ordinal, status_id COLLATE "C", remaining_ticks, stack_count
               FROM death_combat_trace_statuses AS durable_status
               WHERE durable_status.namespace_id = durable.namespace_id
                 AND durable_status.death_id = durable.death_id
                 AND durable_status.trace_ordinal = durable.trace_ordinal)
          )
          AND NOT EXISTS (
              (SELECT status_ordinal, status_id COLLATE "C", remaining_ticks, stack_count
               FROM death_combat_trace_statuses AS durable_status
               WHERE durable_status.namespace_id = durable.namespace_id
                 AND durable_status.death_id = durable.death_id
                 AND durable_status.trace_ordinal = durable.trace_ordinal)
              EXCEPT
              (SELECT status_ordinal, status_id COLLATE "C", remaining_ticks, stack_count
               FROM character_live_damage_trace_statuses_v1 AS live_status
               WHERE live_status.namespace_id = live.namespace_id
                 AND live_status.account_id = live.account_id
                 AND live_status.trace_tick_id = live.trace_tick_id
                 AND live_status.event_ordinal = live.event_ordinal)
          )
    ) THEN
        RAISE EXCEPTION 'death live-trace provenance does not match its live and durable payload';
    END IF;
    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER death_live_trace_root_graph_complete_v1
AFTER INSERT OR DELETE ON death_live_trace_sets_v1
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_death_live_trace_promotion_graph_v1();
CREATE CONSTRAINT TRIGGER death_live_trace_receipt_graph_complete_v1
AFTER INSERT OR DELETE ON death_live_trace_receipt_links_v1
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_death_live_trace_promotion_graph_v1();
CREATE CONSTRAINT TRIGGER death_live_trace_provenance_graph_complete_v1
AFTER INSERT OR DELETE ON death_live_trace_entry_provenance_v1
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_death_live_trace_promotion_graph_v1();
CREATE CONSTRAINT TRIGGER death_requires_live_trace_promotion_v1
AFTER INSERT OR DELETE ON death_events
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_death_live_trace_promotion_graph_v1();

CREATE TRIGGER death_live_trace_root_insert_window_v1
BEFORE INSERT ON death_live_trace_sets_v1
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_live_trace_receipt_insert_window_v1
BEFORE INSERT ON death_live_trace_receipt_links_v1
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_live_trace_provenance_insert_window_v1
BEFORE INSERT ON death_live_trace_entry_provenance_v1
FOR EACH ROW EXECUTE FUNCTION enforce_death_child_insert_window_v1();
CREATE TRIGGER death_live_trace_provenance_source_exact_v1
BEFORE INSERT ON death_live_trace_entry_provenance_v1
FOR EACH ROW EXECUTE FUNCTION enforce_death_live_trace_provenance_source_v1();

CREATE TRIGGER death_live_trace_root_immutable_v1
BEFORE UPDATE OR DELETE ON death_live_trace_sets_v1
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_live_trace_receipt_immutable_v1
BEFORE UPDATE OR DELETE ON death_live_trace_receipt_links_v1
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_live_trace_provenance_immutable_v1
BEFORE UPDATE OR DELETE ON death_live_trace_entry_provenance_v1
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();
CREATE TRIGGER death_live_trace_conflict_immutable_v1
BEFORE UPDATE OR DELETE ON death_live_trace_promotion_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION reject_death_history_mutation_v1();

CREATE FUNCTION enforce_death_live_trace_conflict_audit_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.observed_at IS DISTINCT FROM transaction_timestamp()
        OR NOT EXISTS (
            SELECT 1
            FROM death_live_trace_sets_v1 AS promotion
            WHERE promotion.namespace_id = NEW.namespace_id
              AND promotion.death_id = NEW.death_id
              AND promotion.account_id = NEW.account_id
              AND promotion.character_id = NEW.character_id
              AND promotion.promotion_digest = NEW.stored_promotion_digest
              AND promotion.terminal_payload_hash = NEW.stored_terminal_payload_hash
        )
    THEN
        RAISE EXCEPTION 'live-trace promotion conflict must name the stored death authority';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER death_live_trace_conflict_authority_v1
BEFORE INSERT ON death_live_trace_promotion_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_death_live_trace_conflict_audit_v1();

COMMENT ON TABLE death_live_trace_sets_v1 IS
    'Immutable contract-1 retained-receipt window and promotion hash required by every durable death.';
COMMENT ON TABLE death_live_trace_receipt_links_v1 IS
    'Exact ordered copies of retained live-trace receipts promoted by the owning death transaction.';
COMMENT ON TABLE death_live_trace_entry_provenance_v1 IS
    'Per-entry durable/simulation identity and live digest provenance paired to the durable death trace.';
COMMENT ON TABLE death_live_trace_promotion_conflict_audits_v1 IS
    'Append-only hash-only TECH-021 evidence for altered promotion replay after a committed death.';

-- Downgrade/recovery: Core remains wipeable. Restore a pre-0049 backup or wipe/reapply the Core
-- namespace. Never synthesize promotion rows for accepted death history, never remove conflict
-- evidence from a live namespace, and never rewrite migrations 0043 or 0048 in place.
