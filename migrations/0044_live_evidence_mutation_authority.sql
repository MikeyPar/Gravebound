-- GB-M03-06B live-evidence mutation authority closure.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md TECH-021 requires mutation ID, authenticated
--   account/character binding, expected state version, command type, payload hash, and issue time;
-- - Gravebound_Content_Production_Spec_v1.md fixes reward-qualified Core deed sources;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06/13 requires exact replay and restart proof;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md decision 4.
--
-- Migration 0043 established the normalized tables before any production writer existed. This
-- forward-only closure stores the remaining TECH-021 authority explicitly rather than hiding it
-- only inside a request hash. Existing migration history is not rewritten.

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM character_life_clock_checkpoint_receipts_v1)
        OR EXISTS (SELECT 1 FROM character_life_deed_completion_receipts_v1)
    THEN
        RAISE EXCEPTION '0044 requires dormant live death-evidence receipt tables';
    END IF;
END
$$;

ALTER TABLE character_life_clock_checkpoint_receipts_v1
    ADD COLUMN issued_at TIMESTAMPTZ NOT NULL,
    ADD CONSTRAINT life_clock_checkpoint_issue_order CHECK (committed_at >= issued_at);

ALTER TABLE character_life_deed_completion_receipts_v1
    ADD COLUMN expected_character_version BIGINT NOT NULL,
    ADD COLUMN issued_at TIMESTAMPTZ NOT NULL,
    ADD CONSTRAINT life_deed_completion_version_positive CHECK (
        expected_character_version > 0
    ),
    ADD CONSTRAINT life_deed_completion_issue_order CHECK (committed_at >= issued_at);

COMMENT ON COLUMN character_life_clock_checkpoint_receipts_v1.issued_at IS
    'Server-authored command issue time retained explicitly for TECH-021 replay audit.';
COMMENT ON COLUMN character_life_deed_completion_receipts_v1.expected_character_version IS
    'Authenticated living-character version observed by the reward completion command.';
COMMENT ON COLUMN character_life_deed_completion_receipts_v1.issued_at IS
    'Server-authored command issue time retained explicitly for TECH-021 replay audit.';
