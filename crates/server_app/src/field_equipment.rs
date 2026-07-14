use std::path::Path;

use persistence::{PostgresPersistence, StoredFieldEquipmentItem, StoredFieldEquipmentSnapshot};
use sim_content::{
    CompiledProductionItemCatalog, CoreEquipmentComparison, CoreEquipmentPresentation,
    compare_core_equipment, resolve_core_equipment_presentation,
};
use sim_core::{
    DurableEquipmentItem, DurableRunBackpackSlot, EquipmentRarity, FieldEquipmentPreview,
    FieldEquipmentSnapshot, FieldEquipmentSource, ItemUid, RUN_BACKPACK_CAPACITY,
    plan_field_equipment_swap,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldEquipmentPreviewSource {
    RunBackpack {
        slot_index: u8,
    },
    PersonalGround {
        item_uid: [u8; 16],
        pickup_id: [u8; 16],
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeFieldEquipmentPreview {
    pub mutation: FieldEquipmentPreview,
    pub incoming: CoreEquipmentPresentation,
    pub current: Option<CoreEquipmentPresentation>,
    pub comparison: CoreEquipmentComparison,
}

#[derive(Debug, Clone)]
pub struct PostgresFieldEquipmentService {
    persistence: PostgresPersistence,
    catalog: CompiledProductionItemCatalog,
}

impl PostgresFieldEquipmentService {
    pub fn load(
        persistence: PostgresPersistence,
        content_root: &Path,
    ) -> Result<Self, FieldEquipmentServiceError> {
        let catalog = sim_content::load_core_development_items(content_root)
            .map_err(|_| FieldEquipmentServiceError::Content)?;
        Ok(Self {
            persistence,
            catalog,
        })
    }

    pub async fn preview(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        source: FieldEquipmentPreviewSource,
        now_tick: u64,
    ) -> Result<AuthoritativeFieldEquipmentPreview, FieldEquipmentServiceError> {
        let stored = self
            .persistence
            .load_field_equipment_snapshot(account_id, character_id)
            .await
            .map_err(|_| FieldEquipmentServiceError::Persistence)?;
        let (snapshot, source) = self.project_snapshot(stored, source)?;
        let mutation =
            plan_field_equipment_swap(&snapshot, source, self.catalog.revision_label(), now_tick)?;
        let incoming = self.presentation(&mutation.incoming)?;
        let current = mutation
            .replaced
            .as_ref()
            .map(|item| self.presentation(item))
            .transpose()?;
        let comparison = compare_core_equipment(current.as_ref(), &incoming)
            .map_err(|_| FieldEquipmentServiceError::Content)?;
        Ok(AuthoritativeFieldEquipmentPreview {
            mutation,
            incoming,
            current,
            comparison,
        })
    }

    fn project_snapshot(
        &self,
        stored: StoredFieldEquipmentSnapshot,
        requested: FieldEquipmentPreviewSource,
    ) -> Result<(FieldEquipmentSnapshot, FieldEquipmentSource), FieldEquipmentServiceError> {
        let mut snapshot = FieldEquipmentSnapshot {
            inventory_version: stored.inventory_version,
            equipped: std::array::from_fn(|_| None),
            backpack: std::array::from_fn(|_| DurableRunBackpackSlot::Empty),
        };
        for index in stored.occupied_backpack_slots {
            snapshot.backpack[usize::from(index)] = DurableRunBackpackSlot::Consumable;
        }
        let mut ground_source = None;
        for item in stored.equipment {
            let durable = self.durable(&item)?;
            match item.location_kind {
                0 => {
                    let index = item
                        .slot_index
                        .ok_or(FieldEquipmentServiceError::CorruptSnapshot)?;
                    if usize::from(index) != durable.legal_slot.index()
                        || snapshot.equipped[usize::from(index)]
                            .replace(durable)
                            .is_some()
                    {
                        return Err(FieldEquipmentServiceError::CorruptSnapshot);
                    }
                }
                2 => {
                    let index = item
                        .slot_index
                        .ok_or(FieldEquipmentServiceError::CorruptSnapshot)?;
                    if matches!(
                        snapshot.backpack[usize::from(index)],
                        DurableRunBackpackSlot::Equipment(_)
                    ) {
                        return Err(FieldEquipmentServiceError::CorruptSnapshot);
                    }
                    snapshot.backpack[usize::from(index)] =
                        DurableRunBackpackSlot::Equipment(durable);
                }
                3 => {
                    if let FieldEquipmentPreviewSource::PersonalGround {
                        item_uid,
                        pickup_id,
                    } = requested
                        && item.item_uid == item_uid
                        && item.pickup_id == Some(pickup_id)
                    {
                        ground_source = Some(FieldEquipmentSource::PersonalGround {
                            item: durable,
                            pickup_id,
                            expires_at_tick: item
                                .expires_at_tick
                                .ok_or(FieldEquipmentServiceError::CorruptSnapshot)?,
                        });
                    }
                }
                _ => return Err(FieldEquipmentServiceError::CorruptSnapshot),
            }
        }
        let source = match requested {
            FieldEquipmentPreviewSource::RunBackpack { slot_index }
                if usize::from(slot_index) < RUN_BACKPACK_CAPACITY =>
            {
                FieldEquipmentSource::RunBackpack { slot_index }
            }
            FieldEquipmentPreviewSource::PersonalGround { .. } => {
                ground_source.ok_or(FieldEquipmentServiceError::SourceUnavailable)?
            }
            FieldEquipmentPreviewSource::RunBackpack { .. } => {
                return Err(FieldEquipmentServiceError::SourceUnavailable);
            }
        };
        Ok((snapshot, source))
    }

    fn durable(
        &self,
        item: &StoredFieldEquipmentItem,
    ) -> Result<DurableEquipmentItem, FieldEquipmentServiceError> {
        if item.content_revision != self.catalog.revision_label() {
            return Err(FieldEquipmentServiceError::ContentMismatch);
        }
        let rarity = match item.rarity {
            0 => EquipmentRarity::Worn,
            1 => EquipmentRarity::Forged,
            2 => EquipmentRarity::Oathed,
            3 => EquipmentRarity::Relic,
            4 => EquipmentRarity::Sainted,
            _ => return Err(FieldEquipmentServiceError::CorruptSnapshot),
        };
        let presentation = resolve_core_equipment_presentation(
            &self.catalog,
            &item.template_id,
            item.item_level,
            rarity,
        )
        .map_err(|_| FieldEquipmentServiceError::Content)?;
        Ok(DurableEquipmentItem {
            item_uid: ItemUid::new(item.item_uid)
                .map_err(|_| FieldEquipmentServiceError::CorruptSnapshot)?,
            template_id: item.template_id.clone(),
            legal_slot: presentation.slot,
            item_level: item.item_level,
            rarity,
            item_version: item.item_version,
        })
    }

    fn presentation(
        &self,
        item: &DurableEquipmentItem,
    ) -> Result<CoreEquipmentPresentation, FieldEquipmentServiceError> {
        resolve_core_equipment_presentation(
            &self.catalog,
            &item.template_id,
            item.item_level,
            item.rarity,
        )
        .map_err(|_| FieldEquipmentServiceError::Content)
    }
}

#[derive(Debug, Error)]
pub enum FieldEquipmentServiceError {
    #[error("Core equipment content failed strict validation")]
    Content,
    #[error("Core equipment content revision does not match the durable item")]
    ContentMismatch,
    #[error("field equipment persistence is unavailable")]
    Persistence,
    #[error("stored field equipment aggregate is corrupt")]
    CorruptSnapshot,
    #[error("requested field equipment source is unavailable")]
    SourceUnavailable,
    #[error(transparent)]
    Plan(#[from] sim_core::ItemLifecycleError),
}
