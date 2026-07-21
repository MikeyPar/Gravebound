//! Immutable server-journal identities for private-route simulation entities.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `SIM-004`, `DTH-001`, and
//! `TECH-021`-`023`; `Gravebound_Content_Production_Spec_v1.md` `CONT-WORLD-001`,
//! `CONT-ROOM-007`, and `CONT-BOSS-001`; and `Gravebound_Development_Roadmap_v1.md`
//! `GB-M03-03`/`06`. The client never authors or remaps these identities.

use std::collections::{BTreeMap, BTreeSet};

use sim_core::EntityId;
use thiserror::Error;

use crate::{
    CorePrivateDangerEntryAuthority, CorePrivatePlayerDamageFactV1, DeathEntityIdentityAuthority,
};

const PRIVATE_ROUTE_ENTITY_CONTEXT: &[u8] = b"gravebound/private-route-entity-journal/v1\0";

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CorePrivateDeathEntityIdentityError {
    #[error("private-route entity identity set is empty")]
    Empty,
    #[error("private-route entity identity derivation collided")]
    Collision,
}

/// Derives one reconnect-stable journal identity from the opaque committed danger authority and
/// the run-local simulation identity. The route actor generation prevents ABA reuse, while the
/// lineage and restore root bind the identity to exactly one dangerous life.
#[must_use]
pub fn derive_private_route_death_entity_id(
    authority: &CorePrivateDangerEntryAuthority,
    entity_id: EntityId,
) -> [u8; 16] {
    let terminal = authority.terminal();
    let mut hasher = blake3::Hasher::new();
    hasher.update(PRIVATE_ROUTE_ENTITY_CONTEXT);
    hasher.update(terminal.account_id());
    hasher.update(terminal.character_id());
    hasher.update(terminal.lineage_id());
    hasher.update(terminal.restore_point_id());
    hasher.update(&authority.route_lease().actor_generation().to_le_bytes());
    hasher.update(&entity_id.get().to_le_bytes());
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    if identity == [0; 16] {
        identity[15] = 1;
    }
    identity
}

/// Seals every source and player target referenced by one committed damage frame. Repeated facts
/// reuse the same mapping; a collision across distinct simulation entities fails closed.
pub fn private_route_damage_entity_identities(
    authority: &CorePrivateDangerEntryAuthority,
    facts: &[CorePrivatePlayerDamageFactV1],
) -> Result<DeathEntityIdentityAuthority, CorePrivateDeathEntityIdentityError> {
    if facts.is_empty() {
        return Err(CorePrivateDeathEntityIdentityError::Empty);
    }
    let mut entities = BTreeSet::new();
    for fact in facts {
        entities.insert(fact.source_entity_id);
        entities.insert(fact.target_entity_id);
    }
    let mut durable = BTreeSet::new();
    let mut by_sim_entity = BTreeMap::new();
    for entity in entities {
        let identity = derive_private_route_death_entity_id(authority, entity);
        if !durable.insert(identity) {
            return Err(CorePrivateDeathEntityIdentityError::Collision);
        }
        by_sim_entity.insert(entity, identity);
    }
    Ok(DeathEntityIdentityAuthority { by_sim_entity })
}

#[cfg(test)]
mod tests {
    use protocol::{CorePrivateRouteContentRevisionV1, ManifestHash, WorldFlowContentRevisionV1};

    use super::*;
    use crate::{
        AccountId, AuthenticatedAccount, AuthenticatedNamespace, CorePrivateRouteActorDirectory,
        CorePrivateRouteActorPosition, CorePrivateRouteActorSeed,
        core_private_route_actor::CorePrivateRouteEnterMicrorealmTransition,
    };

    fn hash(byte: char) -> ManifestHash {
        ManifestHash::new(byte.to_string().repeat(64)).unwrap()
    }

    fn route_revision() -> CorePrivateRouteContentRevisionV1 {
        CorePrivateRouteContentRevisionV1 {
            records_blake3: hash('a'),
            assets_blake3: hash('b'),
            localization_blake3: hash('c'),
        }
    }

    fn world_revision() -> WorldFlowContentRevisionV1 {
        WorldFlowContentRevisionV1 {
            records_blake3: hash('d'),
            assets_blake3: hash('e'),
            localization_blake3: hash('f'),
        }
    }

    #[tokio::test]
    async fn danger_generation_derives_stable_distinct_entity_journal_ids() {
        let directory = CorePrivateRouteActorDirectory::new();
        let authenticated = AuthenticatedAccount {
            account_id: AccountId::new([1; 16]).unwrap(),
            namespace: AuthenticatedNamespace::WipeableTest,
        };
        let lease = directory
            .register_actor(
                authenticated,
                CorePrivateRouteActorSeed {
                    character_id: [2; 16],
                    character_version: 4,
                    content_revision: route_revision(),
                    world_flow_revision: world_revision(),
                    position: CorePrivateRouteActorPosition::hall(),
                },
                9,
            )
            .unwrap();
        directory
            .reconcile_enter_microrealm(
                lease,
                CorePrivateRouteEnterMicrorealmTransition {
                    transfer_id: [3; 16],
                    source_character_version: 4,
                    destination_character_version: 5,
                    instance_lineage_id: [4; 16],
                    entry_restore_point_id: [5; 16],
                    content_revision: world_revision(),
                },
            )
            .await
            .unwrap();
        let authority = directory.danger_entry_authority(lease).unwrap();
        let first = derive_private_route_death_entity_id(&authority, EntityId::new(41).unwrap());
        let replay = derive_private_route_death_entity_id(&authority, EntityId::new(41).unwrap());
        let second = derive_private_route_death_entity_id(&authority, EntityId::new(42).unwrap());
        assert_eq!(first, replay);
        assert_ne!(first, second);
        assert_ne!(first, [0; 16]);

        directory.begin_shutdown();
        assert!(directory.finish_shutdown().await.unwrap().zero_residue);
    }
}
