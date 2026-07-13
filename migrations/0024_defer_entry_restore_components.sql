DO $$
DECLARE
    existing_constraint name;
BEGIN
    SELECT constraint_name.conname
    INTO existing_constraint
    FROM pg_constraint AS constraint_name
    WHERE constraint_name.conrelid = 'entry_restore_progression_v1'::regclass
      AND constraint_name.confrelid = 'character_entry_restore_points'::regclass
      AND constraint_name.contype = 'f';

    IF existing_constraint IS NULL THEN
        RAISE EXCEPTION 'entry restore progression root constraint is missing';
    END IF;

    EXECUTE format(
        'ALTER TABLE entry_restore_progression_v1 DROP CONSTRAINT %I',
        existing_constraint
    );
END
$$;

ALTER TABLE entry_restore_progression_v1
    ADD CONSTRAINT entry_restore_progression_root_owned FOREIGN KEY (
        namespace_id,
        account_id,
        character_id,
        restore_point_id
    ) REFERENCES character_entry_restore_points (
        namespace_id,
        account_id,
        character_id,
        restore_point_id
    ) ON DELETE CASCADE
      DEFERRABLE INITIALLY DEFERRED;
