-- GB-M03 forward-only extension: every accepted Core B3 progression receipt records an exact B4
-- disposition. Result code 3 is a durable NoOffer result for a below-level or already-consumed
-- temporary Core milestone; it changes no Bargain slot/version and grants no Ash.
--
-- Recovery/downgrade: the wipeable Core namespace may be restored from the pre-0064 backup. A
-- live downgrade is intentionally unsupported after code-3 rows exist because deleting immutable
-- receipts would violate replay authority. The application remains fail closed if this migration
-- is absent.

ALTER TABLE bargain_milestone_results
    DROP CONSTRAINT bargain_milestone_result_code_known,
    DROP CONSTRAINT bargain_milestone_version_shape,
    DROP CONSTRAINT bargain_milestone_slot_transition_exact,
    DROP CONSTRAINT bargain_milestone_once_per_life;

ALTER TABLE bargain_milestone_results
    ADD CONSTRAINT bargain_milestone_result_code_known CHECK (result_code BETWEEN 0 AND 3),
    ADD CONSTRAINT bargain_milestone_version_shape CHECK (
        pre_oath_bargain_version > 0
        AND (
            (result_code = 0 AND post_oath_bargain_version = pre_oath_bargain_version + 1
                AND offer_id IS NOT NULL AND ash_mutation_id IS NULL)
            OR (result_code = 1
                AND post_oath_bargain_version = pre_oath_bargain_version + 1
                AND offer_id IS NOT NULL AND ash_mutation_id IS NOT NULL
                AND octet_length(ash_mutation_id) = 16
                AND ash_mutation_id <> decode(repeat('00', 16), 'hex'))
            OR (result_code = 2
                AND post_oath_bargain_version = pre_oath_bargain_version
                AND offer_id IS NULL AND ash_mutation_id IS NOT NULL
                AND octet_length(ash_mutation_id) = 16
                AND ash_mutation_id <> decode(repeat('00', 16), 'hex'))
            OR (result_code = 3
                AND post_oath_bargain_version = pre_oath_bargain_version
                AND offer_id IS NULL AND ash_mutation_id IS NULL)
        )
    ),
    ADD CONSTRAINT bargain_milestone_slot_transition_exact CHECK (
        pre_earned_bargain_slots BETWEEN 0 AND 3
        AND post_earned_bargain_slots BETWEEN 0 AND 3
        AND (
            (result_code IN (0, 1)
                AND post_earned_bargain_slots = pre_earned_bargain_slots + 1)
            OR (result_code IN (2, 3)
                AND post_earned_bargain_slots = pre_earned_bargain_slots)
        )
    );

-- Only an actually earned temporary milestone is once-per-life. Disposition-only receipts remain
-- individually idempotent by the table primary key and do not consume a future level-5 trigger.
CREATE UNIQUE INDEX bargain_milestone_once_per_life
    ON bargain_milestone_results(namespace_id, account_id, character_id, milestone_id)
    WHERE result_code BETWEEN 0 AND 2;
