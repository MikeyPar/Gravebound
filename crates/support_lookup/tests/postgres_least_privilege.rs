//! Disposable `PostgreSQL` acceptance gate for GB-M03-10.
//!
//! Authorities:
//! - `Gravebound_Production_GDD_v1_Canonical.md` TECH-005, TECH-020, TECH-030,
//!   TECH-050, TECH-120, TECH-122, TECH-124, and TECH-125;
//! - `Gravebound_Content_Production_Spec_v1.md` CONT-LOC-001;
//! - `Gravebound_Development_Roadmap_v1.md` GB-M03-10 and GB-M04-10.

use std::error::Error;

use persistence::{
    DESTRUCTIVE_TEST_OPT_IN_ENV, PersistenceConfig, PostgresPersistence, TEST_DATABASE_URL_ENV,
};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use support_lookup::{
    LookupReason, LookupTarget, OperatorDirectory, OperatorRecord, OperatorToken,
    PostgresSupportLookup, SupportLookupError, SupportLookupRequest, SupportLookupResult,
};
use uuid::Uuid;

const NAMESPACE: &str = "test.core";
const ACCOUNT_ID: [u8; 16] = [11; 16];
const CHARACTER_ID: [u8; 16] = [12; 16];
const ITEM_ID: [u8; 16] = [13; 16];
const MISSING_CHARACTER_ID: [u8; 16] = [14; 16];
const MISSING_ITEM_ID: [u8; 16] = [15; 16];
const MISSING_DEATH_ID: [u8; 16] = [16; 16];
const CREATION_REQUEST_ID: [u8; 16] = [17; 16];
const DEATH_MUTATION_ID: [u8; 16] = [18; 16];
const INSTANCE_ID: [u8; 16] = [19; 16];
const LINEAGE_ID: [u8; 16] = [20; 16];
const RESTORE_POINT_ID: [u8; 16] = [21; 16];
const BARGAIN_CLEANUP_EVENT_ID: [u8; 16] = [22; 16];
const SUPPORT_FUNCTIONS: &[&str] = &[
    "support_lookup_character_v1(bytea)",
    "support_lookup_character_transitions_v1(bytea)",
    "support_lookup_item_v1(bytea)",
    "support_lookup_item_transitions_v1(bytea)",
    "support_lookup_death_v1(bytea)",
    "support_lookup_death_transitions_v1(bytea)",
];

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct Harness {
    persistence: PostgresPersistence,
    admin_pool: PgPool,
    support_pool: PgPool,
    role_name: String,
    death_id: [u8; 16],
}

impl Harness {
    async fn start() -> TestResult<Self> {
        require_destructive_opt_in()?;
        let database_url = std::env::var(TEST_DATABASE_URL_ENV)?;
        let config = PersistenceConfig::from_test_environment()?;
        let persistence = PostgresPersistence::connect(&config).await?;
        persistence.verify_disposable_test_database().await?;
        persistence.migrate().await?;

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await?;
        reset_fixture_data(&admin_pool).await?;

        let role_name = format!("gb_support_smoke_{}", Uuid::now_v7().simple());
        create_support_role(&admin_pool, &role_name).await?;
        let support_pool = role_pool(&database_url, &role_name).await?;
        let death_id = *Uuid::now_v7().as_bytes();
        seed_lookup_fixture(&admin_pool, death_id).await?;

        Ok(Self {
            persistence,
            admin_pool,
            support_pool,
            role_name,
            death_id,
        })
    }

