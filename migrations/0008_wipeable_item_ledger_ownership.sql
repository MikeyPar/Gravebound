ALTER TABLE item_ledger_events
    DROP CONSTRAINT item_ledger_events_namespace_id_item_uid_fkey,
    DROP CONSTRAINT item_ledger_events_namespace_id_account_id_character_id_fkey,
    ADD CONSTRAINT item_ledger_item_owned
        FOREIGN KEY (namespace_id, item_uid)
        REFERENCES item_instances(namespace_id, item_uid) ON DELETE CASCADE,
    ADD CONSTRAINT item_ledger_character_owned
        FOREIGN KEY (namespace_id, account_id, character_id)
        REFERENCES character_inventories(namespace_id, account_id, character_id) ON DELETE CASCADE;
