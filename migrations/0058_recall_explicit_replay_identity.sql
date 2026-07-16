-- GB-M03-08 durable explicit-Recall replay identity correction.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-010 and TECH-015/021-023.
-- - Gravebound_Content_Production_Spec_v1.md Core dangerous-route Recall and
--   CONT-HUB-001/002.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-03/08 restart, idempotency,
--   and no-duplication gates.
-- - Accepted SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md.
--
-- Recovery/downgrade:
-- The pre-0058 implementation did not persist the explicit frame's client tick,
-- so no existing explicit terminal can be upgraded without inventing replay
-- material. Core remains wipeable and pre-gate; this migration therefore
-- requires the Recall terminal table to be empty. Before restoring a pre-0058
-- binary, prove the table remains empty. Published migration history must never
-- be rewritten or down-migrated in place.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM character_recall_terminal_results_v1
        LIMIT 1
    ) THEN
        RAISE EXCEPTION
            '0058 requires an empty pre-gate Recall terminal graph because explicit client ticks cannot be reconstructed';
    END IF;
END
$$;

ALTER TABLE character_recall_terminal_results_v1
    ADD COLUMN explicit_client_tick BIGINT,
    DROP CONSTRAINT recall_terminal_trigger_exact,
    ADD CONSTRAINT recall_terminal_trigger_exact CHECK (
        (
            terminal_kind = 3
            AND trigger_kind = 0
            AND explicit_request_sequence IS NOT NULL
            AND explicit_request_sequence BETWEEN 1 AND 4294967295
            AND explicit_client_tick IS NOT NULL
            AND explicit_client_tick BETWEEN 1 AND 9223372036854775807
            AND completion_tick = trigger_started_tick + 12
        )
        OR (
            terminal_kind = 4
            AND trigger_kind = 1
            AND explicit_request_sequence IS NULL
            AND explicit_client_tick IS NULL
            AND completion_tick = trigger_started_tick + 90
        )
    );

COMMENT ON COLUMN character_recall_terminal_results_v1.explicit_client_tick IS
    'Exact explicit Recall frame client tick; NULL only for LinkLost. It is diagnostic, not completion authority, but is mandatory altered-replay material.';
