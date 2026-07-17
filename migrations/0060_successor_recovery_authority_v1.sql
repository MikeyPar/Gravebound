-- GB-M03-07 durable successor recovery authority.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-020/021, UI-007-009,
--   TECH-021-023, and QA-101.
-- - Gravebound_Content_Production_Spec_v1.md CONT-CATALOG-003.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-07 and the M03 exit gate.
-- - Accepted SPEC-CONFLICT-031.
--
-- Append-only discriminants:
--   Appearance 0 CoreBaseSilhouette.
--   Reservation 0 Active, 1 Consumed, 2 Superseded.
--   Result/audit/outbox event 1 SuccessorCreated.
--
-- Existing death rows do not contain the canonical BLAKE3 preset hash. This wipeable-Core
-- migration refuses to invent that authority after the death transaction. Clear the Core
-- namespace before migration if any death history exists. A pre-0060 binary may be restored
-- only on a fresh Core database; published migration history is never dropped or rewritten.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM death_events LIMIT 1) THEN
        RAISE EXCEPTION
            '0060 requires no preexisting death rows; clear the wipeable Core namespace';
    END IF;
END
$$;

CREATE TABLE death_successor_presets_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    former_character_id BYTEA NOT NULL,
    death_id BYTEA NOT NULL,
    preset_revision SMALLINT NOT NULL,
    former_roster_ordinal SMALLINT NOT NULL,
    class_id TEXT NOT NULL,
    appearance_kind SMALLINT NOT NULL,
    base_silhouette_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    preset_hash BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, death_id),
    UNIQUE (namespace_id, account_id, death_id),
    UNIQUE (namespace_id, account_id, former_character_id, death_id),
    UNIQUE (namespace_id, account_id, death_id, former_roster_ordinal),
    UNIQUE (
        namespace_id, account_id, death_id, former_roster_ordinal, class_id,
        appearance_kind, base_silhouette_id, content_revision, preset_hash
    ),
    FOREIGN KEY (namespace_id, account_id, former_character_id, death_id)
        REFERENCES death_events(namespace_id, account_id, character_id, death_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT successor_preset_ids_exact CHECK (
        octet_length(account_id) = 16
        AND octet_length(former_character_id) = 16
        AND octet_length(death_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
        AND former_character_id <> decode(repeat('00', 16), 'hex')
        AND death_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT successor_preset_core_exact CHECK (
        preset_revision = 1
        AND former_roster_ordinal BETWEEN 1 AND 2
        AND class_id = 'class.grave_arbalist'
        AND appearance_kind = 0
        AND base_silhouette_id = 'sprite.class.grave_arbalist'
        AND content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT successor_preset_hash_exact CHECK (
        octet_length(preset_hash) = 32
        AND preset_hash <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE successor_roster_reservations_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    death_id BYTEA NOT NULL,
    former_roster_ordinal SMALLINT NOT NULL,
    reservation_state SMALLINT NOT NULL,
    reserved_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    consumed_mutation_id BYTEA,
    consumed_successor_id BYTEA,
    consumed_receipt_id BYTEA,
    consumed_at TIMESTAMPTZ,
    superseded_by_death_id BYTEA,
    superseded_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, account_id, death_id),
    UNIQUE (namespace_id, account_id, death_id, former_roster_ordinal),
    FOREIGN KEY (namespace_id, account_id, death_id, former_roster_ordinal)
        REFERENCES death_successor_presets_v1(
            namespace_id, account_id, death_id, former_roster_ordinal
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, superseded_by_death_id)
        REFERENCES death_events(namespace_id, account_id, death_id)
        DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT successor_reservation_ids_exact CHECK (
        octet_length(account_id) = 16
        AND octet_length(death_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
        AND death_id <> decode(repeat('00', 16), 'hex')
        AND (consumed_mutation_id IS NULL OR (
            octet_length(consumed_mutation_id) = 16
            AND consumed_mutation_id <> decode(repeat('00', 16), 'hex')
        ))
        AND (consumed_successor_id IS NULL OR (
            octet_length(consumed_successor_id) = 16
            AND consumed_successor_id <> decode(repeat('00', 16), 'hex')
        ))
        AND (consumed_receipt_id IS NULL OR (
            octet_length(consumed_receipt_id) = 16
            AND consumed_receipt_id <> decode(repeat('00', 16), 'hex')
        ))
        AND (superseded_by_death_id IS NULL OR (
            octet_length(superseded_by_death_id) = 16
            AND superseded_by_death_id <> decode(repeat('00', 16), 'hex')
        ))
    ),
    CONSTRAINT successor_reservation_state_shape CHECK (
        former_roster_ordinal BETWEEN 1 AND 2
        AND (
            (reservation_state = 0
                AND consumed_mutation_id IS NULL
                AND consumed_successor_id IS NULL
                AND consumed_receipt_id IS NULL
                AND consumed_at IS NULL
                AND superseded_by_death_id IS NULL
                AND superseded_at IS NULL)
            OR
            (reservation_state = 1
                AND consumed_mutation_id IS NOT NULL
                AND consumed_successor_id IS NOT NULL
                AND consumed_receipt_id IS NOT NULL
                AND consumed_at IS NOT NULL
                AND superseded_by_death_id IS NULL
                AND superseded_at IS NULL
                AND consumed_mutation_id <> consumed_successor_id
                AND consumed_mutation_id <> consumed_receipt_id
                AND consumed_successor_id <> consumed_receipt_id
                AND death_id <> consumed_mutation_id
                AND death_id <> consumed_successor_id
                AND death_id <> consumed_receipt_id)
            OR
            (reservation_state = 2
                AND consumed_mutation_id IS NULL
                AND consumed_successor_id IS NULL
                AND consumed_receipt_id IS NULL
                AND consumed_at IS NULL
                AND superseded_by_death_id IS NOT NULL
                AND superseded_by_death_id <> death_id
                AND superseded_at IS NOT NULL)
        )
    )
);

CREATE UNIQUE INDEX one_active_successor_reservation_v1
    ON successor_roster_reservations_v1 (namespace_id, account_id)
    WHERE reservation_state = 0;

CREATE UNIQUE INDEX one_consumed_successor_identity_v1
    ON successor_roster_reservations_v1 (namespace_id, account_id, consumed_successor_id)
    WHERE reservation_state = 1;

CREATE TABLE successor_mutation_results_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    death_id BYTEA NOT NULL,
    successor_id BYTEA NOT NULL,
    selected_character_id BYTEA NOT NULL,
    receipt_id BYTEA NOT NULL,
    contract_version SMALLINT NOT NULL,
    protocol_major SMALLINT NOT NULL,
    protocol_minor SMALLINT NOT NULL,
    canonical_request_hash BYTEA NOT NULL,
    former_roster_ordinal SMALLINT NOT NULL,
    class_id TEXT NOT NULL,
    appearance_kind SMALLINT NOT NULL,
    base_silhouette_id TEXT NOT NULL,
    preset_hash BYTEA NOT NULL,
    content_revision TEXT NOT NULL,
    result_code SMALLINT NOT NULL,
    result_payload BYTEA NOT NULL,
    result_hash BYTEA NOT NULL,
    pre_account_version BIGINT NOT NULL,
    post_account_version BIGINT NOT NULL,
    post_character_version BIGINT NOT NULL,
    post_progression_version BIGINT NOT NULL,
    post_world_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    post_life_metrics_version BIGINT NOT NULL,
    post_oath_bargain_version BIGINT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    UNIQUE (namespace_id, account_id, mutation_id, canonical_request_hash),
    UNIQUE (namespace_id, account_id, death_id),
    UNIQUE (namespace_id, account_id, successor_id),
    UNIQUE (namespace_id, account_id, receipt_id),
    UNIQUE (
        namespace_id, account_id, death_id, mutation_id, successor_id, receipt_id
    ),
    FOREIGN KEY (namespace_id, account_id)
        REFERENCES accounts(namespace_id, account_id) ON DELETE CASCADE,
    FOREIGN KEY (
        namespace_id, account_id, death_id, former_roster_ordinal, class_id,
        appearance_kind, base_silhouette_id, content_revision, preset_hash
    ) REFERENCES death_successor_presets_v1(
        namespace_id, account_id, death_id, former_roster_ordinal, class_id,
        appearance_kind, base_silhouette_id, content_revision, preset_hash
    ) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, death_id, former_roster_ordinal)
        REFERENCES successor_roster_reservations_v1(
            namespace_id, account_id, death_id, former_roster_ordinal
        ) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, successor_id)
        REFERENCES characters(namespace_id, account_id, character_id)
        DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT successor_result_ids_exact CHECK (
        octet_length(account_id) = 16
        AND octet_length(mutation_id) = 16
        AND octet_length(death_id) = 16
        AND octet_length(successor_id) = 16
        AND octet_length(selected_character_id) = 16
        AND octet_length(receipt_id) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND death_id <> decode(repeat('00', 16), 'hex')
        AND successor_id <> decode(repeat('00', 16), 'hex')
        AND selected_character_id <> decode(repeat('00', 16), 'hex')
        AND receipt_id <> decode(repeat('00', 16), 'hex')
        AND selected_character_id = successor_id
        AND mutation_id <> death_id
        AND mutation_id <> successor_id
        AND mutation_id <> receipt_id
        AND death_id <> successor_id
        AND death_id <> receipt_id
        AND successor_id <> receipt_id
    ),
    CONSTRAINT successor_result_contract_exact CHECK (
        contract_version = 1
        AND protocol_major = 1
        AND protocol_minor = 17
        AND result_code = 1
        AND former_roster_ordinal BETWEEN 1 AND 2
        AND class_id = 'class.grave_arbalist'
        AND appearance_kind = 0
        AND base_silhouette_id = 'sprite.class.grave_arbalist'
        AND content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT successor_result_hashes_exact CHECK (
        octet_length(canonical_request_hash) = 32
        AND canonical_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(preset_hash) = 32
        AND preset_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_hash) = 32
        AND result_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT successor_result_versions_exact CHECK (
        pre_account_version > 0
        AND post_account_version = pre_account_version + 1
        AND post_character_version = 1
        AND post_progression_version = 1
        AND post_world_version = 1
        AND post_inventory_version = 2
        AND post_life_metrics_version = 1
        AND post_oath_bargain_version = 1
    )
);

CREATE TABLE successor_creation_receipts_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    receipt_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    death_id BYTEA NOT NULL,
    successor_id BYTEA NOT NULL,
    initializer_revision TEXT NOT NULL,
    initializer_request_hash BYTEA NOT NULL,
    initializer_result_hash BYTEA NOT NULL,
    weapon_uid BYTEA NOT NULL,
    relic_uid BYTEA NOT NULL,
    tonic_uid_0 BYTEA NOT NULL,
    tonic_uid_1 BYTEA NOT NULL,
    item_count SMALLINT NOT NULL,
    item_content_revision TEXT NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, receipt_id),
    UNIQUE (namespace_id, account_id, mutation_id),
    UNIQUE (namespace_id, account_id, death_id),
    UNIQUE (
        namespace_id, account_id, death_id, mutation_id, successor_id, receipt_id
    ),
    FOREIGN KEY (namespace_id, account_id, mutation_id)
        REFERENCES successor_mutation_results_v1(namespace_id, account_id, mutation_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, successor_id, initializer_revision)
        REFERENCES starter_initializer_results(
            namespace_id, account_id, character_id, initializer_revision
        ) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, weapon_uid)
        REFERENCES item_instances(namespace_id, item_uid) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, relic_uid)
        REFERENCES item_instances(namespace_id, item_uid) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, tonic_uid_0)
        REFERENCES item_instances(namespace_id, item_uid) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, tonic_uid_1)
        REFERENCES item_instances(namespace_id, item_uid) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT successor_receipt_ids_exact CHECK (
        octet_length(account_id) = 16
        AND octet_length(receipt_id) = 16
        AND octet_length(mutation_id) = 16
        AND octet_length(death_id) = 16
        AND octet_length(successor_id) = 16
        AND octet_length(weapon_uid) = 16
        AND octet_length(relic_uid) = 16
        AND octet_length(tonic_uid_0) = 16
        AND octet_length(tonic_uid_1) = 16
        AND account_id <> decode(repeat('00', 16), 'hex')
        AND receipt_id <> decode(repeat('00', 16), 'hex')
        AND mutation_id <> decode(repeat('00', 16), 'hex')
        AND death_id <> decode(repeat('00', 16), 'hex')
        AND successor_id <> decode(repeat('00', 16), 'hex')
        AND weapon_uid <> decode(repeat('00', 16), 'hex')
        AND relic_uid <> decode(repeat('00', 16), 'hex')
        AND tonic_uid_0 <> decode(repeat('00', 16), 'hex')
        AND tonic_uid_1 <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT successor_receipt_items_distinct CHECK (
        weapon_uid <> relic_uid
        AND weapon_uid <> tonic_uid_0
        AND weapon_uid <> tonic_uid_1
        AND relic_uid <> tonic_uid_0
        AND relic_uid <> tonic_uid_1
        AND tonic_uid_0 <> tonic_uid_1
        AND weapon_uid NOT IN (mutation_id, death_id, successor_id, receipt_id)
        AND relic_uid NOT IN (mutation_id, death_id, successor_id, receipt_id)
        AND tonic_uid_0 NOT IN (mutation_id, death_id, successor_id, receipt_id)
        AND tonic_uid_1 NOT IN (mutation_id, death_id, successor_id, receipt_id)
    ),
    CONSTRAINT successor_receipt_starter_exact CHECK (
        initializer_revision = 'starter.core-dev.v1'
        AND item_count = 4
        AND item_content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
        AND octet_length(initializer_request_hash) = 32
        AND initializer_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(initializer_result_hash) = 32
        AND initializer_result_hash <> decode(repeat('00', 32), 'hex')
    )
);

