-- GB-M03-06B live-deed authority closure.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md ECH-001 and TECH-021/023;
-- - Gravebound_Content_Production_Spec_v1.md Core miniboss/boss reward and XP bindings;
-- - Gravebound_Development_Roadmap_v1.md GB-M03-06/13 atomicity, replay, and restart gates;
-- - owner-approved docs/spec-conflicts/SPEC-CONFLICT-009-m03-death-memorial.md.
--
-- The unreleased v1 deed receipt shape from migration 0043 lacked durable reward, restore,
-- revocation, and aggregate-version authority. It remains append-only migration history but has no
-- writer. This additive v2 contract is the only supported live-deed write boundary.

ALTER TABLE character_life_deeds
    DROP CONSTRAINT life_deed_kind_known,
    ADD CONSTRAINT life_deed_kind_known CHECK (deed_kind IN (0, 1, 2));

ALTER TABLE character_life_deed_completion_receipts_v1
    DROP CONSTRAINT life_deed_completion_kind_known,
    ADD CONSTRAINT life_deed_completion_kind_known CHECK (deed_kind IN (0, 1, 2));

ALTER TABLE character_entry_restore_points
    ADD CONSTRAINT restore_point_live_deed_v2_unique UNIQUE (
        namespace_id, account_id, character_id, lineage_id, restore_point_id
    );

ALTER TABLE character_xp_award_results
    ADD CONSTRAINT xp_live_deed_v2_unique UNIQUE (
        namespace_id, account_id, character_id, reward_event_id
    );

