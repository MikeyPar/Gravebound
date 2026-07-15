-- GB-M03-02D / GB-M03-06A wipeable crash-restoration Ash ownership correction.
--
-- Authorities: GDD TECH-020/021/023, Content CONT-HUB-002, Roadmap GB-M03-02/06,
-- and accepted SPEC-CONFLICT-027/028. Migration 0037 remains immutable.
--
-- The restoration result already owns each normalized Ash change with ON DELETE CASCADE. Its two
-- source-result references were immediate NO ACTION relations, so an authorized account/namespace
-- wipe could reach ash_mutation_results before the owned change row and fail on trigger ordering.
-- Direct gameplay history deletion remains protected by the existing immutability triggers; only
-- the established wipeable ownership cascade may remove these rows.

DO $$
DECLARE
    constraint_name name;
BEGIN
    FOR constraint_name IN
        SELECT relation_constraint.conname
        FROM pg_constraint AS relation_constraint
        WHERE relation_constraint.conrelid = 'danger_crash_restore_ash_changes'::regclass
          AND relation_constraint.confrelid = 'ash_mutation_results'::regclass
          AND relation_constraint.contype = 'f'
    LOOP
        EXECUTE format(
            'ALTER TABLE danger_crash_restore_ash_changes DROP CONSTRAINT %I',
            constraint_name
        );
    END LOOP;
END
$$;

ALTER TABLE danger_crash_restore_ash_changes
    ADD CONSTRAINT danger_crash_ash_original_owned FOREIGN KEY (
        namespace_id, account_id, original_ash_mutation_id
    ) REFERENCES ash_mutation_results (
        namespace_id, account_id, mutation_id
    ) ON DELETE CASCADE,
    ADD CONSTRAINT danger_crash_ash_compensation_owned FOREIGN KEY (
        namespace_id, account_id, compensation_ash_mutation_id
    ) REFERENCES ash_mutation_results (
        namespace_id, account_id, mutation_id
    ) ON DELETE CASCADE;
