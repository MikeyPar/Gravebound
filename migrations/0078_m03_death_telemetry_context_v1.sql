-- TEL-003 facts captured by the same terminal-frame authority as durable death.
-- Historical wipeable-Core rows remain nullable and are never backfilled with invented values.
-- Authority: Gravebound_Production_GDD_v1_Canonical.md (TEL-003/SOC-010/SIM-012),
-- Gravebound_Content_Production_Spec_v1.md (Core capacity one/CONT-BOSS-001/002),
-- and Gravebound_Development_Roadmap_v1.md (GB-M03-06/09).
ALTER TABLE death_events
    ADD COLUMN party_size SMALLINT,
    ADD COLUMN boss_phase_id TEXT,
    ADD COLUMN contribution_centi_units BIGINT,
    ADD COLUMN contribution_reference_health BIGINT,
    ADD COLUMN network_source_kind SMALLINT,
    ADD COLUMN network_transport_generation BIGINT,
    ADD COLUMN network_sampled_at_unix_ms BIGINT,
    ADD COLUMN network_ping_millis INTEGER,
    ADD COLUMN network_jitter_millis INTEGER,
    ADD COLUMN network_loss_basis_points INTEGER,
    ADD COLUMN network_correction_count BIGINT,
    ADD CONSTRAINT death_events_m03_party_size_check CHECK (
        party_size IS NULL OR party_size = 1
    ),
    ADD CONSTRAINT death_events_boss_phase_shape_check CHECK (
        boss_phase_id IS NULL OR (
            char_length(boss_phase_id) BETWEEN 3 AND 96
            AND boss_phase_id ~ '^[a-z0-9_-]+(\.[a-z0-9_-]+)+$'
        )
    ),
    ADD CONSTRAINT death_events_contribution_shape_check CHECK (
        num_nonnulls(contribution_centi_units, contribution_reference_health) IN (0, 2)
        AND (contribution_centi_units IS NULL) = (boss_phase_id IS NULL)
        AND (
            contribution_centi_units IS NULL
            OR (
                contribution_centi_units >= 0
                AND contribution_reference_health > 0
                AND contribution_centi_units::numeric
                    <= contribution_reference_health::numeric * 100
            )
        )
    ),
    ADD CONSTRAINT death_events_network_health_shape_check CHECK (
        num_nonnulls(
            network_source_kind,
            network_transport_generation,
            network_sampled_at_unix_ms,
            network_ping_millis,
            network_jitter_millis,
            network_loss_basis_points
        ) IN (0, 6)
        AND (
            network_source_kind IS NULL
            OR (
                network_source_kind = 1
                AND network_transport_generation > 0
                AND network_sampled_at_unix_ms > 0
                AND network_ping_millis BETWEEN 0 AND 65535
                AND network_jitter_millis BETWEEN 0 AND 65535
                AND network_loss_basis_points BETWEEN 0 AND 10000
            )
        )
        AND (
            network_correction_count IS NULL
            OR (
                network_source_kind IS NOT NULL
                AND network_correction_count BETWEEN 0 AND 4294967295
            )
        )
    ),
    ADD CONSTRAINT death_events_m03_telemetry_generation_check CHECK (
        (
            party_size IS NULL
            AND boss_phase_id IS NULL
            AND contribution_centi_units IS NULL
            AND contribution_reference_health IS NULL
            AND network_source_kind IS NULL
            AND network_transport_generation IS NULL
            AND network_sampled_at_unix_ms IS NULL
            AND network_ping_millis IS NULL
            AND network_jitter_millis IS NULL
            AND network_loss_basis_points IS NULL
            AND network_correction_count IS NULL
        )
        OR (
            party_size = 1
            AND network_source_kind = 1
        )
    );

COMMENT ON COLUMN death_events.party_size IS
    'M03 private-life authored capacity; exactly one for new M03 deaths, NULL only for historical rows.';
COMMENT ON COLUMN death_events.network_source_kind IS
    '1 = server-observed QUIC RTT/jitter/loss; client correction count remains explicitly optional.';
