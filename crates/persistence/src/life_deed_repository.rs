//! Serializable reward-qualified deed persistence for `GB-M03-06B`.
//!
//! Authority is intentionally split across the three canonical design documents:
//! - `Gravebound_Production_GDD_v1_Canonical.md` `ECH-001`, `TECH-021`, and `TECH-023`;
//! - `Gravebound_Content_Production_Spec_v1.md` Core miniboss/boss reward and XP bindings;
//! - `Gravebound_Development_Roadmap_v1.md` `GB-M03-06`/`13` atomic replay gates.
//!
//! Callers provide no deed ID, source ID, reward table, XP profile, kind, or result hash. Those
//! values are derived from locked terminal reward evidence. One account lock serializes reward,
//! crash restoration, and terminal death for the character.

use sqlx::{PgConnection, Row};

use crate::{
    PersistenceError, PostgresPersistence, WIPEABLE_CORE_NAMESPACE,
    is_retryable_transaction_failure,
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_TRANSACTION_ATTEMPTS: u8 = 3;
const REQUEST_HASH_CONTEXT: &str = "gravebound.life-deed-completion.request.v2";
const RESULT_DIGEST_CONTEXT: &str = "gravebound.life-deed-completion.result.v2";
const PROJECTION_DIGEST_CONTEXT: &str = "gravebound.life-deed-projection.v2";
const CONFLICT_AUDIT_CONTEXT: &str = "gravebound.life-deed-conflict-audit.v2";

pub const CORE_PROGRESSION_RECORDS_BLAKE3: &str =
    "051f86a69b9d2a9dd911f0d92bf53b40e460ef13c9058d6f0b1f32f11b226f95";
pub const CORE_WORLD_RECORDS_BLAKE3: &str =
    "97b7188e26329b9430b7289d1e17d347c9b9472863b7db6bd48501fd3b773158";
pub const CORE_WORLD_ASSETS_BLAKE3: &str =
    "32ce9fce6f1d49d5cd6cb570fa0590a5ee5644388c2620b67846320d4b2a3759";
pub const CORE_WORLD_LOCALIZATION_BLAKE3: &str =
    "895c38724abfdef4909751743d91b5cff90d7f073c553bc044601abff4763a26";

const DEED_SEPULCHER: &str = "deed.core.sepulcher_knight_defeated";
const DEED_CALDUS: &str = "deed.core.sir_caldus_defeated";
const SOURCE_SEPULCHER: &str = "miniboss.sepulcher_knight";
const SOURCE_CALDUS: &str = "boss.sir_caldus";
const REWARD_SEPULCHER: &str = "reward.miniboss_t1";
const REWARD_CALDUS: &str = "reward.boss_caldus";
const XP_SEPULCHER: &str = "xp.miniboss_t1";
const XP_CALDUS: &str = "xp.boss_caldus";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeDeedContentAuthorityV2 {
    pub item_content_revision: String,
    pub progression_records_blake3: String,
    pub world_records_blake3: String,
    pub world_assets_blake3: String,
    pub world_localization_blake3: String,
}

impl LifeDeedContentAuthorityV2 {
    #[must_use]
    pub fn core() -> Self {
        Self {
            item_content_revision: crate::CORE_ITEM_CONTENT_REVISION.to_owned(),
            progression_records_blake3: CORE_PROGRESSION_RECORDS_BLAKE3.to_owned(),
            world_records_blake3: CORE_WORLD_RECORDS_BLAKE3.to_owned(),
            world_assets_blake3: CORE_WORLD_ASSETS_BLAKE3.to_owned(),
            world_localization_blake3: CORE_WORLD_LOCALIZATION_BLAKE3.to_owned(),
        }
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        if self != &Self::core() {
            return Err(PersistenceError::LifeDeedContentMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeDeedCompletionCommandV2 {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub completion_id: [u8; ID_BYTES],
    pub expected_character_version: u64,
    pub expected_life_metrics_version: u64,
    pub lineage_id: [u8; ID_BYTES],
    pub restore_point_id: [u8; ID_BYTES],
    /// Server-simulation tick captured with the terminal reward notification.
    pub achieved_tick: u64,
    pub content: LifeDeedContentAuthorityV2,
    pub issued_at_unix_ms: u64,
}

impl LifeDeedCompletionCommandV2 {
    fn validate(&self) -> Result<(), PersistenceError> {
        if all_zero(&self.account_id)
            || all_zero(&self.character_id)
            || all_zero(&self.completion_id)
            || all_zero(&self.lineage_id)
            || all_zero(&self.restore_point_id)
            || self.expected_character_version == 0
            || self.expected_life_metrics_version == 0
            || self.achieved_tick == 0
            || self.issued_at_unix_ms == 0
            || i64::try_from(self.achieved_tick).is_err()
            || i64::try_from(self.issued_at_unix_ms).is_err()
        {
            return Err(PersistenceError::CorruptStoredLifeDeed);
        }
        self.content.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeDeedCompletionRequestV2 {
    pub command: LifeDeedCompletionCommandV2,
    pub request_hash: [u8; HASH_BYTES],
}

impl LifeDeedCompletionRequestV2 {
    pub fn seal(command: LifeDeedCompletionCommandV2) -> Result<Self, PersistenceError> {
        command.validate()?;
        let request_hash = canonical_request_hash(&command)?;
        Ok(Self {
            command,
            request_hash,
        })
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        self.command.validate()?;
        if all_zero(&self.request_hash)
            || self.request_hash != canonical_request_hash(&self.command)?
        {
            return Err(PersistenceError::CorruptStoredLifeDeed);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifeDeedKindV2 {
    DungeonBoss,
    FinalDeedOnly,
}

impl LifeDeedKindV2 {
    const fn code(self) -> i16 {
        match self {
            Self::DungeonBoss => 0,
            Self::FinalDeedOnly => 2,
        }
    }

    fn from_code(code: i16) -> Result<Self, PersistenceError> {
        match code {
            0 => Ok(Self::DungeonBoss),
            2 => Ok(Self::FinalDeedOnly),
            _ => Err(PersistenceError::CorruptStoredLifeDeed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifeDeedProjectionOutcomeV2 {
    Inserted,
    Advanced,
    RetainedNewer,
}

impl LifeDeedProjectionOutcomeV2 {
    const fn code(self) -> i16 {
        match self {
            Self::Inserted => 0,
            Self::Advanced => 1,
            Self::RetainedNewer => 2,
        }
    }

    fn from_code(code: i16) -> Result<Self, PersistenceError> {
        match code {
            0 => Ok(Self::Inserted),
            1 => Ok(Self::Advanced),
            2 => Ok(Self::RetainedNewer),
            _ => Err(PersistenceError::CorruptStoredLifeDeed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLifeDeedRevocationV2 {
    pub restore_point_id: [u8; ID_BYTES],
    pub crash_mutation_id: [u8; ID_BYTES],
    pub change_ordinal: u32,
    pub revocation_digest: [u8; HASH_BYTES],
    pub post_projection_digest: [u8; HASH_BYTES],
    pub revoked_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLifeDeedCompletionV2 {
    pub command: LifeDeedCompletionCommandV2,
    pub source_instance_id: [u8; ID_BYTES],
    pub deed_id: String,
    pub source_content_id: String,
    pub kind: LifeDeedKindV2,
    pub reward_table_id: String,
    pub xp_profile_id: String,
    pub base_xp: u32,
    pub reward_result_hash: [u8; HASH_BYTES],
    pub progression_payload_hash: [u8; HASH_BYTES],
    pub pre_life_metrics_version: u64,
    pub post_life_metrics_version: u64,
    pub projection_outcome: LifeDeedProjectionOutcomeV2,
    pub request_hash: [u8; HASH_BYTES],
    pub result_digest: [u8; HASH_BYTES],
    pub committed_at_unix_ms: u64,
    pub revocation: Option<StoredLifeDeedRevocationV2>,
}

impl StoredLifeDeedCompletionV2 {
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.revocation.is_none()
    }

    fn validate(&self) -> Result<(), PersistenceError> {
        self.command.validate()?;
        if self.request_hash != canonical_request_hash(&self.command)?
            || self.pre_life_metrics_version != self.command.expected_life_metrics_version
            || self.post_life_metrics_version != self.pre_life_metrics_version + 1
            || self.committed_at_unix_ms < self.command.issued_at_unix_ms
            || self.result_digest != result_digest(self)?
        {
            return Err(PersistenceError::CorruptStoredLifeDeed);
        }
        if let Some(revocation) = &self.revocation
            && (revocation.restore_point_id != self.command.restore_point_id
                || revocation.change_ordinal > 4_094
                || all_zero(&revocation.revocation_digest)
                || all_zero(&revocation.post_projection_digest)
                || revocation.revoked_at_unix_ms < self.committed_at_unix_ms)
        {
            return Err(PersistenceError::CorruptStoredLifeDeed);
        }
        validate_derived_tuple(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifeDeedCompletionTransactionV2 {
    Committed(StoredLifeDeedCompletionV2),
    Replayed(StoredLifeDeedCompletionV2),
}

impl LifeDeedCompletionTransactionV2 {
    #[must_use]
    pub const fn receipt(&self) -> &StoredLifeDeedCompletionV2 {
        match self {
            Self::Committed(receipt) | Self::Replayed(receipt) => receipt,
        }
    }
}

#[derive(Debug)]
struct DerivedRewardAuthority {
    source_instance_id: [u8; ID_BYTES],
    deed_id: &'static str,
    source_content_id: String,
    kind: LifeDeedKindV2,
    reward_table_id: String,
    xp_profile_id: String,
    base_xp: u32,
    reward_result_hash: [u8; HASH_BYTES],
    progression_payload_hash: [u8; HASH_BYTES],
}

impl PostgresPersistence {
    pub async fn transact_life_deed_completion_v2(
        &self,
        request: &LifeDeedCompletionRequestV2,
    ) -> Result<LifeDeedCompletionTransactionV2, PersistenceError> {
        request.validate()?;
        for attempt in 1..=MAX_TRANSACTION_ATTEMPTS {
            match self.transact_life_deed_completion_v2_once(request).await {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS
                        && is_retryable_transaction_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded life-deed transaction loop always returns")
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the account-first authority, replay, and commit sequence is contiguous for audit"
    )]
    async fn transact_life_deed_completion_v2_once(
        &self,
        request: &LifeDeedCompletionRequestV2,
    ) -> Result<LifeDeedCompletionTransactionV2, PersistenceError> {
        let command = &request.command;
        let mut transaction = self.begin_transaction().await?;
        let selected_character = lock_account(transaction.connection(), command.account_id).await?;

        if let Some(stored) = load_receipt(
            transaction.connection(),
            command.account_id,
            command.completion_id,
        )
        .await?
        {
            if stored.request_hash != request.request_hash {
                insert_conflict_audit(transaction.connection(), request, &stored).await?;
                transaction.commit().await?;
                return Err(PersistenceError::LifeDeedIdempotencyConflict);
            }
            stored.validate()?;
            if &stored.command != command {
                transaction.rollback().await?;
                return Err(PersistenceError::CorruptStoredLifeDeed);
            }
            validate_life_deed_projection_graph(
                transaction.connection(),
                command.account_id,
                command.character_id,
            )
            .await?;
            transaction.rollback().await?;
            return Ok(LifeDeedCompletionTransactionV2::Replayed(stored));
        }

        if legacy_receipt_identity_exists(
            transaction.connection(),
            command.account_id,
            command.completion_id,
        )
        .await?
        {
            transaction.rollback().await?;
            return Err(PersistenceError::CorruptStoredLifeDeed);
        }

        if selected_character != Some(command.character_id) {
            transaction.rollback().await?;
            return Err(PersistenceError::LifeDeedBindingMismatch);
        }
        let character_version = lock_character(transaction.connection(), command).await?;
        if character_version != command.expected_character_version {
            transaction.rollback().await?;
            return Err(PersistenceError::LifeDeedCharacterVersionMismatch {
                expected: command.expected_character_version,
                actual: character_version,
            });
        }
        lock_active_danger_root(transaction.connection(), command).await?;
        let life_metrics_version = lock_life_metrics(transaction.connection(), command).await?;
        if life_metrics_version != command.expected_life_metrics_version {
            let projection_digest = life_deed_projection_digest(
                transaction.connection(),
                command.account_id,
                command.character_id,
            )
            .await?;
            transaction.rollback().await?;
            return Err(PersistenceError::LifeDeedMetricsVersionMismatch {
                expected: command.expected_life_metrics_version,
                actual: life_metrics_version,
                projection_digest,
            });
        }
        validate_life_deed_projection_graph(
            transaction.connection(),
            command.account_id,
            command.character_id,
        )
        .await?;
        let authority = load_reward_authority(transaction.connection(), command).await?;
        let committed_at_unix_ms = transaction_timestamp_ms(transaction.connection()).await?;
        if command.issued_at_unix_ms > committed_at_unix_ms {
            transaction.rollback().await?;
            return Err(PersistenceError::CorruptStoredLifeDeed);
        }

        let post_life_metrics_version = life_metrics_version
            .checked_add(1)
            .ok_or(PersistenceError::CorruptStoredLifeDeed)?;
        advance_life_metrics_version(
            transaction.connection(),
            command,
            life_metrics_version,
            post_life_metrics_version,
        )
        .await?;
        let projection_outcome = apply_projection(
            transaction.connection(),
            command,
            &authority,
            committed_at_unix_ms,
        )
        .await?;
        let mut stored = StoredLifeDeedCompletionV2 {
            command: command.clone(),
            source_instance_id: authority.source_instance_id,
            deed_id: authority.deed_id.to_owned(),
            source_content_id: authority.source_content_id,
            kind: authority.kind,
            reward_table_id: authority.reward_table_id,
            xp_profile_id: authority.xp_profile_id,
            base_xp: authority.base_xp,
            reward_result_hash: authority.reward_result_hash,
            progression_payload_hash: authority.progression_payload_hash,
            pre_life_metrics_version: life_metrics_version,
            post_life_metrics_version,
            projection_outcome,
            request_hash: request.request_hash,
            result_digest: [0; HASH_BYTES],
            committed_at_unix_ms,
            revocation: None,
        };
        stored.result_digest = result_digest(&stored)?;
        insert_receipt(transaction.connection(), &stored).await?;
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(transaction.connection())
            .await?;
        transaction.commit().await?;
        Ok(LifeDeedCompletionTransactionV2::Committed(stored))
    }
}

async fn lock_account(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    let row = sqlx::query(
        "SELECT selected_character_id FROM accounts \
         WHERE namespace_id=$1 AND account_id=$2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await?;
    let Some(row) = row else {
        return Err(PersistenceError::LifeDeedOwnerNotFound);
    };
    row.try_get::<Option<Vec<u8>>, _>("selected_character_id")?
        .map(exact_id)
        .transpose()
}

async fn lock_character(
    connection: &mut PgConnection,
    command: &LifeDeedCompletionCommandV2,
) -> Result<u64, PersistenceError> {
    let row = sqlx::query(
        "SELECT roster_ordinal,life_state,security_state,character_state_version \
         FROM characters WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .fetch_optional(connection)
    .await?;
    let Some(row) = row else {
        return Err(PersistenceError::LifeDeedOwnerNotFound);
    };
    if row.try_get::<Option<i16>, _>("roster_ordinal")?.is_none()
        || row.try_get::<i16, _>("life_state")? != 0
        || row.try_get::<i16, _>("security_state")? != 0
    {
        return Err(PersistenceError::LifeDeedBindingMismatch);
    }
    positive_u64(row.try_get("character_state_version")?)
}

async fn lock_active_danger_root(
    connection: &mut PgConnection,
    command: &LifeDeedCompletionCommandV2,
) -> Result<(), PersistenceError> {
    let root = sqlx::query(
        "SELECT lineage_id,restore_state,records_blake3,assets_blake3,localization_blake3 \
         FROM character_entry_restore_points \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND restore_point_id=$4 \
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(command.restore_point_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let world = sqlx::query(
        "SELECT location_kind,instance_lineage_id,entry_restore_point_id \
         FROM character_world_locations \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let lineage = sqlx::query(
        "SELECT lineage_state,records_blake3,assets_blake3,localization_blake3 \
         FROM character_instance_lineages \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND lineage_id=$4 \
         FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(command.lineage_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let (Some(root), Some(world), Some(lineage)) = (root, world, lineage) else {
        return Err(PersistenceError::LifeDeedBindingMismatch);
    };
    let content = &command.content;
    if exact_id(root.try_get("lineage_id")?)? != command.lineage_id
        || root.try_get::<i16, _>("restore_state")? != 0
        || root.try_get::<String, _>("records_blake3")? != content.world_records_blake3
        || root.try_get::<String, _>("assets_blake3")? != content.world_assets_blake3
        || root.try_get::<String, _>("localization_blake3")? != content.world_localization_blake3
        || world.try_get::<i16, _>("location_kind")? != 2
        || optional_id(world.try_get("instance_lineage_id")?)? != Some(command.lineage_id)
        || optional_id(world.try_get("entry_restore_point_id")?)? != Some(command.restore_point_id)
        || !matches!(lineage.try_get::<i16, _>("lineage_state")?, 0 | 1)
        || lineage.try_get::<String, _>("records_blake3")? != content.world_records_blake3
        || lineage.try_get::<String, _>("assets_blake3")? != content.world_assets_blake3
        || lineage.try_get::<String, _>("localization_blake3")? != content.world_localization_blake3
    {
        return Err(PersistenceError::LifeDeedBindingMismatch);
    }
    Ok(())
}

async fn lock_life_metrics(
    connection: &mut PgConnection,
    command: &LifeDeedCompletionCommandV2,
) -> Result<u64, PersistenceError> {
    let version = sqlx::query_scalar(
        "SELECT life_metrics_version FROM character_life_metrics \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .fetch_optional(connection)
    .await?
    .ok_or(PersistenceError::LifeDeedOwnerNotFound)?;
    positive_u64(version)
}

async fn load_reward_authority(
    connection: &mut PgConnection,
    command: &LifeDeedCompletionCommandV2,
) -> Result<DerivedRewardAuthority, PersistenceError> {
    let row = sqlx::query(
        "SELECT reward.source_instance_id,reward.reward_table_id,reward.content_revision, \
                reward.request_state,reward.result_hash, \
                xp.character_id AS xp_character_id,xp.payload_hash,xp.source_content_id, \
                xp.xp_profile_id,xp.progression_content_revision,xp.eligibility_kind,xp.eligible, \
                xp.encounter_life_state,xp.encounter_recall_state,xp.encounter_trust_state, \
                xp.base_xp,xp.result_code,xp.entry_restore_point_id, \
                xp.revoked_by_restore_point_id \
         FROM reward_requests AS reward \
         JOIN character_xp_award_results AS xp \
           ON xp.namespace_id=reward.namespace_id AND xp.account_id=reward.account_id \
          AND xp.reward_event_id=reward.reward_request_id \
         WHERE reward.namespace_id=$1 AND reward.account_id=$2 \
           AND reward.character_id=$3 AND reward.reward_request_id=$4 \
         FOR SHARE OF reward,xp",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(command.completion_id.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Err(PersistenceError::LifeDeedRewardNotTerminal);
    };
    let source_instance_id = exact_id(row.try_get("source_instance_id")?)?;
    let reward_table_id: String = row.try_get("reward_table_id")?;
    let source_content_id: String = row.try_get("source_content_id")?;
    let xp_profile_id: String = row.try_get("xp_profile_id")?;
    let base_xp_i32: i32 = row.try_get("base_xp")?;
    let base_xp =
        u32::try_from(base_xp_i32).map_err(|_| PersistenceError::CorruptStoredLifeDeed)?;
    let (deed_id, kind) = derive_core_deed(
        &source_content_id,
        &reward_table_id,
        &xp_profile_id,
        base_xp,
    )?;
    let reward_result_hash = exact_hash(
        row.try_get::<Option<Vec<u8>>, _>("result_hash")?
            .ok_or(PersistenceError::LifeDeedRewardNotTerminal)?,
    )?;
    let progression_payload_hash = exact_hash(row.try_get("payload_hash")?)?;
    if row.try_get::<i16, _>("request_state")? != 1
        || row.try_get::<String, _>("content_revision")? != command.content.item_content_revision
        || exact_id(row.try_get("xp_character_id")?)? != command.character_id
        || row.try_get::<String, _>("progression_content_revision")?
            != command.content.progression_records_blake3
        || row.try_get::<i16, _>("eligibility_kind")? != 1
        || !row.try_get::<bool, _>("eligible")?
        || row.try_get::<Option<i16>, _>("encounter_life_state")? != Some(0)
        || row.try_get::<Option<i16>, _>("encounter_recall_state")? != Some(0)
        || row.try_get::<Option<i16>, _>("encounter_trust_state")? != Some(0)
        || row.try_get::<i16, _>("result_code")? != 0
        || optional_id(row.try_get("entry_restore_point_id")?)? != Some(command.restore_point_id)
        || row
            .try_get::<Option<Vec<u8>>, _>("revoked_by_restore_point_id")?
            .is_some()
    {
        return Err(PersistenceError::LifeDeedRewardMismatch);
    }
    if kind == LifeDeedKindV2::DungeonBoss {
        verify_caldus_owner(
            connection,
            command,
            source_instance_id,
            reward_result_hash,
            progression_payload_hash,
        )
        .await?;
    }
    Ok(DerivedRewardAuthority {
        source_instance_id,
        deed_id,
        source_content_id,
        kind,
        reward_table_id,
        xp_profile_id,
        base_xp,
        reward_result_hash,
        progression_payload_hash,
    })
}

async fn verify_caldus_owner(
    connection: &mut PgConnection,
    command: &LifeDeedCompletionCommandV2,
    encounter_id: [u8; ID_BYTES],
    reward_result_hash: [u8; HASH_BYTES],
    progression_payload_hash: [u8; HASH_BYTES],
) -> Result<(), PersistenceError> {
    let exact: bool = sqlx::query_scalar(
        "SELECT EXISTS( \
            SELECT 1 FROM caldus_victory_exit_owners AS owner \
            JOIN caldus_victory_exits AS victory \
              ON victory.namespace_id=owner.namespace_id AND victory.encounter_id=owner.encounter_id \
            WHERE owner.namespace_id=$1 AND owner.encounter_id=$2 AND owner.account_id=$3 \
              AND owner.character_id=$4 AND owner.reward_request_id=$5 \
              AND owner.reward_result_hash=$6 AND owner.progression_payload_hash=$7 \
              AND victory.instance_lineage_id=$8)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(encounter_id.as_slice())
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(command.completion_id.as_slice())
    .bind(reward_result_hash.as_slice())
    .bind(progression_payload_hash.as_slice())
    .bind(command.lineage_id.as_slice())
    .fetch_one(connection)
    .await?;
    if !exact {
        return Err(PersistenceError::LifeDeedRewardMismatch);
    }
    Ok(())
}

fn derive_core_deed(
    source_content_id: &str,
    reward_table_id: &str,
    xp_profile_id: &str,
    base_xp: u32,
) -> Result<(&'static str, LifeDeedKindV2), PersistenceError> {
    match (source_content_id, reward_table_id, xp_profile_id, base_xp) {
        (SOURCE_CALDUS, REWARD_CALDUS, XP_CALDUS, 450) => {
            Ok((DEED_CALDUS, LifeDeedKindV2::DungeonBoss))
        }
        (SOURCE_SEPULCHER, REWARD_SEPULCHER, XP_SEPULCHER, 120) => {
            Ok((DEED_SEPULCHER, LifeDeedKindV2::FinalDeedOnly))
        }
        _ => Err(PersistenceError::LifeDeedRewardMismatch),
    }
}

async fn advance_life_metrics_version(
    connection: &mut PgConnection,
    command: &LifeDeedCompletionCommandV2,
    pre_version: u64,
    post_version: u64,
) -> Result<(), PersistenceError> {
    let rows = sqlx::query(
        "UPDATE character_life_metrics SET life_metrics_version=$1,updated_at=transaction_timestamp() \
         WHERE namespace_id=$2 AND account_id=$3 AND character_id=$4 AND life_metrics_version=$5",
    )
    .bind(i64_value(post_version)?)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(i64_value(pre_version)?)
    .execute(connection)
    .await?
    .rows_affected();
    if rows != 1 {
        return Err(PersistenceError::CorruptStoredLifeDeed);
    }
    Ok(())
}

async fn apply_projection(
    connection: &mut PgConnection,
    command: &LifeDeedCompletionCommandV2,
    authority: &DerivedRewardAuthority,
    committed_at_unix_ms: u64,
) -> Result<LifeDeedProjectionOutcomeV2, PersistenceError> {
    let existing = sqlx::query(
        "SELECT reward_event_id,achieved_tick FROM character_life_deeds \
         WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 AND deed_id=$4 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(authority.deed_id)
    .fetch_optional(&mut *connection)
    .await?;
    let outcome = if let Some(existing) = existing {
        let existing_id = exact_id(existing.try_get("reward_event_id")?)?;
        let existing_tick = positive_u64(existing.try_get("achieved_tick")?)?;
        if (command.achieved_tick, command.completion_id) > (existing_tick, existing_id) {
            let rows = sqlx::query(
                "UPDATE character_life_deeds SET reward_event_id=$1,source_content_id=$2, \
                 deed_kind=$3,achieved_tick=$4,content_revision=$5, \
                 committed_at=to_timestamp($6::double precision / 1000.0) \
                 WHERE namespace_id=$7 AND account_id=$8 AND character_id=$9 AND deed_id=$10",
            )
            .bind(command.completion_id.as_slice())
            .bind(&authority.source_content_id)
            .bind(authority.kind.code())
            .bind(i64_value(command.achieved_tick)?)
            .bind(&command.content.item_content_revision)
            .bind(i64_value(committed_at_unix_ms)?)
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(command.account_id.as_slice())
            .bind(command.character_id.as_slice())
            .bind(authority.deed_id)
            .execute(&mut *connection)
            .await?
            .rows_affected();
            if rows != 1 {
                return Err(PersistenceError::CorruptStoredLifeDeed);
            }
            LifeDeedProjectionOutcomeV2::Advanced
        } else {
            LifeDeedProjectionOutcomeV2::RetainedNewer
        }
    } else {
        let rows = sqlx::query(
            "INSERT INTO character_life_deeds \
             (namespace_id,account_id,character_id,deed_id,reward_event_id,source_content_id, \
              deed_kind,achieved_tick,content_revision,committed_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,to_timestamp($10::double precision / 1000.0))",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(command.account_id.as_slice())
        .bind(command.character_id.as_slice())
        .bind(authority.deed_id)
        .bind(command.completion_id.as_slice())
        .bind(&authority.source_content_id)
        .bind(authority.kind.code())
        .bind(i64_value(command.achieved_tick)?)
        .bind(&command.content.item_content_revision)
        .bind(i64_value(committed_at_unix_ms)?)
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if rows != 1 {
            return Err(PersistenceError::CorruptStoredLifeDeed);
        }
        LifeDeedProjectionOutcomeV2::Inserted
    };
    Ok(outcome)
}

async fn insert_receipt(
    connection: &mut PgConnection,
    stored: &StoredLifeDeedCompletionV2,
) -> Result<(), PersistenceError> {
    let command = &stored.command;
    sqlx::query(
        "INSERT INTO character_life_deed_completion_receipts_v2 \
         (namespace_id,account_id,character_id,completion_id,deed_id,source_content_id,deed_kind, \
          achieved_tick,content_revision,source_instance_id,lineage_id,restore_point_id, \
          reward_table_id,xp_profile_id,base_xp,progression_records_blake3,world_records_blake3, \
          world_assets_blake3,world_localization_blake3,reward_result_hash, \
          progression_payload_hash,expected_character_version,expected_life_metrics_version, \
          pre_life_metrics_version,post_life_metrics_version,projection_outcome,request_hash, \
          result_digest,issued_at,committed_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19, \
                 $20,$21,$22,$23,$24,$25,$26,$27,$28, \
                 to_timestamp($29::double precision / 1000.0), \
                 to_timestamp($30::double precision / 1000.0))",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(command.account_id.as_slice())
    .bind(command.character_id.as_slice())
    .bind(command.completion_id.as_slice())
    .bind(&stored.deed_id)
    .bind(&stored.source_content_id)
    .bind(stored.kind.code())
    .bind(i64_value(command.achieved_tick)?)
    .bind(&command.content.item_content_revision)
    .bind(stored.source_instance_id.as_slice())
    .bind(command.lineage_id.as_slice())
    .bind(command.restore_point_id.as_slice())
    .bind(&stored.reward_table_id)
    .bind(&stored.xp_profile_id)
    .bind(i32::try_from(stored.base_xp).map_err(|_| PersistenceError::CorruptStoredLifeDeed)?)
    .bind(&command.content.progression_records_blake3)
    .bind(&command.content.world_records_blake3)
    .bind(&command.content.world_assets_blake3)
    .bind(&command.content.world_localization_blake3)
    .bind(stored.reward_result_hash.as_slice())
    .bind(stored.progression_payload_hash.as_slice())
    .bind(i64_value(command.expected_character_version)?)
    .bind(i64_value(command.expected_life_metrics_version)?)
    .bind(i64_value(stored.pre_life_metrics_version)?)
    .bind(i64_value(stored.post_life_metrics_version)?)
    .bind(stored.projection_outcome.code())
    .bind(stored.request_hash.as_slice())
    .bind(stored.result_digest.as_slice())
    .bind(i64_value(command.issued_at_unix_ms)?)
    .bind(i64_value(stored.committed_at_unix_ms)?)
    .execute(connection)
    .await?;
    Ok(())
}

async fn load_receipt(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    completion_id: [u8; ID_BYTES],
) -> Result<Option<StoredLifeDeedCompletionV2>, PersistenceError> {
    let row = sqlx::query(
        "SELECT receipt.*, \
                CAST(EXTRACT(EPOCH FROM receipt.issued_at)*1000 AS BIGINT) AS issued_at_ms, \
                CAST(EXTRACT(EPOCH FROM receipt.committed_at)*1000 AS BIGINT) AS committed_at_ms, \
                revocation.restore_point_id AS revoked_restore_point_id, \
                revocation.crash_mutation_id,revocation.change_ordinal, \
                revocation.revocation_digest,revocation.post_projection_digest, \
                CAST(EXTRACT(EPOCH FROM revocation.revoked_at)*1000 AS BIGINT) AS revoked_at_ms \
         FROM character_life_deed_completion_receipts_v2 AS receipt \
         LEFT JOIN character_life_deed_revocations_v2 AS revocation \
           ON revocation.namespace_id=receipt.namespace_id \
          AND revocation.account_id=receipt.account_id \
          AND revocation.character_id=receipt.character_id \
          AND revocation.completion_id=receipt.completion_id \
         WHERE receipt.namespace_id=$1 AND receipt.account_id=$2 AND receipt.completion_id=$3 \
         FOR SHARE OF receipt",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(completion_id.as_slice())
    .fetch_optional(connection)
    .await?;
    row.map(|row| decode_receipt(&row)).transpose()
}

async fn legacy_receipt_identity_exists(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    completion_id: [u8; ID_BYTES],
) -> Result<bool, PersistenceError> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM character_life_deed_completion_receipts_v1 \
         WHERE namespace_id=$1 AND account_id=$2 AND completion_id=$3)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(completion_id.as_slice())
    .fetch_one(connection)
    .await
    .map_err(PersistenceError::from)
}

fn decode_receipt(
    row: &sqlx::postgres::PgRow,
) -> Result<StoredLifeDeedCompletionV2, PersistenceError> {
    let command = LifeDeedCompletionCommandV2 {
        account_id: exact_id(row.try_get("account_id")?)?,
        character_id: exact_id(row.try_get("character_id")?)?,
        completion_id: exact_id(row.try_get("completion_id")?)?,
        expected_character_version: positive_u64(row.try_get("expected_character_version")?)?,
        expected_life_metrics_version: positive_u64(row.try_get("expected_life_metrics_version")?)?,
        lineage_id: exact_id(row.try_get("lineage_id")?)?,
        restore_point_id: exact_id(row.try_get("restore_point_id")?)?,
        achieved_tick: positive_u64(row.try_get("achieved_tick")?)?,
        content: LifeDeedContentAuthorityV2 {
            item_content_revision: row.try_get("content_revision")?,
            progression_records_blake3: row.try_get("progression_records_blake3")?,
            world_records_blake3: row.try_get("world_records_blake3")?,
            world_assets_blake3: row.try_get("world_assets_blake3")?,
            world_localization_blake3: row.try_get("world_localization_blake3")?,
        },
        issued_at_unix_ms: positive_u64(row.try_get("issued_at_ms")?)?,
    };
    let revocation = decode_revocation(row)?;
    let stored = StoredLifeDeedCompletionV2 {
        command,
        source_instance_id: exact_id(row.try_get("source_instance_id")?)?,
        deed_id: row.try_get("deed_id")?,
        source_content_id: row.try_get("source_content_id")?,
        kind: LifeDeedKindV2::from_code(row.try_get("deed_kind")?)?,
        reward_table_id: row.try_get("reward_table_id")?,
        xp_profile_id: row.try_get("xp_profile_id")?,
        base_xp: u32::try_from(row.try_get::<i32, _>("base_xp")?)
            .map_err(|_| PersistenceError::CorruptStoredLifeDeed)?,
        reward_result_hash: exact_hash(row.try_get("reward_result_hash")?)?,
        progression_payload_hash: exact_hash(row.try_get("progression_payload_hash")?)?,
        pre_life_metrics_version: positive_u64(row.try_get("pre_life_metrics_version")?)?,
        post_life_metrics_version: positive_u64(row.try_get("post_life_metrics_version")?)?,
        projection_outcome: LifeDeedProjectionOutcomeV2::from_code(
            row.try_get("projection_outcome")?,
        )?,
        request_hash: exact_hash(row.try_get("request_hash")?)?,
        result_digest: exact_hash(row.try_get("result_digest")?)?,
        committed_at_unix_ms: positive_u64(row.try_get("committed_at_ms")?)?,
        revocation,
    };
    stored
        .validate()
        .map_err(|_| PersistenceError::CorruptStoredLifeDeed)?;
    Ok(stored)
}

fn decode_revocation(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<StoredLifeDeedRevocationV2>, PersistenceError> {
    let restore = row.try_get::<Option<Vec<u8>>, _>("revoked_restore_point_id")?;
    let mutation = row.try_get::<Option<Vec<u8>>, _>("crash_mutation_id")?;
    let ordinal = row.try_get::<Option<i32>, _>("change_ordinal")?;
    let digest = row.try_get::<Option<Vec<u8>>, _>("revocation_digest")?;
    let projection = row.try_get::<Option<Vec<u8>>, _>("post_projection_digest")?;
    let revoked_at = row.try_get::<Option<i64>, _>("revoked_at_ms")?;
    match (restore, mutation, ordinal, digest, projection, revoked_at) {
        (None, None, None, None, None, None) => Ok(None),
        (
            Some(restore),
            Some(mutation),
            Some(ordinal),
            Some(digest),
            Some(projection),
            Some(at),
        ) => Ok(Some(StoredLifeDeedRevocationV2 {
            restore_point_id: exact_id(restore)?,
            crash_mutation_id: exact_id(mutation)?,
            change_ordinal: u32::try_from(ordinal)
                .map_err(|_| PersistenceError::CorruptStoredLifeDeed)?,
            revocation_digest: exact_hash(digest)?,
            post_projection_digest: exact_hash(projection)?,
            revoked_at_unix_ms: positive_u64(at)?,
        })),
        _ => Err(PersistenceError::CorruptStoredLifeDeed),
    }
}

async fn insert_conflict_audit(
    connection: &mut PgConnection,
    request: &LifeDeedCompletionRequestV2,
    stored: &StoredLifeDeedCompletionV2,
) -> Result<(), PersistenceError> {
    let audit_id = conflict_audit_id(request)?;
    sqlx::query(
        "INSERT INTO character_life_deed_conflict_audits_v2 \
         (namespace_id,account_id,character_id,completion_id,attempted_character_id,audit_id, \
          stored_request_hash,attempted_request_hash,attempted_issued_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,to_timestamp($9::double precision / 1000.0)) \
         ON CONFLICT (namespace_id,account_id,completion_id,attempted_request_hash) DO NOTHING",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(stored.command.account_id.as_slice())
    .bind(stored.command.character_id.as_slice())
    .bind(stored.command.completion_id.as_slice())
    .bind(request.command.character_id.as_slice())
    .bind(audit_id.as_slice())
    .bind(stored.request_hash.as_slice())
    .bind(request.request_hash.as_slice())
    .bind(i64_value(request.command.issued_at_unix_ms)?)
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) async fn validate_life_deed_projection_graph(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<(), PersistenceError> {
    let divergent: bool = sqlx::query_scalar(
        "WITH ranked AS ( \
             SELECT receipt.*,row_number() OVER ( \
                 PARTITION BY receipt.deed_id \
                 ORDER BY receipt.achieved_tick DESC,receipt.completion_id DESC) AS deed_ordinal \
             FROM character_life_deed_completion_receipts_v2 AS receipt \
             LEFT JOIN character_life_deed_revocations_v2 AS revocation \
               ON revocation.namespace_id=receipt.namespace_id \
              AND revocation.account_id=receipt.account_id \
              AND revocation.character_id=receipt.character_id \
              AND revocation.completion_id=receipt.completion_id \
             WHERE receipt.namespace_id=$1 AND receipt.account_id=$2 AND receipt.character_id=$3 \
               AND revocation.completion_id IS NULL \
         ), expected AS (SELECT * FROM ranked WHERE deed_ordinal=1), \
         actual AS (SELECT * FROM character_life_deeds \
                    WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3) \
         SELECT EXISTS(SELECT 1 FROM expected FULL OUTER JOIN actual USING (deed_id) \
             WHERE expected.deed_id IS NULL OR actual.deed_id IS NULL \
                OR actual.reward_event_id IS DISTINCT FROM expected.completion_id \
                OR actual.source_content_id IS DISTINCT FROM expected.source_content_id \
                OR actual.deed_kind IS DISTINCT FROM expected.deed_kind \
                OR actual.achieved_tick IS DISTINCT FROM expected.achieved_tick \
                OR actual.content_revision IS DISTINCT FROM expected.content_revision)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_one(connection)
    .await?;
    if divergent {
        return Err(PersistenceError::CorruptStoredLifeDeed);
    }
    Ok(())
}

pub(crate) async fn life_deed_projection_digest(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let rows = sqlx::query(
        "SELECT deed_id,reward_event_id,source_content_id,deed_kind,achieved_tick,content_revision \
         FROM character_life_deeds WHERE namespace_id=$1 AND account_id=$2 AND character_id=$3 \
         ORDER BY deed_id COLLATE \"C\"",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_all(connection)
    .await?;
    projection_digest_from_rows(rows)
}

/// Reconstructs the latest-deed projection at a historical life-metrics boundary. Crash replay
/// uses aggregate versions rather than timestamps so later lives cannot alter the stored result.
pub(crate) async fn life_deed_projection_digest_at_version(
    connection: &mut PgConnection,
    account_id: [u8; ID_BYTES],
    character_id: [u8; ID_BYTES],
    post_life_metrics_version: u64,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let rows = sqlx::query(
        "WITH eligible AS ( \
             SELECT receipt.*,result.post_life_metrics_version AS revoked_at_life_version \
             FROM character_life_deed_completion_receipts_v2 AS receipt \
             LEFT JOIN character_life_deed_revocations_v2 AS revocation \
               ON revocation.namespace_id=receipt.namespace_id \
              AND revocation.account_id=receipt.account_id \
              AND revocation.character_id=receipt.character_id \
              AND revocation.completion_id=receipt.completion_id \
             LEFT JOIN danger_crash_restore_results AS result \
               ON result.namespace_id=revocation.namespace_id \
              AND result.account_id=revocation.account_id \
              AND result.mutation_id=revocation.crash_mutation_id \
             WHERE receipt.namespace_id=$1 AND receipt.account_id=$2 \
               AND receipt.character_id=$3 AND receipt.post_life_metrics_version < $4 \
               AND (revocation.completion_id IS NULL \
                    OR result.post_life_metrics_version > $4) \
         ), ranked AS ( \
             SELECT eligible.*,row_number() OVER (PARTITION BY deed_id \
                 ORDER BY achieved_tick DESC,completion_id DESC) AS deed_ordinal \
             FROM eligible) \
         SELECT deed_id,completion_id AS reward_event_id,source_content_id,deed_kind, \
                achieved_tick,content_revision FROM ranked WHERE deed_ordinal=1 \
         ORDER BY deed_id COLLATE \"C\"",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .bind(i64_value(post_life_metrics_version)?)
    .fetch_all(connection)
    .await?;
    projection_digest_from_rows(rows)
}

fn projection_digest_from_rows(
    rows: Vec<sqlx::postgres::PgRow>,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    update_field(&mut hasher, PROJECTION_DIGEST_CONTEXT.as_bytes())?;
    for row in rows {
        let deed_id: String = row.try_get("deed_id")?;
        let reward_event_id = exact_id(row.try_get("reward_event_id")?)?;
        let source_content_id: String = row.try_get("source_content_id")?;
        let kind: i16 = row.try_get("deed_kind")?;
        if !matches!(kind, 0..=2) {
            return Err(PersistenceError::CorruptStoredLifeDeed);
        }
        let achieved_tick = positive_u64(row.try_get("achieved_tick")?)?;
        let content_revision: String = row.try_get("content_revision")?;
        update_field(&mut hasher, deed_id.as_bytes())?;
        update_field(&mut hasher, &reward_event_id)?;
        update_field(&mut hasher, source_content_id.as_bytes())?;
        update_field(&mut hasher, &kind.to_le_bytes())?;
        update_field(&mut hasher, &achieved_tick.to_le_bytes())?;
        update_field(&mut hasher, content_revision.as_bytes())?;
    }
    Ok(*hasher.finalize().as_bytes())
}

async fn transaction_timestamp_ms(connection: &mut PgConnection) -> Result<u64, PersistenceError> {
    let value: i64 = sqlx::query_scalar(
        "SELECT CAST(EXTRACT(EPOCH FROM transaction_timestamp()) * 1000 AS BIGINT)",
    )
    .fetch_one(connection)
    .await?;
    positive_u64(value)
}

fn validate_derived_tuple(stored: &StoredLifeDeedCompletionV2) -> Result<(), PersistenceError> {
    let expected = derive_core_deed(
        &stored.source_content_id,
        &stored.reward_table_id,
        &stored.xp_profile_id,
        stored.base_xp,
    )
    .map_err(|_| PersistenceError::CorruptStoredLifeDeed)?;
    if stored.deed_id != expected.0 || stored.kind != expected.1 {
        return Err(PersistenceError::CorruptStoredLifeDeed);
    }
    Ok(())
}

fn canonical_request_hash(
    command: &LifeDeedCompletionCommandV2,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    update_field(&mut hasher, REQUEST_HASH_CONTEXT.as_bytes())?;
    update_field(&mut hasher, &command.account_id)?;
    update_field(&mut hasher, &command.character_id)?;
    update_field(&mut hasher, &command.completion_id)?;
    update_field(
        &mut hasher,
        &command.expected_character_version.to_le_bytes(),
    )?;
    update_field(
        &mut hasher,
        &command.expected_life_metrics_version.to_le_bytes(),
    )?;
    update_field(&mut hasher, &command.lineage_id)?;
    update_field(&mut hasher, &command.restore_point_id)?;
    update_field(&mut hasher, &command.achieved_tick.to_le_bytes())?;
    update_field(
        &mut hasher,
        command.content.item_content_revision.as_bytes(),
    )?;
    update_field(
        &mut hasher,
        command.content.progression_records_blake3.as_bytes(),
    )?;
    update_field(&mut hasher, command.content.world_records_blake3.as_bytes())?;
    update_field(&mut hasher, command.content.world_assets_blake3.as_bytes())?;
    update_field(
        &mut hasher,
        command.content.world_localization_blake3.as_bytes(),
    )?;
    update_field(&mut hasher, &command.issued_at_unix_ms.to_le_bytes())?;
    Ok(*hasher.finalize().as_bytes())
}

fn result_digest(
    stored: &StoredLifeDeedCompletionV2,
) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    update_field(&mut hasher, RESULT_DIGEST_CONTEXT.as_bytes())?;
    update_field(&mut hasher, &stored.request_hash)?;
    update_field(&mut hasher, &stored.source_instance_id)?;
    update_field(&mut hasher, stored.deed_id.as_bytes())?;
    update_field(&mut hasher, stored.source_content_id.as_bytes())?;
    update_field(&mut hasher, &stored.kind.code().to_le_bytes())?;
    update_field(&mut hasher, stored.reward_table_id.as_bytes())?;
    update_field(&mut hasher, stored.xp_profile_id.as_bytes())?;
    update_field(&mut hasher, &stored.base_xp.to_le_bytes())?;
    update_field(&mut hasher, &stored.reward_result_hash)?;
    update_field(&mut hasher, &stored.progression_payload_hash)?;
    update_field(&mut hasher, &stored.pre_life_metrics_version.to_le_bytes())?;
    update_field(&mut hasher, &stored.post_life_metrics_version.to_le_bytes())?;
    update_field(&mut hasher, &stored.projection_outcome.code().to_le_bytes())?;
    update_field(&mut hasher, &stored.committed_at_unix_ms.to_le_bytes())?;
    Ok(*hasher.finalize().as_bytes())
}

fn conflict_audit_id(
    request: &LifeDeedCompletionRequestV2,
) -> Result<[u8; ID_BYTES], PersistenceError> {
    let mut hasher = blake3::Hasher::new();
    update_field(&mut hasher, CONFLICT_AUDIT_CONTEXT.as_bytes())?;
    update_field(&mut hasher, &request.command.account_id)?;
    update_field(&mut hasher, &request.command.completion_id)?;
    update_field(&mut hasher, &request.request_hash)?;
    let digest = hasher.finalize();
    let mut id = [0; ID_BYTES];
    id.copy_from_slice(&digest.as_bytes()[..ID_BYTES]);
    if all_zero(&id) {
        return Err(PersistenceError::CorruptStoredLifeDeed);
    }
    Ok(id)
}

fn update_field(hasher: &mut blake3::Hasher, bytes: &[u8]) -> Result<(), PersistenceError> {
    let length = u64::try_from(bytes.len()).map_err(|_| PersistenceError::CorruptStoredLifeDeed)?;
    hasher.update(&length.to_le_bytes());
    hasher.update(bytes);
    Ok(())
}

fn exact_id(value: Vec<u8>) -> Result<[u8; ID_BYTES], PersistenceError> {
    let fixed: [u8; ID_BYTES] = value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredLifeDeed)?;
    if all_zero(&fixed) {
        return Err(PersistenceError::CorruptStoredLifeDeed);
    }
    Ok(fixed)
}

fn optional_id(value: Option<Vec<u8>>) -> Result<Option<[u8; ID_BYTES]>, PersistenceError> {
    value.map(exact_id).transpose()
}

fn exact_hash(value: Vec<u8>) -> Result<[u8; HASH_BYTES], PersistenceError> {
    let fixed: [u8; HASH_BYTES] = value
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredLifeDeed)?;
    if all_zero(&fixed) {
        return Err(PersistenceError::CorruptStoredLifeDeed);
    }
    Ok(fixed)
}

fn positive_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(PersistenceError::CorruptStoredLifeDeed)
}

fn i64_value(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::CorruptStoredLifeDeed)
}

fn all_zero<const N: usize>(value: &[u8; N]) -> bool {
    value.iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> LifeDeedCompletionCommandV2 {
        LifeDeedCompletionCommandV2 {
            account_id: [1; ID_BYTES],
            character_id: [2; ID_BYTES],
            completion_id: [3; ID_BYTES],
            expected_character_version: 4,
            expected_life_metrics_version: 5,
            lineage_id: [6; ID_BYTES],
            restore_point_id: [7; ID_BYTES],
            achieved_tick: 8,
            content: LifeDeedContentAuthorityV2::core(),
            issued_at_unix_ms: 9,
        }
    }

    #[test]
    fn request_hash_binds_every_authoritative_axis() {
        let baseline = LifeDeedCompletionRequestV2::seal(command()).unwrap();
        let mut variants = Vec::new();
        let mut changed = command();
        changed.account_id[0] += 1;
        variants.push(changed);
        let mut changed = command();
        changed.character_id[0] += 1;
        variants.push(changed);
        let mut changed = command();
        changed.completion_id[0] += 1;
        variants.push(changed);
        let mut changed = command();
        changed.expected_character_version += 1;
        variants.push(changed);
        let mut changed = command();
        changed.expected_life_metrics_version += 1;
        variants.push(changed);
        let mut changed = command();
        changed.lineage_id[0] += 1;
        variants.push(changed);
        let mut changed = command();
        changed.restore_point_id[0] += 1;
        variants.push(changed);
        let mut changed = command();
        changed.achieved_tick += 1;
        variants.push(changed);
        let mut changed = command();
        changed.issued_at_unix_ms += 1;
        variants.push(changed);
        for variant in variants {
            assert_ne!(
                LifeDeedCompletionRequestV2::seal(variant)
                    .unwrap()
                    .request_hash,
                baseline.request_hash
            );
        }

        let mut content_variants = Vec::new();
        let mut changed = command();
        changed.content.item_content_revision.push('0');
        content_variants.push(changed);
        let mut changed = command();
        changed
            .content
            .progression_records_blake3
            .replace_range(0..1, "1");
        content_variants.push(changed);
        let mut changed = command();
        changed
            .content
            .world_records_blake3
            .replace_range(0..1, "0");
        content_variants.push(changed);
        let mut changed = command();
        changed.content.world_assets_blake3.replace_range(0..1, "0");
        content_variants.push(changed);
        let mut changed = command();
        changed
            .content
            .world_localization_blake3
            .replace_range(0..1, "0");
        content_variants.push(changed);
        for variant in content_variants {
            assert_ne!(
                canonical_request_hash(&variant).unwrap(),
                baseline.request_hash
            );
        }
    }

    #[test]
    fn exact_content_and_identity_bounds_fail_closed() {
        for mutate in [
            |command: &mut LifeDeedCompletionCommandV2| {
                command.content.item_content_revision =
                    "core-dev.blake3.".to_owned() + &"0".repeat(64);
            },
            |command: &mut LifeDeedCompletionCommandV2| {
                command.content.progression_records_blake3 = "0".repeat(64);
            },
            |command: &mut LifeDeedCompletionCommandV2| {
                command.content.world_records_blake3 = "0".repeat(64);
            },
            |command: &mut LifeDeedCompletionCommandV2| {
                command.content.world_assets_blake3 = "0".repeat(64);
            },
            |command: &mut LifeDeedCompletionCommandV2| {
                command.content.world_localization_blake3 = "0".repeat(64);
            },
        ] {
            let mut invalid = command();
            mutate(&mut invalid);
            assert!(matches!(
                LifeDeedCompletionRequestV2::seal(invalid),
                Err(PersistenceError::LifeDeedContentMismatch)
            ));
        }
        for mutate in [
            |command: &mut LifeDeedCompletionCommandV2| command.account_id = [0; ID_BYTES],
            |command: &mut LifeDeedCompletionCommandV2| command.character_id = [0; ID_BYTES],
            |command: &mut LifeDeedCompletionCommandV2| command.completion_id = [0; ID_BYTES],
            |command: &mut LifeDeedCompletionCommandV2| command.lineage_id = [0; ID_BYTES],
            |command: &mut LifeDeedCompletionCommandV2| command.restore_point_id = [0; ID_BYTES],
            |command: &mut LifeDeedCompletionCommandV2| command.expected_character_version = 0,
            |command: &mut LifeDeedCompletionCommandV2| command.expected_life_metrics_version = 0,
            |command: &mut LifeDeedCompletionCommandV2| command.achieved_tick = 0,
            |command: &mut LifeDeedCompletionCommandV2| command.issued_at_unix_ms = 0,
        ] {
            let mut invalid = command();
            mutate(&mut invalid);
            assert!(matches!(
                LifeDeedCompletionRequestV2::seal(invalid),
                Err(PersistenceError::CorruptStoredLifeDeed)
            ));
        }
    }

    #[test]
    fn core_reward_tuple_distinguishes_boss_from_final_deed_only() {
        assert_eq!(
            derive_core_deed(SOURCE_CALDUS, REWARD_CALDUS, XP_CALDUS, 450).unwrap(),
            (DEED_CALDUS, LifeDeedKindV2::DungeonBoss)
        );
        assert_eq!(
            derive_core_deed(SOURCE_SEPULCHER, REWARD_SEPULCHER, XP_SEPULCHER, 120).unwrap(),
            (DEED_SEPULCHER, LifeDeedKindV2::FinalDeedOnly)
        );
        assert!(derive_core_deed(SOURCE_SEPULCHER, REWARD_SEPULCHER, XP_SEPULCHER, 450).is_err());
    }

    #[test]
    fn stored_result_digest_and_revocation_shape_fail_closed() {
        let command = command();
        let request = LifeDeedCompletionRequestV2::seal(command.clone()).unwrap();
        let mut stored = StoredLifeDeedCompletionV2 {
            command,
            source_instance_id: [10; ID_BYTES],
            deed_id: DEED_SEPULCHER.to_owned(),
            source_content_id: SOURCE_SEPULCHER.to_owned(),
            kind: LifeDeedKindV2::FinalDeedOnly,
            reward_table_id: REWARD_SEPULCHER.to_owned(),
            xp_profile_id: XP_SEPULCHER.to_owned(),
            base_xp: 120,
            reward_result_hash: [11; HASH_BYTES],
            progression_payload_hash: [12; HASH_BYTES],
            pre_life_metrics_version: 5,
            post_life_metrics_version: 6,
            projection_outcome: LifeDeedProjectionOutcomeV2::Inserted,
            request_hash: request.request_hash,
            result_digest: [0; HASH_BYTES],
            committed_at_unix_ms: 10,
            revocation: None,
        };
        stored.result_digest = result_digest(&stored).unwrap();
        stored.validate().unwrap();

        let mut corrupt = stored.clone();
        corrupt.base_xp += 1;
        assert!(matches!(
            corrupt.validate(),
            Err(PersistenceError::CorruptStoredLifeDeed)
        ));

        let mut corrupt = stored;
        corrupt.revocation = Some(StoredLifeDeedRevocationV2 {
            restore_point_id: [99; ID_BYTES],
            crash_mutation_id: [13; ID_BYTES],
            change_ordinal: 0,
            revocation_digest: [14; HASH_BYTES],
            post_projection_digest: [15; HASH_BYTES],
            revoked_at_unix_ms: 11,
        });
        assert!(matches!(
            corrupt.validate(),
            Err(PersistenceError::CorruptStoredLifeDeed)
        ));
    }
}
