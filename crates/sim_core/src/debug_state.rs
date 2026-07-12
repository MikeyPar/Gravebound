//! Stable gameplay-only debug snapshots for `LocalLab` inspection and replay comparison.

use thiserror::Error;

use crate::{EntityId, Tick};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugEnemyState {
    pub entity_id: EntityId,
    pub x_bits: u32,
    pub y_bits: u32,
    pub health: u32,
    pub alive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugBossState {
    pub entity_id: EntityId,
    pub local_tick: Tick,
    pub state_code: u8,
    pub current_health: u32,
    pub maximum_health: u32,
    pub active_projectiles: u32,
    pub active_lanes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDebugStateInput {
    pub run_ordinal: u32,
    pub seed: u64,
    pub encounter_tick: Tick,
    pub encounter_state_code: u8,
    pub combat_tick: Tick,
    pub player_x_bits: u32,
    pub player_y_bits: u32,
    pub health: u32,
    pub maximum_health: u32,
    pub enemies: Vec<DebugEnemyState>,
    pub boss: Option<DebugBossState>,
    pub friendly_projectiles: u32,
    pub hostile_projectiles: u32,
    pub hostile_hazards: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDebugStateSnapshot {
    pub state_hash_blake3: String,
    pub enemy_count: u32,
    pub projectile_count: u32,
}

impl LocalDebugStateSnapshot {
    pub fn compile(input: &LocalDebugStateInput) -> Result<Self, DebugStateError> {
        if input.run_ordinal == 0
            || input.maximum_health == 0
            || input.health > input.maximum_health
        {
            return Err(DebugStateError::InvalidPlayerState);
        }
        if !f32::from_bits(input.player_x_bits).is_finite()
            || !f32::from_bits(input.player_y_bits).is_finite()
        {
            return Err(DebugStateError::NonFinitePlayerPosition);
        }
        if input
            .enemies
            .windows(2)
            .any(|pair| pair[0].entity_id >= pair[1].entity_id)
        {
            return Err(DebugStateError::EnemyOrder);
        }
        if input.enemies.iter().any(|enemy| {
            !f32::from_bits(enemy.x_bits).is_finite() || !f32::from_bits(enemy.y_bits).is_finite()
        }) {
            return Err(DebugStateError::NonFiniteEnemyPosition);
        }
        if input.boss.is_some_and(|boss| {
            boss.maximum_health == 0 || boss.current_health > boss.maximum_health
        }) {
            return Err(DebugStateError::InvalidBossState);
        }
        let projectile_count = input
            .friendly_projectiles
            .checked_add(input.hostile_projectiles)
            .ok_or(DebugStateError::CountOverflow)?;
        let enemy_count =
            u32::try_from(input.enemies.len()).map_err(|_| DebugStateError::CountOverflow)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"gravebound-local-debug-state-v3\0");
        hasher.update(&input.run_ordinal.to_le_bytes());
        hasher.update(&input.seed.to_le_bytes());
        hasher.update(&input.encounter_tick.0.to_le_bytes());
        hasher.update(&[input.encounter_state_code]);
        hasher.update(&input.combat_tick.0.to_le_bytes());
        hasher.update(&input.player_x_bits.to_le_bytes());
        hasher.update(&input.player_y_bits.to_le_bytes());
        hasher.update(&input.health.to_le_bytes());
        hasher.update(&input.maximum_health.to_le_bytes());
        for enemy in &input.enemies {
            hasher.update(&enemy.entity_id.get().to_le_bytes());
            hasher.update(&enemy.x_bits.to_le_bytes());
            hasher.update(&enemy.y_bits.to_le_bytes());
            hasher.update(&enemy.health.to_le_bytes());
            hasher.update(&[u8::from(enemy.alive)]);
        }
        match input.boss {
            Some(boss) => {
                hasher.update(&[1]);
                hasher.update(&boss.entity_id.get().to_le_bytes());
                hasher.update(&boss.local_tick.0.to_le_bytes());
                hasher.update(&[boss.state_code]);
                hasher.update(&boss.current_health.to_le_bytes());
                hasher.update(&boss.maximum_health.to_le_bytes());
                hasher.update(&boss.active_projectiles.to_le_bytes());
                hasher.update(&boss.active_lanes.to_le_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
        hasher.update(&input.friendly_projectiles.to_le_bytes());
        hasher.update(&input.hostile_projectiles.to_le_bytes());
        hasher.update(&input.hostile_hazards.to_le_bytes());
        Ok(Self {
            state_hash_blake3: hasher.finalize().to_hex().to_string(),
            enemy_count,
            projectile_count,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DebugStateError {
    #[error("debug player state is invalid")]
    InvalidPlayerState,
    #[error("debug player position is nonfinite")]
    NonFinitePlayerPosition,
    #[error("debug enemy states must be strictly sorted by entity ID")]
    EnemyOrder,
    #[error("debug enemy position is nonfinite")]
    NonFiniteEnemyPosition,
    #[error("debug state count overflowed")]
    CountOverflow,
    #[error("debug boss state is invalid")]
    InvalidBossState,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> LocalDebugStateInput {
        LocalDebugStateInput {
            run_ordinal: 1,
            seed: 0xB311_A501,
            encounter_tick: Tick(5),
            encounter_state_code: 3,
            combat_tick: Tick(5),
            player_x_bits: 4.0_f32.to_bits(),
            player_y_bits: 12.0_f32.to_bits(),
            health: 128,
            maximum_health: 128,
            enemies: vec![DebugEnemyState {
                entity_id: EntityId::new(10).expect("ID"),
                x_bits: 8.0_f32.to_bits(),
                y_bits: 3.0_f32.to_bits(),
                health: 85,
                alive: true,
            }],
            boss: None,
            friendly_projectiles: 2,
            hostile_projectiles: 4,
            hostile_hazards: 1,
        }
    }

    #[test]
    fn identical_state_hashes_identically_and_gameplay_mutation_changes_hash() {
        let first = LocalDebugStateSnapshot::compile(&input()).expect("snapshot");
        assert_eq!(
            first,
            LocalDebugStateSnapshot::compile(&input()).expect("replay")
        );
        let mut changed = input();
        changed.health -= 1;
        assert_ne!(
            first.state_hash_blake3,
            LocalDebugStateSnapshot::compile(&changed)
                .expect("changed")
                .state_hash_blake3
        );
        assert_eq!(first.enemy_count, 1);
        assert_eq!(first.projectile_count, 6);

        let mut boss_changed = input();
        boss_changed.boss = Some(DebugBossState {
            entity_id: EntityId::new(40_001).expect("boss"),
            local_tick: Tick(12),
            state_code: 1,
            current_health: 3_000,
            maximum_health: 3_000,
            active_projectiles: 5,
            active_lanes: 0,
        });
        assert_ne!(
            first.state_hash_blake3,
            LocalDebugStateSnapshot::compile(&boss_changed)
                .expect("boss state")
                .state_hash_blake3
        );
    }

    #[test]
    fn ambiguous_enemy_order_and_nonfinite_position_fail_closed() {
        let mut duplicate = input();
        duplicate.enemies.push(duplicate.enemies[0]);
        assert_eq!(
            LocalDebugStateSnapshot::compile(&duplicate),
            Err(DebugStateError::EnemyOrder)
        );
        let mut nonfinite = input();
        nonfinite.player_x_bits = f32::NAN.to_bits();
        assert_eq!(
            LocalDebugStateSnapshot::compile(&nonfinite),
            Err(DebugStateError::NonFinitePlayerPosition)
        );
    }
}
