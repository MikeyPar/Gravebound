-- GB-M03 bounded Vault/Overflow read authority and Overflow withdrawal.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md LOOT-002/050, WRLD-001, TECH-020/021.
-- - Gravebound_Content_Production_Spec_v1.md CONT-HUB-001/002.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-02/03/04 and the M03 restart gate.
--
-- Command kind 3 is OverflowToCharacterSafe. The client still supplies only the source slot and
-- exact aggregate versions; the server plans the lowest legal CharacterSafe destination.
-- This migration intentionally creates no expiry/salvage writer. M03 exposes the durable expiry
-- and salvage value read-only while automatic expiry salvage remains GB-M05-10 scope.
--
-- Recovery: keep 0068 applied. A schema-67 binary must not run against schema 68. Core remains
-- wipeable; rollback requires a pre-0068 restore or namespace wipe and intended migration set.
-- Never rewrite or down-migrate published migration history in place.

ALTER TABLE safe_inventory_mutations
    DROP CONSTRAINT safe_inventory_command_known,
    DROP CONSTRAINT safe_inventory_source_shape,
    DROP CONSTRAINT safe_inventory_versions_exact,
    ADD CONSTRAINT safe_inventory_command_known CHECK (command_kind BETWEEN 0 AND 3),
    ADD CONSTRAINT safe_inventory_source_shape CHECK (
        (command_kind IN (0, 2) AND source_slot_index BETWEEN 0 AND 7)
        OR (command_kind = 1 AND source_slot_index BETWEEN 0 AND 159)
        OR (command_kind = 3 AND source_slot_index BETWEEN 0 AND 19)
    ),
    ADD CONSTRAINT safe_inventory_versions_exact CHECK (
        pre_account_version > 0
        AND pre_inventory_version > 0
        AND post_inventory_version = pre_inventory_version + 1
        AND (
            (command_kind IN (0, 1, 3) AND post_account_version = pre_account_version + 1)
            OR (command_kind = 2 AND post_account_version = pre_account_version)
        )
    );

-- Exact account/location/slot prefix keeps every page lookup bounded without introducing an
-- unrestricted catalog or account scan. Item UID is the canonical within-stack unit order.
CREATE INDEX safe_storage_page_v1
    ON item_instances (namespace_id, account_id, location_kind, slot_index, item_uid)
    WHERE location_kind IN (5, 6, 8);

COMMENT ON INDEX safe_storage_page_v1 IS
    'Bounded CharacterSafe, Vault, and Overflow page projection for protocol 1.22.';
