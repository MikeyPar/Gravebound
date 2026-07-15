-- GB-M03-02D / GB-M03-06A forward correction for the v3 root terminal trigger.
--
-- Migration 0005 replaced character_entry_restore_points.content_revision with three exact
-- manifest hashes. Published migration 0034 accidentally referenced the removed column in its
-- UPDATE-only immutability function. Replace only that function; all schema-34/35 history remains
-- immutable.

CREATE OR REPLACE FUNCTION enforce_entry_restore_v3_root_immutability()
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
            NEW.records_blake3, NEW.assets_blake3, NEW.localization_blake3,
            NEW.snapshot_contract_version, NEW.account_version,
            NEW.character_version, NEW.progression_version, NEW.inventory_version,
            NEW.oath_bargain_version, NEW.life_metrics_version, NEW.ash_wallet_version,
            NEW.component_mask, NEW.composite_digest, NEW.created_at
        ) IS NOT DISTINCT FROM ROW(
            OLD.namespace_id, OLD.account_id, OLD.character_id, OLD.restore_point_id,
            OLD.lineage_id, OLD.source_location_id, OLD.restore_location_id,
            OLD.records_blake3, OLD.assets_blake3, OLD.localization_blake3,
            OLD.snapshot_contract_version, OLD.account_version,
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
