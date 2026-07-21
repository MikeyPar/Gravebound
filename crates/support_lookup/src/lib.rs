//! Authenticated, bounded, read-only support lookup for GB-M03-10.
//!
//! Authority comes from `Gravebound_Production_GDD_v1_Canonical.md` TECH-005, TECH-020,
//! TECH-030, TECH-050, TECH-120, TECH-122, TECH-124, and TECH-125;
//! `Gravebound_Content_Production_Spec_v1.md` CONT-LOC-001; and
//! `Gravebound_Development_Roadmap_v1.md` GB-M03-10/GB-M04-10.

use std::fmt;

use serde::Serialize;
use sqlx::{PgPool, Postgres, Row, Transaction};
use thiserror::Error;

const WIPEABLE_CORE_NAMESPACE: &str = "test.core";
const OPERATOR_TOKEN_DOMAIN: &str = "gravebound/support/operator-token/v1";
const MIN_OPERATOR_TOKEN_BYTES: usize = 32;
const MAX_OPERATOR_TOKEN_BYTES: usize = 256;
const MAX_OPERATOR_RECORDS: usize = 256;
const MAX_TRANSITIONS: i64 = 64;

pub type DurableId = [u8; 16];
pub type Digest = [u8; 32];

#[derive(Clone, PartialEq, Eq)]
pub struct OperatorToken(Vec<u8>);

impl OperatorToken {
    pub fn new(bytes: Vec<u8>) -> Result<Self, SupportLookupError> {
        if !(MIN_OPERATOR_TOKEN_BYTES..=MAX_OPERATOR_TOKEN_BYTES).contains(&bytes.len()) {
            return Err(SupportLookupError::InvalidCredential);
        }
        Ok(Self(bytes))
    }

    fn digest(&self) -> Digest {
        blake3::derive_key(OPERATOR_TOKEN_DOMAIN, &self.0)
    }
}

impl fmt::Debug for OperatorToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperatorToken([REDACTED])")
    }
}

impl Drop for OperatorToken {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportRole {
    ReadOnlyLookup,
}

#[derive(Clone, PartialEq, Eq)]
pub struct OperatorRecord {
    operator_id: String,
    token_digest: Digest,
    role: SupportRole,
    active: bool,
}

impl fmt::Debug for OperatorRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OperatorRecord")
            .field("operator_id", &self.operator_id)
            .field("token_digest", &"[REDACTED]")
            .field("role", &self.role)
            .field("active", &self.active)
            .finish()
    }
}

impl OperatorRecord {
    pub fn active_read_only(
        operator_id: impl Into<String>,
        token: &OperatorToken,
    ) -> Result<Self, SupportLookupError> {
        let operator_id = operator_id.into();
        validate_operator_id(&operator_id)?;
        Ok(Self {
            operator_id,
            token_digest: token.digest(),
            role: SupportRole::ReadOnlyLookup,
            active: true,
        })
    }