    async fn cleanup(self) -> TestResult {
        self.support_pool.close().await;
        reset_fixture_data(&self.admin_pool).await?;
        let residual_rows: i64 = sqlx::query_scalar(
            "SELECT \
             (SELECT COUNT(*) FROM support_lookup_audit_events_v1) + \
             (SELECT COUNT(*) FROM accounts WHERE namespace_id = 'test.core')",
        )
        .fetch_one(&self.admin_pool)
        .await?;
        check(
            residual_rows == 0,
            "support smoke fixture cleanup left rows",
        )?;
        let role = quoted_role(&self.role_name)?;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP OWNED BY {role}")))
            .execute(&self.admin_pool)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP ROLE {role}")))
            .execute(&self.admin_pool)
            .await?;
        let role_exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
                .bind(&self.role_name)
                .fetch_one(&self.admin_pool)
                .await?;
        check(!role_exists, "support smoke role cleanup failed")?;
        self.admin_pool.close().await;
        self.persistence.close().await;
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn exact_lookup_audit_and_support_role_privileges_fail_closed() {
    let harness = Harness::start().await.unwrap();
    let outcome = exercise_gate(&harness).await;
    let cleanup = harness.cleanup().await;
    outcome.unwrap();
    cleanup.unwrap();
}

async fn exercise_gate(harness: &Harness) -> TestResult {
    assert_exact_privilege_shape(harness).await?;
    let lookup = PostgresSupportLookup::bind_least_privilege(harness.support_pool.clone()).await?;
    let token = OperatorToken::new(vec![0x5a; 48])?;
    let operators = OperatorDirectory::new(vec![OperatorRecord::active_read_only(
        "support.reader-acceptance",
        &token,
    )?])?;

    let requests = [
        request(31, LookupTarget::Character(CHARACTER_ID)),
        request(32, LookupTarget::Character(MISSING_CHARACTER_ID)),
        request(33, LookupTarget::Item(ITEM_ID)),
        request(34, LookupTarget::Item(MISSING_ITEM_ID)),
        request(35, LookupTarget::Death(harness.death_id)),
        request(36, LookupTarget::Death(MISSING_DEATH_ID)),
    ];
    let mut results = Vec::with_capacity(requests.len());
    for request in &requests {
        results.push(
            lookup
                .lookup(&operators, "support.reader-acceptance", &token, request)
                .await?,
        );
    }
    assert_exact_results(&results, harness.death_id)?;
    assert_durable_audits(&harness.admin_pool, &requests).await?;

    let duplicate = lookup
        .lookup(
            &operators,
            "support.reader-acceptance",
            &token,
            &requests[0],
        )
        .await;
    check(
        duplicate == Err(SupportLookupError::ServiceUnavailable),
        "duplicate support request was not rejected",
    )?;
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM support_lookup_audit_events_v1 WHERE namespace_id = $1",
    )
    .bind(NAMESPACE)
    .fetch_one(&harness.admin_pool)
    .await?;
    check(
        audit_count == 6,
        "duplicate request appended a second audit",
    )?;

    assert_direct_access_rejected(&harness.support_pool).await?;
    assert_append_only_trigger(&harness.admin_pool).await?;
    Ok(())
}

fn request(request_byte: u8, target: LookupTarget) -> SupportLookupRequest {
    SupportLookupRequest {
        request_id: [request_byte; 16],
        target,
        reason: LookupReason::IncidentInvestigation,
        case_reference: format!("GB-INCIDENT-{request_byte:04}"),
    }
}

fn assert_exact_results(results: &[SupportLookupResult], death_id: [u8; 16]) -> TestResult {
    check(results.len() == 6, "support result cardinality changed")?;
    match &results[0] {
        SupportLookupResult::Character(value) => {
            check(value.character_id == CHARACTER_ID, "wrong character hit")?;
            check(value.account_id == ACCOUNT_ID, "wrong character owner")?;
            check(value.class_id == "class.grave_arbalist", "wrong class ID")?;
        }
        _ => return Err("exact character lookup did not hit".into()),
    }
    check(
        results[1]
            == SupportLookupResult::NotFound {
                target: LookupTarget::Character(MISSING_CHARACTER_ID),
            },
        "exact character miss changed shape",
    )?;
    match &results[2] {
        SupportLookupResult::Item(value) => {
            check(value.item_uid == ITEM_ID, "wrong item hit")?;
            check(value.character_id == CHARACTER_ID, "wrong item custodian")?;
            check(value.item_version == 1, "wrong item version")?;
        }
        _ => return Err("exact item lookup did not hit".into()),
    }
    check(
        results[3]
            == SupportLookupResult::NotFound {
                target: LookupTarget::Item(MISSING_ITEM_ID),
            },
        "exact item miss changed shape",
    )?;
    match &results[4] {
        SupportLookupResult::Death(value) => {
            check(value.death_id == death_id, "wrong death hit")?;
            check(value.character_id == CHARACTER_ID, "wrong death character")?;
            check(value.trace_digest == [0x44; 32], "wrong death trace digest")?;
        }
        _ => return Err("exact death lookup did not hit".into()),
    }
    check(
        results[5]
            == SupportLookupResult::NotFound {
                target: LookupTarget::Death(MISSING_DEATH_ID),
            },
        "exact death miss changed shape",
    )?;
    Ok(())
}

async fn assert_durable_audits(
    admin_pool: &PgPool,
    requests: &[SupportLookupRequest],
) -> TestResult {
    let rows = sqlx::query(
        "SELECT request_id, target_kind, target_id, outcome_kind, result_count, operator_id, \
         reason_kind, case_reference FROM support_lookup_audit_events_v1 \
         WHERE namespace_id = $1 ORDER BY request_id",
    )
    .bind(NAMESPACE)
    .fetch_all(admin_pool)
    .await?;
    check(rows.len() == requests.len(), "durable audit count changed")?;
    for (index, row) in rows.iter().enumerate() {
        let request = &requests[index];
        let request_id: Vec<u8> = row.try_get("request_id")?;
        let target_id: Vec<u8> = row.try_get("target_id")?;
        let target_kind: i16 = row.try_get("target_kind")?;
        let outcome_kind: i16 = row.try_get("outcome_kind")?;
        let result_count: i16 = row.try_get("result_count")?;
        let operator_id: String = row.try_get("operator_id")?;
        let reason_kind: i16 = row.try_get("reason_kind")?;
        let case_reference: String = row.try_get("case_reference")?;
        let (expected_kind, expected_target) = target_identity(request.target);
        check(request_id == request.request_id, "audit request ID changed")?;
        check(target_id == expected_target, "audit target ID changed")?;
        check(target_kind == expected_kind, "audit target kind changed")?;
        check(
            outcome_kind == i16::from(index % 2 == 1),
            "audit outcome changed",
        )?;
        check(
            result_count == i16::from(index % 2 == 0),
            "audit disclosure count changed",
        )?;
        check(
            operator_id == "support.reader-acceptance",
            "audit operator changed",
        )?;
        check(reason_kind == 1, "audit reason changed")?;
        check(
            case_reference == request.case_reference,
            "audit case reference changed",
        )?;
    }
    Ok(())
}

const fn target_identity(target: LookupTarget) -> (i16, [u8; 16]) {
    match target {
        LookupTarget::Character(id) => (0, id),
        LookupTarget::Item(id) => (1, id),
        LookupTarget::Death(id) => (2, id),
    }
}

async fn assert_exact_privilege_shape(harness: &Harness) -> TestResult {
    let role_flags: (bool, bool, bool, bool, bool, bool) = sqlx::query_as(
        "SELECT rolsuper, rolinherit, rolcreaterole, rolcreatedb, rolcanlogin, rolbypassrls \
         FROM pg_roles WHERE rolname = $1",
    )
    .bind(&harness.role_name)
    .fetch_one(&harness.admin_pool)
    .await?;
    check(
        role_flags == (false, false, false, false, false, false),
        "support role has an administrative or login capability",
    )?;

    let table_grants: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name, privilege_type FROM information_schema.role_table_grants \
         WHERE grantee = $1 AND table_schema = 'public' ORDER BY table_name, privilege_type",
    )
    .bind(&harness.role_name)
    .fetch_all(&harness.admin_pool)
    .await?;
    check(
        table_grants
            == vec![(
                "support_lookup_audit_events_v1".to_owned(),
                "INSERT".to_owned(),
            )],
        "support role has an unexpected explicit relation grant",
    )?;

