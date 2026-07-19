//! Canonical simulation-authored player-damage facts for the ordinary Core private route.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`SIM-004`, `SIM-010`,
//! `COM-002`, and `DTH-001`/`010`), `Gravebound_Content_Production_Spec_v1.md`
//! (`CONT-WORLD-001`, `CONT-ROOM-007`, and `CONT-BOSS-001`), and
//! `Gravebound_Development_Roadmap_v1.md` (`GB-M03-03`, `GB-M03-06`, and `GB-M03-08`).
//!
//! These are scene facts, not durable trace observations. The route terminal owner later joins
//! them with exact connection, Recall, and status state. Keeping those actor-owned axes out of
//! scene simulation prevents transport state from being guessed or backfilled after a lethal hit.

use sim_core::{
    AppliedHostileDamage, AuthoritativeDeathCauseKind, DamageEvent, DamageType, EntityId,
    HostileCollisionTarget, HostileEvent, NormalWaveLaneEvent, NormalWaveStep, SimulationVector,
    Tick,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct CorePrivatePlayerDamageFactV1 {
    pub tick: Tick,
    pub event_ordinal: u32,
    pub cause_kind: AuthoritativeDeathCauseKind,
    pub source_content_id: &'static str,
    pub source_entity_id: EntityId,
    pub pattern_id: &'static str,
    pub attack_id: &'static str,
    pub raw_damage: u32,
    pub final_damage: u32,
    pub damage_type: DamageType,
    pub pre_health: u32,
    pub post_health: u32,
    pub source_position: SimulationVector,
}

impl CorePrivatePlayerDamageFactV1 {
    #[must_use]
    pub const fn lethal(&self) -> bool {
        self.post_health == 0
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CorePrivatePlayerDamageError {
    #[error("private-route player damage belongs to another tick or target")]
    ForeignFrameAuthority,
    #[error("private-route player damage references unknown promoted Core content")]
    UnknownContent,
    #[error("private-route player damage arithmetic is inconsistent")]
    InvalidDamage,
    #[error("private-route player lethality does not match its ordered damage facts")]
    LethalityMismatch,
    #[error("private-route player damage fact capacity was exceeded")]
    CapacityExceeded,
}

pub fn normal_wave_player_damage_facts(
    step: &NormalWaveStep,
    player: EntityId,
    player_died: bool,
) -> Result<Vec<CorePrivatePlayerDamageFactV1>, CorePrivatePlayerDamageError> {
    let mut facts = Vec::new();
    for event in &step.lane_events {
        let NormalWaveLaneEvent::Contact {
            source_entity_id,
            source_position,
            pattern_id,
            player_entity_id,
            damage,
            ..
        } = event
        else {
            continue;
        };
        if *player_entity_id != player {
            return Err(CorePrivatePlayerDamageError::ForeignFrameAuthority);
        }
        push_applied(
            &mut facts,
            step.tick,
            *source_entity_id,
            player,
            *source_position,
            pattern_id,
            damage,
        )?;
    }
    push_hostile_contacts(&mut facts, step.tick, player, &step.hostile_step.events)?;
    finish_facts(facts, step.tick, player_died)
}

pub fn fixed_room_player_damage_facts(
    step: &sim_content::CoreFixedDungeonRoomStep,
    player: EntityId,
    player_died: bool,
) -> Result<Vec<CorePrivatePlayerDamageFactV1>, CorePrivatePlayerDamageError> {
    let mut facts = match step {
        sim_content::CoreFixedDungeonRoomStep::B1(step)
        | sim_content::CoreFixedDungeonRoomStep::B5(step) => step.wave_step.as_ref().map_or_else(
            || Ok(Vec::new()),
            |wave| normal_wave_player_damage_facts(wave, player, player_died),
        )?,
        sim_content::CoreFixedDungeonRoomStep::B2(step) => step.combat.as_ref().map_or_else(
            || Ok(Vec::new()),
            |combat| normal_wave_player_damage_facts(&combat.immutable_wave, player, player_died),
        )?,
        sim_content::CoreFixedDungeonRoomStep::B3(step) => {
            let mut facts = Vec::new();
            if let Some(combat) = &step.combat {
                for contact in &combat.charge_contacts {
                    if contact.target != player {
                        return Err(CorePrivatePlayerDamageError::ForeignFrameAuthority);
                    }
                    push_applied(
                        &mut facts,
                        contact.tick,
                        contact.application.damage.source,
                        player,
                        contact.source_position,
                        "miniboss.sepulcher_knight.charge_lane",
                        &contact.application,
                    )?;
                }
                push_hostile_contacts(
                    &mut facts,
                    combat.tick,
                    player,
                    &combat.hostile_step.events,
                )?;
            }
            facts
        }
    };
    let tick = fixed_room_tick(step);
    reordinal(&mut facts)?;
    finish_facts(facts, tick, player_died)
}

pub fn caldus_player_damage_facts(
    tick: Tick,
    encounter: Option<&sim_core::CoreCaldusEncounterStep>,
    player: EntityId,
    player_died: bool,
) -> Result<Vec<CorePrivatePlayerDamageFactV1>, CorePrivatePlayerDamageError> {
    let mut facts = Vec::new();
    if let Some(encounter) = encounter {
        if encounter.tick != tick {
            return Err(CorePrivatePlayerDamageError::ForeignFrameAuthority);
        }
        for contact in &encounter.charge_damage {
            if contact.participant.entity_id != player {
                return Err(CorePrivatePlayerDamageError::ForeignFrameAuthority);
            }
            push_applied(
                &mut facts,
                contact.tick,
                contact.damage.damage.source,
                player,
                contact.source_position,
                "boss.caldus.charge_lane",
                &contact.damage,
            )?;
        }
        push_hostile_contacts(&mut facts, tick, player, &encounter.hostile_step.events)?;
    }
    finish_facts(facts, tick, player_died)
}

fn push_hostile_contacts(
    facts: &mut Vec<CorePrivatePlayerDamageFactV1>,
    tick: Tick,
    player: EntityId,
    events: &[HostileEvent],
) -> Result<(), CorePrivatePlayerDamageError> {
    for event in events {
        let HostileEvent::Contact {
            tick: event_tick,
            source_entity_id,
            source_position,
            pattern_id,
            target: HostileCollisionTarget::Player(target),
            damage: Some(damage),
            health_application: Some(application),
            debug_invulnerable: false,
            ..
        } = event
        else {
            continue;
        };
        if *event_tick != tick || *target != player {
            return Err(CorePrivatePlayerDamageError::ForeignFrameAuthority);
        }
        validate_damage(tick, damage, application)?;
        push_damage(
            facts,
            tick,
            *source_entity_id,
            player,
            *source_position,
            pattern_id,
            damage,
        )?;
    }
    Ok(())
}

fn push_applied(
    facts: &mut Vec<CorePrivatePlayerDamageFactV1>,
    tick: Tick,
    source: EntityId,
    player: EntityId,
    source_position: SimulationVector,
    pattern_id: &'static str,
    applied: &AppliedHostileDamage,
) -> Result<(), CorePrivatePlayerDamageError> {
    if applied.debug_invulnerable {
        return Ok(());
    }
    validate_damage(tick, &applied.damage, &applied.health_application)?;
    push_damage(
        facts,
        tick,
        source,
        player,
        source_position,
        pattern_id,
        &applied.damage,
    )
}

fn push_damage(
    facts: &mut Vec<CorePrivatePlayerDamageFactV1>,
    tick: Tick,
    source: EntityId,
    player: EntityId,
    source_position: SimulationVector,
    pattern_id: &'static str,
    damage: &DamageEvent,
) -> Result<(), CorePrivatePlayerDamageError> {
    if damage.source != source || damage.target != player || !source_position.is_finite() {
        return Err(CorePrivatePlayerDamageError::InvalidDamage);
    }
    let event_ordinal =
        u32::try_from(facts.len()).map_err(|_| CorePrivatePlayerDamageError::CapacityExceeded)?;
    facts.push(CorePrivatePlayerDamageFactV1 {
        tick,
        event_ordinal,
        cause_kind: AuthoritativeDeathCauseKind::DirectHit,
        source_content_id: source_content_id(pattern_id)?,
        source_entity_id: source,
        pattern_id,
        attack_id: pattern_id,
        raw_damage: damage.raw_damage,
        final_damage: damage.health_damage_applied,
        damage_type: damage.damage_type,
        pre_health: damage.health_before,
        post_health: damage.health_after,
        source_position,
    });
    Ok(())
}

fn validate_damage(
    tick: Tick,
    damage: &DamageEvent,
    application: &sim_core::DamageAppliedEvent,
) -> Result<(), CorePrivatePlayerDamageError> {
    let sim_core::DamageAppliedEvent {
        tick: application_tick,
        applied,
        health_before: before,
        health_after: after,
        ..
    } = *application;
    if damage.raw_damage == 0
        || damage.health_before == 0
        || application_tick != tick
        || damage.health_damage_applied != applied
        || damage.health_before != before
        || damage.health_after != after
        || after != before.saturating_sub(applied)
        || damage.lethal != (after == 0)
    {
        return Err(CorePrivatePlayerDamageError::InvalidDamage);
    }
    Ok(())
}

fn finish_facts(
    mut facts: Vec<CorePrivatePlayerDamageFactV1>,
    tick: Tick,
    player_died: bool,
) -> Result<Vec<CorePrivatePlayerDamageFactV1>, CorePrivatePlayerDamageError> {
    reordinal(&mut facts)?;
    if facts.iter().any(|fact| fact.tick != tick)
        || facts.iter().filter(|fact| fact.lethal()).count() != usize::from(player_died)
        || facts
            .iter()
            .position(CorePrivatePlayerDamageFactV1::lethal)
            .is_some_and(|index| index + 1 != facts.len())
        || facts
            .windows(2)
            .any(|pair| pair[0].post_health != pair[1].pre_health)
    {
        return Err(CorePrivatePlayerDamageError::LethalityMismatch);
    }
    Ok(facts)
}

fn reordinal(
    facts: &mut [CorePrivatePlayerDamageFactV1],
) -> Result<(), CorePrivatePlayerDamageError> {
    for (index, fact) in facts.iter_mut().enumerate() {
        fact.event_ordinal =
            u32::try_from(index).map_err(|_| CorePrivatePlayerDamageError::CapacityExceeded)?;
    }
    Ok(())
}

const fn fixed_room_tick(step: &sim_content::CoreFixedDungeonRoomStep) -> Tick {
    match step {
        sim_content::CoreFixedDungeonRoomStep::B1(step)
        | sim_content::CoreFixedDungeonRoomStep::B5(step) => step.tick,
        sim_content::CoreFixedDungeonRoomStep::B2(step) => step.tick,
        sim_content::CoreFixedDungeonRoomStep::B3(step) => step.tick,
    }
}

fn source_content_id(pattern_id: &str) -> Result<&'static str, CorePrivatePlayerDamageError> {
    let source = match pattern_id {
        "pattern.enemy.drowned_pilgrim.fan" => "enemy.drowned_pilgrim",
        "pattern.enemy.bell_reed.gap_ring" => "enemy.bell_reed",
        "pattern.enemy.chain_sentry.cross_lanes" => "enemy.chain_sentry",
        "pattern.enemy.bell_acolyte.alternating_fan" => "enemy.bell_acolyte",
        "pattern.enemy.choir_skull.rotor" => "enemy.choir_skull",
        "pattern.enemy.mire_leech.charge" => "enemy.mire_leech",
        "miniboss.sepulcher_knight.charge_lane"
        | "miniboss.sepulcher_knight.shield_fan"
        | "miniboss.sepulcher_knight.stop_ring" => "miniboss.sepulcher_knight",
        "miniboss.choir_abbot.rotor" | "miniboss.choir_abbot.recovery_ring" => {
            "miniboss.choir_abbot"
        }
        "boss.caldus.shield_arc"
        | "boss.caldus.bell_ring"
        | "boss.caldus.charge_lane"
        | "boss.caldus.charge_stop_ring" => "boss.sir_caldus",
        _ => return Err(CorePrivatePlayerDamageError::UnknownContent),
    };
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> EntityId {
        EntityId::new(value).unwrap()
    }

    fn applied(tick: Tick, health: u32, raw_damage: u32) -> AppliedHostileDamage {
        let damage = sim_core::resolve_direct_hit(
            &sim_core::DirectHitRequest::new(sim_core::DirectHitParameters {
                source: id(10),
                target: id(20),
                collision_confirmed: true,
                target_is_immune: false,
                raw_damage,
                damage_type: DamageType::Physical,
                attacker_multiplier_basis_points: 10_000,
                target_resistance_basis_points: 0,
                direct_damage_reductions_basis_points: Vec::new(),
                armor: 0,
                current_barrier: 0,
                health_damage_cap_basis_points: None,
                current_health: health,
                max_health: 100,
            })
            .unwrap(),
        )
        .unwrap();
        AppliedHostileDamage {
            health_application: sim_core::DamageAppliedEvent {
                tick,
                requested: damage.health_damage_applied,
                applied: damage.health_damage_applied,
                health_before: damage.health_before,
                health_after: damage.health_after,
                restore_continues: false,
            },
            damage,
            focused_transition: None,
            debug_invulnerable: false,
        }
    }

    #[test]
    fn promoted_pattern_closure_maps_to_stable_damage_sources() {
        for (pattern, expected) in [
            ("pattern.enemy.drowned_pilgrim.fan", "enemy.drowned_pilgrim"),
            ("pattern.enemy.bell_reed.gap_ring", "enemy.bell_reed"),
            (
                "pattern.enemy.chain_sentry.cross_lanes",
                "enemy.chain_sentry",
            ),
            (
                "pattern.enemy.bell_acolyte.alternating_fan",
                "enemy.bell_acolyte",
            ),
            ("pattern.enemy.choir_skull.rotor", "enemy.choir_skull"),
            ("pattern.enemy.mire_leech.charge", "enemy.mire_leech"),
            (
                "miniboss.sepulcher_knight.charge_lane",
                "miniboss.sepulcher_knight",
            ),
            (
                "miniboss.sepulcher_knight.shield_fan",
                "miniboss.sepulcher_knight",
            ),
            (
                "miniboss.sepulcher_knight.stop_ring",
                "miniboss.sepulcher_knight",
            ),
            ("miniboss.choir_abbot.rotor", "miniboss.choir_abbot"),
            ("miniboss.choir_abbot.recovery_ring", "miniboss.choir_abbot"),
            ("boss.caldus.shield_arc", "boss.sir_caldus"),
            ("boss.caldus.bell_ring", "boss.sir_caldus"),
            ("boss.caldus.charge_lane", "boss.sir_caldus"),
            ("boss.caldus.charge_stop_ring", "boss.sir_caldus"),
        ] {
            assert_eq!(source_content_id(pattern).unwrap(), expected);
        }
        assert_eq!(
            source_content_id("pattern.unknown"),
            Err(CorePrivatePlayerDamageError::UnknownContent)
        );
    }

    #[test]
    fn ordered_facts_preserve_source_origin_and_require_final_lethality() {
        let tick = Tick(41);
        let mut facts = Vec::new();
        push_applied(
            &mut facts,
            tick,
            id(10),
            id(20),
            SimulationVector::new(4.0, 6.0),
            "pattern.enemy.drowned_pilgrim.fan",
            &applied(tick, 100, 10),
        )
        .unwrap();
        push_applied(
            &mut facts,
            tick,
            id(10),
            id(20),
            SimulationVector::new(7.0, 8.0),
            "pattern.enemy.drowned_pilgrim.fan",
            &applied(tick, 90, 100),
        )
        .unwrap();
        let facts = finish_facts(facts, tick, true).unwrap();
        assert_eq!(facts.len(), 2);
        assert_eq!((facts[0].event_ordinal, facts[1].event_ordinal), (0, 1));
        assert_eq!(facts[0].source_position, SimulationVector::new(4.0, 6.0));
        assert_eq!(facts[1].source_position, SimulationVector::new(7.0, 8.0));
        assert!(!facts[0].lethal());
        assert!(facts[1].lethal());

        assert_eq!(
            finish_facts(facts.clone(), tick, false),
            Err(CorePrivatePlayerDamageError::LethalityMismatch)
        );
        let mut reversed = facts;
        reversed.swap(0, 1);
        assert_eq!(
            finish_facts(reversed, tick, true),
            Err(CorePrivatePlayerDamageError::LethalityMismatch)
        );
    }

    #[test]
    fn foreign_target_tick_discontinuity_and_debug_damage_fail_closed() {
        let tick = Tick(41);
        let position = SimulationVector::new(4.0, 6.0);
        let pattern = "pattern.enemy.drowned_pilgrim.fan";
        assert_eq!(
            push_applied(
                &mut Vec::new(),
                tick,
                id(10),
                id(21),
                position,
                pattern,
                &applied(tick, 100, 10),
            ),
            Err(CorePrivatePlayerDamageError::InvalidDamage)
        );
        assert_eq!(
            push_applied(
                &mut Vec::new(),
                tick,
                id(10),
                id(20),
                position,
                pattern,
                &applied(Tick(42), 100, 10),
            ),
            Err(CorePrivatePlayerDamageError::InvalidDamage)
        );

        let mut facts = Vec::new();
        push_applied(
            &mut facts,
            tick,
            id(10),
            id(20),
            position,
            pattern,
            &applied(tick, 100, 10),
        )
        .unwrap();
        push_applied(
            &mut facts,
            tick,
            id(10),
            id(20),
            position,
            pattern,
            &applied(tick, 80, 10),
        )
        .unwrap();
        assert_eq!(
            finish_facts(facts, tick, false),
            Err(CorePrivatePlayerDamageError::LethalityMismatch)
        );

        let mut debug = applied(tick, 100, 10);
        debug.debug_invulnerable = true;
        let mut facts = Vec::new();
        push_applied(&mut facts, tick, id(10), id(20), position, pattern, &debug).unwrap();
        assert!(facts.is_empty());
    }
}
