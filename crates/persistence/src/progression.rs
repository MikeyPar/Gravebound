//! Typed `PostgreSQL` repository for Core character XP awards.
//!
//! The repository owns durability, lock order, replay, and stored-shape validation. Eligibility,
//! XP arithmetic, and level-stat rules remain server-owned gameplay decisions under GDD
//! `PROG-001` through `PROG-003`.

use sqlx::Row;

use crate::{
    PersistenceError, PostgresPersistence, StagedBargainMilestone, StoredAshWallet,
    StoredBargainMilestoneLife, WIPEABLE_CORE_NAMESPACE,
    ash_wallet::lock_ash_wallet_on_connection,
    bargain_milestone::{
        BargainMilestoneBinding, lock_bargain_milestone_life, persist_bargain_milestone,
    },
};

const ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const MAX_RESULT_PAYLOAD_BYTES: usize = 65_536;

/// Compiled progression values required to validate durable projections.
///
/// Persistence owns neither the thresholds nor a fallback. The server must supply the exact
/// validated values from its active content manifest for every transaction, including replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProgressionContract {
    pub cumulative_xp: [i32; 10],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProgression {
    pub total_xp: i32,
    pub level: i16,
    pub current_health: i32,
    pub progression_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProgressionSnapshot {
    pub character: StoredLockedProgressionCharacter,
    pub progression: StoredProgression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOrdinaryXpEvidence {
    pub delta_x_milli_tiles: i32,
    pub delta_y_milli_tiles: i32,
    pub window_ticks: i32,
    pub actual_health_damage: i64,
    pub effective_support: bool,
    pub living_at_enemy_death: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredEncounterLifeState {
    Living,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredEncounterRecallState {
    Present,
    Recalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredEncounterTrustState {
    Valid,
    InvalidSession,
    AntiCheatRejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEncounterXpEvidence {
    pub active_ticks: i64,
    pub present_ticks: i64,
    pub longest_inactivity_ticks: i64,
    pub reference_health: i64,
    pub direct_damage: i64,
    pub effective_healing: i64,
    pub damage_prevented: i64,
    pub objective_credits: i16,
    pub life_state: StoredEncounterLifeState,
    pub recall_state: StoredEncounterRecallState,
    pub trust_state: StoredEncounterTrustState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredXpEligibilityEvidence {
    Ordinary(StoredOrdinaryXpEvidence),
    Encounter(StoredEncounterXpEvidence),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredXpAwardResult {
    pub account_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
    pub reward_event_id: [u8; ID_BYTES],
    pub payload_hash: [u8; HASH_BYTES],
    pub source_content_id: String,
    pub xp_profile_id: Option<String>,
    /// Exact lowercase BLAKE3 manifest digest, without a mutable development label.
    pub progression_content_revision: String,
    /// Present only when the award was resolved while this restore point was active in danger.
    pub entry_restore_point_id: Option<[u8; ID_BYTES]>,
    /// Set by TECH-023 restoration; the immutable award evidence and original result remain.
    pub revoked_by_restore_point_id: Option<[u8; ID_BYTES]>,
    pub revocation_progression_version: Option<i64>,
    pub evidence: StoredXpEligibilityEvidence,
    pub eligible: bool,
    pub first_clear_awarded: bool,
    pub base_xp: i32,
    pub bonus_xp: i32,
    pub requested_xp: i32,
    pub applied_xp: i32,
    pub discarded_xp: i32,
    pub pre_total_xp: i32,
    pub post_total_xp: i32,
    pub pre_level: i16,
    pub post_level: i16,
    pub pre_progression_version: i64,
    pub post_progression_version: i64,
    pub result_code: i16,
    pub result_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBossFirstClear {
    pub boss_id: String,
    pub reward_event_id: [u8; ID_BYTES],
    pub character_id: [u8; ID_BYTES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredBossFirstClearState {
    NotApplicable,
    Vacant { boss_id: String },
    Awarded(StoredBossFirstClear),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLockedProgressionCharacter {
    pub cached_level: i16,
    pub life_state: i16,
    pub security_state: i16,
    pub character_state_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProgressionAwardLocation {
    pub location_kind: i16,
    pub location_content_id: Option<String>,
    pub layout_id: Option<String>,
    pub instance_lineage_id: Option<[u8; ID_BYTES]>,
    pub entry_restore_point_id: Option<[u8; ID_BYTES]>,
}

/// Mutable state exposed only for a fresh reward event under one serializable transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressionAwardTransactionState {
    pub selected_character_id: Option<[u8; ID_BYTES]>,
    pub character: StoredLockedProgressionCharacter,
    pub progression: StoredProgression,
    pub entry_restore_point_id: Option<[u8; ID_BYTES]>,
    pub location: StoredProgressionAwardLocation,
    pub boss_first_clear: StoredBossFirstClearState,
    pub bargain_life: StoredBargainMilestoneLife,
    pub ash_wallet: StoredAshWallet,
    pub new_result: Option<StoredXpAwardResult>,
    pub new_boss_first_clear: Option<StoredBossFirstClear>,
    pub new_bargain_milestone: Option<StagedBargainMilestone>,
}

/// Replay is a deliberate no-op: no caller closure runs and no timestamp-bearing row is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressionAwardTransaction<T> {
    Replayed(Box<StoredXpAwardResult>),
    Committed(T),
}

impl PostgresPersistence {
    /// Reads one owned character's validated progression without taking mutation locks.
    pub async fn progression_snapshot(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        contract: &StoredProgressionContract,
    ) -> Result<Option<StoredProgressionSnapshot>, PersistenceError> {
        validate_contract(contract)?;
        let mut transaction = self.begin_transaction().await?;
        let row = sqlx::query(
            "SELECT c.level AS cached_level, c.life_state, c.security_state, \
                    c.character_state_version, p.total_xp, p.level, p.current_health, \
                    p.progression_version \
             FROM characters c \
             JOIN character_progression p \
               ON p.namespace_id = c.namespace_id \
              AND p.account_id = c.account_id \
              AND p.character_id = c.character_id \
             WHERE c.namespace_id = $1 AND c.account_id = $2 AND c.character_id = $3",
        )
        .bind(WIPEABLE_CORE_NAMESPACE)
        .bind(account_id.as_slice())
        .bind(character_id.as_slice())
        .fetch_optional(transaction.connection())
        .await
        .map_err(PersistenceError::Database)?;
        transaction.rollback().await?;
        row.map(|row| {
            let cached_level: i32 = row
                .try_get("cached_level")
                .map_err(PersistenceError::Database)?;
            let character = StoredLockedProgressionCharacter {
                cached_level: cached_level
                    .try_into()
                    .map_err(|_| PersistenceError::CorruptStoredProgression)?,
                life_state: row
                    .try_get("life_state")
                    .map_err(PersistenceError::Database)?,
                security_state: row
                    .try_get("security_state")
                    .map_err(PersistenceError::Database)?,
                character_state_version: row
                    .try_get("character_state_version")
                    .map_err(PersistenceError::Database)?,
            };
            let progression = decode_progression(&row, contract)?;
            validate_locked_character(&character, contract)?;
            if character.cached_level != progression.level {
                return Err(PersistenceError::CorruptStoredProgression);
            }
            Ok(StoredProgressionSnapshot {
                character,
                progression,
            })
        })
        .transpose()
    }

    /// Applies one XP award or returns its exact prior result.
    ///
    /// Lock order for a fresh award is account -> character -> location -> progression. The account-wide
    /// reward key is checked immediately after the account lock so a replay never depends on the
    /// character's current state and never reaches the mutation closure.
    pub async fn transact_progression_award<T, F>(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        reward_event_id: [u8; ID_BYTES],
        boss_id: Option<&str>,
        contract: &StoredProgressionContract,
        mut operation: F,
    ) -> Result<ProgressionAwardTransaction<T>, PersistenceError>
    where
        T: Send,
        F: FnMut(&mut ProgressionAwardTransactionState) -> Result<T, PersistenceError> + Send,
    {
        const MAX_SERIALIZATION_ATTEMPTS: u8 = 3;

        validate_nonzero_id(&reward_event_id)?;
        validate_contract(contract)?;
        if let Some(boss_id) = boss_id {
            validate_bounded_id(boss_id)?;
        }

        for attempt in 1..=MAX_SERIALIZATION_ATTEMPTS {
            match self
                .transact_progression_award_once(
                    account_id,
                    character_id,
                    reward_event_id,
                    boss_id,
                    contract,
                    &mut operation,
                )
                .await
            {
                Err(error)
                    if attempt < MAX_SERIALIZATION_ATTEMPTS
                        && crate::is_serialization_failure(&error) => {}
                result => return result,
            }
        }
        unreachable!("bounded progression transaction loop always returns")
    }

    async fn transact_progression_award_once<T, F>(
        &self,
        account_id: [u8; ID_BYTES],
        character_id: [u8; ID_BYTES],
        reward_event_id: [u8; ID_BYTES],
        boss_id: Option<&str>,
        contract: &StoredProgressionContract,
        operation: &mut F,
    ) -> Result<ProgressionAwardTransaction<T>, PersistenceError>
    where
        T: Send,
        F: FnMut(&mut ProgressionAwardTransactionState) -> Result<T, PersistenceError> + Send,
    {
        let mut transaction = self.begin_transaction().await?;
        let selected_character_id = lock_account(transaction.connection(), &account_id).await?;
        if let Some(existing) = load_award_result(
            transaction.connection(),
            &account_id,
            &reward_event_id,
            contract,
        )
        .await?
        {
            transaction.rollback().await?;
            return Ok(ProgressionAwardTransaction::Replayed(Box::new(existing)));
        }

        let selected_character_id = match selected_character_id {
            Some(bytes) => {
                let id = fixed_bytes(bytes)?;
                validate_nonzero_id(&id)?;
                Some(id)
            }
            None => None,
        };
        let (locked_character, location, initial_progression) = lock_fresh_award_aggregates(
            transaction.connection(),
            &account_id,
            &character_id,
            contract,
        )
        .await?;
        let entry_restore_point_id = location.entry_restore_point_id;
        let bargain_life =
            lock_bargain_milestone_life(transaction.connection(), &account_id, &character_id)
                .await?;
        let ash_wallet =
            lock_ash_wallet_on_connection(transaction.connection(), &account_id).await?;
        let boss_first_clear = match boss_id {
            None => StoredBossFirstClearState::NotApplicable,
            Some(boss_id) => load_first_clear(transaction.connection(), &account_id, boss_id)
                .await?
                .map_or_else(
                    || StoredBossFirstClearState::Vacant {
                        boss_id: boss_id.to_owned(),
                    },
                    StoredBossFirstClearState::Awarded,
                ),
        };
        let mut state = ProgressionAwardTransactionState {
            selected_character_id,
            character: locked_character.clone(),
            progression: initial_progression.clone(),
            entry_restore_point_id,
            location: location.clone(),
            boss_first_clear,
            bargain_life: bargain_life.clone(),
            ash_wallet,
            new_result: None,
            new_boss_first_clear: None,
            new_bargain_milestone: None,
        };
        let value = operation(&mut state)?;
        validate_fresh_state(
            &state,
            &FreshAwardBinding {
                initial_progression: &initial_progression,
                initial_character: &locked_character,
                initial_selected_character_id: selected_character_id,
                initial_entry_restore_point_id: entry_restore_point_id,
                account_id: &account_id,
                character_id: &character_id,
                reward_event_id: &reward_event_id,
                contract,
            },
        )?;

        persist_fresh_award(
            transaction.connection(),
            &state,
            FreshAwardCommitBinding {
                account_id: &account_id,
                character_id: &character_id,
                reward_event_id: &reward_event_id,
                initial_progression: &initial_progression,
                initial_bargain_life: &bargain_life,
                location: &location,
                ash_wallet,
                contract,
            },
        )
        .await?;
        transaction.commit().await?;
        Ok(ProgressionAwardTransaction::Committed(value))
    }
}

struct FreshAwardCommitBinding<'a> {
    account_id: &'a [u8; ID_BYTES],
    character_id: &'a [u8; ID_BYTES],
    reward_event_id: &'a [u8; ID_BYTES],
    initial_progression: &'a StoredProgression,
    initial_bargain_life: &'a StoredBargainMilestoneLife,
    location: &'a StoredProgressionAwardLocation,
    ash_wallet: StoredAshWallet,
    contract: &'a StoredProgressionContract,
}

async fn persist_fresh_award(
    connection: &mut sqlx::PgConnection,
    state: &ProgressionAwardTransactionState,
    binding: FreshAwardCommitBinding<'_>,
) -> Result<(), PersistenceError> {
    if state.progression != *binding.initial_progression {
        persist_progression(
            connection,
            binding.account_id,
            binding.character_id,
            &state.progression,
            binding.contract,
        )
        .await?;
    }
    let result = state
        .new_result
        .as_ref()
        .ok_or(PersistenceError::ProgressionAwardResultRequired)?;
    if let Some(staged) = &state.new_bargain_milestone {
        persist_bargain_milestone(
            connection,
            staged,
            BargainMilestoneBinding {
                account_id: binding.account_id,
                character_id: binding.character_id,
                reward_event_id: binding.reward_event_id,
                reward_payload_hash: &result.payload_hash,
                layout_id: binding.location.layout_id.as_deref(),
                instance_lineage_id: binding.location.instance_lineage_id.as_ref(),
                entry_restore_point_id: binding.location.entry_restore_point_id.as_ref(),
                initial_life: binding.initial_bargain_life,
                locked_wallet: binding.ash_wallet,
            },
        )
        .await?;
    }
    insert_award_result(connection, result, binding.contract).await?;
    if let Some(marker) = &state.new_boss_first_clear {
        insert_first_clear(connection, binding.account_id, marker).await?;
    }
    Ok(())
}

async fn lock_fresh_award_aggregates(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<
    (
        StoredLockedProgressionCharacter,
        StoredProgressionAwardLocation,
        StoredProgression,
    ),
    PersistenceError,
> {
    let character = lock_character(connection, account_id, character_id, contract).await?;
    let location = lock_award_location(
        connection,
        account_id,
        character_id,
        character.character_state_version,
    )
    .await?;
    let progression = lock_progression(connection, account_id, character_id, contract).await?;
    if character.cached_level != progression.level {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok((character, location, progression))
}

async fn lock_award_location(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    character_state_version: i64,
) -> Result<StoredProgressionAwardLocation, PersistenceError> {
    let row = sqlx::query(
        "SELECT character_version, location_kind, location_content_id, instance_lineage_id, \
                entry_restore_point_id \
         FROM character_world_locations WHERE namespace_id = $1 AND account_id = $2 \
         AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::ProgressionCharacterNotFound)?;
    let location_version: i64 = row
        .try_get("character_version")
        .map_err(PersistenceError::Database)?;
    let location_kind: i16 = row
        .try_get("location_kind")
        .map_err(PersistenceError::Database)?;
    let restore_point_id = row
        .try_get::<Option<Vec<u8>>, _>("entry_restore_point_id")
        .map_err(PersistenceError::Database)?
        .map(fixed_bytes)
        .transpose()?;
    let instance_lineage_id = row
        .try_get::<Option<Vec<u8>>, _>("instance_lineage_id")
        .map_err(PersistenceError::Database)?
        .map(fixed_bytes)
        .transpose()?;
    if location_version != character_state_version
        || !matches!(
            (location_kind, restore_point_id),
            (0 | 1, None) | (2, Some(_))
        )
        || matches!(location_kind, 0 | 1) && instance_lineage_id.is_some()
        || location_kind == 2 && instance_lineage_id.is_none()
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    let layout_id = match instance_lineage_id {
        None => None,
        Some(lineage_id) => {
            let row = sqlx::query(
                "SELECT layout_id, lineage_state FROM character_instance_lineages \
                 WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
                 AND lineage_id = $4 FOR UPDATE",
            )
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .bind(character_id.as_slice())
            .bind(lineage_id.as_slice())
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(PersistenceError::CorruptStoredProgression)?;
            let lineage_state: i16 = row.try_get("lineage_state")?;
            if !matches!(lineage_state, 0 | 1) {
                return Err(PersistenceError::CorruptStoredProgression);
            }
            let restore_state: i16 = sqlx::query_scalar(
                "SELECT restore_state FROM character_entry_restore_points \
                 WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 \
                 AND restore_point_id = $4 AND lineage_id = $5 FOR UPDATE",
            )
            .bind(WIPEABLE_CORE_NAMESPACE)
            .bind(account_id.as_slice())
            .bind(character_id.as_slice())
            .bind(
                restore_point_id
                    .ok_or(PersistenceError::CorruptStoredProgression)?
                    .as_slice(),
            )
            .bind(lineage_id.as_slice())
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(PersistenceError::CorruptStoredProgression)?;
            if restore_state != 0 {
                return Err(PersistenceError::CorruptStoredProgression);
            }
            row.try_get("layout_id")?
        }
    };
    Ok(StoredProgressionAwardLocation {
        location_kind,
        location_content_id: row.try_get("location_content_id")?,
        layout_id,
        instance_lineage_id,
        entry_restore_point_id: restore_point_id,
    })
}

async fn lock_account(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
) -> Result<Option<Vec<u8>>, PersistenceError> {
    let row = sqlx::query(
        "SELECT selected_character_id FROM accounts \
         WHERE namespace_id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    let row = row.ok_or(PersistenceError::ProgressionCharacterNotFound)?;
    row.try_get("selected_character_id")
        .map_err(PersistenceError::Database)
}

async fn lock_character(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<StoredLockedProgressionCharacter, PersistenceError> {
    let row = sqlx::query(
        "SELECT level, life_state, security_state, character_state_version FROM characters \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::ProgressionCharacterNotFound)?;
    let cached_level: i32 = row.try_get("level").map_err(PersistenceError::Database)?;
    let character = StoredLockedProgressionCharacter {
        cached_level: cached_level
            .try_into()
            .map_err(|_| PersistenceError::CorruptStoredProgression)?,
        life_state: row
            .try_get("life_state")
            .map_err(PersistenceError::Database)?,
        security_state: row
            .try_get("security_state")
            .map_err(PersistenceError::Database)?,
        character_state_version: row
            .try_get("character_state_version")
            .map_err(PersistenceError::Database)?,
    };
    validate_locked_character(&character, contract)?;
    Ok(character)
}

async fn lock_progression(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<StoredProgression, PersistenceError> {
    let row = sqlx::query(
        "SELECT total_xp, level, current_health, progression_version \
         FROM character_progression \
         WHERE namespace_id = $1 AND account_id = $2 AND character_id = $3 FOR UPDATE",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?
    .ok_or(PersistenceError::ProgressionCharacterNotFound)?;
    decode_progression(&row, contract)
}

async fn load_award_result(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    reward_event_id: &[u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<Option<StoredXpAwardResult>, PersistenceError> {
    let row = sqlx::query(
        "SELECT character_id, payload_hash, source_content_id, xp_profile_id, \
                progression_content_revision, eligibility_kind, eligible, \
                normal_delta_x_milli_tiles, normal_delta_y_milli_tiles, normal_window_ticks, \
                normal_actual_damage, normal_effective_support, normal_living_at_death, \
                encounter_active_ticks, \
                encounter_present_ticks, encounter_longest_inactivity_ticks, \
                encounter_reference_health, encounter_direct_damage, \
                encounter_effective_healing, encounter_damage_prevented, \
                encounter_objective_credits, encounter_life_state, encounter_recall_state, \
                encounter_trust_state, first_clear_awarded, base_xp, bonus_xp, requested_xp, \
                applied_xp, discarded_xp, pre_total_xp, post_total_xp, pre_level, post_level, \
                pre_progression_version, post_progression_version, result_code, result_payload, \
                entry_restore_point_id, revoked_by_restore_point_id, \
                revocation_progression_version \
         FROM character_xp_award_results \
         WHERE namespace_id = $1 AND account_id = $2 AND reward_event_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(reward_event_id.as_slice())
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    row.map(|row| decode_award_result(&row, account_id, reward_event_id, contract))
        .transpose()
}

async fn load_first_clear(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    boss_id: &str,
) -> Result<Option<StoredBossFirstClear>, PersistenceError> {
    let row = sqlx::query(
        "SELECT reward_event_id, character_id FROM account_boss_first_clears \
         WHERE namespace_id = $1 AND account_id = $2 AND boss_id = $3",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(boss_id)
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Database)?;
    row.map(|row| {
        let marker = StoredBossFirstClear {
            boss_id: boss_id.to_owned(),
            reward_event_id: fixed_bytes(
                row.try_get("reward_event_id")
                    .map_err(PersistenceError::Database)?,
            )?,
            character_id: fixed_bytes(
                row.try_get("character_id")
                    .map_err(PersistenceError::Database)?,
            )?,
        };
        validate_first_clear(&marker)?;
        Ok(marker)
    })
    .transpose()
}

fn decode_progression(
    row: &sqlx::postgres::PgRow,
    contract: &StoredProgressionContract,
) -> Result<StoredProgression, PersistenceError> {
    let progression = StoredProgression {
        total_xp: row
            .try_get("total_xp")
            .map_err(PersistenceError::Database)?,
        level: row.try_get("level").map_err(PersistenceError::Database)?,
        current_health: row
            .try_get("current_health")
            .map_err(PersistenceError::Database)?,
        progression_version: row
            .try_get("progression_version")
            .map_err(PersistenceError::Database)?,
    };
    validate_progression(&progression, contract)?;
    Ok(progression)
}

fn decode_award_result(
    row: &sqlx::postgres::PgRow,
    account_id: &[u8; ID_BYTES],
    reward_event_id: &[u8; ID_BYTES],
    contract: &StoredProgressionContract,
) -> Result<StoredXpAwardResult, PersistenceError> {
    let eligibility_kind: i16 = row
        .try_get("eligibility_kind")
        .map_err(PersistenceError::Database)?;
    let evidence = decode_evidence(row, eligibility_kind)?;
    let result = StoredXpAwardResult {
        account_id: *account_id,
        character_id: fixed_bytes(
            row.try_get("character_id")
                .map_err(PersistenceError::Database)?,
        )?,
        reward_event_id: *reward_event_id,
        payload_hash: fixed_bytes(
            row.try_get("payload_hash")
                .map_err(PersistenceError::Database)?,
        )?,
        source_content_id: row
            .try_get("source_content_id")
            .map_err(PersistenceError::Database)?,
        xp_profile_id: row
            .try_get("xp_profile_id")
            .map_err(PersistenceError::Database)?,
        progression_content_revision: row
            .try_get("progression_content_revision")
            .map_err(PersistenceError::Database)?,
        entry_restore_point_id: row
            .try_get::<Option<Vec<u8>>, _>("entry_restore_point_id")
            .map_err(PersistenceError::Database)?
            .map(fixed_bytes)
            .transpose()?,
        revoked_by_restore_point_id: row
            .try_get::<Option<Vec<u8>>, _>("revoked_by_restore_point_id")
            .map_err(PersistenceError::Database)?
            .map(fixed_bytes)
            .transpose()?,
        revocation_progression_version: row
            .try_get("revocation_progression_version")
            .map_err(PersistenceError::Database)?,
        evidence,
        eligible: row
            .try_get("eligible")
            .map_err(PersistenceError::Database)?,
        first_clear_awarded: row
            .try_get("first_clear_awarded")
            .map_err(PersistenceError::Database)?,
        base_xp: row.try_get("base_xp").map_err(PersistenceError::Database)?,
        bonus_xp: row
            .try_get("bonus_xp")
            .map_err(PersistenceError::Database)?,
        requested_xp: row
            .try_get("requested_xp")
            .map_err(PersistenceError::Database)?,
        applied_xp: row
            .try_get("applied_xp")
            .map_err(PersistenceError::Database)?,
        discarded_xp: row
            .try_get("discarded_xp")
            .map_err(PersistenceError::Database)?,
        pre_total_xp: row
            .try_get("pre_total_xp")
            .map_err(PersistenceError::Database)?,
        post_total_xp: row
            .try_get("post_total_xp")
            .map_err(PersistenceError::Database)?,
        pre_level: row
            .try_get("pre_level")
            .map_err(PersistenceError::Database)?,
        post_level: row
            .try_get("post_level")
            .map_err(PersistenceError::Database)?,
        pre_progression_version: row
            .try_get("pre_progression_version")
            .map_err(PersistenceError::Database)?,
        post_progression_version: row
            .try_get("post_progression_version")
            .map_err(PersistenceError::Database)?,
        result_code: row
            .try_get("result_code")
            .map_err(PersistenceError::Database)?,
        result_payload: row
            .try_get("result_payload")
            .map_err(PersistenceError::Database)?,
    };
    validate_award_result(&result, contract)?;
    Ok(result)
}

fn decode_evidence(
    row: &sqlx::postgres::PgRow,
    eligibility_kind: i16,
) -> Result<StoredXpEligibilityEvidence, PersistenceError> {
    match eligibility_kind {
        0 => {
            ensure_encounter_columns_absent(row)?;
            Ok(StoredXpEligibilityEvidence::Ordinary(
                StoredOrdinaryXpEvidence {
                    delta_x_milli_tiles: required_column(row, "normal_delta_x_milli_tiles")?,
                    delta_y_milli_tiles: required_column(row, "normal_delta_y_milli_tiles")?,
                    window_ticks: required_column(row, "normal_window_ticks")?,
                    actual_health_damage: required_column(row, "normal_actual_damage")?,
                    effective_support: required_column(row, "normal_effective_support")?,
                    living_at_enemy_death: required_column(row, "normal_living_at_death")?,
                },
            ))
        }
        1 => {
            ensure_ordinary_columns_absent(row)?;
            Ok(StoredXpEligibilityEvidence::Encounter(
                StoredEncounterXpEvidence {
                    active_ticks: required_column(row, "encounter_active_ticks")?,
                    present_ticks: required_column(row, "encounter_present_ticks")?,
                    longest_inactivity_ticks: required_column(
                        row,
                        "encounter_longest_inactivity_ticks",
                    )?,
                    reference_health: required_column(row, "encounter_reference_health")?,
                    direct_damage: required_column(row, "encounter_direct_damage")?,
                    effective_healing: required_column(row, "encounter_effective_healing")?,
                    damage_prevented: required_column(row, "encounter_damage_prevented")?,
                    objective_credits: required_column(row, "encounter_objective_credits")?,
                    life_state: decode_life_state(required_column(row, "encounter_life_state")?)?,
                    recall_state: decode_recall_state(required_column(
                        row,
                        "encounter_recall_state",
                    )?)?,
                    trust_state: decode_trust_state(required_column(
                        row,
                        "encounter_trust_state",
                    )?)?,
                },
            ))
        }
        _ => Err(PersistenceError::CorruptStoredProgression),
    }
}

fn required_column<T>(row: &sqlx::postgres::PgRow, column: &str) -> Result<T, PersistenceError>
where
    for<'decode> T: sqlx::Decode<'decode, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get::<Option<T>, _>(column)
        .map_err(PersistenceError::Database)?
        .ok_or(PersistenceError::CorruptStoredProgression)
}

fn ensure_ordinary_columns_absent(row: &sqlx::postgres::PgRow) -> Result<(), PersistenceError> {
    ensure_null::<i32>(row, "normal_delta_x_milli_tiles")?;
    ensure_null::<i32>(row, "normal_delta_y_milli_tiles")?;
    ensure_null::<i32>(row, "normal_window_ticks")?;
    ensure_null::<i64>(row, "normal_actual_damage")?;
    ensure_null::<bool>(row, "normal_effective_support")?;
    ensure_null::<bool>(row, "normal_living_at_death")
}

fn ensure_encounter_columns_absent(row: &sqlx::postgres::PgRow) -> Result<(), PersistenceError> {
    for column in [
        "encounter_active_ticks",
        "encounter_present_ticks",
        "encounter_longest_inactivity_ticks",
        "encounter_reference_health",
        "encounter_direct_damage",
        "encounter_effective_healing",
        "encounter_damage_prevented",
    ] {
        ensure_null::<i64>(row, column)?;
    }
    for column in [
        "encounter_objective_credits",
        "encounter_life_state",
        "encounter_recall_state",
        "encounter_trust_state",
    ] {
        ensure_null::<i16>(row, column)?;
    }
    Ok(())
}

fn ensure_null<T>(row: &sqlx::postgres::PgRow, column: &str) -> Result<(), PersistenceError>
where
    for<'decode> T: sqlx::Decode<'decode, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    if row
        .try_get::<Option<T>, _>(column)
        .map_err(PersistenceError::Database)?
        .is_some()
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

async fn persist_progression(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    character_id: &[u8; ID_BYTES],
    progression: &StoredProgression,
    contract: &StoredProgressionContract,
) -> Result<(), PersistenceError> {
    validate_progression(progression, contract)?;
    let updated = sqlx::query(
        "UPDATE character_progression \
         SET total_xp = $1, level = $2, current_health = $3, progression_version = $4, \
             updated_at = transaction_timestamp() \
         WHERE namespace_id = $5 AND account_id = $6 AND character_id = $7",
    )
    .bind(progression.total_xp)
    .bind(progression.level)
    .bind(progression.current_health)
    .bind(progression.progression_version)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(&mut *connection)
    .await
    .map_err(PersistenceError::Database)?;
    if updated.rows_affected() != 1 {
        return Err(PersistenceError::ProgressionCharacterNotFound);
    }
    let cached = sqlx::query(
        "UPDATE characters SET level = $1, updated_at = transaction_timestamp() \
         WHERE namespace_id = $2 AND account_id = $3 AND character_id = $4",
    )
    .bind(progression.level)
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(character_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    if cached.rows_affected() != 1 {
        return Err(PersistenceError::ProgressionCharacterNotFound);
    }
    Ok(())
}

async fn insert_award_result(
    connection: &mut sqlx::PgConnection,
    result: &StoredXpAwardResult,
    contract: &StoredProgressionContract,
) -> Result<(), PersistenceError> {
    validate_award_result(result, contract)?;
    let (
        eligibility_kind,
        horizontal_delta,
        vertical_delta,
        normal_ticks,
        normal_damage,
        normal_support,
        normal_living,
        encounter_active,
        encounter_present,
        encounter_inactive,
        encounter_health,
        encounter_damage,
        encounter_healing,
        encounter_prevented,
        encounter_objectives,
        encounter_life,
        encounter_recall,
        encounter_trust,
    ) = evidence_columns(&result.evidence);
    sqlx::query(
        "INSERT INTO character_xp_award_results \
         (namespace_id, account_id, character_id, reward_event_id, payload_hash, \
          source_content_id, xp_profile_id, progression_content_revision, eligibility_kind, \
          eligible, normal_delta_x_milli_tiles, normal_delta_y_milli_tiles, \
          normal_window_ticks, normal_actual_damage, normal_effective_support, \
          normal_living_at_death, \
          encounter_active_ticks, encounter_present_ticks, encounter_longest_inactivity_ticks, \
          encounter_reference_health, encounter_direct_damage, encounter_effective_healing, \
          encounter_damage_prevented, encounter_objective_credits, encounter_life_state, \
          encounter_recall_state, encounter_trust_state, first_clear_awarded, base_xp, bonus_xp, \
          requested_xp, applied_xp, discarded_xp, pre_total_xp, post_total_xp, pre_level, \
          post_level, pre_progression_version, post_progression_version, result_code, result_payload, \
          entry_restore_point_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
                 $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, \
                 $30, $31, $32, $33, $34, $35, $36, $37, $38, $39, $40, $41, $42)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(result.account_id.as_slice())
    .bind(result.character_id.as_slice())
    .bind(result.reward_event_id.as_slice())
    .bind(result.payload_hash.as_slice())
    .bind(&result.source_content_id)
    .bind(result.xp_profile_id.as_deref())
    .bind(&result.progression_content_revision)
    .bind(eligibility_kind)
    .bind(result.eligible)
    .bind(horizontal_delta)
    .bind(vertical_delta)
    .bind(normal_ticks)
    .bind(normal_damage)
    .bind(normal_support)
    .bind(normal_living)
    .bind(encounter_active)
    .bind(encounter_present)
    .bind(encounter_inactive)
    .bind(encounter_health)
    .bind(encounter_damage)
    .bind(encounter_healing)
    .bind(encounter_prevented)
    .bind(encounter_objectives)
    .bind(encounter_life)
    .bind(encounter_recall)
    .bind(encounter_trust)
    .bind(result.first_clear_awarded)
    .bind(result.base_xp)
    .bind(result.bonus_xp)
    .bind(result.requested_xp)
    .bind(result.applied_xp)
    .bind(result.discarded_xp)
    .bind(result.pre_total_xp)
    .bind(result.post_total_xp)
    .bind(result.pre_level)
    .bind(result.post_level)
    .bind(result.pre_progression_version)
    .bind(result.post_progression_version)
    .bind(result.result_code)
    .bind(&result.result_payload)
    .bind(
        result
            .entry_restore_point_id
            .as_ref()
            .map(<[u8; ID_BYTES]>::as_slice),
    )
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

#[allow(clippy::type_complexity)]
fn evidence_columns(
    evidence: &StoredXpEligibilityEvidence,
) -> (
    i16,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i64>,
    Option<bool>,
    Option<bool>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i16>,
    Option<i16>,
    Option<i16>,
    Option<i16>,
) {
    match evidence {
        StoredXpEligibilityEvidence::Ordinary(evidence) => (
            0,
            Some(evidence.delta_x_milli_tiles),
            Some(evidence.delta_y_milli_tiles),
            Some(evidence.window_ticks),
            Some(evidence.actual_health_damage),
            Some(evidence.effective_support),
            Some(evidence.living_at_enemy_death),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ),
        StoredXpEligibilityEvidence::Encounter(evidence) => (
            1,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(evidence.active_ticks),
            Some(evidence.present_ticks),
            Some(evidence.longest_inactivity_ticks),
            Some(evidence.reference_health),
            Some(evidence.direct_damage),
            Some(evidence.effective_healing),
            Some(evidence.damage_prevented),
            Some(evidence.objective_credits),
            Some(encode_life_state(evidence.life_state)),
            Some(encode_recall_state(evidence.recall_state)),
            Some(encode_trust_state(evidence.trust_state)),
        ),
    }
}

async fn insert_first_clear(
    connection: &mut sqlx::PgConnection,
    account_id: &[u8; ID_BYTES],
    marker: &StoredBossFirstClear,
) -> Result<(), PersistenceError> {
    validate_first_clear(marker)?;
    sqlx::query(
        "INSERT INTO account_boss_first_clears \
         (namespace_id, account_id, boss_id, reward_event_id, character_id) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(WIPEABLE_CORE_NAMESPACE)
    .bind(account_id.as_slice())
    .bind(&marker.boss_id)
    .bind(marker.reward_event_id.as_slice())
    .bind(marker.character_id.as_slice())
    .execute(connection)
    .await
    .map_err(PersistenceError::Database)?;
    Ok(())
}

struct FreshAwardBinding<'a> {
    initial_progression: &'a StoredProgression,
    initial_character: &'a StoredLockedProgressionCharacter,
    initial_selected_character_id: Option<[u8; ID_BYTES]>,
    initial_entry_restore_point_id: Option<[u8; ID_BYTES]>,
    account_id: &'a [u8; ID_BYTES],
    character_id: &'a [u8; ID_BYTES],
    reward_event_id: &'a [u8; ID_BYTES],
    contract: &'a StoredProgressionContract,
}

fn validate_fresh_state(
    state: &ProgressionAwardTransactionState,
    binding: &FreshAwardBinding<'_>,
) -> Result<(), PersistenceError> {
    validate_progression(&state.progression, binding.contract)?;
    validate_locked_character(&state.character, binding.contract)?;
    let result = state
        .new_result
        .as_ref()
        .ok_or(PersistenceError::ProgressionAwardResultRequired)?;
    validate_award_result(result, binding.contract)?;
    if state.selected_character_id != binding.initial_selected_character_id
        || state.entry_restore_point_id != binding.initial_entry_restore_point_id
        || &state.character != binding.initial_character
        || &result.account_id != binding.account_id
        || &result.character_id != binding.character_id
        || &result.reward_event_id != binding.reward_event_id
        || result.entry_restore_point_id != binding.initial_entry_restore_point_id
        || result.revoked_by_restore_point_id.is_some()
        || result.revocation_progression_version.is_some()
        || result.pre_total_xp != binding.initial_progression.total_xp
        || result.pre_level != binding.initial_progression.level
        || result.pre_progression_version != binding.initial_progression.progression_version
        || result.post_total_xp != state.progression.total_xp
        || result.post_level != state.progression.level
        || result.post_progression_version != state.progression.progression_version
        || state.progression.current_health != binding.initial_progression.current_health
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }

    match (&state.boss_first_clear, &state.new_boss_first_clear) {
        (_, None) => {
            if result.first_clear_awarded {
                return Err(PersistenceError::CorruptStoredProgression);
            }
        }
        (StoredBossFirstClearState::Vacant { boss_id }, Some(marker)) => {
            if !result.first_clear_awarded
                || marker.boss_id != *boss_id
                || marker.reward_event_id != *binding.reward_event_id
                || marker.character_id != *binding.character_id
            {
                return Err(PersistenceError::CorruptStoredProgression);
            }
            validate_first_clear(marker)?;
        }
        (
            StoredBossFirstClearState::NotApplicable | StoredBossFirstClearState::Awarded(_),
            Some(_),
        ) => {
            return Err(PersistenceError::CorruptStoredProgression);
        }
    }
    Ok(())
}

fn validate_progression(
    progression: &StoredProgression,
    contract: &StoredProgressionContract,
) -> Result<(), PersistenceError> {
    validate_contract(contract)?;
    let maximum_xp = *contract
        .cumulative_xp
        .last()
        .ok_or(PersistenceError::CorruptStoredProgression)?;
    if progression.total_xp < 0
        || progression.total_xp > maximum_xp
        || progression.current_health < 1
        || progression.progression_version < 1
        || level_for_total_xp(progression.total_xp, contract) != Some(progression.level)
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn validate_award_result(
    result: &StoredXpAwardResult,
    contract: &StoredProgressionContract,
) -> Result<(), PersistenceError> {
    validate_contract(contract)?;
    let maximum_xp = *contract
        .cumulative_xp
        .last()
        .ok_or(PersistenceError::CorruptStoredProgression)?;
    validate_nonzero_id(&result.account_id)?;
    validate_nonzero_id(&result.character_id)?;
    validate_nonzero_id(&result.reward_event_id)?;
    if result.payload_hash.iter().all(|byte| *byte == 0) {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    validate_bounded_id(&result.source_content_id)?;
    if let Some(profile_id) = &result.xp_profile_id {
        validate_bounded_id(profile_id)?;
    }
    if result.progression_content_revision.len() != 64
        || !result
            .progression_content_revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    validate_evidence(&result.evidence)?;
    if result.base_xp < 0
        || result.bonus_xp < 0
        || result.applied_xp < 0
        || result.discarded_xp < 0
        || result.base_xp.checked_add(result.bonus_xp) != Some(result.requested_xp)
        || result.applied_xp.checked_add(result.discarded_xp) != Some(result.requested_xp)
        || result.pre_total_xp.checked_add(result.applied_xp) != Some(result.post_total_xp)
        || !(0..=maximum_xp).contains(&result.pre_total_xp)
        || !(result.pre_total_xp..=maximum_xp).contains(&result.post_total_xp)
        || level_for_total_xp(result.pre_total_xp, contract) != Some(result.pre_level)
        || level_for_total_xp(result.post_total_xp, contract) != Some(result.post_level)
        || result.post_level < result.pre_level
        || result.pre_progression_version < 1
        || result.post_progression_version
            != result.pre_progression_version + i64::from(result.applied_xp > 0)
        || !(0..=12).contains(&result.result_code)
        || result.result_payload.is_empty()
        || result.result_payload.len() > MAX_RESULT_PAYLOAD_BYTES
        || (result.eligible && result.xp_profile_id.is_none())
        || (!result.eligible
            && (result.first_clear_awarded
                || result.base_xp != 0
                || result.bonus_xp != 0
                || result.requested_xp != 0
                || result.applied_xp != 0
                || result.discarded_xp != 0))
        || (result.first_clear_awarded && result.bonus_xp == 0)
        || result
            .entry_restore_point_id
            .is_some_and(|id| id == [0; ID_BYTES])
        || result
            .revoked_by_restore_point_id
            .is_some_and(|id| id == [0; ID_BYTES])
        || match (
            result.entry_restore_point_id,
            result.revoked_by_restore_point_id,
            result.revocation_progression_version,
        ) {
            (_, None, None) => false,
            (Some(entry), Some(revoked), Some(version)) => {
                entry != revoked || version <= result.post_progression_version
            }
            _ => true,
        }
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn validate_evidence(evidence: &StoredXpEligibilityEvidence) -> Result<(), PersistenceError> {
    let valid = match evidence {
        StoredXpEligibilityEvidence::Ordinary(evidence) => {
            (0..=300).contains(&evidence.window_ticks) && evidence.actual_health_damage >= 0
        }
        StoredXpEligibilityEvidence::Encounter(evidence) => {
            evidence.active_ticks > 0
                && (0..=evidence.active_ticks).contains(&evidence.present_ticks)
                && evidence.longest_inactivity_ticks >= 0
                && evidence.reference_health > 0
                && evidence.direct_damage >= 0
                && evidence.effective_healing >= 0
                && evidence.damage_prevented >= 0
                && (0..=2).contains(&evidence.objective_credits)
        }
    };
    if !valid {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn validate_first_clear(marker: &StoredBossFirstClear) -> Result<(), PersistenceError> {
    validate_bounded_id(&marker.boss_id)?;
    validate_nonzero_id(&marker.reward_event_id)?;
    validate_nonzero_id(&marker.character_id)
}

fn validate_bounded_id(value: &str) -> Result<(), PersistenceError> {
    if !(3..=96).contains(&value.chars().count()) {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn validate_nonzero_id(id: &[u8; ID_BYTES]) -> Result<(), PersistenceError> {
    if id.iter().all(|byte| *byte == 0) {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn validate_contract(contract: &StoredProgressionContract) -> Result<(), PersistenceError> {
    if contract.cumulative_xp[0] != 0
        || contract
            .cumulative_xp
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn validate_locked_character(
    character: &StoredLockedProgressionCharacter,
    contract: &StoredProgressionContract,
) -> Result<(), PersistenceError> {
    if !(1..=i16::try_from(contract.cumulative_xp.len())
        .map_err(|_| PersistenceError::CorruptStoredProgression)?)
        .contains(&character.cached_level)
        || character.life_state < 0
        || character.security_state < 0
        || character.character_state_version < 1
    {
        return Err(PersistenceError::CorruptStoredProgression);
    }
    Ok(())
}

fn level_for_total_xp(total_xp: i32, contract: &StoredProgressionContract) -> Option<i16> {
    if total_xp < 0 || total_xp > *contract.cumulative_xp.last()? {
        return None;
    }
    contract
        .cumulative_xp
        .partition_point(|threshold| *threshold <= total_xp)
        .try_into()
        .ok()
}

const fn encode_life_state(state: StoredEncounterLifeState) -> i16 {
    match state {
        StoredEncounterLifeState::Living => 0,
        StoredEncounterLifeState::Dead => 1,
    }
}

fn decode_life_state(value: i16) -> Result<StoredEncounterLifeState, PersistenceError> {
    match value {
        0 => Ok(StoredEncounterLifeState::Living),
        1 => Ok(StoredEncounterLifeState::Dead),
        _ => Err(PersistenceError::CorruptStoredProgression),
    }
}

const fn encode_recall_state(state: StoredEncounterRecallState) -> i16 {
    match state {
        StoredEncounterRecallState::Present => 0,
        StoredEncounterRecallState::Recalled => 1,
    }
}

fn decode_recall_state(value: i16) -> Result<StoredEncounterRecallState, PersistenceError> {
    match value {
        0 => Ok(StoredEncounterRecallState::Present),
        1 => Ok(StoredEncounterRecallState::Recalled),
        _ => Err(PersistenceError::CorruptStoredProgression),
    }
}

const fn encode_trust_state(state: StoredEncounterTrustState) -> i16 {
    match state {
        StoredEncounterTrustState::Valid => 0,
        StoredEncounterTrustState::InvalidSession => 1,
        StoredEncounterTrustState::AntiCheatRejected => 2,
    }
}

fn decode_trust_state(value: i16) -> Result<StoredEncounterTrustState, PersistenceError> {
    match value {
        0 => Ok(StoredEncounterTrustState::Valid),
        1 => Ok(StoredEncounterTrustState::InvalidSession),
        2 => Ok(StoredEncounterTrustState::AntiCheatRejected),
        _ => Err(PersistenceError::CorruptStoredProgression),
    }
}

fn fixed_bytes<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], PersistenceError> {
    bytes
        .try_into()
        .map_err(|_| PersistenceError::CorruptStoredProgression)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> StoredProgressionContract {
        StoredProgressionContract {
            cumulative_xp: [0, 100, 200, 300, 400, 500, 600, 700, 800, 900],
        }
    }

    fn ordinary_result() -> StoredXpAwardResult {
        StoredXpAwardResult {
            account_id: [1; 16],
            character_id: [2; 16],
            reward_event_id: [3; 16],
            payload_hash: [4; 32],
            source_content_id: "enemy.drowned_pilgrim".to_owned(),
            xp_profile_id: Some("xp.normal_t1".to_owned()),
            progression_content_revision: "a".repeat(64),
            entry_restore_point_id: None,
            revoked_by_restore_point_id: None,
            revocation_progression_version: None,
            evidence: StoredXpEligibilityEvidence::Ordinary(StoredOrdinaryXpEvidence {
                delta_x_milli_tiles: 1_000,
                delta_y_milli_tiles: -2_000,
                window_ticks: 300,
                actual_health_damage: 1,
                effective_support: false,
                living_at_enemy_death: true,
            }),
            eligible: true,
            first_clear_awarded: false,
            base_xp: 5,
            bonus_xp: 0,
            requested_xp: 5,
            applied_xp: 5,
            discarded_xp: 0,
            pre_total_xp: 95,
            post_total_xp: 100,
            pre_level: 1,
            post_level: 2,
            pre_progression_version: 7,
            post_progression_version: 8,
            result_code: 0,
            result_payload: vec![9],
        }
    }

    #[test]
    fn ordinary_and_encounter_evidence_are_disjoint_and_bounded() {
        let ordinary = ordinary_result();
        assert!(validate_award_result(&ordinary, &contract()).is_ok());

        let mut encounter = ordinary;
        encounter.source_content_id = "boss.sir_caldus".to_owned();
        encounter.xp_profile_id = Some("xp.boss_caldus".to_owned());
        encounter.evidence = StoredXpEligibilityEvidence::Encounter(StoredEncounterXpEvidence {
            active_ticks: 3_000,
            present_ticks: 2_000,
            longest_inactivity_ticks: 20,
            reference_health: 4_200,
            direct_damage: 210,
            effective_healing: 0,
            damage_prevented: 0,
            objective_credits: 0,
            life_state: StoredEncounterLifeState::Living,
            recall_state: StoredEncounterRecallState::Present,
            trust_state: StoredEncounterTrustState::Valid,
        });
        assert!(validate_award_result(&encounter, &contract()).is_ok());
        if let StoredXpEligibilityEvidence::Encounter(evidence) = &mut encounter.evidence {
            evidence.present_ticks = evidence.active_ticks + 1;
        }
        assert!(validate_award_result(&encounter, &contract()).is_err());
    }

    #[test]
    fn award_validation_rejects_arithmetic_versions_and_corrupt_revisions() {
        let mut result = ordinary_result();
        result.post_progression_version = result.pre_progression_version;
        assert!(validate_award_result(&result, &contract()).is_err());
        result = ordinary_result();
        result.requested_xp += 1;
        assert!(validate_award_result(&result, &contract()).is_err());
        result = ordinary_result();
        result.progression_content_revision = "A".repeat(64);
        assert!(validate_award_result(&result, &contract()).is_err());
    }

    #[test]
    fn revoked_award_requires_the_exact_bound_restore_and_later_version() {
        let mut result = ordinary_result();
        result.entry_restore_point_id = Some([5; 16]);
        result.revoked_by_restore_point_id = Some([5; 16]);
        result.revocation_progression_version = Some(9);
        assert!(validate_award_result(&result, &contract()).is_ok());

        result.revoked_by_restore_point_id = Some([6; 16]);
        assert!(validate_award_result(&result, &contract()).is_err());
        result.revoked_by_restore_point_id = Some([5; 16]);
        result.revocation_progression_version = Some(result.post_progression_version);
        assert!(validate_award_result(&result, &contract()).is_err());
    }

    #[test]
    fn only_rejected_awards_may_omit_an_xp_profile() {
        let mut rejected = ordinary_result();
        rejected.xp_profile_id = None;
        rejected.eligible = false;
        rejected.base_xp = 0;
        rejected.requested_xp = 0;
        rejected.applied_xp = 0;
        rejected.pre_total_xp = 95;
        rejected.post_total_xp = 95;
        rejected.pre_level = 1;
        rejected.post_level = 1;
        rejected.post_progression_version = rejected.pre_progression_version;
        assert!(validate_award_result(&rejected, &contract()).is_ok());

        rejected.eligible = true;
        assert!(validate_award_result(&rejected, &contract()).is_err());
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn fresh_state_requires_exact_projection_and_first_clear_marker_binding() {
        let initial = StoredProgression {
            total_xp: 0,
            level: 1,
            current_health: 120,
            progression_version: 1,
        };
        let locked_character = StoredLockedProgressionCharacter {
            cached_level: 1,
            life_state: 0,
            security_state: 0,
            character_state_version: 1,
        };
        let mut result = ordinary_result();
        result.source_content_id = "boss.sir_caldus".to_owned();
        result.xp_profile_id = Some("xp.boss_caldus".to_owned());
        result.evidence = StoredXpEligibilityEvidence::Encounter(StoredEncounterXpEvidence {
            active_ticks: 100,
            present_ticks: 100,
            longest_inactivity_ticks: 0,
            reference_health: 4_200,
            direct_damage: 100,
            effective_healing: 0,
            damage_prevented: 0,
            objective_credits: 0,
            life_state: StoredEncounterLifeState::Living,
            recall_state: StoredEncounterRecallState::Present,
            trust_state: StoredEncounterTrustState::Valid,
        });
        result.first_clear_awarded = true;
        result.base_xp = 450;
        result.bonus_xp = 225;
        result.requested_xp = 675;
        result.applied_xp = 675;
        result.pre_total_xp = 0;
        result.post_total_xp = 675;
        result.pre_level = 1;
        result.post_level = 7;
        result.pre_progression_version = 1;
        result.post_progression_version = 2;
        let marker = StoredBossFirstClear {
            boss_id: "boss.sir_caldus".to_owned(),
            reward_event_id: result.reward_event_id,
            character_id: result.character_id,
        };
        let mut state = ProgressionAwardTransactionState {
            selected_character_id: Some(result.character_id),
            character: locked_character.clone(),
            progression: StoredProgression {
                total_xp: 675,
                level: 7,
                current_health: 120,
                progression_version: 2,
            },
            entry_restore_point_id: None,
            location: StoredProgressionAwardLocation {
                location_kind: 1,
                location_content_id: Some("hub.lantern_halls_01".into()),
                layout_id: None,
                instance_lineage_id: None,
                entry_restore_point_id: None,
            },
            boss_first_clear: StoredBossFirstClearState::Vacant {
                boss_id: marker.boss_id.clone(),
            },
            bargain_life: StoredBargainMilestoneLife {
                earned_bargain_slots: 0,
                oath_bargain_version: 1,
                active_bargain_ids: Vec::new(),
                core_milestone_awarded: false,
            },
            ash_wallet: StoredAshWallet {
                balance: 0,
                wallet_version: 1,
            },
            new_result: Some(result.clone()),
            new_boss_first_clear: Some(marker),
            new_bargain_milestone: None,
        };
        assert!(
            validate_fresh_state(
                &state,
                &FreshAwardBinding {
                    initial_progression: &initial,
                    initial_character: &locked_character,
                    initial_selected_character_id: Some(result.character_id),
                    initial_entry_restore_point_id: None,
                    account_id: &result.account_id,
                    character_id: &result.character_id,
                    reward_event_id: &result.reward_event_id,
                    contract: &contract(),
                },
            )
            .is_ok()
        );
        state.new_boss_first_clear = None;
        assert!(
            validate_fresh_state(
                &state,
                &FreshAwardBinding {
                    initial_progression: &initial,
                    initial_character: &locked_character,
                    initial_selected_character_id: Some(result.character_id),
                    initial_entry_restore_point_id: None,
                    account_id: &result.account_id,
                    character_id: &result.character_id,
                    reward_event_id: &result.reward_event_id,
                    contract: &contract(),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn supplied_cap_discards_overflow_without_advancing_version_on_zero_application() {
        let mut result = ordinary_result();
        result.pre_total_xp = 900;
        result.post_total_xp = 900;
        result.pre_level = 10;
        result.post_level = 10;
        result.applied_xp = 0;
        result.discarded_xp = 5;
        result.pre_progression_version = 8;
        result.post_progression_version = 8;
        assert!(validate_award_result(&result, &contract()).is_ok());
    }

    #[test]
    fn progression_values_come_only_from_the_supplied_contract() {
        let shifted = StoredProgressionContract {
            cumulative_xp: [0, 25, 50, 75, 100, 125, 150, 175, 200, 225],
        };
        let progression = StoredProgression {
            total_xp: 100,
            level: 2,
            current_health: 120,
            progression_version: 1,
        };
        assert!(validate_progression(&progression, &contract()).is_ok());
        assert!(validate_progression(&progression, &shifted).is_err());
    }
}
