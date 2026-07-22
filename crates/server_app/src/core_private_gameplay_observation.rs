//! Bounded, presentation-only observations emitted by the authoritative private-life runtimes.
//!
//! The canonical GDD owns the 30 Hz simulation and 15 Hz latest-state snapshot contract
//! (`SIM-004`, `SIM-010`, `TECH-011`, `TECH-012`, `TECH-015`); the Content Production
//! Specification fixes the closed Core enemy/boss route; and the Development Roadmap requires
//! reconnect-safe ordinary play for `GB-M03-03`. Projection therefore happens beside the sole
//! runtime owner after commit, never by reconstructing state from network events.

use std::collections::{BTreeMap, BTreeSet};

use protocol::{
    CoreCombatActorBindingV1, CoreCombatActorKindV1, CoreCombatTelegraphV1, ENTITY_STATE_ALIVE,
    EntityKind, EntitySnapshot, MAX_SNAPSHOT_CHUNKS, MAX_SNAPSHOT_ENTITIES_PER_CHUNK,
    SnapshotChunk,
};
use sim_core::{CombatStep, EntityId, FriendlyProjectile, HostileProjectile, SimulationVector};
use thiserror::Error;

const MAX_PRIVATE_OBSERVATION_ENTITIES: usize =
    MAX_SNAPSHOT_ENTITIES_PER_CHUNK * MAX_SNAPSHOT_CHUNKS as usize;

pub(crate) fn combat_actor_binding(
    entity_id: EntityId,
    kind: CoreCombatActorKindV1,
    content_id: impl Into<String>,
) -> Result<CoreCombatActorBindingV1, CorePrivateGameplayObservationError> {
    Ok(CoreCombatActorBindingV1 {
        entity_id: entity_id.get(),
        kind,
        content_id: protocol::WireText::new(content_id.into())
            .map_err(|_| CorePrivateGameplayObservationError::InvalidPresentation)?,
    })
}