CREATE TABLE character_life_deed_completion_receipts_v2 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    completion_id BYTEA NOT NULL,
    deed_id TEXT NOT NULL,
    source_content_id TEXT NOT NULL,
    deed_kind SMALLINT NOT NULL,
    achieved_tick BIGINT NOT NULL,
    content_revision TEXT NOT NULL,
    source_instance_id BYTEA NOT NULL,
    lineage_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    reward_table_id TEXT NOT NULL,
    xp_profile_id TEXT NOT NULL,
    base_xp INTEGER NOT NULL,
    progression_records_blake3 TEXT NOT NULL,
    world_records_blake3 TEXT NOT NULL,
    world_assets_blake3 TEXT NOT NULL,
    world_localization_blake3 TEXT NOT NULL,
    reward_result_hash BYTEA NOT NULL,
    progression_payload_hash BYTEA NOT NULL,
    expected_character_version BIGINT NOT NULL,
    expected_life_metrics_version BIGINT NOT NULL,
    pre_life_metrics_version BIGINT NOT NULL,
    post_life_metrics_version BIGINT NOT NULL,
    projection_outcome SMALLINT NOT NULL,
    request_hash BYTEA NOT NULL,
    result_digest BYTEA NOT NULL,
    issued_at TIMESTAMPTZ NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, completion_id),
    UNIQUE (namespace_id, account_id, character_id, completion_id),
    FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES characters(namespace_id, account_id, character_id) ON DELETE CASCADE,
    FOREIGN KEY (namespace_id, account_id, character_id, restore_point_id)
        REFERENCES character_entry_restore_points(
            namespace_id, account_id, character_id, restore_point_id
        ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id, restore_point_id
    ) REFERENCES character_entry_restore_points(
        namespace_id, account_id, character_id, lineage_id, restore_point_id
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        world_records_blake3, world_assets_blake3, world_localization_blake3
    ) REFERENCES character_entry_restore_points(
        namespace_id, account_id, character_id, lineage_id, restore_point_id,
        records_blake3, assets_blake3, localization_blake3
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id, completion_id)
        REFERENCES reward_requests(
            namespace_id, account_id, character_id, reward_request_id
        ) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (namespace_id, account_id, character_id, completion_id)
        REFERENCES character_xp_award_results(
            namespace_id, account_id, character_id, reward_event_id
        ) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT life_deed_v2_completion_id_exact CHECK (
        octet_length(completion_id) = 16
        AND completion_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT life_deed_v2_lineage_id_exact CHECK (
        octet_length(source_instance_id) = 16
        AND source_instance_id <> decode(repeat('00', 16), 'hex')
        AND
        octet_length(lineage_id) = 16
        AND lineage_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT life_deed_v2_ids_bounded CHECK (
        length(deed_id) BETWEEN 3 AND 96
        AND length(source_content_id) BETWEEN 3 AND 96
        AND length(reward_table_id) BETWEEN 3 AND 96
        AND length(xp_profile_id) BETWEEN 3 AND 96
    ),
    CONSTRAINT life_deed_v2_kind_known CHECK (deed_kind IN (0, 1, 2)),
    CONSTRAINT life_deed_v2_core_authority_exact CHECK (
        (
            deed_id = 'deed.core.sir_caldus_defeated'
            AND source_content_id = 'boss.sir_caldus'
            AND deed_kind = 0
            AND reward_table_id = 'reward.boss_caldus'
            AND xp_profile_id = 'xp.boss_caldus'
            AND base_xp = 450
        ) OR (
            deed_id = 'deed.core.sepulcher_knight_defeated'
            AND source_content_id = 'miniboss.sepulcher_knight'
            AND deed_kind = 2
            AND reward_table_id = 'reward.miniboss_t1'
            AND xp_profile_id = 'xp.miniboss_t1'
            AND base_xp = 120
        )
    ),
    CONSTRAINT life_deed_v2_tick_positive CHECK (achieved_tick > 0),
    CONSTRAINT life_deed_v2_revision_exact CHECK (
        content_revision =
            'core-dev.blake3.27818db710b7553520a162f6f8337dcd0419c459d20c6513a7e12c78fed24ebb'
        AND progression_records_blake3 =
            '051f86a69b9d2a9dd911f0d92bf53b40e460ef13c9058d6f0b1f32f11b226f95'
        AND world_records_blake3 =
            '97b7188e26329b9430b7289d1e17d347c9b9472863b7db6bd48501fd3b773158'
        AND world_assets_blake3 =
            '32ce9fce6f1d49d5cd6cb570fa0590a5ee5644388c2620b67846320d4b2a3759'
        AND world_localization_blake3 =
            '895c38724abfdef4909751743d91b5cff90d7f073c553bc044601abff4763a26'
    ),
    CONSTRAINT life_deed_v2_versions_exact CHECK (
        expected_character_version > 0
        AND expected_life_metrics_version > 0
        AND pre_life_metrics_version = expected_life_metrics_version
        AND post_life_metrics_version = pre_life_metrics_version + 1
    ),
    CONSTRAINT life_deed_v2_projection_known CHECK (projection_outcome BETWEEN 0 AND 2),
    CONSTRAINT life_deed_v2_hashes_exact CHECK (
        octet_length(reward_result_hash) = 32
        AND reward_result_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(progression_payload_hash) = 32
        AND progression_payload_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(request_hash) = 32
        AND request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(result_digest) = 32
        AND result_digest <> decode(repeat('00', 32), 'hex')
    ),
    CONSTRAINT life_deed_v2_issue_order CHECK (committed_at >= issued_at)
);

CREATE INDEX life_deed_completions_latest_v2
    ON character_life_deed_completion_receipts_v2 (
        namespace_id, account_id, character_id,
        deed_id COLLATE "C", achieved_tick DESC, completion_id DESC
    );

ALTER TABLE danger_crash_restore_results
    ADD COLUMN life_deed_contract_version SMALLINT NOT NULL DEFAULT 0,
    ADD COLUMN revoked_life_deed_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN life_deed_projection_digest BYTEA,
    ADD CONSTRAINT danger_crash_life_deed_v2_shape CHECK (
        (
            life_deed_contract_version = 0
            AND revoked_life_deed_count = 0
            AND life_deed_projection_digest IS NULL
        ) OR (
            life_deed_contract_version = 1
            AND revoked_life_deed_count BETWEEN 0 AND 4095
            AND octet_length(life_deed_projection_digest) = 32
            AND life_deed_projection_digest <> decode(repeat('00', 32), 'hex')
        )
    );

CREATE TABLE character_life_deed_revocations_v2 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    completion_id BYTEA NOT NULL,
    restore_point_id BYTEA NOT NULL,
    crash_mutation_id BYTEA NOT NULL,
    change_ordinal INTEGER NOT NULL,
    revocation_digest BYTEA NOT NULL,
    post_projection_digest BYTEA NOT NULL,
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, completion_id),
    UNIQUE (
        namespace_id, account_id, character_id, restore_point_id,
        crash_mutation_id, completion_id
    ),
    UNIQUE (namespace_id, account_id, crash_mutation_id, change_ordinal),
    FOREIGN KEY (namespace_id, account_id, character_id, completion_id)
        REFERENCES character_life_deed_completion_receipts_v2(
            namespace_id, account_id, character_id, completion_id
        ) ON DELETE CASCADE,
    FOREIGN KEY (
        namespace_id, account_id, character_id, restore_point_id, crash_mutation_id
    ) REFERENCES danger_crash_restore_results(
        namespace_id, account_id, character_id, restore_point_id, mutation_id
    ) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT life_deed_revocation_v2_hash_exact CHECK (
        change_ordinal BETWEEN 0 AND 4094
        AND
        octet_length(revocation_digest) = 32
        AND revocation_digest <> decode(repeat('00', 32), 'hex')
        AND octet_length(post_projection_digest) = 32
        AND post_projection_digest <> decode(repeat('00', 32), 'hex')
    )
);

