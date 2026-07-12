DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM character_instance_lineages
        UNION ALL SELECT 1 FROM character_entry_restore_points
        UNION ALL SELECT 1 FROM character_world_transfer_results
        UNION ALL SELECT 1 FROM character_danger_checkpoints
    ) THEN
        RAISE EXCEPTION 'world-flow revision migration requires dormant world-flow tables';
    END IF;
END
$$;

ALTER TABLE character_world_locations
    DROP CONSTRAINT world_location_shape;

UPDATE character_world_locations
SET safe_arrival_kind = 0
WHERE location_kind = 0;

ALTER TABLE character_world_locations
    ADD CONSTRAINT world_location_shape CHECK (
        (location_kind = 0
            AND location_content_id IS NULL
            AND safe_arrival_kind IS NOT NULL
            AND ((safe_arrival_kind = 0 AND safe_spawn_id IS NULL)
                OR (safe_arrival_kind = 1 AND safe_spawn_id IS NOT NULL))
            AND instance_lineage_id IS NULL
            AND entry_restore_point_id IS NULL)
        OR
        (location_kind = 1
            AND location_content_id IS NOT NULL
            AND safe_arrival_kind IS NOT NULL
            AND ((safe_arrival_kind = 0 AND safe_spawn_id IS NULL)
                OR (safe_arrival_kind = 1 AND safe_spawn_id IS NOT NULL))
            AND instance_lineage_id IS NULL
            AND entry_restore_point_id IS NULL)
        OR
        (location_kind = 2
            AND location_content_id IS NOT NULL
            AND safe_arrival_kind IS NULL
            AND safe_spawn_id IS NULL
            AND instance_lineage_id IS NOT NULL
            AND entry_restore_point_id IS NOT NULL)
    );

ALTER TABLE character_instance_lineages
    DROP CONSTRAINT lineage_content_revision_bounded,
    DROP COLUMN content_revision,
    ADD COLUMN records_blake3 TEXT NOT NULL,
    ADD COLUMN assets_blake3 TEXT NOT NULL,
    ADD COLUMN localization_blake3 TEXT NOT NULL;

ALTER TABLE character_entry_restore_points
    DROP CONSTRAINT restore_content_revision_bounded,
    DROP COLUMN content_revision,
    ADD COLUMN records_blake3 TEXT NOT NULL,
    ADD COLUMN assets_blake3 TEXT NOT NULL,
    ADD COLUMN localization_blake3 TEXT NOT NULL;

ALTER TABLE character_danger_checkpoints
    DROP CONSTRAINT checkpoint_content_revision_bounded,
    DROP COLUMN content_revision,
    ADD COLUMN records_blake3 TEXT NOT NULL,
    ADD COLUMN assets_blake3 TEXT NOT NULL,
    ADD COLUMN localization_blake3 TEXT NOT NULL;

ALTER TABLE character_world_transfer_results
    ADD COLUMN records_blake3 TEXT NOT NULL,
    ADD COLUMN assets_blake3 TEXT NOT NULL,
    ADD COLUMN localization_blake3 TEXT NOT NULL;

ALTER TABLE character_instance_lineages
    ADD CONSTRAINT lineage_world_flow_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    );

ALTER TABLE character_entry_restore_points
    ADD CONSTRAINT restore_world_flow_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    );

ALTER TABLE character_danger_checkpoints
    ADD CONSTRAINT checkpoint_world_flow_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    );

ALTER TABLE character_world_transfer_results
    ADD CONSTRAINT receipt_world_flow_revision_exact CHECK (
        records_blake3 ~ '^[0-9a-f]{64}$'
        AND assets_blake3 ~ '^[0-9a-f]{64}$'
        AND localization_blake3 ~ '^[0-9a-f]{64}$'
    );
