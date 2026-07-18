-- GB-M03-08 durable production extraction-intent acceptance and conflict audit.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-011, LOOT-002, LOOT-060,
--   and TECH-015/021-023 require authoritative extraction, exact replay, audited changed
--   mutation material, immediate persistence, and committed-result precedence.
-- - Gravebound_Content_Production_Spec_v1.md CONT-HUB-001/002, the Core Bell
--   Sepulcher/Sir Caldus route, and CONT-VALID-001 require a committed placement result before
--   Hall arrival and deterministic retry-safe Core authority.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-03/08 and the M03 restart/nonduplication gates.
-- - Accepted SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md requires exact retry
--   before current-state validation and durable idempotency-conflict evidence.
--
-- The primary key is exactly (namespace_id, extraction_request_id). The broad route revision
-- and narrower world-flow revision remain independent typed identities. Reliable transport
-- sequence is intentionally absent because an exact retry may use a later delivery sequence.
--
-- Recovery/downgrade:
-- Keep 0063 applied when rolling application binaries forward or back within schema 63. A
-- schema-62 binary does not understand the durable first-acceptance boundary and must not serve
-- normal extraction intent against schema 63. Core remains wipeable; rollback requires either a
-- pre-0063 database restore or a wipe followed by the intended migration set. Never rewrite
-- published migrations 0001 through 0062. Before any restore, retain these rows as incident
-- evidence because conflict payloads prove altered replay material across process restart.

CREATE TABLE production_extraction_intent_acceptances_v1 (
    namespace_id TEXT NOT NULL,
    extraction_request_id BYTEA NOT NULL,
    authenticated_account_id BYTEA NOT NULL,
    attempted_character_id BYTEA NOT NULL,
    attempted_mutation_id BYTEA NOT NULL,
    contract_version SMALLINT NOT NULL,
    frame_schema_version SMALLINT NOT NULL,
    frame_payload_hash BYTEA NOT NULL,
    extraction_receipt_id BYTEA NOT NULL,
    terminal_id BYTEA NOT NULL,
    actor_generation BIGINT NOT NULL,
    accepted_pre_route_state_version BIGINT NOT NULL,
    accepted_post_route_state_version BIGINT NOT NULL,
    route_records_blake3 TEXT NOT NULL,
    route_assets_blake3 TEXT NOT NULL,
    route_localization_blake3 TEXT NOT NULL,
    world_records_blake3 TEXT NOT NULL,
    world_assets_blake3 TEXT NOT NULL,
    world_localization_blake3 TEXT NOT NULL,
    canonical_attempt_hash BYTEA NOT NULL,
    commit_request_hash BYTEA NOT NULL,
    attempt_payload BYTEA NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    observed_tick BIGINT NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, extraction_request_id),
    FOREIGN KEY (namespace_id, authenticated_account_id, attempted_character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (
        namespace_id, authenticated_account_id, attempted_character_id, actor_generation
    ) REFERENCES private_route_generation_allocations_v1(
        namespace_id, account_id, character_id, actor_generation
    ) ON DELETE CASCADE,
    CONSTRAINT production_extraction_intent_ids_exact CHECK (
        octet_length(extraction_request_id) = 16
        AND octet_length(authenticated_account_id) = 16
        AND octet_length(attempted_character_id) = 16
        AND octet_length(attempted_mutation_id) = 16
        AND octet_length(extraction_receipt_id) = 16
        AND octet_length(terminal_id) = 16
        AND extraction_request_id <> decode(repeat('00', 16), 'hex')
        AND authenticated_account_id <> decode(repeat('00', 16), 'hex')
        AND attempted_character_id <> decode(repeat('00', 16), 'hex')
        AND attempted_mutation_id <> decode(repeat('00', 16), 'hex')
        AND extraction_receipt_id <> decode(repeat('00', 16), 'hex')
        AND terminal_id <> decode(repeat('00', 16), 'hex')
        AND attempted_mutation_id <> extraction_request_id
        AND attempted_mutation_id <> extraction_receipt_id
        AND attempted_mutation_id <> terminal_id
        AND extraction_request_id <> extraction_receipt_id
        AND extraction_request_id <> terminal_id
        AND extraction_receipt_id <> terminal_id
    ),
    CONSTRAINT production_extraction_intent_contract_exact CHECK (
        contract_version = 1
        AND frame_schema_version = 1
        AND actor_generation > 0
        AND accepted_pre_route_state_version > 0
        AND accepted_post_route_state_version = accepted_pre_route_state_version + 1
        AND observed_tick > 0
    ),
    CONSTRAINT production_extraction_intent_hashes_exact CHECK (
        octet_length(frame_payload_hash) = 32
        AND frame_payload_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(canonical_attempt_hash) = 32
        AND canonical_attempt_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(commit_request_hash) = 32
        AND commit_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempt_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT production_extraction_intent_route_revision_exact CHECK (
        route_records_blake3 ~ '^[0-9a-f]{64}$'
        AND route_records_blake3 <> repeat('0', 64)
        AND route_assets_blake3 ~ '^[0-9a-f]{64}$'
        AND route_assets_blake3 <> repeat('0', 64)
        AND route_localization_blake3 ~ '^[0-9a-f]{64}$'
        AND route_localization_blake3 <> repeat('0', 64)
    ),
    CONSTRAINT production_extraction_intent_world_revision_exact CHECK (
        world_records_blake3 ~ '^[0-9a-f]{64}$'
        AND world_records_blake3 <> repeat('0', 64)
        AND world_assets_blake3 ~ '^[0-9a-f]{64}$'
        AND world_assets_blake3 <> repeat('0', 64)
        AND world_localization_blake3 ~ '^[0-9a-f]{64}$'
        AND world_localization_blake3 <> repeat('0', 64)
    ),
    CONSTRAINT production_extraction_intent_time_order CHECK (
        issued_at <= accepted_at
    )
);

CREATE TABLE production_extraction_intent_conflict_audits_v1 (
    namespace_id TEXT NOT NULL,
    extraction_request_id BYTEA NOT NULL,
    conflict_audit_id BYTEA NOT NULL,
    attempted_account_id BYTEA NOT NULL,
    attempted_character_id BYTEA NOT NULL,
    attempted_mutation_id BYTEA NOT NULL,
    attempted_actor_generation BIGINT NOT NULL,
    attempted_pre_route_state_version BIGINT NOT NULL,
    attempted_post_route_state_version BIGINT NOT NULL,
    attempted_commit_request_hash BYTEA NOT NULL,
    stored_attempt_hash BYTEA NOT NULL,
    attempted_attempt_hash BYTEA NOT NULL,
    attempted_payload BYTEA NOT NULL,
    attempted_issued_at TIMESTAMPTZ NOT NULL,
    attempted_observed_tick BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, conflict_audit_id),
    UNIQUE (namespace_id, extraction_request_id, attempted_attempt_hash),
    FOREIGN KEY (namespace_id, extraction_request_id)
        REFERENCES production_extraction_intent_acceptances_v1(
            namespace_id, extraction_request_id
        ) ON DELETE CASCADE,
    CONSTRAINT production_extraction_intent_conflict_ids_exact CHECK (
        octet_length(conflict_audit_id) = 16
        AND conflict_audit_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(attempted_account_id) = 16
        AND attempted_account_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(attempted_character_id) = 16
        AND attempted_character_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(attempted_mutation_id) = 16
        AND attempted_mutation_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT production_extraction_intent_conflict_versions_exact CHECK (
        attempted_actor_generation > 0
        AND attempted_pre_route_state_version > 0
        AND attempted_post_route_state_version = attempted_pre_route_state_version + 1
        AND attempted_observed_tick > 0
    ),
    CONSTRAINT production_extraction_intent_conflict_hashes_exact CHECK (
        octet_length(attempted_commit_request_hash) = 32
        AND attempted_commit_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(stored_attempt_hash) = 32
        AND stored_attempt_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_attempt_hash) = 32
        AND attempted_attempt_hash <> decode(repeat('00', 32), 'hex')
        AND stored_attempt_hash <> attempted_attempt_hash
        AND octet_length(attempted_payload) BETWEEN 1 AND 65536
    ),
    CONSTRAINT production_extraction_intent_conflict_time_order CHECK (
        attempted_issued_at <= created_at
    )
);