CREATE TABLE character_life_deed_conflict_audits_v2 (
    namespace_id TEXT NOT NULL,
    account_id BYTEA NOT NULL,
    character_id BYTEA NOT NULL,
    completion_id BYTEA NOT NULL,
    attempted_character_id BYTEA NOT NULL,
    audit_id BYTEA NOT NULL,
    stored_request_hash BYTEA NOT NULL,
    attempted_request_hash BYTEA NOT NULL,
    attempted_issued_at TIMESTAMPTZ NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(),
    PRIMARY KEY (namespace_id, account_id, audit_id),
    UNIQUE (namespace_id, account_id, completion_id, attempted_request_hash),
    FOREIGN KEY (namespace_id, account_id, character_id, completion_id)
        REFERENCES character_life_deed_completion_receipts_v2(
            namespace_id, account_id, character_id, completion_id
        ) ON DELETE CASCADE,
    CONSTRAINT life_deed_conflict_v2_ids_exact CHECK (
        octet_length(attempted_character_id) = 16
        AND attempted_character_id <> decode(repeat('00', 16), 'hex')
        AND octet_length(audit_id) = 16
        AND audit_id <> decode(repeat('00', 16), 'hex')
    ),
    CONSTRAINT life_deed_conflict_v2_hashes_exact CHECK (
        octet_length(stored_request_hash) = 32
        AND stored_request_hash <> decode(repeat('00', 32), 'hex')
        AND octet_length(attempted_request_hash) = 32
        AND attempted_request_hash <> decode(repeat('00', 32), 'hex')
        AND attempted_request_hash <> stored_request_hash
    )
);

CREATE FUNCTION enforce_danger_crash_life_deed_graph_v1()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT;
    target_account BYTEA;
    target_mutation BYTEA;
    stored_contract SMALLINT;
    stored_count INTEGER;
    stored_projection_digest BYTEA;
    actual_count BIGINT;
BEGIN
    IF TG_TABLE_NAME = 'danger_crash_restore_results' THEN
        IF TG_OP = 'DELETE' THEN
            target_namespace := OLD.namespace_id;
            target_account := OLD.account_id;
            target_mutation := OLD.mutation_id;
        ELSE
            target_namespace := NEW.namespace_id;
            target_account := NEW.account_id;
            target_mutation := NEW.mutation_id;
        END IF;
    ELSIF TG_OP = 'DELETE' THEN
        target_namespace := OLD.namespace_id;
        target_account := OLD.account_id;
        target_mutation := OLD.crash_mutation_id;
    ELSE
        target_namespace := NEW.namespace_id;
        target_account := NEW.account_id;
        target_mutation := NEW.crash_mutation_id;
    END IF;

    SELECT life_deed_contract_version, revoked_life_deed_count,
           life_deed_projection_digest
    INTO stored_contract, stored_count, stored_projection_digest
    FROM danger_crash_restore_results
    WHERE namespace_id = target_namespace
      AND account_id = target_account
      AND mutation_id = target_mutation;
    IF NOT FOUND THEN RETURN NULL; END IF;

    SELECT count(*) INTO actual_count
    FROM character_life_deed_revocations_v2
    WHERE namespace_id = target_namespace
      AND account_id = target_account
      AND crash_mutation_id = target_mutation;

    IF stored_contract = 0 THEN
        IF actual_count <> 0 OR stored_count <> 0 OR stored_projection_digest IS NOT NULL THEN
            RAISE EXCEPTION 'legacy crash result cannot carry live deed revocations';
        END IF;
        RETURN NULL;
    END IF;

    IF stored_contract <> 1
        OR actual_count <> stored_count
        OR EXISTS (
            SELECT 1
            FROM (
                SELECT change_ordinal,
                    row_number() OVER (ORDER BY completion_id) - 1 AS expected_ordinal,
                    post_projection_digest
                FROM character_life_deed_revocations_v2
                WHERE namespace_id = target_namespace
                  AND account_id = target_account
                  AND crash_mutation_id = target_mutation
            ) AS ordered
            WHERE ordered.change_ordinal <> ordered.expected_ordinal
               OR ordered.post_projection_digest <> stored_projection_digest
        )
    THEN
        RAISE EXCEPTION 'crash result live deed revocation graph is incomplete or noncanonical';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER danger_crash_life_deed_result_exact_v1
