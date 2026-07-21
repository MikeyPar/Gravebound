//! Reusable hosted fixture for the disposable committed-death QUIC proof.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` `DTH-001`, `DTH-020`, and
//! `TECH-020`-`023`; `Gravebound_Content_Production_Spec_v1.md` `CONT-ECHO-009` and
//! `CONT-HUB-002`; and `Gravebound_Development_Roadmap_v1.md` `GB-M03-02`, `GB-M03-06`, and
//! `GB-M03-13`. This fixture enters through production server authority and never constructs a
//! client-authored lethal command.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use persistence::{
    CORE_DEATH_VIEW_ASSETS_BLAKE3, CORE_DEATH_VIEW_LOCALIZATION_BLAKE3,
    CORE_DEATH_VIEW_RECORDS_BLAKE3, CORE_ITEM_CONTENT_REVISION, CORE_WORLD_ASSETS_BLAKE3,
    CORE_WORLD_LOCALIZATION_BLAKE3, CORE_WORLD_RECORDS_BLAKE3, DeathAggregateVersionsV1,
    DeathVersionAdvanceV1, DurableDeathContentAuthorityV1, DurableDeathItemContentAuthorityV1,
    DurableDestructionLocationV1, DurableEquipmentSlotV1, LiveDamageTraceContentAuthorityV1,
    LiveDamageTraceDangerAuthorityV1, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
    stage_danger_entry_ash_wallet_restore_v3, stage_danger_entry_inventory_restore_v3,
    stage_danger_entry_life_metrics_restore_v3, stage_danger_entry_oath_bargain_restore_v3,
};
use protocol::{DeathViewContentRevisionV1, ManifestHash};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, DeathAtRiskItem,
    DeathAtRiskRunMaterial, DeathCustodySnapshot, DeathEntityIdentityAuthority, DeathHeroSnapshot,
    DeathLineageState, DeathMutationAuthority, DeathProvenance, DeathWorldAuthority,
    EchoAvailabilityProjection, EligibleEchoProjection, LiveDamageTraceBinding,
    LiveDamageTraceIngestOutcome, LiveDamageTraceMutationAuthority, LiveDamageTraceService,
    PreparedDurableDeathCommit, ServerAuthoredDeathContext, build_durable_death_commit,
};
use sim_core::{
    AuthoritativeDeathCauseKind, AuthoritativeDeathInputs, DEED_NONE_ID, DamageTraceObservation,
    DamageType, DeathClockSnapshot, DeathTraceNetworkState, DeathTraceRecallState,
    DeathTraceStatus, EntityId, FinalDeed, SimulationVector, Tick, ticks_to_milliseconds,
};

/// The production Core identity runtime derives this account from `AUTH_TICKET` with BLAKE3.
pub const ACCOUNT_ID: [u8; 16] = [
    165, 92, 48, 136, 62, 13, 73, 61, 120, 165, 179, 215, 25, 114, 58, 100,
];
#[allow(
    dead_code,
    reason = "shared integration support is compiled by targets that do not open real QUIC"
)]
pub const AUTH_TICKET: &[u8] = b"disposable-core-route";
#[allow(
    dead_code,
    reason = "the parallel account is consumed only by the dedicated concurrency target"
)]
pub const PARALLEL_ACCOUNT_ID: [u8; 16] = [180; 16];
pub const MATERIAL_ID: &str = "material.bell_brass";
pub const ITEM_TEMPLATE_ID: &str = "item.weapon.crossbow.pine_crossbow";
pub const DEED_ID: &str = "deed.core.sir_caldus_defeated";

const ISSUED_AT_UNIX_MS: u64 = 1;
const TERMINAL_TICK: u64 = 20_000;
const CHECKPOINT_TICK: u64 = TERMINAL_TICK - 10;
const DEED_TICK: u64 = 19_000;

