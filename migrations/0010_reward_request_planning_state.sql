ALTER TABLE reward_requests
    ADD COLUMN request_state SMALLINT NOT NULL DEFAULT 1,
    ALTER COLUMN plan_hash DROP NOT NULL,
    ALTER COLUMN result_hash DROP NOT NULL,
    ALTER COLUMN audit_digest DROP NOT NULL,
    ALTER COLUMN post_inventory_version DROP NOT NULL,
    DROP CONSTRAINT reward_versions_advance_once,
    ADD CONSTRAINT reward_request_state_known CHECK (request_state IN (0, 1)),
    ADD CONSTRAINT reward_request_state_shape CHECK (
        (request_state = 0
            AND plan_hash IS NULL
            AND result_hash IS NULL
            AND audit_digest IS NULL
            AND pre_inventory_version > 0
            AND post_inventory_version IS NULL)
        OR
        (request_state = 1
            AND plan_hash IS NOT NULL
            AND result_hash IS NOT NULL
            AND audit_digest IS NOT NULL
            AND pre_inventory_version > 0
            AND post_inventory_version = pre_inventory_version + 1)
    );

ALTER TABLE reward_requests
    ALTER COLUMN request_state DROP DEFAULT;
