ALTER TABLE character_life_outbox
    DROP CONSTRAINT life_outbox_event_type_known,
    ADD CONSTRAINT life_outbox_event_type_known CHECK (
        event_type IN (
            'oath_selected',
            'bargain_selected',
            'bargains_cleared_death',
            'bargains_cleared_retirement'
        )
    );

CREATE UNIQUE INDEX one_bargain_cleanup_event_per_life_reason
    ON character_life_outbox (namespace_id, account_id, character_id, event_type)
    WHERE event_type IN ('bargains_cleared_death', 'bargains_cleared_retirement');