AFTER INSERT OR UPDATE OR DELETE ON danger_crash_restore_results
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_life_deed_graph_v1();
CREATE CONSTRAINT TRIGGER danger_crash_life_deed_children_exact_v1
AFTER INSERT OR DELETE ON character_life_deed_revocations_v2
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_danger_crash_life_deed_graph_v1();

CREATE FUNCTION enforce_life_deed_reward_authority_v2()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    authority_is_exact BOOLEAN;
BEGIN
    SELECT TRUE
    INTO authority_is_exact
    FROM reward_requests AS reward
    JOIN character_xp_award_results AS xp
      ON xp.namespace_id = reward.namespace_id
     AND xp.account_id = reward.account_id
     AND xp.reward_event_id = reward.reward_request_id
    JOIN character_entry_restore_points AS root
      ON root.namespace_id = NEW.namespace_id
     AND root.account_id = NEW.account_id
     AND root.character_id = NEW.character_id
     AND root.restore_point_id = NEW.restore_point_id
    WHERE reward.namespace_id = NEW.namespace_id
      AND reward.account_id = NEW.account_id
      AND reward.character_id = NEW.character_id
      AND reward.reward_request_id = NEW.completion_id
      AND reward.request_state = 1
      AND reward.source_instance_id = NEW.source_instance_id
      AND reward.reward_table_id = NEW.reward_table_id
      AND reward.content_revision = NEW.content_revision
      AND reward.result_hash = NEW.reward_result_hash
      AND xp.character_id = NEW.character_id
      AND xp.source_content_id = NEW.source_content_id
      AND xp.xp_profile_id = NEW.xp_profile_id
      AND xp.progression_content_revision = NEW.progression_records_blake3
      AND xp.payload_hash = NEW.progression_payload_hash
      AND xp.eligible
      AND xp.eligibility_kind = 1
      AND xp.encounter_life_state = 0
      AND xp.encounter_recall_state = 0
      AND xp.encounter_trust_state = 0
      AND xp.result_code = 0
      AND xp.base_xp = NEW.base_xp
      AND xp.entry_restore_point_id = NEW.restore_point_id
      AND root.lineage_id = NEW.lineage_id
      AND root.records_blake3 = NEW.world_records_blake3
      AND root.assets_blake3 = NEW.world_assets_blake3
      AND root.localization_blake3 = NEW.world_localization_blake3
      AND root.restore_state = 0
      AND (
          (NEW.deed_kind = 0 AND xp.base_xp = 450)
          OR (NEW.deed_kind = 2 AND xp.base_xp = 120)
      )
      AND (
          NEW.deed_kind <> 0
          OR EXISTS (
              SELECT 1
              FROM caldus_victory_exit_owners AS owner
              JOIN caldus_victory_exits AS victory
                ON victory.namespace_id = owner.namespace_id
               AND victory.encounter_id = owner.encounter_id
              WHERE owner.namespace_id = NEW.namespace_id
                AND owner.encounter_id = NEW.source_instance_id
                AND owner.account_id = NEW.account_id
                AND owner.character_id = NEW.character_id
                AND owner.reward_request_id = NEW.completion_id
                AND owner.reward_result_hash = NEW.reward_result_hash
                AND owner.progression_payload_hash = NEW.progression_payload_hash
                AND victory.instance_lineage_id = NEW.lineage_id
          )
      )
      AND (
          (
              xp.revoked_by_restore_point_id IS NULL
              AND NOT EXISTS (
                  SELECT 1 FROM character_life_deed_revocations_v2 AS revocation
                  WHERE revocation.namespace_id = NEW.namespace_id
                    AND revocation.account_id = NEW.account_id
                    AND revocation.character_id = NEW.character_id
                    AND revocation.completion_id = NEW.completion_id
              )
          ) OR (
              xp.revoked_by_restore_point_id = NEW.restore_point_id
              AND EXISTS (
                  SELECT 1 FROM character_life_deed_revocations_v2 AS revocation
                  WHERE revocation.namespace_id = NEW.namespace_id
                    AND revocation.account_id = NEW.account_id
                    AND revocation.character_id = NEW.character_id
                    AND revocation.completion_id = NEW.completion_id
                    AND revocation.restore_point_id = NEW.restore_point_id
              )
          )
      )
    FOR SHARE OF reward, xp, root;

    IF authority_is_exact IS DISTINCT FROM TRUE THEN
        RAISE EXCEPTION 'life deed receipt lacks exact terminal reward authority';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER life_deed_reward_authority_exact_v2
