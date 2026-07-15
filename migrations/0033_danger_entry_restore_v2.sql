-- GB-M03-02D / GB-M03-06A danger-entry restore contract v2.
--
-- Authority: TECH-023, SPEC-CONFLICT-009, and the GB-M03 roadmap require one atomic,
-- component-complete pre-danger snapshot. Normal player routes are still disabled, so this
-- forward-only migration deliberately fails if a v1 restore graph exists rather than silently
-- claiming that an incomplete v1 graph satisfies v2. Downgrade to schema 32 is safe only while
-- no v2 restore roots or components exist.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM character_entry_restore_points LIMIT 1) THEN
        RAISE EXCEPTION
            '0033 requires no existing danger-entry restore points; clear the wipeable Core namespace';
    END IF;
END
$$;

-- Life metrics are part of every character aggregate after schema 31. The trigger closes the
-- future-row gap left by the schema-31 backfill, while the application still performs an explicit
-- idempotent insert so initialization remains visible at the repository boundary.
CREATE FUNCTION initialize_character_life_metrics_v2()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO character_life_metrics (
        namespace_id,
        account_id,
        character_id,
        lifetime_ticks,
        permadeath_combat_ticks,
        life_metrics_version
    ) VALUES (NEW.namespace_id, NEW.account_id, NEW.character_id, 0, 0, 1)
    ON CONFLICT (namespace_id, account_id, character_id) DO NOTHING;
    RETURN NEW;
END
$$;

CREATE TRIGGER character_life_metrics_v2_initialize
AFTER INSERT ON characters
FOR EACH ROW EXECUTE FUNCTION initialize_character_life_metrics_v2();

ALTER TABLE character_entry_restore_points
    DROP CONSTRAINT restore_contract_v1,
    DROP CONSTRAINT restore_components_complete,
    ADD COLUMN life_metrics_version BIGINT NOT NULL,
    ADD CONSTRAINT restore_contract_v2 CHECK (snapshot_contract_version = 2),
    ADD CONSTRAINT restore_components_v2_complete CHECK (component_mask = 15),
    ADD CONSTRAINT restore_life_metrics_version_positive CHECK (life_metrics_version > 0);

ALTER TABLE entry_restore_progression_v1
    ADD CONSTRAINT entry_restore_progression_v2_identity UNIQUE (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        progression_version
    );

ALTER TABLE entry_restore_inventory_v1
    DROP CONSTRAINT restore_inventory_counts_bounded,
    ADD CONSTRAINT restore_inventory_v2_counts_bounded CHECK (
        risk_item_count BETWEEN 0 AND 16
        AND safe_placement_count BETWEEN 0 AND 48
    ),
    ADD CONSTRAINT entry_restore_inventory_v2_identity UNIQUE (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        post_inventory_version
    );

