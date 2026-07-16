-- GB-M03-06E durable cleanup receipt hardening.
--
-- Authority:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-001 and TECH-020-023 require the committed
--   death graph and its acknowledgement evidence to remain exact across replay and restart.
-- - Gravebound_Content_Production_Spec_v1.md CONT-HUB-001/002 require stored terminal history,
--   rather than later reconstruction, to drive the Hall and Memorial presentation.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-02D/06/13 require atomic, nonduplicating,
--   restart-safe death, cleanup, Memorial, and Echo state.
-- - SPEC-CONFLICT-009-m03-death-memorial.md makes Bargain cleanup a participant in the one
--   qualifying-death transaction.
--
-- Published migrations 0001-0051 remain immutable. This additive correction makes every
-- character-life outbox payload append-only while preserving the publisher's one legal mutation:
-- setting published_at exactly once. Never rewrite or delete accepted event authority in place.

CREATE FUNCTION enforce_character_life_outbox_publish_only_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF pg_trigger_depth() > 1 THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'character life outbox history is immutable';
    END IF;
    IF OLD.published_at IS NOT NULL
       OR NEW.published_at IS NULL
       OR NEW.namespace_id IS DISTINCT FROM OLD.namespace_id
       OR NEW.account_id IS DISTINCT FROM OLD.account_id
       OR NEW.character_id IS DISTINCT FROM OLD.character_id
       OR NEW.event_id IS DISTINCT FROM OLD.event_id
       OR NEW.event_type IS DISTINCT FROM OLD.event_type
       OR NEW.aggregate_version IS DISTINCT FROM OLD.aggregate_version
       OR NEW.event_payload IS DISTINCT FROM OLD.event_payload
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR NEW.published_at < OLD.created_at THEN
        RAISE EXCEPTION 'character life outbox history is immutable';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER character_life_outbox_publish_only
BEFORE UPDATE OR DELETE ON character_life_outbox
FOR EACH ROW EXECUTE FUNCTION enforce_character_life_outbox_publish_only_v1();

COMMENT ON FUNCTION enforce_character_life_outbox_publish_only_v1() IS
    'GB-M03-06E: character-life payloads are append-only; published_at may advance once.';

-- Recovery/downgrade: this migration changes no row shape or stored payload. If deployment fails
-- after application, keep 0052 applied and roll the application binary forward or back; older
-- writers already use INSERT-only event authority and remain compatible. A deliberate wipeable
-- Core downgrade may drop this trigger and function only after the namespace is drained, but
-- published migration history must never be rewritten.