AFTER INSERT ON character_life_deed_completion_receipts_v2
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_life_deed_reward_authority_v2();

CREATE FUNCTION enforce_life_deed_revocation_authority_v2()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM character_xp_award_results AS xp
        WHERE xp.namespace_id = NEW.namespace_id
          AND xp.account_id = NEW.account_id
          AND xp.character_id = NEW.character_id
          AND xp.reward_event_id = NEW.completion_id
          AND xp.entry_restore_point_id = NEW.restore_point_id
          AND xp.revoked_by_restore_point_id = NEW.restore_point_id
          AND xp.revoked_at IS NOT NULL
          AND xp.revocation_progression_version IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'life deed revocation lacks matching progression revocation';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER life_deed_revocation_authority_exact_v2
AFTER INSERT ON character_life_deed_revocations_v2
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_life_deed_revocation_authority_v2();

CREATE FUNCTION enforce_xp_deed_revocation_pair_v2()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM character_life_deed_completion_receipts_v2 AS receipt
        WHERE receipt.namespace_id = NEW.namespace_id
          AND receipt.account_id = NEW.account_id
          AND receipt.character_id = NEW.character_id
          AND receipt.completion_id = NEW.reward_event_id
    ) AND (
        (NEW.revoked_by_restore_point_id IS NULL AND EXISTS (
            SELECT 1 FROM character_life_deed_revocations_v2 AS revocation
            WHERE revocation.namespace_id = NEW.namespace_id
              AND revocation.account_id = NEW.account_id
              AND revocation.character_id = NEW.character_id
              AND revocation.completion_id = NEW.reward_event_id
        )) OR (NEW.revoked_by_restore_point_id IS NOT NULL AND NOT EXISTS (
            SELECT 1 FROM character_life_deed_revocations_v2 AS revocation
            WHERE revocation.namespace_id = NEW.namespace_id
              AND revocation.account_id = NEW.account_id
              AND revocation.character_id = NEW.character_id
              AND revocation.completion_id = NEW.reward_event_id
              AND revocation.restore_point_id = NEW.revoked_by_restore_point_id
        ))
    ) THEN
        RAISE EXCEPTION 'progression and live deed revocation evidence must commit together';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER xp_deed_revocation_pair_exact_v2
AFTER INSERT OR UPDATE ON character_xp_award_results
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_xp_deed_revocation_pair_v2();

CREATE FUNCTION enforce_life_deed_projection_v2()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    target_namespace TEXT;
    target_account BYTEA;
    target_character BYTEA;
