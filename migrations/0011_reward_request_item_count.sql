ALTER TABLE reward_requests
    ADD COLUMN reward_item_count SMALLINT;

UPDATE reward_requests
SET reward_item_count = 0
WHERE request_state = 1;

ALTER TABLE reward_requests
    DROP CONSTRAINT reward_request_state_shape,
    ADD CONSTRAINT reward_request_item_count_bounded CHECK (
        reward_item_count IS NULL OR reward_item_count BETWEEN 0 AND 64
    ),
    ADD CONSTRAINT reward_request_state_shape CHECK (
        (request_state = 0
            AND plan_hash IS NULL
            AND result_hash IS NULL
            AND audit_digest IS NULL
            AND pre_inventory_version > 0
            AND post_inventory_version IS NULL
            AND reward_item_count IS NULL)
        OR
        (request_state = 1
            AND plan_hash IS NOT NULL
            AND result_hash IS NOT NULL
            AND audit_digest IS NOT NULL
            AND pre_inventory_version > 0
            AND reward_item_count IS NOT NULL
            AND post_inventory_version = pre_inventory_version
                + CASE WHEN reward_item_count = 0 THEN 0 ELSE 1 END)
    );
