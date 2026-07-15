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
    DurableDestructionEntryV1, DurableDestructionLocationV1, DurableEquipmentSlotV1,
    LiveDamageTraceContentAuthorityV1, LiveDamageTraceDangerAuthorityV1, PostgresPersistence,
    WIPEABLE_CORE_NAMESPACE, stage_danger_entry_ash_wallet_restore_v3,
    stage_danger_entry_inventory_restore_v3, stage_danger_entry_life_metrics_restore_v3,
    stage_danger_entry_oath_bargain_restore_v3,
};
use protocol::{DeathViewContentRevisionV1, ManifestHash};
use server_app::{
    AccountId, AuthenticatedAccount, AuthenticatedNamespace, DeathEntityIdentityAuthority,
    DeathHeroSnapshot, DeathLineageState, DeathMutationAuthority, DeathProvenance,
    DeathWorldAuthority, EchoAvailabilityProjection, EligibleEchoProjection,
    LiveDamageTraceBinding, LiveDamageTraceIngestOutcome, LiveDamageTraceMutationAuthority,
    LiveDamageTraceService, PreparedDurableDeathCommit, ServerAuthoredDeathContext,
    build_durable_death_commit,
};
use sim_core::{
    AuthoritativeDeathCauseKind, AuthoritativeDeathInputs, DamageTraceObservation, DamageType,
    DeathClockSnapshot, DeathTraceNetworkState, DeathTraceRecallState, DeathTraceStatus, EntityId,
    FinalDeed, SimulationVector, Tick, ticks_to_milliseconds,
};

pub const ACCOUNT_ID: [u8; 16] = [230; 16];
pub const CHARACTER_ID: [u8; 16] = [231; 16];
pub const LINEAGE_ID: [u8; 16] = [232; 16];
pub const RESTORE_POINT_ID: [u8; 16] = [233; 16];
pub const INSTANCE_ID: [u8; 16] = [234; 16];
pub const ITEM_UID: [u8; 16] = [235; 16];
pub const ITEM_LEDGER_ID: [u8; 16] = [236; 16];
pub const ENTRY_MUTATION_ID: [u8; 16] = [237; 16];
pub const DEED_REWARD_ID: [u8; 16] = [238; 16];
pub const NONLETHAL_TRACE_TICK_ID: [u8; 16] = [239; 16];
pub const LETHAL_TRACE_TICK_ID: [u8; 16] = [240; 16];
pub const DEATH_ID: [u8; 16] = uuid_v7(41);
pub const ECHO_ID: [u8; 16] = uuid_v7(42);
pub const DEATH_MUTATION_ID: [u8; 16] = uuid_v7(43);
pub const MATERIAL_ID: &str = "material.bell_brass";
pub const ITEM_TEMPLATE_ID: &str = "item.weapon.crossbow.pine_crossbow";
pub const DEED_ID: &str = "deed.core.sir_caldus_defeated";
pub const SOURCE_SIM_ENTITY_ID: u64 = 81;

const ISSUED_AT_UNIX_MS: u64 = 1;

const fn uuid_v7(seed: u8) -> [u8; 16] {
    let mut value = [seed; 16];
    value[6] = 0x70 | (seed & 0x0f);
    value[8] = 0x80 | (seed & 0x3f);
    value
}

