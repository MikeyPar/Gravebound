-- GB-M03-06 / GB-M03-13 item-shape authority repair.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md ECH-002 and TECH-022;
-- - Gravebound_Content_Production_Spec_v1.md CONT-ECHO-001;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06 and GB-M03-13.
--
-- Migration 0065 made BlackUnique rarity code 5 authoritative in the deferred
-- death/Echo graph, but the original Core item checks from migration 0007 still
-- admitted only item levels 1..10 and rarity codes 0..4. Replace only those two
-- checks with their canonical 1..20 / 0..5 supersets. Existing rows remain valid.

ALTER TABLE reward_result_entries
    DROP CONSTRAINT reward_entry_shape,
    ADD CONSTRAINT reward_entry_shape CHECK (
        (item_kind = 0 AND quantity = 1
            AND item_level IS NOT NULL AND item_level BETWEEN 1 AND 20
            AND rarity IS NOT NULL AND rarity BETWEEN 0 AND 5)
        OR (item_kind = 1 AND item_level IS NULL AND rarity IS NULL)
    );

ALTER TABLE item_instances
    DROP CONSTRAINT item_shape,
    ADD CONSTRAINT item_shape CHECK (
        (item_kind = 0
            AND item_level IS NOT NULL AND item_level BETWEEN 1 AND 20
            AND rarity IS NOT NULL AND rarity BETWEEN 0 AND 5)
        OR (item_kind = 1 AND item_level IS NULL AND rarity IS NULL)
    );

COMMENT ON CONSTRAINT reward_entry_shape ON reward_result_entries IS
    'Canonical equipment result shape: item levels 1..20 and rarity codes Worn..BlackUnique (0..5).';

COMMENT ON CONSTRAINT item_shape ON item_instances IS
    'Canonical durable equipment shape: item levels 1..20 and rarity codes Worn..BlackUnique (0..5).';

-- Recovery/downgrade:
-- - this is a constraint superset and performs no data rewrite;
-- - restoring the 1..10 / 0..4 checks requires first proving no level 11..20 or
--   rarity-5 rows exist, or resetting the wipeable Core namespace;
-- - published migration history must never be rewritten.
