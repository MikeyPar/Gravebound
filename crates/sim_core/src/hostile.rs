//! Authoritative First Playable hostile projectile, lane, actor, collision, and damage integration.
//!
//! The GDD `SIM-005`, `SIM-010`, `SIM-011`, and `COM-001` through `COM-003` establish the
//! simulation, collision, and damage order. `CONT-FP-004` supplies the exact fan/ring/lane
//! payloads. The roadmap orders this seam after `GB-M01-03A`–`03C` and before `GB-M01-04A` and
//! `GB-M01-05A`. Enemy timelines authorize spawns; this module never invents an early attack.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    AimDirection, AimDirectionError, AimVector, ArenaGeometry, AttackCastId, BossEvent,
    CollisionError, CollisionTarget, Counterplay, DamageAppliedEvent, DamageBand, DamageError,
    DamageEvent, DamageType, DirectHitParameters, DirectHitRequest, EchoMemoryFamily, EnemyEvent,
    EnemyHurtbox, EntityId, EntityIdAllocator, FocusedTransition, HostileDisposition, HurtboxError,
    LaneAttackDefinition, PilgrimTargetInput, PlayerCombatState, ProjectileAttackDefinition,
    ProjectileCollisionWorld, RedTonicSimulation, SimulationVector, SolidColliderId, SweepHit,
    Tick, resolve_direct_hit,
};

