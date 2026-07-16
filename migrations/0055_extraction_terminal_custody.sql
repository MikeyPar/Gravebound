-- GB-M03-08 extraction terminal custody.
--
-- Authorities:
-- - Gravebound_Production_GDD_v1_Canonical.md DTH-011, LOOT-002, LOOT-050, LOOT-060.
-- - Gravebound_Content_Production_Spec_v1.md CONT-HUB-001/002 and CONT-VALID-001.
-- - Gravebound_Development_Roadmap_v1.md GB-M03-03 and GB-M03-08.
-- - Accepted SPEC-CONFLICT-029-m03-extraction-recall-terminal-authority.md.
--
-- Durable discriminants append after Consumed=7:
--   location 8 Overflow, location 9 ResolutionHold, ledger source 5 Extraction.
-- Existing values are never renumbered. Schema 55 adds custody shape only; schema 56 owns the
-- production extraction result, placement, receipt, outbox, audit, and foreign-key authority.
--
-- Recovery/downgrade:
-- A pre-0055 binary may be restored only after proving there are no rows in location 8 or 9,
-- no item/ledger extraction identities, and no non-null extraction timestamps/deadlines.
-- Published migration history must never be rewritten or down-migrated in place.

ALTER TABLE item_instances
    ADD COLUMN terminal_extraction_id BYTEA,
    ADD COLUMN extracted_at TIMESTAMPTZ,
    ADD COLUMN overflow_expires_at TIMESTAMPTZ,
    DROP CONSTRAINT item_location_known,
    DROP CONSTRAINT item_location_shape,
    ADD CONSTRAINT item_location_known CHECK (location_kind BETWEEN 0 AND 9),
    ADD CONSTRAINT item_extraction_identity_shape CHECK (
        (terminal_extraction_id IS NULL AND extracted_at IS NULL)
        OR (
            terminal_extraction_id IS NOT NULL
            AND octet_length(terminal_extraction_id) = 16
            AND terminal_extraction_id <> decode(repeat('00', 16), 'hex')
            AND extracted_at IS NOT NULL
        )
    ),
    ADD CONSTRAINT item_location_shape CHECK (
        (location_kind = 0 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 3 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state IN (0, 1) AND item_kind = 0)
        OR (location_kind = 1 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 1 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state IN (0, 1) AND item_kind = 1)
        OR (location_kind = 2 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND terminal_extraction_id IS NULL
            AND extracted_at IS NULL AND overflow_expires_at IS NULL
            AND security_state = 2)
        OR (location_kind = 3 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NOT NULL AND octet_length(instance_id) = 16
            AND instance_id <> decode(repeat('00', 16), 'hex')
            AND pickup_id IS NOT NULL AND octet_length(pickup_id) = 16
            AND pickup_id <> decode(repeat('00', 16), 'hex')
            AND expires_at_tick > 0 AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND terminal_extraction_id IS NULL
            AND extracted_at IS NULL AND overflow_expires_at IS NULL
            AND security_state = 2)
        OR (location_kind = 4 AND character_id IS NOT NULL AND slot_index IS NULL
            AND instance_id IS NULL AND pickup_id IS NULL AND expires_at_tick IS NULL
            AND destruction_reason IS NOT NULL AND security_state = 3
            AND terminal_extraction_id IS NULL AND extracted_at IS NULL
            AND overflow_expires_at IS NULL
            AND (
                (destruction_reason = 'permadeath'
                    AND terminal_death_id IS NOT NULL
                    AND octet_length(terminal_death_id) = 16
                    AND terminal_death_id <> decode(repeat('00', 16), 'hex'))
                OR (destruction_reason IN ('ground_expired', 'crash_revoked')
                    AND terminal_death_id IS NULL)
            ))
        OR (location_kind = 5 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state = 0)
        OR (location_kind = 6 AND character_id IS NULL
            AND slot_index BETWEEN 0 AND 159 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND overflow_expires_at IS NULL
            AND security_state = 0)
        OR (location_kind = 7 AND character_id IS NOT NULL AND item_kind = 1
            AND slot_index BETWEEN 0 AND 1 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason = 'consumed'
            AND terminal_death_id IS NULL AND terminal_extraction_id IS NULL
            AND extracted_at IS NULL AND overflow_expires_at IS NULL
            AND security_state = 4)
        OR (location_kind = 8 AND character_id IS NULL
            AND slot_index BETWEEN 0 AND 19 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND terminal_extraction_id IS NOT NULL
            AND extracted_at IS NOT NULL
            AND overflow_expires_at = extracted_at + INTERVAL '72 hours'
            AND security_state = 0)
        OR (location_kind = 9 AND character_id IS NOT NULL
            AND slot_index BETWEEN 0 AND 7 AND instance_id IS NULL AND pickup_id IS NULL
            AND expires_at_tick IS NULL AND destruction_reason IS NULL
            AND terminal_death_id IS NULL AND terminal_extraction_id IS NOT NULL
            AND extracted_at IS NOT NULL AND overflow_expires_at IS NULL
            AND security_state = 0)
    );