ALTER TABLE successor_mutation_results_v1
    ADD CONSTRAINT successor_result_receipt_owned FOREIGN KEY (
        namespace_id, account_id, death_id, mutation_id, successor_id, receipt_id
    ) REFERENCES successor_creation_receipts_v1(
        namespace_id, account_id, death_id, mutation_id, successor_id, receipt_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE successor_roster_reservations_v1
    ADD CONSTRAINT successor_consumption_result_owned FOREIGN KEY (
        namespace_id, account_id, death_id, consumed_mutation_id,
        consumed_successor_id, consumed_receipt_id
    ) REFERENCES successor_mutation_results_v1(
        namespace_id, account_id, death_id, mutation_id, successor_id, receipt_id
    ) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE successor_mutation_audit_events_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    death_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    successor_id BYTEA NOT NULL,
    event_id BYTEA NOT NULL,
    event_type SMALLINT NOT NULL,
    event_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, account_id, mutation_id, event_type),
    FOREIGN KEY (namespace_id, account_id, mutation_id)
        REFERENCES successor_mutation_results_v1(namespace_id, account_id, mutation_id)
        ON DELETE CASCADE,
    CONSTRAINT successor_audit_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
        AND event_type = 1
        AND octet_length(event_digest) = 32
        AND event_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE successor_mutation_conflict_audits_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    incoming_death_id BYTEA NOT NULL,
    stored_request_hash BYTEA NOT NULL,
    incoming_request_hash BYTEA NOT NULL,
    conflict_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id, conflict_digest),
    UNIQUE (namespace_id, account_id, mutation_id, incoming_request_hash),
    FOREIGN KEY (
        namespace_id, account_id, mutation_id, stored_request_hash
    ) REFERENCES successor_mutation_results_v1(
        namespace_id, account_id, mutation_id, canonical_request_hash
    ) ON DELETE CASCADE,
    CONSTRAINT successor_conflict_exact CHECK (
        octet_length(incoming_death_id) = 16
        AND incoming_death_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(stored_request_hash) = 32
        AND octet_length(incoming_request_hash) = 32
        AND octet_length(conflict_digest) = 32
        AND stored_request_hash <> decode(repeat('00', 32), 'hex')
        AND incoming_request_hash <> decode(repeat('00', 32), 'hex')
        AND conflict_digest <> decode(repeat('00', 32), 'hex')
        AND stored_request_hash <> incoming_request_hash
    )
);

