-- GB-M03-09 additive repair for schema-0071 loot telemetry projection.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md LOOT-010, ECO-002,
--   TECH-123, and TEL-001..005;
-- - Gravebound_Content_Production_Spec_v1.md CONT-REWARD-001..004 and the
--   exact Core stable IDs;
-- - Gravebound_Development_Roadmap_v1.md ADR-005 and GB-M03-09.
--
-- Schema 0071's trigger calls the smallint event-ID function with PostgreSQL
-- integer literals. Function resolution is exact and therefore rejected those
-- calls before the trigger's fail-open exception handler returned the gameplay
-- row. This additive overload accepts only the trigger's integer call shape and
-- delegates to the original immutable smallint authority. It changes no
-- gameplay, ledger, session, payload, or publication state.

CREATE FUNCTION derive_m03_loot_telemetry_event_id_v1(
    loot_action INTEGER,
    ledger_identity BYTEA
)
RETURNS BYTEA
LANGUAGE SQL
IMMUTABLE
STRICT
AS $$
    SELECT derive_m03_loot_telemetry_event_id_v1(
        loot_action::SMALLINT,
        ledger_identity
    )
$$;

COMMENT ON FUNCTION derive_m03_loot_telemetry_event_id_v1(INTEGER, BYTEA) IS
    'Schema-0072 exact integer-literal compatibility overload for the immutable schema-0071 loot projector.';

-- Recovery/downgrade: stop telemetry polling before dropping only this exact
-- INTEGER overload. Do not remove or replace the schema-0071 SMALLINT function,
-- rewrite item-ledger history, or backfill telemetry origins.
