-- GB-M03-02D / GB-M03-06A exact danger-entry and crash-restore contract v3.
--
-- Authorities: GDD TECH-015/020/021/023, Content CONT-014/CONT-HUB-002,
-- Roadmap GB-M03-02/06/08, and accepted SPEC-CONFLICT-009/027/028. Published migrations
-- 0031-0033 remain immutable. Normal routes are disabled and Core state is wipeable, so this
-- migration fails closed rather than reinterpret an existing v2 restore graph.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM character_entry_restore_points LIMIT 1) THEN
        RAISE EXCEPTION
            '0034 requires no existing danger-entry restore points; clear the wipeable Core namespace';
    END IF;
END
$$;

-- Schema 33 retained item identity with an immediate RESTRICT reference. That reference could
-- fire before the same wipeable-account statement cascaded through its restore root. Deferral
-- preserves gameplay ownership while allowing the explicitly wipeable namespace to clean up
-- atomically.
ALTER TABLE entry_restore_inventory_items_v1
    DROP CONSTRAINT entry_restore_inventory_items_v1_namespace_id_item_uid_fkey,
    ADD CONSTRAINT entry_restore_inventory_item_v1_owned FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid)
        DEFERRABLE INITIALLY DEFERRED;

-- Durable terminal discriminants are append-only: 0 Active, 1 ExtractionCommitted,
-- 2 DeathCommitted, 3 RecallCommitted, 4 CrashRestored.
ALTER TABLE character_entry_restore_points
    DROP CONSTRAINT restore_v2_progression_component_required,
    DROP CONSTRAINT restore_v2_inventory_component_required,
    DROP CONSTRAINT restore_v2_oath_component_required,
    DROP CONSTRAINT restore_v2_life_component_required,
    DROP CONSTRAINT restore_contract_v2,
    DROP CONSTRAINT restore_components_v2_complete,
    ADD COLUMN ash_wallet_version BIGINT NOT NULL,
    ADD COLUMN crash_restore_mutation_id BYTEA,
    ADD CONSTRAINT restore_contract_v3 CHECK (snapshot_contract_version = 3),
    ADD CONSTRAINT restore_components_v3_complete CHECK (component_mask = 31),
    ADD CONSTRAINT restore_ash_wallet_version_positive CHECK (ash_wallet_version > 0),
    ADD CONSTRAINT restore_crash_result_identity_shape CHECK (
        (restore_state = 4 AND crash_restore_mutation_id IS NOT NULL
            AND octet_length(crash_restore_mutation_id) = 16
            AND crash_restore_mutation_id <> decode(repeat('00', 16), 'hex'))
        OR (restore_state <> 4 AND crash_restore_mutation_id IS NULL)
    );

CREATE TABLE entry_restore_progression_v3 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    level SMALLINT NOT NULL,
    total_xp INTEGER NOT NULL,
    current_health INTEGER NOT NULL,
    progression_version BIGINT NOT NULL,
    component_digest BYTEA NOT NULL,
    restored_progression_version BIGINT,
    PRIMARY KEY (namespace_id, restore_point_id),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, progression_version),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, restored_progression_version),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_progression_v3_level_core CHECK (level BETWEEN 1 AND 10),
    CONSTRAINT entry_restore_progression_v3_xp_core CHECK (total_xp BETWEEN 0 AND 2700),
    CONSTRAINT entry_restore_progression_v3_health_living CHECK (current_health >= 1),
    CONSTRAINT entry_restore_progression_v3_versions CHECK (
        progression_version > 0
        AND (restored_progression_version IS NULL
            OR restored_progression_version >= progression_version)
    ),
    CONSTRAINT entry_restore_progression_v3_digest_exact CHECK (
        octet_length(component_digest) = 32
        AND component_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE entry_restore_inventory_v3 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    pre_inventory_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    baseline_item_count SMALLINT NOT NULL,
    safe_placement_count SMALLINT NOT NULL,
    component_digest BYTEA NOT NULL,
    restored_inventory_version BIGINT,
    PRIMARY KEY (namespace_id, restore_point_id),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, post_inventory_version),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, restored_inventory_version),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_inventory_v3_versions CHECK (
        pre_inventory_version > 0
        AND post_inventory_version >= pre_inventory_version
        AND post_inventory_version <= pre_inventory_version + 1
        AND (restored_inventory_version IS NULL
            OR restored_inventory_version >= post_inventory_version)
    ),
    CONSTRAINT entry_restore_inventory_v3_counts CHECK (
        baseline_item_count BETWEEN 0 AND 64
        AND safe_placement_count BETWEEN 0 AND 48
    ),
    CONSTRAINT entry_restore_inventory_v3_digest_exact CHECK (
        octet_length(component_digest) = 32
        AND component_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE entry_restore_inventory_items_v3 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    item_ordinal SMALLINT NOT NULL,
    item_uid BYTEA NOT NULL,
    template_id TEXT NOT NULL,
    content_revision TEXT NOT NULL,
    item_kind SMALLINT NOT NULL,
    creation_kind SMALLINT NOT NULL,
    creation_request_id BYTEA NOT NULL,
    roll_index INTEGER NOT NULL,
    unit_ordinal INTEGER NOT NULL,
    provenance_kind SMALLINT NOT NULL,
    location_kind SMALLINT NOT NULL,
    slot_index SMALLINT NOT NULL,
    entry_item_version BIGINT NOT NULL,
    entry_security_state SMALLINT NOT NULL,
    PRIMARY KEY (namespace_id, restore_point_id, item_ordinal),
    UNIQUE (namespace_id, restore_point_id, item_uid),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES entry_restore_inventory_v3(
            namespace_id, account_id, character_id, restore_point_id
        )
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_inventory_item_v3_ordinal CHECK (item_ordinal BETWEEN 0 AND 63),
    CONSTRAINT entry_restore_inventory_item_v3_id_exact CHECK (
        octet_length(item_uid) = 16
        AND item_uid <> decode(repeat('00', 16), 'hex')
        AND octet_length(creation_request_id) = 16
        AND creation_request_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT entry_restore_inventory_item_v3_content CHECK (
        length(template_id) BETWEEN 3 AND 96
        AND content_revision ~ '^core-dev[.]blake3[.][0-9a-f]{64}$'
    ),
    CONSTRAINT entry_restore_inventory_item_v3_provenance CHECK (
        creation_kind BETWEEN 0 AND 3
        AND roll_index BETWEEN 0 AND 65535
        AND unit_ordinal BETWEEN 0 AND 65535
        AND provenance_kind BETWEEN 0 AND 7
    ),
    CONSTRAINT entry_restore_inventory_item_v3_location CHECK (
        (location_kind = 0 AND item_kind = 0
            AND slot_index BETWEEN 0 AND 3 AND entry_security_state = 1)
        OR (location_kind = 1 AND item_kind = 1
            AND slot_index BETWEEN 0 AND 1 AND entry_security_state = 1)
        OR (location_kind = 2 AND item_kind IN (0, 1)
            AND slot_index BETWEEN 0 AND 7 AND entry_security_state = 2)
    ),
    CONSTRAINT entry_restore_inventory_item_v3_version CHECK (entry_item_version > 0)
);

CREATE TABLE entry_restore_oath_bargain_v3 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    oath_id TEXT,
    earned_bargain_slots SMALLINT NOT NULL,
    active_bargain_count SMALLINT NOT NULL,
    oath_bargain_version BIGINT NOT NULL,
    component_digest BYTEA NOT NULL,
    restored_oath_bargain_version BIGINT,
    PRIMARY KEY (namespace_id, restore_point_id),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, oath_bargain_version),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, restored_oath_bargain_version),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_oath_v3_id CHECK (oath_id IS NULL OR length(oath_id) BETWEEN 3 AND 96),
    CONSTRAINT entry_restore_oath_v3_counts CHECK (
        earned_bargain_slots BETWEEN 0 AND 3
        AND active_bargain_count BETWEEN 0 AND earned_bargain_slots
    ),
    CONSTRAINT entry_restore_oath_v3_versions CHECK (
        oath_bargain_version > 0
        AND (restored_oath_bargain_version IS NULL
            OR restored_oath_bargain_version >= oath_bargain_version)
    ),
    CONSTRAINT entry_restore_oath_v3_digest_exact CHECK (
        octet_length(component_digest) = 32
        AND component_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE entry_restore_active_bargains_v3 (
    namespace_id TEXT NOT NULL,
    restore_point_id BYTEA NOT NULL,
    acquisition_ordinal SMALLINT NOT NULL,
    bargain_id TEXT NOT NULL,
    acquired_by_offer_id BYTEA NOT NULL,
    source_reward_event_id BYTEA NOT NULL,
    content_version TEXT NOT NULL,
    records_blake3 TEXT NOT NULL,
    assets_blake3 TEXT NOT NULL,
    localization_blake3 TEXT NOT NULL,
    PRIMARY KEY (namespace_id, restore_point_id, acquisition_ordinal),
    UNIQUE (namespace_id, restore_point_id, bargain_id),
    UNIQUE (namespace_id, restore_point_id, acquired_by_offer_id),
    FOREIGN KEY (namespace_id, restore_point_id)
        REFERENCES entry_restore_oath_bargain_v3(namespace_id, restore_point_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_bargain_v3_ordinal CHECK (acquisition_ordinal BETWEEN 1 AND 3),
    CONSTRAINT entry_restore_bargain_v3_id CHECK (length(bargain_id) BETWEEN 3 AND 96),
    CONSTRAINT entry_restore_bargain_v3_source_ids CHECK (
        octet_length(acquired_by_offer_id) = 16
        AND acquired_by_offer_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(source_reward_event_id) = 16
        AND source_reward_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT entry_restore_bargain_v3_revision CHECK (
        length(content_version) BETWEEN 1 AND 96
        AND records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    )
);

CREATE TABLE entry_restore_life_metrics_v3 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    captured_lifetime_ticks BIGINT NOT NULL,
    rollback_permadeath_combat_ticks BIGINT NOT NULL,
    life_metrics_version BIGINT NOT NULL,
    component_digest BYTEA NOT NULL,
    restored_life_metrics_version BIGINT,
    PRIMARY KEY (namespace_id, restore_point_id),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, life_metrics_version),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, restored_life_metrics_version),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_life_v3_ticks CHECK (
        captured_lifetime_ticks >= 0
        AND rollback_permadeath_combat_ticks >= 0
        AND rollback_permadeath_combat_ticks <= captured_lifetime_ticks
    ),
    CONSTRAINT entry_restore_life_v3_versions CHECK (
        life_metrics_version > 0
        AND (restored_life_metrics_version IS NULL
            OR restored_life_metrics_version >= life_metrics_version)
    ),
    CONSTRAINT entry_restore_life_v3_digest_exact CHECK (
        octet_length(component_digest) = 32
        AND component_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE entry_restore_ash_wallet_v3 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    ash_wallet_version BIGINT NOT NULL,
    component_digest BYTEA NOT NULL,
    restored_ash_wallet_version BIGINT,
    PRIMARY KEY (namespace_id, restore_point_id),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, ash_wallet_version),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, restored_ash_wallet_version),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_ash_v3_versions CHECK (
        ash_wallet_version > 0
        AND (restored_ash_wallet_version IS NULL
            OR restored_ash_wallet_version >= ash_wallet_version)
    ),
    CONSTRAINT entry_restore_ash_v3_digest_exact CHECK (
        octet_length(component_digest) = 32
        AND component_digest <> decode(repeat('00', 32), 'hex')
    )
);

