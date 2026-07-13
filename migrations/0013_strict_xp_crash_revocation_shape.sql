ALTER TABLE character_xp_award_results
    DROP CONSTRAINT xp_crash_revocation_shape,
    ADD CONSTRAINT xp_crash_revocation_shape CHECK (
        (revoked_by_restore_point_id IS NULL
            AND revoked_at IS NULL
            AND revocation_progression_version IS NULL)
        OR
        (entry_restore_point_id IS NOT NULL
            AND revoked_by_restore_point_id IS NOT NULL
            AND revoked_by_restore_point_id = entry_restore_point_id
            AND octet_length(revoked_by_restore_point_id) = 16
            AND revoked_at IS NOT NULL
            AND revocation_progression_version IS NOT NULL
            AND revocation_progression_version > post_progression_version)
    );
