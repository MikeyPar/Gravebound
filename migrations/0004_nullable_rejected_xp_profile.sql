DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM character_xp_award_results) THEN
        RAISE EXCEPTION '0004 cannot invent living-at-death evidence; wipe Core XP awards first';
    END IF;
END $$;

ALTER TABLE character_xp_award_results
    ADD COLUMN normal_living_at_death BOOLEAN,
    ALTER COLUMN xp_profile_id DROP NOT NULL,
    DROP CONSTRAINT xp_profile_id_bounded,
    DROP CONSTRAINT xp_normal_evidence_shape,
    ADD CONSTRAINT xp_profile_id_bounded CHECK (
        xp_profile_id IS NULL OR length(xp_profile_id) BETWEEN 3 AND 96
    ),
    ADD CONSTRAINT xp_normal_evidence_shape CHECK (
        (eligibility_kind = 0
            AND normal_delta_x_milli_tiles IS NOT NULL
            AND normal_delta_y_milli_tiles IS NOT NULL
            AND normal_window_ticks IS NOT NULL
            AND normal_window_ticks BETWEEN 0 AND 300
            AND normal_actual_damage IS NOT NULL
            AND normal_actual_damage >= 0
            AND normal_effective_support IS NOT NULL
            AND normal_living_at_death IS NOT NULL
            AND encounter_active_ticks IS NULL
            AND encounter_present_ticks IS NULL
            AND encounter_longest_inactivity_ticks IS NULL
            AND encounter_reference_health IS NULL
            AND encounter_direct_damage IS NULL
            AND encounter_effective_healing IS NULL
            AND encounter_damage_prevented IS NULL
            AND encounter_objective_credits IS NULL
            AND encounter_life_state IS NULL
            AND encounter_recall_state IS NULL
            AND encounter_trust_state IS NULL)
        OR
        (eligibility_kind = 1
            AND normal_delta_x_milli_tiles IS NULL
            AND normal_delta_y_milli_tiles IS NULL
            AND normal_window_ticks IS NULL
            AND normal_actual_damage IS NULL
            AND normal_effective_support IS NULL
            AND normal_living_at_death IS NULL
            AND encounter_active_ticks IS NOT NULL
            AND encounter_active_ticks > 0
            AND encounter_present_ticks IS NOT NULL
            AND encounter_present_ticks BETWEEN 0 AND encounter_active_ticks
            AND encounter_longest_inactivity_ticks IS NOT NULL
            AND encounter_longest_inactivity_ticks >= 0
            AND encounter_reference_health IS NOT NULL
            AND encounter_reference_health > 0
            AND encounter_direct_damage IS NOT NULL
            AND encounter_direct_damage >= 0
            AND encounter_effective_healing IS NOT NULL
            AND encounter_effective_healing >= 0
            AND encounter_damage_prevented IS NOT NULL
            AND encounter_damage_prevented >= 0
            AND encounter_objective_credits IS NOT NULL
            AND encounter_objective_credits BETWEEN 0 AND 2
            AND encounter_life_state IS NOT NULL
            AND encounter_life_state BETWEEN 0 AND 1
            AND encounter_recall_state IS NOT NULL
            AND encounter_recall_state BETWEEN 0 AND 1
            AND encounter_trust_state IS NOT NULL
            AND encounter_trust_state BETWEEN 0 AND 2)
    );
