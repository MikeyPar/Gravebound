-- GB-M03-03 persistent private-route actor generations.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md TECH-010 through TECH-023 require one
--   authoritative owner, reconnect-safe state, and fail-closed crash handling.
-- - Gravebound_Content_Production_Spec_v1.md CONT-WORLD-001, CONT-ROOM-007, and
--   CONT-HUB-001/002 define the capacity-one Core route whose actor generation is owned here.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-03 and the M03 exit gate require restart,
--   retry, and no-stale-control proof for the complete private character life.
-- - ADR-037 requires actor replacement to advance a generation before route-state publication.
--
-- This counter is intentionally independent of transport generation and route state version.
-- Allocations may contain gaps after an ambiguous connection failure; reuse is forbidden.
--
-- Recovery: keep 0062 applied. Schema-61 binaries must not run against schema 62. Core remains
-- wipeable; rollback requires a pre-0062 database restore or a wipe followed by the intended
-- migration set. Published migrations 0001 through 0061 are never rewritten.

CREATE TABLE character_private_route_generation_heads_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    last_generation BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    CONSTRAINT private_route_generation_account_id_exact_length
        CHECK (octet_length(account_id) = 16),
    CONSTRAINT private_route_generation_character_id_exact_length
        CHECK (octet_length(character_id) = 16),
    CONSTRAINT private_route_generation_positive CHECK (last_generation > 0)
);

CREATE TABLE private_route_generation_allocations_v1 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    actor_generation BIGINT NOT NULL,
    allocated_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, character_id, actor_generation),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_private_route_generation_heads_v1(
            namespace_id, account_id, character_id
        ) ON DELETE CASCADE,
    CONSTRAINT private_route_allocation_account_id_exact_length
        CHECK (octet_length(account_id) = 16),
    CONSTRAINT private_route_allocation_character_id_exact_length
        CHECK (octet_length(character_id) = 16),
    CONSTRAINT private_route_allocation_generation_positive CHECK (actor_generation > 0)
);

CREATE FUNCTION enforce_private_route_generation_allocation_insert_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    head_generation BIGINT;
    head_updated_at TIMESTAMPTZ;
BEGIN
    SELECT last_generation, updated_at INTO head_generation, head_updated_at
    FROM character_private_route_generation_heads_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND character_id = NEW.character_id;
    IF NOT FOUND
        OR head_generation <> NEW.actor_generation
        OR head_updated_at IS DISTINCT FROM transaction_timestamp()
        OR NEW.allocated_at IS DISTINCT FROM transaction_timestamp()
    THEN
        RAISE EXCEPTION 'private-route allocation requires the current transaction generation head';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER private_route_generation_allocation_insert_v1
BEFORE INSERT ON private_route_generation_allocations_v1
FOR EACH ROW EXECUTE FUNCTION enforce_private_route_generation_allocation_insert_v1();

CREATE FUNCTION prevent_private_route_generation_allocation_mutation_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM characters
        WHERE namespace_id = OLD.namespace_id
          AND account_id = OLD.account_id
          AND character_id = OLD.character_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'private-route generation allocations are immutable';
END
$$;

CREATE TRIGGER private_route_generation_allocation_immutable_v1
BEFORE UPDATE OR DELETE ON private_route_generation_allocations_v1
FOR EACH ROW EXECUTE FUNCTION prevent_private_route_generation_allocation_mutation_v1();

CREATE FUNCTION enforce_private_route_generation_head_closure_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    maximum_allocation BIGINT;
BEGIN
    SELECT MAX(actor_generation) INTO maximum_allocation
    FROM private_route_generation_allocations_v1
    WHERE namespace_id = NEW.namespace_id
      AND account_id = NEW.account_id
      AND character_id = NEW.character_id;
    IF maximum_allocation IS NULL OR maximum_allocation <> NEW.last_generation THEN
        RAISE EXCEPTION 'private-route generation head must close over its immutable allocation audit';
    END IF;
    RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER private_route_generation_head_closure_v1
AFTER INSERT OR UPDATE ON character_private_route_generation_heads_v1
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_private_route_generation_head_closure_v1();

COMMENT ON TABLE character_private_route_generation_heads_v1 IS
    'GB-M03-03 restart-stable last allocated actor generation; gaps are valid and reuse is forbidden.';
COMMENT ON TABLE private_route_generation_allocations_v1 IS
    'GB-M03-03 immutable actor-generation allocation audit.';
COMMENT ON FUNCTION enforce_private_route_generation_allocation_insert_v1() IS
    'Permits only the allocation matching the head advanced in the current transaction.';
COMMENT ON FUNCTION prevent_private_route_generation_allocation_mutation_v1() IS
    'Preserves append-only actor-generation allocation evidence while permitting parent aggregate wipe cascade.';
COMMENT ON FUNCTION enforce_private_route_generation_head_closure_v1() IS
    'Requires every generation-head advance to close over its allocation audit before commit.';
