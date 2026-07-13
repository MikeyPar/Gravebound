//! Native, accessible initial-Oath confirmation model for the Core identity surface.

use anyhow::{Context, Result};
use protocol::{
    InitialOathSelectionFrame, InitialOathSelectionPayload, InitialOathSelectionResult,
    LONG_VIGIL_ID, NAILKEEPER_ID, OathContentRevisionV1, OathProjection, OathResultCode,
    OathSelectionState, OathViewFrame, OathViewResult, WireText,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OathUiAction {
    LongVigil,
    Nailkeeper,
    Confirm,
    Cancel,
}

impl OathUiAction {
    const fn oath_id(self) -> Option<&'static str> {
        match self {
            Self::LongVigil => Some(LONG_VIGIL_ID),
            Self::Nailkeeper => Some(NAILKEEPER_ID),
            Self::Confirm | Self::Cancel => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OathUiCopy {
    pub(crate) revision: OathContentRevisionV1,
    long_vigil_name: String,
    long_vigil_description: String,
    nailkeeper_name: String,
    nailkeeper_description: String,
    initial_warning: String,
}

impl OathUiCopy {
    pub(crate) fn from_catalog(catalog: &sim_content::CompiledOathBargainCatalog) -> Result<Self> {
        let localized = |key| {
            catalog
                .localized(key)
                .with_context(|| format!("compiled Oath catalog is missing localization key {key}"))
                .map(str::to_owned)
        };
        let hashes = catalog.hashes();
        Ok(Self {
            revision: OathContentRevisionV1 {
                records_blake3: protocol::ManifestHash::new(hashes.records_blake3.clone())?,
                assets_blake3: protocol::ManifestHash::new(hashes.assets_blake3.clone())?,
                localization_blake3: protocol::ManifestHash::new(
                    hashes.localization_blake3.clone(),
                )?,
            },
            long_vigil_name: localized("oath.arbalist.long_vigil.name")?,
            long_vigil_description: localized("oath.arbalist.long_vigil.description")?,
            nailkeeper_name: localized("oath.arbalist.nailkeeper.name")?,
            nailkeeper_description: localized("oath.arbalist.nailkeeper.description")?,
            initial_warning: localized("ui.oath.initial_warning")?,
        })
    }

    fn name(&self, oath_id: &str) -> &str {
        match oath_id {
            LONG_VIGIL_ID => &self.long_vigil_name,
            NAILKEEPER_ID => &self.nailkeeper_name,
            _ => "Unknown Oath",
        }
    }

    fn description(&self, oath_id: &str) -> &str {
        match oath_id {
            LONG_VIGIL_ID => &self.long_vigil_description,
            NAILKEEPER_ID => &self.nailkeeper_description,
            _ => "The authoritative Oath record is unavailable.",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OathUiModel {
    requested_character_id: Option<[u8; 16]>,
    next_sequence: u32,
    projection: Option<OathProjection>,
    error: Option<OathResultCode>,
    pending_oath_id: Option<&'static str>,
    mutation_in_flight: bool,
}

impl OathUiModel {
    pub(crate) fn request_for_selected(
        &mut self,
        selected_character_id: Option<[u8; 16]>,
        revision: OathContentRevisionV1,
    ) -> Option<OathViewFrame> {
        if selected_character_id == self.requested_character_id {
            return None;
        }
        self.requested_character_id = selected_character_id;
        self.projection = None;
        self.error = None;
        self.pending_oath_id = None;
        self.mutation_in_flight = false;
        let character_id = selected_character_id?;
        let sequence = self.next_sequence.max(1);
        self.next_sequence = sequence.checked_add(1)?;
        Some(OathViewFrame {
            sequence,
            character_id,
            content_revision: revision,
        })
    }

    pub(crate) fn request_failed(&mut self) {
        self.requested_character_id = None;
        self.error = Some(OathResultCode::ServiceUnavailable);
    }

    pub(crate) fn mutation_failed(&mut self) {
        self.mutation_in_flight = false;
        self.error = Some(OathResultCode::ServiceUnavailable);
    }

    pub(crate) fn apply_view(&mut self, result: OathViewResult) {
        self.mutation_in_flight = false;
        if result.code == OathResultCode::Available {
            self.projection = result.projection;
            self.error = None;
        } else {
            self.error = Some(result.code);
        }
    }

    pub(crate) fn apply_selection(&mut self, result: InitialOathSelectionResult) {
        self.mutation_in_flight = false;
        if result.code == OathResultCode::Accepted {
            self.projection = result.projection;
            self.pending_oath_id = None;
            self.error = None;
        } else {
            if let Some(projection) = result.projection {
                self.projection = Some(projection);
            }
            self.error = Some(result.code);
        }
    }

    pub(crate) fn choose(&mut self, action: OathUiAction) {
        if self.action_available(action) {
            self.pending_oath_id = action.oath_id();
            self.error = None;
        }
    }

    pub(crate) fn cancel(&mut self) {
        if !self.mutation_in_flight {
            self.pending_oath_id = None;
            self.error = None;
        }
    }

    pub(crate) fn confirm(
        &mut self,
        mutation_id: [u8; 16],
        issued_at_unix_millis: u64,
        revision: OathContentRevisionV1,
    ) -> Option<InitialOathSelectionFrame> {
        if !self.action_available(OathUiAction::Confirm) {
            return None;
        }
        let projection = self.projection.as_ref()?;
        let oath_id = self.pending_oath_id?;
        let payload = InitialOathSelectionPayload {
            character_id: projection.character_id,
            oath_id: WireText::new(oath_id).ok()?,
            content_revision: revision,
            confirmed: true,
        };
        let frame = InitialOathSelectionFrame {
            mutation_id,
            expected_character_version: projection.character_version,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis,
            payload,
        };
        self.mutation_in_flight = true;
        Some(frame)
    }

    pub(crate) fn action_available(&self, action: OathUiAction) -> bool {
        if self.mutation_in_flight {
            return false;
        }
        let eligible = matches!(
            self.projection.as_ref().map(|value| &value.state),
            Some(OathSelectionState::Eligible {
                current_level: 10..=20
            })
        );
        match action {
            OathUiAction::LongVigil | OathUiAction::Nailkeeper => {
                eligible && self.pending_oath_id.is_none()
            }
            OathUiAction::Confirm => eligible && self.pending_oath_id.is_some(),
            OathUiAction::Cancel => self.pending_oath_id.is_some(),
        }
    }

    pub(crate) fn render(&self, copy: &OathUiCopy) -> String {
        if let Some(error) = self.error {
            return format!(
                "OATH SHRINE - {}\n{}",
                error_label(error),
                "No permanent choice was committed. Retry after the listed condition is resolved."
            );
        }
        let Some(projection) = &self.projection else {
            return if self.requested_character_id.is_some() {
                "OATH SHRINE\nLoading authoritative Oath eligibility...".to_owned()
            } else {
                "OATH SHRINE\nSelect a living character to inspect Oath eligibility.".to_owned()
            };
        };
        match &projection.state {
            OathSelectionState::Locked {
                current_level,
                required_level,
            } => {
                format!("OATH SHRINE - LOCKED\nLevel {current_level} / {required_level} required.")
            }
            OathSelectionState::Eligible { .. } => {
                if let Some(oath_id) = self.pending_oath_id {
                    format!(
                        "PERMANENT OATH CONFIRMATION\n{}\n\n{}\n\n{}\n\nChoose CONFIRM PERMANENT OATH or CANCEL.",
                        copy.name(oath_id),
                        copy.description(oath_id),
                        copy.initial_warning
                    )
                } else {
                    format!(
                        "OATH SHRINE - FIRST SELECTION IS FREE\n\n{}\n{}\n\n{}\n{}\n\nSelect one Oath to review the permanent-life warning before confirmation.",
                        copy.long_vigil_name,
                        copy.long_vigil_description,
                        copy.nailkeeper_name,
                        copy.nailkeeper_description
                    )
                }
            }
            OathSelectionState::Selected { oath_id, .. } => format!(
                "OATH BOUND - {}\n{}\n\nLater changes remain unavailable in this Core stage.",
                copy.name(oath_id.as_str()),
                copy.description(oath_id.as_str())
            ),
        }
    }
}

const fn error_label(code: OathResultCode) -> &'static str {
    match code {
        OathResultCode::LevelRequired => "LEVEL 10 REQUIRED",
        OathResultCode::LocationRequired => "LANTERN HALLS REQUIRED",
        OathResultCode::InventoryNotSafe => "INVENTORY NOT SAFE",
        OathResultCode::UnresolvedMutation => "MUTATION STILL RESOLVING",
        OathResultCode::ContentMismatch => "CONTENT UPDATE REQUIRED",
        OathResultCode::CharacterDead => "CHARACTER MEMORIALIZED",
        OathResultCode::CharacterNotOwned | OathResultCode::CharacterNotSelected => {
            "CHARACTER SELECTION MISMATCH"
        }
        OathResultCode::StateVersionMismatch => "CHARACTER STATE CHANGED",
        OathResultCode::AlreadySelected => "OATH ALREADY SELECTED",
        OathResultCode::StageDisabled => "LATER CHANGES UNAVAILABLE",
        OathResultCode::IdempotencyConflict
        | OathResultCode::PayloadHashMismatch
        | OathResultCode::IllegalOath
        | OathResultCode::IssuedAtInvalid
        | OathResultCode::ContentDisabled
        | OathResultCode::ServiceUnavailable
        | OathResultCode::Available
        | OathResultCode::Accepted => "OATH SERVICE UNAVAILABLE",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn copy() -> OathUiCopy {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let catalog = sim_content::load_core_development_oaths_bargains(&root).unwrap();
        OathUiCopy::from_catalog(&catalog).unwrap()
    }

    fn eligible() -> OathProjection {
        OathProjection {
            character_id: [2; 16],
            character_version: 7,
            state: OathSelectionState::Eligible { current_level: 10 },
            later_change_stage_disabled: true,
        }
    }

    #[test]
    fn initial_choice_requires_review_then_explicit_confirmation() {
        let copy = copy();
        let mut model = OathUiModel::default();
        model.apply_view(OathViewResult {
            sequence: 1,
            code: OathResultCode::Available,
            projection: Some(eligible()),
        });
        assert!(!model.action_available(OathUiAction::Confirm));
        model.choose(OathUiAction::LongVigil);
        assert!(model.action_available(OathUiAction::Confirm));
        let rendered = model.render(&copy);
        assert!(rendered.contains("PERMANENT OATH CONFIRMATION"));
        assert!(rendered.contains(&copy.initial_warning));
        let frame = model
            .confirm([3; 16], 1_000, copy.revision.clone())
            .unwrap();
        assert_eq!(frame.payload.oath_id.as_str(), LONG_VIGIL_ID);
        assert!(frame.payload.confirmed);
        assert!(!model.action_available(OathUiAction::Cancel));
    }

    #[test]
    fn cancellation_and_errors_never_fabricate_a_committed_oath() {
        let copy = copy();
        let mut model = OathUiModel::default();
        model.apply_view(OathViewResult {
            sequence: 1,
            code: OathResultCode::Available,
            projection: Some(eligible()),
        });
        model.choose(OathUiAction::Nailkeeper);
        model.cancel();
        assert!(!model.action_available(OathUiAction::Confirm));
        model.apply_selection(InitialOathSelectionResult {
            mutation_id: [4; 16],
            code: OathResultCode::InventoryNotSafe,
            projection: Some(eligible()),
        });
        assert!(model.render(&copy).contains("INVENTORY NOT SAFE"));
        assert!(matches!(
            model.projection.as_ref().map(|value| &value.state),
            Some(OathSelectionState::Eligible { .. })
        ));
    }
}