BEGIN
    IF TG_OP = 'DELETE' THEN
        target_namespace := OLD.namespace_id;
        target_account := OLD.account_id;
        target_character := OLD.character_id;
    ELSE
        target_namespace := NEW.namespace_id;
        target_account := NEW.account_id;
        target_character := NEW.character_id;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM characters
        WHERE namespace_id = target_namespace
          AND account_id = target_account
          AND character_id = target_character
    ) THEN
        RETURN NULL;
    END IF;

    -- Migration 0031 exposed this projection for hosted death fixtures before any live-deed writer
    -- existed. Those legacy-only rows remain readable. The first v2 receipt opts the character into
    -- the strict symmetric graph and thereafter no extra, missing, or altered row can commit.
    IF NOT EXISTS (
        SELECT 1 FROM character_life_deed_completion_receipts_v2
        WHERE namespace_id = target_namespace
          AND account_id = target_account
          AND character_id = target_character
    ) THEN
        RETURN NULL;
    END IF;

    IF EXISTS (
        WITH ranked AS (
            SELECT receipt.*,
                row_number() OVER (
                    PARTITION BY receipt.deed_id
                    ORDER BY receipt.achieved_tick DESC, receipt.completion_id DESC
                ) AS deed_ordinal
            FROM character_life_deed_completion_receipts_v2 AS receipt
            LEFT JOIN character_life_deed_revocations_v2 AS revocation
              ON revocation.namespace_id = receipt.namespace_id
             AND revocation.account_id = receipt.account_id
             AND revocation.character_id = receipt.character_id
             AND revocation.completion_id = receipt.completion_id
            WHERE receipt.namespace_id = target_namespace
              AND receipt.account_id = target_account
              AND receipt.character_id = target_character
              AND revocation.completion_id IS NULL
        ), expected AS (
            SELECT * FROM ranked WHERE deed_ordinal = 1
        ), actual AS (
            SELECT * FROM character_life_deeds
            WHERE namespace_id = target_namespace
              AND account_id = target_account
              AND character_id = target_character
        )
        SELECT 1
        FROM expected
        FULL OUTER JOIN actual USING (deed_id)
        WHERE expected.deed_id IS NULL
           OR actual.deed_id IS NULL
           OR actual.reward_event_id IS DISTINCT FROM expected.completion_id
           OR actual.source_content_id IS DISTINCT FROM expected.source_content_id
           OR actual.deed_kind IS DISTINCT FROM expected.deed_kind
           OR actual.achieved_tick IS DISTINCT FROM expected.achieved_tick
           OR actual.content_revision IS DISTINCT FROM expected.content_revision
    ) THEN
        RAISE EXCEPTION 'character life deed projection diverges from immutable active receipts';
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER life_deed_receipt_projection_exact_v2
AFTER INSERT OR DELETE ON character_life_deed_completion_receipts_v2
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_life_deed_projection_v2();
CREATE CONSTRAINT TRIGGER life_deed_revocation_projection_exact_v2
AFTER INSERT OR DELETE ON character_life_deed_revocations_v2
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_life_deed_projection_v2();
CREATE CONSTRAINT TRIGGER life_deed_projection_self_exact_v2
AFTER INSERT OR UPDATE OR DELETE ON character_life_deeds
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION enforce_life_deed_projection_v2();

CREATE TRIGGER life_deed_completion_receipt_append_only_v2
BEFORE UPDATE OR DELETE ON character_life_deed_completion_receipts_v2
FOR EACH ROW EXECUTE FUNCTION reject_live_death_evidence_receipt_mutation_v1();
CREATE TRIGGER life_deed_revocation_append_only_v2
BEFORE UPDATE OR DELETE ON character_life_deed_revocations_v2
FOR EACH ROW EXECUTE FUNCTION reject_live_death_evidence_receipt_mutation_v1();
CREATE TRIGGER life_deed_conflict_audit_append_only_v2
BEFORE UPDATE OR DELETE ON character_life_deed_conflict_audits_v2
FOR EACH ROW EXECUTE FUNCTION reject_live_death_evidence_receipt_mutation_v1();

CREATE TRIGGER dead_life_deed_completion_insert_v2
BEFORE INSERT ON character_life_deed_completion_receipts_v2
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();
CREATE TRIGGER dead_life_deed_revocation_insert_v2
BEFORE INSERT ON character_life_deed_revocations_v2
FOR EACH ROW EXECUTE FUNCTION reject_dead_character_insert_v1();

COMMENT ON TABLE character_life_deed_completion_receipts_v1 IS
    'Unreleased GB-M03-06B v1 shape retained only as append-only migration history; no writer.';
COMMENT ON TABLE character_life_deed_completion_receipts_v2 IS
    'Exact reward-qualified Core deed receipts with restore, version, hash, replay, and crash authority.';
COMMENT ON TABLE character_life_deed_revocations_v2 IS
    'Append-only crash revocations paired with progression revocation and the terminal restore result.';
COMMENT ON TABLE character_life_deed_conflict_audits_v2 IS
    'Bounded TECH-021 changed-payload conflict evidence; no raw payload or network secret is stored.';
COMMENT ON FUNCTION enforce_life_deed_projection_v2() IS
    'Requires character_life_deeds to equal the deterministic latest nonrevoked v2 receipt per deed.';