-- All five v3 components are mandatory at commit. Component-before-root insertion remains legal
-- because both directions are deferred.
ALTER TABLE character_entry_restore_points
    ADD CONSTRAINT restore_v3_progression_component_required FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, progression_version
    ) REFERENCES entry_restore_progression_v3 (
        namespace_id, account_id, character_id, restore_point_id, progression_version
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT restore_v3_inventory_component_required FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, inventory_version
    ) REFERENCES entry_restore_inventory_v3 (
        namespace_id, account_id, character_id, restore_point_id, post_inventory_version
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT restore_v3_oath_component_required FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, oath_bargain_version
    ) REFERENCES entry_restore_oath_bargain_v3 (
        namespace_id, account_id, character_id, restore_point_id, oath_bargain_version
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT restore_v3_life_component_required FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, life_metrics_version
    ) REFERENCES entry_restore_life_metrics_v3 (
        namespace_id, account_id, character_id, restore_point_id, life_metrics_version
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT restore_v3_ash_component_required FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, ash_wallet_version
    ) REFERENCES entry_restore_ash_wallet_v3 (
        namespace_id, account_id, character_id, restore_point_id, ash_wallet_version
    ) DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION validate_entry_restore_inventory_v3_parent(
    target_namespace TEXT,
    target_restore_point BYTEA
)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    expected_count INTEGER;
    actual_count INTEGER;
BEGIN
    SELECT baseline_item_count INTO expected_count FROM entry_restore_inventory_v3
    WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point;
    IF expected_count IS NULL THEN RETURN; END IF;
    SELECT count(*) INTO actual_count FROM entry_restore_inventory_items_v3
    WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point;
    IF actual_count <> expected_count THEN
        RAISE EXCEPTION 'danger-entry v3 inventory count mismatch: expected %, found %',
            expected_count, actual_count;
    END IF;
    IF EXISTS (
        SELECT 1
        FROM entry_restore_inventory_items_v3
        WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point
        GROUP BY location_kind, slot_index
        HAVING (location_kind = 0 AND count(*) > 1)
            OR (location_kind = 1 AND count(*) > 6)
            OR (location_kind = 2 AND max(item_kind) = 0 AND count(*) > 1)
            OR (location_kind = 2 AND max(item_kind) = 1 AND count(*) > 6)
            OR (location_kind IN (1, 2)
                AND count(DISTINCT (item_kind, template_id, content_revision)) <> 1)
    ) THEN
        RAISE EXCEPTION 'danger-entry v3 inventory stack shape is not canonical';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM (
            SELECT item_ordinal,
                row_number() OVER (
                    ORDER BY location_kind, slot_index, item_uid
                ) - 1 AS canonical_ordinal
            FROM entry_restore_inventory_items_v3
            WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point
        ) AS ordered_items
        WHERE item_ordinal <> canonical_ordinal
    ) THEN
        RAISE EXCEPTION 'danger-entry v3 inventory ordinals are not canonical';
    END IF;
END
$$;

CREATE FUNCTION enforce_entry_restore_inventory_v3_count()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'INSERT' AND (
        OLD.namespace_id IS DISTINCT FROM NEW.namespace_id
        OR OLD.restore_point_id IS DISTINCT FROM NEW.restore_point_id
    ) THEN
        PERFORM validate_entry_restore_inventory_v3_parent(
            OLD.namespace_id, OLD.restore_point_id
        );
    END IF;
    PERFORM validate_entry_restore_inventory_v3_parent(
        COALESCE(NEW.namespace_id, OLD.namespace_id),
        COALESCE(NEW.restore_point_id, OLD.restore_point_id)
    );
    RETURN NULL;
END
$$;

CREATE FUNCTION enforce_entry_restore_inventory_v3_item_capture()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM item_instances AS item
        WHERE item.namespace_id = NEW.namespace_id
          AND item.account_id = NEW.account_id
          AND item.character_id = NEW.character_id
          AND item.item_uid = NEW.item_uid
          AND item.template_id = NEW.template_id
          AND item.content_revision = NEW.content_revision
          AND item.item_kind = NEW.item_kind
          AND item.creation_kind = NEW.creation_kind
          AND item.creation_request_id = NEW.creation_request_id
          AND item.roll_index = NEW.roll_index
          AND item.unit_ordinal = NEW.unit_ordinal
          AND item.provenance_kind = NEW.provenance_kind
          AND item.location_kind = NEW.location_kind
          AND item.slot_index = NEW.slot_index
          AND item.item_version = NEW.entry_item_version
          AND item.security_state = NEW.entry_security_state
    ) THEN
        RAISE EXCEPTION 'danger-entry v3 item capture does not match authoritative item state';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER entry_restore_inventory_v3_item_capture_exact
