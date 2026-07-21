//! Sealed production planner from one lethal private-route frame to durable permadeath.
//!
//! Authority: the canonical GDD `DTH-001`, `DTH-020`, `ECH-001`, and `TECH-021..023`;
//! the Content Production Spec `CONT-ECHO-001`, `CONT-HUB-002`, and Core route records; and
//! roadmap `GB-M03-03`, `GB-M03-06`, and `GB-M03-13`. The planner accepts no client-authored
//! destination, destruction, cause, Echo, aggregate version, or identity material.

use std::{collections::BTreeSet, future::Future, sync::Arc};

use persistence::{
    DurableDeathContentAuthorityV1, DurableDeathItemContentAuthorityV1,
    DurableDestructionLocationV1, DurableEquipmentSlotV1, PersistenceError, PostgresPersistence,
    PrivateDeathPlanningRequestV1, StoredPrivateDeathDeedKindV1, StoredPrivateDeathEchoQueueV1,
    StoredPrivateDeathPlanningSnapshotV1,
};
use protocol::{CorePrivateRouteSceneV1, CorePrivateRouteStateV1};
use sim_content::{
    CompiledProductionItemCatalog, CoreDevelopmentDeathView, CorePrivateLifeContent,
};
use sim_core::{
    DEATH_AUTHORITY_SCHEMA_VERSION, DangerEntryClockSnapshot, DangerTerminalOutcome,
    DeathAuthorityError, DeathClockAggregate, DeathClockCheckpointV1, DeedAggregate,
    DeedCheckpointV1, DeedCompletionKind, RewardQualifiedDeed, Tick,
    compile_authoritative_death_inputs,
};
use thiserror::Error;

use crate::{
    AuthenticatedAccount, AuthenticatedNamespace, CorePrivateDangerEntryAuthority, DeathAtRiskItem,
    DeathAtRiskRunMaterial, DeathCustodySnapshot, DeathHeroSnapshot, DeathLineageState,
    DeathMutationAuthority, DeathProvenance, DeathWorldAuthority, DurableDeathBuildError,
    EchoAvailabilityProjection, EligibleEchoProjection, PreparedDurableDeathCommit,
    PreparedTerminalLiveDamageTrace, ServerAuthoredDeathContext, build_durable_death_commit,
};

const CORE_CLASS_ID: &str = "class.grave_arbalist";
const CORE_HERO_LABEL_KEY: &str = "hero.core.grave_arbalist";
const CORE_MEMORIAL_PRESENTATION_KEY: &str = "memorial.presentation.core_default";
const CORE_DANGER_CONTENT_ID: &str = "world.core_microrealm_01";
const CORE_PRIVATE_LAYOUT_ID: &str = "layout.core_private_life_01";
const CORE_ECHO_APPEARANCE_ID: &str = "appearance.default.grave_arbalist";
const CORE_ECHO_THEME_ID: &str = "theme.echo.arbalist_ash";

/// Exact process-owned authority presented by the private-route terminal owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateDeathPlanningAuthority {
    pub authenticated_account: AuthenticatedAccount,
    pub danger_entry: CorePrivateDangerEntryAuthority,
    pub route: CorePrivateRouteStateV1,
    pub terminal_trace: PreparedTerminalLiveDamageTrace,
    pub issued_at_unix_ms: u64,
}

/// Narrow read seam. The write authority remains exclusively in `transact_durable_death`.
pub trait PrivateDeathContextRepository: Send + Sync {
    fn load_death_planning_snapshot(
        &self,
        request: &PrivateDeathPlanningRequestV1,
    ) -> impl Future<Output = Result<StoredPrivateDeathPlanningSnapshotV1, PersistenceError>> + Send;
}

impl PrivateDeathContextRepository for PostgresPersistence {
    async fn load_death_planning_snapshot(
        &self,
        request: &PrivateDeathPlanningRequestV1,
    ) -> Result<StoredPrivateDeathPlanningSnapshotV1, PersistenceError> {
        self.load_private_death_planning_snapshot_v1(request).await
    }
}

