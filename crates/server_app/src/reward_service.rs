use std::path::Path;

use content_schema::ProductionItemRarity;
use persistence::{
    PersistenceError, PostgresPersistence, RewardPlanningState, RewardTransaction,
    StoredActiveDangerAuthorityV1, StoredRewardCommit, StoredRewardEntry, StoredRewardItem,
    StoredRewardOutcome, StoredRewardRequest,
};
use serde::{Deserialize, Serialize};
use sim_content::{
    CompiledProductionItemCatalog, ProductionRewardPlanEntry, ProductionRewardPlanRequest,
};
use sim_core::{
    EquipmentPlacementPlan, RunBackpackSlot, derive_reward_item_uid,
    plan_consumable_reward_placement, plan_equipment_reward_placement,
};
use thiserror::Error;

use crate::{RewardSeedMaterial, SecretRewardEpoch};

const CORE_ITEM_POLICY_ID: &str = "policy.items.core";
const RED_TONIC_ID: &str = "consumable.red_tonic";
const PERSONAL_GROUND_LIFETIME_TICKS: u64 = 60 * 30;
const PERSONAL_GROUND_CONTEXT: &str = "gravebound.personal-ground.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardGrantContext<'a> {
    pub reward_request_id: [u8; 16],
    pub account_id: [u8; 16],
    pub character_id: [u8; 16],
    pub source_instance_id: [u8; 16],
    pub reward_table_id: &'a str,
    pub current_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewardGrantResult {
    pub reward_request_id: [u8; 16],
    pub epoch_id: String,
    pub plan_hash: [u8; 32],
    pub items: Vec<RewardGrantedItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewardGrantTransaction {
    Fresh {
        result: RewardGrantResult,
        durable: StoredRewardOutcome,
    },
    Replay {
        result: RewardGrantResult,
        durable: StoredRewardOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewardGrantedItem {
    pub item_uid: [u8; 16],
    pub roll_index: u16,
    pub unit_ordinal: u16,
    pub template_id: String,
    pub item_level: Option<u8>,
    pub rarity: Option<ProductionItemRarity>,
    pub placement: RewardPlacement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RewardPlacement {
    RunBackpack {
        slot_index: u8,
    },
    PersonalGround {
        pickup_id: [u8; 16],
        expires_at_tick: u64,
    },
}

#[derive(Debug, Clone)]
pub struct PostgresRewardService {
    persistence: PostgresPersistence,
    catalog: CompiledProductionItemCatalog,
    epoch: SecretRewardEpoch,
}

impl PostgresRewardService {
    pub fn load(
        persistence: PostgresPersistence,
        content_root: &Path,
        epoch: SecretRewardEpoch,
    ) -> Result<Self, RewardGrantError> {
        let catalog = sim_content::load_core_development_items(content_root)
            .map_err(|_| RewardGrantError::Content)?;
        Ok(Self {
            persistence,
            catalog,
            epoch,
        })
    }

    pub async fn grant(
        &self,
        context: RewardGrantContext<'_>,
    ) -> Result<RewardGrantTransaction, RewardGrantError> {
        self.grant_inner(context, None).await
    }

    pub async fn grant_in_active_danger(
        &self,
        context: RewardGrantContext<'_>,
        authority: StoredActiveDangerAuthorityV1,
    ) -> Result<RewardGrantTransaction, RewardGrantError> {
        self.grant_inner(context, Some(authority)).await
    }

    async fn grant_inner(
        &self,
        context: RewardGrantContext<'_>,
        authority: Option<StoredActiveDangerAuthorityV1>,
    ) -> Result<RewardGrantTransaction, RewardGrantError> {
        let seed_material = RewardSeedMaterial {
            reward_request_id: context.reward_request_id,
            character_id: context.character_id,
            source_instance_id: context.source_instance_id,
            reward_table_id: context.reward_table_id,
            content_revision: self.catalog.revision_label(),
        };
        let request = StoredRewardRequest {
            reward_request_id: context.reward_request_id,
            account_id: context.account_id,
            character_id: context.character_id,
            source_instance_id: context.source_instance_id,
            reward_table_id: context.reward_table_id.to_owned(),
            content_revision: self.catalog.revision_label().to_owned(),
            epoch_id: self.epoch.epoch_id().to_owned(),
            canonical_request_hash: seed_material
                .canonical_request_hash()
                .map_err(|_| RewardGrantError::InvalidRequest)?,
        };
        let planner = |state: &RewardPlanningState| {
            self.plan_fresh(&context, &seed_material, state)
                .map_err(|_| PersistenceError::RewardPlanningFailed)
        };
        let transaction = match authority {
            Some(authority) => {
                self.persistence
                    .transact_reward_in_active_danger(request, authority, planner)
                    .await?
            }
            None => self.persistence.transact_reward(request, planner).await?,
        };
        Ok(match transaction {
            RewardTransaction::Fresh {
                service_result,
                outcome,
            } => RewardGrantTransaction::Fresh {
                result: service_result,
                durable: outcome,
            },
            RewardTransaction::Replay(outcome) => RewardGrantTransaction::Replay {
                result: replay_result(&outcome)?,
                durable: outcome,
            },
        })
    }

    fn plan_fresh(
        &self,
        context: &RewardGrantContext<'_>,
        seed_material: &RewardSeedMaterial<'_>,
        state: &RewardPlanningState,
    ) -> Result<(RewardGrantResult, StoredRewardCommit), RewardGrantError> {
        plan_fresh_reward(&self.catalog, &self.epoch, context, seed_material, state)
    }
}

fn plan_fresh_reward(
    catalog: &CompiledProductionItemCatalog,
    epoch: &SecretRewardEpoch,
    context: &RewardGrantContext<'_>,
    seed_material: &RewardSeedMaterial<'_>,
    state: &RewardPlanningState,
) -> Result<(RewardGrantResult, StoredRewardCommit), RewardGrantError> {
    let mut rng = epoch
        .planner(seed_material)
        .map_err(|_| RewardGrantError::InvalidRequest)?;
    let plan = catalog
        .plan_reward(
            &ProductionRewardPlanRequest {
                reward_table_id: context.reward_table_id,
                stage_policy_id: CORE_ITEM_POLICY_ID,
                current_class_id: protocol::GRAVE_ARBALIST_CLASS_ID,
            },
            &mut rng,
        )
        .map_err(|_| RewardGrantError::Planning)?;
    let plan_hash = plan
        .canonical_hash()
        .map_err(|_| RewardGrantError::Planning)?;
    let mut slots = project_pending_slots(state)?;
    let expires_at_tick = context
        .current_tick
        .checked_add(PERSONAL_GROUND_LIFETIME_TICKS)
        .ok_or(RewardGrantError::InvalidRequest)?;
    let mut entries = Vec::new();
    let mut stored_items = Vec::new();
    let mut granted_items = Vec::new();
    for entry in &plan.entries {
        plan_entry(
            context,
            entry,
            expires_at_tick,
            &mut slots,
            &mut entries,
            &mut stored_items,
            &mut granted_items,
        )?;
    }
    let result = RewardGrantResult {
        reward_request_id: context.reward_request_id,
        epoch_id: epoch.epoch_id().to_owned(),
        plan_hash,
        items: granted_items,
    };
    let canonical_result = postcard::to_stdvec(&result).map_err(|_| RewardGrantError::Encoding)?;
    let result_hash = *blake3::hash(&canonical_result).as_bytes();
    let audit_digest = epoch
        .audit_digest(seed_material, &canonical_result)
        .map_err(|_| RewardGrantError::Encoding)?;
    Ok((
        result,
        StoredRewardCommit {
            plan_hash,
            result_hash,
            audit_digest,
            entries,
            items: stored_items,
        },
    ))
}

#[derive(Debug, Error)]
pub enum RewardGrantError {
    #[error("Core item content could not be loaded")]
    Content,
    #[error("reward request is invalid")]
    InvalidRequest,
    #[error("reward planning failed")]
    Planning,
    #[error("reward result encoding failed")]
    Encoding,
    #[error("reward persistence failed")]
    Persistence(#[from] PersistenceError),
}

fn project_pending_slots(
    state: &RewardPlanningState,
) -> Result<Vec<RunBackpackSlot>, RewardGrantError> {
    let mut slots = vec![RunBackpackSlot::Empty; sim_core::RUN_BACKPACK_CAPACITY];
    for item in &state.pending_items {
        let index = usize::try_from(item.slot_index).map_err(|_| RewardGrantError::Planning)?;
        let slot = slots.get_mut(index).ok_or(RewardGrantError::Planning)?;
        match item.item_kind {
            0 if matches!(slot, RunBackpackSlot::Empty) => *slot = RunBackpackSlot::Equipment,
            1 => match slot {
                RunBackpackSlot::Empty => {
                    *slot = RunBackpackSlot::Consumable {
                        template_id: item.template_id.clone(),
                        quantity: 1,
                    };
                }
                RunBackpackSlot::Consumable {
                    template_id,
                    quantity,
                } if *template_id == item.template_id && *quantity < 6 => *quantity += 1,
                _ => return Err(RewardGrantError::Planning),
            },
            _ => return Err(RewardGrantError::Planning),
        }
    }
    Ok(slots)
}

#[allow(clippy::too_many_arguments)]
fn plan_entry(
    context: &RewardGrantContext<'_>,
    entry: &ProductionRewardPlanEntry,
    expires_at_tick: u64,
    slots: &mut [RunBackpackSlot],
    entries: &mut Vec<StoredRewardEntry>,
    stored_items: &mut Vec<StoredRewardItem>,
    granted_items: &mut Vec<RewardGrantedItem>,
) -> Result<(), RewardGrantError> {
    match entry {
        ProductionRewardPlanEntry::Equipment {
            roll_index,
            template_id,
            item_level,
            rarity,
        } => {
            let placement = match plan_equipment_reward_placement(slots)
                .map_err(|_| RewardGrantError::Planning)?
            {
                EquipmentPlacementPlan::RunBackpack { slot_index } => {
                    slots[usize::from(slot_index)] = RunBackpackSlot::Equipment;
                    RewardPlacement::RunBackpack { slot_index }
                }
                EquipmentPlacementPlan::PersonalGround => RewardPlacement::PersonalGround {
                    pickup_id: derive_pickup_id(context.reward_request_id, *roll_index)?,
                    expires_at_tick,
                },
            };
            entries.push(stored_entry(entry));
            push_unit(
                context,
                *roll_index,
                0,
                template_id,
                Some(*item_level),
                Some(*rarity),
                placement,
                stored_items,
                granted_items,
            )?;
        }
        ProductionRewardPlanEntry::Material {
            roll_index,
            item_id,
            quantity,
        } => {
            if item_id != RED_TONIC_ID {
                return Err(RewardGrantError::Planning);
            }
            entries.push(stored_entry(entry));
            let placement = plan_consumable_reward_placement(slots, item_id, *quantity)
                .map_err(|_| RewardGrantError::Planning)?;
            let mut ordinal = 0_u16;
            for allocation in placement.backpack {
                apply_consumable_allocation(
                    slots,
                    item_id,
                    allocation.slot_index,
                    allocation.quantity,
                )?;
                for _ in 0..allocation.quantity {
                    push_unit(
                        context,
                        *roll_index,
                        ordinal,
                        item_id,
                        None,
                        None,
                        RewardPlacement::RunBackpack {
                            slot_index: allocation.slot_index,
                        },
                        stored_items,
                        granted_items,
                    )?;
                    ordinal += 1;
                }
            }
            if placement.personal_ground_quantity > 0 {
                let pickup_id = derive_pickup_id(context.reward_request_id, *roll_index)?;
                for _ in 0..placement.personal_ground_quantity {
                    push_unit(
                        context,
                        *roll_index,
                        ordinal,
                        item_id,
                        None,
                        None,
                        RewardPlacement::PersonalGround {
                            pickup_id,
                            expires_at_tick,
                        },
                        stored_items,
                        granted_items,
                    )?;
                    ordinal += 1;
                }
            }
        }
    }
    Ok(())
}

fn stored_entry(entry: &ProductionRewardPlanEntry) -> StoredRewardEntry {
    match entry {
        ProductionRewardPlanEntry::Equipment {
            roll_index,
            template_id,
            item_level,
            rarity,
        } => StoredRewardEntry {
            roll_index: i32::from(*roll_index),
            template_id: template_id.clone(),
            item_kind: 0,
            quantity: 1,
            item_level: Some(i16::from(*item_level)),
            rarity: Some(i16::from(rarity_index(*rarity))),
        },
        ProductionRewardPlanEntry::Material {
            roll_index,
            item_id,
            quantity,
        } => StoredRewardEntry {
            roll_index: i32::from(*roll_index),
            template_id: item_id.clone(),
            item_kind: 1,
            quantity: i16::try_from(*quantity).unwrap_or(i16::MAX),
            item_level: None,
            rarity: None,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn push_unit(
    context: &RewardGrantContext<'_>,
    roll_index: u16,
    unit_ordinal: u16,
    template_id: &str,
    item_level: Option<u8>,
    rarity: Option<ProductionItemRarity>,
    placement: RewardPlacement,
    stored_items: &mut Vec<StoredRewardItem>,
    granted_items: &mut Vec<RewardGrantedItem>,
) -> Result<(), RewardGrantError> {
    let item_uid = derive_reward_item_uid(context.reward_request_id, roll_index, unit_ordinal)
        .map_err(|_| RewardGrantError::Planning)?
        .bytes();
    let (location_kind, slot_index, instance_id, pickup_id, expires_at_tick) = match placement {
        RewardPlacement::RunBackpack { slot_index } => {
            (2, Some(i16::from(slot_index)), None, None, None)
        }
        RewardPlacement::PersonalGround {
            pickup_id,
            expires_at_tick,
        } => (
            3,
            None,
            Some(context.source_instance_id),
            Some(pickup_id),
            Some(i64::try_from(expires_at_tick).map_err(|_| RewardGrantError::Planning)?),
        ),
    };
    let (salvage_band, salvage_value) = item_level.map_or((0, 0), salvage_for_level);
    stored_items.push(StoredRewardItem {
        item_uid,
        ledger_event_id: item_uid,
        roll_index: i32::from(roll_index),
        unit_ordinal: i32::from(unit_ordinal),
        template_id: template_id.to_owned(),
        item_kind: i16::from(item_level.is_none()),
        item_level: item_level.map(i16::from),
        rarity: rarity.map(|value| i16::from(rarity_index(value))),
        location_kind,
        slot_index,
        instance_id,
        pickup_id,
        expires_at_tick,
        provenance_kind: 1,
        salvage_band,
        salvage_value,
    });
    granted_items.push(RewardGrantedItem {
        item_uid,
        roll_index,
        unit_ordinal,
        template_id: template_id.to_owned(),
        item_level,
        rarity,
        placement,
    });
    Ok(())
}

fn apply_consumable_allocation(
    slots: &mut [RunBackpackSlot],
    template_id: &str,
    slot_index: u8,
    added: u16,
) -> Result<(), RewardGrantError> {
    let slot = slots
        .get_mut(usize::from(slot_index))
        .ok_or(RewardGrantError::Planning)?;
    match slot {
        RunBackpackSlot::Empty => {
            *slot = RunBackpackSlot::Consumable {
                template_id: template_id.to_owned(),
                quantity: added,
            };
        }
        RunBackpackSlot::Consumable {
            template_id: stored,
            quantity,
        } if stored == template_id => {
            *quantity = quantity
                .checked_add(added)
                .filter(|value| *value <= 6)
                .ok_or(RewardGrantError::Planning)?;
        }
        _ => return Err(RewardGrantError::Planning),
    }
    Ok(())
}

const fn salvage_for_level(level: u8) -> (i16, i32) {
    match level {
        1..=6 => (1, 4),
        7..=13 => (2, 12),
        14..=20 => (3, 36),
        _ => (0, 0),
    }
}

const fn rarity_index(rarity: ProductionItemRarity) -> u8 {
    match rarity {
        ProductionItemRarity::Worn => 0,
        ProductionItemRarity::Forged => 1,
        ProductionItemRarity::Oathed => 2,
        ProductionItemRarity::Relic => 3,
        ProductionItemRarity::Sainted => 4,
        ProductionItemRarity::BlackUnique => 5,
    }
}

fn derive_pickup_id(request_id: [u8; 16], roll_index: u16) -> Result<[u8; 16], RewardGrantError> {
    let mut material = Vec::new();
    let roll_bytes = roll_index.to_le_bytes();
    for field in [request_id.as_slice(), roll_bytes.as_slice()] {
        let length = u32::try_from(field.len()).map_err(|_| RewardGrantError::Encoding)?;
        material.extend_from_slice(&length.to_le_bytes());
        material.extend_from_slice(field);
    }
    let derived = blake3::derive_key(PERSONAL_GROUND_CONTEXT, &material);
    let mut pickup_id = [0; 16];
    pickup_id.copy_from_slice(&derived[..16]);
    if pickup_id == [0; 16] {
        return Err(RewardGrantError::Encoding);
    }
    Ok(pickup_id)
}

pub fn replay_result(outcome: &StoredRewardOutcome) -> Result<RewardGrantResult, RewardGrantError> {
    let items = outcome
        .items
        .iter()
        .map(|item| {
            let placement = match item.location_kind {
                2 => RewardPlacement::RunBackpack {
                    slot_index: u8::try_from(item.slot_index.ok_or(RewardGrantError::Encoding)?)
                        .map_err(|_| RewardGrantError::Encoding)?,
                },
                3 => RewardPlacement::PersonalGround {
                    pickup_id: item.pickup_id.ok_or(RewardGrantError::Encoding)?,
                    expires_at_tick: u64::try_from(
                        item.expires_at_tick.ok_or(RewardGrantError::Encoding)?,
                    )
                    .map_err(|_| RewardGrantError::Encoding)?,
                },
                _ => return Err(RewardGrantError::Encoding),
            };
            Ok(RewardGrantedItem {
                item_uid: item.item_uid,
                roll_index: u16::try_from(item.roll_index)
                    .map_err(|_| RewardGrantError::Encoding)?,
                unit_ordinal: u16::try_from(item.unit_ordinal)
                    .map_err(|_| RewardGrantError::Encoding)?,
                template_id: item.template_id.clone(),
                item_level: item
                    .item_level
                    .map(u8::try_from)
                    .transpose()
                    .map_err(|_| RewardGrantError::Encoding)?,
                rarity: item.rarity.map(rarity_from_index).transpose()?,
                placement,
            })
        })
        .collect::<Result<Vec<_>, RewardGrantError>>()?;
    let result = RewardGrantResult {
        reward_request_id: outcome.reward_request_id,
        epoch_id: outcome.epoch_id.clone(),
        plan_hash: outcome.plan_hash,
        items,
    };
    let canonical = postcard::to_stdvec(&result).map_err(|_| RewardGrantError::Encoding)?;
    if *blake3::hash(&canonical).as_bytes() != outcome.result_hash {
        return Err(RewardGrantError::Encoding);
    }
    Ok(result)
}

const fn rarity_from_index(index: i16) -> Result<ProductionItemRarity, RewardGrantError> {
    match index {
        0 => Ok(ProductionItemRarity::Worn),
        1 => Ok(ProductionItemRarity::Forged),
        2 => Ok(ProductionItemRarity::Oathed),
        3 => Ok(ProductionItemRarity::Relic),
        4 => Ok(ProductionItemRarity::Sainted),
        5 => Ok(ProductionItemRarity::BlackUnique),
        _ => Err(RewardGrantError::Encoding),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    #[test]
    fn pending_projection_rejects_mixed_or_overfull_stacks() {
        let state = RewardPlanningState {
            inventory_version: 2,
            pending_items: vec![
                persistence::StoredPendingItem {
                    item_uid: [1; 16],
                    template_id: RED_TONIC_ID.to_owned(),
                    item_kind: 1,
                    slot_index: 0,
                },
                persistence::StoredPendingItem {
                    item_uid: [2; 16],
                    template_id: "consumable.other".to_owned(),
                    item_kind: 1,
                    slot_index: 0,
                },
            ],
        };
        assert!(project_pending_slots(&state).is_err());
    }

    #[test]
    fn salvage_and_ground_identity_follow_exact_contracts() {
        assert_eq!(salvage_for_level(1), (1, 4));
        assert_eq!(salvage_for_level(7), (2, 12));
        let pickup = derive_pickup_id([1; 16], 2).unwrap();
        assert_eq!(pickup, derive_pickup_id([1; 16], 2).unwrap());
        assert_ne!(pickup, derive_pickup_id([1; 16], 3).unwrap());
    }

    #[test]
    fn caldus_plan_is_deterministic_and_full_backpack_places_every_unit_on_ground() {
        let catalog = sim_content::load_core_development_items(&content_root()).unwrap();
        let epoch = SecretRewardEpoch::new("test-epoch", [0x5a; 32]).unwrap();
        let context = RewardGrantContext {
            reward_request_id: [11; 16],
            account_id: [12; 16],
            character_id: [13; 16],
            source_instance_id: [14; 16],
            reward_table_id: "reward.boss_caldus",
            current_tick: 900,
        };
        let seed = RewardSeedMaterial {
            reward_request_id: context.reward_request_id,
            character_id: context.character_id,
            source_instance_id: context.source_instance_id,
            reward_table_id: context.reward_table_id,
            content_revision: catalog.revision_label(),
        };
        let state = RewardPlanningState {
            inventory_version: 7,
            pending_items: (0_u8..8)
                .map(|slot| persistence::StoredPendingItem {
                    item_uid: [slot + 1; 16],
                    template_id: format!("item.fixture.{slot}"),
                    item_kind: 0,
                    slot_index: i16::from(slot),
                })
                .collect(),
        };
        let first = plan_fresh_reward(&catalog, &epoch, &context, &seed, &state).unwrap();
        let second = plan_fresh_reward(&catalog, &epoch, &context, &seed, &state).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.0.items.len(), 4);
        assert!(first.0.items.iter().all(|item| matches!(
            item.placement,
            RewardPlacement::PersonalGround {
                expires_at_tick: 2_700,
                ..
            }
        )));
        assert_eq!(
            first
                .0
                .items
                .iter()
                .map(|item| item.item_uid)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4
        );
        assert_eq!(first.1.items[0].salvage_band, 2);
        assert_eq!(first.1.items[0].salvage_value, 12);
    }
}
