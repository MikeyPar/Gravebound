-- GB-M03-09 / TECH-040: keep committed loot telemetry immutable during
-- ordinary operation while preserving the explicitly wipeable Core account
-- graph. Published migration 0071 rejected every DELETE, including deletes
-- initiated by the account-owned foreign-key cascade.
CREATE OR REPLACE FUNCTION enforce_item_ledger_telemetry_immutability_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    accepted_at TIMESTAMPTZ;
BEGIN
    IF TG_OP = 'DELETE' THEN
        -- The sidecar has no direct account foreign key, so its character,
        -- session, or ledger parent can cascade first. The absent owning
        -- account is the single narrow authority for Core/TestRegion cleanup.
        IF NOT EXISTS (
            SELECT 1
              FROM accounts
             WHERE namespace_id = OLD.namespace_id
               AND account_id = OLD.account_id
        ) THEN
            RETURN OLD;
        END IF;
        RAISE EXCEPTION 'item-ledger telemetry source history is immutable';
    END IF;
    IF OLD.published_at IS NOT NULL
       OR NEW.published_at IS NULL
       OR NEW.published_at < OLD.created_at THEN
        RAISE EXCEPTION 'item-ledger telemetry publication may advance exactly once';
    END IF;
    accepted_at := NEW.published_at;
    NEW.published_at := OLD.published_at;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'item-ledger telemetry source payload is immutable';
    END IF;
    NEW.published_at := accepted_at;
    RETURN NEW;
END
$$;

COMMENT ON FUNCTION enforce_item_ledger_telemetry_immutability_v1() IS
    'Rejects direct loot telemetry mutation; permits deletion only through an already-removed owning account cascade in the wipeable Core namespace.';

-- Recovery/downgrade: restore the schema-0071 function body only after
-- completing any planned Core/TestRegion wipe. Do not delete telemetry rows
-- directly and never apply this wipe contract to a durable Early Access
-- namespace.
