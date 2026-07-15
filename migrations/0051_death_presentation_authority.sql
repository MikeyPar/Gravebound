-- GB-M03-02D / GB-M03-06D durable death-presentation authority.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-020 and TECH-020..022;
-- - Gravebound_Content_Production_Spec_v1.md CONT-HUB-002 and CONT-LOC-001;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-02/06 and the durable Memorial gate;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md.
--
-- The world_* columns added by 0037 remain the exact dangerous-world revision used by entry,
-- lineage, restore, and retained-trace authority. A death's renderer/localization package is an
-- independent immutable authority and must never reuse or reinterpret those columns.
-- Normal player-visible death routes remain disabled and the Core namespace is explicitly
-- wipeable, so refuse to fabricate presentation authority for any earlier disposable death.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM death_events LIMIT 1) THEN
        RAISE EXCEPTION
            '0051 requires no existing death rows; clear the wipeable Core namespace instead of reinterpreting world authority';
    END IF;
END
$$;

ALTER TABLE death_events
    ADD COLUMN presentation_records_blake3 TEXT NOT NULL,
    ADD COLUMN presentation_assets_blake3 TEXT NOT NULL,
    ADD COLUMN presentation_localization_blake3 TEXT NOT NULL,
    ADD CONSTRAINT death_presentation_revision_exact CHECK (
        presentation_records_blake3 ~ '^[0-9a-f]{64}$'
        AND presentation_assets_blake3 ~ '^[0-9a-f]{64}$'
        AND presentation_localization_blake3 ~ '^[0-9a-f]{64}$'
    );

COMMENT ON COLUMN death_events.world_records_blake3 IS
    'Immutable dangerous-world record revision; never presentation/localization authority.';
COMMENT ON COLUMN death_events.world_assets_blake3 IS
    'Immutable dangerous-world asset revision; never presentation/localization authority.';
COMMENT ON COLUMN death_events.world_localization_blake3 IS
    'Immutable dangerous-world localization revision; never death-view presentation authority.';
COMMENT ON COLUMN death_events.presentation_records_blake3 IS
    'Immutable transitive record revision used to render this stored death and Memorial snapshot.';
COMMENT ON COLUMN death_events.presentation_assets_blake3 IS
    'Immutable transitive asset revision used to render this stored death and Memorial snapshot.';
COMMENT ON COLUMN death_events.presentation_localization_blake3 IS
    'Immutable transitive localization revision used to render this stored death and Memorial snapshot.';

-- Downgrade/recovery: Core remains wipeable. Restore a pre-0051 backup or clear the wipeable Core
-- namespace and reapply migrations. Never drop these columns in place, rewrite migration 0037, or
-- copy world_* values into presentation_*.