AFTER INSERT OR UPDATE ON entry_restore_inventory_items_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_inventory_v3_item_capture();

CREATE CONSTRAINT TRIGGER entry_restore_inventory_v3_count_complete
AFTER INSERT OR UPDATE ON entry_restore_inventory_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_inventory_v3_count();

CREATE CONSTRAINT TRIGGER entry_restore_inventory_v3_child_count_complete
AFTER INSERT OR UPDATE OR DELETE ON entry_restore_inventory_items_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_inventory_v3_count();

CREATE FUNCTION validate_entry_restore_oath_v3_parent(
    target_namespace TEXT,
    target_restore_point BYTEA
)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    expected_count INTEGER;
    actual_count INTEGER;
BEGIN
    SELECT active_bargain_count INTO expected_count FROM entry_restore_oath_bargain_v3
    WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point;
    IF expected_count IS NULL THEN RETURN; END IF;
    SELECT count(*) INTO actual_count FROM entry_restore_active_bargains_v3
    WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point;
    IF actual_count <> expected_count THEN
        RAISE EXCEPTION 'danger-entry v3 Oath/Bargain count mismatch: expected %, found %',
            expected_count, actual_count;
    END IF;
END
$$;

CREATE FUNCTION enforce_entry_restore_oath_v3_count()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'INSERT' AND (
        OLD.namespace_id IS DISTINCT FROM NEW.namespace_id
        OR OLD.restore_point_id IS DISTINCT FROM NEW.restore_point_id
    ) THEN
        PERFORM validate_entry_restore_oath_v3_parent(
            OLD.namespace_id, OLD.restore_point_id
        );
    END IF;
    PERFORM validate_entry_restore_oath_v3_parent(
        COALESCE(NEW.namespace_id, OLD.namespace_id),
        COALESCE(NEW.restore_point_id, OLD.restore_point_id)
    );
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER entry_restore_oath_v3_count_complete
AFTER INSERT OR UPDATE ON entry_restore_oath_bargain_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_oath_v3_count();

CREATE CONSTRAINT TRIGGER entry_restore_oath_v3_child_count_complete
AFTER INSERT OR UPDATE OR DELETE ON entry_restore_active_bargains_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_oath_v3_count();

CREATE FUNCTION enforce_entry_restore_bargain_v3_source()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM entry_restore_oath_bargain_v3 AS snapshot
        JOIN character_active_bargains AS active
          ON active.namespace_id = snapshot.namespace_id
         AND active.account_id = snapshot.account_id
         AND active.character_id = snapshot.character_id
         AND active.bargain_id = NEW.bargain_id
         AND active.acquisition_ordinal = NEW.acquisition_ordinal
         AND active.acquired_by_offer_id = NEW.acquired_by_offer_id
        JOIN bargain_offers AS offer
          ON offer.namespace_id = active.namespace_id
         AND offer.account_id = active.account_id
         AND offer.character_id = active.character_id
         AND offer.offer_id = active.acquired_by_offer_id
         AND offer.source_reward_event_id = NEW.source_reward_event_id
         AND offer.content_version = NEW.content_version
         AND offer.records_blake3 = NEW.records_blake3
         AND offer.assets_blake3 = NEW.assets_blake3
         AND offer.localization_blake3 = NEW.localization_blake3
        WHERE snapshot.namespace_id = NEW.namespace_id
          AND snapshot.restore_point_id = NEW.restore_point_id
    ) THEN
        RAISE EXCEPTION 'danger-entry v3 Bargain capture does not match authoritative source';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER entry_restore_bargain_v3_source_exact
AFTER INSERT OR UPDATE ON entry_restore_active_bargains_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_bargain_v3_source();

CREATE FUNCTION enforce_entry_restore_v3_component_immutability()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    valid_recovery_transition BOOLEAN := FALSE;
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'danger-entry v3 component history is immutable';
    END IF;
    IF TG_TABLE_NAME = 'entry_restore_progression_v3' THEN
        valid_recovery_transition := ROW(
            NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.restore_point_id,
            NEW.level, NEW.total_xp, NEW.current_health, NEW.progression_version,
            NEW.component_digest
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.restore_point_id,
            OLD.level, OLD.total_xp, OLD.current_health, OLD.progression_version,
            OLD.component_digest
        ) AND OLD.restored_progression_version IS NULL
          AND NEW.restored_progression_version IS NOT NULL;
    ELSIF TG_TABLE_NAME = 'entry_restore_inventory_v3' THEN
        valid_recovery_transition := ROW(
            NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.restore_point_id,
            NEW.pre_inventory_version, NEW.post_inventory_version, NEW.baseline_item_count,
            NEW.safe_placement_count, NEW.component_digest
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.restore_point_id,
            OLD.pre_inventory_version, OLD.post_inventory_version, OLD.baseline_item_count,
            OLD.safe_placement_count, OLD.component_digest
        ) AND OLD.restored_inventory_version IS NULL
          AND NEW.restored_inventory_version IS NOT NULL;
    ELSIF TG_TABLE_NAME = 'entry_restore_oath_bargain_v3' THEN
        valid_recovery_transition := ROW(
            NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.restore_point_id,
            NEW.oath_id, NEW.earned_bargain_slots, NEW.active_bargain_count,
            NEW.oath_bargain_version, NEW.component_digest
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.restore_point_id,
            OLD.oath_id, OLD.earned_bargain_slots, OLD.active_bargain_count,
            OLD.oath_bargain_version, OLD.component_digest
        ) AND OLD.restored_oath_bargain_version IS NULL
          AND NEW.restored_oath_bargain_version IS NOT NULL;
    ELSIF TG_TABLE_NAME = 'entry_restore_life_metrics_v3' THEN
        valid_recovery_transition := ROW(
            NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.restore_point_id,
            NEW.captured_lifetime_ticks, NEW.rollback_permadeath_combat_ticks,
            NEW.life_metrics_version, NEW.component_digest
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.restore_point_id,
            OLD.captured_lifetime_ticks, OLD.rollback_permadeath_combat_ticks,
            OLD.life_metrics_version, OLD.component_digest
        ) AND OLD.restored_life_metrics_version IS NULL
          AND NEW.restored_life_metrics_version IS NOT NULL;
    ELSIF TG_TABLE_NAME = 'entry_restore_ash_wallet_v3' THEN
        valid_recovery_transition := ROW(
            NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.restore_point_id,
            NEW.ash_wallet_version, NEW.component_digest
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.restore_point_id,
            OLD.ash_wallet_version, OLD.component_digest
        ) AND OLD.restored_ash_wallet_version IS NULL
          AND NEW.restored_ash_wallet_version IS NOT NULL;
    END IF;
    IF NOT valid_recovery_transition THEN
        RAISE EXCEPTION 'danger-entry v3 component history is immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER entry_restore_progression_v3_immutable
BEFORE UPDATE OR DELETE ON entry_restore_progression_v3
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_v3_component_immutability();
CREATE TRIGGER entry_restore_inventory_v3_immutable
BEFORE UPDATE OR DELETE ON entry_restore_inventory_v3
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_v3_component_immutability();
CREATE TRIGGER entry_restore_oath_v3_immutable
BEFORE UPDATE OR DELETE ON entry_restore_oath_bargain_v3
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_v3_component_immutability();
CREATE TRIGGER entry_restore_life_v3_immutable
BEFORE UPDATE OR DELETE ON entry_restore_life_metrics_v3
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_v3_component_immutability();
CREATE TRIGGER entry_restore_ash_v3_immutable
BEFORE UPDATE OR DELETE ON entry_restore_ash_wallet_v3
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_v3_component_immutability();