    let routine_grants: Vec<String> = sqlx::query_scalar(
        "SELECT routine_name FROM information_schema.role_routine_grants \
         WHERE grantee = $1 AND routine_schema = 'public' ORDER BY routine_name",
    )
    .bind(&harness.role_name)
    .fetch_all(&harness.admin_pool)
    .await?;
    let mut expected_routines = SUPPORT_FUNCTIONS
        .iter()
        .map(|signature| signature.split('(').next().unwrap().to_owned())
        .collect::<Vec<_>>();
    expected_routines.sort();
    check(
        routine_grants == expected_routines,
        "support role has an unexpected explicit function grant",
    )?;

    let relations: Vec<String> = sqlx::query_scalar(
        "SELECT class.relname::text FROM pg_class AS class \
         JOIN pg_namespace AS namespace ON namespace.oid = class.relnamespace \
         WHERE namespace.nspname = 'public' AND class.relkind IN ('r', 'p', 'v', 'm') \
         ORDER BY class.relname",
    )
    .fetch_all(&harness.admin_pool)
    .await?;
    for relation in relations {
        for privilege in ["SELECT", "INSERT", "UPDATE", "DELETE", "TRUNCATE"] {
            let granted: bool = sqlx::query_scalar("SELECT has_table_privilege($1, $2, $3)")
                .bind(&harness.role_name)
                .bind(format!("public.{relation}"))
                .bind(privilege)
                .fetch_one(&harness.admin_pool)
                .await?;
            let expected = relation == "support_lookup_audit_events_v1" && privilege == "INSERT";
            check(
                granted == expected,
                "support role relation privilege shape is broader than approved",
            )?;
        }
    }
    Ok(())
}

