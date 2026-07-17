pub use persistence::CORE_ITEM_CONTENT_REVISION;
use persistence::{
    STARTER_INITIALIZER_REVISION, StoredStarterInitialization, StoredStarterItem,
    canonical_starter_request_hash_v1, canonical_starter_result_hash_v1,
};
use sim_core::derive_starter_item_uid;
use thiserror::Error;

pub const STARTER_WEAPON_ID: &str = "item.weapon.crossbow.pine_crossbow";
pub const STARTER_RELIC_ID: &str = "item.relic.arbalist.cracked_mark_lens";
pub const STARTER_TONIC_ID: &str = "consumable.red_tonic";

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
        let request_hash = canonical_starter_request_hash_v1(character_id)
            .map_err(|_| StarterItemError::Identity)?;
        let result_hash = canonical_starter_result_hash_v1(request_hash, &items)
            .map_err(|_| StarterItemError::Identity)?;
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

    #[test]
    fn successor_hashes_match_the_append_only_protocol_contract() {
        let account_id = [0x41; 16];
        let death_id = [0x42; 16];
        let mutation_id = [0x43; 16];
        let successor_id =
            persistence::derive_successor_character_id_v1(account_id, death_id, mutation_id);
        let receipt_id = persistence::derive_successor_receipt_id_v1(
            account_id,
            death_id,
            mutation_id,
            successor_id,
        );
        let payload = protocol::SuccessorCreatePayloadV1 {
            death_id,
            content_revision: protocol::WireText::new(CORE_ITEM_CONTENT_REVISION).unwrap(),
        };
        let starter = StarterItemPlan::for_character(successor_id).unwrap();
        let request = persistence::SuccessorCreateRequestV1 {
            contract_version: persistence::SUCCESSOR_CONTRACT_VERSION_V1,
            namespace_id: persistence::WIPEABLE_CORE_NAMESPACE.to_owned(),
            account_id,
            mutation_id,
            death_id,
            successor_id,
            receipt_id,
            canonical_request_hash: payload.canonical_hash(),
            content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
            starter_request_hash: starter.request_hash,
            starter_result_hash: starter.result_hash,
            starter_items: starter.items,
        };
        request.validate().unwrap();
        assert_eq!(
            request.expected_request_hash().unwrap(),
            payload.canonical_hash()
        );

        let mut preset = persistence::DurableSuccessorPresetV1 {
            namespace_id: persistence::WIPEABLE_CORE_NAMESPACE.to_owned(),
            account_id,
            former_character_id: [0x44; 16],
            death_id,
            former_roster_ordinal: 1,
            class_id: persistence::CORE_SUCCESSOR_CLASS_ID.to_owned(),
            appearance_kind: persistence::SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
            base_silhouette_id: persistence::CORE_SUCCESSOR_BASE_SILHOUETTE_ID.to_owned(),
            content_revision: CORE_ITEM_CONTENT_REVISION.to_owned(),
            created_at_unix_ms: 1,
            preset_hash: [0; 32],
        };
        preset.preset_hash = preset.expected_hash().unwrap();
        let stored =
            persistence::StoredSuccessorResultV1::from_request(&request, &preset, 3).unwrap();
        let wire = protocol::StoredSuccessorResultV1 {
            mutation_id: stored.mutation_id,
            death_id: stored.death_id,
            successor_id: stored.successor_id,
            receipt_id: stored.receipt_id,
            former_roster_ordinal: stored.former_roster_ordinal,
            class_id: protocol::WireText::new(stored.class_id.clone()).unwrap(),
            appearance: protocol::SuccessorAppearanceSnapshotV1::CoreBaseSilhouette,
            starter_items: protocol::SuccessorStarterItemsV1 {
                weapon_uid: stored.starter_items.weapon_uid,
                relic_uid: stored.starter_items.relic_uid,
                tonic_unit_uids: stored.starter_items.tonic_unit_uids,
            },
            versions: protocol::SuccessorVersionVectorV1 {
                account: stored.versions.account,
                character: stored.versions.character,
                progression: stored.versions.progression,
                world: stored.versions.world,
                inventory: stored.versions.inventory,
                life_metrics: stored.versions.life_metrics,
                oath_bargain: stored.versions.oath_bargain,
            },
            content_revision: protocol::WireText::new(stored.content_revision.clone()).unwrap(),
            selected_character_id: stored.selected_character_id,
            result_hash: stored.result_hash,
        };
        wire.validate().unwrap();
        assert_eq!(wire.canonical_result_hash(), stored.result_hash);
    }
}
