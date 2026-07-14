ALTER TABLE safe_inventory_mutations
    ADD COLUMN result_code SMALLINT NOT NULL DEFAULT 1,
    ADD CONSTRAINT safe_inventory_result_accepted CHECK (result_code = 1);

ALTER TABLE safe_inventory_mutations
    ALTER COLUMN result_code DROP DEFAULT;