CREATE TABLE successor_mutation_outbox_events_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    death_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    successor_id BYTEA NOT NULL,
    receipt_id BYTEA NOT NULL,
    event_id BYTEA NOT NULL,
    event_type SMALLINT NOT NULL,
    event_payload BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    published_at TIMESTAMPTZ,
    PRIMARY KEY (namespace_id, event_id),
    UNIQUE (namespace_id, account_id, mutation_id, event_type),
    FOREIGN KEY (namespace_id, account_id, mutation_id)
        REFERENCES successor_mutation_results_v1(namespace_id, account_id, mutation_id)
        ON DELETE CASCADE,
    CONSTRAINT successor_outbox_exact CHECK (
        octet_length(event_id) = 16
        AND event_id <> decode(repeat('00', 16), 'hex')
        AND event_type = 1
        AND octet_length(event_payload) BETWEEN 1 AND 65536
        AND (published_at IS NULL OR published_at >= created_at)
    )
);

CREATE INDEX unpublished_successor_events_v1
    ON successor_mutation_outbox_events_v1(namespace_id, created_at, event_id)
    WHERE published_at IS NULL;

CREATE FUNCTION enforce_successor_death_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    death_time TIMESTAMPTZ;
    provenance SMALLINT;
BEGIN
    SELECT committed_at, death_provenance INTO death_time, provenance
    FROM death_events
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND death_id = NEW.death_id;
    IF NOT FOUND
        OR provenance <> 0
        OR death_time IS DISTINCT FROM transaction_timestamp()
    THEN
        RAISE EXCEPTION
            '% may be inserted only with its ordinary owning death', TG_TABLE_NAME;
    END IF;
    IF TG_TABLE_NAME = 'death_successor_presets_v1'
        AND NEW.created_at IS DISTINCT FROM death_time
    THEN
        RAISE EXCEPTION 'successor preset time must equal its death commit';
    END IF;
    IF TG_TABLE_NAME = 'successor_roster_reservations_v1'
        AND (NEW.reservation_state <> 0 OR NEW.reserved_at IS DISTINCT FROM death_time)
    THEN
        RAISE EXCEPTION 'successor reservation must begin Active with its death commit';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER successor_preset_insert_window_v1