CREATE FUNCTION reject_entry_restore_v3_child_mutation()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
    RAISE EXCEPTION 'danger-entry v3 snapshot children are immutable';
END
$$;

CREATE TRIGGER entry_restore_inventory_items_v3_immutable
BEFORE UPDATE OR DELETE ON entry_restore_inventory_items_v3
FOR EACH ROW EXECUTE FUNCTION reject_entry_restore_v3_child_mutation();
CREATE TRIGGER entry_restore_active_bargains_v3_immutable
BEFORE UPDATE OR DELETE ON entry_restore_active_bargains_v3
FOR EACH ROW EXECUTE FUNCTION reject_entry_restore_v3_child_mutation();

CREATE FUNCTION enforce_entry_restore_v3_terminal_state()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT := COALESCE(NEW.namespace_id, OLD.namespace_id);
    target_restore_point BYTEA := COALESCE(NEW.restore_point_id, OLD.restore_point_id);
    target_state SMALLINT;
    recovery_complete BOOLEAN;
    recovery_pristine BOOLEAN;
BEGIN
    SELECT root.restore_state,
        progression.restored_progression_version IS NOT NULL
            AND inventory.restored_inventory_version IS NOT NULL
            AND oath.restored_oath_bargain_version IS NOT NULL
            AND life.restored_life_metrics_version IS NOT NULL
            AND ash.restored_ash_wallet_version IS NOT NULL,
        progression.restored_progression_version IS NULL
            AND inventory.restored_inventory_version IS NULL
            AND oath.restored_oath_bargain_version IS NULL
            AND life.restored_life_metrics_version IS NULL
            AND ash.restored_ash_wallet_version IS NULL
    INTO target_state, recovery_complete, recovery_pristine
    FROM character_entry_restore_points AS root
    JOIN entry_restore_progression_v3 AS progression USING (namespace_id, restore_point_id)
    JOIN entry_restore_inventory_v3 AS inventory USING (namespace_id, restore_point_id)
    JOIN entry_restore_oath_bargain_v3 AS oath USING (namespace_id, restore_point_id)
    JOIN entry_restore_life_metrics_v3 AS life USING (namespace_id, restore_point_id)
    JOIN entry_restore_ash_wallet_v3 AS ash USING (namespace_id, restore_point_id)
    WHERE root.namespace_id = target_namespace
      AND root.restore_point_id = target_restore_point;
    IF NOT FOUND THEN RETURN NULL; END IF;
    IF (target_state = 4 AND NOT recovery_complete)
        OR (target_state <> 4 AND NOT recovery_pristine) THEN
        RAISE EXCEPTION 'danger-entry v3 recovery versions do not match terminal state';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER entry_restore_v3_root_terminal_complete
AFTER INSERT OR UPDATE ON character_entry_restore_points
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_v3_terminal_state();
CREATE CONSTRAINT TRIGGER entry_restore_v3_progression_terminal_complete
AFTER INSERT OR UPDATE ON entry_restore_progression_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_v3_terminal_state();
CREATE CONSTRAINT TRIGGER entry_restore_v3_inventory_terminal_complete
AFTER INSERT OR UPDATE ON entry_restore_inventory_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_v3_terminal_state();
CREATE CONSTRAINT TRIGGER entry_restore_v3_oath_terminal_complete
AFTER INSERT OR UPDATE ON entry_restore_oath_bargain_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_v3_terminal_state();
CREATE CONSTRAINT TRIGGER entry_restore_v3_life_terminal_complete
AFTER INSERT OR UPDATE ON entry_restore_life_metrics_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_v3_terminal_state();
CREATE CONSTRAINT TRIGGER entry_restore_v3_ash_terminal_complete
AFTER INSERT OR UPDATE ON entry_restore_ash_wallet_v3
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_entry_restore_v3_terminal_state();

CREATE FUNCTION enforce_entry_restore_v3_root_immutability()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    valid_terminal_transition BOOLEAN;
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.restore_state = 0 AND NEW.consumed_at IS NULL
            AND NEW.crash_restore_mutation_id IS NULL THEN
            RETURN NEW;
        END IF;
        RAISE EXCEPTION 'danger-entry v3 root must begin Active';
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'danger-entry v3 root history is immutable';
    END IF;
    valid_terminal_transition := OLD.restore_state = 0
        AND NEW.restore_state BETWEEN 1 AND 4
        AND OLD.consumed_at IS NULL
        AND NEW.consumed_at IS NOT NULL
        AND ROW(
            NEW.namespace_id, NEW.account_id, NEW.character_id, NEW.restore_point_id,
            NEW.lineage_id, NEW.source_location_id, NEW.restore_location_id,
            NEW.content_revision, NEW.snapshot_contract_version, NEW.account_version,
            NEW.character_version, NEW.progression_version, NEW.inventory_version,
            NEW.oath_bargain_version, NEW.life_metrics_version, NEW.ash_wallet_version,
            NEW.component_mask, NEW.composite_digest, NEW.created_at
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.restore_point_id,
            OLD.lineage_id, OLD.source_location_id, OLD.restore_location_id,
            OLD.content_revision, OLD.snapshot_contract_version, OLD.account_version,
            OLD.character_version, OLD.progression_version, OLD.inventory_version,
            OLD.oath_bargain_version, OLD.life_metrics_version, OLD.ash_wallet_version,
            OLD.component_mask, OLD.composite_digest, OLD.created_at
        );
    IF NOT valid_terminal_transition THEN
        RAISE EXCEPTION 'danger-entry v3 root history is immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER entry_restore_v3_root_immutable
BEFORE INSERT OR UPDATE OR DELETE ON character_entry_restore_points
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_v3_root_immutability();

-- A consumed Belt unit remains addressable by UID. Durable discriminants only append:
-- security 4 Consumed, location 7 Consumed, event 3 Consumed, event 4 CrashResolution,
-- source 4 CrashRestore.
ALTER TABLE item_instances
    DROP CONSTRAINT item_security_known,
    DROP CONSTRAINT item_location_known,
    DROP CONSTRAINT item_location_shape,
    ADD CONSTRAINT item_security_known CHECK (security_state BETWEEN 0 AND 4),
    ADD CONSTRAINT item_location_known CHECK (location_kind BETWEEN 0 AND 7),
    ADD CONSTRAINT item_location_shape CHECK (
        (location_kind = 0 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 3 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND security_state IN (0, 1) AND item_kind = 0)
        OR (location_kind = 1 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 1 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND security_state IN (0, 1) AND item_kind = 1)
        OR (location_kind = 2 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL AND security_state = 2)
        OR (location_kind = 3 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NOT NULL AND octet_length(instance_id) = 16
            AND instance_id <> decode(repeat('00', 16), 'hex')
            AND pickup_id IS NOT NULL AND octet_length(pickup_id) = 16
            AND pickup_id <> decode(repeat('00', 16), 'hex')
            AND expires_at_tick > 0 AND destruction_reason IS NULL AND security_state = 2)
        OR (location_kind = 4 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NOT NULL
            AND destruction_reason IN ('ground_expired', 'permadeath', 'crash_revoked')
            AND security_state = 3)
        OR (location_kind = 5 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL AND security_state = 0)
        OR (location_kind = 6 AND character_id IS NULL
            AND slot_index BETWEEN 0 AND 159 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL AND security_state = 0)
        OR (location_kind = 7 AND character_id IS NOT NULL AND item_kind = 1
            AND slot_index BETWEEN 0 AND 1 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NOT NULL
            AND destruction_reason = 'consumed' AND security_state = 4)
    );

