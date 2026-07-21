-- GB-M03-10 read-only support lookup boundary.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md TECH-005, TECH-020, TECH-030,
--   TECH-050, TECH-120, TECH-122, TECH-124, and TECH-125;
-- - Gravebound_Content_Production_Spec_v1.md CONT-LOC-001;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-10 and GB-M04-10.
--
-- The views deliberately omit account credentials, platform identity, network addresses,
-- serialized result payloads, and localized presentation text. The support application receives
-- SELECT on these views and INSERT on the audit table only; gameplay tables remain outside its
-- database role. Exact-ID SECURITY DEFINER functions are the only lookup grants; the support role
-- receives no direct SELECT on either the views or gameplay tables, preventing ad-hoc scans.

CREATE TABLE support_lookup_audit_events_v1 (
    namespace_id TEXT NOT NULL REFERENCES gravebound_namespaces(namespace_id),
    audit_event_id BYTEA NOT NULL,
    request_id BYTEA NOT NULL,
    operator_id TEXT NOT NULL,
    target_kind SMALLINT NOT NULL,
    target_id BYTEA NOT NULL,
    reason_kind SMALLINT NOT NULL,
    case_reference TEXT NOT NULL,
    outcome_kind SMALLINT NOT NULL,
    result_count SMALLINT NOT NULL,
    queried_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, audit_event_id),
    UNIQUE (namespace_id, request_id),
    CONSTRAINT support_audit_event_id_exact CHECK (
        octet_length(audit_event_id) = 16
        AND audit_event_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT support_audit_request_id_exact CHECK (
        octet_length(request_id) = 16
        AND request_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT support_audit_operator_id_bounded CHECK (
        length(operator_id) BETWEEN 3 AND 64
        AND operator_id ~ '^[a-z0-9][a-z0-9._-]*$'
    ),
    CONSTRAINT support_audit_target_known CHECK (target_kind BETWEEN 0 AND 2),
    CONSTRAINT support_audit_target_id_exact CHECK (
        octet_length(target_id) = 16
        AND target_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT support_audit_reason_known CHECK (reason_kind BETWEEN 0 AND 2),
    CONSTRAINT support_audit_case_reference_bounded CHECK (
        length(case_reference) BETWEEN 3 AND 64
        AND case_reference ~ '^[A-Z0-9][A-Z0-9._-]*$'
    ),
    CONSTRAINT support_audit_outcome_known CHECK (outcome_kind BETWEEN 0 AND 3),
    CONSTRAINT support_audit_result_count_bounded CHECK (result_count BETWEEN 0 AND 65)
);

CREATE INDEX support_lookup_audit_by_operator_time_v1
    ON support_lookup_audit_events_v1
    (namespace_id, operator_id, queried_at DESC, audit_event_id);

CREATE FUNCTION reject_support_lookup_audit_mutation_v1()
RETURNS trigger
LANGUAGE plpgsql
AS $function$
BEGIN
    RAISE EXCEPTION 'support lookup audit records are append-only';
END
$function$;

CREATE TRIGGER support_lookup_audit_no_update_v1
    BEFORE UPDATE OR DELETE ON support_lookup_audit_events_v1
    FOR EACH ROW EXECUTE FUNCTION reject_support_lookup_audit_mutation_v1();

CREATE VIEW support_character_lookup_v1
WITH (security_barrier = true)
AS
SELECT
    c.namespace_id,
    c.account_id,
    c.character_id,
    c.roster_ordinal,
    c.class_id,
    c.level,
    c.oath_id,
    c.life_state,
    c.security_state,
    a.state_version AS account_version,
    c.created_at,
    c.updated_at
FROM characters c
JOIN accounts a
  ON a.namespace_id = c.namespace_id
 AND a.account_id = c.account_id;

CREATE VIEW support_character_transition_lookup_v1
WITH (security_barrier = true)
AS
SELECT
    namespace_id,
    character_id,
    mutation_id AS event_id,
    command_kind AS event_kind,
    pre_character_version AS pre_state_version,
    post_character_version AS post_state_version,
    result_code,
    transfer_id AS related_id,
    committed_at
FROM character_world_transfer_results;

CREATE VIEW support_item_lookup_v1
WITH (security_barrier = true)
AS
SELECT
    namespace_id,
    account_id,
    character_id,
    item_uid,
    template_id,
    content_revision,
    item_version,
    security_state,
    location_kind,
    slot_index,
    creation_request_id,
    created_at,
    updated_at
FROM item_instances;

CREATE VIEW support_item_transition_lookup_v1
WITH (security_barrier = true)
AS
SELECT
    namespace_id,
    item_uid,
    ledger_event_id AS event_id,
    mutation_id,
    event_kind,
    source_kind,
    pre_item_version AS pre_state_version,
    post_item_version AS post_state_version,
    pre_security_state,
    post_security_state,
    pre_location_kind,
    post_location_kind,
    reason,
    committed_at
FROM item_ledger_events;

CREATE VIEW support_death_lookup_v1
WITH (security_barrier = true)
AS
SELECT
    namespace_id,
    death_id,
    account_id,
    character_id,
    mutation_id,
    content_revision,
    instance_id,
    lineage_id,
    restore_point_id,
    region_id,
    room_id,
    death_tick,
    cause_kind,
    killer_content_id,
    killer_pattern_id,
    killer_attack_id,
    final_damage,
    damage_type,
    pre_hit_health,
    network_state,
    recall_state,
    pre_character_version,
    post_character_version,
    trace_digest,
    committed_at
FROM death_events;

CREATE VIEW support_death_transition_lookup_v1
WITH (security_barrier = true)
AS
SELECT
    namespace_id,
    death_id,
    audit_event_id AS event_id,
    mutation_id,
    event_kind,
    event_digest,
    created_at AS committed_at
FROM death_audit_events
WHERE death_id IS NOT NULL;

CREATE FUNCTION support_lookup_character_v1(requested_character_id BYTEA)
RETURNS SETOF support_character_lookup_v1
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
    SELECT *
    FROM public.support_character_lookup_v1
    WHERE namespace_id = 'test.core'
      AND character_id = requested_character_id
      AND octet_length(requested_character_id) = 16
      AND requested_character_id <> decode(repeat('00', 16), 'hex')
    LIMIT 1
$function$;

CREATE FUNCTION support_lookup_character_transitions_v1(requested_character_id BYTEA)
RETURNS SETOF support_character_transition_lookup_v1
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
    SELECT *
    FROM public.support_character_transition_lookup_v1
    WHERE namespace_id = 'test.core'
      AND character_id = requested_character_id
      AND octet_length(requested_character_id) = 16
      AND requested_character_id <> decode(repeat('00', 16), 'hex')
    ORDER BY committed_at DESC, event_id DESC
    LIMIT 65
$function$;

CREATE FUNCTION support_lookup_item_v1(requested_item_uid BYTEA)
RETURNS SETOF support_item_lookup_v1
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
    SELECT *
    FROM public.support_item_lookup_v1
    WHERE namespace_id = 'test.core'
      AND item_uid = requested_item_uid
      AND octet_length(requested_item_uid) = 16
      AND requested_item_uid <> decode(repeat('00', 16), 'hex')
    LIMIT 1
$function$;

CREATE FUNCTION support_lookup_item_transitions_v1(requested_item_uid BYTEA)
RETURNS SETOF support_item_transition_lookup_v1
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
    SELECT *
    FROM public.support_item_transition_lookup_v1
    WHERE namespace_id = 'test.core'
      AND item_uid = requested_item_uid
      AND octet_length(requested_item_uid) = 16
      AND requested_item_uid <> decode(repeat('00', 16), 'hex')
    ORDER BY committed_at DESC, event_id DESC
    LIMIT 65
$function$;

CREATE FUNCTION support_lookup_death_v1(requested_death_id BYTEA)
RETURNS SETOF support_death_lookup_v1
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
    SELECT *
    FROM public.support_death_lookup_v1
    WHERE namespace_id = 'test.core'
      AND death_id = requested_death_id
      AND octet_length(requested_death_id) = 16
      AND requested_death_id <> decode(repeat('00', 16), 'hex')
    LIMIT 1
$function$;

CREATE FUNCTION support_lookup_death_transitions_v1(requested_death_id BYTEA)
RETURNS SETOF support_death_transition_lookup_v1
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
    SELECT *
    FROM public.support_death_transition_lookup_v1
    WHERE namespace_id = 'test.core'
      AND death_id = requested_death_id
      AND octet_length(requested_death_id) = 16
      AND requested_death_id <> decode(repeat('00', 16), 'hex')
    ORDER BY committed_at DESC, event_id DESC
    LIMIT 65
$function$;

REVOKE ALL ON FUNCTION support_lookup_character_v1(BYTEA) FROM PUBLIC;
REVOKE ALL ON FUNCTION support_lookup_character_transitions_v1(BYTEA) FROM PUBLIC;
REVOKE ALL ON FUNCTION support_lookup_item_v1(BYTEA) FROM PUBLIC;
REVOKE ALL ON FUNCTION support_lookup_item_transitions_v1(BYTEA) FROM PUBLIC;
REVOKE ALL ON FUNCTION support_lookup_death_v1(BYTEA) FROM PUBLIC;
REVOKE ALL ON FUNCTION support_lookup_death_transitions_v1(BYTEA) FROM PUBLIC;

COMMENT ON TABLE support_lookup_audit_events_v1 IS
    'Append-only GB-M03-10 operator lookup audit. Contains no credential, platform identity, network address, free-form reason, or localized reconstruction.';

-- Recovery/downgrade:
-- - revoke support-role EXECUTE/INSERT grants before dropping these functions/views/table;
-- - restoring a pre-0066 backup requires the matching pre-0066 binary or a wipeable namespace reset;
-- - forward changes remain additive and published migration history is never rewritten.