BEFORE INSERT ON death_successor_presets_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_death_insert_window_v1();

CREATE TRIGGER successor_reservation_insert_window_v1
BEFORE INSERT ON successor_roster_reservations_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_death_insert_window_v1();

CREATE FUNCTION enforce_successor_preset_immutable_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
    RAISE EXCEPTION 'death-time successor preset is immutable';
END
$$;

CREATE TRIGGER successor_preset_immutable_v1
BEFORE UPDATE OR DELETE ON death_successor_presets_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_preset_immutable_v1();

CREATE FUNCTION enforce_successor_reservation_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'successor reservation history is immutable';
    END IF;
    IF OLD.reservation_state <> 0
        OR NEW.reservation_state NOT IN (1, 2)
        OR ROW(
            NEW.namespace_id, NEW.account_id, NEW.death_id,
            NEW.former_roster_ordinal, NEW.reserved_at
        ) IS DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.death_id,
            OLD.former_roster_ordinal, OLD.reserved_at
        )
    THEN
        RAISE EXCEPTION 'successor reservation permits one terminal transition only';
    END IF;
    IF NEW.reservation_state = 1
        AND NEW.consumed_at IS DISTINCT FROM transaction_timestamp()
    THEN
        RAISE EXCEPTION 'successor consumption time is transaction authority';
    END IF;
    IF NEW.reservation_state = 2
        AND NEW.superseded_at IS DISTINCT FROM transaction_timestamp()
    THEN
        RAISE EXCEPTION 'successor supersession time is transaction authority';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER successor_reservation_terminal_only_v1
BEFORE UPDATE OR DELETE ON successor_roster_reservations_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_reservation_mutation_v1();