ALTER TABLE item_ledger_events
    DROP CONSTRAINT ledger_event_kind_known,
    DROP CONSTRAINT ledger_source_kind_known,
    DROP CONSTRAINT ledger_security_known,
    DROP CONSTRAINT ledger_location_known,
    DROP CONSTRAINT ledger_creation_shape,
    ADD CONSTRAINT ledger_event_kind_known CHECK (event_kind BETWEEN 0 AND 4),
    ADD CONSTRAINT ledger_source_kind_known CHECK (source_kind BETWEEN 0 AND 4),
    ADD CONSTRAINT ledger_security_known CHECK (
        (pre_security_state IS NULL OR pre_security_state BETWEEN 0 AND 4)
        AND post_security_state BETWEEN 0 AND 4
    ),
    ADD CONSTRAINT ledger_location_known CHECK (
        (pre_location_kind IS NULL OR pre_location_kind BETWEEN 0 AND 7)
        AND post_location_kind BETWEEN 0 AND 7
    ),
    ADD CONSTRAINT ledger_creation_shape CHECK (
        (event_kind = 0 AND pre_item_version = 0
            AND pre_security_state IS NULL AND pre_location_kind IS NULL AND reason IS NULL)
        OR (event_kind = 1 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL AND reason IS NULL)
        OR (event_kind = 2 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND post_security_state = 3 AND post_location_kind = 4
            AND reason IS NOT NULL
            AND reason IN ('ground_expired', 'permadeath'))
        OR (event_kind = 3 AND pre_item_version > 0 AND source_kind <> 4
            AND pre_security_state = 1 AND pre_location_kind = 1
            AND post_security_state = 4 AND post_location_kind = 7
            AND reason IS NOT NULL AND reason = 'consumed')
        OR (event_kind = 4 AND pre_item_version > 0 AND source_kind = 4
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND reason IS NOT NULL
            AND (
                (reason = 'crash_restored'
                    AND ((post_security_state = 0 AND post_location_kind IN (0, 1))
                        OR (post_security_state = 2 AND post_location_kind = 2)))
                OR (reason = 'crash_revoked'
                    AND post_security_state = 3 AND post_location_kind = 4)
            ))
    );

ALTER TABLE item_ledger_events
    ADD CONSTRAINT item_ledger_crash_resolution_identity UNIQUE (
        namespace_id, account_id, character_id, item_uid, mutation_id, ledger_event_id,
        event_kind, source_kind, pre_item_version, post_item_version,
        pre_security_state, post_security_state, pre_location_kind, post_location_kind, reason
    );

ALTER TABLE character_run_material_stacks
    ADD COLUMN terminal_reason TEXT,
    ADD COLUMN terminal_restore_point_id BYTEA,
    DROP CONSTRAINT run_material_security_shape,
    ADD CONSTRAINT run_material_security_shape CHECK (
        (security_state = 2 AND quantity > 0
            AND terminal_reason IS NULL AND terminal_restore_point_id IS NULL)
        OR (security_state = 3 AND quantity = 0
            AND terminal_reason IS NOT NULL
            AND terminal_reason IN ('permadeath', 'crash_revoked')
            AND ((terminal_reason = 'permadeath' AND terminal_restore_point_id IS NULL)
                OR (terminal_reason = 'crash_revoked'
                    AND terminal_restore_point_id IS NOT NULL
                    AND octet_length(terminal_restore_point_id) = 16
                    AND terminal_restore_point_id <> decode(repeat('00', 16), 'hex'))))
    ),
    ADD CONSTRAINT run_material_terminal_restore_owned FOREIGN KEY (
        namespace_id, account_id, character_id, terminal_restore_point_id
    ) REFERENCES character_entry_restore_points(
        namespace_id, account_id, character_id, restore_point_id
    ) DEFERRABLE INITIALLY DEFERRED;

