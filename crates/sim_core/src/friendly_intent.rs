//! Shared fail-closed provenance validation for player damage against authoritative actors.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    CollisionTarget, CombatStep, EntityId, FriendlyProjectileSource, RawDamageIntentSource,
};

pub(crate) fn validate_friendly_intents(
    combat: &CombatStep,
    target: EntityId,
) -> Result<(), FriendlyIntentError> {
    let mut previous = None;
    let mut seen = BTreeSet::new();
    for intent in &combat.raw_damage_intents {
        if intent.tick != combat.tick || intent.target != target {
            return Err(FriendlyIntentError::InvalidProvenance);
        }
        let key = (intent.projectile_id, intent.contact_ordinal);
        if previous.is_some_and(|prior| key < prior) || !seen.insert(key) {
            return Err(FriendlyIntentError::UnstableOrder);
        }
        previous = Some(key);
        if intent.source == RawDamageIntentSource::NailTrap {
            let count = combat
                .nail_traps
                .triggers
                .iter()
                .filter(|trigger| {
                    trigger.tick == intent.tick
                        && trigger.trap_id == intent.projectile_id
                        && trigger.target_id == target
                        && trigger.snapshot_weapon_raw_damage == intent.base_raw_damage
                        && trigger.raw_damage == intent.resolved_raw_damage
                })
                .count();
            if count != 1 {
                return Err(FriendlyIntentError::InvalidProvenance);
            }
            continue;
        }
        let source = match intent.source {
            RawDamageIntentSource::Primary => FriendlyProjectileSource::Primary,
            RawDamageIntentSource::BellDebtRepeat => FriendlyProjectileSource::BellDebtRepeat,
            RawDamageIntentSource::GraveMark => FriendlyProjectileSource::GraveMark,
            RawDamageIntentSource::NailTrap => unreachable!("handled above"),
        };
        let count = combat
            .collisions
            .iter()
            .filter(|collision| {
                collision.tick == intent.tick
                    && collision.projectile_id == intent.projectile_id
                    && collision.source == source
                    && collision.contact_ordinal == intent.contact_ordinal
                    && collision.target == CollisionTarget::Enemy(target)
            })
            .count();
        if count != 1 {
            return Err(FriendlyIntentError::InvalidProvenance);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum FriendlyIntentError {
    #[error("friendly damage intent has no unique matching collision provenance")]
    InvalidProvenance,
    #[error("friendly damage intents are not uniquely sorted by projectile and contact")]
    UnstableOrder,
}
