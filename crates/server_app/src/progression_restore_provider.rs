//! PostgreSQL-backed progression component for TECH-023 entry restore points.

use persistence::{
    PersistenceError, PersistenceTransaction, StoredProgression, StoredProgressionContract,
    capture_progression_restore, restore_progression_after_crash,
};

use crate::{
    CrashRestoreContext, EntryCaptureContext, EntryRestoreProvider, ProgressionRestoreV1,
    RestorePointError,
};

#[derive(Debug, Clone)]
pub struct PostgresProgressionRestoreProvider {
    contract: StoredProgressionContract,
}

impl PostgresProgressionRestoreProvider {
    pub fn new(
        content: &sim_content::CoreDevelopmentProgression,
    ) -> Result<Self, crate::ProgressionAwardError> {
        let rules = crate::CoreProgressionRules::from_content(content)?;
        let cumulative_xp = rules
            .curve()
            .cumulative_xp
            .map(i32::try_from)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| crate::ProgressionAwardError::InvalidContent)?
            .try_into()
            .map_err(|_| crate::ProgressionAwardError::InvalidContent)?;
        Ok(Self {
            contract: StoredProgressionContract { cumulative_xp },
        })
    }
}

impl EntryRestoreProvider for PostgresProgressionRestoreProvider {
    type Snapshot = ProgressionRestoreV1;

    async fn capture<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: EntryCaptureContext,
    ) -> Result<Self::Snapshot, RestorePointError> {
        let stored = capture_progression_restore(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
            &self.contract,
        )
        .await
        .map_err(|error| map_persistence_error(&error))?;
        protocol_snapshot(&stored)
    }

    async fn restore_and_revoke_post_entry<'a>(
        &'a self,
        transaction: &'a mut PersistenceTransaction<'_>,
        context: CrashRestoreContext,
    ) -> Result<(), RestorePointError> {
        restore_progression_after_crash(
            transaction,
            context.account_id,
            context.character_id,
            context.restore_point_id,
            &self.contract,
        )
        .await
        .map(|_| ())
        .map_err(|error| map_persistence_error(&error))
    }
}

fn protocol_snapshot(
    stored: &StoredProgression,
) -> Result<ProgressionRestoreV1, RestorePointError> {
    Ok(ProgressionRestoreV1 {
        level: u16::try_from(stored.level).map_err(|_| RestorePointError::InvalidProgression)?,
        xp: u32::try_from(stored.total_xp).map_err(|_| RestorePointError::InvalidProgression)?,
        current_health: u32::try_from(stored.current_health)
            .map_err(|_| RestorePointError::InvalidProgression)?,
        progression_version: u64::try_from(stored.progression_version)
            .map_err(|_| RestorePointError::InvalidProgression)?,
    })
}

fn map_persistence_error(error: &PersistenceError) -> RestorePointError {
    match error {
        PersistenceError::ProgressionRestorePointNotFound => {
            RestorePointError::IncompleteRestorePoint
        }
        PersistenceError::ProgressionRestoreSuperseded => RestorePointError::RestoreSuperseded,
        _ => RestorePointError::Persistence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_snapshot_conversion_is_exact_and_bounded() {
        let stored = StoredProgression {
            total_xp: 1_350,
            level: 7,
            current_health: 88,
            progression_version: 9,
        };
        assert_eq!(
            protocol_snapshot(&stored).unwrap(),
            ProgressionRestoreV1 {
                level: 7,
                xp: 1_350,
                current_health: 88,
                progression_version: 9,
            }
        );
    }
}
