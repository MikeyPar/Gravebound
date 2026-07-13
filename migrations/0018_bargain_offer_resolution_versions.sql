ALTER TABLE bargain_offers
    DROP CONSTRAINT bargain_offer_resolution_shape,
    ADD CONSTRAINT bargain_offer_resolution_shape CHECK (
        (offer_state = 0 AND selected_bargain_id IS NULL
            AND resolved_oath_bargain_version IS NULL AND resolved_at IS NULL)
        OR (offer_state = 1 AND selected_bargain_id IS NOT NULL
            AND selected_bargain_id IN (
                'bargain.bell_debt',
                'bargain.cinder_hunger',
                'bargain.lantern_ash'
            ) AND resolved_oath_bargain_version IS NOT NULL
            AND resolved_oath_bargain_version > created_oath_bargain_version
            AND resolved_at IS NOT NULL)
        OR (offer_state = 2 AND selected_bargain_id IS NULL
            AND resolved_oath_bargain_version IS NOT NULL
            AND resolved_oath_bargain_version >= created_oath_bargain_version
            AND resolved_at IS NOT NULL)
        OR (offer_state = 3 AND selected_bargain_id IS NULL
            AND resolved_oath_bargain_version = created_oath_bargain_version
            AND resolved_at IS NOT NULL)
    );
