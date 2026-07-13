DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM bargain_offers) THEN
        RAISE EXCEPTION '0017 requires dormant pre-route Bargain offer tables';
    END IF;
END $$;

ALTER TABLE bargain_offers
    ADD COLUMN source_content_id TEXT NOT NULL,
    ADD COLUMN source_layout_id TEXT NOT NULL,
    ADD COLUMN instance_lineage_id BYTEA NOT NULL,
    ADD COLUMN entry_restore_point_id BYTEA NOT NULL,
    ADD CONSTRAINT bargain_offer_core_source_exact CHECK (
        source_content_id = 'miniboss.sepulcher_knight'
        AND source_layout_id = 'layout.core_private_life_01'
    ),
    ADD CONSTRAINT bargain_offer_lineage_id_exact CHECK (
        octet_length(instance_lineage_id) = 16
        AND instance_lineage_id <> decode(repeat('00', 16), 'hex')
    ),
    ADD CONSTRAINT bargain_offer_restore_id_exact CHECK (
        octet_length(entry_restore_point_id) = 16
        AND entry_restore_point_id <> decode(repeat('00', 16), 'hex')
    ),
    ADD CONSTRAINT bargain_offer_lineage_owned FOREIGN KEY (
        namespace_id, account_id, character_id, instance_lineage_id
    ) REFERENCES character_instance_lineages(
        namespace_id, account_id, character_id, lineage_id
    ),
    ADD CONSTRAINT bargain_offer_restore_owned FOREIGN KEY (
        namespace_id, account_id, character_id, entry_restore_point_id
    ) REFERENCES character_entry_restore_points(
        namespace_id, account_id, character_id, restore_point_id
    );
