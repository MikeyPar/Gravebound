-- GB-M03-06B authoritative clock correction.
--
-- Gravebound_Production_GDD_v1_Canonical.md counts lifetime only while a living character is
-- controllable, while the permadeath-combat clock also includes committed danger staging and the
-- vulnerable LinkLost window. Gravebound_Content_Production_Spec_v1.md preserves that committed
-- danger context for Echo eligibility. Gravebound_Development_Roadmap_v1.md requires the resulting
-- death/Echo state to survive restart exactly. The clocks are therefore independently monotonic;
-- neither is a mathematical upper bound for the other.
--
-- This forward-only change relaxes one invalid cross-clock predicate. It rewrites no row and is
-- safe for existing wipeable Core data. Schema 39 can be restored only after proving no accepted
-- restore snapshot depends on the independent-clock shape.

ALTER TABLE entry_restore_life_metrics_v3
    DROP CONSTRAINT entry_restore_life_v3_ticks,
    ADD CONSTRAINT entry_restore_life_v3_ticks CHECK (
        captured_lifetime_ticks >= 0
        AND rollback_permadeath_combat_ticks >= 0
    );