-- Post-entry Bargain and Ash records retain their original outcome and gain explicit recovery
-- metadata. Replay can therefore return a typed revoked result without rewriting history.
ALTER TABLE bargain_offers
    ADD COLUMN revoked_by_restore_point_id BYTEA,
    ADD COLUMN revoked_at TIMESTAMPTZ,
    ADD CONSTRAINT bargain_offer_crash_revocation_shape CHECK (
        (revoked_by_restore_point_id IS NULL AND revoked_at IS NULL)
        OR (revoked_by_restore_point_id IS NOT NULL AND revoked_at IS NOT NULL
            AND octet_length(revoked_by_restore_point_id) = 16
            AND revoked_by_restore_point_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT bargain_offer_crash_restore_owned FOREIGN KEY (
        namespace_id, account_id, character_id, revoked_by_restore_point_id
    ) REFERENCES character_entry_restore_points(
        namespace_id, account_id, character_id, restore_point_id
    ) DEFERRABLE INITIALLY DEFERRED;

DROP INDEX open_bargain_offers_by_character;
CREATE INDEX open_bargain_offers_by_character
    ON bargain_offers (namespace_id, account_id, character_id, created_at, offer_id)
    WHERE offer_state = 0 AND revoked_by_restore_point_id IS NULL;

ALTER TABLE bargain_milestone_results
    ADD COLUMN revoked_by_restore_point_id BYTEA,
    ADD COLUMN revoked_at TIMESTAMPTZ,
    ADD CONSTRAINT bargain_milestone_crash_revocation_shape CHECK (
        (revoked_by_restore_point_id IS NULL AND revoked_at IS NULL)
        OR (revoked_by_restore_point_id IS NOT NULL AND revoked_at IS NOT NULL
            AND revoked_by_restore_point_id = entry_restore_point_id)
    ),
    ADD CONSTRAINT bargain_milestone_crash_restore_owned FOREIGN KEY (
        namespace_id, account_id, character_id, revoked_by_restore_point_id
    ) REFERENCES character_entry_restore_points(
        namespace_id, account_id, character_id, restore_point_id
    ) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE bargain_decision_results
    ADD COLUMN revoked_by_restore_point_id BYTEA,
    ADD COLUMN revoked_at TIMESTAMPTZ,
    ADD CONSTRAINT bargain_decision_crash_revocation_shape CHECK (
        (revoked_by_restore_point_id IS NULL AND revoked_at IS NULL)
        OR (revoked_by_restore_point_id IS NOT NULL AND revoked_at IS NOT NULL
            AND octet_length(revoked_by_restore_point_id) = 16
            AND revoked_by_restore_point_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT bargain_decision_crash_restore_owned FOREIGN KEY (
        namespace_id, account_id, character_id, revoked_by_restore_point_id
    ) REFERENCES character_entry_restore_points(
        namespace_id, account_id, character_id, restore_point_id
    ) DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION enforce_bargain_crash_revocation_immutability_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.revoked_by_restore_point_id IS NULL OR pg_trigger_depth() > 1 THEN
            RETURN OLD;
        END IF;
        RAISE EXCEPTION 'crash-revoked Bargain source history is immutable';
    END IF;
    IF OLD.revoked_by_restore_point_id IS NOT NULL AND (
        NEW.revoked_by_restore_point_id IS DISTINCT FROM OLD.revoked_by_restore_point_id
        OR NEW.revoked_at IS DISTINCT FROM OLD.revoked_at
    ) THEN
        RAISE EXCEPTION 'crash-revoked Bargain source history is immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER bargain_offer_crash_revocation_immutable
BEFORE UPDATE OR DELETE ON bargain_offers
FOR EACH ROW EXECUTE FUNCTION enforce_bargain_crash_revocation_immutability_v3();
CREATE TRIGGER bargain_milestone_crash_revocation_immutable
BEFORE UPDATE OR DELETE ON bargain_milestone_results
FOR EACH ROW EXECUTE FUNCTION enforce_bargain_crash_revocation_immutability_v3();
CREATE TRIGGER bargain_decision_crash_revocation_immutable
BEFORE UPDATE OR DELETE ON bargain_decision_results
FOR EACH ROW EXECUTE FUNCTION enforce_bargain_crash_revocation_immutability_v3();

ALTER TABLE ash_mutation_results
    ADD COLUMN entry_restore_point_id BYTEA,
    ADD COLUMN reversed_by_restore_point_id BYTEA,
    ADD COLUMN reversed_by_mutation_id BYTEA,
    ADD COLUMN reversed_at TIMESTAMPTZ,
    ADD CONSTRAINT ash_result_danger_binding_shape CHECK (
        entry_restore_point_id IS NULL
        OR (octet_length(entry_restore_point_id) = 16
            AND entry_restore_point_id <> decode(repeat('00', 16), 'hex'))
    ),
    ADD CONSTRAINT ash_result_crash_reversal_shape CHECK (
        (reversed_by_restore_point_id IS NULL AND reversed_by_mutation_id IS NULL
            AND reversed_at IS NULL)
        OR (entry_restore_point_id IS NOT NULL
            AND reversed_by_restore_point_id IS NOT NULL
            AND reversed_by_mutation_id IS NOT NULL
            AND reversed_at IS NOT NULL
            AND reversed_by_restore_point_id = entry_restore_point_id
            AND octet_length(reversed_by_mutation_id) = 16
            AND reversed_by_mutation_id <> decode(repeat('00', 16), 'hex'))
    ),
    ADD CONSTRAINT ash_result_entry_restore_owned FOREIGN KEY (
        namespace_id, entry_restore_point_id
    ) REFERENCES character_entry_restore_points(namespace_id, restore_point_id)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT ash_result_reversal_restore_owned FOREIGN KEY (
        namespace_id, reversed_by_restore_point_id
    ) REFERENCES character_entry_restore_points(namespace_id, restore_point_id)
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT ash_result_reversal_mutation_owned FOREIGN KEY (
        namespace_id, account_id, reversed_by_mutation_id
    ) REFERENCES ash_mutation_results(namespace_id, account_id, mutation_id)
        DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION enforce_ash_crash_binding_immutability_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF (OLD.entry_restore_point_id IS NULL
                AND OLD.reversed_by_restore_point_id IS NULL)
            OR pg_trigger_depth() > 1 THEN
            RETURN OLD;
        END IF;
        RAISE EXCEPTION 'danger-bound Ash result history is immutable';
    END IF;
    IF OLD.entry_restore_point_id IS NOT NULL
        AND NEW.entry_restore_point_id IS DISTINCT FROM OLD.entry_restore_point_id THEN
        RAISE EXCEPTION 'danger-bound Ash result history is immutable';
    END IF;
    IF OLD.reversed_by_restore_point_id IS NOT NULL AND (
        NEW.entry_restore_point_id IS DISTINCT FROM OLD.entry_restore_point_id
        OR NEW.reversed_by_restore_point_id IS DISTINCT FROM OLD.reversed_by_restore_point_id
        OR NEW.reversed_by_mutation_id IS DISTINCT FROM OLD.reversed_by_mutation_id
        OR NEW.reversed_at IS DISTINCT FROM OLD.reversed_at
    ) THEN
        RAISE EXCEPTION 'danger-bound Ash result history is immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER ash_crash_binding_immutable
BEFORE UPDATE OR DELETE ON ash_mutation_results
FOR EACH ROW EXECUTE FUNCTION enforce_ash_crash_binding_immutability_v3();

CREATE TABLE danger_crash_restore_results (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    request_hash BYTEA NOT NULL,
    result_code SMALLINT NOT NULL,
    post_account_version BIGINT NOT NULL,
    post_character_version BIGINT NOT NULL,
    post_progression_version BIGINT NOT NULL,
    post_inventory_version BIGINT NOT NULL,
    post_oath_bargain_version BIGINT NOT NULL,
    post_life_metrics_version BIGINT NOT NULL,
    post_ash_wallet_version BIGINT NOT NULL,
    restored_item_count INTEGER NOT NULL,
    revoked_item_count INTEGER NOT NULL,
    revoked_material_count INTEGER NOT NULL,
    revoked_bargain_record_count INTEGER NOT NULL,
    compensated_ash_count INTEGER NOT NULL,
    result_digest BYTEA NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, mutation_id),
    UNIQUE (namespace_id, restore_point_id),
    UNIQUE (namespace_id, account_id, character_id, mutation_id),
    UNIQUE (namespace_id, account_id, character_id, restore_point_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id, post_progression_version)
        REFERENCES entry_restore_progression_v3(
            namespace_id, account_id, character_id, restore_point_id, restored_progression_version
        ) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id, post_inventory_version)
        REFERENCES entry_restore_inventory_v3(
            namespace_id, account_id, character_id, restore_point_id, restored_inventory_version
        ) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id, post_oath_bargain_version)
        REFERENCES entry_restore_oath_bargain_v3(
            namespace_id, account_id, character_id, restore_point_id, restored_oath_bargain_version
        ) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id, post_life_metrics_version)
        REFERENCES entry_restore_life_metrics_v3(
            namespace_id, account_id, character_id, restore_point_id, restored_life_metrics_version
        ) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id, post_ash_wallet_version)
        REFERENCES entry_restore_ash_wallet_v3(
            namespace_id, account_id, character_id, restore_point_id, restored_ash_wallet_version
        ) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT danger_crash_result_ids_exact CHECK (
        octet_length(mutation_id) = 16
        AND mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT danger_crash_result_hashes_exact CHECK (
        octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_digest) = 32
        AND result_digest <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT danger_crash_result_committed CHECK (result_code = 0),
    CONSTRAINT danger_crash_result_versions_positive CHECK (
        post_account_version > 0 AND post_character_version > 0
        AND post_progression_version > 0 AND post_inventory_version > 0
        AND post_oath_bargain_version > 0 AND post_life_metrics_version > 0
        AND post_ash_wallet_version > 0
    ),
    CONSTRAINT danger_crash_result_counts_bounded CHECK (
        restored_item_count BETWEEN 0 AND 64
        AND revoked_item_count BETWEEN 0 AND 4095
        AND restored_item_count + revoked_item_count BETWEEN 0 AND 4095
        AND revoked_material_count BETWEEN 0 AND 4095
        AND revoked_bargain_record_count BETWEEN 0 AND 4095
        AND compensated_ash_count BETWEEN 0 AND 4095
    )
);

ALTER TABLE character_entry_restore_points
    ADD CONSTRAINT restore_v3_crash_result_required FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, crash_restore_mutation_id
    ) REFERENCES danger_crash_restore_results(
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    ) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE danger_crash_restore_item_changes (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    change_ordinal INTEGER NOT NULL,
    change_kind SMALLINT NOT NULL,
    item_uid BYTEA NOT NULL,
    ledger_event_id BYTEA NOT NULL,
    ledger_event_kind SMALLINT NOT NULL,
    ledger_source_kind SMALLINT NOT NULL,
    ledger_reason TEXT NOT NULL,
    pre_item_version BIGINT NOT NULL,
    post_item_version BIGINT NOT NULL,
    pre_security_state SMALLINT NOT NULL,
    post_security_state SMALLINT NOT NULL,
    pre_location_kind SMALLINT NOT NULL,
    post_location_kind SMALLINT NOT NULL,
    post_slot_index SMALLINT,
    PRIMARY KEY (namespace_id, account_id, mutation_id, change_ordinal),
    UNIQUE (namespace_id, account_id, mutation_id, item_uid),
    FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    )
        REFERENCES danger_crash_restore_results(
            namespace_id, account_id, character_id, restore_point_id, mutation_id
        ) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (
        namespace_id, account_id, character_id, item_uid, mutation_id, ledger_event_id,
        ledger_event_kind, ledger_source_kind, pre_item_version, post_item_version,
        pre_security_state, post_security_state, pre_location_kind, post_location_kind,
        ledger_reason
    ) REFERENCES item_ledger_events(
        namespace_id, account_id, character_id, item_uid, mutation_id, ledger_event_id,
        event_kind, source_kind, pre_item_version, post_item_version,
        pre_security_state, post_security_state, pre_location_kind, post_location_kind, reason
    ) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT danger_crash_item_ordinal_bounded CHECK (change_ordinal BETWEEN 0 AND 4094),
    CONSTRAINT danger_crash_item_kind_known CHECK (change_kind IN (0, 1)),
    CONSTRAINT danger_crash_item_versions_exact CHECK (
        pre_item_version > 0 AND post_item_version = pre_item_version + 1
    ),
    CONSTRAINT danger_crash_item_ledger_exact CHECK (
        ledger_event_kind = 4 AND ledger_source_kind = 4
        AND ledger_reason = CASE change_kind
            WHEN 0 THEN 'crash_restored'
            WHEN 1 THEN 'crash_revoked'
        END
    ),
    CONSTRAINT danger_crash_item_shape CHECK (
        (change_kind = 0 AND (
            (post_security_state = 0 AND post_location_kind = 0
                AND post_slot_index IS NOT NULL AND post_slot_index BETWEEN 0 AND 3)
            OR (post_security_state = 0 AND post_location_kind = 1
                AND post_slot_index IS NOT NULL AND post_slot_index BETWEEN 0 AND 1)
            OR (post_security_state = 2 AND post_location_kind = 2
                AND post_slot_index IS NOT NULL AND post_slot_index BETWEEN 0 AND 7)))
        OR (change_kind = 1 AND post_security_state = 3
            AND post_location_kind = 4 AND post_slot_index IS NULL)
    )
);