const fn uuid_v7(seed: u8) -> [u8; 16] {
    let mut value = [seed; 16];
    value[6] = 0x70 | (seed & 0x0f);
    value[8] = 0x80 | (seed & 0x3f);
    value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableDeathFixtureIdentityV1 {
    pub character_id: [u8; 16],
    pub lineage_id: [u8; 16],
    pub restore_point_id: [u8; 16],
    pub instance_id: [u8; 16],
    pub item_uid: [u8; 16],
    pub entry_mutation_id: [u8; 16],
    pub deed_reward_id: [u8; 16],
    pub nonlethal_trace_tick_id: [u8; 16],
    pub lethal_trace_tick_id: [u8; 16],
    pub death_id: [u8; 16],
    pub echo_id: [u8; 16],
    pub death_mutation_id: [u8; 16],
    pub source_sim_entity_id: u64,
}

pub const PRIMARY_IDENTITY: DurableDeathFixtureIdentityV1 = DurableDeathFixtureIdentityV1 {
    character_id: [231; 16],
    lineage_id: [232; 16],
    restore_point_id: [233; 16],
    instance_id: [234; 16],
    item_uid: [235; 16],
    entry_mutation_id: [237; 16],
    deed_reward_id: [238; 16],
    nonlethal_trace_tick_id: [239; 16],
    lethal_trace_tick_id: [240; 16],
    death_id: uuid_v7(41),
    echo_id: uuid_v7(42),
    death_mutation_id: uuid_v7(43),
    source_sim_entity_id: 81,
};

#[allow(
    dead_code,
    reason = "shared integration support is compiled separately by tests that use only one identity"
)]
pub const SECONDARY_IDENTITY: DurableDeathFixtureIdentityV1 = DurableDeathFixtureIdentityV1 {
    character_id: [201; 16],
    lineage_id: [202; 16],
    restore_point_id: [203; 16],
    instance_id: [204; 16],
    item_uid: [205; 16],
    entry_mutation_id: [207; 16],
    deed_reward_id: [208; 16],
    nonlethal_trace_tick_id: [209; 16],
    lethal_trace_tick_id: [210; 16],
    death_id: uuid_v7(51),
    echo_id: uuid_v7(52),
    death_mutation_id: uuid_v7(53),
    source_sim_entity_id: 82,
};

#[allow(
    dead_code,
    reason = "the parallel identity is consumed only by the dedicated concurrency target"
)]
pub const PARALLEL_IDENTITY: DurableDeathFixtureIdentityV1 = DurableDeathFixtureIdentityV1 {
    character_id: [181; 16],
    lineage_id: [182; 16],
    restore_point_id: [183; 16],
    instance_id: [184; 16],
    item_uid: [185; 16],
    entry_mutation_id: [187; 16],
    deed_reward_id: [188; 16],
    nonlethal_trace_tick_id: [189; 16],
    lethal_trace_tick_id: [190; 16],
    death_id: uuid_v7(61),
    echo_id: uuid_v7(62),
    death_mutation_id: uuid_v7(63),
    source_sim_entity_id: 83,
};

#[allow(
    dead_code,
    reason = "legacy primary aliases are consumed by other independently compiled integration targets"
)]
pub const CHARACTER_ID: [u8; 16] = PRIMARY_IDENTITY.character_id;
#[allow(
    dead_code,
    reason = "legacy primary aliases are consumed by other independently compiled integration targets"
)]
pub const DEATH_ID: [u8; 16] = PRIMARY_IDENTITY.death_id;