CREATE FUNCTION enforce_successor_reservation_transition_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.reservation_state = 1 AND NOT EXISTS (
        SELECT 1
        FROM successor_mutation_results_v1 AS result
        JOIN successor_creation_receipts_v1 AS receipt
          ON receipt.namespace_id = result.namespace_id
         AND receipt.account_id = result.account_id
         AND receipt.mutation_id = result.mutation_id
        WHERE result.namespace_id = NEW.namespace_id
          AND result.account_id = NEW.account_id
          AND result.death_id = NEW.death_id
          AND result.mutation_id = NEW.consumed_mutation_id
          AND result.successor_id = NEW.consumed_successor_id
          AND result.receipt_id = NEW.consumed_receipt_id
          AND result.former_roster_ordinal = NEW.former_roster_ordinal
          AND result.committed_at = NEW.consumed_at
          AND receipt.receipt_id = NEW.consumed_receipt_id
          AND receipt.committed_at = NEW.consumed_at
    ) THEN
        RAISE EXCEPTION 'consumed successor reservation lacks its exact stored result';
    END IF;
    IF NEW.reservation_state = 2 AND NOT EXISTS (
        SELECT 1
        FROM death_events AS death
        JOIN successor_roster_reservations_v1 AS replacement
          ON replacement.namespace_id = death.namespace_id
         AND replacement.account_id = death.account_id
         AND replacement.death_id = death.death_id
        WHERE death.namespace_id = NEW.namespace_id
          AND death.account_id = NEW.account_id
          AND death.death_id = NEW.superseded_by_death_id
          AND death.death_provenance = 0
          AND death.committed_at = NEW.superseded_at
          AND NEW.superseded_at > NEW.reserved_at
          AND replacement.reservation_state = 0
          AND replacement.reserved_at = death.committed_at
    ) THEN
        RAISE EXCEPTION 'superseded successor reservation lacks its newer Active death';
    END IF;
    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER complete_successor_reservation_transition_v1
AFTER UPDATE OF reservation_state ON successor_roster_reservations_v1
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_successor_reservation_transition_v1();

CREATE FUNCTION enforce_complete_death_successor_authority_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.death_provenance = 0 THEN
        IF (SELECT count(*)
            FROM death_successor_presets_v1 AS preset
            JOIN death_summary_snapshots AS summary
              ON summary.namespace_id = preset.namespace_id
             AND summary.death_id = preset.death_id
            WHERE preset.namespace_id = NEW.namespace_id
              AND preset.account_id = NEW.account_id
              AND preset.former_character_id = NEW.character_id
              AND preset.death_id = NEW.death_id
              AND preset.former_roster_ordinal = NEW.former_roster_ordinal
              AND preset.class_id = summary.class_id
              AND preset.class_id = 'class.grave_arbalist'
              AND preset.appearance_kind = 0
              AND preset.base_silhouette_id = 'sprite.class.grave_arbalist'
              AND preset.content_revision = NEW.content_revision
              AND summary.content_revision = NEW.content_revision
              AND preset.created_at = NEW.committed_at) <> 1
            OR (SELECT count(*)
                FROM successor_roster_reservations_v1
                WHERE namespace_id = NEW.namespace_id
                  AND account_id = NEW.account_id
                  AND death_id = NEW.death_id
                  AND former_roster_ordinal = NEW.former_roster_ordinal
                  AND reservation_state = 0
                  AND reserved_at = NEW.committed_at) <> 1
            OR (SELECT count(*)
                FROM successor_roster_reservations_v1
                WHERE namespace_id = NEW.namespace_id
                  AND account_id = NEW.account_id
                  AND reservation_state = 2
                  AND superseded_by_death_id = NEW.death_id
                  AND superseded_at = NEW.committed_at) > 1
            OR EXISTS (
                SELECT 1 FROM successor_roster_reservations_v1
                WHERE namespace_id = NEW.namespace_id
                  AND account_id = NEW.account_id
                  AND reservation_state = 1
                  AND consumed_at = NEW.committed_at
            )
        THEN
            RAISE EXCEPTION 'ordinary death successor preset/reservation is incomplete';
        END IF;
    ELSIF EXISTS (
        SELECT 1 FROM death_successor_presets_v1
        WHERE namespace_id = NEW.namespace_id AND death_id = NEW.death_id
    ) OR EXISTS (
        SELECT 1 FROM successor_roster_reservations_v1
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id AND death_id = NEW.death_id
    ) THEN
        RAISE EXCEPTION 'non-ordinary death cannot create successor authority';
    END IF;
    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER complete_death_successor_authority_v1
AFTER INSERT ON death_events
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_complete_death_successor_authority_v1();

CREATE FUNCTION enforce_successor_result_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    result_time TIMESTAMPTZ;
BEGIN
    IF TG_TABLE_NAME = 'successor_mutation_results_v1' THEN
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
    END IF;
    IF TG_TABLE_NAME = 'successor_mutation_outbox_events_v1'
        AND NEW.published_at IS NOT NULL
    THEN
        RAISE EXCEPTION 'successor outbox must be inserted unpublished';
    END IF;
    SELECT committed_at INTO result_time
    FROM successor_mutation_results_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND mutation_id = NEW.mutation_id;
    IF NOT FOUND OR result_time IS DISTINCT FROM transaction_timestamp()
        OR NEW.created_at IS DISTINCT FROM result_time
    THEN
        RAISE EXCEPTION '% may be inserted only with its successor result', TG_TABLE_NAME;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER successor_result_insert_window_v1
BEFORE INSERT ON successor_mutation_results_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_result_insert_window_v1();

CREATE TRIGGER successor_audit_insert_window_v1
BEFORE INSERT ON successor_mutation_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_result_insert_window_v1();

CREATE TRIGGER successor_outbox_insert_window_v1
BEFORE INSERT ON successor_mutation_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_result_insert_window_v1();

