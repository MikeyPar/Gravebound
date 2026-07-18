//! Server-owned bridge from coherent danger custody to protocol 1.19.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `LOOT-002`, `LOOT-010`,
//! `LOOT-033`, `LOOT-060`, and `TECH-015`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-REWARD-003` and the Sir Caldus exit contract; and
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`, `GB-M03-04`, and `GB-M03-08`.

use persistence::{
    StoredCurrentDangerExtractionSnapshotV1, StoredCurrentDangerPendingItemKindV1,
    StoredCurrentDangerPendingItemLocationV1,
};
use protocol::{
    CORE_PENDING_INVENTORY_SCHEMA_VERSION, CorePendingInventoryStateV1, CorePendingItemKindV1,
    CorePendingItemLocationV1, CorePendingItemV1, CorePendingMaterialV1, ManifestHash,
    TerminalExpectedVersionsV1, WireText, WorldFlowContentRevisionV1,
};
use thiserror::Error;

pub fn project_core_pending_inventory(
    snapshot: &StoredCurrentDangerExtractionSnapshotV1,
) -> Result<CorePendingInventoryStateV1, CorePendingInventoryProjectionError> {
    snapshot
        .validate()
        .map_err(CorePendingInventoryProjectionError::Persistence)?;
    if !snapshot.pending_materials.is_empty() {
        return Err(CorePendingInventoryProjectionError::CoreContentAuthority);
    }
    let state = CorePendingInventoryStateV1 {
        schema_version: CORE_PENDING_INVENTORY_SCHEMA_VERSION,
        character_id: snapshot.authority.character_id,
        instance_lineage_id: snapshot.authority.instance_lineage_id,
        entry_restore_point_id: snapshot.authority.entry_restore_point_id,
        location_content_id: WireText::new(&snapshot.location_content_id)
            .map_err(|_| CorePendingInventoryProjectionError::WireBounds)?,
        content_revision: WorldFlowContentRevisionV1 {
            records_blake3: manifest(&snapshot.content_revision.records_blake3)?,
            assets_blake3: manifest(&snapshot.content_revision.assets_blake3)?,
            localization_blake3: manifest(&snapshot.content_revision.localization_blake3)?,
        },
        expected_extraction_versions: TerminalExpectedVersionsV1 {
            account: snapshot.expected_versions.account,
            character: snapshot.expected_versions.character,
            world: snapshot.expected_versions.world,
            inventory: snapshot.expected_versions.inventory,
            life_clock: snapshot.expected_versions.life_metrics,
        },
        items: snapshot
            .pending_items
            .iter()
            .map(|item| {
                Ok(CorePendingItemV1 {
                    item_uid: item.item_uid,
                    template_id: WireText::new(&item.template_id)
                        .map_err(|_| CorePendingInventoryProjectionError::WireBounds)?,
                    kind: match item.kind {
                        StoredCurrentDangerPendingItemKindV1::Equipment => {
                            CorePendingItemKindV1::Equipment
                        }
                        StoredCurrentDangerPendingItemKindV1::Consumable => {
                            CorePendingItemKindV1::Consumable
                        }
                    },
                    item_version: item.item_version,
                    location: match item.location {
                        StoredCurrentDangerPendingItemLocationV1::RunBackpack(index) => {
                            CorePendingItemLocationV1::RunBackpack { index }
                        }
                        StoredCurrentDangerPendingItemLocationV1::PersonalGround {
                            instance_id,
                            pickup_id,
                            expires_at_tick,
                        } => CorePendingItemLocationV1::PersonalGround {
                            instance_id,
                            pickup_id,
                            expires_at_tick,
                        },
                    },
                })
            })
            .collect::<Result<Vec<_>, CorePendingInventoryProjectionError>>()?,
        materials: snapshot
            .pending_materials
            .iter()
            .map(|material| {
                Ok(CorePendingMaterialV1 {
                    material_id: WireText::new(&material.material_id)
                        .map_err(|_| CorePendingInventoryProjectionError::WireBounds)?,
                    quantity: material.quantity,
                    material_version: material.material_version,
                })
            })
            .collect::<Result<Vec<_>, CorePendingInventoryProjectionError>>()?,
    };
    state
        .validate()
        .map_err(CorePendingInventoryProjectionError::Protocol)?;
    Ok(state)
}

fn manifest(value: &str) -> Result<ManifestHash, CorePendingInventoryProjectionError> {
    ManifestHash::new(value).map_err(|_| CorePendingInventoryProjectionError::WireBounds)
}

#[derive(Debug, Error)]
pub enum CorePendingInventoryProjectionError {
    #[error("stored current-danger snapshot is invalid")]
    Persistence(#[source] persistence::PersistenceError),
    #[error("stored current-danger snapshot exceeds protocol bounds")]
    WireBounds,
    #[error("Core pending inventory contains content unavailable in the Core release stage")]
    CoreContentAuthority,
    #[error("projected current-danger snapshot is invalid")]
    Protocol(#[source] protocol::CorePendingInventoryValidationError),
}

#[cfg(test)]
mod tests {
    use persistence::{
        ProductionExtractionExpectedVersionsV1, StoredActiveDangerAuthorityV1,
        StoredCurrentDangerPendingItemV1, StoredCurrentDangerPendingMaterialV1,
        StoredWorldFlowRevisionV1,
    };

    use super::*;

    fn snapshot() -> StoredCurrentDangerExtractionSnapshotV1 {
        StoredCurrentDangerExtractionSnapshotV1 {
            schema_version: persistence::CURRENT_DANGER_EXTRACTION_SNAPSHOT_SCHEMA_VERSION_V1,
            authority: StoredActiveDangerAuthorityV1 {
                account_id: [1; 16],
                character_id: [2; 16],
                instance_lineage_id: [3; 16],
                entry_restore_point_id: [4; 16],
            },
            location_content_id: "dungeon.bell_sepulcher".to_owned(),
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "1".repeat(64),
                assets_blake3: "2".repeat(64),
                localization_blake3: "3".repeat(64),
            },
            expected_versions: ProductionExtractionExpectedVersionsV1 {
                account: 4,
                character: 5,
                world: 5,
                inventory: 6,
                life_metrics: 7,
            },
            pending_items: vec![StoredCurrentDangerPendingItemV1 {
                item_uid: [5; 16],
                template_id: "item.weapon.sword.bell_cleaver_caldus".to_owned(),
                kind: StoredCurrentDangerPendingItemKindV1::Equipment,
                item_version: 1,
                location: StoredCurrentDangerPendingItemLocationV1::RunBackpack(0),
            }],
            pending_materials: Vec::new(),
        }
    }

    #[test]
    fn coherent_storage_projects_without_reauthoring_versions_or_custody() {
        let projected = project_core_pending_inventory(&snapshot()).expect("projection");
        assert_eq!(projected.character_id, [2; 16]);
        assert_eq!(projected.expected_extraction_versions.inventory, 6);
        assert_eq!(projected.items[0].item_uid, [5; 16]);
        assert!(projected.materials.is_empty());
    }

    #[test]
    fn malformed_storage_never_becomes_wire_authority() {
        let mut snapshot = snapshot();
        snapshot.pending_items[0].item_uid = [0; 16];
        assert!(matches!(
            project_core_pending_inventory(&snapshot),
            Err(CorePendingInventoryProjectionError::Persistence(_))
        ));
    }

    #[test]
    fn non_core_run_material_never_becomes_core_wire_authority() {
        let mut snapshot = snapshot();
        snapshot
            .pending_materials
            .push(StoredCurrentDangerPendingMaterialV1 {
                material_id: "material.bell_brass".to_owned(),
                quantity: 2,
                material_version: 1,
            });
        assert!(matches!(
            project_core_pending_inventory(&snapshot),
            Err(CorePendingInventoryProjectionError::CoreContentAuthority)
        ));
    }
}