#[allow(
    dead_code,
    reason = "shared integration support is compiled separately by tests that use one branch"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureEchoAvailabilityV1 {
    None,
    SelfPromote,
    ExistingAvailable { echo_id: [u8; 16] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableDeathScenarioV1 {
    pub account_id: [u8; 16],
    pub identity: DurableDeathFixtureIdentityV1,
    pub reset_account: bool,
    pub character_precreated: bool,
    pub roster_ordinal: u8,
    pub account_pre_version: u64,
    pub inventory_pre_version: u64,
    pub level: u8,
    pub lifetime_ticks: u64,
    pub permadeath_combat_ticks: u64,
    pub boss_deed: bool,
    pub provenance: DeathProvenance,
    pub echo_availability: FixtureEchoAvailabilityV1,
}

impl DurableDeathScenarioV1 {
    #[must_use]
    pub const fn primary_eligible() -> Self {
        Self {
            account_id: ACCOUNT_ID,
            identity: PRIMARY_IDENTITY,
            reset_account: true,
            character_precreated: false,
            roster_ordinal: 1,
            account_pre_version: 1,
            inventory_pre_version: 2,
            level: 10,
            lifetime_ticks: TERMINAL_TICK,
            permadeath_combat_ticks: 18_000,
            boss_deed: true,
            provenance: DeathProvenance::OrdinaryGameplay,
            echo_availability: FixtureEchoAvailabilityV1::SelfPromote,
        }
    }

    #[must_use]
    #[allow(
        dead_code,
        reason = "only the hosted branch-matrix integration target creates a second mortal life"
    )]
    pub const fn secondary_with_existing_available(echo_id: [u8; 16]) -> Self {
        Self {
            account_id: ACCOUNT_ID,
            identity: SECONDARY_IDENTITY,
            reset_account: false,
            character_precreated: true,
            roster_ordinal: 2,
            account_pre_version: 2,
            inventory_pre_version: 2,
            level: 10,
            lifetime_ticks: TERMINAL_TICK,
            permadeath_combat_ticks: 18_000,
            boss_deed: true,
            provenance: DeathProvenance::OrdinaryGameplay,
            echo_availability: FixtureEchoAvailabilityV1::ExistingAvailable { echo_id },
        }
    }

    #[must_use]
    #[allow(
        dead_code,
        reason = "the parallel scenario is consumed only by the dedicated concurrency target"
    )]
    pub const fn parallel_eligible() -> Self {
        Self {
            account_id: PARALLEL_ACCOUNT_ID,
            identity: PARALLEL_IDENTITY,
            reset_account: true,
            character_precreated: false,
            roster_ordinal: 1,
            account_pre_version: 1,
            inventory_pre_version: 2,
            level: 10,
            lifetime_ticks: TERMINAL_TICK,
            permadeath_combat_ticks: 18_000,
            boss_deed: true,
            provenance: DeathProvenance::OrdinaryGameplay,
            echo_availability: FixtureEchoAvailabilityV1::SelfPromote,
        }
    }

    fn echo_eligible(&self) -> bool {
        self.level >= 10
            && self.permadeath_combat_ticks >= 18_000
            && self.boss_deed
            && self.provenance == DeathProvenance::OrdinaryGameplay
    }
}

#[allow(
    dead_code,
    reason = "legacy callers use the canonical primary authenticated account"
)]
pub fn authenticated_account() -> AuthenticatedAccount {
    authenticated_account_for(ACCOUNT_ID)
}

fn authenticated_account_for(account_id: [u8; 16]) -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new(account_id).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

#[allow(
    dead_code,
    reason = "shared integration support is compiled by targets that do not open death views"
)]
pub fn death_view_revision() -> DeathViewContentRevisionV1 {
    DeathViewContentRevisionV1 {
        records_blake3: ManifestHash::new(CORE_DEATH_VIEW_RECORDS_BLAKE3).unwrap(),
        assets_blake3: ManifestHash::new(CORE_DEATH_VIEW_ASSETS_BLAKE3).unwrap(),
        localization_blake3: ManifestHash::new(CORE_DEATH_VIEW_LOCALIZATION_BLAKE3).unwrap(),
    }
}

/// Seeds exactly one living danger aggregate. The staging helpers are the same V3 custody
/// boundary used by production danger entry; final death rows are deliberately absent.
#[allow(
    dead_code,
    clippy::too_many_lines,
    reason = "the complete danger root remains explicit for hosted authority review"
)]
pub async fn seed_danger_root(persistence: &PostgresPersistence) {
    seed_danger_root_for(persistence, &DurableDeathScenarioV1::primary_eligible()).await;
}

