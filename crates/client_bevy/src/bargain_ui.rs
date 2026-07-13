//! Native, accessible three-cell Veil Bargain shrine model.

use anyhow::{Context, Result};
use protocol::{
    BargainContentRevisionV1, BargainDecision, BargainDecisionFrame, BargainDecisionPayload,
    BargainDecisionResult, BargainOfferCell, BargainOfferState, BargainProjection,
    BargainResultCode, BargainStatComparison, BargainViewFrame, BargainViewResult, WireText,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BargainUiAction {
    Cell(usize),
    Refuse,
    Confirm,
    Cancel,
}

#[derive(Debug, Clone)]
pub(crate) struct BargainUiCopy {
    pub(crate) revision: BargainContentRevisionV1,
    names: [String; 3],
    descriptions: [String; 3],
}

impl BargainUiCopy {
    pub(crate) fn from_catalog(catalog: &sim_content::CompiledOathBargainCatalog) -> Result<Self> {
        let localized = |key| {
            catalog
                .localized(key)
                .with_context(|| format!("compiled Bargain catalog is missing {key}"))
                .map(str::to_owned)
        };
        let hashes = catalog.hashes();
        Ok(Self {
            revision: BargainContentRevisionV1 {
                records_blake3: protocol::ManifestHash::new(hashes.records_blake3.clone())?,
                assets_blake3: protocol::ManifestHash::new(hashes.assets_blake3.clone())?,
                localization_blake3: protocol::ManifestHash::new(
                    hashes.localization_blake3.clone(),
                )?,
            },
            names: [
                localized("bargain.bell_debt.name")?,
                localized("bargain.cinder_hunger.name")?,
                localized("bargain.lantern_ash.name")?,
            ],
            descriptions: [
                localized("bargain.bell_debt.description")?,
                localized("bargain.cinder_hunger.description")?,
                localized("bargain.lantern_ash.description")?,
            ],
        })
    }

    fn index(id: &str) -> Option<usize> {
        match id {
            protocol::BELL_DEBT_ID => Some(0),
            protocol::CINDER_HUNGER_ID => Some(1),
            protocol::LANTERN_ASH_ID => Some(2),
            _ => None,
        }
    }

    fn name(&self, id: &str) -> &str {
        Self::index(id).map_or("Unknown Bargain", |index| &self.names[index])
    }

    fn description(&self, id: &str) -> &str {
        Self::index(id).map_or("Authoritative Bargain copy unavailable.", |index| {
            &self.descriptions[index]
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingDecision {
    Select(String),
    Refuse,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BargainUiModel {
    requested_character_id: Option<[u8; 16]>,
    next_sequence: u32,
    projection: Option<BargainProjection>,
    error: Option<BargainResultCode>,
    pending: Option<PendingDecision>,
    mutation_in_flight: bool,
}

impl BargainUiModel {
    pub(crate) fn request_for_selected(
        &mut self,
        selected_character_id: Option<[u8; 16]>,
        revision: BargainContentRevisionV1,
    ) -> Option<BargainViewFrame> {
        if selected_character_id == self.requested_character_id {
            return None;
        }
        self.requested_character_id = selected_character_id;
        self.projection = None;
        self.error = None;
        self.pending = None;
        self.mutation_in_flight = false;
        let character_id = selected_character_id?;
        let sequence = self.next_sequence.max(1);
        self.next_sequence = sequence.checked_add(1)?;
        Some(BargainViewFrame {
            sequence,
            character_id,
            content_revision: revision,
        })
    }

    pub(crate) fn request_failed(&mut self) {
        self.requested_character_id = None;
        self.error = Some(BargainResultCode::ServiceUnavailable);
    }

    pub(crate) fn mutation_failed(&mut self) {
        self.mutation_in_flight = false;
        self.error = Some(BargainResultCode::ServiceUnavailable);
    }

    pub(crate) fn apply_view(&mut self, result: BargainViewResult) {
        self.mutation_in_flight = false;
        if matches!(
            result.code,
            BargainResultCode::Available | BargainResultCode::NoOffer
        ) {
            self.projection = result.projection;
            self.error = None;
        } else {
            self.error = Some(result.code);
        }
    }

    pub(crate) fn apply_decision(&mut self, result: BargainDecisionResult) {
        self.mutation_in_flight = false;
        if matches!(
            result.code,
            BargainResultCode::Accepted | BargainResultCode::Refused
        ) {
            self.projection = result.projection;
            self.pending = None;
            self.error = None;
        } else {
            if let Some(projection) = result.projection {
                self.projection = Some(projection);
            }
            self.error = Some(result.code);
        }
    }

    pub(crate) fn choose(&mut self, action: BargainUiAction) {
        if !self.action_available(action) {
            return;
        }
        self.pending = match action {
            BargainUiAction::Cell(index) => self
                .open_cells()
                .and_then(|cells| cells.get(index))
                .and_then(|cell| match cell {
                    BargainOfferCell::Available { bargain_id, .. } => {
                        Some(PendingDecision::Select(bargain_id.as_str().into()))
                    }
                    BargainOfferCell::Unavailable => None,
                }),
            BargainUiAction::Refuse => Some(PendingDecision::Refuse),
            BargainUiAction::Confirm | BargainUiAction::Cancel => self.pending.clone(),
        };
        self.error = None;
    }

    /// Cancels local review only. It never fabricates the explicit Refuse mutation.
    pub(crate) fn cancel(&mut self) {
        if !self.mutation_in_flight {
            self.pending = None;
            self.error = None;
        }
    }

    pub(crate) fn confirm(
        &mut self,
        mutation_id: [u8; 16],
        issued_at_unix_millis: u64,
        revision: BargainContentRevisionV1,
    ) -> Option<BargainDecisionFrame> {
        if !self.action_available(BargainUiAction::Confirm) {
            return None;
        }
        let projection = self.projection.as_ref()?;
        let offer_id = projection.offer.as_ref()?.offer_id;
        let decision = match self.pending.as_ref()? {
            PendingDecision::Select(id) => BargainDecision::Select {
                bargain_id: WireText::new(id.clone()).ok()?,
            },
            PendingDecision::Refuse => BargainDecision::Refuse,
        };
        let payload = BargainDecisionPayload {
            character_id: projection.character_id,
            offer_id,
            decision,
            content_revision: revision,
            confirmed: true,
        };
        let frame = BargainDecisionFrame {
            mutation_id,
            expected_oath_bargain_version: projection.oath_bargain_version,
            payload_hash: payload.canonical_hash(),
            issued_at_unix_millis,
            payload,
        };
        self.mutation_in_flight = true;
        Some(frame)
    }

    pub(crate) fn action_available(&self, action: BargainUiAction) -> bool {
        if self.mutation_in_flight {
            return false;
        }
        let cells = self.open_cells();
        match action {
            BargainUiAction::Cell(index) => {
                self.pending.is_none()
                    && cells
                        .and_then(|values| values.get(index))
                        .is_some_and(|cell| matches!(cell, BargainOfferCell::Available { .. }))
            }
            BargainUiAction::Refuse => self.pending.is_none() && cells.is_some(),
            BargainUiAction::Confirm | BargainUiAction::Cancel => self.pending.is_some(),
        }
    }

    /// The shrine owns shared choice and confirmation inputs while an offer is open or reviewed.
    pub(crate) fn captures_input(&self) -> bool {
        self.open_cells().is_some() || self.pending.is_some() || self.mutation_in_flight
    }

    fn open_cells(&self) -> Option<&[BargainOfferCell]> {
        let offer = self.projection.as_ref()?.offer.as_ref()?;
        matches!(offer.state, BargainOfferState::Open).then_some(offer.cells.as_slice())
    }

    pub(crate) fn render(&self, copy: &BargainUiCopy) -> String {
        if let Some(error) = self.error {
            return format!(
                "VEIL BARGAIN - {}\nNo Bargain decision was committed.",
                error_label(error)
            );
        }
        let Some(projection) = &self.projection else {
            return if self.requested_character_id.is_some() {
                "VEIL BARGAIN\nLoading authoritative shrine state...".into()
            } else {
                "VEIL BARGAIN\nSelect a living character to inspect Bargains.".into()
            };
        };
        if let Some(pending) = &self.pending {
            return match pending {
                PendingDecision::Select(id) => format!(
                    "CONFIRM VEIL BARGAIN\n{}\n{}\n\nThis boon and curse persist for this character's life.\nChoose CONFIRM or CANCEL.",
                    copy.name(id),
                    copy.description(id)
                ),
                PendingDecision::Refuse => "CONFIRM REFUSAL\nTake no Bargain. The earned slot stays unfilled.\nChoose CONFIRM or CANCEL.".into(),
            };
        }
        let active = if projection.active_bargain_ids.is_empty() {
            "None".into()
        } else {
            projection
                .active_bargain_ids
                .iter()
                .map(|id| copy.name(id.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let Some(offer) = &projection.offer else {
            return format!(
                "VEIL BARGAINS - {}/{} SLOTS\nActive: {active}\n\nNo offer is available at this shrine.",
                projection.active_bargain_ids.len(),
                projection.earned_bargain_slots
            );
        };
        let cells = offer
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| render_cell(index, cell, copy))
            .collect::<Vec<_>>()
            .join("\n\n");
        format!(
            "VEIL BARGAIN - CHOOSE ONE OR REFUSE\nActive: {active}\n\n{cells}\n\n[REFUSE ALL] leaves the earned slot unfilled."
        )
    }
}

fn render_cell(index: usize, cell: &BargainOfferCell, copy: &BargainUiCopy) -> String {
    match cell {
        BargainOfferCell::Unavailable => format!("{}. UNAVAILABLE - CONTENT DISABLED", index + 1),
        BargainOfferCell::Available {
            bargain_id,
            comparison,
        } => format!(
            "{}. {}\n{}\n{}",
            index + 1,
            copy.name(bargain_id.as_str()),
            copy.description(bargain_id.as_str()),
            render_comparison(*comparison)
        ),
    }
}

fn render_comparison(value: BargainStatComparison) -> String {
    format!(
        "Health {} -> {} | Damage {} -> {} | Cooldown {} -> {} | Move {} -> {}\nHealing {} -> {} | Attack rate {} -> {} | Belt slots {} -> {}",
        value.max_health_before_basis_points,
        value.max_health_after_basis_points,
        value.direct_damage_before_basis_points,
        value.direct_damage_after_basis_points,
        value.cooldown_before_basis_points,
        value.cooldown_after_basis_points,
        value.movement_before_basis_points,
        value.movement_after_basis_points,
        value.healing_before_basis_points,
        value.healing_after_basis_points,
        value.attack_rate_before_basis_points,
        value.attack_rate_after_basis_points,
        value.active_belt_slots_before,
        value.active_belt_slots_after,
    )
}

const fn error_label(code: BargainResultCode) -> &'static str {
    match code {
        BargainResultCode::CharacterDead => "CHARACTER MEMORIALIZED",
        BargainResultCode::LocationRequired => "BOUND SHRINE REQUIRED",
        BargainResultCode::ContentMismatch => "CONTENT UPDATE REQUIRED",
        BargainResultCode::StateVersionMismatch => "BARGAIN STATE CHANGED",
        BargainResultCode::OfferResolved => "OFFER ALREADY RESOLVED",
        BargainResultCode::CandidateUnavailable => "CHOICE NO LONGER AVAILABLE",
        BargainResultCode::CharacterNotOwned | BargainResultCode::CharacterNotSelected => {
            "CHARACTER SELECTION MISMATCH"
        }
        _ => "BARGAIN SERVICE UNAVAILABLE",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use protocol::{BELL_DEBT_ID, BargainOfferProjection};

    use super::*;

    fn copy() -> BargainUiCopy {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content");
        let catalog = sim_content::load_core_development_oaths_bargains(&root).unwrap();
        BargainUiCopy::from_catalog(&catalog).unwrap()
    }

    fn comparison() -> BargainStatComparison {
        BargainStatComparison {
            max_health_before_basis_points: 10_000,
            max_health_after_basis_points: 10_000,
            direct_damage_before_basis_points: 10_000,
            direct_damage_after_basis_points: 10_000,
            cooldown_before_basis_points: 10_000,
            cooldown_after_basis_points: 10_000,
            movement_before_basis_points: 10_000,
            movement_after_basis_points: 10_000,
            healing_before_basis_points: 10_000,
            healing_after_basis_points: 10_000,
            attack_rate_before_basis_points: 10_000,
            attack_rate_after_basis_points: 8_500,
            active_belt_slots_before: 2,
            active_belt_slots_after: 2,
        }
    }

    fn projection() -> BargainProjection {
        BargainProjection {
            character_id: [2; 16],
            oath_bargain_version: 2,
            earned_bargain_slots: 1,
            active_bargain_ids: Vec::new(),
            offer: Some(BargainOfferProjection {
                offer_id: [3; 16],
                state: BargainOfferState::Open,
                cells: vec![
                    BargainOfferCell::Available {
                        bargain_id: WireText::new(BELL_DEBT_ID).unwrap(),
                        comparison: comparison(),
                    },
                    BargainOfferCell::Unavailable,
                    BargainOfferCell::Unavailable,
                ],
            }),
        }
    }

    #[test]
    fn three_cells_refusal_confirmation_and_escape_semantics_are_explicit() {
        let copy = copy();
        let mut model = BargainUiModel::default();
        model.apply_view(BargainViewResult {
            sequence: 1,
            code: BargainResultCode::Available,
            projection: Some(projection()),
        });
        let rendered = model.render(&copy);
        assert!(rendered.contains("1. Bell Debt"));
        assert!(rendered.contains("2. UNAVAILABLE"));
        assert!(rendered.contains("Attack rate 10000 -> 8500"));
        assert!(model.captures_input());
        model.choose(BargainUiAction::Refuse);
        assert!(model.render(&copy).contains("CONFIRM REFUSAL"));
        model.cancel();
        assert!(model.action_available(BargainUiAction::Refuse));
    }

    #[test]
    fn confirmed_selection_locks_input_until_authoritative_result() {
        let copy = copy();
        let mut model = BargainUiModel::default();
        model.apply_view(BargainViewResult {
            sequence: 1,
            code: BargainResultCode::Available,
            projection: Some(projection()),
        });
        model.choose(BargainUiAction::Cell(0));
        let frame = model
            .confirm([4; 16], 1_000, copy.revision.clone())
            .unwrap();
        assert!(matches!(
            frame.payload.decision,
            BargainDecision::Select { .. }
        ));
        assert!(!model.action_available(BargainUiAction::Cancel));
    }
}