CREATE FUNCTION enforce_successor_receipt_insert_window_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    result_time TIMESTAMPTZ;
BEGIN
    SELECT committed_at INTO result_time
    FROM successor_mutation_results_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND mutation_id = NEW.mutation_id;
    IF NOT FOUND
        OR result_time IS DISTINCT FROM transaction_timestamp()
        OR NEW.committed_at IS DISTINCT FROM result_time
    THEN
        RAISE EXCEPTION 'successor receipt may be inserted only with its stored result';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER successor_receipt_insert_window_v1
BEFORE INSERT ON successor_creation_receipts_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_receipt_insert_window_v1();

CREATE FUNCTION enforce_successor_conflict_time_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.created_at IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION 'successor conflict time is PostgreSQL transaction authority';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER successor_conflict_insert_time_v1
BEFORE INSERT ON successor_mutation_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_conflict_time_v1();

CREATE FUNCTION enforce_successor_history_immutable_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
    RAISE EXCEPTION 'successor creation history is immutable';
END
$$;

CREATE TRIGGER successor_result_immutable_v1
BEFORE UPDATE OR DELETE ON successor_mutation_results_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_history_immutable_v1();

CREATE TRIGGER successor_receipt_immutable_v1
BEFORE UPDATE OR DELETE ON successor_creation_receipts_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_history_immutable_v1();

CREATE TRIGGER successor_audit_immutable_v1
BEFORE UPDATE OR DELETE ON successor_mutation_audit_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_history_immutable_v1();

CREATE TRIGGER successor_conflict_immutable_v1
BEFORE UPDATE OR DELETE ON successor_mutation_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_history_immutable_v1();

CREATE FUNCTION enforce_successor_outbox_publish_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'successor outbox history is immutable';
    END IF;
    IF OLD.published_at IS NULL
        AND NEW.published_at IS NOT NULL
        AND ROW(
            NEW.namespace_id, NEW.account_id, NEW.death_id, NEW.mutation_id,
            NEW.successor_id, NEW.receipt_id, NEW.event_id, NEW.event_type,
            NEW.event_payload, NEW.created_at
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.death_id, OLD.mutation_id,
            OLD.successor_id, OLD.receipt_id, OLD.event_id, OLD.event_type,
            OLD.event_payload, OLD.created_at
        )
    THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'successor outbox permits only first publication';
END
$$;

CREATE TRIGGER successor_outbox_publish_only_v1
BEFORE UPDATE OR DELETE ON successor_mutation_outbox_events_v1
FOR EACH ROW EXECUTE FUNCTION enforce_successor_outbox_publish_v1();