/// Adds the second living roster identity before a predecessor death reserves successor recovery.
/// The remaining aggregate roots are initialized only when that already-living identity is later
/// selected for the hosted existing-Available Echo branch.
#[allow(
    dead_code,
    reason = "only the hosted branch-matrix integration target needs a pre-existing second life"
)]
pub async fn precreate_living_character_for(
    persistence: &PostgresPersistence,
    scenario: &DurableDeathScenarioV1,
) {
    assert!(scenario.character_precreated);
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let account_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM accounts WHERE namespace_id=$1 AND account_id=$2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(scenario.account_id.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    assert!(account_exists);
    sqlx::query(
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id, \
         level,oath_id,life_state,security_state,character_state_version) \
         VALUES ($1,$2,$3,$4,'class.grave_arbalist',$5,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(scenario.account_id.as_slice())
    .bind(scenario.identity.character_id.as_slice())
    .bind(i16::from(scenario.roster_ordinal))
    .bind(i16::from(scenario.level))
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

#[allow(
    clippy::too_many_lines,
    reason = "the complete parameterized danger root remains explicit for hosted authority review"
)]
pub async fn seed_danger_root_for(
    persistence: &PostgresPersistence,
    scenario: &DurableDeathScenarioV1,
) {
    let identity = scenario.identity;
    let account_id = scenario.account_id;
    assert!((1..=10).contains(&scenario.level));
    assert!(scenario.lifetime_ticks >= 10);
    assert!(scenario.permadeath_combat_ticks >= 10);
    assert!(scenario.permadeath_combat_ticks <= scenario.lifetime_ticks);
    let mut transaction = persistence.begin_transaction().await.unwrap();
    if scenario.reset_account {
        sqlx::query("DELETE FROM accounts WHERE namespace_id=$1 AND account_id=$2")
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .execute(transaction.connection())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity) \
             VALUES ($1,$2,$3,2)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(i64::try_from(scenario.account_pre_version).unwrap())
        .execute(transaction.connection())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO ash_wallets (namespace_id,account_id,balance,wallet_version) \
             VALUES ($1,$2,0,1)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    } else {
        let account: (i64, Option<Vec<u8>>) = sqlx::query_as(
            "SELECT state_version,selected_character_id FROM accounts \
             WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .fetch_one(transaction.connection())
        .await
        .unwrap();
        assert_eq!(
            account,
            (i64::try_from(scenario.account_pre_version).unwrap(), None)
        );
    }
    if scenario.character_precreated {
        let character: (i16, String, i32, i16, i16, i64) = sqlx::query_as(
            "SELECT roster_ordinal,class_id,level,life_state,security_state, \
             character_state_version FROM characters WHERE namespace_id=$1 AND account_id=$2 \
             AND character_id=$3 FOR UPDATE",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(identity.character_id.as_slice())
        .fetch_one(transaction.connection())
        .await
        .unwrap();
        assert_eq!(
            character,
            (
                i16::from(scenario.roster_ordinal),
                "class.grave_arbalist".to_owned(),
                i32::from(scenario.level),
                0,
                0,
                1,
            )
        );
    } else {
        sqlx::query(
            "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id, \
             level,oath_id,life_state,security_state,character_state_version) \
             VALUES ($1,$2,$3,$4,'class.grave_arbalist',$5,NULL,0,0,1)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(identity.character_id.as_slice())
        .bind(i16::from(scenario.roster_ordinal))
        .bind(i16::from(scenario.level))
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(identity.character_id.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level, \
         current_health,progression_version) VALUES ($1,$2,$3,$4,$5,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(i64::from(scenario.level) * 270)
    .bind(i16::from(scenario.level))
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id,account_id,character_id,inventory_version) \
         VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id,account_id,character_id, \
         earned_bargain_slots,oath_bargain_version) VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_instances (namespace_id,item_uid,account_id,character_id,template_id, \
         content_revision,item_kind,item_level,rarity,creation_kind,creation_request_id,roll_index, \
         unit_ordinal,item_version,security_state,location_kind,slot_index,provenance_kind, \
         salvage_band,salvage_value) \
         VALUES ($1,$2,$3,$4,$5,$6,0,10,0,0,$2,0,0,1,0,0,0,0,0,0)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(identity.item_uid.as_slice())
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(ITEM_TEMPLATE_ID)
    .bind(CORE_ITEM_CONTENT_REVISION)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_instance_lineages (namespace_id,account_id,character_id,lineage_id, \
         content_id,layout_id,lineage_state,records_blake3,assets_blake3,localization_blake3) \
         VALUES ($1,$2,$3,$4,'world.core_microrealm_01','layout.core_private_life_01',0,$5,$6,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(identity.lineage_id.as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_entry_restore_points (namespace_id,account_id,character_id, \
         restore_point_id,lineage_id,source_location_id,restore_location_id, \
         snapshot_contract_version,account_version,character_version,progression_version, \
         inventory_version,oath_bargain_version,life_metrics_version,ash_wallet_version, \
         component_mask,composite_digest,restore_state,records_blake3,assets_blake3, \
         localization_blake3) VALUES ($1,$2,$3,$4,$5,'hub.lantern_halls_01', \
         'hub.lantern_halls_01',3,$6,1,1,2,1,1,1,31,$7,0,$8,$9,$10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(identity.restore_point_id.as_slice())
    .bind(identity.lineage_id.as_slice())
    .bind(i64::try_from(scenario.account_pre_version).unwrap())
    .bind([91_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v3 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version,component_digest) \
         VALUES ($1,$2,$3,$4,$5,$6,120,1,$7)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(identity.restore_point_id.as_slice())
    .bind(i16::from(scenario.level))
    .bind(i64::from(scenario.level) * 270)
    .bind([92_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v1 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version) \
         VALUES ($1,$2,$3,$4,$5,$6,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(identity.restore_point_id.as_slice())
    .bind(i16::from(scenario.level))
    .bind(i64::from(scenario.level) * 270)
    .execute(transaction.connection())
    .await
    .unwrap();
    stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        account_id,
        identity.character_id,
        identity.restore_point_id,
        identity.entry_mutation_id,
        0,
    )
    .await
    .unwrap();
    stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        account_id,
        identity.character_id,
        identity.restore_point_id,
    )
    .await
    .unwrap();
    stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        account_id,
        identity.character_id,
        identity.restore_point_id,
    )
    .await
    .unwrap();
    stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        account_id,
        identity.character_id,
        identity.restore_point_id,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,instance_lineage_id, \
         entry_restore_point_id) VALUES ($1,$2,$3,2,2,'world.core_microrealm_01',$4,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(identity.lineage_id.as_slice())
    .bind(identity.restore_point_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version=2 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_progression SET current_health=50,progression_version=2 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=$1,permadeath_combat_ticks=$2, \
         life_metrics_version=2 WHERE namespace_id=$3 AND account_id=$4 AND character_id=$5",
    )
    .bind(i64::try_from(scenario.lifetime_ticks - 10).unwrap())
    .bind(i64::try_from(scenario.permadeath_combat_ticks - 10).unwrap())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    let checkpoint_payload = [1_u8];
    let checkpoint_payload_digest = blake3::hash(&checkpoint_payload);
    sqlx::query(
        "INSERT INTO character_danger_checkpoints (namespace_id,account_id,character_id, \
         lineage_id,checkpoint_tick,component_mask,composite_digest,character_version, \
         progression_version,inventory_version,oath_bargain_version,records_blake3, \
         assets_blake3,localization_blake3,checkpoint_schema_version,checkpoint_payload, \
         checkpoint_payload_digest) VALUES ($1,$2,$3,$4,$5,15,$6,2,2,2,1,$7,$8,$9,1,$10,$11)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(identity.lineage_id.as_slice())
    .bind(i64::try_from(CHECKPOINT_TICK).unwrap())
    .bind([94_u8; 32].as_slice())
    .bind(CORE_WORLD_RECORDS_BLAKE3)
    .bind(CORE_WORLD_ASSETS_BLAKE3)
    .bind(CORE_WORLD_LOCALIZATION_BLAKE3)
    .bind(checkpoint_payload.as_slice())
    .bind(checkpoint_payload_digest.as_bytes().as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_run_material_stacks (namespace_id,account_id,character_id, \
         material_id,quantity,material_version,security_state) VALUES ($1,$2,$3,$4,7,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(identity.character_id.as_slice())
    .bind(MATERIAL_ID)
    .execute(transaction.connection())
    .await
    .unwrap();
    if scenario.boss_deed {
        sqlx::query(
            "INSERT INTO character_life_deeds (namespace_id,account_id,character_id,deed_id, \
             reward_event_id,source_content_id,deed_kind,achieved_tick,content_revision) \
             VALUES ($1,$2,$3,$4,$5,'boss.sir_caldus',0,$6,$7)",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(identity.character_id.as_slice())
        .bind(DEED_ID)
        .bind(identity.deed_reward_id.as_slice())
        .bind(i64::try_from(DEED_TICK).unwrap())
        .bind(CORE_ITEM_CONTENT_REVISION)
        .execute(transaction.connection())
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
}

fn observation(
    source_sim_entity_id: u64,
    tick: u64,
    pre_health: u32,
    final_damage: u32,
) -> DamageTraceObservation {
    DamageTraceObservation {
        tick: Tick(tick),
        event_ordinal: 0,
        cause_kind: AuthoritativeDeathCauseKind::DirectHit,
        source_content_id: "miniboss.sepulcher_knight".into(),
        source_entity_id: Some(EntityId::new(source_sim_entity_id).unwrap()),
        pattern_id: Some("miniboss.sepulcher_knight.charge_lane".into()),
        attack_id: "miniboss.sepulcher_knight.charge_lane".into(),
        raw_damage: final_damage,
        final_damage,
        damage_type: DamageType::Physical,
        pre_health,
        post_health: pre_health.saturating_sub(final_damage),
        source_position: SimulationVector::new(1.25, -0.5),
        statuses: (tick == CHECKPOINT_TICK)
            .then_some(DeathTraceStatus {
                status_id: "status.hex".into(),
                remaining_ticks: 30,
                stack_count: 1,
            })
            .into_iter()
            .collect(),
        network_state: DeathTraceNetworkState::Connected,
        recall_state: DeathTraceRecallState::Inactive,
    }
}

fn versions(account_pre_version: u64, inventory_pre_version: u64) -> DeathAggregateVersionsV1 {
    DeathAggregateVersionsV1 {
        account: DeathVersionAdvanceV1 {
            pre: account_pre_version,
            post: account_pre_version + 1,
        },
        character: DeathVersionAdvanceV1 { pre: 2, post: 3 },
        progression: DeathVersionAdvanceV1 { pre: 2, post: 3 },
        inventory: DeathVersionAdvanceV1 {
            pre: inventory_pre_version,
            post: inventory_pre_version + 1,
        },
        oath_bargain: DeathVersionAdvanceV1 { pre: 1, post: 2 },
        life_metrics: DeathVersionAdvanceV1 { pre: 2, post: 3 },
    }
}

fn build_echo_projection(
    scenario: &DurableDeathScenarioV1,
    server_computed_power_band: u8,
) -> Option<EligibleEchoProjection> {
    let identity = scenario.identity;
    if !scenario.echo_eligible() {
        assert_eq!(scenario.echo_availability, FixtureEchoAvailabilityV1::None);
        return None;
    }
    assert!((1..=5).contains(&server_computed_power_band));
    let availability = match scenario.echo_availability {
        FixtureEchoAvailabilityV1::None => {
            panic!("eligible hosted death requires an account-locked Echo decision")
        }
        FixtureEchoAvailabilityV1::SelfPromote => {
            EchoAvailabilityProjection::PromoteOldestDormant {
                echo_id: identity.echo_id,
                echo_death_id: identity.death_id,
                next_transition_ordinal: 1,
            }
        }
        FixtureEchoAvailabilityV1::ExistingAvailable { echo_id } => {
            EchoAvailabilityProjection::ExistingAvailable { echo_id }
        }
    };
    Some(EligibleEchoProjection {
        echo_id: identity.echo_id,
        appearance_snapshot_id: persistence::CORE_ECHO_BASE_SILHOUETTE_ID.into(),
        appearance_theme_id: persistence::CORE_ECHO_PRESENTATION_PLACEHOLDER_ID.into(),
        weapon_signature_tag: None,
        relic_signature_tag: None,
        deed_tags: vec![DEED_ID.into()],
        power_band: server_computed_power_band,
        availability,
    })
}

/// Produces one sealed death by traversing the production live trace and server death builder.
#[allow(
    dead_code,
    clippy::too_many_lines,
    reason = "the trace, simulation evidence, and sealed server context stay contiguous for audit"
)]
pub async fn prepare_death(persistence: PostgresPersistence) -> PreparedDurableDeathCommit {
    prepare_death_for(persistence, &DurableDeathScenarioV1::primary_eligible()).await
}

#[allow(
    clippy::too_many_lines,
    reason = "the parameterized trace, evidence, and server context remain contiguous for audit"
)]
pub async fn prepare_death_for(
    persistence: PostgresPersistence,
    scenario: &DurableDeathScenarioV1,
) -> PreparedDurableDeathCommit {
    let identity = scenario.identity;
    prepare_death_for_with_custody(
        persistence,
        scenario,
        1,
        DeathCustodySnapshot {
            items: vec![DeathAtRiskItem {
                content_id: ITEM_TEMPLATE_ID.into(),
                item_uid: identity.item_uid,
                location: DurableDestructionLocationV1::Equipment {
                    slot: DurableEquipmentSlotV1::Weapon,
                },
                item_version: 2,
            }],
            run_materials: vec![DeathAtRiskRunMaterial {
                material_id: MATERIAL_ID.into(),
                quantity: 7,
                material_version: 1,
            }],
        },
        vec![DurableDeathItemContentAuthorityV1 {
            template_id: ITEM_TEMPLATE_ID.into(),
            echo_signature_tag: None,
        }],
    )
    .await
}

#[allow(
    dead_code,
    clippy::too_many_lines,
    reason = "the parameterized trace, custody, evidence, and server context remain contiguous for audit"
)]
pub async fn prepare_death_for_with_custody(
    persistence: PostgresPersistence,
    scenario: &DurableDeathScenarioV1,
    server_computed_echo_power_band: u8,
    custody: DeathCustodySnapshot,
    enabled_items: Vec<DurableDeathItemContentAuthorityV1>,
) -> PreparedDurableDeathCommit {
    let identity = scenario.identity;
    let account_id = scenario.account_id;
    let presentation = Arc::new(
        sim_content::load_core_development_death_view(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
        )
        .unwrap(),
    );
    let danger = LiveDamageTraceDangerAuthorityV1 {
        lineage_id: identity.lineage_id,
        restore_point_id: identity.restore_point_id,
        checkpoint_tick: CHECKPOINT_TICK,
    };
    let content = LiveDamageTraceContentAuthorityV1::core();
    let identities = DeathEntityIdentityAuthority {
        by_sim_entity: BTreeMap::from([(
            EntityId::new(identity.source_sim_entity_id).unwrap(),
            [u8::try_from(identity.source_sim_entity_id).unwrap(); 16],
        )]),
    };
    let mut trace = LiveDamageTraceService::start_or_resume(
        persistence,
        LiveDamageTraceBinding::new(
            account_id,
            identity.character_id,
            2,
            danger.clone(),
            content.clone(),
        )
        .unwrap(),
        identities.clone(),
        presentation.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        trace
            .ingest_tick(
                LiveDamageTraceMutationAuthority::new(
                    identity.nonlethal_trace_tick_id,
                    2,
                    danger.clone(),
                    ISSUED_AT_UNIX_MS,
                )
                .unwrap(),
                vec![observation(
                    identity.source_sim_entity_id,
                    CHECKPOINT_TICK,
                    60,
                    10,
                )],
            )
            .await
            .unwrap(),
        LiveDamageTraceIngestOutcome::Committed(_)
    ));
    let LiveDamageTraceIngestOutcome::TerminalPrepared(terminal_trace) = trace
        .ingest_tick(
            LiveDamageTraceMutationAuthority::new(
                identity.lethal_trace_tick_id,
                2,
                danger,
                ISSUED_AT_UNIX_MS,
            )
            .unwrap(),
            vec![observation(
                identity.source_sim_entity_id,
                TERMINAL_TICK,
                50,
                50,
            )],
        )
        .await
        .unwrap()
    else {
        panic!("lethal server observation must prepare terminal evidence")
    };
    let verified = terminal_trace.terminal_snapshot();
    let inputs = AuthoritativeDeathInputs {
        clocks: DeathClockSnapshot {
            lifetime_ticks: scenario.lifetime_ticks,
            lifetime_ms: ticks_to_milliseconds(scenario.lifetime_ticks).unwrap(),
            permadeath_combat_ticks: scenario.permadeath_combat_ticks,
            echo_time_eligible: scenario.permadeath_combat_ticks >= 18_000,
            danger_active: false,
            link_lost_ticks: 0,
            dead: true,
        },
        final_deed: FinalDeed {
            deed_id: if scenario.boss_deed {
                DEED_ID.into()
            } else {
                DEED_NONE_ID.into()
            },
            achieved_tick: scenario.boss_deed.then_some(Tick(DEED_TICK)),
        },
        echo_deed_eligible: scenario.boss_deed,
        cause: verified.cause.clone(),
        trace: verified.trace.clone(),
        last_five: verified.last_five.clone(),
        trace_digest: verified.canonical_hash_blake3,
    };
    let server_context = ServerAuthoredDeathContext {
        mutation: DeathMutationAuthority {
            authenticated_account: authenticated_account_for(account_id),
            selected_character_id: identity.character_id,
            former_roster_ordinal: scenario.roster_ordinal,
            mutation_id: identity.death_mutation_id,
            death_id: identity.death_id,
            issued_at_unix_ms: ISSUED_AT_UNIX_MS,
            accepted_at_unix_ms: ISSUED_AT_UNIX_MS,
        },
        world: DeathWorldAuthority {
            instance_id: identity.instance_id,
            lineage_id: identity.lineage_id,
            restore_point_id: identity.restore_point_id,
            region_id: "region.core.microrealm".into(),
            room_id: "room.core.sepulcher".into(),
            lineage_state: DeathLineageState::ActivePermadeath(scenario.provenance),
        },
        content: DurableDeathContentAuthorityV1 {
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            records_blake3: CORE_WORLD_RECORDS_BLAKE3.into(),
            assets_blake3: CORE_WORLD_ASSETS_BLAKE3.into(),
            localization_blake3: CORE_WORLD_LOCALIZATION_BLAKE3.into(),
            enabled_items,
        },
        versions: versions(scenario.account_pre_version, scenario.inventory_pre_version),
        custody,
        hero: DeathHeroSnapshot {
            hero_label_key: "hero.core.grave_arbalist".into(),
            character_name: "Hosted Hero".into(),
            class_id: "class.grave_arbalist".into(),
            level: scenario.level,
            oath_id: None,
            bargain_ids: vec![],
            memorial_presentation_key: "memorial.presentation.core_default".into(),
        },
        terminal_trace: terminal_trace.as_ref().clone(),
        echo: build_echo_projection(scenario, server_computed_echo_power_band),
    };
    build_durable_death_commit(&inputs, &server_context, &presentation).unwrap()
}

#[allow(
    dead_code,
    reason = "the primary graph assertion is consumed by other independently compiled targets"
)]
async fn count(persistence: &PostgresPersistence, query: &'static str) -> i64 {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    let value = sqlx::query_scalar::<_, i64>(query)
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .fetch_one(transaction.connection())
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    value
}

/// Verifies terminal cleanup and exact nonduplication after replay/restart.
#[allow(
    dead_code,
    reason = "the primary graph assertion is consumed by other independently compiled targets"
)]
pub async fn assert_committed_graph(persistence: &PostgresPersistence) {
    for (label, query, expected) in [
        (
            "death events",
            "SELECT count(*) FROM death_events WHERE namespace_id=$1 AND account_id=$2",
            1,
        ),
        (
            "death summaries",
            "SELECT count(*) FROM death_summary_snapshots AS summary JOIN death_events AS death \
             USING (namespace_id,death_id) WHERE summary.namespace_id=$1 AND death.account_id=$2",
            1,
        ),
        (
            "memorials",
            "SELECT count(*) FROM memorial_records WHERE namespace_id=$1 AND account_id=$2",
            1,
        ),
        (
            "destruction entries",
            "SELECT count(*) FROM death_destruction_entries AS destroyed JOIN death_events AS death \
             USING (namespace_id,death_id) WHERE destroyed.namespace_id=$1 AND death.account_id=$2",
            2,
        ),
        (
            "death mutation results",
            "SELECT count(*) FROM death_mutation_results WHERE namespace_id=$1 AND account_id=$2",
            1,
        ),
        (
            "Echoes",
            "SELECT count(*) FROM echo_records WHERE namespace_id=$1 AND account_id=$2",
            1,
        ),
        (
            "Echo transitions",
            "SELECT count(*) FROM echo_state_transitions AS transition JOIN echo_records AS echo \
             USING (namespace_id,echo_id) WHERE transition.namespace_id=$1 AND echo.account_id=$2",
            2,
        ),
        (
            "death outbox events",
            "SELECT count(*) FROM death_outbox_events AS outbox JOIN death_events AS death \
             USING (namespace_id,death_id) WHERE outbox.namespace_id=$1 AND death.account_id=$2",
            3,
        ),
        (
            "retained trace receipts",
            "SELECT count(*) FROM character_live_damage_trace_ingest_receipts_v1 \
             WHERE namespace_id=$1 AND account_id=$2",
            2,
        ),
        (
            "promoted trace sets",
            "SELECT count(*) FROM death_live_trace_sets_v1 WHERE namespace_id=$1 AND account_id=$2",
            1,
        ),
    ] {
        assert_eq!(count(persistence, query).await, expected, "{label}");
    }

    let mut transaction = persistence.begin_transaction().await.unwrap();
    let normalized: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
         (SELECT count(*) FROM character_live_damage_trace_ticks_v1 \
          WHERE namespace_id=$1 AND account_id=$2), \
         (SELECT count(*) FROM character_live_damage_trace_entries_v1 \
          WHERE namespace_id=$1 AND account_id=$2), \
         (SELECT count(*) FROM character_live_damage_trace_statuses_v1 \
          WHERE namespace_id=$1 AND account_id=$2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .fetch_one(transaction.connection())
    .await
    .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(normalized, (0, 0, 0));
}