pub fn authenticated_account() -> AuthenticatedAccount {
    AuthenticatedAccount {
        account_id: AccountId::new(ACCOUNT_ID).unwrap(),
        namespace: AuthenticatedNamespace::WipeableTest,
    }
}

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
    clippy::too_many_lines,
    reason = "the complete danger root remains explicit for hosted authority review"
)]
pub async fn seed_danger_root(persistence: &PostgresPersistence) {
    let mut transaction = persistence.begin_transaction().await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE namespace_id=$1 AND account_id=$2")
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(ACCOUNT_ID.as_slice())
        .execute(transaction.connection())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (namespace_id,account_id,state_version,slot_capacity) \
         VALUES ($1,$2,1,2)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ash_wallets (namespace_id,account_id,balance,wallet_version) \
         VALUES ($1,$2,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO characters (namespace_id,account_id,character_id,roster_ordinal,class_id, \
         level,oath_id,life_state,security_state,character_state_version) \
         VALUES ($1,$2,$3,1,'class.grave_arbalist',10,NULL,0,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE accounts SET selected_character_id=$1 WHERE namespace_id=$2 AND account_id=$3",
    )
    .bind(CHARACTER_ID.as_slice())
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_progression (namespace_id,account_id,character_id,total_xp,level, \
         current_health,progression_version) VALUES ($1,$2,$3,2700,10,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_inventories (namespace_id,account_id,character_id,inventory_version) \
         VALUES ($1,$2,$3,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_oath_bargain_state (namespace_id,account_id,character_id, \
         earned_bargain_slots,oath_bargain_version) VALUES ($1,$2,$3,0,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
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
    .bind(ITEM_UID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
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
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
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
         'hub.lantern_halls_01',3,1,1,1,2,1,1,1,31,$6,0,$7,$8,$9)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
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
         VALUES ($1,$2,$3,$4,10,2700,120,1,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .bind([92_u8; 32].as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO entry_restore_progression_v1 (namespace_id,account_id,character_id, \
         restore_point_id,level,total_xp,current_health,progression_version) \
         VALUES ($1,$2,$3,$4,10,2700,120,1)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    stage_danger_entry_inventory_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
        ENTRY_MUTATION_ID,
        0,
    )
    .await
    .unwrap();
    stage_danger_entry_oath_bargain_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    stage_danger_entry_life_metrics_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    stage_danger_entry_ash_wallet_restore_v3(
        &mut transaction,
        ACCOUNT_ID,
        CHARACTER_ID,
        RESTORE_POINT_ID,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_world_locations (namespace_id,account_id,character_id, \
         character_version,location_kind,location_content_id,instance_lineage_id, \
         entry_restore_point_id) VALUES ($1,$2,$3,2,2,'world.core_microrealm_01',$4,$5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE characters SET character_state_version=2 WHERE namespace_id=$1 \
         AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_progression SET current_health=50,progression_version=2 \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE character_life_metrics SET lifetime_ticks=19990,permadeath_combat_ticks=17990, \
         life_metrics_version=2 WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
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
         checkpoint_payload_digest) VALUES ($1,$2,$3,$4,19990,15,$5,2,2,2,1,$6,$7,$8,1,$9,$10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
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
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(MATERIAL_ID)
    .execute(transaction.connection())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO character_life_deeds (namespace_id,account_id,character_id,deed_id, \
         reward_event_id,source_content_id,deed_kind,achieved_tick,content_revision) \
         VALUES ($1,$2,$3,$4,$5,'boss.sir_caldus',0,19000,$6)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(DEED_ID)
    .bind(DEED_REWARD_ID.as_slice())
    .bind(CORE_ITEM_CONTENT_REVISION)
    .execute(transaction.connection())
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn observation(tick: u64, pre_health: u32, final_damage: u32) -> DamageTraceObservation {
    DamageTraceObservation {
        tick: Tick(tick),
        event_ordinal: 0,
        cause_kind: AuthoritativeDeathCauseKind::DirectHit,
        source_content_id: "miniboss.sepulcher_knight".into(),
        source_entity_id: Some(EntityId::new(SOURCE_SIM_ENTITY_ID).unwrap()),
        pattern_id: Some("miniboss.sepulcher_knight.charge_lane".into()),
        attack_id: "miniboss.sepulcher_knight.charge_lane".into(),
        raw_damage: final_damage,
        final_damage,
        damage_type: DamageType::Physical,
        pre_health,
        post_health: pre_health.saturating_sub(final_damage),
        source_position: SimulationVector::new(1.25, -0.5),
        statuses: (tick == 19_990)
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

fn versions() -> DeathAggregateVersionsV1 {
    DeathAggregateVersionsV1 {
        account: DeathVersionAdvanceV1 { pre: 1, post: 2 },
        character: DeathVersionAdvanceV1 { pre: 2, post: 3 },
        progression: DeathVersionAdvanceV1 { pre: 2, post: 3 },
        inventory: DeathVersionAdvanceV1 { pre: 2, post: 3 },
        oath_bargain: DeathVersionAdvanceV1 { pre: 1, post: 2 },
        life_metrics: DeathVersionAdvanceV1 { pre: 2, post: 3 },
    }
}

/// Produces one sealed death by traversing the production live trace and server death builder.
#[allow(
    clippy::too_many_lines,
    reason = "the trace, simulation evidence, and sealed server context stay contiguous for audit"
)]
pub async fn prepare_death(persistence: PostgresPersistence) -> PreparedDurableDeathCommit {
    let presentation = Arc::new(
        sim_content::load_core_development_death_view(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content"),
        )
        .unwrap(),
    );
    let danger = LiveDamageTraceDangerAuthorityV1 {
        lineage_id: LINEAGE_ID,
        restore_point_id: RESTORE_POINT_ID,
        checkpoint_tick: 19_990,
    };
    let content = LiveDamageTraceContentAuthorityV1::core();
    let identities = DeathEntityIdentityAuthority {
        by_sim_entity: BTreeMap::from([(
            EntityId::new(SOURCE_SIM_ENTITY_ID).unwrap(),
            [u8::try_from(SOURCE_SIM_ENTITY_ID).unwrap(); 16],
        )]),
    };
    let mut trace = LiveDamageTraceService::start_or_resume(
        persistence,
        LiveDamageTraceBinding::new(ACCOUNT_ID, CHARACTER_ID, 2, danger.clone(), content.clone())
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
                    NONLETHAL_TRACE_TICK_ID,
                    2,
                    danger.clone(),
                    ISSUED_AT_UNIX_MS,
                )
                .unwrap(),
                vec![observation(19_990, 60, 10)],
            )
            .await
            .unwrap(),
        LiveDamageTraceIngestOutcome::Committed(_)
    ));
    let LiveDamageTraceIngestOutcome::TerminalPrepared(terminal_trace) = trace
        .ingest_tick(
            LiveDamageTraceMutationAuthority::new(
                LETHAL_TRACE_TICK_ID,
                2,
                danger,
                ISSUED_AT_UNIX_MS,
            )
            .unwrap(),
            vec![observation(20_000, 50, 50)],
        )
        .await
        .unwrap()
    else {
        panic!("lethal server observation must prepare terminal evidence")
    };
    let verified = terminal_trace.terminal_snapshot();
    let inputs = AuthoritativeDeathInputs {
        clocks: DeathClockSnapshot {
            lifetime_ticks: 20_000,
            lifetime_ms: ticks_to_milliseconds(20_000).unwrap(),
            permadeath_combat_ticks: 18_000,
            echo_time_eligible: true,
            danger_active: false,
            link_lost_ticks: 0,
            dead: true,
        },
        final_deed: FinalDeed {
            deed_id: DEED_ID.into(),
            achieved_tick: Some(Tick(19_000)),
        },
        echo_deed_eligible: true,
        cause: verified.cause.clone(),
        trace: verified.trace.clone(),
        last_five: verified.last_five.clone(),
        trace_digest: verified.canonical_hash_blake3,
    };
    let server_context = ServerAuthoredDeathContext {
        mutation: DeathMutationAuthority {
            authenticated_account: authenticated_account(),
            selected_character_id: CHARACTER_ID,
            former_roster_ordinal: 1,
            mutation_id: DEATH_MUTATION_ID,
            death_id: DEATH_ID,
            issued_at_unix_ms: ISSUED_AT_UNIX_MS,
            accepted_at_unix_ms: ISSUED_AT_UNIX_MS,
        },
        world: DeathWorldAuthority {
            instance_id: INSTANCE_ID,
            lineage_id: LINEAGE_ID,
            restore_point_id: RESTORE_POINT_ID,
            region_id: "region.core.microrealm".into(),
            room_id: "room.core.sepulcher".into(),
            lineage_state: DeathLineageState::ActivePermadeath(DeathProvenance::OrdinaryGameplay),
        },
        content: DurableDeathContentAuthorityV1 {
            content_revision: CORE_ITEM_CONTENT_REVISION.into(),
            records_blake3: CORE_WORLD_RECORDS_BLAKE3.into(),
            assets_blake3: CORE_WORLD_ASSETS_BLAKE3.into(),
            localization_blake3: CORE_WORLD_LOCALIZATION_BLAKE3.into(),
            enabled_items: vec![DurableDeathItemContentAuthorityV1 {
                template_id: ITEM_TEMPLATE_ID.into(),
                echo_signature_tag: None,
            }],
        },
        versions: versions(),
        destruction: vec![
            DurableDestructionEntryV1::Item {
                ordinal: 0,
                content_id: ITEM_TEMPLATE_ID.into(),
                item_uid: ITEM_UID,
                location: DurableDestructionLocationV1::Equipment {
                    slot: DurableEquipmentSlotV1::Weapon,
                },
                pre_item_version: 2,
                post_item_version: 3,
                ledger_event_id: ITEM_LEDGER_ID,
            },
            DurableDestructionEntryV1::RunMaterial {
                ordinal: 1,
                material_id: MATERIAL_ID.into(),
                destroyed_quantity: 7,
                pre_material_quantity: 7,
                pre_material_version: 1,
                post_material_version: 2,
            },
        ],
        hero: DeathHeroSnapshot {
            hero_label_key: "hero.core.grave_arbalist".into(),
            character_name: "Hosted Hero".into(),
            class_id: "class.grave_arbalist".into(),
            level: 10,
            oath_id: None,
            bargain_ids: vec![],
            memorial_presentation_key: "memorial.presentation.core_default".into(),
        },
        entity_identities: identities,
        terminal_trace: terminal_trace.as_ref().clone(),
        echo: Some(EligibleEchoProjection {
            echo_id: ECHO_ID,
            appearance_snapshot_id: "appearance.default.grave_arbalist".into(),
            appearance_theme_id: "theme.echo.arbalist_ash".into(),
            weapon_signature_tag: None,
            relic_signature_tag: None,
            deed_tags: vec![DEED_ID.into()],
            power_band: 1,
            availability: EchoAvailabilityProjection::PromoteOldestDormant {
                echo_id: ECHO_ID,
                echo_death_id: DEATH_ID,
                next_transition_ordinal: 1,
            },
        }),
    };
    build_durable_death_commit(&inputs, &server_context, &presentation).unwrap()
}

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
