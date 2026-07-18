//! Terminal-first `PostgreSQL` bootstrap for the ordinary Core private-life runtime.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `TECH-015` and
//! `TECH-021`-`023`; `Gravebound_Content_Production_Spec_v1.md`
//! `CONT-ROOM-002`/`005`, `CONT-BOSS-001`, and `CONT-HUB-001`-`002`;
//! `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08` and the M03 restart gate;
//! plus accepted `ADR-037`.
//!
//! A process restart must never reconstruct live danger from a checkpoint. This reader observes
//! account selection, the complete selected-character heads, durable world/root ownership,
//! current terminal authority, and `ResolutionHold` under one serializable snapshot. The dedicated
//! restart resolver then invokes the existing atomic crash-restoration writer with a stable
//! mutation identity. A committed death, extraction, or Recall wins that race.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgConnection, Row};

use crate::danger_crash_restore::derived_identity;
use crate::durable_terminal_recovery::load_committed_death_terminal_v1_on;
use crate::extraction_terminal_recovery::load_committed_extraction_terminal_v1_on;
use crate::recall_terminal_recovery::load_committed_recall_terminal_v1_on;
use crate::resolution_hold_repository::load_resolution_hold_snapshot_v1_on;
use crate::world_flow::decode_location;
use crate::{
    DangerCrashRestoreCode, DangerCrashRestoreReceipt, DangerCrashRestoreRequest,
    DangerCrashRestoreTransaction, DangerCrashRestoreVersions, PersistenceError,
    PostgresPersistence, ProductionExtractionExpectedVersionsV1, StoredActiveDangerAuthorityV1,
    StoredCommittedDeathTerminalV1, StoredCommittedExtractionTerminalV1,
    StoredCommittedRecallTerminalV1, StoredResolutionHoldSnapshotV1, StoredSafeArrival,
    StoredWorldFlowRevisionV1, StoredWorldLocation, WIPEABLE_CORE_NAMESPACE,
    is_retryable_transaction_failure,
};

pub const PRIVATE_LIFE_BOOTSTRAP_SCHEMA_VERSION_V1: u16 = 1;
pub const PRIVATE_LIFE_HALL_ID_V1: &str = "hub.lantern_halls_01";
pub const PRIVATE_LIFE_LAYOUT_ID_V1: &str = "layout.core_private_life_01";
pub const PRIVATE_LIFE_CLASS_ID_V1: &str = "class.grave_arbalist";
pub const PRIVATE_LIFE_CHARACTER_SELECT_RETURN_SPAWN_ID_V1: &str =
    "spawn.hub.character_select_return";
pub const CURRENT_DANGER_EXTRACTION_SNAPSHOT_SCHEMA_VERSION_V1: u16 = 1;
pub const MAX_CURRENT_DANGER_PENDING_ITEMS_V1: usize = 64;
pub const MAX_CURRENT_DANGER_PENDING_MATERIALS_V1: usize = 4;

const MAX_TRANSACTION_ATTEMPTS: u8 = 8;
const LIFE_LIVING: i16 = 0;
const SECURITY_NORMAL: i16 = 0;
const SECURITY_STORAGE_RESOLUTION_REQUIRED: i16 = 1;
const RESTORE_ACTIVE: i16 = 0;
const LINEAGE_STAGED: i16 = 0;
const LINEAGE_ACTIVE: i16 = 1;
const LINEAGE_ACTIVE_U8: u8 = 1;
const CORE_RESTORE_CONTRACT_VERSION: i16 = 3;
const CORE_RESTORE_COMPONENT_MASK: i16 = 31;
const CRASH_MUTATION_CONTEXT_V1: &str = "gravebound.private-life-process-restart-crash-mutation.v1";
const ITEM_EQUIPMENT: i16 = 0;
const ITEM_CONSUMABLE: i16 = 1;
const SECURITY_AT_RISK_PENDING: i16 = 2;
const LOCATION_RUN_BACKPACK: i16 = 2;
const LOCATION_PERSONAL_GROUND: i16 = 3;
const CORE_RED_TONIC_ID: &str = "consumable.red_tonic";
const CORE_RED_TONIC_STACK_CAP: usize = 6;
const RUN_MATERIAL_STACK_CAP: u16 = 99;

const SELECTED_CHARACTER_SQL: &str =
    "SELECT character.class_id,character.level,character.life_state,
            character.security_state,character.character_state_version,
            world.character_version,world.location_kind,world.location_content_id,
            world.safe_arrival_kind,world.safe_spawn_id,world.instance_lineage_id,
            world.entry_restore_point_id,inventory.inventory_version,
            progression.progression_version,oath.oath_bargain_version,
            life.life_metrics_version,ash.wallet_version
     FROM characters AS character
     JOIN character_world_locations AS world
       ON world.namespace_id=character.namespace_id
      AND world.account_id=character.account_id
      AND world.character_id=character.character_id
     JOIN character_inventories AS inventory
       ON inventory.namespace_id=character.namespace_id
      AND inventory.account_id=character.account_id
      AND inventory.character_id=character.character_id
     JOIN character_progression AS progression
       ON progression.namespace_id=character.namespace_id
      AND progression.account_id=character.account_id
      AND progression.character_id=character.character_id
     JOIN character_oath_bargain_state AS oath
       ON oath.namespace_id=character.namespace_id
      AND oath.account_id=character.account_id
      AND oath.character_id=character.character_id
     JOIN character_life_metrics AS life
       ON life.namespace_id=character.namespace_id
      AND life.account_id=character.account_id
      AND life.character_id=character.character_id
     JOIN ash_wallets AS ash
       ON ash.namespace_id=character.namespace_id
      AND ash.account_id=character.account_id
     WHERE character.namespace_id=$1 AND character.account_id=$2
       AND character.character_id=$3
     FOR UPDATE OF character,world,inventory,progression,oath,life,ash";

const DANGER_ROOT_SQL: &str = "SELECT root.source_location_id,root.restore_location_id,
            root.snapshot_contract_version,root.account_version,root.character_version,
            root.progression_version,root.inventory_version,root.oath_bargain_version,
            root.life_metrics_version,root.ash_wallet_version,root.component_mask,
            root.composite_digest,root.restore_state,(root.consumed_at IS NULL) AS root_open,
            root.records_blake3,root.assets_blake3,root.localization_blake3,
            lineage.content_id,lineage.layout_id,lineage.lineage_state,
            (lineage.closed_at IS NULL) AS lineage_open,
            lineage.records_blake3 AS lineage_records_blake3,
            lineage.assets_blake3 AS lineage_assets_blake3,
            lineage.localization_blake3 AS lineage_localization_blake3
     FROM character_entry_restore_points AS root
     JOIN character_instance_lineages AS lineage
       ON lineage.namespace_id=root.namespace_id
      AND lineage.account_id=root.account_id
      AND lineage.character_id=root.character_id
      AND lineage.lineage_id=root.lineage_id
     WHERE root.namespace_id=$1 AND root.account_id=$2 AND root.character_id=$3
       AND root.lineage_id=$4 AND root.restore_point_id=$5
     LIMIT 2";

const CURRENT_SAFE_TERMINAL_SQL: &str =
    "SELECT 1::smallint AS terminal_kind,extraction_request_id AS request_id,
            extraction_receipt_id AS result_id
       FROM character_extraction_terminal_results_v1
      WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
        AND post_account_version=$4 AND post_character_version=$5
        AND post_world_version=$6 AND post_inventory_version=$7
        AND post_life_metrics_version=$8
     UNION ALL
     SELECT 2::smallint AS terminal_kind,mutation_id AS request_id,terminal_id AS result_id
       FROM character_recall_terminal_results_v1
      WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
        AND post_account_version=$4 AND post_character_version=$5
        AND post_world_version=$6 AND post_inventory_version=$7
        AND post_life_metrics_version=$8
        AND preserved_progression_version=$9
        AND preserved_oath_bargain_version=$10
        AND preserved_ash_wallet_version=$11";

