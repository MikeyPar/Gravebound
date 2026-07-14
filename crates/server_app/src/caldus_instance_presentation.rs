//! Sir Caldus instance presentation derived only from a durable victory exit.
//!
//! The ordinary M02 scheduler remains untouched. This owner is the 03E seam consumed by the
//! native instance renderer and, later, the extraction interaction boundary.

use content_schema::{CoreCaldusSafeArrival, MilliTilePoint};
use persistence::StoredCaldusVictoryExit;
use sim_content::CoreDevelopmentCaldus;
use thiserror::Error;

const CALDUS_EXIT_ID: &str = "portal.exit.dungeon.bell_sepulcher";
const CALDUS_EXIT_ASSET_ID: &str = "sprite.portal.exit.dungeon.bell_sepulcher";
const CALDUS_EXIT_DESTINATION_ID: &str = "hub.lantern_halls_01";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusExitPresentation {
    pub exit_instance_id: [u8; 16],
    pub content_id: String,
    pub asset_id: String,
    pub display_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub point: MilliTilePoint,
    pub destination_content_id: String,
    pub arrival: CoreCaldusSafeArrival,
    pub requires_committed_extraction_receipt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaldusExitPresentationCommit {
    Fresh,
    Replay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaldusInstancePresentation {
    instance_lineage_id: [u8; 16],
    attempt_ordinal: u32,
    exit: Option<CaldusExitPresentation>,
}

impl CaldusInstancePresentation {
    pub fn new(
        instance_lineage_id: [u8; 16],
        attempt_ordinal: u32,
    ) -> Result<Self, CaldusInstancePresentationError> {
        validate_attempt(instance_lineage_id, attempt_ordinal)?;
        Ok(Self {
            instance_lineage_id,
            attempt_ordinal,
            exit: None,
        })
    }

    #[must_use]
    pub const fn instance_lineage_id(&self) -> [u8; 16] {
        self.instance_lineage_id
    }

    #[must_use]
    pub const fn attempt_ordinal(&self) -> u32 {
        self.attempt_ordinal
    }

    #[must_use]
    pub const fn exit(&self) -> Option<&CaldusExitPresentation> {
        self.exit.as_ref()
    }

    pub fn reset_for_attempt(
        &mut self,
        attempt_ordinal: u32,
    ) -> Result<(), CaldusInstancePresentationError> {
        if self.exit.is_some() {
            return Err(CaldusInstancePresentationError::CommittedExitCannotReset);
        }
        if attempt_ordinal <= self.attempt_ordinal {
            return Err(CaldusInstancePresentationError::AttemptDidNotAdvance);
        }
        self.attempt_ordinal = attempt_ordinal;
        Ok(())
    }

    pub fn present_committed_exit(
        &mut self,
        content: &CoreDevelopmentCaldus,
        committed: &StoredCaldusVictoryExit,
    ) -> Result<CaldusExitPresentationCommit, CaldusInstancePresentationError> {
        if committed.instance_lineage_id != self.instance_lineage_id
            || committed.attempt_ordinal != self.attempt_ordinal
            || committed.exit_instance_id == [0; 16]
            || committed.owners.is_empty()
        {
            return Err(CaldusInstancePresentationError::CommittedExitBindingMismatch);
        }
        let exit_record = content.exit();
        let asset_id = exit_record
            .header
            .asset_ids
            .first()
            .ok_or(CaldusInstancePresentationError::ContentMismatch)?;
        if exit_record.header.id.as_str() != CALDUS_EXIT_ID
            || exit_record.header.asset_ids.len() != 1
            || asset_id.as_str() != CALDUS_EXIT_ASSET_ID
            || exit_record.destination_content_id.as_str() != CALDUS_EXIT_DESTINATION_ID
            || exit_record.arrival != CoreCaldusSafeArrival::HallDefault
            || !exit_record.requires_committed_extraction_receipt
        {
            return Err(CaldusInstancePresentationError::ContentMismatch);
        }
        let display_name = content
            .localized(exit_record.header.localization_name_key.as_str())
            .ok_or(CaldusInstancePresentationError::ContentMismatch)?;
        let description = content
            .localized(exit_record.header.localization_description_key.as_str())
            .ok_or(CaldusInstancePresentationError::ContentMismatch)?;
        let presentation = CaldusExitPresentation {
            exit_instance_id: committed.exit_instance_id,
            content_id: exit_record.header.id.as_str().to_owned(),
            asset_id: asset_id.as_str().to_owned(),
            display_name: display_name.to_owned(),
            description: description.to_owned(),
            tags: exit_record.header.tags.clone(),
            point: exit_record.point,
            destination_content_id: exit_record.destination_content_id.as_str().to_owned(),
            arrival: exit_record.arrival,
            requires_committed_extraction_receipt: true,
        };
        match &self.exit {
            None => {
                self.exit = Some(presentation);
                Ok(CaldusExitPresentationCommit::Fresh)
            }
            Some(existing) if existing == &presentation => Ok(CaldusExitPresentationCommit::Replay),
            Some(_) => Err(CaldusInstancePresentationError::PresentationConflict),
        }
    }
}

fn validate_attempt(
    instance_lineage_id: [u8; 16],
    attempt_ordinal: u32,
) -> Result<(), CaldusInstancePresentationError> {
    if instance_lineage_id == [0; 16] || attempt_ordinal == 0 {
        return Err(CaldusInstancePresentationError::InvalidAttempt);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CaldusInstancePresentationError {
    #[error("Caldus presentation requires a nonzero lineage and attempt")]
    InvalidAttempt,
    #[error("Caldus presentation reset must advance the attempt ordinal")]
    AttemptDidNotAdvance,
    #[error("a committed Caldus exit cannot be hidden by an encounter reset")]
    CommittedExitCannotReset,
    #[error("durable Caldus exit does not bind the presented lineage and attempt")]
    CommittedExitBindingMismatch,
    #[error("validated Caldus exit content is unavailable or inconsistent")]
    ContentMismatch,
    #[error("Caldus exit presentation identity was replayed with different material")]
    PresentationConflict,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use persistence::StoredCaldusVictoryOwner;
    use sim_core::{
        CoreBossParticipant, CoreBossParticipantLock, CoreCaldusVictoryIdentities, EntityId,
    };

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn lock(attempt_ordinal: u32) -> CoreBossParticipantLock {
        CoreBossParticipantLock {
            attempt_ordinal,
            participants: vec![CoreBossParticipant {
                entity_id: EntityId::new(41).unwrap(),
                party_slot: 0,
            }],
            maximum_health: 7_200,
        }
    }

    fn committed_exit(lineage_id: [u8; 16], attempt_ordinal: u32) -> StoredCaldusVictoryExit {
        let identities =
            CoreCaldusVictoryIdentities::derive(lineage_id, &lock(attempt_ordinal)).unwrap();
        StoredCaldusVictoryExit {
            replayed: false,
            encounter_id: identities.encounter_id.bytes(),
            instance_lineage_id: lineage_id,
            attempt_ordinal,
            exit_instance_id: identities.exit_instance_id.bytes(),
            canonical_request_hash: [9; 32],
            owners: vec![StoredCaldusVictoryOwner {
                party_slot: 0,
                participant_entity_id: 41,
                account_id: [2; 16],
                character_id: [3; 16],
                reward_request_id: identities
                    .reward_for(lock(attempt_ordinal).participants[0])
                    .unwrap()
                    .bytes(),
                reward_result_hash: [4; 32],
                progression_payload_hash: [5; 32],
            }],
        }
    }

    #[test]
    fn exit_is_hidden_until_matching_durable_commit_then_replays_exactly() {
        let content = sim_content::load_core_development_caldus(&content_root()).unwrap();
        let lineage_id = [7; 16];
        let committed = committed_exit(lineage_id, 1);
        let mut presentation = CaldusInstancePresentation::new(lineage_id, 1).unwrap();
        assert_eq!(presentation.exit(), None);
        assert_eq!(
            presentation.present_committed_exit(&content, &committed),
            Ok(CaldusExitPresentationCommit::Fresh)
        );
        let exit = presentation.exit().unwrap();
        assert_eq!(exit.exit_instance_id, committed.exit_instance_id);
        assert_eq!(exit.content_id, CALDUS_EXIT_ID);
        assert_eq!(exit.asset_id, CALDUS_EXIT_ASSET_ID);
        assert_eq!(exit.point, MilliTilePoint { x: 2_500, y: 9_000 });
        assert_eq!(
            exit.tags,
            [
                "portal",
                "dungeon_exit",
                "successful_extraction",
                "requires_committed_boss_reward",
            ]
        );
        assert!(exit.requires_committed_extraction_receipt);
        assert_eq!(
            presentation.present_committed_exit(&content, &committed),
            Ok(CaldusExitPresentationCommit::Replay)
        );
        assert_eq!(
            presentation.reset_for_attempt(2),
            Err(CaldusInstancePresentationError::CommittedExitCannotReset)
        );
    }

    #[test]
    fn reset_advances_identity_and_rejects_stale_or_conflicting_commit() {
        let content = sim_content::load_core_development_caldus(&content_root()).unwrap();
        let lineage_id = [8; 16];
        let first = committed_exit(lineage_id, 1);
        let second = committed_exit(lineage_id, 2);
        assert_ne!(first.exit_instance_id, second.exit_instance_id);
        let mut presentation = CaldusInstancePresentation::new(lineage_id, 1).unwrap();
        presentation.reset_for_attempt(2).unwrap();
        assert_eq!(presentation.exit(), None);
        assert_eq!(
            presentation.present_committed_exit(&content, &first),
            Err(CaldusInstancePresentationError::CommittedExitBindingMismatch)
        );
        presentation
            .present_committed_exit(&content, &second)
            .unwrap();
        let mut conflicting = second;
        conflicting.exit_instance_id[0] ^= 1;
        assert_eq!(
            presentation.present_committed_exit(&content, &conflicting),
            Err(CaldusInstancePresentationError::PresentationConflict)
        );
    }
}
