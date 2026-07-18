//! Shared server-owned player movement/combat frame primitive for Core private-danger scenes.
//!
//! The three authorities are `Gravebound_Production_GDD_v1_Canonical.md` (`SIM-004`,
//! `TECH-012`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-WORLD-001`,
//! `CONT-ROOM-007`), and `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`). Scene runtimes
//! stage this primitive with their own lifecycle and route CAS; it never commits authority alone.

use sim_core::{
    ArenaGeometry, CombatAction, CombatStep, ConsumableAction, MOVEMENT_RESPONSE_TICKS,
    MovementStep, PlayerMovementConfig, PlayerMovementState, ProjectileCollisionWorld,
};

use crate::{CorePrivateMicrorealmInput, CorePrivateMicrorealmRuntimeError};

#[allow(clippy::cast_precision_loss)]
pub(crate) fn core_player_movement_config(
    movement_milli_tiles_per_second: u32,
    player_radius_milli_tiles: i32,
) -> Result<PlayerMovementConfig, CorePrivateMicrorealmRuntimeError> {
    if movement_milli_tiles_per_second == 0 || player_radius_milli_tiles <= 0 {
        return Err(CorePrivateMicrorealmRuntimeError::InvalidComposition);
    }
    Ok(PlayerMovementConfig {
        final_speed_tiles_per_second: movement_milli_tiles_per_second as f32 / 1_000.0,
        response_ticks: MOVEMENT_RESPONSE_TICKS,
        collision_radius_tiles: player_radius_milli_tiles as f32 / 1_000.0,
    })
}

pub(crate) fn step_live_player_combat(
    player: &mut sim_core::EnemyLabPlayer,
    movement: &mut PlayerMovementState,
    input: &CorePrivateMicrorealmInput,
    arena: &ArenaGeometry,
    collision_world: &ProjectileCollisionWorld,
) -> Result<(CombatStep, MovementStep), CorePrivateMicrorealmRuntimeError> {
    if player.target.position != movement.position() {
        return Err(CorePrivateMicrorealmRuntimeError::InvalidComposition);
    }
    let (step, movement_step) = player.combat.step_with_movement_outcome(
        movement,
        CombatAction {
            aim: input.aim,
            movement: input.movement,
            primary_held: input.primary_held,
            primary_press_sequence: input.primary_sequence,
            ability_1_press_sequence: input.ability_1_sequence,
            ability_2_press_sequence: input.ability_2_sequence,
        },
        arena,
        collision_world,
    )?;
    player.target.position = movement_step.position;
    player.consumables.step(ConsumableAction::default())?;
    player
        .target
        .additional_direct_damage_reductions_basis_points =
        (step.direct_damage_reduction_basis_points != 0)
            .then_some(step.direct_damage_reduction_basis_points)
            .into_iter()
            .collect();
    Ok((step, movement_step))
}
