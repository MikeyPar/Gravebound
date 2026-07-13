ALTER TABLE item_instances
    ADD COLUMN provenance_kind SMALLINT NOT NULL DEFAULT 0,
    ADD COLUMN salvage_band SMALLINT NOT NULL DEFAULT 0,
    ADD COLUMN salvage_value INTEGER NOT NULL DEFAULT 0,
    ADD CONSTRAINT item_provenance_known CHECK (provenance_kind BETWEEN 0 AND 5),
    ADD CONSTRAINT item_salvage_band_known CHECK (salvage_band BETWEEN 0 AND 5),
    ADD CONSTRAINT item_salvage_value_nonnegative CHECK (salvage_value >= 0),
    ADD CONSTRAINT item_creation_provenance_shape CHECK (
        (creation_kind = 0 AND provenance_kind IN (0, 4))
        OR (creation_kind = 1 AND provenance_kind IN (1, 4))
    ),
    ADD CONSTRAINT item_zero_salvage_shape CHECK (
        (provenance_kind = 0 AND salvage_band = 0 AND salvage_value = 0)
        OR (item_kind = 1 AND salvage_band = 0 AND salvage_value = 0)
        OR (provenance_kind <> 0 AND item_kind = 0)
    );

ALTER TABLE item_instances
    ALTER COLUMN provenance_kind DROP DEFAULT,
    ALTER COLUMN salvage_band DROP DEFAULT,
    ALTER COLUMN salvage_value DROP DEFAULT;
