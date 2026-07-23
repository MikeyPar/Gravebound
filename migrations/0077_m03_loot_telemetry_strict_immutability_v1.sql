-- GB-M03-09 / TEL-003: restore strict committed-source deletion semantics.
-- Published schema 0076 narrowly allowed a child delete after the owning
-- account disappeared. Hosted proof established that account deletion is not
-- the Core wipe mechanism: other immutable telemetry roots intentionally
-- retain account references. Core/TestRegion wipe uses the guarded disposable
-- database reset instead.
CREATE OR REPLACE FUNCTION enforce_item_ledger_telemetry_immutability_v1()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    accepted_at TIMESTAMPTZ;
BEGIN
    IF TG_OP = 'DELETE' THEN
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
    'Rejects every direct or cascading loot telemetry delete and permits only one publication acknowledgement advance.';

-- Recovery/downgrade: schema 0077 is the safe production state. Do not restore
-- the schema-0076 account-absence exception. Wipe only through the explicitly
-- guarded disposable-database reset before the durable namespace cutover.
