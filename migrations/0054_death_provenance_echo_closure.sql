-- GB-M03-02D / GB-M03-06A / GB-M03-06E provenance-aware Echo closure.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md ECH-001 and TECH-022;
-- - Gravebound_Content_Production_Spec_v1.md CONT-ECHO-009;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06, GB-M03-13, and the M03 exit gate;
-- - docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md.
--
-- Migration 0039 owns the complete deferred death-graph validator. Repeating that long function
-- here would create a second review surface that could drift from custody, trace, summary,
-- Memorial, Echo-history, and outbox closure. Instead, this migration requires the exact audited
-- legacy eligibility predicate to occur once and changes only that predicate. Any unexpected
-- function revision aborts migration rather than silently weakening the terminal graph.
DO $migration$
DECLARE
    definition TEXT;
    legacy_fragment CONSTANT TEXT := 'SELECT summary.level = 10';
    replacement_fragment CONSTANT TEXT :=
        E'SELECT death.death_provenance = 0\n        AND summary.level = 10';
    occurrence_count INTEGER;
BEGIN
    SELECT pg_get_functiondef(
        'public.enforce_complete_death_graph_v1()'::regprocedure
    ) INTO STRICT definition;

    occurrence_count := (
        length(definition) - length(replace(definition, legacy_fragment, ''))
    ) / length(legacy_fragment);
    IF occurrence_count <> 1 THEN
        RAISE EXCEPTION
            '0054 expected exactly one legacy death Echo eligibility predicate, found %',
            occurrence_count;
    END IF;

    definition := replace(definition, legacy_fragment, replacement_fragment);
    EXECUTE definition;

    SELECT pg_get_functiondef(
        'public.enforce_complete_death_graph_v1()'::regprocedure
    ) INTO STRICT definition;
    IF position(replacement_fragment IN definition) = 0
        OR position(legacy_fragment IN replace(
            definition,
            replacement_fragment,
            ''
        )) <> 0
    THEN
        RAISE EXCEPTION '0054 failed to install provenance-aware death Echo eligibility';
    END IF;
END
$migration$;

COMMENT ON FUNCTION enforce_complete_death_graph_v1() IS
    'Deferred complete death graph closure. Since migration 0054, ECH-001 eligibility also requires ordinary-gameplay death provenance.';

-- Recovery/downgrade:
-- - this Core namespace remains wipeable;
-- - do not restore the pre-0054 function while any death_provenance value is nonzero;
-- - a forward repair must retain the provenance predicate and every 0039 graph invariant;
-- - published migration history must never be rewritten.