CREATE FUNCTION enforce_danger_crash_item_change_source_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM item_instances AS item
        WHERE item.namespace_id = NEW.namespace_id
          AND item.account_id = NEW.account_id
          AND item.character_id = NEW.character_id
          AND item.item_uid = NEW.item_uid
          AND item.item_version = NEW.post_item_version
          AND item.security_state = NEW.post_security_state
          AND item.location_kind = NEW.post_location_kind
          AND item.slot_index IS NOT DISTINCT FROM NEW.post_slot_index
          AND (
              (NEW.change_kind = 0 AND item.destruction_reason IS NULL)
              OR (NEW.change_kind = 1 AND item.destruction_reason = 'crash_revoked')
          )
    ) THEN
        RAISE EXCEPTION 'danger crash item change is not bound to authoritative item state';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER danger_crash_item_change_source_exact
AFTER INSERT OR UPDATE ON danger_crash_restore_item_changes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_item_change_source_v3();

CREATE TABLE danger_crash_restore_material_changes (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    change_ordinal INTEGER NOT NULL,
    material_id TEXT NOT NULL,
    pre_quantity INTEGER NOT NULL,
    pre_material_version BIGINT NOT NULL,
    post_material_version BIGINT NOT NULL,
    PRIMARY KEY (namespace_id, account_id, mutation_id, change_ordinal),
    UNIQUE (namespace_id, account_id, mutation_id, material_id),
    FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    ) REFERENCES danger_crash_restore_results(
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    )
        ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, material_id)
        REFERENCES character_run_material_stacks(
            namespace_id, account_id, character_id, material_id
        ) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT danger_crash_material_ordinal_bounded CHECK (change_ordinal BETWEEN 0 AND 4094),
    CONSTRAINT danger_crash_material_id_bounded CHECK (length(material_id) BETWEEN 3 AND 96),
    CONSTRAINT danger_crash_material_shape CHECK (
        pre_quantity > 0 AND pre_material_version > 0
        AND post_material_version = pre_material_version + 1
    )
);

CREATE FUNCTION enforce_danger_crash_material_change_source_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM character_run_material_stacks AS material
        WHERE material.namespace_id = NEW.namespace_id
          AND material.account_id = NEW.account_id
          AND material.character_id = NEW.character_id
          AND material.material_id = NEW.material_id
          AND material.material_version = NEW.post_material_version
          AND material.security_state = 3
          AND material.quantity = 0
          AND material.terminal_reason = 'crash_revoked'
          AND material.terminal_restore_point_id = NEW.restore_point_id
    ) THEN
        RAISE EXCEPTION 'danger crash material change is not bound to revoked material state';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER danger_crash_material_change_source_exact
AFTER INSERT OR UPDATE ON danger_crash_restore_material_changes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_material_change_source_v3();

CREATE TABLE danger_crash_restore_bargain_changes (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    change_ordinal INTEGER NOT NULL,
    record_kind SMALLINT NOT NULL,
    record_id BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, account_id, mutation_id, change_ordinal),
    UNIQUE (namespace_id, account_id, mutation_id, record_kind, record_id),
    FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    ) REFERENCES danger_crash_restore_results(
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    )
        ON DELETE CASCADE,
    CONSTRAINT danger_crash_bargain_ordinal_bounded CHECK (change_ordinal BETWEEN 0 AND 4094),
    CONSTRAINT danger_crash_bargain_kind_known CHECK (record_kind BETWEEN 0 AND 2),
    CONSTRAINT danger_crash_bargain_record_id_exact CHECK (
        octet_length(record_id) = 16
        AND record_id <> decode(repeat('00', 16), 'hex')
    )
);

CREATE FUNCTION enforce_danger_crash_bargain_change_source_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    source_exists BOOLEAN;
BEGIN
    IF NEW.record_kind = 0 THEN
        SELECT EXISTS (
            SELECT 1 FROM bargain_offers AS offer
            WHERE offer.namespace_id = NEW.namespace_id
              AND offer.account_id = NEW.account_id
              AND offer.character_id = NEW.character_id
              AND offer.offer_id = NEW.record_id
              AND offer.revoked_by_restore_point_id = NEW.restore_point_id
              AND offer.revoked_at IS NOT NULL
        ) INTO source_exists;
    ELSIF NEW.record_kind = 1 THEN
        SELECT EXISTS (
            SELECT 1 FROM bargain_milestone_results AS milestone
            WHERE milestone.namespace_id = NEW.namespace_id
              AND milestone.account_id = NEW.account_id
              AND milestone.character_id = NEW.character_id
              AND milestone.source_reward_event_id = NEW.record_id
              AND milestone.revoked_by_restore_point_id = NEW.restore_point_id
              AND milestone.revoked_at IS NOT NULL
        ) INTO source_exists;
    ELSE
        SELECT EXISTS (
            SELECT 1 FROM bargain_decision_results AS decision
            WHERE decision.namespace_id = NEW.namespace_id
              AND decision.account_id = NEW.account_id
              AND decision.character_id = NEW.character_id
              AND decision.mutation_id = NEW.record_id
              AND decision.revoked_by_restore_point_id = NEW.restore_point_id
              AND decision.revoked_at IS NOT NULL
        ) INTO source_exists;
    END IF;
    IF NOT source_exists THEN
        RAISE EXCEPTION 'danger crash Bargain change is not bound to a revoked source record';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER danger_crash_bargain_change_source_exact
AFTER INSERT OR UPDATE ON danger_crash_restore_bargain_changes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_bargain_change_source_v3();

CREATE TABLE danger_crash_restore_ash_changes (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    mutation_id BYTEA NOT NULL,
    change_ordinal INTEGER NOT NULL,
    original_ash_mutation_id BYTEA NOT NULL,
    compensation_ash_mutation_id BYTEA NOT NULL,
    amount INTEGER NOT NULL,
    pre_wallet_version BIGINT NOT NULL,
    post_wallet_version BIGINT NOT NULL,
    PRIMARY KEY (namespace_id, account_id, mutation_id, change_ordinal),
    UNIQUE (namespace_id, account_id, mutation_id, original_ash_mutation_id),
    UNIQUE (namespace_id, account_id, mutation_id, compensation_ash_mutation_id),
    FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    ) REFERENCES danger_crash_restore_results(
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    )
        ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, original_ash_mutation_id)
        REFERENCES ash_mutation_results(namespace_id, account_id, mutation_id),
    FOREIGN KEY (namespace_id, account_id, compensation_ash_mutation_id)
        REFERENCES ash_mutation_results(namespace_id, account_id, mutation_id),
    CONSTRAINT danger_crash_ash_ordinal_bounded CHECK (change_ordinal BETWEEN 0 AND 4094),
    CONSTRAINT danger_crash_ash_amount_bounded CHECK (amount BETWEEN 1 AND 99999),
    CONSTRAINT danger_crash_ash_versions_exact CHECK (
        pre_wallet_version > 0 AND post_wallet_version = pre_wallet_version + 1
    )
);