ALTER TABLE item_ledger_events
    ADD COLUMN terminal_extraction_id BYTEA,
    DROP CONSTRAINT ledger_source_kind_known,
    DROP CONSTRAINT ledger_location_known,
    DROP CONSTRAINT ledger_creation_shape,
    ADD CONSTRAINT ledger_source_kind_known CHECK (source_kind BETWEEN 0 AND 5),
    ADD CONSTRAINT ledger_location_known CHECK (
        (pre_location_kind IS NULL OR pre_location_kind BETWEEN 0 AND 9)
        AND post_location_kind BETWEEN 0 AND 9
    ),
    ADD CONSTRAINT ledger_extraction_identity_shape CHECK (
        terminal_extraction_id IS NULL
        OR (
            octet_length(terminal_extraction_id) = 16
            AND terminal_extraction_id <> decode(repeat('00', 16), 'hex')
        )
    ),
    ADD CONSTRAINT ledger_extraction_source_shape CHECK (
        (source_kind = 5 AND event_kind = 1 AND terminal_extraction_id IS NOT NULL)
        OR (source_kind <> 5 AND terminal_extraction_id IS NULL)
    ),
    ADD CONSTRAINT ledger_creation_shape CHECK (
        (event_kind = 0 AND pre_item_version = 0
            AND pre_security_state IS NULL AND pre_location_kind IS NULL
            AND reason IS NULL AND terminal_death_id IS NULL)
        OR (event_kind = 1 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND reason IS NULL AND terminal_death_id IS NULL)
        OR (event_kind = 2 AND pre_item_version > 0
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND post_security_state = 3 AND post_location_kind = 4
            AND reason IS NOT NULL
            AND (
                (reason = 'permadeath'
                    AND terminal_death_id IS NOT NULL
                    AND octet_length(terminal_death_id) = 16
                    AND terminal_death_id <> decode(repeat('00', 16), 'hex'))
                OR (reason = 'ground_expired' AND terminal_death_id IS NULL)
            ))
        OR (event_kind = 3 AND pre_item_version > 0 AND source_kind <> 4
            AND pre_security_state = 1 AND pre_location_kind = 1
            AND post_security_state = 4 AND post_location_kind = 7
            AND reason = 'consumed' AND terminal_death_id IS NULL)
        OR (event_kind = 4 AND pre_item_version > 0 AND source_kind = 4
            AND pre_security_state IS NOT NULL AND pre_location_kind IS NOT NULL
            AND reason IS NOT NULL AND terminal_death_id IS NULL
            AND (
                (reason = 'crash_restored'
                    AND ((post_security_state = 0 AND post_location_kind IN (0, 1))
                        OR (post_security_state = 2 AND post_location_kind = 2)))
                OR (reason = 'crash_revoked'
                    AND post_security_state = 3 AND post_location_kind = 4)
            ))
    );

CREATE UNIQUE INDEX one_overflow_equipment_per_slot
    ON item_instances (namespace_id, account_id, slot_index)
    WHERE location_kind = 8 AND item_kind = 0;

CREATE UNIQUE INDEX one_resolution_hold_equipment_per_stack
    ON item_instances (
        namespace_id, account_id, character_id, terminal_extraction_id, slot_index
    )
    WHERE location_kind = 9 AND item_kind = 0;

CREATE INDEX overflow_units_by_projected_stack
    ON item_instances (
        namespace_id, account_id, slot_index, template_id, item_uid
    )
    WHERE location_kind = 8;

CREATE INDEX resolution_hold_units_by_projected_stack
    ON item_instances (
        namespace_id, account_id, character_id, terminal_extraction_id,
        slot_index, template_id, item_uid
    )
    WHERE location_kind = 9;

CREATE INDEX overflow_by_expiry
    ON item_instances (
        namespace_id, account_id, overflow_expires_at, slot_index, item_uid
    )
    WHERE location_kind = 8;

COMMENT ON COLUMN item_instances.location_kind IS
    '0 Equipped, 1 Belt, 2 RunBackpack, 3 PersonalGround, 4 Destroyed, '
    '5 CharacterSafe, 6 Vault, 7 Consumed, 8 Overflow, 9 ResolutionHold. '
    'Schema-54 rollback requires zero rows in 8 or 9 and no extraction metadata.';

COMMENT ON COLUMN item_instances.overflow_expires_at IS
    'Authoritative 72-hour deadline recorded by extraction. M03 never auto-deletes or salvages.';

COMMENT ON COLUMN item_ledger_events.source_kind IS
    '0 Starter, 1 Reward, 2 Field, 3 Death, 4 CrashRestore, 5 Extraction.';