async fn assert_direct_access_rejected(support_pool: &PgPool) -> TestResult {
    for statement in [
        "SELECT character_id FROM characters LIMIT 1",
        "SELECT character_id FROM support_character_lookup_v1 LIMIT 1",
        "UPDATE characters SET level = level WHERE false",
        "DELETE FROM item_instances WHERE false",
        "INSERT INTO characters DEFAULT VALUES",
        "UPDATE support_lookup_audit_events_v1 SET case_reference = case_reference WHERE false",
        "DELETE FROM support_lookup_audit_events_v1 WHERE false",
    ] {
        let error = sqlx::query(statement)
            .execute(support_pool)
            .await
            .expect_err("support role unexpectedly executed prohibited SQL");
        check_sqlstate(&error, "42501")?;
    }
    Ok(())
}

async fn assert_append_only_trigger(admin_pool: &PgPool) -> TestResult {
    for statement in [
        "UPDATE support_lookup_audit_events_v1 SET case_reference = case_reference \
         WHERE namespace_id = 'test.core'",
        "DELETE FROM support_lookup_audit_events_v1 WHERE namespace_id = 'test.core'",
    ] {
        let error = sqlx::query(statement)
            .execute(admin_pool)
            .await
            .expect_err("database owner bypassed append-only support audit trigger");
        check_sqlstate(&error, "P0001")?;
    }
    Ok(())
}

async fn create_support_role(admin_pool: &PgPool, role_name: &str) -> TestResult {
    let role = quoted_role(role_name)?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT"
    )))
    .execute(admin_pool)
    .await?;
    for function in SUPPORT_FUNCTIONS {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "GRANT EXECUTE ON FUNCTION {function} TO {role}"
        )))
        .execute(admin_pool)
        .await?;
    }
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "GRANT INSERT ON TABLE support_lookup_audit_events_v1 TO {role}"
    )))
    .execute(admin_pool)
    .await?;
    Ok(())
}

async fn role_pool(database_url: &str, role_name: &str) -> TestResult<PgPool> {
    let role_statement = format!("SET ROLE {}", quoted_role(role_name)?);
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |connection, _metadata| {
            let role_statement = role_statement.clone();
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(role_statement))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await?;
    Ok(pool)
}

