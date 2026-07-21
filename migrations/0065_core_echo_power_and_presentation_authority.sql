-- GB-M03-06 / GB-M03-13 Core Echo power and presentation authority repair.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md ECH-001/ECH-002 and TECH-021/TECH-022;
-- - Gravebound_Content_Production_Spec_v1.md CONT-ECHO-001;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06, GB-M03-13, and the M03 exit gate;
-- - approved docs/spec-conflicts/SPEC-CONFLICT-004-m03-core-identity.md.
--
-- Migration 0039 owns the complete deferred death-graph validator and migration 0054 already
-- demonstrated the guarded pg_get_functiondef repair pattern. Repeating that long function here
-- would create a second graph-closure implementation. This migration instead requires the exact
-- four-slot legacy rarity expression and two legacy appearance literals before changing only
-- those audited fragments. Any drift aborts migration without weakening atomic death closure.
DO $migration$
DECLARE
    definition TEXT;
    legacy_rarity CONSTANT TEXT := 'WHEN 3 THEN 20 WHEN 4 THEN 30 END';
    repaired_rarity CONSTANT TEXT :=
        'WHEN 3 THEN 20 WHEN 4 THEN 30 WHEN 5 THEN 30 END';
    legacy_appearance CONSTANT TEXT := 'appearance.default.grave_arbalist';
    legacy_theme CONSTANT TEXT := 'theme.echo.arbalist_ash';
    core_silhouette CONSTANT TEXT := 'sprite.class.grave_arbalist';
    occurrence_count INTEGER;
    legacy_echo_count BIGINT;
BEGIN
    SELECT count(*) INTO STRICT legacy_echo_count
    FROM echo_records
    WHERE namespace_id = 'test.core'
      AND (
          appearance_snapshot_id = legacy_appearance
          OR appearance_theme_id = legacy_theme
      );
    IF legacy_echo_count <> 0 THEN
        RAISE EXCEPTION
            '0065 cannot reinterpret % immutable legacy Core Echo records; reset the wipeable Core namespace',
            legacy_echo_count;
    END IF;

    SELECT pg_get_functiondef(
        'public.enforce_complete_death_graph_v1()'::regprocedure
    ) INTO STRICT definition;

    occurrence_count := (
        length(definition) - length(replace(definition, legacy_rarity, ''))
    ) / length(legacy_rarity);
    IF occurrence_count <> 4 THEN
        RAISE EXCEPTION
            '0065 expected four legacy equipment rarity expressions, found %',
            occurrence_count;
    END IF;
    occurrence_count := (
        length(definition) - length(replace(definition, legacy_appearance, ''))
    ) / length(legacy_appearance);
    IF occurrence_count <> 1 THEN
        RAISE EXCEPTION
            '0065 expected one legacy Echo appearance literal, found %',
            occurrence_count;
    END IF;
    occurrence_count := (
        length(definition) - length(replace(definition, legacy_theme, ''))
    ) / length(legacy_theme);
    IF occurrence_count <> 1 THEN
        RAISE EXCEPTION
            '0065 expected one legacy Echo theme literal, found %',
            occurrence_count;
    END IF;

    definition := replace(definition, legacy_rarity, repaired_rarity);
    definition := replace(definition, legacy_appearance, core_silhouette);
    definition := replace(definition, legacy_theme, core_silhouette);
    EXECUTE definition;

    SELECT pg_get_functiondef(
        'public.enforce_complete_death_graph_v1()'::regprocedure
    ) INTO STRICT definition;
    occurrence_count := (
        length(definition) - length(replace(definition, repaired_rarity, ''))
    ) / length(repaired_rarity);
    IF occurrence_count <> 4
        OR position(legacy_appearance IN definition) <> 0
        OR position(legacy_theme IN definition) <> 0
    THEN
        RAISE EXCEPTION '0065 failed to install exact Core Echo power/presentation authority';
    END IF;
END
$migration$;

COMMENT ON FUNCTION enforce_complete_death_graph_v1() IS
    'Deferred complete death graph closure. Since migration 0065, CONT-ECHO-001 treats BlackUnique rarity as +30 tenths and Core Echo compatibility fields snapshot only the approved non-entitlement base silhouette.';

-- Recovery/downgrade:
-- - this Core namespace remains wipeable and the presentation placeholder cannot migrate;
-- - restoring a pre-0065 backup requires the matching pre-0065 binary or a namespace reset;
-- - forward repair must retain rarity code 5 at +30 and every 0039/0054 graph invariant;
-- - published migration history must never be rewritten.