pub(crate) fn normal_wave_telegraphs(
    step: Option<&sim_core::NormalWaveStep>,
    entities: &[EntitySnapshot],
) -> Result<Vec<CoreCombatTelegraphV1>, CorePrivateGameplayObservationError> {
    let Some(step) = step else {
        return Ok(Vec::new());
    };
    step.timeline_events
        .iter()
        .filter_map(|timeline| {
            let origin = entities
                .iter()
                .find(|entity| entity.entity_id == timeline.entity_id.get())?;
            let (cast_id, resolves_at, pattern_id, damage_type, target, shape) =
                match &timeline.event {
                    sim_core::EnemyEvent::AimLocked {
                        cast_id,
                        direction,
                        fires_at,
                    } => (
                        cast_id.get(),
                        fires_at.0,
                        "pattern.enemy.drowned_pilgrim.fan",
                        protocol::CoreCombatDamageTypeV1::Physical,
                        (
                            origin.x_milli_tiles.saturating_add(direction.x),
                            origin.y_milli_tiles.saturating_add(direction.y),
                        ),
                        protocol::CoreCombatTelegraphShapeV1::Fan {
                            ray_count: 3,
                            ray_offsets_milli_degrees: [-15_000, 0, 15_000, 0, 0, 0, 0, 0],
                            extent_milli_tiles: 12_100,
                            ray_width_milli_tiles: 240,
                        },
                    ),
                    sim_core::EnemyEvent::RingTelegraph {
                        cast_id,
                        omitted_indices,
                        fires_at,
                    } => (
                        cast_id.get(),
                        fires_at.0,
                        "pattern.enemy.bell_reed.gap_ring",
                        protocol::CoreCombatDamageTypeV1::Veil,
                        (origin.x_milli_tiles, origin.y_milli_tiles),
                        protocol::CoreCombatTelegraphShapeV1::Ring {
                            segment_count: 8,
                            gap_start_index: omitted_indices[0],
                            gap_count: 2,
                            radius_milli_tiles: 2_200,
                            segment_width_milli_tiles: 260,
                        },
                    ),
                    sim_core::EnemyEvent::LaneTelegraph {
                        cast_id,
                        axes_degrees,
                        impacts_at,
                        ..
                    } => (
                        cast_id.get(),
                        impacts_at.0,
                        "pattern.enemy.chain_sentry.cross_lanes",
                        protocol::CoreCombatDamageTypeV1::Physical,
                        (origin.x_milli_tiles, origin.y_milli_tiles),
                        protocol::CoreCombatTelegraphShapeV1::Lanes {
                            axes_degrees: *axes_degrees,
                            width_milli_tiles: 900,
                        },
                    ),
                    _ => return None,
                };
            let Ok(pattern_id) = protocol::WireText::new(pattern_id) else {
                return Some(Err(
                    CorePrivateGameplayObservationError::InvalidPresentation,
                ));
            };
            Some(Ok(CoreCombatTelegraphV1 {
                source_entity_id: timeline.entity_id.get(),
                cast_id,
                pattern_id,
                damage_type,
                starts_at_tick: step.tick.0,
                resolves_at_tick: resolves_at,
                origin_x_milli_tiles: origin.x_milli_tiles,
                origin_y_milli_tiles: origin.y_milli_tiles,
                target_x_milli_tiles: target.0,
                target_y_milli_tiles: target.1,
                shape,
            }))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CorePrivateGameplayObservation {
    pub tick: u64,
    pub actor_generation: u64,
    pub route_state_version: u64,
    pub acknowledged_input_sequence: u32,
    pub entities: Vec<EntitySnapshot>,
    pub presentation_actors: Vec<CoreCombatActorBindingV1>,
    pub presentation_telegraphs: Vec<CoreCombatTelegraphV1>,
}

impl CorePrivateGameplayObservation {
    pub(crate) fn new(
        tick: u64,
        actor_generation: u64,
        route_state_version: u64,
        acknowledged_input_sequence: u64,
        mut entities: Vec<EntitySnapshot>,
    ) -> Result<Self, CorePrivateGameplayObservationError> {
        if tick == 0 || actor_generation == 0 || route_state_version == 0 || entities.is_empty() {
            return Err(CorePrivateGameplayObservationError::InvalidAuthority);
        }
        let acknowledged_input_sequence = u32::try_from(acknowledged_input_sequence)
            .map_err(|_| CorePrivateGameplayObservationError::InputSequenceOverflow)?;
        if entities.len() > MAX_PRIVATE_OBSERVATION_ENTITIES {
            return Err(CorePrivateGameplayObservationError::EntityOverflow);
        }
        entities.sort_by_key(|entity| entity.entity_id);
        let mut identities = BTreeSet::new();
        for entity in &entities {
            entity
                .validate()
                .map_err(|_| CorePrivateGameplayObservationError::InvalidEntity)?;
            if !identities.insert(entity.entity_id) {
                return Err(CorePrivateGameplayObservationError::DuplicateEntity);
            }
        }
        Ok(Self {
            tick,
            actor_generation,
            route_state_version,
            acknowledged_input_sequence,
            entities,
            presentation_actors: Vec::new(),
            presentation_telegraphs: Vec::new(),
        })
    }

    pub(crate) fn with_presentation(
        mut self,
        mut actors: Vec<CoreCombatActorBindingV1>,
        mut telegraphs: Vec<CoreCombatTelegraphV1>,
    ) -> Result<Self, CorePrivateGameplayObservationError> {
        if actors.len() > protocol::CORE_COMBAT_PRESENTATION_MAX_ACTORS
            || telegraphs.len() > protocol::CORE_COMBAT_PRESENTATION_MAX_TELEGRAPHS
        {
            return Err(CorePrivateGameplayObservationError::PresentationOverflow);
        }
        let entity_ids = self
            .entities
            .iter()
            .map(|entity| entity.entity_id)
            .collect::<BTreeSet<_>>();
        let required_actor_ids = self
            .entities
            .iter()
            .filter(|entity| {
                matches!(
                    entity.kind,
                    EntityKind::Player | EntityKind::Enemy | EntityKind::Boss
                )
            })
            .map(|entity| entity.entity_id)
            .collect::<BTreeSet<_>>();
        let mut actor_ids = BTreeSet::new();
        for actor in &actors {
            let snapshot_kind = self
                .entities
                .iter()
                .find(|entity| entity.entity_id == actor.entity_id)
                .map(|entity| entity.kind);
            let kind_matches = matches!(
                (actor.kind, snapshot_kind),
                (CoreCombatActorKindV1::Player, Some(EntityKind::Player))
                    | (CoreCombatActorKindV1::Enemy, Some(EntityKind::Enemy))
                    | (CoreCombatActorKindV1::Boss, Some(EntityKind::Boss))
            );
            if !entity_ids.contains(&actor.entity_id)
                || !kind_matches
                || !actor_ids.insert(actor.entity_id)
            {
                return Err(CorePrivateGameplayObservationError::InvalidPresentation);
            }
        }
        if actor_ids != required_actor_ids
            || telegraphs
                .iter()
                .any(|telegraph| !actor_ids.contains(&telegraph.source_entity_id))
        {
            return Err(CorePrivateGameplayObservationError::InvalidPresentation);
        }
        actors.sort_by_key(|actor| actor.entity_id);
        telegraphs.sort_by_key(|telegraph| (telegraph.source_entity_id, telegraph.cast_id));
        self.presentation_actors = actors;
        self.presentation_telegraphs = telegraphs;
        Ok(self)
    }

    pub(crate) fn snapshot_chunks(
        &self,
        sequence: u32,
    ) -> Result<Vec<SnapshotChunk>, CorePrivateGameplayObservationError> {
        if sequence == 0 {
            return Err(CorePrivateGameplayObservationError::InvalidSnapshotSequence);
        }
        let chunk_count = self
            .entities
            .len()
            .div_ceil(MAX_SNAPSHOT_ENTITIES_PER_CHUNK);
        let chunk_count = u16::try_from(chunk_count)
            .map_err(|_| CorePrivateGameplayObservationError::EntityOverflow)?;
        self.entities
            .chunks(MAX_SNAPSHOT_ENTITIES_PER_CHUNK)
            .enumerate()
            .map(|(index, entities)| {
                let chunk = SnapshotChunk {
                    sequence,
                    server_tick: self.tick,
                    state_version: self.route_state_version,
                    acknowledged_input_sequence: self.acknowledged_input_sequence,
                    chunk_index: u16::try_from(index)
                        .map_err(|_| CorePrivateGameplayObservationError::EntityOverflow)?,
                    chunk_count,
                    entities: entities.to_vec(),
                };
                chunk
                    .validate()
                    .map_err(|_| CorePrivateGameplayObservationError::InvalidSnapshotChunk)?;
                Ok(chunk)
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CorePrivateProjectileProvenance {
    by_projectile: BTreeMap<EntityId, (u32, u16)>,
}

impl CorePrivateProjectileProvenance {
    pub(crate) fn apply_committed_combat(
        &mut self,
        combat: &CombatStep,
        active: &[FriendlyProjectile],
    ) -> Result<(), CorePrivateGameplayObservationError> {
        let mut ordinals = BTreeMap::<u32, u16>::new();
        for shot in &combat.shots {
            let ordinal = ordinals.entry(shot.press_sequence).or_default();
            self.by_projectile
                .insert(shot.projectile.id(), (shot.press_sequence, *ordinal));
            *ordinal = ordinal
                .checked_add(1)
                .ok_or(CorePrivateGameplayObservationError::ProjectileOrdinalOverflow)?;
        }
        self.by_projectile.retain(|projectile_id, _| {
            active
                .iter()
                .any(|projectile| projectile.id() == *projectile_id)
        });
        Ok(())
    }

    pub(crate) fn friendly_snapshot(
        &self,
        owner: EntityId,
        projectile: &FriendlyProjectile,
    ) -> Result<EntitySnapshot, CorePrivateGameplayObservationError> {
        let (source_input_sequence, source_projectile_ordinal) = self
            .by_projectile
            .get(&projectile.id())
            .copied()
            .ok_or(CorePrivateGameplayObservationError::MissingProjectileProvenance)?;
        entity_snapshot(
            projectile.id().get(),
            EntityKind::FriendlyProjectile,
            projectile.position(),
            projectile.direction().vector() * projectile.speed_tiles_per_second(),
            owner.get(),
            source_input_sequence,
            source_projectile_ordinal,
            0,
            0,
            true,
        )
    }
}

pub(crate) fn player_snapshot(
    player: &sim_core::EnemyLabPlayer,
    position: SimulationVector,
    velocity: SimulationVector,
) -> Result<EntitySnapshot, CorePrivateGameplayObservationError> {
    let vitals = player.consumables.vitals();
    entity_snapshot(
        player.target.entity_id.get(),
        EntityKind::Player,
        position,
        velocity,
        0,
        0,
        0,
        vitals.current_health(),
        vitals.maximum_health(),
        vitals.current_health() != 0,
    )
}

pub(crate) fn enemy_snapshot(
    entity_id: EntityId,
    position: SimulationVector,
    current_health: u32,
    maximum_health: u32,
    alive: bool,
) -> Result<EntitySnapshot, CorePrivateGameplayObservationError> {
    entity_snapshot(
        entity_id.get(),
        EntityKind::Enemy,
        position,
        SimulationVector::default(),
        0,
        0,
        0,
        current_health,
        maximum_health,
        alive,
    )
}

pub(crate) fn boss_snapshot(
    entity_id: EntityId,
    position: SimulationVector,
    current_health: u32,
    maximum_health: u32,
    alive: bool,
) -> Result<EntitySnapshot, CorePrivateGameplayObservationError> {
    let mut snapshot = enemy_snapshot(entity_id, position, current_health, maximum_health, alive)?;
    snapshot.kind = EntityKind::Boss;
    snapshot
        .validate()
        .map_err(|_| CorePrivateGameplayObservationError::InvalidEntity)?;
    Ok(snapshot)
}

pub(crate) fn hostile_projectile_snapshot(
    projectile: &HostileProjectile,
) -> Result<EntitySnapshot, CorePrivateGameplayObservationError> {
    entity_snapshot(
        projectile.id().get(),
        EntityKind::HostileProjectile,
        projectile.position(),
        projectile.direction().vector() * projectile.speed_tiles_per_second(),
        0,
        0,
        0,
        0,
        0,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn entity_snapshot(
    entity_id: u64,
    kind: EntityKind,
    position: SimulationVector,
    velocity: SimulationVector,
    source_entity_id: u64,
    source_input_sequence: u32,
    source_projectile_ordinal: u16,
    current_health: u32,
    maximum_health: u32,
    alive: bool,
) -> Result<EntitySnapshot, CorePrivateGameplayObservationError> {
    let snapshot = EntitySnapshot {
        entity_id,
        kind,
        x_milli_tiles: tiles_to_milli(position.x)?,
        y_milli_tiles: tiles_to_milli(position.y)?,
        velocity_x_milli_tiles_per_second: tiles_to_milli(velocity.x)?,
        velocity_y_milli_tiles_per_second: tiles_to_milli(velocity.y)?,
        source_entity_id,
        source_input_sequence,
        source_projectile_ordinal,
        current_health,
        maximum_health,
        state_flags: if alive { ENTITY_STATE_ALIVE } else { 0 },
    };
    snapshot
        .validate()
        .map_err(|_| CorePrivateGameplayObservationError::InvalidEntity)?;
    Ok(snapshot)
}

#[allow(clippy::cast_possible_truncation)]
fn tiles_to_milli(value: f32) -> Result<i32, CorePrivateGameplayObservationError> {
    if !value.is_finite() {
        return Err(CorePrivateGameplayObservationError::NonFinitePosition);
    }
    let scaled = (f64::from(value) * 1_000.0).round();
    if scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
        return Err(CorePrivateGameplayObservationError::PositionOverflow);
    }
    Ok(scaled as i32)
}

#[cfg(test)]
pub(crate) fn core_private_gameplay_observation_test_fixture(
    tick: u64,
    actor_generation: u64,
    route_state_version: u64,
    acknowledged_input_sequence: u32,
) -> CorePrivateGameplayObservation {
    CorePrivateGameplayObservation::new(
        tick.max(1),
        actor_generation,
        route_state_version,
        u64::from(acknowledged_input_sequence),
        vec![EntitySnapshot {
            entity_id: 1,
            kind: EntityKind::Player,
            x_milli_tiles: 0,
            y_milli_tiles: 0,
            velocity_x_milli_tiles_per_second: 0,
            velocity_y_milli_tiles_per_second: 0,
            source_entity_id: 0,
            source_input_sequence: 0,
            source_projectile_ordinal: 0,
            current_health: 1,
            maximum_health: 1,
            state_flags: ENTITY_STATE_ALIVE,
        }],
    )
    .expect("canonical private gameplay observation fixture")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(entity_id: u64, kind: EntityKind) -> EntitySnapshot {
        EntitySnapshot {
            entity_id,
            kind,
            x_milli_tiles: 1_000,
            y_milli_tiles: 1_000,
            velocity_x_milli_tiles_per_second: 0,
            velocity_y_milli_tiles_per_second: 0,
            source_entity_id: 0,
            source_input_sequence: 0,
            source_projectile_ordinal: 0,
            current_health: 10,
            maximum_health: 10,
            state_flags: ENTITY_STATE_ALIVE,
        }
    }

    #[test]
    fn combat_presentation_rejects_an_incomplete_actor_binding_set() {
        let observation = CorePrivateGameplayObservation::new(
            1,
            2,
            3,
            0,
            vec![actor(1, EntityKind::Player), actor(2, EntityKind::Enemy)],
        )
        .expect("valid observation");
        let result = observation.with_presentation(
            vec![CoreCombatActorBindingV1 {
                entity_id: 1,
                kind: CoreCombatActorKindV1::Player,
                content_id: protocol::WireText::new(protocol::GRAVE_ARBALIST_CLASS_ID)
                    .expect("class id"),
            }],
            Vec::new(),
        );

        assert_eq!(
            result,
            Err(CorePrivateGameplayObservationError::InvalidPresentation)
        );
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CorePrivateGameplayObservationError {
    #[error("private gameplay observation authority is invalid")]
    InvalidAuthority,
    #[error("private gameplay input acknowledgement overflowed")]
    InputSequenceOverflow,
    #[error("private gameplay snapshot sequence is invalid")]
    InvalidSnapshotSequence,
    #[error("private gameplay observation exceeds its entity bound")]
    EntityOverflow,
    #[error("private gameplay observation contains an invalid entity")]
    InvalidEntity,
    #[error("private gameplay observation contains duplicate entities")]
    DuplicateEntity,
    #[error("private gameplay observation produced an invalid snapshot chunk")]
    InvalidSnapshotChunk,
    #[error("private gameplay projectile ordinal overflowed")]
    ProjectileOrdinalOverflow,
    #[error("private gameplay projectile is missing input provenance")]
    MissingProjectileProvenance,
    #[error("private gameplay position is non-finite")]
    NonFinitePosition,
    #[error("private gameplay position exceeds fixed-point bounds")]
    PositionOverflow,
    #[error("private gameplay presentation exceeds its bound")]
    PresentationOverflow,
    #[error("private gameplay presentation is not bound to its snapshot")]
    InvalidPresentation,
}