CREATE FUNCTION enforce_danger_crash_ash_change_source_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM ash_mutation_results AS original
        WHERE original.namespace_id = NEW.namespace_id
          AND original.account_id = NEW.account_id
          AND original.mutation_id = NEW.original_ash_mutation_id
          AND original.result_code = 0
          AND original.mutation_kind = 0
          AND original.requested_amount = NEW.amount
          AND original.entry_restore_point_id = NEW.restore_point_id
          AND original.reversed_by_restore_point_id = NEW.restore_point_id
          AND original.reversed_by_mutation_id = NEW.compensation_ash_mutation_id
          AND original.reversed_at IS NOT NULL
    ) OR NOT EXISTS (
        SELECT 1 FROM ash_mutation_results AS compensation
        WHERE compensation.namespace_id = NEW.namespace_id
          AND compensation.account_id = NEW.account_id
          AND compensation.mutation_id = NEW.compensation_ash_mutation_id
          AND compensation.result_code = 0
          AND compensation.mutation_kind = 1
          AND compensation.requested_amount = NEW.amount
          AND compensation.entry_restore_point_id = NEW.restore_point_id
          AND compensation.reversed_by_restore_point_id IS NULL
          AND compensation.reversed_by_mutation_id IS NULL
          AND compensation.reversed_at IS NULL
          AND compensation.pre_wallet_version = NEW.pre_wallet_version
          AND compensation.post_wallet_version = NEW.post_wallet_version
    ) THEN
        RAISE EXCEPTION 'danger crash Ash change is not bound to exact earn and compensation rows';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER danger_crash_ash_change_source_exact
AFTER INSERT OR UPDATE ON danger_crash_restore_ash_changes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_ash_change_source_v3();

CREATE FUNCTION enforce_danger_crash_result_counts_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT := COALESCE(NEW.namespace_id, OLD.namespace_id);
    target_account BYTEA := COALESCE(NEW.account_id, OLD.account_id);
    target_mutation BYTEA := COALESCE(NEW.mutation_id, OLD.mutation_id);
    expected danger_crash_restore_results%ROWTYPE;
    restored_items INTEGER;
    revoked_items INTEGER;
    materials INTEGER;
    bargains INTEGER;
    ash_changes INTEGER;
BEGIN
    SELECT * INTO expected FROM danger_crash_restore_results
    WHERE namespace_id = target_namespace AND account_id = target_account
      AND mutation_id = target_mutation;
    IF NOT FOUND THEN RETURN NULL; END IF;
    SELECT count(*) FILTER (WHERE change_kind = 0), count(*) FILTER (WHERE change_kind = 1)
      INTO restored_items, revoked_items FROM danger_crash_restore_item_changes
      WHERE namespace_id = target_namespace AND account_id = target_account
        AND mutation_id = target_mutation;
    SELECT count(*) INTO materials FROM danger_crash_restore_material_changes
      WHERE namespace_id = target_namespace AND account_id = target_account
        AND mutation_id = target_mutation;
    SELECT count(*) INTO bargains FROM danger_crash_restore_bargain_changes
      WHERE namespace_id = target_namespace AND account_id = target_account
        AND mutation_id = target_mutation;
    SELECT count(*) INTO ash_changes FROM danger_crash_restore_ash_changes
      WHERE namespace_id = target_namespace AND account_id = target_account
        AND mutation_id = target_mutation;
    IF restored_items <> expected.restored_item_count
        OR revoked_items <> expected.revoked_item_count
        OR materials <> expected.revoked_material_count
        OR bargains <> expected.revoked_bargain_record_count
        OR ash_changes <> expected.compensated_ash_count THEN
        RAISE EXCEPTION 'danger crash result child count mismatch';
    END IF;
    IF EXISTS (
        SELECT 1 FROM (
            SELECT change_ordinal,
                row_number() OVER (
                    ORDER BY change_kind, pre_location_kind, item_uid
                ) - 1 AS canonical_ordinal
            FROM danger_crash_restore_item_changes
            WHERE namespace_id = target_namespace AND account_id = target_account
              AND mutation_id = target_mutation
        ) AS ordered WHERE change_ordinal <> canonical_ordinal
    ) OR EXISTS (
        SELECT 1 FROM (
            SELECT change_ordinal,
                row_number() OVER (ORDER BY material_id COLLATE "C") - 1 AS canonical_ordinal
            FROM danger_crash_restore_material_changes
            WHERE namespace_id = target_namespace AND account_id = target_account
              AND mutation_id = target_mutation
        ) AS ordered WHERE change_ordinal <> canonical_ordinal
    ) OR EXISTS (
        SELECT 1 FROM (
            SELECT change_ordinal,
                row_number() OVER (ORDER BY record_kind, record_id) - 1 AS canonical_ordinal
            FROM danger_crash_restore_bargain_changes
            WHERE namespace_id = target_namespace AND account_id = target_account
              AND mutation_id = target_mutation
        ) AS ordered WHERE change_ordinal <> canonical_ordinal
    ) OR EXISTS (
        SELECT 1 FROM (
            SELECT change_ordinal,
                row_number() OVER (
                    ORDER BY pre_wallet_version, original_ash_mutation_id
                ) - 1 AS canonical_ordinal
            FROM danger_crash_restore_ash_changes
            WHERE namespace_id = target_namespace AND account_id = target_account
              AND mutation_id = target_mutation
        ) AS ordered WHERE change_ordinal <> canonical_ordinal
    ) THEN
        RAISE EXCEPTION 'danger crash result child order is not canonical';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER danger_crash_result_counts_complete
AFTER INSERT OR UPDATE ON danger_crash_restore_results
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_result_counts_v3();

CREATE CONSTRAINT TRIGGER danger_crash_item_counts_complete
AFTER INSERT OR UPDATE OR DELETE ON danger_crash_restore_item_changes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_result_counts_v3();
CREATE CONSTRAINT TRIGGER danger_crash_material_counts_complete
AFTER INSERT OR UPDATE OR DELETE ON danger_crash_restore_material_changes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_result_counts_v3();
CREATE CONSTRAINT TRIGGER danger_crash_bargain_counts_complete
AFTER INSERT OR UPDATE OR DELETE ON danger_crash_restore_bargain_changes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_result_counts_v3();
CREATE CONSTRAINT TRIGGER danger_crash_ash_counts_complete
AFTER INSERT OR UPDATE OR DELETE ON danger_crash_restore_ash_changes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_result_counts_v3();

CREATE FUNCTION reject_danger_crash_result_mutation_v3()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'danger crash restore result history is immutable';
END
$$;

CREATE TRIGGER danger_crash_results_immutable
BEFORE UPDATE OR DELETE ON danger_crash_restore_results
FOR EACH ROW EXECUTE FUNCTION reject_danger_crash_result_mutation_v3();
CREATE TRIGGER danger_crash_item_changes_immutable
BEFORE UPDATE OR DELETE ON danger_crash_restore_item_changes
FOR EACH ROW EXECUTE FUNCTION reject_danger_crash_result_mutation_v3();
CREATE TRIGGER danger_crash_material_changes_immutable
BEFORE UPDATE OR DELETE ON danger_crash_restore_material_changes
FOR EACH ROW EXECUTE FUNCTION reject_danger_crash_result_mutation_v3();
CREATE TRIGGER danger_crash_bargain_changes_immutable
BEFORE UPDATE OR DELETE ON danger_crash_restore_bargain_changes
FOR EACH ROW EXECUTE FUNCTION reject_danger_crash_result_mutation_v3();
CREATE TRIGGER danger_crash_ash_changes_immutable
BEFORE UPDATE OR DELETE ON danger_crash_restore_ash_changes
FOR EACH ROW EXECUTE FUNCTION reject_danger_crash_result_mutation_v3();
