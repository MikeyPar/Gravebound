-- GB-M03 production-route repair: allow authoritative live damage evidence before the first
-- periodic danger checkpoint.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md TECH-021..023 requires damage evidence throughout
--   danger while the first process-resume/debug checkpoint is written only after 30 seconds;
-- - Gravebound_Content_Production_Spec_v1.md owns the Core damage and encounter identities;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06 requires the ordered ten-second damage trace
--   and atomic death graph on the ordinary route;
-- - docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md keeps trace authority server-owned.
--
-- Published schema 0043 incorrectly made each normalized live trace tick reference the optional
-- character_danger_checkpoints row. The repository already binds every tick to the immutable
-- danger-entry restore point and validates the current world, lineage, content hashes, and exact
-- optional checkpoint tick under locks. Keeping the additional checkpoint FK rejects legitimate
-- damage during the first 30-second interval, before TECH-023 creates that row.
--
-- This forward-only repair drops only that redundant FK. It preserves every table and row, the
-- immutable entry-restore FK, retained ingest receipt ownership, canonical graph triggers, and all
-- terminal promotion constraints.

ALTER TABLE character_live_damage_trace_ticks_v1
    DROP CONSTRAINT IF EXISTS character_live_damage_trace_t_namespace_id_account_id_char_fkey;

COMMENT ON TABLE character_live_damage_trace_ticks_v1 IS
    'Prunable authoritative ten-second damage window; entry-restore owned and valid before the first optional 30-second danger checkpoint.';

-- Downgrade/recovery: Core remains wipeable. Restore a pre-0079 backup or wipe/reapply the Core
-- namespace. Do not re-add the checkpoint FK while damage can occur before the first periodic
-- checkpoint; doing so recreates the production-route rejection corrected by this migration.
