ALTER TABLE ash_mutation_results
    ADD COLUMN expected_wallet_version BIGINT;

UPDATE ash_mutation_results
SET expected_wallet_version = pre_wallet_version
WHERE expected_wallet_version IS NULL;

ALTER TABLE ash_mutation_results
    ALTER COLUMN expected_wallet_version SET NOT NULL,
    DROP CONSTRAINT ash_result_code_known,
    DROP CONSTRAINT ash_result_rejection_kind,
    ADD CONSTRAINT ash_result_expected_version_positive CHECK (expected_wallet_version > 0),
    ADD CONSTRAINT ash_result_expected_version_match CHECK (
        (result_code = 3 AND expected_wallet_version <> pre_wallet_version)
        OR (result_code <> 3 AND expected_wallet_version = pre_wallet_version)
    ),
    ADD CONSTRAINT ash_result_code_known CHECK (result_code BETWEEN 0 AND 3),
    ADD CONSTRAINT ash_result_rejection_kind CHECK (
        result_code IN (0, 3)
        OR (result_code = 1 AND mutation_kind = 1)
        OR (result_code = 2 AND mutation_kind = 0)
    );
