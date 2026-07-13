ALTER TABLE character_instance_lineages
    ADD COLUMN layout_id TEXT,
    ADD CONSTRAINT lineage_layout_id_bounded CHECK (
        layout_id IS NULL OR length(layout_id) BETWEEN 3 AND 96
    ),
    ADD CONSTRAINT lineage_layout_identity UNIQUE (
        namespace_id, account_id, character_id, lineage_id, layout_id
    );

ALTER TABLE bargain_offers
    ADD CONSTRAINT bargain_offer_layout_lineage_owned FOREIGN KEY (
        namespace_id, account_id, character_id, instance_lineage_id, source_layout_id
    ) REFERENCES character_instance_lineages(
        namespace_id, account_id, character_id, lineage_id, layout_id
    );

ALTER TABLE bargain_milestone_results
    ADD CONSTRAINT bargain_milestone_layout_lineage_owned FOREIGN KEY (
        namespace_id, account_id, character_id, instance_lineage_id, source_layout_id
    ) REFERENCES character_instance_lineages(
        namespace_id, account_id, character_id, lineage_id, layout_id
    );
