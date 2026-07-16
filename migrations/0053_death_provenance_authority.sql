-- GB-M03-02D / GB-M03-06A / GB-M03-06E durable death-provenance authority.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md ECH-001, TECH-020, TECH-022, and TECH-023;
-- - Gravebound_Content_Production_Spec_v1.md CONT-ECHO-009 and CONT-HUB-002;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06, GB-M03-13, and the M03 exit gate;
-- - docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md.
--
-- Existing rows predate durable provenance and were accepted only through the ordinary-gameplay
-- writer, so the temporary default is an exact backfill rather than an inferred rewrite.
ALTER TABLE death_events
    ADD COLUMN death_provenance SMALLINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT death_provenance_known CHECK (death_provenance BETWEEN 0 AND 2),
    ADD CONSTRAINT death_echo_provenance_shape CHECK (
        death_provenance = 0 OR NOT echo_expected
    );

ALTER TABLE death_events
    ALTER COLUMN death_provenance DROP DEFAULT;

COMMENT ON COLUMN death_events.death_provenance IS
    'Server-authored terminal provenance: 0 ordinary gameplay, 1 verified server incident, 2 administrative action. ECH-001 forbids Echo creation for values 1 and 2.';

-- Recovery/downgrade:
-- - this Core namespace remains wipeable;
-- - before restoring a pre-0053 binary, prove every death_provenance value is 0;
-- - never erase incident/administrative provenance from retained evidence to force compatibility;
-- - published migration history must never be rewritten.
