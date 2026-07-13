ALTER TABLE character_xp_award_results
    ADD COLUMN entry_restore_point_id BYTEA,
    ADD COLUMN revoked_by_restore_point_id BYTEA,
    ADD COLUMN revoked_at TIMESTAMPTZ,
    ADD COLUMN revocation_progression_version BIGINT,
    ADD CONSTRAINT xp_entry_restore_owned
        FOREIGN KEY (namespace_id, account_id, character_id, entry_restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        )
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT xp_revocation_restore_owned
        FOREIGN KEY (namespace_id, account_id, character_id, revoked_by_restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        )
        DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT xp_restore_binding_shape CHECK (
        entry_restore_point_id IS NULL OR (
            octet_length(entry_restore_point_id) = 16
            AND entry_restore_point_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT xp_crash_revocation_shape CHECK (
        (revoked_by_restore_point_id IS NULL
            AND revoked_at IS NULL
            AND revocation_progression_version IS NULL)
        OR
        (revoked_by_restore_point_id = entry_restore_point_id
            AND octet_length(revoked_by_restore_point_id) = 16
            AND revoked_at IS NOT NULL
            AND revocation_progression_version > post_progression_version)
    );

CREATE INDEX xp_awards_by_entry_restore
    ON character_xp_award_results (
        namespace_id, account_id, character_id, entry_restore_point_id, reward_event_id
    )
    WHERE entry_restore_point_id IS NOT NULL;

ALTER TABLE entry_restore_progression_v1
    ADD COLUMN restored_progression_version BIGINT,
    ADD COLUMN restored_at TIMESTAMPTZ,
    ADD CONSTRAINT restore_progression_completion_shape CHECK (
        (restored_progression_version IS NULL AND restored_at IS NULL)
        OR
        (restored_progression_version > progression_version AND restored_at IS NOT NULL)
    );