CREATE INDEX production_extraction_intent_conflicts_by_request_v1
    ON production_extraction_intent_conflict_audits_v1 (
        namespace_id, extraction_request_id, created_at, conflict_audit_id
    );

CREATE FUNCTION enforce_production_extraction_intent_insert_time_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME = 'production_extraction_intent_acceptances_v1' THEN
        IF NEW.accepted_at IS DISTINCT FROM transaction_timestamp() THEN
            RAISE EXCEPTION 'production extraction intent acceptance time is transaction authority';
        END IF;
    ELSIF NEW.created_at IS DISTINCT FROM transaction_timestamp() THEN
        RAISE EXCEPTION 'production extraction intent conflict time is transaction authority';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER production_extraction_intent_acceptance_insert_time_v1
BEFORE INSERT ON production_extraction_intent_acceptances_v1
FOR EACH ROW EXECUTE FUNCTION enforce_production_extraction_intent_insert_time_v1();

CREATE TRIGGER production_extraction_intent_conflict_insert_time_v1
BEFORE INSERT ON production_extraction_intent_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION enforce_production_extraction_intent_insert_time_v1();

CREATE FUNCTION prevent_production_extraction_intent_history_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND pg_trigger_depth() > 1 THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'production extraction intent history is immutable';
END
$$;

CREATE TRIGGER production_extraction_intent_acceptance_immutable_v1
BEFORE UPDATE OR DELETE ON production_extraction_intent_acceptances_v1
FOR EACH ROW EXECUTE FUNCTION prevent_production_extraction_intent_history_mutation_v1();

CREATE TRIGGER production_extraction_intent_conflict_immutable_v1
BEFORE UPDATE OR DELETE ON production_extraction_intent_conflict_audits_v1
FOR EACH ROW EXECUTE FUNCTION prevent_production_extraction_intent_history_mutation_v1();

COMMENT ON TABLE production_extraction_intent_acceptances_v1 IS
    'GB-M03-08 immutable first accepted live extraction frame and complete terminal request binding.';
COMMENT ON TABLE production_extraction_intent_conflict_audits_v1 IS
    'GB-M03-08 idempotent append-only evidence for altered extraction-intent replay material.';
COMMENT ON COLUMN production_extraction_intent_acceptances_v1.route_records_blake3 IS
    'Broad Core route records revision; intentionally distinct from the narrow world-flow revision.';
COMMENT ON COLUMN production_extraction_intent_acceptances_v1.world_records_blake3 IS
    'Narrow world-flow records revision bound to the production terminal commit request.';
COMMENT ON FUNCTION prevent_production_extraction_intent_history_mutation_v1() IS
    'Preserves acceptance/conflict history while permitting explicit wipeable parent cascades.';
