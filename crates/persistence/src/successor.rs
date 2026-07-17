//! Durable successor authority captured by the ordinary final-death transaction.
//!
//! The contract is derived from the canonical Production GDD (`DTH-020`/`021` and
//! `TECH-021`-`023`), Content Production Spec (`CONT-CATALOG-003`), Development Roadmap
//! (`GB-M03-07`), and accepted `SPEC-CONFLICT-031`. It deliberately stores the non-entitlement
//! Core silhouette token rather than reconstructing appearance authority from mutable content or
//! an optional Echo projection.

use serde::Serialize;

use crate::{
    AuthoritativeDeathPlanV1, DurableDeathProvenanceV1, PersistenceError, WIPEABLE_CORE_NAMESPACE,
};

pub const SUCCESSOR_PRESET_REVISION_V1: u16 = 1;
pub const SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE: u16 = 0;
pub const CORE_SUCCESSOR_CLASS_ID: &str = "class.grave_arbalist";
pub const CORE_SUCCESSOR_BASE_SILHOUETTE_ID: &str = "sprite.class.grave_arbalist";
const SUCCESSOR_PRESET_HASH_CONTEXT_V1: &str = "gravebound.successor-preset.v1";

/// Immutable death-time authority required to initialize one successor later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableSuccessorPresetV1 {
    pub namespace_id: String,
    pub account_id: [u8; 16],
    pub former_character_id: [u8; 16],
    pub death_id: [u8; 16],
    pub former_roster_ordinal: u8,
    pub class_id: String,
    pub appearance_kind: u16,
    pub base_silhouette_id: String,
    pub content_revision: String,
    pub created_at_unix_ms: u64,
    pub preset_hash: [u8; 32],
}

impl DurableSuccessorPresetV1 {
    /// Derives the universal preset for an ordinary player-visible death.
    ///
    /// Incident and administrative terminal records remain durable but cannot mint successor
    /// authority. This preserves the provenance split established by migration `0053`.
    pub fn from_death_plan(
        plan: &AuthoritativeDeathPlanV1,
    ) -> Result<Option<Self>, PersistenceError> {
        if plan.event.provenance != DurableDeathProvenanceV1::OrdinaryGameplay {
            return Ok(None);
        }
        let mut preset = Self {
            namespace_id: plan.event.namespace_id.clone(),
            account_id: plan.event.account_id,
            former_character_id: plan.event.character_id,
            death_id: plan.event.death_id,
            former_roster_ordinal: plan.event.former_roster_ordinal,
            class_id: plan.summary.class_id.clone(),
            appearance_kind: SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
            base_silhouette_id: CORE_SUCCESSOR_BASE_SILHOUETTE_ID.to_owned(),
            content_revision: plan.event.content_revision.clone(),
            created_at_unix_ms: plan.event.committed_at_unix_ms,
            preset_hash: [0; 32],
        };
        preset.preset_hash = preset.expected_hash()?;
        preset.validate(plan)?;
        Ok(Some(preset))
    }

    pub fn expected_hash(&self) -> Result<[u8; 32], PersistenceError> {
        #[derive(Serialize)]
        struct Material<'a> {
            preset_revision: u16,
            namespace_id: &'a str,
            account_id: [u8; 16],
            former_character_id: [u8; 16],
            death_id: [u8; 16],
            former_roster_ordinal: u8,
            class_id: &'a str,
            appearance_kind: u16,
            base_silhouette_id: &'a str,
            content_revision: &'a str,
        }

        let bytes = postcard::to_stdvec(&Material {
            preset_revision: SUCCESSOR_PRESET_REVISION_V1,
            namespace_id: &self.namespace_id,
            account_id: self.account_id,
            former_character_id: self.former_character_id,
            death_id: self.death_id,
            former_roster_ordinal: self.former_roster_ordinal,
            class_id: &self.class_id,
            appearance_kind: self.appearance_kind,
            base_silhouette_id: &self.base_silhouette_id,
            content_revision: &self.content_revision,
        })
        .map_err(|_| PersistenceError::CorruptStoredDurableDeath)?;
        Ok(blake3::derive_key(SUCCESSOR_PRESET_HASH_CONTEXT_V1, &bytes))
    }

    fn validate(&self, plan: &AuthoritativeDeathPlanV1) -> Result<(), PersistenceError> {
        if self.namespace_id != WIPEABLE_CORE_NAMESPACE
            || self.account_id == [0; 16]
            || self.former_character_id == [0; 16]
            || self.death_id == [0; 16]
            || !(1..=2).contains(&self.former_roster_ordinal)
            || self.class_id != CORE_SUCCESSOR_CLASS_ID
            || self.appearance_kind != SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE
            || self.base_silhouette_id != CORE_SUCCESSOR_BASE_SILHOUETTE_ID
            || self.created_at_unix_ms == 0
            || self.content_revision != plan.summary.content_revision
            || self.account_id != plan.event.account_id
            || self.former_character_id != plan.event.character_id
            || self.death_id != plan.event.death_id
            || self.former_roster_ordinal != plan.event.former_roster_ordinal
            || self.preset_hash == [0; 32]
            || self.preset_hash != self.expected_hash()?
        {
            return Err(PersistenceError::CorruptStoredDurableDeath);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_hash_is_domain_separated_and_commit_time_independent() {
        let mut preset = DurableSuccessorPresetV1 {
            namespace_id: WIPEABLE_CORE_NAMESPACE.to_owned(),
            account_id: [1; 16],
            former_character_id: [2; 16],
            death_id: [3; 16],
            former_roster_ordinal: 1,
            class_id: CORE_SUCCESSOR_CLASS_ID.to_owned(),
            appearance_kind: SUCCESSOR_APPEARANCE_KIND_CORE_BASE_SILHOUETTE,
            base_silhouette_id: CORE_SUCCESSOR_BASE_SILHOUETTE_ID.to_owned(),
            content_revision: format!("core-dev.blake3.{}", "a".repeat(64)),
            created_at_unix_ms: 10,
            preset_hash: [0; 32],
        };
        let expected = preset.expected_hash().unwrap();
        preset.created_at_unix_ms = 20;
        assert_eq!(preset.expected_hash().unwrap(), expected);
        preset.former_roster_ordinal = 2;
        assert_ne!(preset.expected_hash().unwrap(), expected);
    }
}