const ACTIVE_SUCCESSOR_DEATH_SQL: &str =
    "SELECT reservation.death_id,preset.former_character_id AS character_id
       FROM successor_roster_reservations_v1 AS reservation
       JOIN death_successor_presets_v1 AS preset
         ON preset.namespace_id=reservation.namespace_id
        AND preset.account_id=reservation.account_id
        AND preset.death_id=reservation.death_id
        AND preset.former_roster_ordinal=reservation.former_roster_ordinal
       JOIN death_events AS death
         ON death.namespace_id=preset.namespace_id
        AND death.account_id=preset.account_id
        AND death.character_id=preset.former_character_id
        AND death.death_id=preset.death_id
      WHERE reservation.namespace_id=$1 AND reservation.account_id=$2
        AND reservation.reservation_state=0 AND death.death_provenance=0
      ORDER BY reservation.death_id
      LIMIT 2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredPrivateLifeLifeStateV1 {
    Living,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredPrivateLifeSecurityStateV1 {
    Normal,
    StorageResolutionRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredPrivateLifeVersionsV1 {
    pub account: u64,
    pub character: u64,
    pub world: u64,
    pub inventory: u64,
    pub progression: u64,
    pub oath_bargain: u64,
    pub life_metrics: u64,
    pub ash_wallet: u64,
}

impl StoredPrivateLifeVersionsV1 {
    fn validate(self) -> Result<(), PersistenceError> {
        if [
            self.account,
            self.character,
            self.world,
            self.inventory,
            self.progression,
            self.oath_bargain,
            self.life_metrics,
            self.ash_wallet,
        ]
        .contains(&0)
            || self.character != self.world
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateLifeSelectedCharacterV1 {
    pub character_id: [u8; 16],
    pub class_id: String,
    pub level: u8,
    pub life_state: StoredPrivateLifeLifeStateV1,
    pub security_state: StoredPrivateLifeSecurityStateV1,
    pub versions: StoredPrivateLifeVersionsV1,
}

impl StoredPrivateLifeSelectedCharacterV1 {
    fn validate(&self, account_version: u64) -> Result<(), PersistenceError> {
        if self.character_id == [0; 16]
            || self.class_id != PRIVATE_LIFE_CLASS_ID_V1
            || !(1..=10).contains(&self.level)
            || self.versions.account != account_version
        {
            return Err(corrupt());
        }
        self.versions.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateLifeDangerRootV1 {
    pub location_content_id: String,
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub source_location_id: String,
    pub restore_location_id: String,
    pub layout_id: String,
    pub lineage_state: u8,
    pub entry_versions: StoredPrivateLifeVersionsV1,
    pub content_revision: StoredWorldFlowRevisionV1,
    pub composite_digest: [u8; 32],
}

impl StoredPrivateLifeDangerRootV1 {
    fn validate(&self) -> Result<(), PersistenceError> {
        if self.lineage_id == [0; 16]
            || self.restore_point_id == [0; 16]
            || self.location_content_id.is_empty()
            || self.source_location_id != PRIVATE_LIFE_HALL_ID_V1
            || self.restore_location_id != PRIVATE_LIFE_HALL_ID_V1
            || self.layout_id != PRIVATE_LIFE_LAYOUT_ID_V1
            || !matches!(self.lineage_state, 0 | 1)
            || self.composite_digest == [0; 32]
            || !valid_revision(&self.content_revision)
        {
            return Err(corrupt());
        }
        self.entry_versions.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateLifeHallV1 {
    pub character: StoredPrivateLifeSelectedCharacterV1,
    pub arrival: StoredSafeArrival,
    pub resolution_hold: StoredResolutionHoldSnapshotV1,
}

impl StoredPrivateLifeHallV1 {
    fn validate(&self, account_id: [u8; 16], account_version: u64) -> Result<(), PersistenceError> {
        self.character.validate(account_version)?;
        self.resolution_hold.validate()?;
        if !valid_core_hall_arrival(&self.arrival)
            || self.resolution_hold.account_id != account_id
            || self.resolution_hold.character_id != self.character.character_id
            || self.resolution_hold.versions.account != self.character.versions.account
            || self.resolution_hold.versions.character != self.character.versions.character
            || self.resolution_hold.versions.world != self.character.versions.world
            || self.resolution_hold.versions.inventory != self.character.versions.inventory
            || self.resolution_hold.storage_resolution_required
                != matches!(
                    self.character.security_state,
                    StoredPrivateLifeSecurityStateV1::StorageResolutionRequired
                )
        {
            return Err(corrupt());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredPrivateLifeBootstrapStateV1 {
    CharacterSelect {
        selected_character: Option<StoredPrivateLifeSelectedCharacterV1>,
        next_hall_arrival: Option<StoredSafeArrival>,
    },
    HallReady(StoredPrivateLifeHallV1),
    HallStorageResolutionRequired(StoredPrivateLifeHallV1),
    DangerRequiresCrashRestore {
        character: StoredPrivateLifeSelectedCharacterV1,
        danger: StoredPrivateLifeDangerRootV1,
    },
    DeathCommitted(Box<StoredCommittedDeathTerminalV1>),
    ExtractionCommitted {
        hall: StoredPrivateLifeHallV1,
        terminal: Box<StoredCommittedExtractionTerminalV1>,
    },
    RecallCommitted {
        hall: StoredPrivateLifeHallV1,
        terminal: Box<StoredCommittedRecallTerminalV1>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPrivateLifeBootstrapV1 {
    pub schema_version: u16,
    pub account_id: [u8; 16],
    pub account_version: u64,
    pub state: StoredPrivateLifeBootstrapStateV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredCurrentDangerPendingItemKindV1 {
    Equipment,
    Consumable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StoredCurrentDangerPendingItemLocationV1 {
    RunBackpack(u8),
    PersonalGround {
        instance_id: [u8; 16],
        pickup_id: [u8; 16],
        expires_at_tick: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCurrentDangerPendingItemV1 {
    pub item_uid: [u8; 16],
    pub template_id: String,
    pub kind: StoredCurrentDangerPendingItemKindV1,
    pub item_version: u64,
    pub location: StoredCurrentDangerPendingItemLocationV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCurrentDangerPendingMaterialV1 {
    pub material_id: String,
    pub quantity: u16,
    pub material_version: u64,
}

/// One coherent, production read-only view of the currently selected danger route.
///
/// This projection is intentionally loaded through the same terminal-first private-life bootstrap
/// path used by normal admission. It never invokes process-restart crash restoration. The
/// authority comes from `Gravebound_Production_GDD_v1_Canonical.md` `TECH-015`/`021`-`023` and
/// `LOOT-033`/`050`; `Gravebound_Content_Production_Spec_v1.md` Core private-life and Bell
/// Sepulcher/Caldus records; and `Gravebound_Development_Roadmap_v1.md` `GB-M03-03`/`08`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCurrentDangerExtractionSnapshotV1 {
    pub schema_version: u16,
    pub authority: StoredActiveDangerAuthorityV1,
    pub location_content_id: String,
    pub content_revision: StoredWorldFlowRevisionV1,
    pub expected_versions: ProductionExtractionExpectedVersionsV1,
    pub pending_items: Vec<StoredCurrentDangerPendingItemV1>,
    pub pending_materials: Vec<StoredCurrentDangerPendingMaterialV1>,
}

impl StoredCurrentDangerExtractionSnapshotV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        self.authority.validate()?;
        if self.schema_version != CURRENT_DANGER_EXTRACTION_SNAPSHOT_SCHEMA_VERSION_V1
            || self.location_content_id.is_empty()
            || !valid_revision(&self.content_revision)
            || self.pending_items.len() > MAX_CURRENT_DANGER_PENDING_ITEMS_V1
            || self.pending_materials.len() > MAX_CURRENT_DANGER_PENDING_MATERIALS_V1
            || [
                self.expected_versions.account,
                self.expected_versions.character,
                self.expected_versions.world,
                self.expected_versions.inventory,
                self.expected_versions.life_metrics,
            ]
            .contains(&0)
            || self.expected_versions.character != self.expected_versions.world
        {
            return Err(corrupt_current_danger_snapshot());
        }
        let mut item_uids = BTreeSet::new();
        let mut stacks: BTreeMap<
            StoredCurrentDangerPendingItemLocationV1,
            (StoredCurrentDangerPendingItemKindV1, &str, usize),
        > = BTreeMap::new();
        let mut previous_item_key = None;
        for item in &self.pending_items {
            let location_key = match item.location {
                StoredCurrentDangerPendingItemLocationV1::RunBackpack(slot) if slot < 8 => {
                    (0_u8, [0; 16], [0; 16], u64::from(slot))
                }
                StoredCurrentDangerPendingItemLocationV1::PersonalGround {
                    instance_id,
                    pickup_id,
                    expires_at_tick,
                } if instance_id != [0; 16] && pickup_id != [0; 16] && expires_at_tick > 0 => {
                    (1, instance_id, pickup_id, expires_at_tick)
                }
                _ => return Err(corrupt_current_danger_snapshot()),
            };
            let key = (location_key, item.item_uid);
            if item.item_uid == [0; 16]
                || !(3..=96).contains(&item.template_id.len())
                || item.item_version == 0
                || !item_uids.insert(item.item_uid)
                || previous_item_key.is_some_and(|previous| previous >= key)
            {
                return Err(corrupt_current_danger_snapshot());
            }
            let stack =
                stacks
                    .entry(item.location)
                    .or_insert((item.kind, item.template_id.as_str(), 0));
            stack.2 = stack.2.saturating_add(1);
            let valid_stack = match item.kind {
                StoredCurrentDangerPendingItemKindV1::Equipment => {
                    stack.0 == StoredCurrentDangerPendingItemKindV1::Equipment
                        && stack.1 == item.template_id.as_str()
                        && stack.2 == 1
                }
                StoredCurrentDangerPendingItemKindV1::Consumable => {
                    stack.0 == StoredCurrentDangerPendingItemKindV1::Consumable
                        && stack.1 == item.template_id.as_str()
                        && stack.1 == CORE_RED_TONIC_ID
                        && stack.2 <= CORE_RED_TONIC_STACK_CAP
                }
            };
            if !valid_stack {
                return Err(corrupt_current_danger_snapshot());
            }
            previous_item_key = Some(key);
        }
        let mut material_ids = BTreeSet::new();
        let mut previous_material_id: Option<&str> = None;
        for material in &self.pending_materials {
            if !(3..=96).contains(&material.material_id.len())
                || material.quantity == 0
                || material.quantity > RUN_MATERIAL_STACK_CAP
                || material.material_version == 0
                || !material_ids.insert(material.material_id.as_str())
                || previous_material_id
                    .is_some_and(|previous| previous >= material.material_id.as_str())
            {
                return Err(corrupt_current_danger_snapshot());
            }
            previous_material_id = Some(&material.material_id);
        }
        Ok(())
    }
}

impl StoredPrivateLifeBootstrapV1 {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.schema_version != PRIVATE_LIFE_BOOTSTRAP_SCHEMA_VERSION_V1
            || self.account_id == [0; 16]
            || self.account_version == 0
        {
            return Err(corrupt());
        }
        match &self.state {
            StoredPrivateLifeBootstrapStateV1::CharacterSelect {
                selected_character,
                next_hall_arrival,
            } => match (selected_character, next_hall_arrival) {
                (None, None) => Ok(()),
                (Some(character), Some(arrival)) => {
                    character.validate(self.account_version)?;
                    if character.security_state != StoredPrivateLifeSecurityStateV1::Normal
                        || !valid_core_hall_arrival(arrival)
                    {
                        return Err(corrupt());
                    }
                    Ok(())
                }
                _ => Err(corrupt()),
            },
            StoredPrivateLifeBootstrapStateV1::HallReady(hall) => {
                hall.validate(self.account_id, self.account_version)?;
                if hall.resolution_hold.storage_resolution_required {
                    return Err(corrupt());
                }
                Ok(())
            }
            StoredPrivateLifeBootstrapStateV1::HallStorageResolutionRequired(hall) => {
                hall.validate(self.account_id, self.account_version)?;
                if !hall.resolution_hold.storage_resolution_required {
                    return Err(corrupt());
                }
                Ok(())
            }
            StoredPrivateLifeBootstrapStateV1::DangerRequiresCrashRestore { character, danger } => {
                character.validate(self.account_version)?;
                danger.validate()?;
                if character.security_state != StoredPrivateLifeSecurityStateV1::Normal {
                    return Err(corrupt());
                }
                Ok(())
            }
            StoredPrivateLifeBootstrapStateV1::DeathCommitted(terminal) => {
                terminal.validate()?;
                if terminal.result.account_id != self.account_id {
                    return Err(corrupt());
                }
                Ok(())
            }
            StoredPrivateLifeBootstrapStateV1::ExtractionCommitted { hall, terminal } => {
                hall.validate(self.account_id, self.account_version)?;
                terminal.validate()?;
                if terminal.result.account_id != self.account_id
                    || terminal.result.character_id != hall.character.character_id
                    || hall.arrival != StoredSafeArrival::HallDefault
                {
                    return Err(corrupt());
                }
                Ok(())
            }
            StoredPrivateLifeBootstrapStateV1::RecallCommitted { hall, terminal } => {
                hall.validate(self.account_id, self.account_version)?;
                terminal.validate()?;
                if terminal.result.account_id != self.account_id
                    || terminal.result.character_id != hall.character.character_id
                    || !terminal.owns_current_hall
                    || hall.arrival != StoredSafeArrival::HallDefault
                {
                    return Err(corrupt());
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPrivateLifeProcessRestartV1 {
    pub bootstrap: StoredPrivateLifeBootstrapV1,
    pub crash_restore: Option<DangerCrashRestoreReceipt>,
}

impl PostgresPersistence {
    /// Loads one coherent bootstrap projection. It never repairs state or reconstructs danger.
    pub async fn load_private_life_bootstrap_v1(
        &self,
        account_id: [u8; 16],
    ) -> Result<StoredPrivateLifeBootstrapV1, PersistenceError> {
        if account_id == [0; 16] {
            return Err(corrupt());
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.load_private_life_bootstrap_once_v1(account_id).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded private-life bootstrap loop always returns")
    }

    async fn load_private_life_bootstrap_once_v1(
        &self,
        account_id: [u8; 16],
    ) -> Result<StoredPrivateLifeBootstrapV1, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let bootstrap =
            load_private_life_bootstrap_v1_on(transaction.connection(), account_id).await?;
        transaction.rollback().await?;
        Ok(bootstrap)
    }

    /// Loads exact post-mutation extraction versions and pending run custody without repairing or
    /// resuming danger. The expected authority and content revision are server-owned bindings.
    pub async fn load_current_danger_extraction_snapshot_v1(
        &self,
        authority: StoredActiveDangerAuthorityV1,
        expected_content_revision: &StoredWorldFlowRevisionV1,
    ) -> Result<StoredCurrentDangerExtractionSnapshotV1, PersistenceError> {
        authority.validate()?;
        if !valid_revision(expected_content_revision) {
            return Err(PersistenceError::CurrentDangerExtractionSnapshotContentMismatch);
        }
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self
                .load_current_danger_extraction_snapshot_once_v1(
                    authority,
                    expected_content_revision,
                )
                .await
            {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded current-danger snapshot loop always returns")
    }

    async fn load_current_danger_extraction_snapshot_once_v1(
        &self,
        authority: StoredActiveDangerAuthorityV1,
        expected_content_revision: &StoredWorldFlowRevisionV1,
    ) -> Result<StoredCurrentDangerExtractionSnapshotV1, PersistenceError> {
        let mut transaction = self.begin_transaction().await?;
        let bootstrap =
            load_private_life_bootstrap_v1_on(transaction.connection(), authority.account_id)
                .await?;
        let StoredPrivateLifeBootstrapStateV1::DangerRequiresCrashRestore { character, danger } =
            bootstrap.state
        else {
            return Err(PersistenceError::CurrentDangerExtractionSnapshotBindingMismatch);
        };
        if character.character_id != authority.character_id
            || danger.lineage_id != authority.instance_lineage_id
            || danger.restore_point_id != authority.entry_restore_point_id
            || danger.lineage_state != LINEAGE_ACTIVE_U8
        {
            return Err(PersistenceError::CurrentDangerExtractionSnapshotBindingMismatch);
        }
        if danger.content_revision != *expected_content_revision {
            return Err(PersistenceError::CurrentDangerExtractionSnapshotContentMismatch);
        }
        reject_current_danger_unresolved_reward_v1(
            transaction.connection(),
            authority.account_id,
            authority.character_id,
        )
        .await?;
        let pending_items = load_current_danger_pending_items_v1(
            transaction.connection(),
            authority.account_id,
            authority.character_id,
        )
        .await?;
        let pending_materials = load_current_danger_pending_materials_v1(
            transaction.connection(),
            authority.account_id,
            authority.character_id,
        )
        .await?;
        if !pending_materials.is_empty() {
            return Err(PersistenceError::CurrentDangerExtractionSnapshotContentMismatch);
        }
        let versions = character.versions;
        let snapshot = StoredCurrentDangerExtractionSnapshotV1 {
            schema_version: CURRENT_DANGER_EXTRACTION_SNAPSHOT_SCHEMA_VERSION_V1,
            authority,
            location_content_id: danger.location_content_id,
            content_revision: danger.content_revision,
            expected_versions: ProductionExtractionExpectedVersionsV1 {
                account: versions.account,
                character: versions.character,
                world: versions.world,
                inventory: versions.inventory,
                life_metrics: versions.life_metrics,
            },
            pending_items,
            pending_materials,
        };
        snapshot.validate()?;
        transaction.rollback().await?;
        Ok(snapshot)
    }

    /// Process-restart-only resolver. Within-process reconnect must reattach the retained actor
    /// generation and must not call this method.
    pub async fn resolve_private_life_process_restart_v1(
        &self,
        account_id: [u8; 16],
    ) -> Result<ResolvedPrivateLifeProcessRestartV1, PersistenceError> {
        let initial = self.load_private_life_bootstrap_v1(account_id).await?;
        let StoredPrivateLifeBootstrapStateV1::DangerRequiresCrashRestore { character, danger } =
            &initial.state
        else {
            return Ok(ResolvedPrivateLifeProcessRestartV1 {
                bootstrap: initial,
                crash_restore: None,
            });
        };
        let mutation_id = derive_private_life_crash_mutation_id_v1(
            account_id,
            character.character_id,
            danger.restore_point_id,
        )?;
        let mut request = DangerCrashRestoreRequest {
            account_id,
            character_id: character.character_id,
            restore_point_id: danger.restore_point_id,
            mutation_id,
            request_hash: [0; 32],
        };
        request.request_hash = request.expected_request_hash();
        let receipt = match self.transact_danger_crash_restore(&request).await? {
            DangerCrashRestoreTransaction::Fresh(receipt)
            | DangerCrashRestoreTransaction::Replayed(receipt) => receipt,
            DangerCrashRestoreTransaction::Conflict { .. } => return Err(corrupt()),
        };
        receipt.validate()?;
        if receipt.account_id != account_id
            || receipt.character_id != character.character_id
            || receipt.restore_point_id != danger.restore_point_id
            || receipt.request_mutation_id != mutation_id
            || receipt.request_hash != request.request_hash
        {
            return Err(corrupt());
        }
        let bootstrap = self.load_private_life_bootstrap_v1(account_id).await?;
        if !receipt_matches_bootstrap(&receipt, &bootstrap) {
            return Err(corrupt());
        }
        Ok(ResolvedPrivateLifeProcessRestartV1 {
            bootstrap,
            crash_restore: Some(receipt),
        })
    }
}

pub fn derive_private_life_crash_mutation_id_v1(
    account_id: [u8; 16],
    character_id: [u8; 16],
    restore_point_id: [u8; 16],
) -> Result<[u8; 16], PersistenceError> {
    if [account_id, character_id, restore_point_id].contains(&[0; 16]) {
        return Err(corrupt());
    }
    let mutation_id = derived_identity(
        CRASH_MUTATION_CONTEXT_V1,
        &[&account_id, &character_id, &restore_point_id],
    );
    if mutation_id == [0; 16] {
        return Err(corrupt());
    }
    Ok(mutation_id)
}

async fn load_private_life_bootstrap_v1_on(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<StoredPrivateLifeBootstrapV1, PersistenceError> {
    let (account_version, selected_character_id) =
        lock_bootstrap_account(connection, account_id).await?;
    let state = match selected_character_id {
        None => load_unselected_state(connection, account_id).await?,
        Some(character_id) => {
            load_selected_state(connection, account_id, account_version, character_id).await?
        }
    };
    let bootstrap = StoredPrivateLifeBootstrapV1 {
        schema_version: PRIVATE_LIFE_BOOTSTRAP_SCHEMA_VERSION_V1,
        account_id,
        account_version,
        state,
    };
    bootstrap.validate()?;
    Ok(bootstrap)
}

async fn load_current_danger_pending_items_v1(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredCurrentDangerPendingItemV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT item_uid,template_id,content_revision,item_kind,item_version,security_state,
                location_kind,slot_index,instance_id,pickup_id,expires_at_tick
           FROM item_instances
          WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
            AND security_state=$4 AND location_kind IN ($5,$6)
          ORDER BY location_kind,slot_index NULLS LAST,instance_id NULLS FIRST,
                   pickup_id NULLS FIRST,item_uid
          LIMIT $7
          FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(SECURITY_AT_RISK_PENDING)
    .bind(LOCATION_RUN_BACKPACK)
    .bind(LOCATION_PERSONAL_GROUND)
    .bind(
        i64::try_from(MAX_CURRENT_DANGER_PENDING_ITEMS_V1 + 1)
            .map_err(|_| corrupt_current_danger_snapshot())?,
    )
    .fetch_all(connection)
    .await?;
    if rows.len() > MAX_CURRENT_DANGER_PENDING_ITEMS_V1 {
        return Err(corrupt_current_danger_snapshot());
    }
    rows.into_iter()
        .map(|row| {
            if row.try_get::<String, _>("content_revision")? != crate::CORE_ITEM_CONTENT_REVISION {
                return Err(PersistenceError::CurrentDangerExtractionSnapshotContentMismatch);
            }
            if row.try_get::<i16, _>("security_state")? != SECURITY_AT_RISK_PENDING {
                return Err(corrupt_current_danger_snapshot());
            }
            let kind = match row.try_get::<i16, _>("item_kind")? {
                ITEM_EQUIPMENT => StoredCurrentDangerPendingItemKindV1::Equipment,
                ITEM_CONSUMABLE => StoredCurrentDangerPendingItemKindV1::Consumable,
                _ => return Err(corrupt_current_danger_snapshot()),
            };
            let location = match row.try_get::<i16, _>("location_kind")? {
                LOCATION_RUN_BACKPACK => StoredCurrentDangerPendingItemLocationV1::RunBackpack(
                    required_u8(row.try_get("slot_index")?)?,
                ),
                LOCATION_PERSONAL_GROUND => {
                    StoredCurrentDangerPendingItemLocationV1::PersonalGround {
                        instance_id: current_snapshot_exact_id(
                            row.try_get::<Option<Vec<u8>>, _>("instance_id")?
                                .ok_or_else(corrupt_current_danger_snapshot)?,
                        )?,
                        pickup_id: current_snapshot_exact_id(
                            row.try_get::<Option<Vec<u8>>, _>("pickup_id")?
                                .ok_or_else(corrupt_current_danger_snapshot)?,
                        )?,
                        expires_at_tick: optional_positive_u64(row.try_get("expires_at_tick")?)?
                            .ok_or_else(corrupt_current_danger_snapshot)?,
                    }
                }
                _ => return Err(corrupt_current_danger_snapshot()),
            };
            Ok(StoredCurrentDangerPendingItemV1 {
                item_uid: current_snapshot_exact_id(row.try_get("item_uid")?)?,
                template_id: row.try_get("template_id")?,
                kind,
                item_version: current_snapshot_positive_u64(row.try_get("item_version")?)?,
                location,
            })
        })
        .collect()
}

async fn reject_current_danger_unresolved_reward_v1(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<(), PersistenceError> {
    let unresolved: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT reward_request_id FROM reward_requests
          WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND request_state=0
          ORDER BY reward_request_id LIMIT 1 FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    if unresolved.is_some() {
        return Err(PersistenceError::ProductionExtractionUnresolvedMutation);
    }
    Ok(())
}

async fn load_current_danger_pending_materials_v1(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
) -> Result<Vec<StoredCurrentDangerPendingMaterialV1>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT material_id,quantity,material_version,security_state
           FROM character_run_material_stacks
          WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3
            AND security_state=$4 AND quantity>0
          ORDER BY material_id COLLATE \"C\"
          LIMIT $5
          FOR SHARE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(SECURITY_AT_RISK_PENDING)
    .bind(
        i64::try_from(MAX_CURRENT_DANGER_PENDING_MATERIALS_V1 + 1)
            .map_err(|_| corrupt_current_danger_snapshot())?,
    )
    .fetch_all(connection)
    .await?;
    if rows.len() > MAX_CURRENT_DANGER_PENDING_MATERIALS_V1 {
        return Err(corrupt_current_danger_snapshot());
    }
    rows.into_iter()
        .map(|row| {
            if row.try_get::<i16, _>("security_state")? != SECURITY_AT_RISK_PENDING {
                return Err(corrupt_current_danger_snapshot());
            }
            let material_id: String = row.try_get("material_id")?;
            if !matches!(
                material_id.as_str(),
                "material.bell_brass"
                    | "material.echo_ember"
                    | "material.funeral_root"
                    | "material.saltglass_shard"
            ) {
                return Err(PersistenceError::CurrentDangerExtractionSnapshotContentMismatch);
            }
            Ok(StoredCurrentDangerPendingMaterialV1 {
                material_id,
                quantity: positive_u16(row.try_get("quantity")?)?,
                material_version: current_snapshot_positive_u64(row.try_get("material_version")?)?,
            })
        })
        .collect()
}

async fn lock_bootstrap_account(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<(u64, Option<[u8; 16]>), PersistenceError> {
    let row = sqlx::query(
        "SELECT state_version,selected_character_id FROM accounts
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::PrivateLifeBootstrapAccountNotFound)?;
    Ok((
        positive_u64(row.try_get("state_version")?)?,
        optional_id(row.try_get("selected_character_id")?)?,
    ))
}

async fn load_unselected_state(
    connection: &mut PgConnection,
    account_id: [u8; 16],
) -> Result<StoredPrivateLifeBootstrapStateV1, PersistenceError> {
    let rows = sqlx::query(ACTIVE_SUCCESSOR_DEATH_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_all(&mut *connection)
        .await?;
    let [row] = rows.as_slice() else {
        if !rows.is_empty() {
            return Err(corrupt());
        }
        return Ok(StoredPrivateLifeBootstrapStateV1::CharacterSelect {
            selected_character: None,
            next_hall_arrival: None,
        });
    };
    let death_id = exact_id(row.try_get("death_id")?)?;
    let character_id = exact_id(row.try_get("character_id")?)?;
    let terminal = load_committed_death_terminal_v1_on(connection, account_id, character_id)
        .await?
        .ok_or_else(corrupt)?;
    if terminal.result.death_id != death_id {
        return Err(corrupt());
    }
    Ok(StoredPrivateLifeBootstrapStateV1::DeathCommitted(Box::new(
        terminal,
    )))
}

async fn load_selected_state(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    account_version: u64,
    character_id: [u8; 16],
) -> Result<StoredPrivateLifeBootstrapStateV1, PersistenceError> {
    let row = sqlx::query(SELECTED_CHARACTER_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(corrupt)?;
    let life_state: i16 = row.try_get("life_state")?;
    if life_state != LIFE_LIVING {
        return Err(corrupt());
    }
    let security_state = match row.try_get::<i16, _>("security_state")? {
        SECURITY_NORMAL => StoredPrivateLifeSecurityStateV1::Normal,
        SECURITY_STORAGE_RESOLUTION_REQUIRED => {
            StoredPrivateLifeSecurityStateV1::StorageResolutionRequired
        }
        _ => return Err(corrupt()),
    };
    let character = StoredPrivateLifeSelectedCharacterV1 {
        character_id,
        class_id: row.try_get("class_id")?,
        level: u8::try_from(row.try_get::<i32, _>("level")?).map_err(|_| corrupt())?,
        life_state: StoredPrivateLifeLifeStateV1::Living,
        security_state,
        versions: StoredPrivateLifeVersionsV1 {
            account: account_version,
            character: positive_u64(row.try_get("character_state_version")?)?,
            world: positive_u64(row.try_get("character_version")?)?,
            inventory: positive_u64(row.try_get("inventory_version")?)?,
            progression: positive_u64(row.try_get("progression_version")?)?,
            oath_bargain: positive_u64(row.try_get("oath_bargain_version")?)?,
            life_metrics: positive_u64(row.try_get("life_metrics_version")?)?,
            ash_wallet: positive_u64(row.try_get("wallet_version")?)?,
        },
    };
    character.validate(account_version)?;
    let location = decode_location(&row)?;
    match location {
        StoredWorldLocation::CharacterSelect {
            next_hall_arrival, ..
        } => {
            if security_state != StoredPrivateLifeSecurityStateV1::Normal {
                return Err(corrupt());
            }
            Ok(StoredPrivateLifeBootstrapStateV1::CharacterSelect {
                selected_character: Some(character),
                next_hall_arrival: Some(next_hall_arrival),
            })
        }
        StoredWorldLocation::Safe {
            location_content_id,
            arrival,
            ..
        } => {
            if location_content_id != PRIVATE_LIFE_HALL_ID_V1 {
                return Err(corrupt());
            }
            load_hall_state(connection, account_id, account_version, character, arrival).await
        }
        StoredWorldLocation::Danger {
            location_content_id,
            instance_lineage_id,
            entry_restore_point_id,
            ..
        } => {
            if security_state != StoredPrivateLifeSecurityStateV1::Normal {
                return Err(corrupt());
            }
            let danger = load_danger_root(
                connection,
                account_id,
                character_id,
                location_content_id,
                instance_lineage_id,
                entry_restore_point_id,
            )
            .await?;
            Ok(StoredPrivateLifeBootstrapStateV1::DangerRequiresCrashRestore { character, danger })
        }
    }
}

async fn load_hall_state(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    account_version: u64,
    character: StoredPrivateLifeSelectedCharacterV1,
    arrival: StoredSafeArrival,
) -> Result<StoredPrivateLifeBootstrapStateV1, PersistenceError> {
    let terminal = load_current_safe_terminal(connection, account_id, &character).await?;
    let resolution_hold =
        load_resolution_hold_snapshot_v1_on(connection, account_id, character.character_id).await?;
    let blocked = resolution_hold.storage_resolution_required;
    let hall = StoredPrivateLifeHallV1 {
        character,
        arrival,
        resolution_hold,
    };
    hall.validate(account_id, account_version)?;
    match terminal {
        Some(CurrentSafeTerminal::Extraction(terminal)) => {
            Ok(StoredPrivateLifeBootstrapStateV1::ExtractionCommitted {
                hall,
                terminal: Box::new(terminal),
            })
        }
        Some(CurrentSafeTerminal::Recall(terminal)) => {
            if blocked {
                return Err(corrupt());
            }
            Ok(StoredPrivateLifeBootstrapStateV1::RecallCommitted {
                hall,
                terminal: Box::new(terminal),
            })
        }
        None if blocked => {
            Ok(StoredPrivateLifeBootstrapStateV1::HallStorageResolutionRequired(hall))
        }
        None => Ok(StoredPrivateLifeBootstrapStateV1::HallReady(hall)),
    }
}

async fn load_danger_root(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character_id: [u8; 16],
    location_content_id: String,
    lineage_id: [u8; 16],
    restore_point_id: [u8; 16],
) -> Result<StoredPrivateLifeDangerRootV1, PersistenceError> {
    let rows = sqlx::query(DANGER_ROOT_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .bind(lineage_id.as_slice())
        .bind(restore_point_id.as_slice())
        .fetch_all(connection)
        .await?;
    let [row] = rows.as_slice() else {
        return Err(corrupt());
    };
    let content_revision = StoredWorldFlowRevisionV1 {
        records_blake3: row.try_get("records_blake3")?,
        assets_blake3: row.try_get("assets_blake3")?,
        localization_blake3: row.try_get("localization_blake3")?,
    };
    let lineage_state = row.try_get::<i16, _>("lineage_state")?;
    if row.try_get::<i16, _>("snapshot_contract_version")? != CORE_RESTORE_CONTRACT_VERSION
        || row.try_get::<i16, _>("component_mask")? != CORE_RESTORE_COMPONENT_MASK
        || row.try_get::<i16, _>("restore_state")? != RESTORE_ACTIVE
        || !row.try_get::<bool, _>("root_open")?
        || !matches!(lineage_state, LINEAGE_STAGED | LINEAGE_ACTIVE)
        || !row.try_get::<bool, _>("lineage_open")?
        || row.try_get::<String, _>("content_id")? != location_content_id
        || row.try_get::<String, _>("lineage_records_blake3")? != content_revision.records_blake3
        || row.try_get::<String, _>("lineage_assets_blake3")? != content_revision.assets_blake3
        || row.try_get::<String, _>("lineage_localization_blake3")?
            != content_revision.localization_blake3
    {
        return Err(corrupt());
    }
    let danger = StoredPrivateLifeDangerRootV1 {
        location_content_id,
        lineage_id,
        restore_point_id,
        source_location_id: row.try_get("source_location_id")?,
        restore_location_id: row.try_get("restore_location_id")?,
        layout_id: row.try_get("layout_id")?,
        lineage_state: u8::try_from(lineage_state).map_err(|_| corrupt())?,
        entry_versions: StoredPrivateLifeVersionsV1 {
            account: positive_u64(row.try_get("account_version")?)?,
            character: positive_u64(row.try_get("character_version")?)?,
            world: positive_u64(row.try_get("character_version")?)?,
            inventory: positive_u64(row.try_get("inventory_version")?)?,
            progression: positive_u64(row.try_get("progression_version")?)?,
            oath_bargain: positive_u64(row.try_get("oath_bargain_version")?)?,
            life_metrics: positive_u64(row.try_get("life_metrics_version")?)?,
            ash_wallet: positive_u64(row.try_get("ash_wallet_version")?)?,
        },
        content_revision,
        composite_digest: exact_hash(row.try_get("composite_digest")?)?,
    };
    danger.validate()?;
    Ok(danger)
}

enum CurrentSafeTerminal {
    Extraction(StoredCommittedExtractionTerminalV1),
    Recall(StoredCommittedRecallTerminalV1),
}

async fn load_current_safe_terminal(
    connection: &mut PgConnection,
    account_id: [u8; 16],
    character: &StoredPrivateLifeSelectedCharacterV1,
) -> Result<Option<CurrentSafeTerminal>, PersistenceError> {
    let versions = character.versions;
    let rows = sqlx::query(CURRENT_SAFE_TERMINAL_SQL)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character.character_id.as_slice())
        .bind(i64_value(versions.account)?)
        .bind(i64_value(versions.character)?)
        .bind(i64_value(versions.world)?)
        .bind(i64_value(versions.inventory)?)
        .bind(i64_value(versions.life_metrics)?)
        .bind(i64_value(versions.progression)?)
        .bind(i64_value(versions.oath_bargain)?)
        .bind(i64_value(versions.ash_wallet)?)
        .fetch_all(&mut *connection)
        .await?;
    let [row] = rows.as_slice() else {
        return if rows.is_empty() {
            Ok(None)
        } else {
            Err(corrupt())
        };
    };
    let request_id = exact_id(row.try_get("request_id")?)?;
    let result_id = exact_id(row.try_get("result_id")?)?;
    match row.try_get::<i16, _>("terminal_kind")? {
        1 => load_committed_extraction_terminal_v1_on(
            connection,
            account_id,
            character.character_id,
            Some(request_id),
            Some(result_id),
        )
        .await?
        .map(CurrentSafeTerminal::Extraction)
        .ok_or_else(corrupt)
        .map(Some),
        2 => {
            let terminal = load_committed_recall_terminal_v1_on(
                connection,
                account_id,
                character.character_id,
                Some(request_id),
                Some(result_id),
            )
            .await?
            .ok_or_else(corrupt)?;
            if !terminal.owns_current_hall {
                return Err(corrupt());
            }
            Ok(Some(CurrentSafeTerminal::Recall(terminal)))
        }
        _ => Err(corrupt()),
    }
}

fn receipt_matches_bootstrap(
    receipt: &DangerCrashRestoreReceipt,
    bootstrap: &StoredPrivateLifeBootstrapV1,
) -> bool {
    if receipt.account_id != bootstrap.account_id {
        return false;
    }
    match (receipt.code, &bootstrap.state) {
        (DangerCrashRestoreCode::Restored, StoredPrivateLifeBootstrapStateV1::HallReady(hall)) => {
            hall.character.character_id == receipt.character_id
                && hall.arrival == StoredSafeArrival::HallDefault
                && receipt
                    .versions
                    .as_ref()
                    .is_some_and(|versions| restored_versions_match_hall(versions, hall))
        }
        (
            DangerCrashRestoreCode::AlreadyCrashRestored,
            StoredPrivateLifeBootstrapStateV1::HallReady(hall),
        ) => {
            hall.character.character_id == receipt.character_id
                && hall.arrival == StoredSafeArrival::HallDefault
        }
        (
            DangerCrashRestoreCode::ExtractionCommitted,
            StoredPrivateLifeBootstrapStateV1::ExtractionCommitted { hall, terminal },
        ) => {
            hall.character.character_id == receipt.character_id
                && terminal.result.character_id == receipt.character_id
                && terminal.restore_point_id == receipt.restore_point_id
        }
        (
            DangerCrashRestoreCode::DeathCommitted,
            StoredPrivateLifeBootstrapStateV1::DeathCommitted(terminal),
        ) => {
            terminal.result.character_id == receipt.character_id
                && terminal.restore_point_id == receipt.restore_point_id
        }
        (
            DangerCrashRestoreCode::RecallCommitted,
            StoredPrivateLifeBootstrapStateV1::RecallCommitted { hall, terminal },
        ) => {
            hall.character.character_id == receipt.character_id
                && terminal.result.character_id == receipt.character_id
                && terminal.restore_point_id == receipt.restore_point_id
        }
        _ => false,
    }
}

const fn restored_versions_match_hall(
    restored: &DangerCrashRestoreVersions,
    hall: &StoredPrivateLifeHallV1,
) -> bool {
    let current = hall.character.versions;
    restored.account == current.account
        && restored.character == current.character
        && restored.progression == current.progression
        && restored.inventory == current.inventory
        && restored.oath_bargain == current.oath_bargain
        && restored.life_metrics == current.life_metrics
        && restored.ash_wallet == current.ash_wallet
}

fn valid_core_hall_arrival(arrival: &StoredSafeArrival) -> bool {
    match arrival {
        StoredSafeArrival::HallDefault => true,
        StoredSafeArrival::SpawnAnchor(spawn_id) => {
            spawn_id == PRIVATE_LIFE_CHARACTER_SELECT_RETURN_SPAWN_ID_V1
        }
    }
}

fn valid_revision(revision: &StoredWorldFlowRevisionV1) -> bool {
    [
        &revision.records_blake3,
        &revision.assets_blake3,
        &revision.localization_blake3,
    ]
    .into_iter()
    .all(|hash| {
        hash.len() == 64
            && !hash.bytes().all(|byte| byte == b'0')
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; 16]>, PersistenceError> {
    value.map(exact_id).transpose()
}

fn exact_id(value: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    let value: [u8; 16] = value.try_into().map_err(|_| corrupt())?;
    if value == [0; 16] {
        return Err(corrupt());
    }
    Ok(value)
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; 32], PersistenceError> {
    let value: [u8; 32] = value.try_into().map_err(|_| corrupt())?;
    if value == [0; 32] {
        return Err(corrupt());
    }
    Ok(value)
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt)
}

fn optional_positive_u64(value: Option<i64>) -> Result<Option<u64>, PersistenceError> {
    value.map(current_snapshot_positive_u64).transpose()
}

fn current_snapshot_positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt_current_danger_snapshot)
}

fn current_snapshot_exact_id(value: Vec<u8>) -> Result<[u8; 16], PersistenceError> {
    let value: [u8; 16] = value
        .try_into()
        .map_err(|_| corrupt_current_danger_snapshot())?;
    if value == [0; 16] {
        return Err(corrupt_current_danger_snapshot());
    }
    Ok(value)
}

fn required_u8(value: Option<i16>) -> Result<u8, PersistenceError> {
    value
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(corrupt_current_danger_snapshot)
}

fn positive_u16(value: i32) -> Result<u16, PersistenceError> {
    u16::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(corrupt_current_danger_snapshot)
}

fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| corrupt())
}

const fn corrupt() -> PersistenceError {
    PersistenceError::CorruptStoredPrivateLifeBootstrap
}

const fn corrupt_current_danger_snapshot() -> PersistenceError {
    PersistenceError::CorruptStoredCurrentDangerExtractionSnapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_restart_crash_identity_is_stable_and_domain_bound() {
        let first = derive_private_life_crash_mutation_id_v1([1; 16], [2; 16], [3; 16])
            .expect("valid identity");
        assert_eq!(
            first,
            derive_private_life_crash_mutation_id_v1([1; 16], [2; 16], [3; 16]).unwrap()
        );
        assert_ne!(
            first,
            derive_private_life_crash_mutation_id_v1([1; 16], [2; 16], [4; 16]).unwrap()
        );
        assert_ne!(first, [0; 16]);
        assert!(derive_private_life_crash_mutation_id_v1([0; 16], [2; 16], [3; 16]).is_err());
    }

    #[test]
    fn restart_receipt_requires_the_exact_character_versions_and_hall_arrival() {
        let bootstrap = hall_bootstrap([2; 16], StoredSafeArrival::HallDefault);
        let receipt = restored_receipt([2; 16]);
        receipt.validate().unwrap();
        bootstrap.validate().unwrap();
        assert!(receipt_matches_bootstrap(&receipt, &bootstrap));

        let foreign_character = hall_bootstrap([3; 16], StoredSafeArrival::HallDefault);
        assert!(!receipt_matches_bootstrap(&receipt, &foreign_character));

        let mut stale_versions = receipt.clone();
        stale_versions.versions.as_mut().unwrap().inventory += 1;
        assert!(!receipt_matches_bootstrap(&stale_versions, &bootstrap));

        let foreign_arrival = hall_bootstrap(
            [2; 16],
            StoredSafeArrival::SpawnAnchor("spawn.hub.foreign".into()),
        );
        assert!(foreign_arrival.validate().is_err());
        assert!(!receipt_matches_bootstrap(&receipt, &foreign_arrival));

        let character_select_return = hall_bootstrap(
            [2; 16],
            StoredSafeArrival::SpawnAnchor(PRIVATE_LIFE_CHARACTER_SELECT_RETURN_SPAWN_ID_V1.into()),
        );
        character_select_return.validate().unwrap();
        assert!(!receipt_matches_bootstrap(
            &receipt,
            &character_select_return
        ));
    }

    #[test]
    fn bootstrap_queries_cover_every_terminal_and_authority_family() {
        for required in [
            "character_world_locations",
            "character_inventories",
            "character_progression",
            "character_oath_bargain_state",
            "character_life_metrics",
            "ash_wallets",
        ] {
            assert!(SELECTED_CHARACTER_SQL.contains(required));
        }
        for required in [
            "character_entry_restore_points",
            "character_instance_lineages",
            "snapshot_contract_version",
            "component_mask",
            "composite_digest",
        ] {
            assert!(DANGER_ROOT_SQL.contains(required));
        }
        assert!(CURRENT_SAFE_TERMINAL_SQL.contains("character_extraction_terminal_results_v1"));
        assert!(CURRENT_SAFE_TERMINAL_SQL.contains("character_recall_terminal_results_v1"));
        for required in [
            "successor_roster_reservations_v1",
            "death_successor_presets_v1",
            "death_events",
            "reservation.reservation_state=0",
            "death.death_provenance=0",
        ] {
            assert!(ACTIVE_SUCCESSOR_DEATH_SQL.contains(required));
        }
        assert!(valid_revision(&StoredWorldFlowRevisionV1 {
            records_blake3: "a".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
        }));
        assert!(!valid_revision(&StoredWorldFlowRevisionV1 {
            records_blake3: "0".repeat(64),
            assets_blake3: "b".repeat(64),
            localization_blake3: "c".repeat(64),
        }));
    }

    #[test]
    fn current_danger_snapshot_accepts_canonical_bounded_pending_custody() {
        let snapshot = current_danger_snapshot();
        snapshot.validate().unwrap();
        assert_eq!(snapshot.expected_versions.inventory, 4);
        assert!(matches!(
            snapshot.pending_items[0].location,
            StoredCurrentDangerPendingItemLocationV1::RunBackpack(0)
        ));
        assert!(matches!(
            snapshot.pending_items[1].location,
            StoredCurrentDangerPendingItemLocationV1::PersonalGround { .. }
        ));
    }

    #[test]
    fn current_danger_snapshot_rejects_stale_or_unbounded_projection_material() {
        let mut stale = current_danger_snapshot();
        stale.expected_versions.world += 1;
        assert!(matches!(
            stale.validate(),
            Err(PersistenceError::CorruptStoredCurrentDangerExtractionSnapshot)
        ));

        let mut duplicate = current_danger_snapshot();
        duplicate.pending_items[1].item_uid = duplicate.pending_items[0].item_uid;
        assert!(duplicate.validate().is_err());

        let mut invalid_slot = current_danger_snapshot();
        invalid_slot.pending_items[0].location =
            StoredCurrentDangerPendingItemLocationV1::RunBackpack(8);
        assert!(invalid_slot.validate().is_err());

        let mut oversized = current_danger_snapshot();
        oversized.pending_items = (0..=MAX_CURRENT_DANGER_PENDING_ITEMS_V1)
            .map(|index| StoredCurrentDangerPendingItemV1 {
                item_uid: {
                    let mut uid = [1; 16];
                    uid[..8].copy_from_slice(&(index as u64 + 1).to_be_bytes());
                    uid
                },
                template_id: "consumable.red_tonic".into(),
                kind: StoredCurrentDangerPendingItemKindV1::Consumable,
                item_version: 1,
                location: StoredCurrentDangerPendingItemLocationV1::PersonalGround {
                    instance_id: [3; 16],
                    pickup_id: {
                        let mut pickup = [4; 16];
                        pickup[..8].copy_from_slice(&(index as u64 + 1).to_be_bytes());
                        pickup
                    },
                    expires_at_tick: 1_800,
                },
            })
            .collect();
        assert!(oversized.validate().is_err());

        let mut duplicate_equipment = current_danger_snapshot();
        duplicate_equipment.pending_items.insert(
            1,
            StoredCurrentDangerPendingItemV1 {
                item_uid: [9; 16],
                template_id: "item.weapon".into(),
                kind: StoredCurrentDangerPendingItemKindV1::Equipment,
                item_version: 1,
                location: StoredCurrentDangerPendingItemLocationV1::RunBackpack(0),
            },
        );
        assert!(duplicate_equipment.validate().is_err());

        let mut over_cap_tonics = current_danger_snapshot();
        over_cap_tonics.pending_items = (1_u8..=7)
            .map(|uid| StoredCurrentDangerPendingItemV1 {
                item_uid: [uid; 16],
                template_id: "consumable.red_tonic".into(),
                kind: StoredCurrentDangerPendingItemKindV1::Consumable,
                item_version: 1,
                location: StoredCurrentDangerPendingItemLocationV1::RunBackpack(0),
            })
            .collect();
        assert!(over_cap_tonics.validate().is_err());

        let mut over_cap_material = current_danger_snapshot();
        over_cap_material.pending_materials[0].quantity = 100;
        assert!(over_cap_material.validate().is_err());
    }

    fn current_danger_snapshot() -> StoredCurrentDangerExtractionSnapshotV1 {
        StoredCurrentDangerExtractionSnapshotV1 {
            schema_version: CURRENT_DANGER_EXTRACTION_SNAPSHOT_SCHEMA_VERSION_V1,
            authority: StoredActiveDangerAuthorityV1 {
                account_id: [1; 16],
                character_id: [2; 16],
                instance_lineage_id: [3; 16],
                entry_restore_point_id: [4; 16],
            },
            location_content_id: "world.core_microrealm_01".into(),
            content_revision: StoredWorldFlowRevisionV1 {
                records_blake3: "a".repeat(64),
                assets_blake3: "b".repeat(64),
                localization_blake3: "c".repeat(64),
            },
            expected_versions: ProductionExtractionExpectedVersionsV1 {
                account: 3,
                character: 2,
                world: 2,
                inventory: 4,
                life_metrics: 5,
            },
            pending_items: vec![
                StoredCurrentDangerPendingItemV1 {
                    item_uid: [5; 16],
                    template_id: "item.weapon".into(),
                    kind: StoredCurrentDangerPendingItemKindV1::Equipment,
                    item_version: 1,
                    location: StoredCurrentDangerPendingItemLocationV1::RunBackpack(0),
                },
                StoredCurrentDangerPendingItemV1 {
                    item_uid: [6; 16],
                    template_id: "consumable.red_tonic".into(),
                    kind: StoredCurrentDangerPendingItemKindV1::Consumable,
                    item_version: 1,
                    location: StoredCurrentDangerPendingItemLocationV1::PersonalGround {
                        instance_id: [7; 16],
                        pickup_id: [8; 16],
                        expires_at_tick: 1_800,
                    },
                },
            ],
            pending_materials: vec![StoredCurrentDangerPendingMaterialV1 {
                material_id: "material.bell_brass".into(),
                quantity: 2,
                material_version: 1,
            }],
        }
    }

    fn hall_bootstrap(
        character_id: [u8; 16],
        arrival: StoredSafeArrival,
    ) -> StoredPrivateLifeBootstrapV1 {
        let versions = StoredPrivateLifeVersionsV1 {
            account: 1,
            character: 2,
            world: 2,
            inventory: 3,
            progression: 4,
            oath_bargain: 5,
            life_metrics: 6,
            ash_wallet: 7,
        };
        StoredPrivateLifeBootstrapV1 {
            schema_version: PRIVATE_LIFE_BOOTSTRAP_SCHEMA_VERSION_V1,
            account_id: [1; 16],
            account_version: versions.account,
            state: StoredPrivateLifeBootstrapStateV1::HallReady(StoredPrivateLifeHallV1 {
                character: StoredPrivateLifeSelectedCharacterV1 {
                    character_id,
                    class_id: PRIVATE_LIFE_CLASS_ID_V1.into(),
                    level: 1,
                    life_state: StoredPrivateLifeLifeStateV1::Living,
                    security_state: StoredPrivateLifeSecurityStateV1::Normal,
                    versions,
                },
                arrival,
                resolution_hold: StoredResolutionHoldSnapshotV1 {
                    account_id: [1; 16],
                    character_id,
                    versions: crate::StoredResolutionHoldVersionsV1 {
                        account: versions.account,
                        character: versions.character,
                        world: versions.world,
                        inventory: versions.inventory,
                    },
                    storage_resolution_required: false,
                    stacks: Vec::new(),
                },
            }),
        }
    }

    fn restored_receipt(character_id: [u8; 16]) -> DangerCrashRestoreReceipt {
        DangerCrashRestoreReceipt {
            contract: crate::DANGER_CRASH_RESTORE_CONTRACT.into(),
            account_id: [1; 16],
            character_id,
            restore_point_id: [3; 16],
            request_mutation_id: [4; 16],
            request_hash: [5; 32],
            code: DangerCrashRestoreCode::Restored,
            committed_mutation_id: Some([4; 16]),
            versions: Some(DangerCrashRestoreVersions {
                account: 1,
                character: 2,
                progression: 4,
                inventory: 3,
                oath_bargain: 5,
                life_metrics: 6,
                ash_wallet: 7,
            }),
            item_changes: Vec::new(),
            material_changes: Vec::new(),
            bargain_changes: Vec::new(),
            ash_changes: Vec::new(),
        }
    }
}