    #[must_use]
    pub fn disabled(mut self) -> Self {
        self.active = false;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorPrincipal {
    operator_id: String,
    role: SupportRole,
}

impl OperatorPrincipal {
    #[must_use]
    pub fn operator_id(&self) -> &str {
        &self.operator_id
    }

    #[must_use]
    pub const fn role(&self) -> SupportRole {
        self.role
    }
}

#[derive(Debug, Clone, Default)]
pub struct OperatorDirectory {
    records: Vec<OperatorRecord>,
}

impl OperatorDirectory {
    pub fn new(records: Vec<OperatorRecord>) -> Result<Self, SupportLookupError> {
        if records.is_empty() || records.len() > MAX_OPERATOR_RECORDS {
            return Err(SupportLookupError::InvalidOperatorDirectory);
        }
        for (index, record) in records.iter().enumerate() {
            validate_operator_id(&record.operator_id)?;
            if records[..index]
                .iter()
                .any(|existing| existing.operator_id == record.operator_id)
            {
                return Err(SupportLookupError::InvalidOperatorDirectory);
            }
        }
        Ok(Self { records })
    }

    pub fn authenticate(
        &self,
        operator_id: &str,
        token: &OperatorToken,
    ) -> Result<OperatorPrincipal, SupportLookupError> {
        validate_operator_id(operator_id).map_err(|_| SupportLookupError::Unauthorized)?;
        let candidate = token.digest();
        let record = self
            .records
            .iter()
            .find(|record| record.operator_id == operator_id)
            .ok_or(SupportLookupError::Unauthorized)?;
        if !record.active || !constant_time_equal(&record.token_digest, &candidate) {
            return Err(SupportLookupError::Unauthorized);
        }
        Ok(OperatorPrincipal {
            operator_id: record.operator_id.clone(),
            role: record.role,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupTarget {
    Character(DurableId),
    Item(DurableId),
    Death(DurableId),
}

impl LookupTarget {
    const fn kind_code(self) -> i16 {
        match self {
            Self::Character(_) => 0,
            Self::Item(_) => 1,
            Self::Death(_) => 2,
        }
    }

    const fn durable_id(self) -> DurableId {
        match self {
            Self::Character(id) | Self::Item(id) | Self::Death(id) => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupReason {
    PlayerSupport,
    IncidentInvestigation,
    IntegrityReview,
}

impl LookupReason {
    const fn code(self) -> i16 {
        match self {
            Self::PlayerSupport => 0,
            Self::IncidentInvestigation => 1,
            Self::IntegrityReview => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportLookupRequest {
    pub request_id: DurableId,
    pub target: LookupTarget,
    pub reason: LookupReason,
    pub case_reference: String,
}

impl SupportLookupRequest {
    pub fn validate(&self) -> Result<(), SupportLookupError> {
        if is_zero(&self.request_id)
            || is_zero(&self.target.durable_id())
            || !valid_case_reference(&self.case_reference)
        {
            return Err(SupportLookupError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CharacterLookup {
    pub namespace_id: String,
    pub account_id: DurableId,
    pub character_id: DurableId,
    pub roster_ordinal: i16,
    pub class_id: String,
    pub level: i32,
    pub oath_id: Option<String>,
    pub life_state: i16,
    pub security_state: i16,
    pub account_version: i64,
    pub created_at_unix_millis: i64,
    pub updated_at_unix_millis: i64,
    pub transitions: Vec<CharacterTransition>,
    pub transitions_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CharacterTransition {
    pub event_id: DurableId,
    pub event_kind: i16,
    pub pre_state_version: i64,
    pub post_state_version: i64,
    pub result_code: i16,
    pub related_id: Option<DurableId>,
    pub committed_at_unix_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemLookup {
    pub namespace_id: String,
    pub account_id: DurableId,
    pub character_id: DurableId,
    pub item_uid: DurableId,
    pub template_id: String,
    pub content_revision: String,
    pub item_version: i64,
    pub security_state: i16,
    pub location_kind: i16,
    pub slot_index: Option<i16>,
    pub creation_request_id: DurableId,
    pub created_at_unix_millis: i64,
    pub updated_at_unix_millis: i64,
    pub transitions: Vec<ItemTransition>,
    pub transitions_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemTransition {
    pub event_id: DurableId,
    pub mutation_id: DurableId,
    pub event_kind: i16,
    pub source_kind: i16,
    pub pre_state_version: i64,
    pub post_state_version: i64,
    pub pre_security_state: Option<i16>,
    pub post_security_state: i16,
    pub pre_location_kind: Option<i16>,
    pub post_location_kind: i16,
    pub reason: Option<String>,
    pub committed_at_unix_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeathLookup {
    pub namespace_id: String,
    pub death_id: DurableId,
    pub account_id: DurableId,
    pub character_id: DurableId,
    pub mutation_id: DurableId,
    pub content_revision: String,
    pub instance_id: DurableId,
    pub lineage_id: DurableId,
    pub restore_point_id: DurableId,
    pub region_id: String,
    pub room_id: String,
    pub death_tick: i64,
    pub cause_kind: i16,
    pub killer_content_id: Option<String>,
    pub killer_pattern_id: Option<String>,
    pub killer_attack_id: Option<String>,
    pub final_damage: i32,
    pub damage_type: i16,
    pub pre_hit_health: i32,
    pub network_state: i16,
    pub recall_state: i16,
    pub pre_character_version: i64,
    pub post_character_version: i64,
    pub trace_digest: Digest,
    pub committed_at_unix_millis: i64,
    pub transitions: Vec<DeathTransition>,
    pub transitions_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeathTransition {
    pub event_id: DurableId,
    pub mutation_id: DurableId,
    pub event_kind: i16,
    pub event_digest: Digest,
    pub committed_at_unix_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SupportLookupResult {
    Character(CharacterLookup),
    Item(ItemLookup),
    Death(DeathLookup),
    NotFound { target: LookupTarget },
}

impl SupportLookupResult {
    fn disclosed_row_count(&self) -> i16 {
        let count = match self {
            Self::Character(value) => 1 + value.transitions.len(),
            Self::Item(value) => 1 + value.transitions.len(),
            Self::Death(value) => 1 + value.transitions.len(),
            Self::NotFound { .. } => 0,
        };
        i16::try_from(count).expect("bounded lookup result count fits i16")
    }
}

#[derive(Debug, Clone)]
pub struct PostgresSupportLookup {
    pool: PgPool,
}

impl PostgresSupportLookup {
    /// Binds only when the supplied database role has the exact read-view/audit-insert shape.
    pub async fn bind_least_privilege(pool: PgPool) -> Result<Self, SupportLookupError> {
        let lookup = Self { pool };
        lookup.verify_least_privilege().await?;
        Ok(lookup)
    }

    pub async fn verify_least_privilege(&self) -> Result<(), SupportLookupError> {
        for function in [
            "support_lookup_character_v1(bytea)",
            "support_lookup_character_transitions_v1(bytea)",
            "support_lookup_item_v1(bytea)",
            "support_lookup_item_transitions_v1(bytea)",
            "support_lookup_death_v1(bytea)",
            "support_lookup_death_transitions_v1(bytea)",
        ] {
            if !has_function_privilege(&self.pool, function, "EXECUTE").await? {
                return Err(SupportLookupError::LeastPrivilegeViolation);
            }
        }
        if !has_table_privilege(&self.pool, "support_lookup_audit_events_v1", "INSERT").await?
            || has_table_privilege(&self.pool, "support_lookup_audit_events_v1", "UPDATE").await?
            || has_table_privilege(&self.pool, "support_lookup_audit_events_v1", "DELETE").await?
        {
            return Err(SupportLookupError::LeastPrivilegeViolation);
        }
        for relation in [
            "accounts",
            "characters",
            "character_world_transfer_results",
            "item_instances",
            "item_ledger_events",
            "death_events",
            "death_audit_events",
            "support_character_lookup_v1",
            "support_character_transition_lookup_v1",
            "support_item_lookup_v1",
            "support_item_transition_lookup_v1",
            "support_death_lookup_v1",
            "support_death_transition_lookup_v1",
        ] {
            for privilege in ["SELECT", "INSERT", "UPDATE", "DELETE"] {
                if has_table_privilege(&self.pool, relation, privilege).await? {
                    return Err(SupportLookupError::LeastPrivilegeViolation);
                }
            }
        }
        Ok(())
    }

    pub async fn lookup(
        &self,
        operators: &OperatorDirectory,
        operator_id: &str,
        token: &OperatorToken,
        request: &SupportLookupRequest,
    ) -> Result<SupportLookupResult, SupportLookupError> {
        request.validate()?;
        let principal = operators.authenticate(operator_id, token)?;
        if principal.role() != SupportRole::ReadOnlyLookup {
            return Err(SupportLookupError::Unauthorized);
        }

        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        let result = match request.target {
            LookupTarget::Character(character_id) => {
                lookup_character(&mut transaction, character_id).await?
            }
            LookupTarget::Item(item_uid) => lookup_item(&mut transaction, item_uid).await?,
            LookupTarget::Death(death_id) => lookup_death(&mut transaction, death_id).await?,
        };
        transaction.commit().await.map_err(database_error)?;

        append_audit(&self.pool, &principal, request, &result).await?;
        Ok(result)
    }
}

async fn has_table_privilege(
    pool: &PgPool,
    relation: &str,
    privilege: &str,
) -> Result<bool, SupportLookupError> {
    sqlx::query_scalar("SELECT has_table_privilege(current_user, $1, $2)")
        .bind(relation)
        .bind(privilege)
        .fetch_one(pool)
        .await
        .map_err(database_error)
}

async fn has_function_privilege(
    pool: &PgPool,
    function: &str,
    privilege: &str,
) -> Result<bool, SupportLookupError> {
    sqlx::query_scalar("SELECT has_function_privilege(current_user, $1, $2)")
        .bind(function)
        .bind(privilege)
        .fetch_one(pool)
        .await
        .map_err(database_error)
}

async fn lookup_character(
    transaction: &mut Transaction<'_, Postgres>,
    character_id: DurableId,
) -> Result<SupportLookupResult, SupportLookupError> {
    let row = sqlx::query(
        "SELECT namespace_id, account_id, character_id, roster_ordinal, class_id, level, oath_id, \
         life_state, security_state, account_version, \
         (extract(epoch FROM created_at) * 1000)::bigint AS created_at_ms, \
         (extract(epoch FROM updated_at) * 1000)::bigint AS updated_at_ms \
         FROM support_lookup_character_v1($1)",
    )
    .bind(character_id.as_slice())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?;
    let Some(row) = row else {
        return Ok(SupportLookupResult::NotFound {
            target: LookupTarget::Character(character_id),
        });
    };
    let transition_rows = sqlx::query(
        "SELECT event_id, event_kind, pre_state_version, post_state_version, result_code, \
         related_id, (extract(epoch FROM committed_at) * 1000)::bigint AS committed_at_ms \
         FROM support_lookup_character_transitions_v1($1)",
    )
    .bind(character_id.as_slice())
    .fetch_all(&mut **transaction)
    .await
    .map_err(database_error)?;
    let (transition_rows, truncated) = bounded_rows(transition_rows);
    let transitions = transition_rows
        .iter()
        .map(character_transition)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SupportLookupResult::Character(CharacterLookup {
        namespace_id: exact_namespace(&row)?,
        account_id: durable_id(&row, "account_id")?,
        character_id: durable_id(&row, "character_id")?,
        roster_ordinal: row.try_get("roster_ordinal").map_err(database_error)?,
        class_id: row.try_get("class_id").map_err(database_error)?,
        level: row.try_get("level").map_err(database_error)?,
        oath_id: row.try_get("oath_id").map_err(database_error)?,
        life_state: row.try_get("life_state").map_err(database_error)?,
        security_state: row.try_get("security_state").map_err(database_error)?,
        account_version: row.try_get("account_version").map_err(database_error)?,
        created_at_unix_millis: row.try_get("created_at_ms").map_err(database_error)?,
        updated_at_unix_millis: row.try_get("updated_at_ms").map_err(database_error)?,
        transitions,
        transitions_truncated: truncated,
    }))
}

async fn lookup_item(
    transaction: &mut Transaction<'_, Postgres>,
    item_uid: DurableId,
) -> Result<SupportLookupResult, SupportLookupError> {
    let row = sqlx::query(
        "SELECT namespace_id, account_id, character_id, item_uid, template_id, content_revision, \
         item_version, security_state, location_kind, slot_index, creation_request_id, \
         (extract(epoch FROM created_at) * 1000)::bigint AS created_at_ms, \
         (extract(epoch FROM updated_at) * 1000)::bigint AS updated_at_ms \
         FROM support_lookup_item_v1($1)",
    )
    .bind(item_uid.as_slice())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?;
    let Some(row) = row else {
        return Ok(SupportLookupResult::NotFound {
            target: LookupTarget::Item(item_uid),
        });
    };
    let transition_rows = sqlx::query(
        "SELECT event_id, mutation_id, event_kind, source_kind, pre_state_version, \
         post_state_version, pre_security_state, post_security_state, pre_location_kind, \
         post_location_kind, reason, \
         (extract(epoch FROM committed_at) * 1000)::bigint AS committed_at_ms \
         FROM support_lookup_item_transitions_v1($1)",
    )
    .bind(item_uid.as_slice())
    .fetch_all(&mut **transaction)
    .await
    .map_err(database_error)?;
    let (transition_rows, truncated) = bounded_rows(transition_rows);
    let transitions = transition_rows
        .iter()
        .map(item_transition)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SupportLookupResult::Item(ItemLookup {
        namespace_id: exact_namespace(&row)?,
        account_id: durable_id(&row, "account_id")?,
        character_id: durable_id(&row, "character_id")?,
        item_uid: durable_id(&row, "item_uid")?,
        template_id: row.try_get("template_id").map_err(database_error)?,
        content_revision: row.try_get("content_revision").map_err(database_error)?,
        item_version: row.try_get("item_version").map_err(database_error)?,
        security_state: row.try_get("security_state").map_err(database_error)?,
        location_kind: row.try_get("location_kind").map_err(database_error)?,
        slot_index: row.try_get("slot_index").map_err(database_error)?,
        creation_request_id: durable_id(&row, "creation_request_id")?,
        created_at_unix_millis: row.try_get("created_at_ms").map_err(database_error)?,
        updated_at_unix_millis: row.try_get("updated_at_ms").map_err(database_error)?,
        transitions,
        transitions_truncated: truncated,
    }))
}

async fn lookup_death(
    transaction: &mut Transaction<'_, Postgres>,
    death_id: DurableId,
) -> Result<SupportLookupResult, SupportLookupError> {
    let row = sqlx::query(
        "SELECT namespace_id, death_id, account_id, character_id, mutation_id, content_revision, \
         instance_id, lineage_id, restore_point_id, region_id, room_id, death_tick, cause_kind, \
         killer_content_id, killer_pattern_id, killer_attack_id, final_damage, damage_type, \
         pre_hit_health, network_state, recall_state, pre_character_version, \
         post_character_version, trace_digest, \
         (extract(epoch FROM committed_at) * 1000)::bigint AS committed_at_ms \
         FROM support_lookup_death_v1($1)",
    )
    .bind(death_id.as_slice())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(database_error)?;
    let Some(row) = row else {
        return Ok(SupportLookupResult::NotFound {
            target: LookupTarget::Death(death_id),
        });
    };
    let transition_rows = sqlx::query(
        "SELECT event_id, mutation_id, event_kind, event_digest, \
         (extract(epoch FROM committed_at) * 1000)::bigint AS committed_at_ms \
         FROM support_lookup_death_transitions_v1($1)",
    )
    .bind(death_id.as_slice())
    .fetch_all(&mut **transaction)
    .await
    .map_err(database_error)?;
    let (transition_rows, truncated) = bounded_rows(transition_rows);
    let transitions = transition_rows
        .iter()
        .map(death_transition)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SupportLookupResult::Death(DeathLookup {
        namespace_id: exact_namespace(&row)?,
        death_id: durable_id(&row, "death_id")?,
        account_id: durable_id(&row, "account_id")?,
        character_id: durable_id(&row, "character_id")?,
        mutation_id: durable_id(&row, "mutation_id")?,
        content_revision: row.try_get("content_revision").map_err(database_error)?,
        instance_id: durable_id(&row, "instance_id")?,
        lineage_id: durable_id(&row, "lineage_id")?,
        restore_point_id: durable_id(&row, "restore_point_id")?,
        region_id: row.try_get("region_id").map_err(database_error)?,
        room_id: row.try_get("room_id").map_err(database_error)?,
        death_tick: row.try_get("death_tick").map_err(database_error)?,
        cause_kind: row.try_get("cause_kind").map_err(database_error)?,
        killer_content_id: row.try_get("killer_content_id").map_err(database_error)?,
        killer_pattern_id: row.try_get("killer_pattern_id").map_err(database_error)?,
        killer_attack_id: row.try_get("killer_attack_id").map_err(database_error)?,
        final_damage: row.try_get("final_damage").map_err(database_error)?,
        damage_type: row.try_get("damage_type").map_err(database_error)?,
        pre_hit_health: row.try_get("pre_hit_health").map_err(database_error)?,
        network_state: row.try_get("network_state").map_err(database_error)?,
        recall_state: row.try_get("recall_state").map_err(database_error)?,
        pre_character_version: row
            .try_get("pre_character_version")
            .map_err(database_error)?,
        post_character_version: row
            .try_get("post_character_version")
            .map_err(database_error)?,
        trace_digest: digest(&row, "trace_digest")?,
        committed_at_unix_millis: row.try_get("committed_at_ms").map_err(database_error)?,
        transitions,
        transitions_truncated: truncated,
    }))
}

async fn append_audit(
    pool: &PgPool,
    principal: &OperatorPrincipal,
    request: &SupportLookupRequest,
    result: &SupportLookupResult,
) -> Result<(), SupportLookupError> {
    let audit_event_id = *uuid::Uuid::now_v7().as_bytes();
    let outcome_kind = match result {
        SupportLookupResult::NotFound { .. } => 1_i16,
        _ => 0_i16,
    };
    sqlx::query(
        "INSERT INTO support_lookup_audit_events_v1 \
         (namespace_id, audit_event_id, request_id, operator_id, target_kind, target_id, \
          reason_kind, case_reference, outcome_kind, result_count) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(audit_event_id.as_slice())
    .bind(request.request_id.as_slice())
    .bind(principal.operator_id())
    .bind(request.target.kind_code())
    .bind(request.target.durable_id().as_slice())
    .bind(request.reason.code())
    .bind(&request.case_reference)
    .bind(outcome_kind)
    .bind(result.disclosed_row_count())
    .execute(pool)
    .await
    .map_err(database_error)?;
    Ok(())
}

fn character_transition(
    row: &sqlx::postgres::PgRow,
) -> Result<CharacterTransition, SupportLookupError> {
    Ok(CharacterTransition {
        event_id: durable_id(row, "event_id")?,
        event_kind: row.try_get("event_kind").map_err(database_error)?,
        pre_state_version: row.try_get("pre_state_version").map_err(database_error)?,
        post_state_version: row.try_get("post_state_version").map_err(database_error)?,
        result_code: row.try_get("result_code").map_err(database_error)?,
        related_id: optional_durable_id(row, "related_id")?,
        committed_at_unix_millis: row.try_get("committed_at_ms").map_err(database_error)?,
    })
}

fn item_transition(row: &sqlx::postgres::PgRow) -> Result<ItemTransition, SupportLookupError> {
    Ok(ItemTransition {
        event_id: durable_id(row, "event_id")?,
        mutation_id: durable_id(row, "mutation_id")?,
        event_kind: row.try_get("event_kind").map_err(database_error)?,
        source_kind: row.try_get("source_kind").map_err(database_error)?,
        pre_state_version: row.try_get("pre_state_version").map_err(database_error)?,
        post_state_version: row.try_get("post_state_version").map_err(database_error)?,
        pre_security_state: row.try_get("pre_security_state").map_err(database_error)?,
        post_security_state: row.try_get("post_security_state").map_err(database_error)?,
        pre_location_kind: row.try_get("pre_location_kind").map_err(database_error)?,
        post_location_kind: row.try_get("post_location_kind").map_err(database_error)?,
        reason: row.try_get("reason").map_err(database_error)?,
        committed_at_unix_millis: row.try_get("committed_at_ms").map_err(database_error)?,
    })
}

fn death_transition(row: &sqlx::postgres::PgRow) -> Result<DeathTransition, SupportLookupError> {
    Ok(DeathTransition {
        event_id: durable_id(row, "event_id")?,
        mutation_id: durable_id(row, "mutation_id")?,
        event_kind: row.try_get("event_kind").map_err(database_error)?,
        event_digest: digest(row, "event_digest")?,
        committed_at_unix_millis: row.try_get("committed_at_ms").map_err(database_error)?,
    })
}

fn bounded_rows(mut rows: Vec<sqlx::postgres::PgRow>) -> (Vec<sqlx::postgres::PgRow>, bool) {
    let (retained, truncated) = bounded_history_length(rows.len());
    rows.truncate(retained);
    (rows, truncated)
}

fn bounded_history_length(length: usize) -> (usize, bool) {
    let maximum = usize::try_from(MAX_TRANSITIONS).expect("positive transition bound fits usize");
    (length.min(maximum), length > maximum)
}

fn exact_namespace(row: &sqlx::postgres::PgRow) -> Result<String, SupportLookupError> {
    let namespace: String = row.try_get("namespace_id").map_err(database_error)?;
    if namespace != WIPEABLE_CORE_NAMESPACE {
        return Err(SupportLookupError::InvalidStoredRecord);
    }
    Ok(namespace)
}

fn durable_id(row: &sqlx::postgres::PgRow, column: &str) -> Result<DurableId, SupportLookupError> {
    let bytes: Vec<u8> = row.try_get(column).map_err(database_error)?;
    bytes
        .try_into()
        .map_err(|_| SupportLookupError::InvalidStoredRecord)
        .and_then(|id: DurableId| {
            if is_zero(&id) {
                Err(SupportLookupError::InvalidStoredRecord)
            } else {
                Ok(id)
            }
        })
}

fn optional_durable_id(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<Option<DurableId>, SupportLookupError> {
    let bytes: Option<Vec<u8>> = row.try_get(column).map_err(database_error)?;
    bytes
        .map(|value| {
            value
                .try_into()
                .map_err(|_| SupportLookupError::InvalidStoredRecord)
                .and_then(|id: DurableId| {
                    if is_zero(&id) {
                        Err(SupportLookupError::InvalidStoredRecord)
                    } else {
                        Ok(id)
                    }
                })
        })
        .transpose()
}

fn digest(row: &sqlx::postgres::PgRow, column: &str) -> Result<Digest, SupportLookupError> {
    let bytes: Vec<u8> = row.try_get(column).map_err(database_error)?;
    bytes
        .try_into()
        .map_err(|_| SupportLookupError::InvalidStoredRecord)
        .and_then(|value: Digest| {
            if value.iter().all(|byte| *byte == 0) {
                Err(SupportLookupError::InvalidStoredRecord)
            } else {
                Ok(value)
            }
        })
}

fn validate_operator_id(operator_id: &str) -> Result<(), SupportLookupError> {
    if (3..=64).contains(&operator_id.len())
        && operator_id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        Ok(())
    } else {
        Err(SupportLookupError::InvalidOperatorDirectory)
    }
}

fn valid_case_reference(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn constant_time_equal(left: &Digest, right: &Digest) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

const fn is_zero(value: &DurableId) -> bool {
    let mut index = 0;
    while index < value.len() {
        if value[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

fn database_error(_error: sqlx::Error) -> SupportLookupError {
    SupportLookupError::ServiceUnavailable
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SupportLookupError {
    #[error("operator credential is outside the accepted bound")]
    InvalidCredential,
    #[error("operator directory is malformed")]
    InvalidOperatorDirectory,
    #[error("support lookup authorization failed")]
    Unauthorized,
    #[error("support lookup request is malformed or unbounded")]
    InvalidRequest,
    #[error("support lookup stored record is malformed")]
    InvalidStoredRecord,
    #[error("support lookup database role violates least privilege")]
    LeastPrivilegeViolation,
    #[error("support lookup service is unavailable")]
    ServiceUnavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(byte: u8) -> OperatorToken {
        OperatorToken::new(vec![byte; MIN_OPERATOR_TOKEN_BYTES]).unwrap()
    }

    fn request(target: LookupTarget) -> SupportLookupRequest {
        SupportLookupRequest {
            request_id: [9; 16],
            target,
            reason: LookupReason::IncidentInvestigation,
            case_reference: "GB-INCIDENT-1042".to_owned(),
        }
    }

    #[test]
    fn operator_authentication_is_exact_and_tokens_are_redacted() {
        let accepted = token(7);
        let directory = OperatorDirectory::new(vec![
            OperatorRecord::active_read_only("support.reader-1", &accepted).unwrap(),
        ])
        .unwrap();

        let principal = directory
            .authenticate("support.reader-1", &accepted)
            .unwrap();
        assert_eq!(principal.operator_id(), "support.reader-1");
        assert_eq!(principal.role(), SupportRole::ReadOnlyLookup);
        assert_eq!(format!("{accepted:?}"), "OperatorToken([REDACTED])");
        let record = OperatorRecord::active_read_only("support.reader-2", &accepted).unwrap();
        assert!(format!("{record:?}").contains("[REDACTED]"));
        assert_eq!(
            directory.authenticate("support.reader-1", &token(8)),
            Err(SupportLookupError::Unauthorized)
        );
        assert_eq!(
            directory.authenticate("support.reader-2", &accepted),
            Err(SupportLookupError::Unauthorized)
        );
    }

    #[test]
    fn disabled_operator_and_duplicate_directory_fail_closed() {
        let accepted = token(7);
        let disabled = OperatorDirectory::new(vec![
            OperatorRecord::active_read_only("support.reader-1", &accepted)
                .unwrap()
                .disabled(),
        ])
        .unwrap();
        assert_eq!(
            disabled.authenticate("support.reader-1", &accepted),
            Err(SupportLookupError::Unauthorized)
        );

        let duplicate = OperatorDirectory::new(vec![
            OperatorRecord::active_read_only("support.reader-1", &accepted).unwrap(),
            OperatorRecord::active_read_only("support.reader-1", &accepted).unwrap(),
        ]);
        assert_eq!(
            duplicate.unwrap_err(),
            SupportLookupError::InvalidOperatorDirectory
        );
    }

    #[test]
    fn request_contract_accepts_only_exact_durable_ids_and_bounded_case_references() {
        for target in [
            LookupTarget::Character([1; 16]),
            LookupTarget::Item([2; 16]),
            LookupTarget::Death([3; 16]),
        ] {
            request(target).validate().unwrap();
        }
        let mut invalid = request(LookupTarget::Character([0; 16]));
        assert_eq!(invalid.validate(), Err(SupportLookupError::InvalidRequest));
        invalid.target = LookupTarget::Character([1; 16]);
        invalid.request_id = [0; 16];
        assert_eq!(invalid.validate(), Err(SupportLookupError::InvalidRequest));
        invalid.request_id = [1; 16];
        invalid.case_reference = "*".to_owned();
        assert_eq!(invalid.validate(), Err(SupportLookupError::InvalidRequest));
        invalid.case_reference = "free form customer secret".to_owned();
        assert_eq!(invalid.validate(), Err(SupportLookupError::InvalidRequest));
    }

    #[test]
    fn result_contract_contains_no_raw_secret_or_localized_fields() {
        let results = [
            SupportLookupResult::Character(CharacterLookup {
                namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
                account_id: [1; 16],
                character_id: [2; 16],
                roster_ordinal: 1,
                class_id: "class.grave_arbalist".to_owned(),
                level: 10,
                oath_id: None,
                life_state: 0,
                security_state: 0,
                account_version: 4,
                created_at_unix_millis: 1,
                updated_at_unix_millis: 2,
                transitions: Vec::new(),
                transitions_truncated: false,
            }),
            SupportLookupResult::Item(ItemLookup {
                namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
                account_id: [1; 16],
                character_id: [2; 16],
                item_uid: [3; 16],
                template_id: "item.weapon.arbalist.pine_crossbow".to_owned(),
                content_revision: "core-dev.blake3.fixture".to_owned(),
                item_version: 1,
                security_state: 0,
                location_kind: 0,
                slot_index: Some(0),
                creation_request_id: [4; 16],
                created_at_unix_millis: 1,
                updated_at_unix_millis: 2,
                transitions: Vec::new(),
                transitions_truncated: false,
            }),
            SupportLookupResult::Death(DeathLookup {
                namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
                death_id: [5; 16],
                account_id: [1; 16],
                character_id: [2; 16],
                mutation_id: [6; 16],
                content_revision: "core-dev.blake3.fixture".to_owned(),
                instance_id: [7; 16],
                lineage_id: [8; 16],
                restore_point_id: [9; 16],
                region_id: "world.core_microrealm_01".to_owned(),
                room_id: "B1".to_owned(),
                death_tick: 30,
                cause_kind: 0,
                killer_content_id: Some("enemy.drowned_pilgrim".to_owned()),
                killer_pattern_id: None,
                killer_attack_id: None,
                final_damage: 10,
                damage_type: 0,
                pre_hit_health: 10,
                network_state: 0,
                recall_state: 0,
                pre_character_version: 2,
                post_character_version: 3,
                trace_digest: [10; 32],
                committed_at_unix_millis: 3,
                transitions: Vec::new(),
                transitions_truncated: false,
            }),
        ];
        for result in results {
            let serialized = serde_json::to_string(&result).unwrap();
            for prohibited in [
                "token",
                "credential",
                "password",
                "email",
                "platform_id",
                "ip_address",
                "localized",
                "display_name",
                "result_payload",
            ] {
                assert!(!serialized.contains(prohibited));
            }
        }
    }

    #[test]
    fn history_output_is_strictly_bounded() {
        assert_eq!(MAX_TRANSITIONS, 64);
        assert_eq!(bounded_history_length(0), (0, false));
        assert_eq!(bounded_history_length(64), (64, false));
        assert_eq!(bounded_history_length(65), (64, true));
        assert_eq!(bounded_history_length(usize::MAX), (64, true));
    }

    #[test]
    fn migration_exposes_only_bounded_views_and_append_only_audit() {
        let migration = include_str!("../../../migrations/0066_read_only_support_lookup_v1.sql");
        for required in [
            "Gravebound_Production_GDD_v1_Canonical.md",
            "Gravebound_Content_Production_Spec_v1.md",
            "Gravebound_Development_Roadmap_v1.md",
            "support_lookup_audit_events_v1",
            "security_barrier = true",
            "support_character_lookup_v1",
            "support_character_transition_lookup_v1",
            "support_item_lookup_v1",
            "support_item_transition_lookup_v1",
            "support_death_lookup_v1",
            "support_death_transition_lookup_v1",
            "SECURITY DEFINER",
            "REVOKE ALL ON FUNCTION support_lookup_character_v1(BYTEA) FROM PUBLIC",
            "LIMIT 65",
            "BEFORE UPDATE OR DELETE ON support_lookup_audit_events_v1",
            "append-only",
        ] {
            assert!(migration.contains(required), "migration omitted {required}");
        }
        for prohibited in [
            "auth_ticket",
            "platform_id",
            "ip_address",
            "result_payload",
            "character_name_snapshot",
            "UPDATE characters",
            "UPDATE item_instances",
            "UPDATE death_events",
            "DELETE FROM characters",
            "DELETE FROM item_instances",
            "DELETE FROM death_events",
        ] {
            assert!(
                !migration.contains(prohibited),
                "support migration leaked {prohibited}"
            );
        }
    }
}