CREATE FUNCTION enforce_complete_successor_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM successor_roster_reservations_v1 AS reservation
        JOIN death_successor_presets_v1 AS preset
          ON preset.namespace_id = reservation.namespace_id
         AND preset.account_id = reservation.account_id
         AND preset.death_id = reservation.death_id
        WHERE reservation.namespace_id = NEW.namespace_id
          AND reservation.account_id = NEW.account_id
          AND reservation.death_id = NEW.death_id
          AND reservation.former_roster_ordinal = NEW.former_roster_ordinal
          AND reservation.reservation_state = 1
          AND reservation.consumed_mutation_id = NEW.mutation_id
          AND reservation.consumed_successor_id = NEW.successor_id
          AND reservation.consumed_receipt_id = NEW.receipt_id
          AND reservation.consumed_at = NEW.committed_at
          AND preset.class_id = NEW.class_id
          AND preset.appearance_kind = NEW.appearance_kind
          AND preset.base_silhouette_id = NEW.base_silhouette_id
          AND preset.content_revision = NEW.content_revision
          AND preset.preset_hash = NEW.preset_hash
    ) OR NOT EXISTS (
        SELECT 1
        FROM accounts AS account
        JOIN characters AS character
          ON character.namespace_id = account.namespace_id
         AND character.account_id = account.account_id
         AND character.character_id = NEW.successor_id
        JOIN character_progression AS progression
          ON progression.namespace_id = character.namespace_id
         AND progression.account_id = character.account_id
         AND progression.character_id = character.character_id
        JOIN character_world_locations AS world
          ON world.namespace_id = character.namespace_id
         AND world.account_id = character.account_id
         AND world.character_id = character.character_id
        JOIN character_inventories AS inventory
          ON inventory.namespace_id = character.namespace_id
         AND inventory.account_id = character.account_id
         AND inventory.character_id = character.character_id
        JOIN character_life_metrics AS life
          ON life.namespace_id = character.namespace_id
         AND life.account_id = character.account_id
         AND life.character_id = character.character_id
        JOIN character_oath_bargain_state AS oath
          ON oath.namespace_id = character.namespace_id
         AND oath.account_id = character.account_id
         AND oath.character_id = character.character_id
        WHERE account.namespace_id = NEW.namespace_id
          AND account.account_id = NEW.account_id
          AND account.selected_character_id = NEW.successor_id
          AND account.state_version = NEW.post_account_version
          AND character.roster_ordinal = NEW.former_roster_ordinal
          AND character.class_id = NEW.class_id
          AND character.level = 1
          AND character.oath_id IS NULL
          AND character.life_state = 0
          AND character.security_state = 0
          AND character.character_state_version = NEW.post_character_version
          AND character.created_at = NEW.committed_at
          AND character.updated_at = NEW.committed_at
          AND progression.total_xp = 0
          AND progression.level = 1
          AND progression.current_health = 120
          AND progression.progression_version = NEW.post_progression_version
          AND progression.updated_at = NEW.committed_at
          AND world.character_version = NEW.post_world_version
          AND world.location_kind = 0
          AND world.location_content_id IS NULL
          AND world.safe_arrival_kind = 0
          AND world.safe_spawn_id IS NULL
          AND world.instance_lineage_id IS NULL
          AND world.entry_restore_point_id IS NULL
          AND world.updated_at = NEW.committed_at
          AND inventory.inventory_version = NEW.post_inventory_version
          AND inventory.updated_at = NEW.committed_at
          AND life.lifetime_ticks = 0
          AND life.permadeath_combat_ticks = 0
          AND life.life_metrics_version = NEW.post_life_metrics_version
          AND life.updated_at = NEW.committed_at
          AND oath.earned_bargain_slots = 0
          AND oath.oath_bargain_version = NEW.post_oath_bargain_version
          AND oath.created_at = NEW.committed_at
          AND oath.updated_at = NEW.committed_at
    ) OR EXISTS (
        SELECT 1 FROM character_active_bargains
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id AND character_id = NEW.successor_id
    ) OR EXISTS (
        SELECT 1 FROM bargain_offers
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id AND character_id = NEW.successor_id
    ) OR (SELECT count(*) FROM successor_creation_receipts_v1
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id
          AND mutation_id = NEW.mutation_id
          AND death_id = NEW.death_id
          AND successor_id = NEW.successor_id
          AND receipt_id = NEW.receipt_id
          AND committed_at = NEW.committed_at) <> 1
        OR NOT EXISTS (
            SELECT 1
            FROM successor_creation_receipts_v1 AS receipt
            JOIN starter_initializer_results AS starter
              ON starter.namespace_id = receipt.namespace_id
             AND starter.account_id = receipt.account_id
             AND starter.character_id = receipt.successor_id
             AND starter.initializer_revision = receipt.initializer_revision
            WHERE receipt.namespace_id = NEW.namespace_id
              AND receipt.account_id = NEW.account_id
              AND receipt.mutation_id = NEW.mutation_id
              AND receipt.initializer_revision = 'starter.core-dev.v1'
              AND receipt.item_content_revision = NEW.content_revision
              AND receipt.item_count = 4
              AND starter.request_hash = receipt.initializer_request_hash
              AND starter.result_hash = receipt.initializer_result_hash
              AND starter.pre_inventory_version = 1
              AND starter.post_inventory_version = NEW.post_inventory_version
              AND starter.committed_at = NEW.committed_at
        )
        OR (SELECT count(*) FROM item_instances
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND character_id = NEW.successor_id) <> 4
        OR EXISTS (
            SELECT 1
            FROM successor_creation_receipts_v1 AS receipt
            CROSS JOIN LATERAL (VALUES
                (receipt.weapon_uid, 'item.weapon.crossbow.pine_crossbow', 0, 1, 0, 0, 0, 0, 0),
                (receipt.relic_uid, 'item.relic.arbalist.cracked_mark_lens', 0, 1, 0, 1, 0, 0, 1),
                (receipt.tonic_uid_0, 'consumable.red_tonic', 1, NULL, NULL, 2, 0, 1, 0),
                (receipt.tonic_uid_1, 'consumable.red_tonic', 1, NULL, NULL, 2, 1, 1, 0)
            ) AS expected(
                item_uid, template_id, item_kind, item_level, rarity,
                roll_index, unit_ordinal, location_kind, slot_index
            )
            LEFT JOIN item_instances AS item
              ON item.namespace_id = receipt.namespace_id
             AND item.item_uid = expected.item_uid
            LEFT JOIN item_ledger_events AS ledger
              ON ledger.namespace_id = item.namespace_id
             AND ledger.item_uid = item.item_uid
             AND ledger.post_item_version = 1
            WHERE receipt.namespace_id = NEW.namespace_id
              AND receipt.account_id = NEW.account_id
              AND receipt.mutation_id = NEW.mutation_id
              AND (
                  item.item_uid IS NULL
                  OR item.account_id IS DISTINCT FROM NEW.account_id
                  OR item.character_id IS DISTINCT FROM NEW.successor_id
                  OR item.template_id IS DISTINCT FROM expected.template_id
                  OR item.content_revision IS DISTINCT FROM receipt.item_content_revision
                  OR item.item_kind IS DISTINCT FROM expected.item_kind
                  OR item.item_level IS DISTINCT FROM expected.item_level
                  OR item.rarity IS DISTINCT FROM expected.rarity
                  OR item.creation_kind IS DISTINCT FROM 0
                  OR item.creation_request_id IS DISTINCT FROM NEW.successor_id
                  OR item.roll_index IS DISTINCT FROM expected.roll_index
                  OR item.unit_ordinal IS DISTINCT FROM expected.unit_ordinal
                  OR item.item_version IS DISTINCT FROM 1
                  OR item.security_state IS DISTINCT FROM 0
                  OR item.location_kind IS DISTINCT FROM expected.location_kind
                  OR item.slot_index IS DISTINCT FROM expected.slot_index
                  OR item.instance_id IS NOT NULL
                  OR item.pickup_id IS NOT NULL
                  OR item.expires_at_tick IS NOT NULL
                  OR item.destruction_reason IS NOT NULL
                  OR item.provenance_kind IS DISTINCT FROM
                     CASE WHEN expected.item_kind = 0 THEN 0 ELSE 4 END
                  OR item.salvage_band IS DISTINCT FROM 0
                  OR item.salvage_value IS DISTINCT FROM 0
                  OR item.terminal_death_id IS NOT NULL
                  OR item.terminal_extraction_id IS NOT NULL
                  OR item.terminal_recall_id IS NOT NULL
                  OR item.created_at IS DISTINCT FROM NEW.committed_at
                  OR item.updated_at IS DISTINCT FROM NEW.committed_at
                  OR ledger.ledger_event_id IS DISTINCT FROM expected.item_uid
                  OR ledger.account_id IS DISTINCT FROM NEW.account_id
                  OR ledger.character_id IS DISTINCT FROM NEW.successor_id
                  OR ledger.mutation_id IS DISTINCT FROM NEW.successor_id
                  OR ledger.event_kind IS DISTINCT FROM 0
                  OR ledger.source_kind IS DISTINCT FROM 0
                  OR ledger.pre_item_version IS DISTINCT FROM 0
                  OR ledger.post_item_version IS DISTINCT FROM 1
                  OR ledger.pre_security_state IS NOT NULL
                  OR ledger.post_security_state IS DISTINCT FROM 0
                  OR ledger.pre_location_kind IS NOT NULL
                  OR ledger.post_location_kind IS DISTINCT FROM expected.location_kind
                  OR ledger.reason IS NOT NULL
                  OR ledger.terminal_death_id IS NOT NULL
                  OR ledger.terminal_extraction_id IS NOT NULL
                  OR ledger.terminal_recall_id IS NOT NULL
                  OR ledger.committed_at IS DISTINCT FROM NEW.committed_at
              )
        )
        OR (SELECT count(*) FROM successor_mutation_audit_events_v1
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND death_id = NEW.death_id
              AND mutation_id = NEW.mutation_id
              AND successor_id = NEW.successor_id
              AND event_type = 1
              AND event_digest = NEW.result_hash
              AND created_at = NEW.committed_at) <> 1
        OR (SELECT count(*) FROM successor_mutation_outbox_events_v1
            WHERE namespace_id = NEW.namespace_id
              AND account_id = NEW.account_id
              AND death_id = NEW.death_id
              AND mutation_id = NEW.mutation_id
              AND successor_id = NEW.successor_id
              AND receipt_id = NEW.receipt_id
              AND event_type = 1
              AND event_payload = NEW.result_payload
              AND created_at = NEW.committed_at
              AND published_at IS NULL) <> 1
    THEN
        RAISE EXCEPTION 'successor mutation graph or starter publication is incomplete';
    END IF;
    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER complete_successor_mutation_v1
