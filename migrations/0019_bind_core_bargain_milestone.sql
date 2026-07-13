DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM bargain_milestone_results) THEN
        RAISE EXCEPTION '0019 requires dormant pre-route Bargain milestone tables';
    END IF;
END $$;

ALTER TABLE bargain_milestone_results
    ADD COLUMN milestone_id TEXT NOT NULL,
    ADD COLUMN source_content_id TEXT NOT NULL,
    ADD COLUMN source_layout_id TEXT NOT NULL,
    ADD COLUMN instance_lineage_id BYTEA NOT NULL,
    ADD COLUMN entry_restore_point_id BYTEA NOT NULL,
    ADD COLUMN pre_earned_bargain_slots SMALLINT NOT NULL,
    ADD COLUMN post_earned_bargain_slots SMALLINT NOT NULL,
    ADD CONSTRAINT bargain_milestone_core_binding_exact CHECK (
        milestone_id = 'milestone.core.sepulcher_knight_first_clear'
        AND source_content_id = 'miniboss.sepulcher_knight'
        AND source_layout_id = 'layout.core_private_life_01'
    ),
    ADD CONSTRAINT bargain_milestone_lineage_id_exact CHECK (
        octet_length(instance_lineage_id) = 16
        AND instance_lineage_id <> decode(repeat('00', 16), 'hex')
    ),
    ADD CONSTRAINT bargain_milestone_restore_id_exact CHECK (
        octet_length(entry_restore_point_id) = 16
        AND entry_restore_point_id <> decode(repeat('00', 16), 'hex')
    ),
    ADD CONSTRAINT bargain_milestone_offer_is_source CHECK (
        offer_id IS NULL OR offer_id = source_reward_event_id
    ),
    ADD CONSTRAINT bargain_milestone_slot_transition_exact CHECK (
        pre_earned_bargain_slots BETWEEN 0 AND 3
        AND post_earned_bargain_slots BETWEEN 0 AND 3
        AND (
            (result_code IN (0, 1)
                AND post_earned_bargain_slots = pre_earned_bargain_slots + 1)
            OR (result_code = 2
                AND post_earned_bargain_slots = pre_earned_bargain_slots)
        )
    ),
    ADD CONSTRAINT bargain_milestone_once_per_life UNIQUE (
        namespace_id, account_id, character_id, milestone_id
    ),
    ADD CONSTRAINT bargain_milestone_lineage_owned FOREIGN KEY (
        namespace_id, account_id, character_id, instance_lineage_id
    ) REFERENCES character_instance_lineages(
        namespace_id, account_id, character_id, lineage_id
    ),
    ADD CONSTRAINT bargain_milestone_restore_owned FOREIGN KEY (
        namespace_id, account_id, character_id, entry_restore_point_id
    ) REFERENCES character_entry_restore_points(
        namespace_id, account_id, character_id, restore_point_id
    ),
    ADD CONSTRAINT bargain_milestone_ash_result_owned FOREIGN KEY (
        namespace_id, account_id, ash_mutation_id
    ) REFERENCES ash_mutation_results(namespace_id, account_id, mutation_id),
    ADD CONSTRAINT bargain_milestone_offer_owned FOREIGN KEY (
        namespace_id, account_id, character_id, offer_id
    ) REFERENCES bargain_offers(namespace_id, account_id, character_id, offer_id);