/// Injected so exact retries and focused tests can control all `UUIDv7` identities.
pub trait DurableDeathIdentitySource: Send + Sync {
    fn next_uuid_v7(&self) -> [u8; 16];
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemDurableDeathIdentitySource;

impl DurableDeathIdentitySource for SystemDurableDeathIdentitySource {
    fn next_uuid_v7(&self) -> [u8; 16] {
        *uuid::Uuid::now_v7().as_bytes()
    }
}

#[derive(Debug, Error)]
pub enum PrivateDeathPlanningError {
    #[error("private death authority is stale, foreign, incomplete, or outside the Core route")]
    InvalidAuthority,
    #[error("private death content is missing or disagrees with durable Core authority")]
    ContentMismatch,
    #[error("private death identity allocation returned a duplicate or non-UUIDv7 value")]
    InvalidIdentity,
    #[error("private death snapshot could not be loaded")]
    Persistence(#[source] PersistenceError),
    #[error("private death clocks, deeds, or trace failed simulation validation")]
    Simulation(#[source] DeathAuthorityError),
    #[error("private death durable request could not be sealed")]
    Build(#[source] DurableDeathBuildError),
}

/// Process-wide immutable content plus the one coherent `PostgreSQL` read model.
pub struct PostgresPrivateDeathContextPlanner<Repository, Ids> {
    repository: Repository,
    ids: Ids,
    death_view: Arc<CoreDevelopmentDeathView>,
    items: Arc<CompiledProductionItemCatalog>,
    private_life_content: Arc<CorePrivateLifeContent>,
}

impl<Repository, Ids> PostgresPrivateDeathContextPlanner<Repository, Ids> {
    pub fn new(
        repository: Repository,
        ids: Ids,
        death_view: Arc<CoreDevelopmentDeathView>,
        items: Arc<CompiledProductionItemCatalog>,
        private_life_content: Arc<CorePrivateLifeContent>,
    ) -> Result<Self, PrivateDeathPlanningError> {
        let planner = Self {
            repository,
            ids,
            death_view,
            items,
            private_life_content,
        };
        planner.validate_content()?;
        Ok(planner)
    }

    fn validate_content(&self) -> Result<(), PrivateDeathPlanningError> {
        let world = persistence::LiveDamageTraceContentAuthorityV1::core();
        let view = persistence::DurableDeathPresentationAuthorityV1::core();
        if self.items.revision_label() != persistence::CORE_ITEM_CONTENT_REVISION
            || self.death_view.item_content_revision() != persistence::CORE_ITEM_CONTENT_REVISION
            || self.death_view.hashes().records_blake3 != view.records_blake3
            || self.death_view.hashes().assets_blake3 != view.assets_blake3
            || self.death_view.hashes().localization_blake3 != view.localization_blake3
            || self
                .private_life_content
                .world_flow()
                .hashes()
                .records_blake3
                != world.records_blake3
            || self
                .private_life_content
                .world_flow()
                .hashes()
                .assets_blake3
                != world.assets_blake3
            || self
                .private_life_content
                .world_flow()
                .hashes()
                .localization_blake3
                != world.localization_blake3
            || self.private_life_content.fixed_layout().id != CORE_PRIVATE_LAYOUT_ID
        {
            return Err(PrivateDeathPlanningError::ContentMismatch);
        }
        Ok(())
    }
}

impl<Repository, Ids> PostgresPrivateDeathContextPlanner<Repository, Ids>
where
    Repository: PrivateDeathContextRepository,
    Ids: DurableDeathIdentitySource,
{
    /// Produces the only value the terminal arbiter may submit to durable death execution.
    pub async fn plan(
        &self,
        authority: PrivateDeathPlanningAuthority,
    ) -> Result<PreparedDurableDeathCommit, PrivateDeathPlanningError> {
        self.validate_content()?;
        validate_live_authority(&authority)?;
        let terminal = authority.danger_entry.terminal();
        let command = &authority.terminal_trace.request().command;
        let request = PrivateDeathPlanningRequestV1 {
            account_id: *terminal.account_id(),
            character_id: *terminal.character_id(),
            lineage_id: *terminal.lineage_id(),
            restore_point_id: *terminal.restore_point_id(),
            expected_character_version: authority.route.character_version,
            death_tick: command.event_tick,
            records_blake3: command.content.records_blake3.clone(),
            assets_blake3: command.content.assets_blake3.clone(),
            localization_blake3: command.content.localization_blake3.clone(),
        };
        let snapshot = self
            .repository
            .load_death_planning_snapshot(&request)
            .await
            .map_err(PrivateDeathPlanningError::Persistence)?;
        validate_snapshot(&snapshot, &authority)?;

        let mut clocks = DeathClockAggregate::from_checkpoint(DeathClockCheckpointV1 {
            schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
            lifetime_ticks: snapshot.clock.lifetime_ticks,
            permadeath_combat_ticks: snapshot.clock.permadeath_combat_ticks,
            danger_entry: Some(DangerEntryClockSnapshot {
                permadeath_combat_ticks: snapshot.clock.danger_entry_permadeath_combat_ticks,
            }),
            link_lost_ticks: snapshot.clock.link_lost_ticks,
            dead: false,
        })
        .map_err(PrivateDeathPlanningError::Simulation)?;
        clocks
            .resolve_danger(DangerTerminalOutcome::Death)
            .map_err(PrivateDeathPlanningError::Simulation)?;
        let deeds = reconstruct_deeds(&snapshot)?;
        let inputs = compile_authoritative_death_inputs(
            &clocks,
            &deeds,
            authority.terminal_trace.aggregate(),
        )
        .map_err(PrivateDeathPlanningError::Simulation)?;

        let mutation_id = self.ids.next_uuid_v7();
        let death_id = self.ids.next_uuid_v7();
        if mutation_id == death_id || !is_uuid_v7(mutation_id) || !is_uuid_v7(death_id) {
            return Err(PrivateDeathPlanningError::InvalidIdentity);
        }
        let echo_eligible =
            snapshot.level >= 10 && inputs.clocks.echo_time_eligible && inputs.echo_deed_eligible;
        let echo = if echo_eligible {
            let echo_id = self.ids.next_uuid_v7();
            if !is_uuid_v7(echo_id) || [mutation_id, death_id].contains(&echo_id) {
                return Err(PrivateDeathPlanningError::InvalidIdentity);
            }
            Some(build_echo_projection(
                &snapshot, &inputs, echo_id, death_id,
            )?)
        } else {
            None
        };
        let context = ServerAuthoredDeathContext {
            mutation: DeathMutationAuthority {
                authenticated_account: authority.authenticated_account,
                selected_character_id: snapshot.character_id,
                former_roster_ordinal: snapshot.former_roster_ordinal,
                mutation_id,
                death_id,
                issued_at_unix_ms: authority.issued_at_unix_ms,
                accepted_at_unix_ms: authority.issued_at_unix_ms,
            },
            world: DeathWorldAuthority {
                // Capacity-one Core private lives use the immutable lineage as their durable
                // instance identity until later realm scheduling introduces a separate record.
                instance_id: *terminal.lineage_id(),
                lineage_id: *terminal.lineage_id(),
                restore_point_id: *terminal.restore_point_id(),
                region_id: authority.route.scene.location_id().to_owned(),
                room_id: route_room_id(&authority.route, &self.private_life_content)?,
                lineage_state: DeathLineageState::ActivePermadeath(
                    DeathProvenance::OrdinaryGameplay,
                ),
            },
            content: death_content_authority(&snapshot, &authority, &self.items)?,
            versions: snapshot.versions.clone(),
            custody: custody_snapshot(&snapshot),
            hero: DeathHeroSnapshot {
                hero_label_key: CORE_HERO_LABEL_KEY.to_owned(),
                character_name: format!("Hero {}", snapshot.former_roster_ordinal),
                class_id: snapshot.class_id.clone(),
                level: snapshot.level,
                oath_id: snapshot.oath_id.clone(),
                bargain_ids: snapshot.active_bargain_ids.clone(),
                memorial_presentation_key: CORE_MEMORIAL_PRESENTATION_KEY.to_owned(),
            },
            terminal_trace: authority.terminal_trace,
            echo,
        };
        build_durable_death_commit(&inputs, &context, &self.death_view)
            .map_err(PrivateDeathPlanningError::Build)
    }
}

fn validate_live_authority(
    authority: &PrivateDeathPlanningAuthority,
) -> Result<(), PrivateDeathPlanningError> {
    let terminal = authority.danger_entry.terminal();
    let command = &authority.terminal_trace.request().command;
    let world_revision = authority.danger_entry.world_flow_revision();
    if authority.authenticated_account.namespace != AuthenticatedNamespace::WipeableTest
        || authority.authenticated_account.account_id.as_bytes() != *terminal.account_id()
        || authority.route.validate().is_err()
        || authority.route.character_id != *terminal.character_id()
        || authority.route.actor_generation
            != authority.danger_entry.route_lease().actor_generation()
        || authority.route.content_revision != *authority.danger_entry.route_content_revision()
        || authority.route.instance_lineage_id != Some(*terminal.lineage_id())
        || authority.route.scene == CorePrivateRouteSceneV1::LanternHalls
        || authority.danger_entry.entry_character_version() > authority.route.character_version
        || command.account_id != *terminal.account_id()
        || command.character_id != *terminal.character_id()
        || command.expected_character_version != authority.route.character_version
        || command.danger.lineage_id != *terminal.lineage_id()
        || command.danger.restore_point_id != *terminal.restore_point_id()
        || command.content.records_blake3 != world_revision.records_blake3.as_str()
        || command.content.assets_blake3 != world_revision.assets_blake3.as_str()
        || command.content.localization_blake3 != world_revision.localization_blake3.as_str()
        || command.entries.last().is_none_or(|entry| !entry.lethal)
        || command.event_tick == 0
        || authority
            .terminal_trace
            .terminal_snapshot()
            .cause
            .lethal_entry
            .tick
            .0
            != command.event_tick
        || authority.issued_at_unix_ms < command.issued_at_unix_ms
        || i64::try_from(authority.issued_at_unix_ms).is_err()
    {
        return Err(PrivateDeathPlanningError::InvalidAuthority);
    }
    Ok(())
}

fn validate_snapshot(
    snapshot: &StoredPrivateDeathPlanningSnapshotV1,
    authority: &PrivateDeathPlanningAuthority,
) -> Result<(), PrivateDeathPlanningError> {
    let terminal = authority.danger_entry.terminal();
    if snapshot.account_id != *terminal.account_id()
        || snapshot.character_id != *terminal.character_id()
        || snapshot.class_id != CORE_CLASS_ID
        || snapshot.location_content_id != CORE_DANGER_CONTENT_ID
        || snapshot.lineage_content_id != CORE_DANGER_CONTENT_ID
        || snapshot.layout_id.as_deref() != Some(CORE_PRIVATE_LAYOUT_ID)
        || snapshot.content_revision != persistence::CORE_ITEM_CONTENT_REVISION
        || snapshot.versions.character.pre != authority.route.character_version
        || snapshot.clock.authoritative_tick
            != authority.terminal_trace.request().command.event_tick
    {
        return Err(PrivateDeathPlanningError::InvalidAuthority);
    }
    Ok(())
}

fn reconstruct_deeds(
    snapshot: &StoredPrivateDeathPlanningSnapshotV1,
) -> Result<DeedAggregate, PrivateDeathPlanningError> {
    let mut completions = snapshot
        .deeds
        .completions
        .iter()
        .map(|deed| RewardQualifiedDeed {
            completion_id: completion_id(deed.completion_id),
            deed_id: deed.deed_id.clone(),
            achieved_tick: Tick(deed.achieved_tick),
            kind: match deed.kind {
                StoredPrivateDeathDeedKindV1::DungeonBoss => DeedCompletionKind::DungeonBoss,
                StoredPrivateDeathDeedKindV1::MajorRealmEvent => {
                    DeedCompletionKind::MajorRealmEvent
                }
                StoredPrivateDeathDeedKindV1::FinalDeedOnly => DeedCompletionKind::FinalDeedOnly,
            },
        })
        .collect::<Vec<_>>();
    completions.sort_by(|left, right| left.completion_id.cmp(&right.completion_id));
    DeedAggregate::from_checkpoint(DeedCheckpointV1 {
        schema_version: DEATH_AUTHORITY_SCHEMA_VERSION,
        completions,
    })
    .map_err(PrivateDeathPlanningError::Simulation)
}

fn death_content_authority(
    snapshot: &StoredPrivateDeathPlanningSnapshotV1,
    authority: &PrivateDeathPlanningAuthority,
    items: &CompiledProductionItemCatalog,
) -> Result<DurableDeathContentAuthorityV1, PrivateDeathPlanningError> {
    let enabled_items = items
        .items()
        .values()
        .filter(|item| item.header.enabled)
        .map(|item| DurableDeathItemContentAuthorityV1 {
            template_id: item.header.id.as_str().to_owned(),
            // Core has no authored weapon/relic Echo signature mapping; the default silhouette
            // is therefore authoritative for every Core item.
            echo_signature_tag: None,
        })
        .collect::<Vec<_>>();
    let content = &authority.terminal_trace.request().command.content;
    let result = DurableDeathContentAuthorityV1 {
        content_revision: snapshot.content_revision.clone(),
        records_blake3: content.records_blake3.clone(),
        assets_blake3: content.assets_blake3.clone(),
        localization_blake3: content.localization_blake3.clone(),
        enabled_items,
    };
    result
        .validate()
        .map_err(|_| PrivateDeathPlanningError::ContentMismatch)?;
    Ok(result)
}

fn custody_snapshot(snapshot: &StoredPrivateDeathPlanningSnapshotV1) -> DeathCustodySnapshot {
    DeathCustodySnapshot {
        items: snapshot
            .custody_items
            .iter()
            .map(|item| DeathAtRiskItem {
                content_id: item.template_id.clone(),
                item_uid: item.item_uid,
                location: item.location.clone(),
                item_version: item.item_version,
            })
            .collect(),
        run_materials: snapshot
            .run_materials
            .iter()
            .map(|material| DeathAtRiskRunMaterial {
                material_id: material.material_id.clone(),
                quantity: material.quantity,
                material_version: material.material_version,
            })
            .collect(),
    }
}

fn route_room_id(
    route: &CorePrivateRouteStateV1,
    content: &CorePrivateLifeContent,
) -> Result<String, PrivateDeathPlanningError> {
    match route.scene {
        CorePrivateRouteSceneV1::CoreMicrorealm => Ok(CORE_DANGER_CONTENT_ID.to_owned()),
        CorePrivateRouteSceneV1::BellSepulcher => {
            let node_id = route
                .room
                .ok_or(PrivateDeathPlanningError::InvalidAuthority)?
                .node_id();
            content
                .fixed_layout()
                .rooms
                .iter()
                .find(|room| room.node_id == node_id)
                .map(|room| room.room.room_id.clone())
                .ok_or(PrivateDeathPlanningError::ContentMismatch)
        }
        CorePrivateRouteSceneV1::LanternHalls => Err(PrivateDeathPlanningError::InvalidAuthority),
    }
}

fn build_echo_projection(
    snapshot: &StoredPrivateDeathPlanningSnapshotV1,
    inputs: &sim_core::AuthoritativeDeathInputs,
    echo_id: [u8; 16],
    death_id: [u8; 16],
) -> Result<EligibleEchoProjection, PrivateDeathPlanningError> {
    let availability = match snapshot.echo_queue {
        StoredPrivateDeathEchoQueueV1::ExistingAvailable { echo_id } => {
            EchoAvailabilityProjection::ExistingAvailable { echo_id }
        }
        StoredPrivateDeathEchoQueueV1::PromoteOldestDormant {
            echo_id,
            death_id,
            next_transition_ordinal,
        } => EchoAvailabilityProjection::PromoteOldestDormant {
            echo_id,
            echo_death_id: death_id,
            next_transition_ordinal,
        },
        StoredPrivateDeathEchoQueueV1::PromoteNewEcho => {
            EchoAvailabilityProjection::PromoteOldestDormant {
                echo_id,
                echo_death_id: death_id,
                next_transition_ordinal: 1,
            }
        }
    };
    let deed_tags = snapshot
        .deeds
        .completions
        .iter()
        .map(|deed| deed.deed_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if !deed_tags.contains(&inputs.final_deed.deed_id) {
        return Err(PrivateDeathPlanningError::InvalidAuthority);
    }
    Ok(EligibleEchoProjection {
        echo_id,
        appearance_snapshot_id: CORE_ECHO_APPEARANCE_ID.to_owned(),
        appearance_theme_id: CORE_ECHO_THEME_ID.to_owned(),
        weapon_signature_tag: None,
        relic_signature_tag: None,
        deed_tags,
        power_band: echo_power_band(snapshot)?,
        availability,
    })
}

fn echo_power_band(
    snapshot: &StoredPrivateDeathPlanningSnapshotV1,
) -> Result<u8, PrivateDeathPlanningError> {
    echo_power_band_from_items(snapshot.level, &snapshot.custody_items)
}

fn echo_power_band_from_items(
    character_level: u8,
    items: &[persistence::StoredPrivateDeathCustodyItemV1],
) -> Result<u8, PrivateDeathPlanningError> {
    let mut effective_by_slot = [None; 4];
    for item in items {
        let DurableDestructionLocationV1::Equipment { slot } = item.location else {
            continue;
        };
        let index = match slot {
            DurableEquipmentSlotV1::Weapon => 0,
            DurableEquipmentSlotV1::Relic => 1,
            DurableEquipmentSlotV1::Armor => 2,
            DurableEquipmentSlotV1::Charm => 3,
        };
        let level = item
            .item_level
            .ok_or(PrivateDeathPlanningError::InvalidAuthority)?;
        let bonus = match item
            .rarity
            .ok_or(PrivateDeathPlanningError::InvalidAuthority)?
        {
            0 => 0_u32,
            1 => 5,
            2 => 10,
            3 => 20,
            4 | 5 => 30,
            _ => return Err(PrivateDeathPlanningError::InvalidAuthority),
        };
        let effective = u32::from(level) * 10 + bonus;
        if effective_by_slot[index].replace(effective).is_some() {
            return Err(PrivateDeathPlanningError::InvalidAuthority);
        }
    }
    let weighted = [35_u32, 25, 25, 15]
        .into_iter()
        .zip(effective_by_slot)
        .map(|(weight, effective)| weight * effective.unwrap_or(0))
        .sum::<u32>();
    let functional = (weighted + 50) / 100;
    let index = (u32::from(character_level) * 10 + functional).div_ceil(2);
    Ok(match index {
        0..=89 => 1,
        90..=119 => 2,
        120..=149 => 3,
        150..=179 => 4,
        _ => 5,
    })
}

fn completion_id(value: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(43);
    result.push_str("completion.");
    for byte in value {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

fn is_uuid_v7(value: [u8; 16]) -> bool {
    value != [0; 16] && (value[6] >> 4) == 7 && (value[8] & 0xc0) == 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equipped_items(
        item_level: u8,
        rarity: u8,
    ) -> Vec<persistence::StoredPrivateDeathCustodyItemV1> {
        [
            DurableEquipmentSlotV1::Weapon,
            DurableEquipmentSlotV1::Relic,
            DurableEquipmentSlotV1::Armor,
            DurableEquipmentSlotV1::Charm,
        ]
        .into_iter()
        .enumerate()
        .map(
            |(index, slot)| persistence::StoredPrivateDeathCustodyItemV1 {
                item_uid: [u8::try_from(index + 1).unwrap(); 16],
                template_id: format!("item.core.power_{index}"),
                content_revision: persistence::CORE_ITEM_CONTENT_REVISION.to_owned(),
                item_level: Some(item_level),
                rarity: Some(rarity),
                item_version: 1,
                location: DurableDestructionLocationV1::Equipment { slot },
            },
        )
        .collect()
    }

    #[test]
    fn completion_identity_is_stable_lowercase_and_domain_separated() {
        assert_eq!(
            completion_id([0xab; 16]),
            "completion.abababababababababababababababab"
        );
    }

    #[test]
    fn planner_echo_power_matches_exact_content_spec_boundaries() {
        assert_eq!(echo_power_band_from_items(10, &[]).unwrap(), 1);
        assert_eq!(
            echo_power_band_from_items(10, &equipped_items(8, 0)).unwrap(),
            2
        );
        assert_eq!(
            echo_power_band_from_items(10, &equipped_items(14, 0)).unwrap(),
            3
        );
        assert_eq!(
            echo_power_band_from_items(10, &equipped_items(20, 0)).unwrap(),
            4
        );
        assert_eq!(
            echo_power_band_from_items(20, &equipped_items(16, 5)).unwrap(),
            5
        );
    }
}
