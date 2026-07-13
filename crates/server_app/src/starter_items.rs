pub use persistence::CORE_ITEM_CONTENT_REVISION;
use persistence::{STARTER_INITIALIZER_REVISION, StoredStarterInitialization, StoredStarterItem};
use sim_core::derive_starter_item_uid;
use thiserror::Error;

pub const STARTER_WEAPON_ID: &str = "item.weapon.crossbow.pine_crossbow";
pub const STARTER_RELIC_ID: &str = "item.relic.arbalist.cracked_mark_lens";
pub const STARTER_TONIC_ID: &str = "consumable.red_tonic";
const STARTER_REQUEST_CONTEXT: &str = "gravebound.starter-request.v1";
const STARTER_RESULT_CONTEXT: &str = "gravebound.starter-result.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterItemPlan {
    pub request_hash: [u8; 32],
    pub result_hash: [u8; 32],
    pub items: Vec<StoredStarterItem>,
}

#[derive(Debug, Error)]
pub enum StarterItemError {
    #[error("starter identity derivation failed")]
    Identity,
    #[error("starter persistence failed")]
    Persistence,
}

impl StarterItemPlan {
    pub fn for_character(character_id: [u8; 16]) -> Result<Self, StarterItemError> {
        let item_specs = [
            (
                STARTER_WEAPON_ID,
                0_i16,
                Some(1_i16),
                Some(0_i16),
                0_i32,
                0_i32,
                0_i16,
                0_i16,
            ),
            (STARTER_RELIC_ID, 0, Some(1), Some(0), 1, 0, 0, 1),
            (STARTER_TONIC_ID, 1, None, None, 2, 0, 1, 0),
            (STARTER_TONIC_ID, 1, None, None, 2, 1, 1, 0),
        ];
        let items = item_specs
            .into_iter()
            .map(
                |(
                    template_id,
                    item_kind,
                    item_level,
                    rarity,
                    roll_index,
                    unit_ordinal,
                    location_kind,
                    slot_index,
                )| {
                    let item_uid = derive_starter_item_uid(
                        character_id,
                        STARTER_INITIALIZER_REVISION,
                        template_id,
                        u16::try_from(unit_ordinal).map_err(|_| StarterItemError::Identity)?,
                    )
                    .map_err(|_| StarterItemError::Identity)?
                    .bytes();
                    Ok(StoredStarterItem {
                        item_uid,
                        // Creation-event identity shares the opaque item UID in its separate
                        // ledger namespace. Later transitions use their mutation identities.
                        ledger_event_id: item_uid,
                        template_id: template_id.to_owned(),
                        item_kind,
                        item_level,
                        rarity,
                        roll_index,
                        unit_ordinal,
                        location_kind,
                        slot_index,
                    })
                },
            )
            .collect::<Result<Vec<_>, StarterItemError>>()?;
        let request_hash = canonical_hash(
            STARTER_REQUEST_CONTEXT,
            &[
                character_id.as_slice(),
                STARTER_INITIALIZER_REVISION.as_bytes(),
                CORE_ITEM_CONTENT_REVISION.as_bytes(),
            ],
        )?;
        let mut result_material = vec![request_hash.to_vec()];
        for item in &items {
            result_material.extend([
                item.item_uid.to_vec(),
                item.template_id.as_bytes().to_vec(),
                item.roll_index.to_le_bytes().to_vec(),
                item.unit_ordinal.to_le_bytes().to_vec(),
                item.location_kind.to_le_bytes().to_vec(),
                item.slot_index.to_le_bytes().to_vec(),
            ]);
        }
        let result_fields = result_material
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>();
        let result_hash = canonical_hash(STARTER_RESULT_CONTEXT, &result_fields)?;
        Ok(Self {
            request_hash,
            result_hash,
            items,
        })
    }
}

pub async fn initialize_postgres_starter(
    persistence: &persistence::PostgresPersistence,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<StoredStarterInitialization, StarterItemError> {
    let plan = StarterItemPlan::for_character(character_id)?;
    persistence
        .initialize_starter_items(
            account_id,
            character_id,
            plan.request_hash,
            plan.result_hash,
            &plan.items,
        )
        .await
        .map_err(|_| StarterItemError::Persistence)
}

fn canonical_hash(context: &str, fields: &[&[u8]]) -> Result<[u8; 32], StarterItemError> {
    let mut bytes = Vec::new();
    for field in fields {
        let length = u32::try_from(field.len()).map_err(|_| StarterItemError::Identity)?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(field);
    }
    Ok(blake3::derive_key(context, &bytes))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn exact_starter_plan_has_distinct_units_and_stable_receipts() {
        let content = sim_content::load_core_development_items(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
        )
        .unwrap();
        assert_eq!(CORE_ITEM_CONTENT_REVISION, content.revision_label());
        let plan = StarterItemPlan::for_character([0x31; 16]).unwrap();
        assert_eq!(plan.items.len(), 4);
        assert_eq!(plan.items[0].template_id, STARTER_WEAPON_ID);
        assert_eq!(plan.items[1].template_id, STARTER_RELIC_ID);
        assert_eq!(plan.items[2].template_id, STARTER_TONIC_ID);
        assert_eq!(plan.items[3].template_id, STARTER_TONIC_ID);
        assert_ne!(plan.items[2].item_uid, plan.items[3].item_uid);
        assert_eq!(plan.items[2].slot_index, 0);
        assert_eq!(plan.items[3].slot_index, 0);
        assert_eq!(plan, StarterItemPlan::for_character([0x31; 16]).unwrap());
        assert_ne!(
            plan.result_hash,
            StarterItemPlan::for_character([0x32; 16])
                .unwrap()
                .result_hash
        );
    }
}
