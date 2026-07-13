ALTER TABLE character_life_outbox
    DROP CONSTRAINT life_outbox_event_type_known,
    ADD CONSTRAINT life_outbox_event_type_known CHECK (
        event_type IN (
            'oath_selected',
            'bargain_offered',
            'bargain_selected',
            'bargain_declined',
            'bargains_cleared_death',
            'bargains_cleared_retirement'
        )
    );
