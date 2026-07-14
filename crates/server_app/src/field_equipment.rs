use std::path::Path;

use persistence::{
    PostgresPersistence, StoredFieldEquipmentCommand, StoredFieldEquipmentItem,
    StoredFieldEquipmentResult, StoredFieldEquipmentSnapshot, StoredFieldEquipmentSource,
};
use protocol::{
    FieldEquipmentComparisonAxisV1, FieldEquipmentComparisonChangeV1,
    FieldEquipmentComparisonPreferenceV1, FieldEquipmentItemV1, FieldEquipmentPreviewProjectionV1,
    FieldEquipmentRarityV1, FieldEquipmentReplacementDestinationV1, FieldEquipmentSlotV1,
    FieldEquipmentSourceV1, WireText,
};
use sim_content::{
    CompiledProductionItemCatalog, CoreEquipmentAxis, CoreEquipmentAxisPreference,
    CoreEquipmentComparison, CoreEquipmentPresentation, compare_core_equipment,
    resolve_core_equipment_presentation,
};
use sim_core::{
    DurableEquipmentItem, DurableRunBackpackSlot, EquipmentRarity, EquipmentSlot,
    FieldEquipmentPreview, FieldEquipmentSnapshot, FieldEquipmentSource, ItemUid,
    RUN_BACKPACK_CAPACITY, ReplacementDestination, plan_field_equipment_swap,
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

impl AuthoritativeFieldEquipmentPreview {
    pub fn wire_projection(
        &self,
        character_id: [u8; 16],
    ) -> Result<FieldEquipmentPreviewProjectionV1, FieldEquipmentServiceError> {
        let current = self
            .mutation
            .replaced
            .as_ref()
            .zip(self.current.as_ref())
            .map(|(item, presentation)| wire_item(item, presentation))
            .transpose()?;
        let projection = FieldEquipmentPreviewProjectionV1 {
            character_id,
            inventory_version: self.mutation.inventory_version,
            content_revision: WireText::new(self.mutation.content_revision.clone())
                .map_err(|_| FieldEquipmentServiceError::ProtocolProjection)?,
            source: match self.mutation.source {
                FieldEquipmentSource::RunBackpack { slot_index } => {
                    FieldEquipmentSourceV1::RunBackpack { slot_index }
                }
                FieldEquipmentSource::PersonalGround {
                    ref item,
                    pickup_id,
                    ..
                } => FieldEquipmentSourceV1::PersonalGround {
                    item_uid: item.item_uid.bytes(),
                    pickup_id,
                },
            },
            incoming: wire_item(&self.mutation.incoming, &self.incoming)?,
            current,
            replacement_destination: match self.mutation.replacement_destination {
                ReplacementDestination::None => FieldEquipmentReplacementDestinationV1::None,
                ReplacementDestination::RunBackpack { slot_index } => {
                    FieldEquipmentReplacementDestinationV1::RunBackpack { slot_index }
                }
            },
            preview_hash: self.mutation.preview_hash,
            behavior_changed: self.comparison.behavior_changed,
            changes: self
                .comparison
                .changes
                .iter()
                .map(|change| FieldEquipmentComparisonChangeV1 {
                    axis: wire_axis(change.axis),
                    before: change.before,
                    after: change.after,
                    delta: change.delta,
                    preference: match change.preference {
                        CoreEquipmentAxisPreference::Higher => {
                            FieldEquipmentComparisonPreferenceV1::Higher
                        }
                        CoreEquipmentAxisPreference::Lower => {
                            FieldEquipmentComparisonPreferenceV1::Lower
                        }
                        CoreEquipmentAxisPreference::Contextual => {
                            FieldEquipmentComparisonPreferenceV1::Contextual
                        }
                    },
                    advanced: change.advanced,
                })
                .collect(),
        };
        projection
            .validate()
            .map_err(|_| FieldEquipmentServiceError::ProtocolProjection)?;
        Ok(projection)
    }
}

fn wire_item(
    item: &DurableEquipmentItem,
    presentation: &CoreEquipmentPresentation,
) -> Result<FieldEquipmentItemV1, FieldEquipmentServiceError> {
    Ok(FieldEquipmentItemV1 {
        item_uid: item.item_uid.bytes(),
        template_id: WireText::new(item.template_id.clone())
            .map_err(|_| FieldEquipmentServiceError::ProtocolProjection)?,
        slot: match item.legal_slot {
            EquipmentSlot::Weapon => FieldEquipmentSlotV1::Weapon,
            EquipmentSlot::Armor => FieldEquipmentSlotV1::Armor,
            EquipmentSlot::Relic => FieldEquipmentSlotV1::Relic,
            EquipmentSlot::Charm => FieldEquipmentSlotV1::Charm,
        },
        item_level: item.item_level,
        rarity: match item.rarity {
            EquipmentRarity::Worn => FieldEquipmentRarityV1::Worn,
            EquipmentRarity::Forged => FieldEquipmentRarityV1::Forged,
            EquipmentRarity::Oathed => FieldEquipmentRarityV1::Oathed,
            EquipmentRarity::Relic => FieldEquipmentRarityV1::Relic,
            EquipmentRarity::Sainted => FieldEquipmentRarityV1::Sainted,
            EquipmentRarity::BlackUnique => FieldEquipmentRarityV1::BlackUnique,
        },
        item_version: item.item_version,
        behavior_key: WireText::new(presentation.behavior_key.clone())
            .map_err(|_| FieldEquipmentServiceError::ProtocolProjection)?,
    })
}

const fn wire_axis(axis: CoreEquipmentAxis) -> FieldEquipmentComparisonAxisV1 {
    match axis {
        CoreEquipmentAxis::WeaponDamage => FieldEquipmentComparisonAxisV1::WeaponDamage,
        CoreEquipmentAxis::AttackIntervalMicros => {
            FieldEquipmentComparisonAxisV1::AttackIntervalMicros
        }
        CoreEquipmentAxis::RangeMilliTiles => FieldEquipmentComparisonAxisV1::RangeMilliTiles,
        CoreEquipmentAxis::ProjectileSpeedMilliTilesPerSecond => {
            FieldEquipmentComparisonAxisV1::ProjectileSpeedMilliTilesPerSecond
        }
        CoreEquipmentAxis::ProjectileRadiusMilliTiles => {
            FieldEquipmentComparisonAxisV1::ProjectileRadiusMilliTiles
        }
        CoreEquipmentAxis::BoltCount => FieldEquipmentComparisonAxisV1::BoltCount,
        CoreEquipmentAxis::PierceCount => FieldEquipmentComparisonAxisV1::PierceCount,
        CoreEquipmentAxis::MaximumHealth => FieldEquipmentComparisonAxisV1::MaximumHealth,
        CoreEquipmentAxis::Armor => FieldEquipmentComparisonAxisV1::Armor,
        CoreEquipmentAxis::ResistanceBasisPoints => {
            FieldEquipmentComparisonAxisV1::ResistanceBasisPoints
        }
        CoreEquipmentAxis::MovementBasisPoints => {
            FieldEquipmentComparisonAxisV1::MovementBasisPoints
        }
        CoreEquipmentAxis::HealingReceivedBasisPoints => {
            FieldEquipmentComparisonAxisV1::HealingReceivedBasisPoints
        }
        CoreEquipmentAxis::NegativeStatusReductionBasisPoints => {
            FieldEquipmentComparisonAxisV1::NegativeStatusReductionBasisPoints
        }
        CoreEquipmentAxis::DirectHitBarrierHealth => {
            FieldEquipmentComparisonAxisV1::DirectHitBarrierHealth
        }
        CoreEquipmentAxis::MarkDamageCoefficientBasisPoints => {
            FieldEquipmentComparisonAxisV1::MarkDamageCoefficientBasisPoints
        }
        CoreEquipmentAxis::MarkDurationMillis => FieldEquipmentComparisonAxisV1::MarkDurationMillis,
        CoreEquipmentAxis::MarkPrimaryBonusBasisPoints => {
            FieldEquipmentComparisonAxisV1::MarkPrimaryBonusBasisPoints
        }
        CoreEquipmentAxis::SlipstepDistanceMilliTiles => {
            FieldEquipmentComparisonAxisV1::SlipstepDistanceMilliTiles
        }
        CoreEquipmentAxis::SlipstepDurationMillis => {
            FieldEquipmentComparisonAxisV1::SlipstepDurationMillis
        }
        CoreEquipmentAxis::SlipstepDamageReductionBasisPoints => {
            FieldEquipmentComparisonAxisV1::SlipstepDamageReductionBasisPoints
        }
        CoreEquipmentAxis::SlipstepCooldownMillis => {
            FieldEquipmentComparisonAxisV1::SlipstepCooldownMillis
        }
        CoreEquipmentAxis::RestedPrimaryBonusBasisPoints => {
            FieldEquipmentComparisonAxisV1::RestedPrimaryBonusBasisPoints
        }
        CoreEquipmentAxis::RestedPrimaryIdleMillis => {
            FieldEquipmentComparisonAxisV1::RestedPrimaryIdleMillis
        }
        CoreEquipmentAxis::PotionHealingBasisPoints => {
            FieldEquipmentComparisonAxisV1::PotionHealingBasisPoints
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldEquipmentConfirmCommand {
    pub command_id: [u8; 16],
    pub source: FieldEquipmentPreviewSource,
    pub preview_hash: [u8; 32],
    pub now_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeFieldEquipmentCommit {
    pub result: StoredFieldEquipmentResult,
    pub comparison: Option<CoreEquipmentComparison>,
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

    pub async fn confirm(
        &self,
        account_id: [u8; 16],
        character_id: [u8; 16],
        command: FieldEquipmentConfirmCommand,
    ) -> Result<AuthoritativeFieldEquipmentCommit, FieldEquipmentServiceError> {
        let request_hash = confirmation_request_hash(
            account_id,
            character_id,
            command,
            self.catalog.revision_label(),
        )?;
        if let Some(result) = self
            .persistence
            .load_field_equipment_replay(account_id, character_id, command.command_id, request_hash)
            .await
            .map_err(|error| map_persistence(&error))?
        {
            return Ok(AuthoritativeFieldEquipmentCommit {
                result,
                comparison: None,
            });
        }
        let preview = self
            .preview(account_id, character_id, command.source, command.now_tick)
            .await?;
        if preview.mutation.preview_hash != command.preview_hash {
            return Err(FieldEquipmentServiceError::StalePreview);
        }
        let result_hash = confirmation_result_hash(request_hash, &preview.mutation)?;
        let stored = stored_command(command, request_hash, result_hash, &preview.mutation)?;
        let result = self
            .persistence
            .commit_field_equipment(account_id, character_id, &stored)
            .await
            .map_err(|error| map_persistence(&error))?;
        Ok(AuthoritativeFieldEquipmentCommit {
            result,
            comparison: Some(preview.comparison),
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
                            instance_id: item
                                .instance_id
                                .ok_or(FieldEquipmentServiceError::CorruptSnapshot)?,
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
    #[error("authoritative field equipment state could not be projected to the bounded protocol")]
    ProtocolProjection,
    #[error("requested field equipment source is unavailable")]
    SourceUnavailable,
    #[error("field equipment confirmation identity is invalid")]
    InvalidConfirmation,
    #[error("field equipment preview is stale or altered")]
    StalePreview,
    #[error("field equipment confirmation reused an identity with different material")]
    IdempotencyConflict,
    #[error("field equipment aggregate changed before confirmation")]
    VersionMismatch,
    #[error(transparent)]
    Plan(#[from] sim_core::ItemLifecycleError),
}

fn confirmation_request_hash(
    account_id: [u8; 16],
    character_id: [u8; 16],
    command: FieldEquipmentConfirmCommand,
    content_revision: &str,
) -> Result<[u8; 32], FieldEquipmentServiceError> {
    if command.command_id == [0; 16] || command.preview_hash == [0; 32] {
        return Err(FieldEquipmentServiceError::InvalidConfirmation);
    }
    let mut fields = vec![
        account_id.to_vec(),
        character_id.to_vec(),
        command.command_id.to_vec(),
        command.preview_hash.to_vec(),
        content_revision.as_bytes().to_vec(),
    ];
    match command.source {
        FieldEquipmentPreviewSource::RunBackpack { slot_index } => fields.push(vec![0, slot_index]),
        FieldEquipmentPreviewSource::PersonalGround {
            item_uid,
            pickup_id,
        } => {
            fields.push(vec![1]);
            fields.push(item_uid.to_vec());
            fields.push(pickup_id.to_vec());
        }
    }
    canonical_hash("gravebound.field-equipment-confirm.v1", &fields)
}

fn confirmation_result_hash(
    request_hash: [u8; 32],
    preview: &FieldEquipmentPreview,
) -> Result<[u8; 32], FieldEquipmentServiceError> {
    let mut fields = vec![
        request_hash.to_vec(),
        preview.incoming.item_uid.bytes().to_vec(),
        preview.incoming.item_version.to_le_bytes().to_vec(),
        vec![preview.incoming.legal_slot as u8],
    ];
    if let Some(replaced) = &preview.replaced {
        fields.push(replaced.item_uid.bytes().to_vec());
        fields.push(replaced.item_version.to_le_bytes().to_vec());
    }
    match preview.replacement_destination {
        ReplacementDestination::None => fields.push(vec![0]),
        ReplacementDestination::RunBackpack { slot_index } => fields.push(vec![1, slot_index]),
    }
    canonical_hash("gravebound.field-equipment-result.v1", &fields)
}

fn stored_command(
    command: FieldEquipmentConfirmCommand,
    canonical_request_hash: [u8; 32],
    result_hash: [u8; 32],
    preview: &FieldEquipmentPreview,
) -> Result<StoredFieldEquipmentCommand, FieldEquipmentServiceError> {
    let source = match &preview.source {
        FieldEquipmentSource::RunBackpack { slot_index } => {
            StoredFieldEquipmentSource::RunBackpack {
                slot_index: *slot_index,
            }
        }
        FieldEquipmentSource::PersonalGround {
            instance_id,
            pickup_id,
            ..
        } => StoredFieldEquipmentSource::PersonalGround {
            instance_id: *instance_id,
            pickup_id: *pickup_id,
        },
    };
    Ok(StoredFieldEquipmentCommand {
        command_id: command.command_id,
        canonical_request_hash,
        preview_hash: preview.preview_hash,
        result_hash,
        content_revision: preview.content_revision.clone(),
        expected_inventory_version: preview.inventory_version,
        incoming_item_uid: preview.incoming.item_uid.bytes(),
        incoming_item_version: preview.incoming.item_version,
        target_slot_index: u8::try_from(preview.incoming.legal_slot.index())
            .map_err(|_| FieldEquipmentServiceError::InvalidConfirmation)?,
        replaced_item_uid: preview.replaced.as_ref().map(|item| item.item_uid.bytes()),
        replaced_item_version: preview.replaced.as_ref().map(|item| item.item_version),
        source,
        replacement_slot_index: match preview.replacement_destination {
            ReplacementDestination::None => None,
            ReplacementDestination::RunBackpack { slot_index } => Some(slot_index),
        },
    })
}

fn canonical_hash(
    context: &str,
    fields: &[Vec<u8>],
) -> Result<[u8; 32], FieldEquipmentServiceError> {
    let mut material = Vec::new();
    for field in fields {
        let length = u32::try_from(field.len())
            .map_err(|_| FieldEquipmentServiceError::InvalidConfirmation)?;
        material.extend_from_slice(&length.to_le_bytes());
        material.extend_from_slice(field);
    }
    Ok(blake3::derive_key(context, &material))
}

fn map_persistence(error: &persistence::PersistenceError) -> FieldEquipmentServiceError {
    match error {
        persistence::PersistenceError::ItemIdempotencyConflict => {
            FieldEquipmentServiceError::IdempotencyConflict
        }
        persistence::PersistenceError::FieldEquipmentVersionMismatch
        | persistence::PersistenceError::FieldEquipmentBindingMismatch => {
            FieldEquipmentServiceError::VersionMismatch
        }
        _ => FieldEquipmentServiceError::Persistence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{EquipmentSlot, ReplacementDestination};

    fn command(source: FieldEquipmentPreviewSource) -> FieldEquipmentConfirmCommand {
        FieldEquipmentConfirmCommand {
            command_id: [1; 16],
            source,
            preview_hash: [2; 32],
            now_tick: 90,
        }
    }

    #[test]
    fn confirmation_hash_binds_owner_source_preview_and_revision() {
        let backpack = command(FieldEquipmentPreviewSource::RunBackpack { slot_index: 2 });
        let first =
            confirmation_request_hash([3; 16], [4; 16], backpack, "core-dev.blake3.test").unwrap();
        assert_eq!(
            first,
            confirmation_request_hash([3; 16], [4; 16], backpack, "core-dev.blake3.test").unwrap()
        );
        assert_ne!(
            first,
            confirmation_request_hash(
                [3; 16],
                [4; 16],
                command(FieldEquipmentPreviewSource::RunBackpack { slot_index: 3 }),
                "core-dev.blake3.test"
            )
            .unwrap()
        );
        assert_ne!(
            first,
            confirmation_request_hash([3; 16], [5; 16], backpack, "core-dev.blake3.test").unwrap()
        );
    }

    #[test]
    fn stored_confirmation_uses_only_authoritative_preview_destinations() {
        let incoming = DurableEquipmentItem {
            item_uid: ItemUid::new([5; 16]).unwrap(),
            template_id: "item.weapon.crossbow.grave_repeater".to_owned(),
            legal_slot: EquipmentSlot::Weapon,
            item_level: 4,
            rarity: EquipmentRarity::Forged,
            item_version: 2,
        };
        let preview = FieldEquipmentPreview {
            inventory_version: 7,
            content_revision: "core-dev.blake3.test".to_owned(),
            source: FieldEquipmentSource::RunBackpack { slot_index: 3 },
            incoming,
            replaced: None,
            replacement_destination: ReplacementDestination::None,
            preview_hash: [6; 32],
        };
        let stored = stored_command(
            command(FieldEquipmentPreviewSource::RunBackpack { slot_index: 3 }),
            [7; 32],
            [8; 32],
            &preview,
        )
        .unwrap();
        assert_eq!(stored.target_slot_index, 0);
        assert_eq!(stored.expected_inventory_version, 7);
        assert_eq!(stored.incoming_item_version, 2);
        assert_eq!(stored.replacement_slot_index, None);
    }
}
