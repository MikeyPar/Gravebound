//! Hostile telegraph/readability compilation for `GB-M01-05B`.
//!
//! `pattern` owns individual geometry and fairness. This module compiles validated patterns into
//! one deterministic encounter manifest, proves the hostile-over-friendly priority stack, assigns
//! grayscale-distinct shape grammar, accounts aggregate active instances/threat, and enforces the
//! GDD rule that repeated-use warning timing is illegal until the full first mechanic completes.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CombatColorFamily, Counterplay, DamageBand, OriginCue, PatternContext, PatternKind, ShapeCue,
    ValidatedPattern,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CombatEffectLayer {
    Decorative,
    Loot,
    FriendlyProjectile,
    HostileProjectile,
    HostileTelegraph,
}

impl CombatEffectLayer {
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::Decorative => 10,
            Self::Loot => 20,
            Self::FriendlyProjectile => 30,
            Self::HostileProjectile => 40,
            Self::HostileTelegraph => 50,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GrayscaleSignature {
    TaperedFanBolt,
    HollowGapRing,
    BandedLane,
    TimelineSequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutlineTreatment {
    StandardHostile,
    ThickWithWhiteCore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WarningAudioPriority {
    Standard,
    MajorOrHigher,
}

impl WarningAudioPriority {
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::Standard => 80,
            Self::MajorOrHigher => 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostileReadabilityProfile {
    pub pattern_id: String,
    pub context: PatternContext,
    pub origin_cue: OriginCue,
    pub shape_cue: ShapeCue,
    pub grayscale_signature: GrayscaleSignature,
    pub color_family: CombatColorFamily,
    pub outline: OutlineTreatment,
    pub counterplay: Counterplay,
    pub damage_band: DamageBand,
    pub first_warning_ticks: u32,
    pub repeated_warning_ticks: u32,
    pub lifetime_ticks: u32,
    pub standard_audio_cue_id: String,
    pub major_audio_cue_id: Option<String>,
    pub audio_priority: WarningAudioPriority,
    pub projectile_layer: CombatEffectLayer,
    pub telegraph_layer: CombatEffectLayer,
    pub threat_cost: u32,
    pub maximum_active_instances: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostileReadabilityManifest {
    profiles: Vec<HostileReadabilityProfile>,
    total_threat_cost: u32,
    total_maximum_active_instances: u32,
    encounter_projectile_cap: u32,
}

impl HostileReadabilityManifest {
    #[must_use]
    pub fn profiles(&self) -> &[HostileReadabilityProfile] {
        &self.profiles
    }

    #[must_use]
    pub const fn total_threat_cost(&self) -> u32 {
        self.total_threat_cost
    }

    #[must_use]
    pub const fn total_maximum_active_instances(&self) -> u32 {
        self.total_maximum_active_instances
    }

    #[must_use]
    pub const fn encounter_projectile_cap(&self) -> u32 {
        self.encounter_projectile_cap
    }

    #[must_use]
    pub fn profile(&self, pattern_id: &str) -> Option<&HostileReadabilityProfile> {
        self.profiles
            .binary_search_by_key(&pattern_id, |profile| profile.pattern_id.as_str())
            .ok()
            .map(|index| &self.profiles[index])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadabilityDiagnostic {
    EmptyManifest,
    MixedEncounterContexts,
    DuplicatePatternId {
        pattern_id: String,
    },
    AggregateInstanceOverflow,
    AggregateThreatOverflow,
    AggregateProjectileCapExceeded {
        cap: u32,
        actual: u32,
    },
    GrayscaleSignatureCollision {
        left_shape: ShapeCue,
        right_shape: ShapeCue,
    },
    InvalidPriorityStack,
}

pub fn compile_hostile_readability_manifest(
    patterns: &[ValidatedPattern],
) -> Result<HostileReadabilityManifest, Vec<ReadabilityDiagnostic>> {
    let mut diagnostics = Vec::new();
    if patterns.is_empty() {
        diagnostics.push(ReadabilityDiagnostic::EmptyManifest);
    }
    let contexts: BTreeSet<_> = patterns
        .iter()
        .map(|pattern| pattern.definition().context)
        .collect();
    if contexts.len() > 1 {
        diagnostics.push(ReadabilityDiagnostic::MixedEncounterContexts);
    }
    if !canonical_priority_stack_is_valid() {
        diagnostics.push(ReadabilityDiagnostic::InvalidPriorityStack);
    }

    let mut profiles: Vec<_> = patterns.iter().map(compile_profile).collect();
    profiles.sort_by(|left, right| left.pattern_id.cmp(&right.pattern_id));
    for pair in profiles.windows(2) {
        if pair[0].pattern_id == pair[1].pattern_id {
            diagnostics.push(ReadabilityDiagnostic::DuplicatePatternId {
                pattern_id: pair[0].pattern_id.clone(),
            });
        }
    }
    validate_grayscale_shape_grammar(&profiles, &mut diagnostics);

    let total_maximum_active_instances = profiles.iter().try_fold(0_u32, |sum, profile| {
        sum.checked_add(profile.maximum_active_instances)
    });
    let total_threat_cost = profiles
        .iter()
        .try_fold(0_u32, |sum, profile| sum.checked_add(profile.threat_cost));
    let encounter_projectile_cap = contexts
        .first()
        .copied()
        .unwrap_or(PatternContext::Normal)
        .projectile_cap();
    let Some(total_maximum_active_instances) = total_maximum_active_instances else {
        diagnostics.push(ReadabilityDiagnostic::AggregateInstanceOverflow);
        return Err(sorted_diagnostics(diagnostics));
    };
    let Some(total_threat_cost) = total_threat_cost else {
        diagnostics.push(ReadabilityDiagnostic::AggregateThreatOverflow);
        return Err(sorted_diagnostics(diagnostics));
    };
    if total_maximum_active_instances > encounter_projectile_cap {
        diagnostics.push(ReadabilityDiagnostic::AggregateProjectileCapExceeded {
            cap: encounter_projectile_cap,
            actual: total_maximum_active_instances,
        });
    }

    if diagnostics.is_empty() {
        Ok(HostileReadabilityManifest {
            profiles,
            total_threat_cost,
            total_maximum_active_instances,
            encounter_projectile_cap,
        })
    } else {
        Err(sorted_diagnostics(diagnostics))
    }
}

fn compile_profile(pattern: &ValidatedPattern) -> HostileReadabilityProfile {
    let definition = pattern.definition();
    HostileReadabilityProfile {
        pattern_id: definition.pattern_id.clone(),
        context: definition.context,
        origin_cue: definition.origin_cue,
        shape_cue: definition.shape_cue,
        grayscale_signature: grayscale_signature(&definition.kind),
        color_family: definition.color_family,
        outline: outline_treatment(definition.damage_band),
        counterplay: definition.counterplay,
        damage_band: definition.damage_band,
        first_warning_ticks: pattern.first_warning_ticks(),
        repeated_warning_ticks: pattern.repeated_warning_ticks(),
        lifetime_ticks: definition.lifetime_ticks,
        standard_audio_cue_id: definition.audio_cue_id.clone(),
        major_audio_cue_id: definition.major_audio_cue_id.clone(),
        audio_priority: audio_priority(definition.damage_band),
        projectile_layer: CombatEffectLayer::HostileProjectile,
        telegraph_layer: CombatEffectLayer::HostileTelegraph,
        threat_cost: definition.threat_cost,
        maximum_active_instances: definition.maximum_active_instances,
    }
}

fn validate_grayscale_shape_grammar(
    profiles: &[HostileReadabilityProfile],
    diagnostics: &mut Vec<ReadabilityDiagnostic>,
) {
    let mut by_signature = BTreeMap::new();
    for profile in profiles {
        if let Some(previous_shape) =
            by_signature.insert(profile.grayscale_signature, profile.shape_cue)
            && previous_shape != profile.shape_cue
        {
            diagnostics.push(ReadabilityDiagnostic::GrayscaleSignatureCollision {
                left_shape: previous_shape,
                right_shape: profile.shape_cue,
            });
        }
    }
}

#[must_use]
pub const fn canonical_priority_stack_is_valid() -> bool {
    CombatEffectLayer::HostileTelegraph.priority() > CombatEffectLayer::HostileProjectile.priority()
        && CombatEffectLayer::HostileProjectile.priority()
            > CombatEffectLayer::FriendlyProjectile.priority()
        && CombatEffectLayer::FriendlyProjectile.priority() > CombatEffectLayer::Loot.priority()
        && CombatEffectLayer::Loot.priority() > CombatEffectLayer::Decorative.priority()
        && WarningAudioPriority::MajorOrHigher.priority()
            > WarningAudioPriority::Standard.priority()
}

const fn grayscale_signature(kind: &PatternKind) -> GrayscaleSignature {
    match kind {
        PatternKind::Fan { .. } => GrayscaleSignature::TaperedFanBolt,
        PatternKind::RingWithGap { .. } => GrayscaleSignature::HollowGapRing,
        PatternKind::TelegraphedLane { .. } => GrayscaleSignature::BandedLane,
        PatternKind::FixedTimeline { .. } => GrayscaleSignature::TimelineSequence,
    }
}

const fn outline_treatment(band: DamageBand) -> OutlineTreatment {
    match band {
        DamageBand::Chip | DamageBand::Pressure => OutlineTreatment::StandardHostile,
        DamageBand::Major | DamageBand::Severe | DamageBand::Execution => {
            OutlineTreatment::ThickWithWhiteCore
        }
    }
}

const fn audio_priority(band: DamageBand) -> WarningAudioPriority {
    match band {
        DamageBand::Chip | DamageBand::Pressure => WarningAudioPriority::Standard,
        DamageBand::Major | DamageBand::Severe | DamageBand::Execution => {
            WarningAudioPriority::MajorOrHigher
        }
    }
}

fn sorted_diagnostics(mut diagnostics: Vec<ReadabilityDiagnostic>) -> Vec<ReadabilityDiagnostic> {
    diagnostics.sort();
    diagnostics.dedup();
    diagnostics
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegraphUse {
    First,
    Repeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegraphExposureEvent {
    TelegraphStarted {
        use_kind: TelegraphUse,
        warning_ticks: u32,
    },
    MechanicResolved,
    MechanicCompleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegraphExposureState {
    Unseen,
    FirstTelegraphActive,
    FirstMechanicResolved,
    ReadyForRepeat,
    RepeatedTelegraphActive,
    RepeatedMechanicResolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegraphExposureTracker {
    profiles: BTreeMap<String, (u32, u32)>,
    states: BTreeMap<String, TelegraphExposureState>,
}

impl TelegraphExposureTracker {
    #[must_use]
    pub fn new(manifest: &HostileReadabilityManifest) -> Self {
        let profiles = manifest
            .profiles()
            .iter()
            .map(|profile| {
                (
                    profile.pattern_id.clone(),
                    (profile.first_warning_ticks, profile.repeated_warning_ticks),
                )
            })
            .collect();
        let states = manifest
            .profiles()
            .iter()
            .map(|profile| (profile.pattern_id.clone(), TelegraphExposureState::Unseen))
            .collect();
        Self { profiles, states }
    }

    pub fn apply(
        &mut self,
        pattern_id: &str,
        event: TelegraphExposureEvent,
    ) -> Result<TelegraphExposureState, TelegraphExposureError> {
        let &(first_warning, repeated_warning) = self
            .profiles
            .get(pattern_id)
            .ok_or(TelegraphExposureError::UnknownPattern)?;
        let state = self
            .states
            .get_mut(pattern_id)
            .ok_or(TelegraphExposureError::UnknownPattern)?;
        let next = match (*state, event) {
            (
                TelegraphExposureState::Unseen,
                TelegraphExposureEvent::TelegraphStarted {
                    use_kind: TelegraphUse::First,
                    warning_ticks,
                },
            ) if warning_ticks == first_warning => TelegraphExposureState::FirstTelegraphActive,
            (
                TelegraphExposureState::FirstTelegraphActive,
                TelegraphExposureEvent::MechanicResolved,
            ) => TelegraphExposureState::FirstMechanicResolved,
            (
                TelegraphExposureState::FirstMechanicResolved
                | TelegraphExposureState::RepeatedMechanicResolved,
                TelegraphExposureEvent::MechanicCompleted,
            ) => TelegraphExposureState::ReadyForRepeat,
            (
                TelegraphExposureState::ReadyForRepeat,
                TelegraphExposureEvent::TelegraphStarted {
                    use_kind: TelegraphUse::Repeated,
                    warning_ticks,
                },
            ) if warning_ticks == repeated_warning => {
                TelegraphExposureState::RepeatedTelegraphActive
            }
            (
                TelegraphExposureState::RepeatedTelegraphActive,
                TelegraphExposureEvent::MechanicResolved,
            ) => TelegraphExposureState::RepeatedMechanicResolved,
            (_, TelegraphExposureEvent::TelegraphStarted { warning_ticks, .. })
                if warning_ticks != first_warning && warning_ticks != repeated_warning =>
            {
                return Err(TelegraphExposureError::WarningTicksMismatch {
                    expected_first: first_warning,
                    expected_repeated: repeated_warning,
                    actual: warning_ticks,
                });
            }
            _ => return Err(TelegraphExposureError::IllegalTransition),
        };
        *state = next;
        Ok(next)
    }

    #[must_use]
    pub fn state(&self, pattern_id: &str) -> Option<TelegraphExposureState> {
        self.states.get(pattern_id).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegraphExposureError {
    UnknownPattern,
    WarningTicksMismatch {
        expected_first: u32,
        expected_repeated: u32,
        actual: u32,
    },
    IllegalTransition,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BellReedDefinition, ChainSentryDefinition, DrownedPilgrimDefinition, PatternDefinition,
        PatternFairnessFixture,
    };

    fn tags(value: &str) -> BTreeSet<String> {
        BTreeSet::from([value.to_owned()])
    }

    fn patterns() -> Vec<ValidatedPattern> {
        let pilgrim = DrownedPilgrimDefinition::first_playable();
        let mut fan = PatternDefinition::from_projectile_attack(
            &pilgrim.parameters().attack,
            PatternKind::Fan {
                projectile_count: 3,
                offsets_degrees: vec![-15, 0, 15],
            },
            PatternContext::Normal,
            300,
            300,
            OriginCue::SourceSilhouette,
            ShapeCue::Fan,
            PatternFairnessFixture::baseline(1_065, 5_000, 0),
        );
        fan.compatibility_tags = tags("fan_projectile");

        let reed = BellReedDefinition::first_playable();
        let mut ring = PatternDefinition::from_projectile_attack(
            &reed.parameters().attack,
            PatternKind::RingWithGap {
                index_count: 8,
                omitted_count: 2,
                omitted_start_advance: 3,
            },
            PatternContext::Normal,
            450,
            300,
            OriginCue::SourceSilhouette,
            ShapeCue::RingGap,
            PatternFairnessFixture::baseline(1_587, 4_000, 0),
        );
        ring.compatibility_tags = tags("radial_projectile");

        let sentry = ChainSentryDefinition::first_playable();
        let mut lane = PatternDefinition::from_lane_attack(
            &sentry.parameters().attack,
            PatternContext::Normal,
            800,
            650,
            PatternFairnessFixture::baseline(800, 2_000, 800),
        );
        lane.compatibility_tags = tags("lane_or_beam");
        vec![
            fan.validate().expect("fan"),
            ring.validate().expect("ring"),
            lane.validate().expect("lane"),
        ]
    }

    #[test]
    fn first_playable_manifest_has_exact_shapes_priority_and_aggregate_budget() {
        let manifest = compile_hostile_readability_manifest(&patterns()).expect("manifest");
        assert_eq!(manifest.total_threat_cost(), 33);
        assert_eq!(manifest.total_maximum_active_instances(), 20);
        assert_eq!(manifest.encounter_projectile_cap(), 300);
        assert!(canonical_priority_stack_is_valid());
        let fan = manifest
            .profile("pattern.enemy.drowned_pilgrim.fan")
            .expect("fan profile");
        assert_eq!(fan.grayscale_signature, GrayscaleSignature::TaperedFanBolt);
        assert_eq!(fan.first_warning_ticks, 9);
        let ring = manifest
            .profile("pattern.enemy.bell_reed.gap_ring")
            .expect("ring profile");
        assert_eq!(ring.grayscale_signature, GrayscaleSignature::HollowGapRing);
        let lane = manifest
            .profile("pattern.enemy.chain_sentry.cross_lanes")
            .expect("lane profile");
        assert_eq!(lane.grayscale_signature, GrayscaleSignature::BandedLane);
        assert_eq!(lane.damage_band, DamageBand::Pressure);
    }

    #[test]
    fn repeated_warning_is_illegal_until_first_mechanic_fully_completes() {
        let manifest = compile_hostile_readability_manifest(&patterns()).expect("manifest");
        let id = "pattern.enemy.bell_reed.gap_ring";
        let mut tracker = TelegraphExposureTracker::new(&manifest);
        assert_eq!(
            tracker.apply(
                id,
                TelegraphExposureEvent::TelegraphStarted {
                    use_kind: TelegraphUse::Repeated,
                    warning_ticks: 9,
                }
            ),
            Err(TelegraphExposureError::IllegalTransition)
        );
        assert_eq!(tracker.state(id), Some(TelegraphExposureState::Unseen));
        assert_eq!(
            tracker
                .apply(
                    id,
                    TelegraphExposureEvent::TelegraphStarted {
                        use_kind: TelegraphUse::First,
                        warning_ticks: 14,
                    }
                )
                .expect("first warning"),
            TelegraphExposureState::FirstTelegraphActive
        );
        assert_eq!(
            tracker
                .apply(id, TelegraphExposureEvent::MechanicResolved)
                .expect("first resolve"),
            TelegraphExposureState::FirstMechanicResolved
        );
        assert_eq!(
            tracker.apply(
                id,
                TelegraphExposureEvent::TelegraphStarted {
                    use_kind: TelegraphUse::Repeated,
                    warning_ticks: 9,
                }
            ),
            Err(TelegraphExposureError::IllegalTransition)
        );
        tracker
            .apply(id, TelegraphExposureEvent::MechanicCompleted)
            .expect("first completion");
        tracker
            .apply(
                id,
                TelegraphExposureEvent::TelegraphStarted {
                    use_kind: TelegraphUse::Repeated,
                    warning_ticks: 9,
                },
            )
            .expect("legal repeat");
    }

    #[test]
    fn exposure_failures_are_transactional_and_typed() {
        let manifest = compile_hostile_readability_manifest(&patterns()).expect("manifest");
        let id = "pattern.enemy.drowned_pilgrim.fan";
        let mut tracker = TelegraphExposureTracker::new(&manifest);
        assert_eq!(
            tracker.apply(
                id,
                TelegraphExposureEvent::TelegraphStarted {
                    use_kind: TelegraphUse::First,
                    warning_ticks: 8,
                }
            ),
            Err(TelegraphExposureError::WarningTicksMismatch {
                expected_first: 9,
                expected_repeated: 9,
                actual: 8,
            })
        );
        assert_eq!(tracker.state(id), Some(TelegraphExposureState::Unseen));
        assert_eq!(
            tracker.apply("pattern.missing", TelegraphExposureEvent::MechanicResolved),
            Err(TelegraphExposureError::UnknownPattern)
        );
    }

    #[test]
    fn duplicate_mixed_context_and_aggregate_caps_fail_deterministically() {
        let mut values = patterns();
        values.push(values[0].clone());
        let duplicate = compile_hostile_readability_manifest(&values).expect_err("duplicate");
        assert!(
            duplicate.contains(&ReadabilityDiagnostic::DuplicatePatternId {
                pattern_id: "pattern.enemy.drowned_pilgrim.fan".to_owned(),
            })
        );

        let mut mixed = patterns();
        let mut mixed_definition = mixed.remove(0).into_definition();
        mixed_definition.context = PatternContext::Boss;
        mixed.push(mixed_definition.validate().expect("boss-context fan"));
        assert!(
            compile_hostile_readability_manifest(&mixed)
                .expect_err("mixed")
                .contains(&ReadabilityDiagnostic::MixedEncounterContexts)
        );

        let mut over_cap = patterns();
        let mut over_cap_definition = over_cap.remove(0).into_definition();
        over_cap_definition.maximum_active_instances = 290;
        over_cap.push(
            over_cap_definition
                .validate()
                .expect("individually legal cap"),
        );
        let diagnostics = compile_hostile_readability_manifest(&over_cap).expect_err("cap");
        assert_eq!(
            diagnostics,
            vec![ReadabilityDiagnostic::AggregateProjectileCapExceeded {
                cap: 300,
                actual: 304,
            }]
        );
    }

    #[test]
    fn major_profile_has_white_core_and_priority_audio() {
        let mut values = patterns();
        let mut definition = values.remove(0).into_definition();
        definition.damage_band = DamageBand::Major;
        definition.major_audio_cue_id = Some(format!("{}.major", definition.audio_cue_id));
        definition.first_warning_ms = 650;
        definition.repeated_warning_ms = 500;
        let revalidated = definition.validate().expect("major");
        let manifest = compile_hostile_readability_manifest(&[revalidated]).expect("manifest");
        let profile = &manifest.profiles()[0];
        assert_eq!(profile.outline, OutlineTreatment::ThickWithWhiteCore);
        assert_eq!(profile.audio_priority, WarningAudioPriority::MajorOrHigher);
        assert!(profile.major_audio_cue_id.is_some());
    }
}