AFTER INSERT ON successor_mutation_results_v1
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_complete_successor_mutation_v1();

CREATE FUNCTION enforce_reserved_character_insert_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.life_state <> 0 THEN RETURN NEW; END IF;
    IF EXISTS (
        SELECT 1 FROM successor_roster_reservations_v1
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id
          AND reservation_state = 0
    ) THEN
        RAISE EXCEPTION 'successor_resolution_required';
    END IF;
    IF EXISTS (
        SELECT 1 FROM successor_roster_reservations_v1
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id
          AND reservation_state = 1
          AND consumed_at = transaction_timestamp()
    ) AND NOT EXISTS (
        SELECT 1 FROM successor_roster_reservations_v1
        WHERE namespace_id = NEW.namespace_id
          AND account_id = NEW.account_id
          AND reservation_state = 1
          AND consumed_successor_id = NEW.character_id
          AND consumed_at = transaction_timestamp()
    ) THEN
        RAISE EXCEPTION 'only the consumed successor may enter the reserved roster';
    END IF;
    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER complete_reserved_character_insert_v1
AFTER INSERT ON characters
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_reserved_character_insert_v1();

COMMENT ON TABLE death_successor_presets_v1 IS
    'Immutable Core class/base-silhouette preset captured by each ordinary final death.';
COMMENT ON TABLE successor_roster_reservations_v1 IS
    'Latest normal death ordinal authority: Active, Consumed, or explicitly Superseded.';
COMMENT ON TABLE successor_mutation_results_v1 IS
    'Exact-replay protocol 1.17 successor creation result and aggregate version publication.';
COMMENT ON TABLE successor_creation_receipts_v1 IS
    'One-to-one binding from successor result to the exact 04D starter initializer and four UIDs.';
COMMENT ON TABLE successor_mutation_audit_events_v1 IS
    'Immutable accepted successor creation audit event.';
COMMENT ON TABLE successor_mutation_conflict_audits_v1 IS
    'Append-only changed-payload reuse evidence for a stored successor mutation.';
COMMENT ON TABLE successor_mutation_outbox_events_v1 IS
    'Reliable unpublished successor creation delivery sourced only from committed domain state.';