pub const PLAYER_HURTBOX_RADIUS_TILES: f32 = 0.25;
pub const HOSTILE_PROJECTILE_GRACE_TICKS: u64 = 3;
const TICKS_PER_SECOND_F32: f32 = 30.0;
const MICRO_UNITS: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostileProjectileSourceKind {
    AimedFan,
    GapRing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostileProjectile {
    id: EntityId,
    source_entity_id: EntityId,
    cast_id: AttackCastId,
    source_kind: HostileProjectileSourceKind,
    pattern_id: &'static str,
    position: SimulationVector,
    direction: AimDirection,
    speed_tiles_per_second: f32,
    radius_tiles: f32,
    remaining_lifetime_ticks: u32,
    raw_damage: u32,
    damage_type: DamageType,
    declared_damage_band: DamageBand,
    threat_cost: u32,
    memory_family: EchoMemoryFamily,
    counterplay: Counterplay,
    disposition: HostileDisposition,
    pierces_players: bool,
    ignored_player_ids: BTreeSet<EntityId>,
}

impl HostileProjectile {
    #[must_use]
    pub const fn id(&self) -> EntityId {
        self.id
    }

    #[must_use]
    pub const fn source_entity_id(&self) -> EntityId {
        self.source_entity_id
    }

    #[must_use]
    pub const fn cast_id(&self) -> AttackCastId {
        self.cast_id
    }

    #[must_use]
    pub const fn source_kind(&self) -> HostileProjectileSourceKind {
        self.source_kind
    }

    #[must_use]
    pub const fn pattern_id(&self) -> &'static str {
        self.pattern_id
    }

    #[must_use]
    pub const fn position(&self) -> SimulationVector {
        self.position
    }

    #[must_use]
    pub const fn direction(&self) -> AimDirection {
        self.direction
    }

    #[must_use]
    pub const fn speed_tiles_per_second(&self) -> f32 {
        self.speed_tiles_per_second
    }

    #[must_use]
    pub const fn radius_tiles(&self) -> f32 {
        self.radius_tiles
    }

    #[must_use]
    pub const fn remaining_lifetime_ticks(&self) -> u32 {
        self.remaining_lifetime_ticks
    }

    #[must_use]
    pub const fn raw_damage(&self) -> u32 {
        self.raw_damage
    }

    #[must_use]
    pub const fn damage_type(&self) -> DamageType {
        self.damage_type
    }

    #[must_use]
    pub const fn declared_damage_band(&self) -> DamageBand {
        self.declared_damage_band
    }

    #[must_use]
    pub const fn pierces_players(&self) -> bool {
        self.pierces_players
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostileTargetState {
    pub entity_id: EntityId,
    pub position: SimulationVector,
    pub target_is_immune: bool,
    pub resistance_basis_points: i32,
    pub additional_direct_damage_reductions_basis_points: Vec<u32>,
    pub armor: u32,
    pub current_barrier: u32,
    pub health_damage_cap_basis_points: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostileEvent {
    Spawned {
        tick: Tick,
        projectile: HostileProjectile,
    },
    Moved {
        tick: Tick,
        projectile_id: EntityId,
        from: SimulationVector,
        to: SimulationVector,
    },
    Contact {
        tick: Tick,
        projectile_id: EntityId,
        source_entity_id: EntityId,
        pattern_id: &'static str,
        cast_id: AttackCastId,
        target: HostileCollisionTarget,
        position: SimulationVector,
        declared_damage_band: DamageBand,
        damage: Option<DamageEvent>,
        health_application: Option<DamageAppliedEvent>,
        debug_invulnerable: bool,
        focused_transition: Option<FocusedTransition>,
    },
    ProjectileGraceIgnored {
        tick: Tick,
        projectile_id: EntityId,
        source_entity_id: EntityId,
        pattern_id: &'static str,
        player_entity_id: EntityId,
        pierces_players: bool,
        consumed: bool,
    },
    Expired {
        tick: Tick,
        projectile_id: EntityId,
        final_position: SimulationVector,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostileCollisionTarget {
    Solid(SolidColliderId),
    Player(EntityId),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostileStep {
    pub tick: Tick,
    pub events: Vec<HostileEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostileProjectileSimulation {
    tick: Tick,
    projectile_ids: EntityIdAllocator,
    projectiles: Vec<HostileProjectile>,
    projectile_damage_allowed_at: Tick,
    damage_policy: HostileDamagePolicy,
}

impl Default for HostileProjectileSimulation {
    fn default() -> Self {
        Self {
            tick: Tick(0),
            projectile_ids: EntityIdAllocator::default(),
            projectiles: Vec::new(),
            projectile_damage_allowed_at: Tick(0),
            damage_policy: HostileDamagePolicy::Standard,
        }
    }
}

impl HostileProjectileSimulation {
    #[must_use]
    pub fn with_allocator(projectile_ids: EntityIdAllocator) -> Self {
        Self {
            tick: Tick(0),
            projectile_ids,
            projectiles: Vec::new(),
            projectile_damage_allowed_at: Tick(0),
            damage_policy: HostileDamagePolicy::Standard,
        }
    }

    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    #[must_use]
    pub fn projectiles(&self) -> &[HostileProjectile] {
        &self.projectiles
    }

    pub fn set_damage_policy(&mut self, policy: HostileDamagePolicy) {
        self.damage_policy = policy;
    }

    /// Drains all active projectiles in stable ID order without rewinding time or identity.
    pub fn clear_projectiles(&mut self) -> Vec<HostileProjectile> {
        self.projectiles.sort_by_key(HostileProjectile::id);
        std::mem::take(&mut self.projectiles)
    }

    /// Returns the monotonic allocator when a higher-level encounter transfers ownership between
    /// hostile phases. Active projectiles remain the caller's responsibility before handoff.
    pub(crate) fn into_allocator(self) -> EntityIdAllocator {
        self.projectile_ids
    }

    /// Consumes only an enemy's authoritative fire event. Telegraph/state events cannot spawn.
    pub fn spawn_from_enemy_event(
        &mut self,
        source_entity_id: EntityId,
        origin: SimulationVector,
        event: &EnemyEvent,
    ) -> Result<Vec<HostileEvent>, HostileError> {
        let mut next = self.clone();
        let events = next.spawn_inner(source_entity_id, origin, event)?;
        *self = next;
        Ok(events)
    }

    /// Consumes one authoritative Bell Proctor fire event without weakening the ordinary-enemy
    /// grammar. Telegraph, preview, lane, and lifecycle events remain non-spawning.
    pub fn spawn_from_boss_event(
        &mut self,
        source_entity_id: EntityId,
        origin: SimulationVector,
        event: &BossEvent,
    ) -> Result<Vec<HostileEvent>, HostileError> {
        let mut next = self.clone();
        let spawned = next.spawn_boss_inner(source_entity_id, origin, event)?;
        *self = next;
        Ok(spawned)
    }

    fn spawn_boss_inner(
        &mut self,
        source_entity_id: EntityId,
        origin: SimulationVector,
        event: &BossEvent,
    ) -> Result<Vec<HostileEvent>, HostileError> {
        if !origin.is_finite() {
            return Err(HostileError::NonFiniteOrigin);
        }
        let (boss_cast_id, source_kind, directions, attack) = match event {
            BossEvent::FanFired {
                cast_id,
                locked_aim,
                offsets_degrees,
                attack,
                ..
            } => {
                if *offsets_degrees != [-20, -10, 0, 10, 20] {
                    return Err(HostileError::UnsupportedBossFanOffsets(*offsets_degrees));
                }
                let base = aim_from_vector(*locked_aim)?;
                let directions = offsets_degrees
                    .iter()
                    .map(|offset| rotate_fan_direction(base, *offset))
                    .collect::<Result<Vec<_>, _>>()?;
                (
                    *cast_id,
                    HostileProjectileSourceKind::AimedFan,
                    directions,
                    attack,
                )
            }
            BossEvent::RingFired {
                cast_id,
                emitted_indices,
                attack,
                ..
            } => {
                if emitted_indices.len() != 12
                    || !emitted_indices.windows(2).all(|pair| pair[0] < pair[1])
                    || emitted_indices.iter().any(|index| *index >= 16)
                {
                    return Err(HostileError::InvalidBossRingIndices);
                }
                let directions = emitted_indices
                    .iter()
                    .map(|index| boss_ring_direction(*index))
                    .collect();
                (
                    *cast_id,
                    HostileProjectileSourceKind::GapRing,
                    directions,
                    attack,
                )
            }
            _ => return Err(HostileError::EventDoesNotAuthorizeProjectileSpawn),
        };
        validate_projectile_attack(attack, source_kind)?;
        if directions.len() != usize::from(attack.projectile_count) {
            return Err(HostileError::BossProjectileCountMismatch);
        }
        let cast_id = AttackCastId::from_ordinal(boss_cast_id.get())
            .ok_or(HostileError::InvalidBossCastId)?;
        let mut projectiles = Vec::with_capacity(directions.len());
        for direction in directions {
            projectiles.push(self.allocate_projectile(
                source_entity_id,
                cast_id,
                source_kind,
                origin,
                direction,
                attack,
            )?);
        }
        self.projectiles
            .extend(projectiles.iter().map(|(_, projectile)| projectile.clone()));
        self.projectiles.sort_by_key(HostileProjectile::id);
        Ok(projectiles
            .into_iter()
            .map(|(_, projectile)| HostileEvent::Spawned {
                tick: self.tick,
                projectile,
            })
            .collect())
    }

    fn spawn_inner(
        &mut self,
        source_entity_id: EntityId,
        origin: SimulationVector,
        event: &EnemyEvent,
    ) -> Result<Vec<HostileEvent>, HostileError> {
        if !origin.is_finite() {
            return Err(HostileError::NonFiniteOrigin);
        }
        let mut spawned = Vec::new();
        match event {
            EnemyEvent::FanFired {
                cast_id,
                direction,
                offsets_degrees,
                origin_offset_milli_tiles,
                attack,
            } => {
                validate_projectile_attack(attack, HostileProjectileSourceKind::AimedFan)?;
                if *offsets_degrees != [-15, 0, 15] {
                    return Err(HostileError::UnsupportedFanOffsets(*offsets_degrees));
                }
                let base = aim_from_vector(*direction)?;
                let spawn_position =
                    origin + base.vector() * (milli_to_tiles(*origin_offset_milli_tiles));
                for offset in offsets_degrees {
                    let direction = rotate_fan_direction(base, *offset)?;
                    spawned.push(self.allocate_projectile(
                        source_entity_id,
                        *cast_id,
                        HostileProjectileSourceKind::AimedFan,
                        spawn_position,
                        direction,
                        attack,
                    )?);
                }
            }
            EnemyEvent::RingFired {
                cast_id,
                emitted_indices,
                attack,
                ..
            } => {
                validate_projectile_attack(attack, HostileProjectileSourceKind::GapRing)?;
                if !emitted_indices.windows(2).all(|pair| pair[0] < pair[1])
                    || emitted_indices.iter().any(|&index| index >= 8)
                {
                    return Err(HostileError::InvalidRingIndices(*emitted_indices));
                }
                for index in emitted_indices {
                    spawned.push(self.allocate_projectile(
                        source_entity_id,
                        *cast_id,
                        HostileProjectileSourceKind::GapRing,
                        origin,
                        ring_direction(*index),
                        attack,
                    )?);
                }
            }
            _ => return Err(HostileError::EventDoesNotAuthorizeProjectileSpawn),
        }
        self.projectiles
            .extend(spawned.iter().map(|event| event.1.clone()));
        self.projectiles.sort_by_key(HostileProjectile::id);
        Ok(spawned
            .into_iter()
            .map(|(_, projectile)| HostileEvent::Spawned {
                tick: self.tick,
                projectile,
            })
            .collect())
    }

    fn allocate_projectile(
        &mut self,
        source_entity_id: EntityId,
        cast_id: AttackCastId,
        source_kind: HostileProjectileSourceKind,
        position: SimulationVector,
        direction: AimDirection,
        attack: &ProjectileAttackDefinition,
    ) -> Result<(EntityId, HostileProjectile), HostileError> {
        let id = self
            .projectile_ids
            .allocate()
            .ok_or(HostileError::ProjectileIdOverflow)?;
        let projectile = HostileProjectile {
            id,
            source_entity_id,
            cast_id,
            source_kind,
            pattern_id: attack.pattern_id,
            position,
            direction,
            speed_tiles_per_second: milli_to_tiles(attack.speed_milli_tiles_per_second),
            radius_tiles: milli_to_tiles(attack.radius_milli_tiles),
            remaining_lifetime_ticks: attack.lifetime_ticks,
            raw_damage: attack.raw_damage,
            damage_type: attack.damage_type,
            declared_damage_band: attack.damage_band,
            threat_cost: attack.threat_cost,
            memory_family: attack.memory_family,
            counterplay: attack.counterplay,
            disposition: attack.disposition,
            pierces_players: attack.pierces_players,
            ignored_player_ids: BTreeSet::new(),
        };
        Ok((id, projectile))
    }

    /// Advances one complete hostile tick transactionally in projectile-ID order.
    pub fn step(
        &mut self,
        arena: &ArenaGeometry,
        target: &mut HostileTargetState,
        tonic: &mut RedTonicSimulation,
        combat: &mut PlayerCombatState,
    ) -> Result<HostileStep, HostileError> {
        let mut next_simulation = self.clone();
        let mut next_target = target.clone();
        let mut next_tonic = tonic.clone();
        let mut next_combat = combat.clone();
        let result = next_simulation.step_inner(
            arena,
            &mut next_target,
            &mut next_tonic,
            &mut next_combat,
        )?;
        *self = next_simulation;
        *target = next_target;
        *tonic = next_tonic;
        *combat = next_combat;
        Ok(result)
    }

    fn step_inner(
        &mut self,
        arena: &ArenaGeometry,
        target: &mut HostileTargetState,
        tonic: &mut RedTonicSimulation,
        combat: &mut PlayerCombatState,
    ) -> Result<HostileStep, HostileError> {
        let player_world = ProjectileCollisionWorld::new(
            arena,
            vec![EnemyHurtbox::new(
                target.entity_id,
                target.position,
                PLAYER_HURTBOX_RADIUS_TILES,
            )?],
        )?;
        let solid_world = ProjectileCollisionWorld::new(arena, Vec::new())?;
        self.projectiles.sort_by_key(HostileProjectile::id);
        let mut survivors = Vec::with_capacity(self.projectiles.len());
        let mut events = Vec::with_capacity(self.projectiles.len() * 2);
        let mut transaction = ProjectileTickTransaction {
            tick: self.tick,
            projectile_damage_allowed_at: &mut self.projectile_damage_allowed_at,
            player_world: &player_world,
            solid_world: &solid_world,
            target,
            tonic,
            combat,
            damage_policy: self.damage_policy,
            events: &mut events,
            survivors: &mut survivors,
        };
        for projectile in self.projectiles.drain(..) {
            transaction.resolve(projectile)?;
        }
        self.projectiles = survivors;
        let result = HostileStep {
            tick: self.tick,
            events,
        };
        self.tick = self.tick.checked_next().ok_or(HostileError::TickOverflow)?;
        Ok(result)
    }
}

struct ProjectileTickTransaction<'a> {
    tick: Tick,
    projectile_damage_allowed_at: &'a mut Tick,
    player_world: &'a ProjectileCollisionWorld,
    solid_world: &'a ProjectileCollisionWorld,
    target: &'a mut HostileTargetState,
    tonic: &'a mut RedTonicSimulation,
    combat: &'a mut PlayerCombatState,
    damage_policy: HostileDamagePolicy,
    events: &'a mut Vec<HostileEvent>,
    survivors: &'a mut Vec<HostileProjectile>,
}

impl ProjectileTickTransaction<'_> {
    fn resolve(&mut self, projectile: HostileProjectile) -> Result<(), HostileError> {
        let from = projectile.position;
        let displacement = projectile.direction.vector()
            * (projectile.speed_tiles_per_second / TICKS_PER_SECOND_F32);
        let world = if projectile
            .ignored_player_ids
            .contains(&self.target.entity_id)
        {
            self.solid_world
        } else {
            self.player_world
        };
        let hit = world.sweep_circle(from, displacement, projectile.radius_tiles)?;
        let Some(hit) = hit else {
            retain_after_motion(
                self.tick,
                projectile,
                from,
                from + displacement,
                self.events,
                self.survivors,
            );
            return Ok(());
        };
        match hit.target {
            CollisionTarget::Solid(solid) => {
                self.push_solid_contact(&projectile, solid, from + displacement * hit.fraction);
                Ok(())
            }
            CollisionTarget::Enemy(player) => {
                self.resolve_player_contact(projectile, player, from, displacement, hit)
            }
        }
    }

    fn resolve_player_contact(
        &mut self,
        mut projectile: HostileProjectile,
        player_entity_id: EntityId,
        from: SimulationVector,
        displacement: SimulationVector,
        hit: SweepHit,
    ) -> Result<(), HostileError> {
        debug_assert_eq!(player_entity_id, self.target.entity_id);
        let contact_position = from + displacement * hit.fraction;
        let player_alive = self.tonic.vitals().current_health() > 0;
        if player_alive && self.tick < *self.projectile_damage_allowed_at {
            self.events.push(HostileEvent::ProjectileGraceIgnored {
                tick: self.tick,
                projectile_id: projectile.id,
                source_entity_id: projectile.source_entity_id,
                pattern_id: projectile.pattern_id,
                player_entity_id,
                pierces_players: projectile.pierces_players,
                consumed: !projectile.pierces_players,
            });
            if !projectile.pierces_players {
                return Ok(());
            }
        } else {
            self.apply_or_record_player_contact(&projectile, player_entity_id, contact_position)?;
            if !projectile.pierces_players {
                return Ok(());
            }
        }
        projectile.ignored_player_ids.insert(player_entity_id);
        self.continue_piercing(
            projectile,
            from,
            displacement,
            hit.fraction,
            contact_position,
        )
    }

    fn apply_or_record_player_contact(
        &mut self,
        projectile: &HostileProjectile,
        player_entity_id: EntityId,
        position: SimulationVector,
    ) -> Result<(), HostileError> {
        let applied = if self.tonic.vitals().current_health() > 0 {
            let result = apply_hostile_contact_transaction_with_policy(
                projectile.source_entity_id,
                projectile.raw_damage,
                projectile.damage_type,
                self.target,
                self.tonic,
                self.combat,
                self.damage_policy,
            )?;
            *self.projectile_damage_allowed_at = Tick(
                self.tick
                    .0
                    .checked_add(HOSTILE_PROJECTILE_GRACE_TICKS)
                    .ok_or(HostileError::TickOverflow)?,
            );
            Some(result)
        } else {
            None
        };
        self.events.push(HostileEvent::Contact {
            tick: self.tick,
            projectile_id: projectile.id,
            source_entity_id: projectile.source_entity_id,
            pattern_id: projectile.pattern_id,
            cast_id: projectile.cast_id,
            target: HostileCollisionTarget::Player(player_entity_id),
            position,
            declared_damage_band: projectile.declared_damage_band,
            damage: applied.as_ref().map(|result| result.damage.clone()),
            health_application: applied.as_ref().map(|result| result.health_application),
            debug_invulnerable: applied
                .as_ref()
                .is_some_and(|result| result.debug_invulnerable),
            focused_transition: applied.and_then(|result| result.focused_transition),
        });
        Ok(())
    }

    fn continue_piercing(
        &mut self,
        projectile: HostileProjectile,
        from: SimulationVector,
        displacement: SimulationVector,
        hit_fraction: f32,
        contact_position: SimulationVector,
    ) -> Result<(), HostileError> {
        let remaining_displacement = displacement * (1.0 - hit_fraction);
        if let Some(solid_hit) = self.solid_world.sweep_circle(
            contact_position,
            remaining_displacement,
            projectile.radius_tiles,
        )? {
            let CollisionTarget::Solid(solid) = solid_hit.target else {
                unreachable!("solid-only collision world returned a player")
            };
            self.push_solid_contact(
                &projectile,
                solid,
                contact_position + remaining_displacement * solid_hit.fraction,
            );
            return Ok(());
        }
        retain_after_motion(
            self.tick,
            projectile,
            contact_position,
            from + displacement,
            self.events,
            self.survivors,
        );
        Ok(())
    }

    fn push_solid_contact(
        &mut self,
        projectile: &HostileProjectile,
        solid: SolidColliderId,
        position: SimulationVector,
    ) {
        self.events.push(HostileEvent::Contact {
            tick: self.tick,
            projectile_id: projectile.id,
            source_entity_id: projectile.source_entity_id,
            pattern_id: projectile.pattern_id,
            cast_id: projectile.cast_id,
            target: HostileCollisionTarget::Solid(solid),
            position,
            declared_damage_band: projectile.declared_damage_band,
            damage: None,
            health_application: None,
            focused_transition: None,
            debug_invulnerable: false,
        });
    }
}

fn retain_after_motion(
    tick: Tick,
    mut projectile: HostileProjectile,
    from: SimulationVector,
    to: SimulationVector,
    events: &mut Vec<HostileEvent>,
    survivors: &mut Vec<HostileProjectile>,
) {
    projectile.position = to;
    events.push(HostileEvent::Moved {
        tick,
        projectile_id: projectile.id,
        from,
        to,
    });
    if projectile.remaining_lifetime_ticks == 1 {
        events.push(HostileEvent::Expired {
            tick,
            projectile_id: projectile.id,
            final_position: to,
        });
    } else {
        projectile.remaining_lifetime_ticks -= 1;
        survivors.push(projectile);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedHostileDamage {
    pub damage: DamageEvent,
    pub health_application: DamageAppliedEvent,
    pub focused_transition: Option<FocusedTransition>,
    pub debug_invulnerable: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum HostileDamagePolicy {
    #[default]
    Standard,
    DebugInvulnerable,
}

/// Resolves and commits collision damage and the Stillness break as one authoritative transaction.
/// Callers must execute this before sampling any later same-tick player action.
pub fn apply_hostile_contact_transaction(
    source_entity_id: EntityId,
    raw_damage: u32,
    damage_type: DamageType,
    target: &mut HostileTargetState,
    tonic: &mut RedTonicSimulation,
    combat: &mut PlayerCombatState,
) -> Result<AppliedHostileDamage, HostileError> {
    apply_hostile_contact_transaction_with_policy(
        source_entity_id,
        raw_damage,
        damage_type,
        target,
        tonic,
        combat,
        HostileDamagePolicy::Standard,
    )
}

/// Resolves collision damage identically while allowing an explicit developer-only rejection at
/// the final mutation boundary. Debug rejection preserves collision and hypothetical damage data,
/// but commits no barrier, health, restore, status, or Focused mutation.
pub fn apply_hostile_contact_transaction_with_policy(
    source_entity_id: EntityId,
    raw_damage: u32,
    damage_type: DamageType,
    target: &mut HostileTargetState,
    tonic: &mut RedTonicSimulation,
    combat: &mut PlayerCombatState,
    policy: HostileDamagePolicy,
) -> Result<AppliedHostileDamage, HostileError> {
    let mut next_target = target.clone();
    let mut next_tonic = tonic.clone();
    let mut next_combat = combat.clone();
    let vitals = next_tonic.vitals();
    let mut reductions = next_target
        .additional_direct_damage_reductions_basis_points
        .clone();
    let movement_reduction = next_combat.direct_damage_reduction_basis_points();
    if movement_reduction > 0 {
        reductions.push(movement_reduction);
    }
    let request = DirectHitRequest::new(DirectHitParameters {
        source: source_entity_id,
        target: next_target.entity_id,
        collision_confirmed: true,
        target_is_immune: next_target.target_is_immune,
        raw_damage,
        damage_type,
        attacker_multiplier_basis_points: 10_000,
        target_resistance_basis_points: next_target.resistance_basis_points,
        direct_damage_reductions_basis_points: reductions,
        armor: next_target.armor,
        current_barrier: next_target.current_barrier,
        health_damage_cap_basis_points: next_target.health_damage_cap_basis_points,
        current_health: vitals.current_health(),
        max_health: vitals.maximum_health(),
    })?;
    let damage = resolve_direct_hit(&request)?;
    if policy == HostileDamagePolicy::DebugInvulnerable {
        return Ok(AppliedHostileDamage {
            health_application: DamageAppliedEvent {
                tick: tonic.tick(),
                requested: damage.health_damage_applied,
                applied: 0,
                health_before: vitals.current_health(),
                health_after: vitals.current_health(),
                restore_continues: tonic.active_restore_remaining_ticks() > 0,
            },
            damage,
            focused_transition: None,
            debug_invulnerable: true,
        });
    }
    let health_application = next_tonic.apply_damage(damage.health_damage_applied);
    if health_application.applied != damage.health_damage_applied
        || health_application.health_after != damage.health_after
    {
        return Err(HostileError::HealthCommitMismatch);
    }
    next_target.current_barrier = damage.barrier_after;
    let focused_transition = next_combat.break_focused_from_damage();
    *target = next_target;
    *tonic = next_tonic;
    *combat = next_combat;
    Ok(AppliedHostileDamage {
        damage,
        health_application,
        focused_transition,
        debug_invulnerable: false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneGeometry {
    pub origin: SimulationVector,
    pub axes_degrees: [u16; 2],
    pub width_tiles: f32,
    pub extends_to_arena_collision: bool,
}

impl LaneGeometry {
    pub fn from_activation(
        event: &EnemyEvent,
    ) -> Result<(AttackCastId, Self, LaneAttackDefinition), HostileError> {
        let EnemyEvent::LanesActivated {
            cast_id,
            axes_degrees,
            attack,
            ..
        } = event
        else {
            return Err(HostileError::EventDoesNotAuthorizeLaneContact);
        };
        if *axes_degrees != [0, 90] && *axes_degrees != [45, 135] {
            return Err(HostileError::UnsupportedLaneAxes(*axes_degrees));
        }
        validate_lane_attack(attack)?;
        Ok((
            *cast_id,
            Self {
                origin: SimulationVector::default(),
                axes_degrees: *axes_degrees,
                width_tiles: milli_to_tiles(attack.width_milli_tiles),
                extends_to_arena_collision: true,
            },
            attack.clone(),
        ))
    }

    pub fn from_boss_activation(
        event: &BossEvent,
    ) -> Result<(AttackCastId, Self, LaneAttackDefinition, Tick), HostileError> {
        let BossEvent::CrossActivated {
            cast_id,
            axes_degrees,
            active_until,
            attack,
            ..
        } = event
        else {
            return Err(HostileError::EventDoesNotAuthorizeLaneContact);
        };
        validate_lane_attack(attack)?;
        let cast_id =
            AttackCastId::from_ordinal(cast_id.get()).ok_or(HostileError::InvalidBossCastId)?;
        Ok((
            cast_id,
            Self {
                origin: SimulationVector::default(),
                axes_degrees: *axes_degrees,
                width_tiles: milli_to_tiles(attack.width_milli_tiles),
                extends_to_arena_collision: true,
            },
            attack.clone(),
            *active_until,
        ))
    }

    #[must_use]
    pub fn with_origin(mut self, origin: SimulationVector) -> Self {
        self.origin = origin;
        self
    }

    #[must_use]
    pub fn contacts_player(self, player_center: SimulationVector) -> bool {
        if !self.origin.is_finite() || !player_center.is_finite() || self.width_tiles <= 0.0 {
            return false;
        }
        let delta = player_center - self.origin;
        let half_width = self.width_tiles * 0.5 + PLAYER_HURTBOX_RADIUS_TILES;
        self.axes_degrees.iter().any(|axis| {
            let perpendicular_distance = match *axis {
                0 => delta.y.abs(),
                90 => delta.x.abs(),
                45 => (delta.y - delta.x).abs() * std::f32::consts::FRAC_1_SQRT_2,
                135 => (delta.y + delta.x).abs() * std::f32::consts::FRAC_1_SQRT_2,
                _ => f32::INFINITY,
            };
            perpendicular_distance <= half_width
        })
    }
}

pub fn resolve_lane_contact(
    source_entity_id: EntityId,
    attack: &LaneAttackDefinition,
    geometry: LaneGeometry,
    target: &mut HostileTargetState,
    tonic: &mut RedTonicSimulation,
    combat: &mut PlayerCombatState,
) -> Result<Option<AppliedHostileDamage>, HostileError> {
    resolve_lane_contact_with_policy(
        source_entity_id,
        attack,
        geometry,
        target,
        tonic,
        combat,
        HostileDamagePolicy::Standard,
    )
}

pub fn resolve_lane_contact_with_policy(
    source_entity_id: EntityId,
    attack: &LaneAttackDefinition,
    geometry: LaneGeometry,
    target: &mut HostileTargetState,
    tonic: &mut RedTonicSimulation,
    combat: &mut PlayerCombatState,
    policy: HostileDamagePolicy,
) -> Result<Option<AppliedHostileDamage>, HostileError> {
    validate_lane_attack(attack)?;
    if (geometry.axes_degrees != [0, 90] && geometry.axes_degrees != [45, 135])
        || (geometry.width_tiles - milli_to_tiles(attack.width_milli_tiles)).abs() > f32::EPSILON
        || !geometry.extends_to_arena_collision
    {
        return Err(HostileError::InvalidLaneGeometry);
    }
    if !geometry.contacts_player(target.position) {
        return Ok(None);
    }
    apply_hostile_contact_transaction_with_policy(
        source_entity_id,
        attack.raw_damage,
        attack.damage_type,
        target,
        tonic,
        combat,
        policy,
    )
    .map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyActorKind {
    DrownedPilgrim,
    BellReed,
    ChainSentry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyActor {
    entity_id: EntityId,
    kind: EnemyActorKind,
    x_milli_tiles: i32,
    y_milli_tiles: i32,
    hurtbox_radius_milli_tiles: u32,
    x_remainder_micro: i64,
    y_remainder_micro: i64,
}

impl EnemyActor {
    pub fn new(
        entity_id: EntityId,
        kind: EnemyActorKind,
        x_milli_tiles: i32,
        y_milli_tiles: i32,
        hurtbox_radius_milli_tiles: u32,
    ) -> Result<Self, HostileError> {
        if hurtbox_radius_milli_tiles == 0 {
            return Err(HostileError::InvalidActorRadius);
        }
        Ok(Self {
            entity_id,
            kind,
            x_milli_tiles,
            y_milli_tiles,
            hurtbox_radius_milli_tiles,
            x_remainder_micro: 0,
            y_remainder_micro: 0,
        })
    }

    #[must_use]
    pub const fn entity_id(&self) -> EntityId {
        self.entity_id
    }

    #[must_use]
    pub const fn kind(&self) -> EnemyActorKind {
        self.kind
    }

    #[must_use]
    pub const fn position_milli_tiles(&self) -> (i32, i32) {
        (self.x_milli_tiles, self.y_milli_tiles)
    }

    #[must_use]
    pub fn position(&self) -> SimulationVector {
        SimulationVector::new(
            signed_milli_to_tiles(self.x_milli_tiles),
            signed_milli_to_tiles(self.y_milli_tiles),
        )
    }

    /// Builds the authoritative Pilgrim AI input from simulation positions.
    pub fn target_input(
        &self,
        player_position: SimulationVector,
    ) -> Result<PilgrimTargetInput, HostileError> {
        if self.kind != EnemyActorKind::DrownedPilgrim {
            return Err(HostileError::FixedEnemyReceivedTargetInput);
        }
        let player_x = tiles_to_milli_i32(player_position.x)?;
        let player_y = tiles_to_milli_i32(player_position.y)?;
        let delta_x = player_x
            .checked_sub(self.x_milli_tiles)
            .ok_or(HostileError::ActorArithmeticOverflow)?;
        let delta_y = player_y
            .checked_sub(self.y_milli_tiles)
            .ok_or(HostileError::ActorArithmeticOverflow)?;
        let dx = i64::from(delta_x);
        let dy = i64::from(delta_y);
        let distance_squared = squared_length(dx, dy)?;
        let distance = u32::try_from(integer_sqrt(
            u64::try_from(distance_squared).map_err(|_| HostileError::ActorArithmeticOverflow)?,
        ))
        .map_err(|_| HostileError::ActorArithmeticOverflow)?;
        Ok(PilgrimTargetInput {
            present: true,
            distance_milli_tiles: distance,
            delta: AimVector {
                x: delta_x,
                y: delta_y,
            },
        })
    }

    /// Applies one authoritative Pilgrim `ApproachIntent`; fixed enemy kinds reject movement.
    pub fn apply_event(
        &mut self,
        arena: &ArenaGeometry,
        event: &EnemyEvent,
    ) -> Result<Option<EnemyActorMovement>, HostileError> {
        let EnemyEvent::ApproachIntent {
            speed_milli_tiles_per_second,
            target_delta,
            stop_distance_milli_tiles,
        } = event
        else {
            return Ok(None);
        };
        if self.kind != EnemyActorKind::DrownedPilgrim {
            return Err(HostileError::FixedEnemyReceivedMovement);
        }
        if *speed_milli_tiles_per_second != 2_200 || *stop_distance_milli_tiles != 5_000 {
            return Err(HostileError::InvalidPilgrimMovementIntent);
        }
        let before = self.position();
        let Some((move_x, move_y)) = self.planned_approach_delta(
            *target_delta,
            *speed_milli_tiles_per_second,
            *stop_distance_milli_tiles,
        )?
        else {
            return Ok(Some(EnemyActorMovement {
                entity_id: self.entity_id,
                from: before,
                to: before,
                blocked_by: None,
            }));
        };
        self.commit_actor_sweep(arena, before, move_x, move_y)
            .map(Some)
    }

    fn planned_approach_delta(
        &mut self,
        target_delta: AimVector,
        speed_milli_tiles_per_second: u32,
        stop_distance_milli_tiles: u32,
    ) -> Result<Option<(i64, i64)>, HostileError> {
        let (dx, dy) = (i64::from(target_delta.x), i64::from(target_delta.y));
        let distance_squared = squared_length(dx, dy)?;
        if distance_squared == 0 {
            return Ok(None);
        }
        let distance = i64::try_from(integer_sqrt(
            u64::try_from(distance_squared).map_err(|_| HostileError::ActorArithmeticOverflow)?,
        ))
        .map_err(|_| HostileError::ActorArithmeticOverflow)?;
        if distance <= i64::from(stop_distance_milli_tiles) {
            return Ok(None);
        }
        let velocity_scale = i64::from(speed_milli_tiles_per_second)
            .checked_mul(MICRO_UNITS)
            .ok_or(HostileError::ActorArithmeticOverflow)?;
        self.x_remainder_micro = self
            .x_remainder_micro
            .checked_add(
                dx.checked_mul(velocity_scale)
                    .ok_or(HostileError::ActorArithmeticOverflow)?
                    / distance,
            )
            .ok_or(HostileError::ActorArithmeticOverflow)?;
        self.y_remainder_micro = self
            .y_remainder_micro
            .checked_add(
                dy.checked_mul(velocity_scale)
                    .ok_or(HostileError::ActorArithmeticOverflow)?
                    / distance,
            )
            .ok_or(HostileError::ActorArithmeticOverflow)?;
        let tick_denominator = 30 * MICRO_UNITS;
        let mut move_x = self.x_remainder_micro / tick_denominator;
        let mut move_y = self.y_remainder_micro / tick_denominator;
        self.x_remainder_micro %= tick_denominator;
        self.y_remainder_micro %= tick_denominator;
        let remaining = distance - i64::from(stop_distance_milli_tiles);
        let planned_squared = squared_length(move_x, move_y)?;
        let planned_length = i64::try_from(integer_sqrt(
            u64::try_from(planned_squared).map_err(|_| HostileError::ActorArithmeticOverflow)?,
        ))
        .map_err(|_| HostileError::ActorArithmeticOverflow)?;
        if planned_length > remaining && planned_length > 0 {
            move_x = move_x * remaining / planned_length;
            move_y = move_y * remaining / planned_length;
        }
        Ok(Some((move_x, move_y)))
    }

    fn commit_actor_sweep(
        &mut self,
        arena: &ArenaGeometry,
        before: SimulationVector,
        move_x: i64,
        move_y: i64,
    ) -> Result<EnemyActorMovement, HostileError> {
        let displacement = SimulationVector::new(
            signed_i64_milli_to_tiles(move_x)?,
            signed_i64_milli_to_tiles(move_y)?,
        );
        let world = ProjectileCollisionWorld::new(arena, Vec::new())?;
        let hit = world.sweep_solids(
            before,
            displacement,
            milli_to_tiles(self.hurtbox_radius_milli_tiles),
        )?;
        let fraction = hit.map_or(1.0, |contact| contact.fraction);
        let applied_x = scale_milli_by_fraction(move_x, fraction)?;
        let applied_y = scale_milli_by_fraction(move_y, fraction)?;
        self.x_milli_tiles = i32::try_from(i64::from(self.x_milli_tiles) + applied_x)
            .map_err(|_| HostileError::ActorArithmeticOverflow)?;
        self.y_milli_tiles = i32::try_from(i64::from(self.y_milli_tiles) + applied_y)
            .map_err(|_| HostileError::ActorArithmeticOverflow)?;
        if hit.is_some() {
            self.x_remainder_micro = 0;
            self.y_remainder_micro = 0;
        }
        Ok(EnemyActorMovement {
            entity_id: self.entity_id,
            from: before,
            to: self.position(),
            blocked_by: hit.and_then(|contact| match contact.target {
                CollisionTarget::Solid(solid) => Some(solid),
                CollisionTarget::Enemy(_) => None,
            }),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemyActorMovement {
    pub entity_id: EntityId,
    pub from: SimulationVector,
    pub to: SimulationVector,
    pub blocked_by: Option<SolidColliderId>,
}

fn validate_projectile_attack(
    attack: &ProjectileAttackDefinition,
    source_kind: HostileProjectileSourceKind,
) -> Result<(), HostileError> {
    let exact = match source_kind {
        HostileProjectileSourceKind::AimedFan => {
            (match attack.pattern_id {
                "pattern.enemy.drowned_pilgrim.fan" => {
                    attack.projectile_count == 3
                        && attack.speed_milli_tiles_per_second == 5_500
                        && attack.radius_milli_tiles == 120
                        && attack.lifetime_ticks == 66
                        && attack.raw_damage == 8
                        && attack.damage_type == DamageType::Physical
                        && attack.damage_band == DamageBand::Chip
                        && attack.threat_cost == 3
                        && attack.maximum_active_instances == 6
                }
                crate::BELL_PROCTOR_FAN_ID => {
                    attack.projectile_count == 5
                        && attack.speed_milli_tiles_per_second == 6_000
                        && attack.radius_milli_tiles == 120
                        && attack.lifetime_ticks == 90
                        && attack.raw_damage == 12
                        && attack.damage_type == DamageType::Veil
                        && attack.damage_band == DamageBand::Chip
                        && attack.threat_cost == 5
                        && attack.maximum_active_instances == 10
                }
                _ => false,
            }) && attack.memory_family == EchoMemoryFamily::FanProjectile
                && attack.counterplay == Counterplay::Strafe
                && !attack.pierces_players
        }
        HostileProjectileSourceKind::GapRing => {
            (match attack.pattern_id {
                "pattern.enemy.bell_reed.gap_ring" => {
                    attack.projectile_count == 6
                        && attack.lifetime_ticks == 90
                        && attack.raw_damage == 10
                        && attack.damage_band == DamageBand::Chip
                        && attack.threat_cost == 6
                        && attack.maximum_active_instances == 12
                }
                crate::BELL_PROCTOR_RING_ID => {
                    attack.projectile_count == 12
                        && attack.lifetime_ticks == 120
                        && attack.raw_damage == 15
                        && attack.damage_band == DamageBand::Pressure
                        && attack.threat_cost == 12
                        && attack.maximum_active_instances == 24
                }
                _ => false,
            }) && attack.speed_milli_tiles_per_second == 4_500
                && attack.radius_milli_tiles == 130
                && attack.damage_type == DamageType::Veil
                && attack.memory_family == EchoMemoryFamily::RadialProjectile
                && attack.counterplay == Counterplay::FollowGap
                && !attack.pierces_players
        }
    };
    if !exact || attack.disposition != HostileDisposition::ConsumeOnPlayerOrSolid {
        return Err(HostileError::InvalidProjectileAttack);
    }
    Ok(())
}

fn validate_lane_attack(attack: &LaneAttackDefinition) -> Result<(), HostileError> {
    let exact_payload = match attack.pattern_id {
        "pattern.enemy.chain_sentry.cross_lanes" => {
            attack.width_milli_tiles == 900
                && attack.active_ticks == 11
                && attack.raw_damage == 22
                && attack.damage_band == DamageBand::Pressure
        }
        crate::BELL_PROCTOR_CROSS_ID => {
            attack.width_milli_tiles == 1_000
                && attack.active_ticks == 15
                && attack.raw_damage == 28
                && attack.damage_band == DamageBand::Major
        }
        _ => false,
    };
    if !exact_payload
        || attack.lane_count != 2
        || attack.damage_type != DamageType::Physical
        || attack.threat_cost_per_lane != 12
        || attack.memory_family != EchoMemoryFamily::LaneOrBeam
        || attack.counterplay != Counterplay::LeaveTelegraph
        || attack.disposition != HostileDisposition::ExpireAtAuthoredEnd
        || attack.maximum_active_instances != 2
    {
        return Err(HostileError::InvalidLaneAttack);
    }
    Ok(())
}

fn aim_from_vector(vector: AimVector) -> Result<AimDirection, HostileError> {
    AimDirection::new(SimulationVector::new(
        signed_component_to_f32(vector.x),
        signed_component_to_f32(vector.y),
    ))
    .map_err(HostileError::Aim)
}

fn rotate_fan_direction(base: AimDirection, offset: i16) -> Result<AimDirection, HostileError> {
    const COS_10: f32 = 0.984_807_7;
    const SIN_10: f32 = 0.173_648_18;
    const COS_15: f32 = 0.965_925_8;
    const SIN_15: f32 = 0.258_819_04;
    const COS_20: f32 = 0.939_692_6;
    const SIN_20: f32 = 0.342_020_15;
    let vector = base.vector();
    let rotate = |cosine: f32, sine: f32| {
        SimulationVector::new(
            vector.x * cosine - vector.y * sine,
            vector.x * sine + vector.y * cosine,
        )
    };
    let rotated = match offset {
        -20 => rotate(COS_20, -SIN_20),
        -15 => rotate(COS_15, -SIN_15),
        -10 => rotate(COS_10, -SIN_10),
        0 => vector,
        10 => rotate(COS_10, SIN_10),
        15 => rotate(COS_15, SIN_15),
        20 => rotate(COS_20, SIN_20),
        _ => return Err(HostileError::UnsupportedFanOffset(offset)),
    };
    AimDirection::new(rotated).map_err(HostileError::Aim)
}

fn boss_ring_direction(index: u8) -> AimDirection {
    const COS_22_5: f32 = 0.923_879_5;
    const SIN_22_5: f32 = 0.382_683_43;
    const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let vector = match index {
        0 => SimulationVector::new(1.0, 0.0),
        1 => SimulationVector::new(COS_22_5, SIN_22_5),
        2 => SimulationVector::new(D, D),
        3 => SimulationVector::new(SIN_22_5, COS_22_5),
        4 => SimulationVector::new(0.0, 1.0),
        5 => SimulationVector::new(-SIN_22_5, COS_22_5),
        6 => SimulationVector::new(-D, D),
        7 => SimulationVector::new(-COS_22_5, SIN_22_5),
        8 => SimulationVector::new(-1.0, 0.0),
        9 => SimulationVector::new(-COS_22_5, -SIN_22_5),
        10 => SimulationVector::new(-D, -D),
        11 => SimulationVector::new(-SIN_22_5, -COS_22_5),
        12 => SimulationVector::new(0.0, -1.0),
        13 => SimulationVector::new(SIN_22_5, -COS_22_5),
        14 => SimulationVector::new(D, -D),
        15 => SimulationVector::new(COS_22_5, -SIN_22_5),
        _ => unreachable!("validated Bell ring index"),
    };
    AimDirection::new(vector).expect("Bell ring table contains finite unit vectors")
}

fn ring_direction(index: u8) -> AimDirection {
    const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let vector = match index {
        0 => SimulationVector::new(1.0, 0.0),
        1 => SimulationVector::new(D, D),
        2 => SimulationVector::new(0.0, 1.0),
        3 => SimulationVector::new(-D, D),
        4 => SimulationVector::new(-1.0, 0.0),
        5 => SimulationVector::new(-D, -D),
        6 => SimulationVector::new(0.0, -1.0),
        7 => SimulationVector::new(D, -D),
        _ => unreachable!("validated ring index"),
    };
    AimDirection::new(vector).expect("ring table contains nonzero finite unit vectors")
}

#[allow(clippy::cast_precision_loss)]
fn milli_to_tiles(value: u32) -> f32 {
    value as f32 / 1_000.0
}

#[allow(clippy::cast_precision_loss)]
fn signed_milli_to_tiles(value: i32) -> f32 {
    value as f32 / 1_000.0
}

fn signed_i64_milli_to_tiles(value: i64) -> Result<f32, HostileError> {
    i32::try_from(value)
        .map(signed_milli_to_tiles)
        .map_err(|_| HostileError::ActorArithmeticOverflow)
}

#[allow(clippy::cast_precision_loss)]
fn signed_component_to_f32(value: i32) -> f32 {
    value as f32
}

fn squared_length(x: i64, y: i64) -> Result<i64, HostileError> {
    x.checked_mul(x)
        .and_then(|x_squared| {
            y.checked_mul(y)
                .and_then(|y_squared| x_squared.checked_add(y_squared))
        })
        .ok_or(HostileError::ActorArithmeticOverflow)
}

#[allow(clippy::cast_possible_truncation)]
fn tiles_to_milli_i32(value: f32) -> Result<i32, HostileError> {
    let scaled = value * 1_000.0;
    #[allow(clippy::cast_precision_loss)]
    if !scaled.is_finite() || scaled < i32::MIN as f32 || scaled > i32::MAX as f32 {
        return Err(HostileError::NonFiniteOrOutOfRangePlayerPosition);
    }
    Ok(scaled.round() as i32)
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn scale_milli_by_fraction(value: i64, fraction: f32) -> Result<i64, HostileError> {
    let scaled = value as f32 * fraction;
    #[allow(clippy::cast_precision_loss)]
    if !scaled.is_finite() || scaled < i64::MIN as f32 || scaled > i64::MAX as f32 {
        return Err(HostileError::ActorArithmeticOverflow);
    }
    Ok(scaled.round() as i64)
}

fn integer_sqrt(value: u64) -> u64 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut next = x.midpoint(value / x);
    while next < x {
        x = next;
        next = x.midpoint(value / x);
    }
    x
}

#[derive(Debug, Error)]
pub enum HostileError {
    #[error("enemy event does not authorize a hostile projectile spawn")]
    EventDoesNotAuthorizeProjectileSpawn,
    #[error("enemy event does not authorize an active lane contact")]
    EventDoesNotAuthorizeLaneContact,
    #[error("hostile projectile origin must be finite")]
    NonFiniteOrigin,
    #[error("hostile projectile ID space exhausted")]
    ProjectileIdOverflow,
    #[error("hostile tick overflow")]
    TickOverflow,
    #[error("hostile attack definition is invalid")]
    InvalidProjectileAttack,
    #[error("hostile lane definition differs from CONT-FP-004")]
    InvalidLaneAttack,
    #[error("hostile lane geometry differs from its exact active cast")]
    InvalidLaneGeometry,
    #[error("unsupported fan offsets {0:?}")]
    UnsupportedFanOffsets([i16; 3]),
    #[error("unsupported fan offset {0}")]
    UnsupportedFanOffset(i16),
    #[error("unsupported Bell Proctor fan offsets {0:?}")]
    UnsupportedBossFanOffsets([i16; 5]),
    #[error("Bell Proctor ring indices must be twelve sorted unique values below sixteen")]
    InvalidBossRingIndices,
    #[error("Bell Proctor projectile count differs from its authored directions")]
    BossProjectileCountMismatch,
    #[error("Bell Proctor cast ID must be nonzero")]
    InvalidBossCastId,
    #[error("ring indices must be sorted, unique, and below eight: {0:?}")]
    InvalidRingIndices([u8; 6]),
    #[error("unsupported lane axes {0:?}")]
    UnsupportedLaneAxes([u16; 2]),
    #[error("health commit diverged from canonical damage result")]
    HealthCommitMismatch,
    #[error("enemy actor hurtbox radius must be positive")]
    InvalidActorRadius,
    #[error("fixed enemy received a movement event")]
    FixedEnemyReceivedMovement,
    #[error("fixed enemy does not produce Drowned Pilgrim target input")]
    FixedEnemyReceivedTargetInput,
    #[error("player position is non-finite or exceeds fixed-point actor range")]
    NonFiniteOrOutOfRangePlayerPosition,
    #[error("Pilgrim movement intent differs from CONT-FP-004")]
    InvalidPilgrimMovementIntent,
    #[error("enemy actor fixed-point arithmetic overflowed")]
    ActorArithmeticOverflow,
    #[error(transparent)]
    Aim(#[from] AimDirectionError),
    #[error(transparent)]
    Collision(#[from] CollisionError),
    #[error(transparent)]
    Hurtbox(#[from] HurtboxError),
    #[error(transparent)]
    Damage(#[from] DamageError),
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;
    use crate::{
        AbilityDefinitionError, ArenaAnchor, GraveMarkDefinition, GraveMarkDefinitionParameters,
        PlayerMovementState, PlayerVitals, SlipstepDefinition, SlipstepDefinitionParameters,
        StillnessDefinition, StillnessDefinitionParameters, TilePoint, TileRectangle, TonicBelt,
        WeaponDefinition, WeaponDefinitionParameters,
    };

    fn id(value: u64) -> EntityId {
        EntityId::new(value).expect("nonzero ID")
    }

    fn arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.prototype.bell_laboratory_01".to_owned(),
            width_milli_tiles: 32_000,
            height_milli_tiles: 24_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(4_000, 12_000),
            boss_spawn: TilePoint::new(24_000, 12_000),
            pillars: vec![TileRectangle::new(10_000, 5_000, 2_000, 3_000)],
            anchors: vec![ArenaAnchor {
                id: "C".to_owned(),
                point: TilePoint::new(16_000, 12_000),
            }],
        }
        .validated()
        .expect("arena")
    }

    #[allow(clippy::too_many_arguments)]
    fn projectile_attack(
        pattern_id: &'static str,
        count: u8,
        speed: u32,
        radius: u32,
        lifetime: u32,
        raw_damage: u32,
        damage_type: DamageType,
        band: DamageBand,
    ) -> ProjectileAttackDefinition {
        ProjectileAttackDefinition {
            pattern_id,
            projectile_count: count,
            speed_milli_tiles_per_second: speed,
            radius_milli_tiles: radius,
            lifetime_ticks: lifetime,
            raw_damage,
            damage_type,
            damage_band: band,
            threat_cost: 3,
            memory_family: EchoMemoryFamily::FanProjectile,
            counterplay: Counterplay::Strafe,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            maximum_active_instances: if count == 3 { 6 } else { 12 },
        }
    }

    fn fan_event() -> EnemyEvent {
        EnemyEvent::FanFired {
            cast_id: AttackCastId::FIRST,
            direction: AimVector::EAST,
            offsets_degrees: [-15, 0, 15],
            origin_offset_milli_tiles: 450,
            attack: projectile_attack(
                "pattern.enemy.drowned_pilgrim.fan",
                3,
                5_500,
                120,
                66,
                8,
                DamageType::Physical,
                DamageBand::Chip,
            ),
        }
    }

    fn contact_projectile(projectile_id: u64, pierces_players: bool) -> HostileProjectile {
        HostileProjectile {
            id: id(projectile_id),
            source_entity_id: id(20),
            cast_id: AttackCastId::FIRST,
            source_kind: HostileProjectileSourceKind::AimedFan,
            pattern_id: "pattern.test.hostile_contact",
            position: SimulationVector::new(5.0, 6.0),
            direction: AimDirection::east(),
            speed_tiles_per_second: 30.0,
            radius_tiles: 0.12,
            remaining_lifetime_ticks: 8,
            raw_damage: 8,
            damage_type: DamageType::Physical,
            declared_damage_band: DamageBand::Chip,
            threat_cost: 3,
            memory_family: EchoMemoryFamily::FanProjectile,
            counterplay: Counterplay::Strafe,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players,
            ignored_player_ids: BTreeSet::new(),
        }
    }

    fn combat() -> Result<PlayerCombatState, AbilityDefinitionError> {
        let weapon = WeaponDefinition::new(WeaponDefinitionParameters {
            content_id: "item.prototype.weapon.pine_crossbow".to_owned(),
            raw_damage: 20,
            attack_interval_ticks: 14,
            range_milli_tiles: 9_500,
            projectile_speed_milli_tiles_per_second: 12_000,
            projectile_radius_milli_tiles: 100,
            projectile_count: 1,
            projectile_directions_millionths: vec![(1_000_000, 0)],
            max_projectiles_per_target: 1,
            pierce: 0,
            stops_on_first_enemy: true,
        })
        .expect("weapon");
        let mark = GraveMarkDefinition::new(GraveMarkDefinitionParameters {
            content_id: "ability.arbalist.grave_mark".to_owned(),
            cooldown_ticks: 150,
            global_cooldown_ticks: 5,
            input_buffer_ticks: 3,
            projectile_speed_milli_tiles_per_second: 12_000,
            range_milli_tiles: 11_000,
            projectile_radius_milli_tiles: 120,
            weapon_damage_multiplier_basis_points: 18_000,
            duration_ticks: 120,
            marked_primary_bonus_basis_points: 1_500,
            maximum_marked_targets: 1,
            consumes_on_solid: true,
        })?;
        let slip = SlipstepDefinition::new(SlipstepDefinitionParameters {
            content_id: "ability.arbalist.slipstep".to_owned(),
            cooldown_ticks: 240,
            global_cooldown_ticks: 5,
            input_buffer_ticks: 3,
            travel_milli_tiles: 2_000,
            travel_ticks: 5,
            direct_damage_reduction_basis_points: 2_500,
            empowered_window_ticks: 45,
            projectile_speed_bonus_basis_points: 3_000,
            pierce_bonus: 1,
            exhaustion_ticks: 45,
        })?;
        let stillness = StillnessDefinition::new(StillnessDefinitionParameters {
            content_id: "ability.arbalist.stillness".to_owned(),
            activation_ticks: 18,
            movement_threshold_basis_points: 2_000,
            projectile_speed_bonus_basis_points: 1_000,
            primary_damage_bonus_basis_points: 800,
            break_on_damage: true,
            break_on_slipstep: true,
        })?;
        Ok(PlayerCombatState::new(weapon, mark, slip, stillness).expect("combat"))
    }

    fn target(position: SimulationVector) -> HostileTargetState {
        HostileTargetState {
            entity_id: id(900),
            position,
            target_is_immune: false,
            resistance_basis_points: 0,
            additional_direct_damage_reductions_basis_points: Vec::new(),
            armor: 2,
            current_barrier: 0,
            health_damage_cap_basis_points: None,
        }
    }

    fn tonic() -> RedTonicSimulation {
        RedTonicSimulation::new(
            crate::RedTonicDefinition::first_playable(),
            PlayerVitals::new(120, 120).expect("vitals"),
            TonicBelt::first_playable(),
        )
        .expect("tonic")
    }

    #[test]
    fn debug_invulnerability_preserves_hypothetical_damage_without_any_mutation() {
        let mut target = target(SimulationVector::new(5.6, 6.0));
        let mut tonic = tonic();
        let mut combat = combat().expect("combat");
        let before = (target.clone(), tonic.clone(), combat.clone());
        let debug = apply_hostile_contact_transaction_with_policy(
            id(20),
            22,
            DamageType::Physical,
            &mut target,
            &mut tonic,
            &mut combat,
            HostileDamagePolicy::DebugInvulnerable,
        )
        .expect("debug damage event");
        assert!(debug.debug_invulnerable);
        assert!(debug.damage.health_damage_applied > 0);
        assert_eq!(
            debug.health_application.requested,
            debug.damage.health_damage_applied
        );
        assert_eq!(debug.health_application.applied, 0);
        assert_eq!(
            debug.health_application.health_before,
            debug.health_application.health_after
        );
        assert!(debug.focused_transition.is_none());
        assert_eq!((target, tonic, combat), before);
    }

    #[test]
    fn projectile_collision_emits_marked_debug_damage_and_consumes_projectile() {
        let mut hostile = HostileProjectileSimulation::default();
        hostile.set_damage_policy(HostileDamagePolicy::DebugInvulnerable);
        hostile.projectiles.push(contact_projectile(41, false));
        let mut target = target(SimulationVector::new(5.6, 6.0));
        let mut tonic = tonic();
        let mut combat = combat().expect("combat");
        let health_before = tonic.vitals().current_health();

        let step = hostile
            .step(&arena(), &mut target, &mut tonic, &mut combat)
            .expect("debug-invulnerable collision");

        assert!(hostile.projectiles().is_empty());
        assert_eq!(tonic.vitals().current_health(), health_before);
        assert!(step.events.iter().any(|event| matches!(
            event,
            HostileEvent::Contact {
                target: HostileCollisionTarget::Player(_),
                damage: Some(damage),
                health_application: Some(application),
                debug_invulnerable: true,
                ..
            } if damage.health_damage_applied > 0
                && application.requested == damage.health_damage_applied
                && application.applied == 0
                && application.health_before == application.health_after
        )));
    }

    #[test]
    fn telegraph_events_cannot_spawn_or_hit() {
        let mut hostile = HostileProjectileSimulation::default();
        let telegraph = EnemyEvent::StateChanged {
            enemy_id: crate::DROWNED_PILGRIM_ID,
            state: crate::EnemyStateKind::AttackTelegraph,
        };
        assert!(matches!(
            hostile.spawn_from_enemy_event(id(10), SimulationVector::new(4.0, 4.0), &telegraph),
            Err(HostileError::EventDoesNotAuthorizeProjectileSpawn)
        ));
        assert!(hostile.projectiles().is_empty());

        let mut drifted_fire = fan_event();
        let EnemyEvent::FanFired { attack, .. } = &mut drifted_fire else {
            unreachable!();
        };
        attack.speed_milli_tiles_per_second += 1;
        assert!(matches!(
            hostile.spawn_from_enemy_event(id(10), SimulationVector::new(4.0, 4.0), &drifted_fire),
            Err(HostileError::InvalidProjectileAttack)
        ));
        assert!(hostile.projectiles().is_empty());
    }

    #[test]
    fn fan_spawn_preserves_source_cast_payload_and_origin() {
        let mut hostile = HostileProjectileSimulation::with_allocator(
            EntityIdAllocator::starting_at(NonZeroU64::new(100).expect("nonzero")),
        );
        let events = hostile
            .spawn_from_enemy_event(id(20), SimulationVector::new(4.0, 12.0), &fan_event())
            .expect("fan spawn");
        assert_eq!(events.len(), 3);
        assert_eq!(hostile.projectiles()[0].id().get(), 100);
        assert_eq!(hostile.projectiles()[0].source_entity_id(), id(20));
        assert_eq!(hostile.projectiles()[0].cast_id(), AttackCastId::FIRST);
        assert_eq!(
            hostile.projectiles()[0].position(),
            SimulationVector::new(4.45, 12.0)
        );
        assert_eq!(hostile.projectiles()[0].raw_damage(), 8);
        assert_eq!(hostile.projectiles()[0].damage_type(), DamageType::Physical);
        assert_eq!(
            hostile.projectiles()[0].declared_damage_band(),
            DamageBand::Chip
        );
    }

    #[test]
    fn clearing_projectiles_preserves_tick_and_monotonic_allocator() {
        let mut hostile = HostileProjectileSimulation::with_allocator(
            EntityIdAllocator::starting_at(NonZeroU64::new(100).expect("nonzero")),
        );
        hostile
            .spawn_from_enemy_event(id(20), SimulationVector::new(4.0, 12.0), &fan_event())
            .expect("first fan");
        let tick = hostile.tick();
        let cleared = hostile.clear_projectiles();
        assert_eq!(
            cleared
                .iter()
                .map(|projectile| projectile.id().get())
                .collect::<Vec<_>>(),
            vec![100, 101, 102]
        );
        assert_eq!(hostile.tick(), tick);
        assert!(hostile.projectiles().is_empty());

        hostile
            .spawn_from_enemy_event(id(20), SimulationVector::new(4.0, 12.0), &fan_event())
            .expect("second fan");
        assert_eq!(hostile.projectiles()[0].id().get(), 103);
    }

    #[test]
    fn high_speed_player_tangent_hits_and_earlier_solid_is_terminal() {
        let mut hostile = HostileProjectileSimulation::default();
        hostile.projectiles.push(HostileProjectile {
            id: id(1),
            source_entity_id: id(20),
            cast_id: AttackCastId::FIRST,
            source_kind: HostileProjectileSourceKind::AimedFan,
            pattern_id: "pattern.enemy.drowned_pilgrim.fan",
            position: SimulationVector::new(4.45, 12.0),
            direction: AimDirection::east(),
            speed_tiles_per_second: 300.0,
            radius_tiles: 0.12,
            remaining_lifetime_ticks: 2,
            raw_damage: 8,
            damage_type: DamageType::Physical,
            declared_damage_band: DamageBand::Chip,
            threat_cost: 3,
            memory_family: EchoMemoryFamily::FanProjectile,
            counterplay: Counterplay::Strafe,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            ignored_player_ids: BTreeSet::new(),
        });
        let mut tangent_target = target(SimulationVector::new(9.0, 12.369));
        let mut tangent_tonic = tonic();
        let mut tangent_combat = combat().expect("combat");
        let step = hostile
            .step(
                &arena(),
                &mut tangent_target,
                &mut tangent_tonic,
                &mut tangent_combat,
            )
            .expect("high-speed sweep");
        assert!(step.events.iter().any(|event| matches!(
            event,
            HostileEvent::Contact {
                target: HostileCollisionTarget::Player(_),
                ..
            }
        )));

        let mut blocked = HostileProjectileSimulation::default();
        blocked.projectiles.push(HostileProjectile {
            id: id(2),
            source_entity_id: id(20),
            cast_id: AttackCastId::FIRST,
            source_kind: HostileProjectileSourceKind::AimedFan,
            pattern_id: "pattern.enemy.drowned_pilgrim.fan",
            position: SimulationVector::new(9.0, 6.0),
            direction: AimDirection::east(),
            speed_tiles_per_second: 300.0,
            radius_tiles: 0.12,
            remaining_lifetime_ticks: 2,
            raw_damage: 8,
            damage_type: DamageType::Physical,
            declared_damage_band: DamageBand::Chip,
            threat_cost: 3,
            memory_family: EchoMemoryFamily::FanProjectile,
            counterplay: Counterplay::Strafe,
            disposition: HostileDisposition::ConsumeOnPlayerOrSolid,
            pierces_players: false,
            ignored_player_ids: BTreeSet::new(),
        });
        let mut target = target(SimulationVector::new(13.0, 6.0));
        let mut tonic = tonic();
        let mut combat = combat().expect("combat");
        let blocked_step = blocked
            .step(&arena(), &mut target, &mut tonic, &mut combat)
            .expect("solid sweep");
        assert!(blocked_step.events.iter().any(|event| matches!(
            event,
            HostileEvent::Contact {
                target: HostileCollisionTarget::Solid(SolidColliderId::Pillar(0)),
                damage: None,
                ..
            }
        )));
        assert_eq!(tonic.vitals().current_health(), 120);
    }

    #[test]
    fn global_projectile_grace_consumes_nonpiercing_hits_for_exactly_three_ticks() {
        let mut hostile = HostileProjectileSimulation {
            projectiles: vec![contact_projectile(1, false), contact_projectile(2, false)],
            ..HostileProjectileSimulation::default()
        };
        let mut target = target(SimulationVector::new(5.6, 6.0));
        let mut tonic = tonic();
        let mut combat = combat().expect("combat");

        let first = hostile
            .step(&arena(), &mut target, &mut tonic, &mut combat)
            .expect("first contact");
        assert_eq!(tonic.vitals().current_health(), 114);
        assert_eq!(
            first
                .events
                .iter()
                .filter(|event| matches!(
                    event,
                    HostileEvent::Contact {
                        damage: Some(_),
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(first.events.iter().any(|event| matches!(
            event,
            HostileEvent::ProjectileGraceIgnored {
                projectile_id,
                pierces_players: false,
                consumed: true,
                ..
            } if *projectile_id == id(2)
        )));

        for projectile_id in 3..=4 {
            hostile
                .projectiles
                .push(contact_projectile(projectile_id, false));
            let ignored = hostile
                .step(&arena(), &mut target, &mut tonic, &mut combat)
                .expect("grace contact");
            assert!(ignored.events.iter().any(|event| matches!(
                event,
                HostileEvent::ProjectileGraceIgnored { consumed: true, .. }
            )));
            assert_eq!(tonic.vitals().current_health(), 114);
        }

        hostile.projectiles.push(contact_projectile(5, false));
        let eligible = hostile
            .step(&arena(), &mut target, &mut tonic, &mut combat)
            .expect("post-grace contact");
        assert!(eligible.events.iter().any(|event| matches!(
            event,
            HostileEvent::Contact {
                damage: Some(_),
                ..
            }
        )));
        assert_eq!(tonic.vitals().current_health(), 108);
    }

    #[test]
    fn piercing_grace_ignore_continues_and_blacklists_player_for_projectile_lifetime() {
        let mut hostile = HostileProjectileSimulation {
            projectiles: vec![contact_projectile(1, false), contact_projectile(2, true)],
            ..HostileProjectileSimulation::default()
        };
        let mut target = target(SimulationVector::new(5.6, 6.0));
        let mut tonic = tonic();
        let mut combat = combat().expect("combat");

        let step = hostile
            .step(&arena(), &mut target, &mut tonic, &mut combat)
            .expect("mixed contact");
        assert_eq!(tonic.vitals().current_health(), 114);
        assert!(step.events.iter().any(|event| matches!(
            event,
            HostileEvent::ProjectileGraceIgnored {
                projectile_id,
                pierces_players: true,
                consumed: false,
                ..
            } if *projectile_id == id(2)
        )));
        let piercing = hostile
            .projectiles()
            .iter()
            .find(|projectile| projectile.id() == id(2))
            .expect("piercing projectile survives grace");
        assert!(piercing.ignored_player_ids.contains(&target.entity_id));

        for _ in 0..3 {
            hostile
                .step(&arena(), &mut target, &mut tonic, &mut combat)
                .expect("piercing continuation");
        }
        assert_eq!(tonic.vitals().current_health(), 114);
    }

    #[test]
    fn contact_applies_canonical_barrier_cap_health_and_breaks_focus_transactionally() {
        let mut target = target(SimulationVector::new(6.0, 6.0));
        target.current_barrier = 3;
        target.health_damage_cap_basis_points = Some(500);
        let mut tonic = tonic();
        let mut combat = combat().expect("combat");
        let combat_arena = arena();
        let combat_world = ProjectileCollisionWorld::new(&combat_arena, Vec::new()).expect("world");
        let mut movement =
            PlayerMovementState::at_arena_spawn(&combat_arena).expect("movement state");
        for _ in 0..18 {
            combat
                .step_with_movement(
                    &mut movement,
                    crate::CombatAction::default(),
                    &combat_arena,
                    &combat_world,
                )
                .expect("Stillness buildup");
        }
        assert!(combat.focused());
        let applied = apply_hostile_contact_transaction(
            id(20),
            22,
            DamageType::Physical,
            &mut target,
            &mut tonic,
            &mut combat,
        )
        .expect("damage");
        assert_eq!(applied.damage.post_armor_damage, 20);
        assert_eq!(applied.damage.barrier_absorbed, 3);
        assert_eq!(applied.damage.health_damage_cap, Some(6));
        assert_eq!(applied.damage.health_damage_applied, 6);
        assert_eq!(tonic.vitals().current_health(), 114);
        assert_eq!(target.current_barrier, 0);
        assert_eq!(applied.health_application.health_after, 114);
        assert!(applied.focused_transition.is_some());
        assert!(!combat.focused());
        let later_action = combat
            .step_with_movement(
                &mut movement,
                crate::CombatAction {
                    primary_held: true,
                    primary_press_sequence: 1,
                    ..crate::CombatAction::default()
                },
                &combat_arena,
                &combat_world,
            )
            .expect("later action");
        assert!(!later_action.shots[0].projectile.focused_by_stillness());
    }

    #[test]
    fn lane_geometry_is_tangent_inclusive_and_uses_canonical_damage() {
        let geometry = LaneGeometry {
            origin: SimulationVector::new(16.0, 12.0),
            axes_degrees: [0, 90],
            width_tiles: 0.9,
            extends_to_arena_collision: true,
        };
        assert!(geometry.contacts_player(SimulationVector::new(20.0, 12.7)));
        assert!(!geometry.contacts_player(SimulationVector::new(20.0, 12.701)));
        let attack = LaneAttackDefinition {
            pattern_id: "pattern.enemy.chain_sentry.cross_lanes",
            lane_count: 2,
            width_milli_tiles: 900,
            active_ticks: 11,
            raw_damage: 22,
            damage_type: DamageType::Physical,
            damage_band: DamageBand::Pressure,
            threat_cost_per_lane: 12,
            memory_family: EchoMemoryFamily::LaneOrBeam,
            counterplay: Counterplay::LeaveTelegraph,
            disposition: HostileDisposition::ExpireAtAuthoredEnd,
            maximum_active_instances: 2,
        };
        let mut standard_target = target(SimulationVector::new(20.0, 12.7));
        let mut standard_tonic = tonic();
        let mut standard_combat = combat().expect("combat");
        let result = resolve_lane_contact(
            id(30),
            &attack,
            geometry,
            &mut standard_target,
            &mut standard_tonic,
            &mut standard_combat,
        )
        .expect("lane")
        .expect("contact");
        assert_eq!(result.damage.damage_type, DamageType::Physical);
        assert_eq!(result.health_application.applied, 20);

        let mut debug_target = target(SimulationVector::new(20.0, 12.7));
        let mut debug_tonic = tonic();
        let mut debug_combat = combat().expect("combat");
        let health_before = debug_tonic.vitals().current_health();
        let debug = resolve_lane_contact_with_policy(
            id(30),
            &attack,
            geometry,
            &mut debug_target,
            &mut debug_tonic,
            &mut debug_combat,
            HostileDamagePolicy::DebugInvulnerable,
        )
        .expect("debug lane")
        .expect("debug contact");
        assert!(debug.debug_invulnerable);
        assert_eq!(debug.health_application.requested, 20);
        assert_eq!(debug.health_application.applied, 0);
        assert_eq!(debug_tonic.vitals().current_health(), health_before);
    }

    #[test]
    fn pilgrim_actor_uses_fixed_remainder_motion_and_fixed_enemies_reject_it() {
        let mut actor = EnemyActor::new(id(50), EnemyActorKind::DrownedPilgrim, 4_000, 12_000, 340)
            .expect("actor");
        let input = actor
            .target_input(SimulationVector::new(14.0, 12.0))
            .expect("authoritative target input");
        assert_eq!(input.distance_milli_tiles, 10_000);
        assert_eq!(input.delta, AimVector { x: 10_000, y: 0 });
        let event = EnemyEvent::ApproachIntent {
            speed_milli_tiles_per_second: 2_200,
            target_delta: input.delta,
            stop_distance_milli_tiles: 5_000,
        };
        for _ in 0..30 {
            actor.apply_event(&arena(), &event).expect("movement");
        }
        assert_eq!(actor.position_milli_tiles(), (6_200, 12_000));

        let mut reed =
            EnemyActor::new(id(51), EnemyActorKind::BellReed, 16_000, 3_000, 420).expect("Reed");
        assert!(matches!(
            reed.apply_event(&arena(), &event),
            Err(HostileError::FixedEnemyReceivedMovement)
        ));
    }

    #[test]
    fn fixed_hostile_replay_is_identical() {
        fn replay() -> blake3::Hash {
            let mut hostile = HostileProjectileSimulation::default();
            hostile
                .spawn_from_enemy_event(id(20), SimulationVector::new(4.0, 12.0), &fan_event())
                .expect("spawn");
            let mut target = target(SimulationVector::new(20.0, 12.0));
            let mut tonic = tonic();
            let mut combat = combat().expect("combat");
            let mut hasher = blake3::Hasher::new();
            for _ in 0..80 {
                let step = hostile
                    .step(&arena(), &mut target, &mut tonic, &mut combat)
                    .expect("step");
                hasher.update(format!("{step:?}").as_bytes());
            }
            hasher.finalize()
        }
        let first = replay();
        assert_eq!(
            first.to_string(),
            "cb9c95a99c1ce1b6b31fefcb0104641b6e7496c907349bd3a8903b032fd7b177"
        );
        assert_eq!(first, replay());
    }
}