async fn seed_lookup_fixture(admin_pool: &PgPool, death_id: [u8; 16]) -> TestResult {
    let mut transaction = admin_pool.begin().await?;
    // The support adapter is under test, not the already-gated terminal-death writer. Replica
    // mode suppresses only terminal-graph FK/closure triggers while the row still satisfies every
    // current NOT NULL and CHECK contract. The disposable fixture is truncated after the probe.
    sqlx::query("SET LOCAL session_replication_role = 'replica'")
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO accounts (namespace_id, account_id, state_version, slot_capacity) \
         VALUES ($1, $2, 7, 2)",
    )
    .bind(NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO characters (namespace_id, account_id, character_id, roster_ordinal, \
         class_id, level, oath_id, life_state, security_state) \
         VALUES ($1, $2, $3, 1, 'class.grave_arbalist', 10, \
         'oath.arbalist.long_vigil', 0, 0)",
    )
    .bind(NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO character_inventories \
         (namespace_id, account_id, character_id, inventory_version) VALUES ($1, $2, $3, 3)",
    )
    .bind(NAMESPACE)
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .execute(&mut *transaction)
    .await?;
    let content_revision = format!("core-dev.blake3.{}", "a".repeat(64));
    sqlx::query(
        "INSERT INTO item_instances \
         (namespace_id, item_uid, account_id, character_id, template_id, content_revision, \
          item_kind, item_level, rarity, creation_kind, creation_request_id, roll_index, \
          unit_ordinal, item_version, security_state, location_kind, slot_index, provenance_kind, \
          salvage_band, salvage_value) \
         VALUES ($1,$2,$3,$4,'item.weapon.crossbow.pine_crossbow',$5,0,1,0,0,$6,0,0,1,0,0,0,0,0,0)",
    )
    .bind(NAMESPACE)
    .bind(ITEM_ID.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(&content_revision)
    .bind(CREATION_REQUEST_ID.as_slice())
    .execute(&mut *transaction)
    .await?;
    let hash_a = "a".repeat(64);
    let hash_b = "b".repeat(64);
    let hash_c = "c".repeat(64);
    sqlx::query(
        "INSERT INTO death_events \
         (namespace_id,death_id,account_id,character_id,contract_kind,mutation_id, \
          canonical_request_hash,content_revision,instance_id,lineage_id,restore_point_id, \
          region_id,room_id,death_tick,cause_kind,killer_content_id,killer_pattern_id, \
          killer_attack_id,raw_damage,final_damage,damage_type,pre_hit_health,source_x_milli_tiles, \
          source_y_milli_tiles,network_state,recall_state,lifetime_ticks,permadeath_combat_ticks, \
          pre_account_version,post_account_version,pre_character_version,post_character_version, \
          pre_progression_version,post_progression_version,pre_inventory_version, \
          post_inventory_version,pre_life_metrics_version,post_life_metrics_version,trace_digest, \
          former_roster_ordinal,echo_expected,preexisting_available_echo_id,promoted_echo_id, \
          world_records_blake3,world_assets_blake3,world_localization_blake3, \
          presentation_records_blake3,presentation_assets_blake3, \
          presentation_localization_blake3,bargain_cleanup_event_id,pre_oath_bargain_version, \
          post_oath_bargain_version,death_provenance) \
         VALUES ($1,$2,$3,$4,'permadeath-v1',$5,$6,$7,$8,$9,$10,'region.core.microrealm', \
          'room.b1',900,0,'enemy.drowned_pilgrim',NULL,'attack.pilgrim.fan',10,10,0,10,0,0,0,0, \
          900,300,1,2,1,2,1,2,1,2,1,2,$11,1,false,NULL,NULL,$12,$13,$14,$12,$13,$14,$15,1,2,0)",
    )
    .bind(NAMESPACE)
    .bind(death_id.as_slice())
    .bind(ACCOUNT_ID.as_slice())
    .bind(CHARACTER_ID.as_slice())
    .bind(DEATH_MUTATION_ID.as_slice())
    .bind([0x33; 32].as_slice())
    .bind(&content_revision)
    .bind(INSTANCE_ID.as_slice())
    .bind(LINEAGE_ID.as_slice())
    .bind(RESTORE_POINT_ID.as_slice())
    .bind([0x44; 32].as_slice())
    .bind(&hash_a)
    .bind(&hash_b)
    .bind(&hash_c)
    .bind(BARGAIN_CLEANUP_EVENT_ID.as_slice())
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn reset_fixture_data(admin_pool: &PgPool) -> TestResult {
    sqlx::query(
        "TRUNCATE TABLE support_lookup_audit_events_v1, accounts, \
         caldus_victory_exits CASCADE",
    )
    .execute(admin_pool)
    .await?;
    Ok(())
}

fn require_destructive_opt_in() -> TestResult {
    check(
        std::env::var(DESTRUCTIVE_TEST_OPT_IN_ENV).as_deref() == Ok("1"),
        "destructive PostgreSQL opt-in is required",
    )
}

fn quoted_role(role_name: &str) -> TestResult<String> {
    check(
        role_name.len() <= 63
            && role_name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "generated PostgreSQL role name is unsafe",
    )?;
    Ok(format!("\"{role_name}\""))
}

fn check_sqlstate(error: &sqlx::Error, expected: &str) -> TestResult {
    let actual = error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .map(std::borrow::Cow::into_owned);
    check(
        actual.as_deref() == Some(expected),
        "PostgreSQL rejection used the wrong SQLSTATE",
    )
}

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