CREATE TABLE entry_restore_oath_bargain_v2 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    oath_id TEXT,
    earned_bargain_slots SMALLINT NOT NULL,
    active_bargain_count SMALLINT NOT NULL,
    oath_bargain_version BIGINT NOT NULL,
    component_digest BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, restore_point_id),
    UNIQUE (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        oath_bargain_version
    ),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_oath_id_bounded CHECK (
        oath_id IS NULL OR length(oath_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT entry_restore_oath_slots_bounded CHECK (earned_bargain_slots BETWEEN 0 AND 3),
    CONSTRAINT entry_restore_active_count_bounded CHECK (
        active_bargain_count BETWEEN 0 AND earned_bargain_slots
    ),
    CONSTRAINT entry_restore_oath_version_positive CHECK (oath_bargain_version > 0),
    CONSTRAINT entry_restore_oath_digest_exact CHECK (
        octet_length(component_digest) = 32
        AND component_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE entry_restore_active_bargains_v2 (
    namespace_id TEXT NOT NULL,
    restore_point_id BYTEA NOT NULL,
    acquisition_ordinal SMALLINT NOT NULL,
    bargain_id TEXT NOT NULL,
    PRIMARY KEY (namespace_id, restore_point_id, acquisition_ordinal),
    UNIQUE (namespace_id, restore_point_id, bargain_id),
    FOREIGN KEY (namespace_id, restore_point_id)
        REFERENCES entry_restore_oath_bargain_v2(namespace_id, restore_point_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_bargain_ordinal_bounded CHECK (acquisition_ordinal BETWEEN 1 AND 3),
    CONSTRAINT entry_restore_bargain_id_bounded CHECK (length(bargain_id) BETWEEN 3 AND 96)
);

CREATE TABLE entry_restore_life_metrics_v2 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    captured_lifetime_ticks BIGINT NOT NULL,
    rollback_permadeath_combat_ticks BIGINT NOT NULL,
    life_metrics_version BIGINT NOT NULL,
    component_digest BYTEA NOT NULL,
    PRIMARY KEY (namespace_id, restore_point_id),
    UNIQUE (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        life_metrics_version
    ),
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT entry_restore_life_ticks_nonnegative CHECK (
        captured_lifetime_ticks >= 0 AND rollback_permadeath_combat_ticks >= 0
    ),
    CONSTRAINT entry_restore_life_version_positive CHECK (life_metrics_version > 0),
    CONSTRAINT entry_restore_life_digest_exact CHECK (
        octet_length(component_digest) = 32
        AND component_digest <> decode(repeat('00', 32), 'hex')
    )
);

-- These deferred reverse references make all four v2 components mandatory at commit while
-- retaining the caller-owned transaction's component-before-root insertion order.
ALTER TABLE character_entry_restore_points
    ADD CONSTRAINT restore_v2_progression_component_required FOREIGN KEY (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        progression_version
    ) REFERENCES entry_restore_progression_v1 (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        progression_version
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT restore_v2_inventory_component_required FOREIGN KEY (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        inventory_version
    ) REFERENCES entry_restore_inventory_v1 (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        post_inventory_version
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT restore_v2_oath_component_required FOREIGN KEY (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        oath_bargain_version
    ) REFERENCES entry_restore_oath_bargain_v2 (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        oath_bargain_version
    ) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT restore_v2_life_component_required FOREIGN KEY (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        life_metrics_version
    ) REFERENCES entry_restore_life_metrics_v2 (
        namespace_id,
        account_id,
        character_id,
        restore_point_id,
        life_metrics_version
    ) DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION enforce_entry_restore_inventory_v2_count()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    target_namespace TEXT := COALESCE(NEW.namespace_id, OLD.namespace_id);
    target_restore_point BYTEA := COALESCE(NEW.restore_point_id, OLD.restore_point_id);
    expected_count INTEGER;
    actual_count INTEGER;
BEGIN
    SELECT risk_item_count INTO expected_count
    FROM entry_restore_inventory_v1
    WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point;

    IF expected_count IS NULL THEN
        RETURN NULL;
    END IF;

    SELECT count(*) INTO actual_count
    FROM entry_restore_inventory_items_v1
    WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point;

    IF actual_count <> expected_count THEN
        RAISE EXCEPTION 'danger-entry inventory component count mismatch: expected %, found %',
            expected_count, actual_count;
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER entry_restore_inventory_v2_count_complete
AFTER INSERT OR UPDATE ON entry_restore_inventory_v1
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_inventory_v2_count();

CREATE CONSTRAINT TRIGGER entry_restore_inventory_v2_child_count_complete
AFTER INSERT OR UPDATE OR DELETE ON entry_restore_inventory_items_v1
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_inventory_v2_count();

CREATE FUNCTION enforce_entry_restore_oath_v2_count()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    target_namespace TEXT := COALESCE(NEW.namespace_id, OLD.namespace_id);
    target_restore_point BYTEA := COALESCE(NEW.restore_point_id, OLD.restore_point_id);
    expected_count INTEGER;
    actual_count INTEGER;
BEGIN
    SELECT active_bargain_count INTO expected_count
    FROM entry_restore_oath_bargain_v2
    WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point;

    IF expected_count IS NULL THEN
        RETURN NULL;
    END IF;

    SELECT count(*) INTO actual_count
    FROM entry_restore_active_bargains_v2
    WHERE namespace_id = target_namespace AND restore_point_id = target_restore_point;

    IF actual_count <> expected_count THEN
        RAISE EXCEPTION 'danger-entry Oath/Bargain component count mismatch: expected %, found %',
            expected_count, actual_count;
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER entry_restore_oath_v2_count_complete
AFTER INSERT OR UPDATE ON entry_restore_oath_bargain_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_oath_v2_count();

CREATE CONSTRAINT TRIGGER entry_restore_oath_v2_child_count_complete
AFTER INSERT OR UPDATE OR DELETE ON entry_restore_active_bargains_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_entry_restore_oath_v2_count();
