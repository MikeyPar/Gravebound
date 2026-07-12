//! Renderer-independent client prediction and network presentation for `GB-M02-03`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bevy::prelude::*;
use protocol::{ENTITY_STATE_ALIVE, EntityKind, EntitySnapshot, SnapshotChunk};
use sim_core::{
    ArenaGeometry, MovementAction, MovementError, PlayerMovementState, SimulationVector, TilePoint,
    tile_point_to_simulation,
};
use thiserror::Error;

use crate::{FrameSet, LoadedArena, arena_view::simulation_point_to_render, player::LocalPlayer};

pub const MAX_PENDING_PREDICTED_INPUTS: usize = 256;
pub const MAX_INCOMPLETE_SNAPSHOTS: usize = 4;
pub const MAX_INTERPOLATION_SAMPLES: usize = 4;
pub const INTERPOLATION_DELAY_TICKS: u64 = 3;
pub const MICRO_CORRECTION_MILLI_TILES: u32 = 100;
pub const SNAP_CORRECTION_MILLI_TILES: u32 = 350;
pub const MICRO_BLEND_MS: u32 = 100;
pub const NOTICEABLE_BLEND_MS: u32 = 60;
pub const UNCONFIRMED_PROJECTILE_LIFETIME_MS: u64 = 250;
const MILLI_TICKS_PER_TICK: u64 = 1_000;
const SERVER_TICKS_PER_SECOND: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteSnapshot {
    pub sequence: u32,
    pub server_tick: u64,
    pub state_version: u64,
    pub acknowledged_input_sequence: u32,
    pub entities: Vec<EntitySnapshot>,
}

#[derive(Debug, Clone)]
struct PendingSnapshot {
    server_tick: u64,
    state_version: u64,
    acknowledged_input_sequence: u32,
    chunk_count: u16,
    chunks: BTreeMap<u16, Vec<EntitySnapshot>>,
}

#[derive(Debug, Default)]
pub struct SnapshotAssembler {
    last_completed_sequence: u32,
    pending: BTreeMap<u32, PendingSnapshot>,
}

impl SnapshotAssembler {
    pub fn push(
        &mut self,
        chunk: SnapshotChunk,
    ) -> Result<Option<CompleteSnapshot>, NetworkPredictionError> {
        chunk
            .validate()
            .map_err(|_| NetworkPredictionError::InvalidSnapshotChunk)?;
        if chunk.sequence <= self.last_completed_sequence {
            return Ok(None);
        }
        if !self.pending.contains_key(&chunk.sequence)
            && self.pending.len() == MAX_INCOMPLETE_SNAPSHOTS
        {
            let oldest = *self
                .pending
                .keys()
                .next()
                .ok_or(NetworkPredictionError::SnapshotAssemblyInvariant)?;
            self.pending.remove(&oldest);
        }
        let pending = self
            .pending
            .entry(chunk.sequence)
            .or_insert_with(|| PendingSnapshot {
                server_tick: chunk.server_tick,
                state_version: chunk.state_version,
                acknowledged_input_sequence: chunk.acknowledged_input_sequence,
                chunk_count: chunk.chunk_count,
                chunks: BTreeMap::new(),
            });
        if pending.server_tick != chunk.server_tick
            || pending.state_version != chunk.state_version
            || pending.acknowledged_input_sequence != chunk.acknowledged_input_sequence
            || pending.chunk_count != chunk.chunk_count
        {
            return Err(NetworkPredictionError::InconsistentSnapshotMetadata);
        }
        if pending.chunks.contains_key(&chunk.chunk_index) {
            return Err(NetworkPredictionError::DuplicateSnapshotChunk {
                sequence: chunk.sequence,
                chunk_index: chunk.chunk_index,
            });
        }
        pending.chunks.insert(chunk.chunk_index, chunk.entities);
        if pending.chunks.len() != usize::from(pending.chunk_count) {
            return Ok(None);
        }

        let pending = self
            .pending
            .remove(&chunk.sequence)
            .ok_or(NetworkPredictionError::SnapshotAssemblyInvariant)?;
        let mut entities = Vec::new();
        let mut entity_ids = BTreeSet::new();
        for index in 0..pending.chunk_count {
            let values = pending
                .chunks
                .get(&index)
                .ok_or(NetworkPredictionError::SnapshotAssemblyInvariant)?;
            for entity in values {
                if !entity_ids.insert(entity.entity_id) {
                    return Err(NetworkPredictionError::DuplicateSnapshotEntity(
                        entity.entity_id,
                    ));
                }
                entities.push(entity.clone());
            }
        }
        self.last_completed_sequence = chunk.sequence;
        self.pending
            .retain(|sequence, _| *sequence > self.last_completed_sequence);
        Ok(Some(CompleteSnapshot {
            sequence: chunk.sequence,
            server_tick: pending.server_tick,
            state_version: pending.state_version,
            acknowledged_input_sequence: pending.acknowledged_input_sequence,
            entities,
        }))
    }

    #[must_use]
    pub const fn last_completed_sequence(&self) -> u32 {
        self.last_completed_sequence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PredictedMovementInput {
    pub sequence: u32,
    pub action: MovementAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionClass {
    MicroBlend,
    NoticeableBlend,
    Snap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionSignal {
    None,
    DebugMetric,
    NetworkWarningAndAnomaly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconciliationEvent {
    pub distance_milli_tiles: u32,
    pub class: CorrectionClass,
    pub blend_duration_ms: u32,
    pub signal: CorrectionSignal,
    pub authoritative_death: bool,
}

#[must_use]
pub const fn classify_correction(distance_milli_tiles: u32) -> ReconciliationEvent {
    if distance_milli_tiles < MICRO_CORRECTION_MILLI_TILES {
        ReconciliationEvent {
            distance_milli_tiles,
            class: CorrectionClass::MicroBlend,
            blend_duration_ms: MICRO_BLEND_MS,
            signal: CorrectionSignal::None,
            authoritative_death: false,
        }
    } else if distance_milli_tiles <= SNAP_CORRECTION_MILLI_TILES {
        ReconciliationEvent {
            distance_milli_tiles,
            class: CorrectionClass::NoticeableBlend,
            blend_duration_ms: NOTICEABLE_BLEND_MS,
            signal: CorrectionSignal::DebugMetric,
            authoritative_death: false,
        }
    } else {
        ReconciliationEvent {
            distance_milli_tiles,
            class: CorrectionClass::Snap,
            blend_duration_ms: 0,
            signal: CorrectionSignal::NetworkWarningAndAnomaly,
            authoritative_death: false,
        }
    }
}

#[derive(Debug, Clone)]
struct LocalMovementPrediction {
    local_entity_id: u64,
    arena: ArenaGeometry,
    state: PlayerMovementState,
    pending: VecDeque<PredictedMovementInput>,
    last_acknowledged_sequence: u32,
    alive: bool,
    presentation_offset: SimulationVector,
    correction_total_ms: u32,
    correction_remaining_ms: u32,
}

impl LocalMovementPrediction {
    fn new(local_entity_id: u64, arena: ArenaGeometry, state: PlayerMovementState) -> Self {
        Self {
            local_entity_id,
            arena,
            state,
            pending: VecDeque::new(),
            last_acknowledged_sequence: 0,
            alive: true,
            presentation_offset: SimulationVector::default(),
            correction_total_ms: 0,
            correction_remaining_ms: 0,
        }
    }

    fn predict(
        &mut self,
        input: PredictedMovementInput,
    ) -> Result<PlayerMovementState, NetworkPredictionError> {
        if !self.alive {
            return Err(NetworkPredictionError::AuthoritativeCharacterDead);
        }
        let last = self
            .pending
            .back()
            .map_or(self.last_acknowledged_sequence, |pending| pending.sequence);
        if input.sequence == 0 || input.sequence <= last {
            return Err(NetworkPredictionError::NonMonotonicPredictedInput {
                received: input.sequence,
                last,
            });
        }
        if self.pending.len() == MAX_PENDING_PREDICTED_INPUTS {
            return Err(NetworkPredictionError::PredictionHistoryFull);
        }
        self.state.step(input.action, &self.arena)?;
        self.pending.push_back(input);
        Ok(self.state)
    }

    fn reconcile(
        &mut self,
        snapshot: &CompleteSnapshot,
    ) -> Result<ReconciliationEvent, NetworkPredictionError> {
        if snapshot.acknowledged_input_sequence < self.last_acknowledged_sequence {
            return Err(NetworkPredictionError::AcknowledgementRegressed {
                received: snapshot.acknowledged_input_sequence,
                last: self.last_acknowledged_sequence,
            });
        }
        let entity = snapshot
            .entities
            .iter()
            .find(|entity| {
                entity.entity_id == self.local_entity_id && entity.kind == EntityKind::Player
            })
            .ok_or(NetworkPredictionError::MissingLocalPlayer)?;
        let prior_position = self.state.position();
        let authoritative_position = snapshot_position(entity);
        let authoritative_velocity = snapshot_velocity(entity);
        let mut replayed = PlayerMovementState::from_authoritative_snapshot(
            authoritative_position,
            authoritative_velocity,
            self.state.config(),
            &self.arena,
        )?;
        self.last_acknowledged_sequence = snapshot.acknowledged_input_sequence;
        self.pending
            .retain(|input| input.sequence > self.last_acknowledged_sequence);
        let authoritative_alive = entity.state_flags & ENTITY_STATE_ALIVE != 0;
        if authoritative_alive {
            for input in &self.pending {
                replayed.step(input.action, &self.arena)?;
            }
        } else {
            self.pending.clear();
        }
        let distance_milli_tiles = distance_milli(prior_position, replayed.position())?;
        let mut correction = classify_correction(distance_milli_tiles);
        correction.authoritative_death = !authoritative_alive;
        self.state = replayed;
        self.alive = authoritative_alive;
        if !authoritative_alive || correction.class == CorrectionClass::Snap {
            self.presentation_offset = SimulationVector::default();
            self.correction_total_ms = 0;
            self.correction_remaining_ms = 0;
        } else {
            self.presentation_offset = prior_position - self.state.position();
            self.correction_total_ms = correction.blend_duration_ms;
            self.correction_remaining_ms = correction.blend_duration_ms;
        }
        Ok(correction)
    }

    fn advance_presentation(&mut self, elapsed_ms: u32) {
        self.correction_remaining_ms = self.correction_remaining_ms.saturating_sub(elapsed_ms);
        if self.correction_remaining_ms == 0 {
            self.presentation_offset = SimulationVector::default();
            self.correction_total_ms = 0;
        }
    }

    fn presentation_position(&self) -> SimulationVector {
        if self.correction_total_ms == 0 {
            return self.state.position();
        }
        let remaining = u16::try_from(self.correction_remaining_ms).unwrap_or(u16::MAX);
        let total = u16::try_from(self.correction_total_ms).unwrap_or(u16::MAX);
        let fraction = f32::from(remaining) / f32::from(total);
        self.state.position() + self.presentation_offset * fraction
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InterpolationSample {
    server_tick: u64,
    entity: EntitySnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpolatedEntity {
    pub entity_id: u64,
    pub kind: EntityKind,
    pub x_milli_tiles: i32,
    pub y_milli_tiles: i32,
    pub velocity_x_milli_tiles_per_second: i32,
    pub velocity_y_milli_tiles_per_second: i32,
    pub state_flags: u32,
}

#[derive(Debug, Default)]
struct RemoteInterpolator {
    tracks: BTreeMap<u64, VecDeque<InterpolationSample>>,
}

impl RemoteInterpolator {
    fn ingest(&mut self, snapshot: &CompleteSnapshot, local_entity_id: u64) {
        for entity in &snapshot.entities {
            if entity.entity_id == local_entity_id {
                continue;
            }
            let samples = self.tracks.entry(entity.entity_id).or_default();
            if samples
                .back()
                .is_some_and(|sample| sample.server_tick >= snapshot.server_tick)
            {
                continue;
            }
            samples.push_back(InterpolationSample {
                server_tick: snapshot.server_tick,
                entity: entity.clone(),
            });
            while samples.len() > MAX_INTERPOLATION_SAMPLES {
                samples.pop_front();
            }
        }
    }

    fn sample(
        &self,
        entity_id: u64,
        estimated_server_tick_millis: u64,
    ) -> Result<Option<InterpolatedEntity>, NetworkPredictionError> {
        let Some(samples) = self.tracks.get(&entity_id) else {
            return Ok(None);
        };
        let target = estimated_server_tick_millis
            .saturating_sub(INTERPOLATION_DELAY_TICKS * MILLI_TICKS_PER_TICK);
        let first = samples
            .front()
            .ok_or(NetworkPredictionError::InterpolationInvariant)?;
        let last = samples
            .back()
            .ok_or(NetworkPredictionError::InterpolationInvariant)?;
        if target <= first.server_tick * MILLI_TICKS_PER_TICK {
            return Ok(Some(interpolated_from_snapshot(&first.entity)));
        }
        if target >= last.server_tick * MILLI_TICKS_PER_TICK {
            return Ok(Some(interpolated_from_snapshot(&last.entity)));
        }
        for pair in samples.as_slices().0.windows(2) {
            let start = pair[0].server_tick * MILLI_TICKS_PER_TICK;
            let end = pair[1].server_tick * MILLI_TICKS_PER_TICK;
            if (start..=end).contains(&target) {
                return Ok(Some(interpolate_entity(
                    &pair[0].entity,
                    &pair[1].entity,
                    target - start,
                    end - start,
                )?));
            }
        }
        let contiguous = samples.iter().collect::<Vec<_>>();
        for pair in contiguous.windows(2) {
            let start = pair[0].server_tick * MILLI_TICKS_PER_TICK;
            let end = pair[1].server_tick * MILLI_TICKS_PER_TICK;
            if (start..=end).contains(&target) {
                return Ok(Some(interpolate_entity(
                    &pair[0].entity,
                    &pair[1].entity,
                    target - start,
                    end - start,
                )?));
            }
        }
        Err(NetworkPredictionError::InterpolationInvariant)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeterministicProjectilePresentation {
    pub source_input_sequence: u32,
    pub source_projectile_ordinal: u16,
    pub authoritative_entity_id: Option<u64>,
    anchor_tick_millis: u64,
    anchor_x_milli_tiles: i32,
    anchor_y_milli_tiles: i32,
    velocity_x_milli_tiles_per_second: i32,
    velocity_y_milli_tiles_per_second: i32,
    expires_unconfirmed_at_millis: u64,
}

impl DeterministicProjectilePresentation {
    pub fn position_at(&self, tick_millis: u64) -> Result<(i32, i32), NetworkPredictionError> {
        let elapsed = tick_millis.saturating_sub(self.anchor_tick_millis);
        Ok((
            advance_milli(
                self.anchor_x_milli_tiles,
                self.velocity_x_milli_tiles_per_second,
                elapsed,
            )?,
            advance_milli(
                self.anchor_y_milli_tiles,
                self.velocity_y_milli_tiles_per_second,
                elapsed,
            )?,
        ))
    }
}

#[derive(Debug, Default)]
struct ProjectilePresentationSet {
    local: BTreeMap<(u32, u16), DeterministicProjectilePresentation>,
}

impl ProjectilePresentationSet {
    fn start_local(
        &mut self,
        source_input_sequence: u32,
        source_projectile_ordinal: u16,
        start_tick_millis: u64,
        origin_milli_tiles: (i32, i32),
        velocity_milli_tiles_per_second: (i32, i32),
    ) -> Result<(), NetworkPredictionError> {
        let source = (source_input_sequence, source_projectile_ordinal);
        if source_input_sequence == 0 || self.local.contains_key(&source) {
            return Err(NetworkPredictionError::InvalidProjectileSourceSequence);
        }
        self.local.insert(
            source,
            DeterministicProjectilePresentation {
                source_input_sequence,
                source_projectile_ordinal,
                authoritative_entity_id: None,
                anchor_tick_millis: start_tick_millis,
                anchor_x_milli_tiles: origin_milli_tiles.0,
                anchor_y_milli_tiles: origin_milli_tiles.1,
                velocity_x_milli_tiles_per_second: velocity_milli_tiles_per_second.0,
                velocity_y_milli_tiles_per_second: velocity_milli_tiles_per_second.1,
                expires_unconfirmed_at_millis: start_tick_millis
                    .saturating_add(UNCONFIRMED_PROJECTILE_LIFETIME_MS),
            },
        );
        Ok(())
    }

    fn ingest(&mut self, snapshot: &CompleteSnapshot) {
        let authoritative = snapshot
            .entities
            .iter()
            .filter(|entity| entity.kind == EntityKind::FriendlyProjectile)
            .map(|entity| (entity.entity_id, entity))
            .collect::<BTreeMap<_, _>>();
        for entity in authoritative.values() {
            if let Some(track) = self.local.get_mut(&(
                entity.source_input_sequence,
                entity.source_projectile_ordinal,
            )) {
                track.authoritative_entity_id = Some(entity.entity_id);
                track.anchor_tick_millis = server_tick_to_millis(snapshot.server_tick);
                track.anchor_x_milli_tiles = entity.x_milli_tiles;
                track.anchor_y_milli_tiles = entity.y_milli_tiles;
                track.velocity_x_milli_tiles_per_second = entity.velocity_x_milli_tiles_per_second;
                track.velocity_y_milli_tiles_per_second = entity.velocity_y_milli_tiles_per_second;
            }
        }
        self.local.retain(|source, track| {
            if let Some(entity_id) = track.authoritative_entity_id {
                authoritative.contains_key(&entity_id)
            } else {
                source.0 > snapshot.acknowledged_input_sequence
            }
        });
    }

    fn active_at(&self, tick_millis: u64) -> Vec<&DeterministicProjectilePresentation> {
        self.local
            .values()
            .filter(|track| {
                track.authoritative_entity_id.is_some()
                    || tick_millis <= track.expires_unconfirmed_at_millis
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct SnapshotApplication {
    pub snapshot: CompleteSnapshot,
    pub correction: ReconciliationEvent,
}

#[derive(Debug)]
pub struct RemoteClientRuntime {
    local_entity_id: u64,
    assembler: SnapshotAssembler,
    local: LocalMovementPrediction,
    remote: RemoteInterpolator,
    projectiles: ProjectilePresentationSet,
}

#[derive(Debug, Resource)]
pub struct NativeNetworkPresentation {
    runtime: RemoteClientRuntime,
    estimated_server_tick_milli: u64,
    presentation_time_ms: u64,
}

impl NativeNetworkPresentation {
    #[must_use]
    pub fn new(runtime: RemoteClientRuntime) -> Self {
        Self {
            runtime,
            estimated_server_tick_milli: 0,
            presentation_time_ms: 0,
        }
    }

    #[must_use]
    pub const fn runtime(&self) -> &RemoteClientRuntime {
        &self.runtime
    }

    pub const fn runtime_mut(&mut self) -> &mut RemoteClientRuntime {
        &mut self.runtime
    }
}

#[derive(Debug, Default, Resource)]
pub struct RemoteSnapshotInbox(Vec<SnapshotChunk>);

impl RemoteSnapshotInbox {
    pub fn push(&mut self, chunk: SnapshotChunk) {
        self.0.push(chunk);
    }

    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, Default, Clone, Resource)]
pub struct NetworkCorrectionDiagnostics {
    pub micro_corrections: u64,
    pub noticeable_corrections: u64,
    pub snaps: u64,
    pub debug_metrics: u64,
    pub anomalies: u64,
    pub network_warning: bool,
    pub latest: Option<ReconciliationEvent>,
}

#[derive(Debug, Clone, Copy, Component)]
struct NetworkEntityPresentation {
    entity_id: u64,
}

#[derive(Debug, Clone, Copy, Component)]
struct PredictedProjectileVisual {
    source_input_sequence: u32,
    source_projectile_ordinal: u16,
}

type RemoteVisualQuery<'world, 'state> = Query<
    'world,
    'state,
    (&'static NetworkEntityPresentation, &'static mut Transform),
    (Without<LocalPlayer>, Without<PredictedProjectileVisual>),
>;
type PredictedVisualQuery<'world, 'state> = Query<
    'world,
    'state,
    (
        Entity,
        &'static PredictedProjectileVisual,
        &'static mut Transform,
    ),
    (
        With<PredictedProjectileVisual>,
        Without<LocalPlayer>,
        Without<NetworkEntityPresentation>,
    ),
>;

pub(crate) fn configure(app: &mut App) {
    app.insert_resource(RemoteSnapshotInbox::default())
        .insert_resource(NetworkCorrectionDiagnostics::default())
        .add_systems(
            Update,
            (process_snapshot_inbox, sync_network_presentation)
                .chain()
                .in_set(FrameSet::Presentation),
        );
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn process_snapshot_inbox(
    mut commands: Commands,
    mut presentation: Option<ResMut<NativeNetworkPresentation>>,
    mut inbox: ResMut<RemoteSnapshotInbox>,
    mut diagnostics: ResMut<NetworkCorrectionDiagnostics>,
    existing: Query<(Entity, &NetworkEntityPresentation)>,
) {
    let Some(presentation) = presentation.as_deref_mut() else {
        return;
    };
    let chunks = std::mem::take(&mut inbox.0);
    for chunk in chunks {
        let application = match presentation.runtime.ingest_snapshot_chunk(chunk) {
            Ok(Some(application)) => application,
            Ok(None) => continue,
            Err(error) => {
                bevy::log::warn!(feature_id = "GB-M02-03", %error, "network snapshot rejected");
                continue;
            }
        };
        presentation.estimated_server_tick_milli = presentation
            .estimated_server_tick_milli
            .max(application.snapshot.server_tick * MILLI_TICKS_PER_TICK);
        record_correction(&mut diagnostics, application.correction);
        synchronize_snapshot_entities(
            &mut commands,
            &existing,
            &application.snapshot,
            presentation.runtime.local_entity_id,
        );
    }
}

fn record_correction(
    diagnostics: &mut NetworkCorrectionDiagnostics,
    correction: ReconciliationEvent,
) {
    match correction.class {
        CorrectionClass::MicroBlend => {
            diagnostics.micro_corrections = diagnostics.micro_corrections.saturating_add(1);
        }
        CorrectionClass::NoticeableBlend => {
            diagnostics.noticeable_corrections =
                diagnostics.noticeable_corrections.saturating_add(1);
        }
        CorrectionClass::Snap => {
            diagnostics.snaps = diagnostics.snaps.saturating_add(1);
        }
    }
    match correction.signal {
        CorrectionSignal::None => {}
        CorrectionSignal::DebugMetric => {
            diagnostics.debug_metrics = diagnostics.debug_metrics.saturating_add(1);
        }
        CorrectionSignal::NetworkWarningAndAnomaly => {
            diagnostics.anomalies = diagnostics.anomalies.saturating_add(1);
        }
    }
    diagnostics.network_warning = correction.signal == CorrectionSignal::NetworkWarningAndAnomaly;
    diagnostics.latest = Some(correction);
}

fn synchronize_snapshot_entities(
    commands: &mut Commands,
    existing: &Query<(Entity, &NetworkEntityPresentation)>,
    snapshot: &CompleteSnapshot,
    local_entity_id: u64,
) {
    let desired = snapshot
        .entities
        .iter()
        .filter(|entity| entity.entity_id != local_entity_id)
        .filter(|entity| entity.kind != EntityKind::FriendlyProjectile)
        .filter(|entity| entity.state_flags & ENTITY_STATE_ALIVE != 0)
        .map(|entity| (entity.entity_id, entity.kind))
        .collect::<BTreeMap<_, _>>();
    let existing_ids = existing
        .iter()
        .map(|(_, presentation)| presentation.entity_id)
        .collect::<BTreeSet<_>>();
    for (entity, presentation) in existing {
        if !desired.contains_key(&presentation.entity_id) {
            commands.entity(entity).despawn();
        }
    }
    for (entity_id, kind) in desired {
        if existing_ids.contains(&entity_id) {
            continue;
        }
        let (color, size, z) = network_visual_style(kind);
        commands.spawn((
            Name::new(format!("Network {kind:?} {entity_id}")),
            NetworkEntityPresentation { entity_id },
            Sprite::from_color(color, Vec2::splat(size)),
            Transform::from_xyz(0.0, 0.0, z),
        ));
    }
}

fn network_visual_style(kind: EntityKind) -> (Color, f32, f32) {
    match kind {
        EntityKind::Player => (Color::srgb_u8(211, 241, 224), 0.54, 8.0),
        EntityKind::Enemy | EntityKind::Boss => (Color::srgb_u8(184, 72, 78), 0.62, 5.5),
        EntityKind::FriendlyProjectile => (Color::srgb_u8(82, 211, 178), 0.18, 6.5),
        EntityKind::HostileProjectile => (Color::srgb_u8(242, 107, 91), 0.22, 7.0),
        EntityKind::PersonalPickup | EntityKind::Loot => (Color::srgb_u8(240, 213, 139), 0.30, 4.5),
        EntityKind::Objective => (Color::srgb_u8(191, 139, 241), 0.40, 4.0),
    }
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrapper values.
fn sync_network_presentation(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<LoadedArena>,
    mut presentation: Option<ResMut<NativeNetworkPresentation>>,
    mut local_player: Query<&mut Transform, With<LocalPlayer>>,
    mut remote: RemoteVisualQuery,
    mut predicted: PredictedVisualQuery,
) {
    let Some(presentation) = presentation.as_deref_mut() else {
        return;
    };
    let elapsed_ms = u32::try_from(time.delta().as_millis()).unwrap_or(u32::MAX);
    presentation.runtime.advance_presentation(elapsed_ms);
    presentation.presentation_time_ms = presentation
        .presentation_time_ms
        .saturating_add(u64::from(elapsed_ms));
    presentation.estimated_server_tick_milli = presentation
        .estimated_server_tick_milli
        .saturating_add(delta_milli_ticks(time.delta()));
    if let Ok(mut transform) = local_player.single_mut() {
        let render = simulation_point_to_render(
            presentation.runtime.local_presentation_position(),
            &arena.0,
        );
        transform.translation.x = render.x;
        transform.translation.y = render.y;
    }
    for (entity, mut transform) in &mut remote {
        if let Ok(Some(sample)) = presentation
            .runtime
            .remote_entity_at(entity.entity_id, presentation.estimated_server_tick_milli)
        {
            let render = simulation_point_to_render(
                milli_vector(sample.x_milli_tiles, sample.y_milli_tiles),
                &arena.0,
            );
            transform.translation.x = render.x;
            transform.translation.y = render.y;
        }
    }
    synchronize_predicted_projectile_visuals(
        &mut commands,
        &mut predicted,
        &presentation.runtime,
        presentation.presentation_time_ms,
        &arena.0,
    );
}

fn synchronize_predicted_projectile_visuals(
    commands: &mut Commands,
    existing: &mut PredictedVisualQuery,
    runtime: &RemoteClientRuntime,
    presentation_time_ms: u64,
    arena: &ArenaGeometry,
) {
    let active = runtime.local_projectiles_at(presentation_time_ms);
    let active_keys = active
        .iter()
        .map(|track| (track.source_input_sequence, track.source_projectile_ordinal))
        .collect::<BTreeSet<_>>();
    let existing_keys = existing
        .iter()
        .map(|(_, visual, _)| {
            (
                visual.source_input_sequence,
                visual.source_projectile_ordinal,
            )
        })
        .collect::<BTreeSet<_>>();
    for (entity, visual, _) in existing.iter() {
        if !active_keys.contains(&(
            visual.source_input_sequence,
            visual.source_projectile_ordinal,
        )) {
            commands.entity(entity).despawn();
        }
    }
    for track in active {
        let key = (track.source_input_sequence, track.source_projectile_ordinal);
        let Ok(position) = track.position_at(presentation_time_ms) else {
            continue;
        };
        let render = simulation_point_to_render(milli_vector(position.0, position.1), arena);
        if let Some((entity, _, _)) = existing.iter().find(|(_, visual, _)| {
            (
                visual.source_input_sequence,
                visual.source_projectile_ordinal,
            ) == key
        }) && let Ok((_, _, mut transform)) = existing.get_mut(entity)
        {
            transform.translation.x = render.x;
            transform.translation.y = render.y;
        } else if !existing_keys.contains(&key) {
            commands.spawn((
                Name::new(format!("Predicted projectile {}:{}", key.0, key.1)),
                PredictedProjectileVisual {
                    source_input_sequence: key.0,
                    source_projectile_ordinal: key.1,
                },
                Sprite::from_color(Color::srgb_u8(82, 211, 178), Vec2::splat(0.18)),
                Transform::from_xyz(render.x, render.y, 6.6),
            ));
        }
    }
}

fn delta_milli_ticks(delta: std::time::Duration) -> u64 {
    let nanos = delta.as_nanos();
    let milli_ticks = nanos
        .saturating_mul(u128::from(SERVER_TICKS_PER_SECOND * MILLI_TICKS_PER_TICK))
        / 1_000_000_000;
    u64::try_from(milli_ticks).unwrap_or(u64::MAX)
}

fn milli_vector(x_milli_tiles: i32, y_milli_tiles: i32) -> SimulationVector {
    tile_point_to_simulation(TilePoint::new(x_milli_tiles, y_milli_tiles))
}

impl RemoteClientRuntime {
    #[must_use]
    pub fn new(
        local_entity_id: u64,
        arena: ArenaGeometry,
        initial_movement: PlayerMovementState,
    ) -> Self {
        Self {
            local_entity_id,
            assembler: SnapshotAssembler::default(),
            local: LocalMovementPrediction::new(local_entity_id, arena, initial_movement),
            remote: RemoteInterpolator::default(),
            projectiles: ProjectilePresentationSet::default(),
        }
    }

    pub fn predict_local_movement(
        &mut self,
        input: PredictedMovementInput,
    ) -> Result<PlayerMovementState, NetworkPredictionError> {
        self.local.predict(input)
    }

    pub fn start_local_projectile(
        &mut self,
        source_input_sequence: u32,
        source_projectile_ordinal: u16,
        start_tick_millis: u64,
        origin_milli_tiles: (i32, i32),
        velocity_milli_tiles_per_second: (i32, i32),
    ) -> Result<(), NetworkPredictionError> {
        self.projectiles.start_local(
            source_input_sequence,
            source_projectile_ordinal,
            start_tick_millis,
            origin_milli_tiles,
            velocity_milli_tiles_per_second,
        )
    }

    pub fn ingest_snapshot_chunk(
        &mut self,
        chunk: SnapshotChunk,
    ) -> Result<Option<SnapshotApplication>, NetworkPredictionError> {
        let Some(snapshot) = self.assembler.push(chunk)? else {
            return Ok(None);
        };
        let correction = self.local.reconcile(&snapshot)?;
        self.remote.ingest(&snapshot, self.local_entity_id);
        self.projectiles.ingest(&snapshot);
        Ok(Some(SnapshotApplication {
            snapshot,
            correction,
        }))
    }

    pub fn advance_presentation(&mut self, elapsed_ms: u32) {
        self.local.advance_presentation(elapsed_ms);
    }

    #[must_use]
    pub fn local_simulation_state(&self) -> PlayerMovementState {
        self.local.state
    }

    #[must_use]
    pub fn local_presentation_position(&self) -> SimulationVector {
        self.local.presentation_position()
    }

    pub fn remote_entity_at(
        &self,
        entity_id: u64,
        estimated_server_tick_millis: u64,
    ) -> Result<Option<InterpolatedEntity>, NetworkPredictionError> {
        self.remote.sample(entity_id, estimated_server_tick_millis)
    }

    #[must_use]
    pub fn local_projectiles_at(
        &self,
        tick_millis: u64,
    ) -> Vec<&DeterministicProjectilePresentation> {
        self.projectiles.active_at(tick_millis)
    }
}

fn snapshot_position(entity: &EntitySnapshot) -> SimulationVector {
    milli_vector(entity.x_milli_tiles, entity.y_milli_tiles)
}

fn snapshot_velocity(entity: &EntitySnapshot) -> SimulationVector {
    milli_vector(
        entity.velocity_x_milli_tiles_per_second,
        entity.velocity_y_milli_tiles_per_second,
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Rounded nonnegative distance is range-checked before conversion.
fn distance_milli(
    first: SimulationVector,
    second: SimulationVector,
) -> Result<u32, NetworkPredictionError> {
    let dx = f64::from(first.x - second.x) * 1_000.0;
    let dy = f64::from(first.y - second.y) * 1_000.0;
    let distance = dx.hypot(dy).round();
    if !distance.is_finite() || !(0.0..=f64::from(u32::MAX)).contains(&distance) {
        return Err(NetworkPredictionError::CorrectionDistanceOutOfRange);
    }
    Ok(distance as u32)
}

fn interpolated_from_snapshot(entity: &EntitySnapshot) -> InterpolatedEntity {
    InterpolatedEntity {
        entity_id: entity.entity_id,
        kind: entity.kind,
        x_milli_tiles: entity.x_milli_tiles,
        y_milli_tiles: entity.y_milli_tiles,
        velocity_x_milli_tiles_per_second: entity.velocity_x_milli_tiles_per_second,
        velocity_y_milli_tiles_per_second: entity.velocity_y_milli_tiles_per_second,
        state_flags: entity.state_flags,
    }
}

fn interpolate_entity(
    first: &EntitySnapshot,
    second: &EntitySnapshot,
    numerator: u64,
    denominator: u64,
) -> Result<InterpolatedEntity, NetworkPredictionError> {
    if denominator == 0 || first.entity_id != second.entity_id || first.kind != second.kind {
        return Err(NetworkPredictionError::InterpolationInvariant);
    }
    let latest = interpolated_from_snapshot(second);
    Ok(InterpolatedEntity {
        x_milli_tiles: lerp_i32(
            first.x_milli_tiles,
            second.x_milli_tiles,
            numerator,
            denominator,
        )?,
        y_milli_tiles: lerp_i32(
            first.y_milli_tiles,
            second.y_milli_tiles,
            numerator,
            denominator,
        )?,
        ..latest
    })
}

fn lerp_i32(
    start: i32,
    end: i32,
    numerator: u64,
    denominator: u64,
) -> Result<i32, NetworkPredictionError> {
    let delta = i128::from(end) - i128::from(start);
    let scaled = delta
        .checked_mul(i128::from(numerator))
        .ok_or(NetworkPredictionError::InterpolationOverflow)?;
    let value = i128::from(start) + scaled / i128::from(denominator);
    i32::try_from(value).map_err(|_| NetworkPredictionError::InterpolationOverflow)
}

fn advance_milli(
    origin: i32,
    velocity_per_second: i32,
    elapsed_ms: u64,
) -> Result<i32, NetworkPredictionError> {
    let displacement = i128::from(velocity_per_second)
        .checked_mul(i128::from(elapsed_ms))
        .ok_or(NetworkPredictionError::ProjectilePresentationOverflow)?
        / 1_000;
    i32::try_from(i128::from(origin) + displacement)
        .map_err(|_| NetworkPredictionError::ProjectilePresentationOverflow)
}

fn server_tick_to_millis(server_tick: u64) -> u64 {
    server_tick.saturating_mul(1_000) / SERVER_TICKS_PER_SECOND
}

#[derive(Debug, Error)]
pub enum NetworkPredictionError {
    #[error("snapshot chunk failed protocol validation")]
    InvalidSnapshotChunk,
    #[error("snapshot chunks disagree on immutable metadata")]
    InconsistentSnapshotMetadata,
    #[error("snapshot {sequence} repeated chunk {chunk_index}")]
    DuplicateSnapshotChunk { sequence: u32, chunk_index: u16 },
    #[error("snapshot repeats entity ID {0}")]
    DuplicateSnapshotEntity(u64),
    #[error("snapshot assembler invariant failed")]
    SnapshotAssemblyInvariant,
    #[error("predicted input sequence {received} is not newer than {last}")]
    NonMonotonicPredictedInput { received: u32, last: u32 },
    #[error("prediction history reached its bounded capacity")]
    PredictionHistoryFull,
    #[error("authoritative character is dead")]
    AuthoritativeCharacterDead,
    #[error("snapshot acknowledgement regressed from {last} to {received}")]
    AcknowledgementRegressed { received: u32, last: u32 },
    #[error("complete snapshot does not contain the local player")]
    MissingLocalPlayer,
    #[error("correction distance is outside supported fixed-point range")]
    CorrectionDistanceOutOfRange,
    #[error("remote interpolation invariant failed")]
    InterpolationInvariant,
    #[error("remote interpolation arithmetic overflowed")]
    InterpolationOverflow,
    #[error("local projectile source sequence is zero or duplicated")]
    InvalidProjectileSourceSequence,
    #[error("projectile presentation arithmetic overflowed")]
    ProjectilePresentationOverflow,
    #[error(transparent)]
    Movement(#[from] MovementError),
}

#[cfg(test)]
mod tests {
    use protocol::{ENTITY_STATE_ALIVE, EntityKind, EntitySnapshot, SnapshotChunk};
    use sim_core::{ArenaGeometry, TilePoint};

    use super::*;

    fn arena() -> ArenaGeometry {
        ArenaGeometry {
            id: "arena.network_prediction_test".to_owned(),
            width_milli_tiles: 100_000,
            height_milli_tiles: 100_000,
            shell_thickness_milli_tiles: 1_000,
            player_spawn: TilePoint::new(10_000, 10_000),
            boss_spawn: TilePoint::new(80_000, 50_000),
            pillars: Vec::new(),
            anchors: Vec::new(),
        }
        .validated()
        .expect("prediction arena")
    }

    fn player(entity_id: u64, x: i32, y: i32, vx: i32, vy: i32, alive: bool) -> EntitySnapshot {
        EntitySnapshot {
            entity_id,
            kind: EntityKind::Player,
            x_milli_tiles: x,
            y_milli_tiles: y,
            velocity_x_milli_tiles_per_second: vx,
            velocity_y_milli_tiles_per_second: vy,
            source_input_sequence: 0,
            source_projectile_ordinal: 0,
            current_health: u32::from(alive) * 128,
            maximum_health: 128,
            state_flags: u32::from(alive) * ENTITY_STATE_ALIVE,
        }
    }

    fn entity(entity_id: u64, kind: EntityKind, x: i32, y: i32) -> EntitySnapshot {
        let health_entity = matches!(kind, EntityKind::Enemy | EntityKind::Boss);
        EntitySnapshot {
            entity_id,
            kind,
            x_milli_tiles: x,
            y_milli_tiles: y,
            velocity_x_milli_tiles_per_second: 0,
            velocity_y_milli_tiles_per_second: 0,
            source_input_sequence: 0,
            source_projectile_ordinal: 0,
            current_health: if health_entity { 100 } else { 0 },
            maximum_health: if health_entity { 100 } else { 0 },
            state_flags: ENTITY_STATE_ALIVE,
        }
    }

    fn friendly_projectile(
        entity_id: u64,
        source_sequence: u32,
        ordinal: u16,
        x: i32,
        velocity_x: i32,
    ) -> EntitySnapshot {
        EntitySnapshot {
            entity_id,
            kind: EntityKind::FriendlyProjectile,
            x_milli_tiles: x,
            y_milli_tiles: 10_000,
            velocity_x_milli_tiles_per_second: velocity_x,
            velocity_y_milli_tiles_per_second: 0,
            source_input_sequence: source_sequence,
            source_projectile_ordinal: ordinal,
            current_health: 0,
            maximum_health: 0,
            state_flags: ENTITY_STATE_ALIVE,
        }
    }

    fn chunk(
        sequence: u32,
        server_tick: u64,
        acknowledged_input_sequence: u32,
        entities: Vec<EntitySnapshot>,
    ) -> SnapshotChunk {
        SnapshotChunk {
            sequence,
            server_tick,
            state_version: u64::from(sequence),
            acknowledged_input_sequence,
            chunk_index: 0,
            chunk_count: 1,
            entities,
        }
    }

    fn runtime(local_entity_id: u64) -> RemoteClientRuntime {
        let arena = arena();
        let movement = PlayerMovementState::at_arena_spawn(&arena).expect("movement");
        RemoteClientRuntime::new(local_entity_id, arena, movement)
    }

    #[test]
    fn correction_thresholds_are_exact_at_every_boundary() {
        assert_eq!(classify_correction(0).class, CorrectionClass::MicroBlend);
        assert_eq!(classify_correction(99).class, CorrectionClass::MicroBlend);
        assert_eq!(
            classify_correction(100).class,
            CorrectionClass::NoticeableBlend
        );
        assert_eq!(
            classify_correction(350).class,
            CorrectionClass::NoticeableBlend
        );
        let snap = classify_correction(351);
        assert_eq!(snap.class, CorrectionClass::Snap);
        assert_eq!(snap.signal, CorrectionSignal::NetworkWarningAndAnomaly);
        assert_eq!(
            classify_correction(100).signal,
            CorrectionSignal::DebugMetric
        );
    }

    #[test]
    fn snapshot_assembly_is_order_independent_bounded_and_fail_closed() {
        let mut assembler = SnapshotAssembler::default();
        let mut first = chunk(1, 10, 4, vec![player(1, 10_000, 10_000, 0, 0, true)]);
        first.chunk_count = 2;
        let mut second = chunk(1, 10, 4, vec![entity(2, EntityKind::Enemy, 20_000, 10_000)]);
        second.chunk_count = 2;
        second.chunk_index = 1;
        assert!(assembler.push(second.clone()).unwrap().is_none());
        assert!(matches!(
            assembler.push(second),
            Err(NetworkPredictionError::DuplicateSnapshotChunk { .. })
        ));
        let complete = assembler.push(first).unwrap().expect("complete snapshot");
        assert_eq!(complete.entities.len(), 2);
        assert_eq!(assembler.last_completed_sequence(), 1);
        assert!(
            assembler
                .push(chunk(1, 10, 4, vec![player(1, 0, 0, 0, 0, true)]))
                .unwrap()
                .is_none()
        );

        let mut inconsistent = SnapshotAssembler::default();
        let mut part = chunk(2, 20, 5, vec![player(1, 10_000, 10_000, 0, 0, true)]);
        part.chunk_count = 2;
        assert!(inconsistent.push(part.clone()).unwrap().is_none());
        part.chunk_index = 1;
        part.server_tick = 21;
        assert!(matches!(
            inconsistent.push(part),
            Err(NetworkPredictionError::InconsistentSnapshotMetadata)
        ));

        let mut duplicate_entity = SnapshotAssembler::default();
        let mut left = chunk(3, 30, 5, vec![player(1, 10_000, 10_000, 0, 0, true)]);
        left.chunk_count = 2;
        let mut right = left.clone();
        right.chunk_index = 1;
        assert!(duplicate_entity.push(left).unwrap().is_none());
        assert!(matches!(
            duplicate_entity.push(right),
            Err(NetworkPredictionError::DuplicateSnapshotEntity(1))
        ));

        let mut bounded = SnapshotAssembler::default();
        for sequence in 1..=5 {
            let mut incomplete = chunk(
                sequence,
                u64::from(sequence),
                0,
                vec![player(1, 10_000, 10_000, 0, 0, true)],
            );
            incomplete.chunk_count = 2;
            assert!(bounded.push(incomplete).unwrap().is_none());
        }
        assert_eq!(bounded.pending.len(), MAX_INCOMPLETE_SNAPSHOTS);
        assert!(!bounded.pending.contains_key(&1));
    }

    #[test]
    fn reconciliation_resets_velocity_then_replays_unacknowledged_inputs() {
        let arena = arena();
        let initial = PlayerMovementState::at_arena_spawn(&arena).unwrap();
        let action = MovementAction::new(1, 0);
        let mut authoritative = initial;
        authoritative.step(action, &arena).unwrap();
        let authority_entity = player(
            1,
            to_milli(authoritative.position().x),
            to_milli(authoritative.position().y),
            to_milli(authoritative.velocity().x),
            to_milli(authoritative.velocity().y),
            true,
        );
        let mut expected = PlayerMovementState::from_authoritative_snapshot(
            snapshot_position(&authority_entity),
            snapshot_velocity(&authority_entity),
            initial.config(),
            &arena,
        )
        .unwrap();
        expected.step(action, &arena).unwrap();
        expected.step(action, &arena).unwrap();

        let mut runtime = RemoteClientRuntime::new(1, arena, initial);
        for sequence in 1..=3 {
            runtime
                .predict_local_movement(PredictedMovementInput { sequence, action })
                .unwrap();
        }
        runtime
            .ingest_snapshot_chunk(chunk(1, 1, 1, vec![authority_entity]))
            .unwrap()
            .expect("application");
        assert_eq!(runtime.local_simulation_state(), expected);
    }

    #[test]
    fn noticeable_reconciliation_blends_presentation_without_corrupting_simulation() {
        let mut runtime = runtime(1);
        let applied = runtime
            .ingest_snapshot_chunk(chunk(1, 1, 0, vec![player(1, 10_200, 10_000, 0, 0, true)]))
            .unwrap()
            .expect("application");
        assert_eq!(applied.correction.class, CorrectionClass::NoticeableBlend);
        assert!((runtime.local_simulation_state().position().x - 10.2).abs() < 1.0e-6);
        assert!((runtime.local_presentation_position().x - 10.0).abs() < 1.0e-6);
        runtime.advance_presentation(30);
        assert!((runtime.local_presentation_position().x - 10.1).abs() < 1.0e-5);
        runtime.advance_presentation(30);
        assert!((runtime.local_presentation_position().x - 10.2).abs() < 1.0e-6);
    }

    #[test]
    fn authoritative_death_is_immediate_and_disables_prediction() {
        let mut runtime = runtime(1);
        let applied = runtime
            .ingest_snapshot_chunk(chunk(1, 1, 0, vec![player(1, 10_000, 10_000, 0, 0, false)]))
            .unwrap()
            .expect("death application");
        assert!(applied.correction.authoritative_death);
        assert!(matches!(
            runtime.predict_local_movement(PredictedMovementInput {
                sequence: 1,
                action: MovementAction::new(1, 0)
            }),
            Err(NetworkPredictionError::AuthoritativeCharacterDead)
        ));
    }

    #[test]
    fn prediction_history_is_strictly_ordered_and_bounded() {
        let mut runtime = runtime(1);
        for sequence in 1..=u32::try_from(MAX_PENDING_PREDICTED_INPUTS).unwrap() {
            runtime
                .predict_local_movement(PredictedMovementInput {
                    sequence,
                    action: MovementAction::default(),
                })
                .unwrap();
        }
        assert!(matches!(
            runtime.predict_local_movement(PredictedMovementInput {
                sequence: u32::try_from(MAX_PENDING_PREDICTED_INPUTS + 1).unwrap(),
                action: MovementAction::default(),
            }),
            Err(NetworkPredictionError::PredictionHistoryFull)
        ));
        assert!(matches!(
            runtime.predict_local_movement(PredictedMovementInput {
                sequence: 1,
                action: MovementAction::default(),
            }),
            Err(NetworkPredictionError::NonMonotonicPredictedInput { .. })
        ));
    }

    #[test]
    fn remote_interpolation_holds_endpoints_and_is_exact_at_midpoint() {
        let mut runtime = runtime(1);
        runtime
            .ingest_snapshot_chunk(chunk(
                1,
                10,
                0,
                vec![
                    player(1, 10_000, 10_000, 0, 0, true),
                    entity(2, EntityKind::Enemy, 0, 5_000),
                ],
            ))
            .unwrap();
        runtime
            .ingest_snapshot_chunk(chunk(
                2,
                12,
                0,
                vec![
                    player(1, 10_000, 10_000, 0, 0, true),
                    entity(2, EntityKind::Enemy, 2_000, 5_000),
                ],
            ))
            .unwrap();
        assert_eq!(
            runtime
                .remote_entity_at(2, 12_000)
                .unwrap()
                .unwrap()
                .x_milli_tiles,
            0
        );
        assert_eq!(
            runtime
                .remote_entity_at(2, 14_000)
                .unwrap()
                .unwrap()
                .x_milli_tiles,
            1_000
        );
        assert_eq!(
            runtime
                .remote_entity_at(2, 16_000)
                .unwrap()
                .unwrap()
                .x_milli_tiles,
            2_000
        );
    }

    #[test]
    fn local_projectile_is_immediate_then_converges_and_retires_authoritatively() {
        let mut runtime = runtime(1);
        runtime
            .start_local_projectile(5, 0, 0, (0, 10_000), (6_000, 0))
            .unwrap();
        let track = runtime.local_projectiles_at(100)[0];
        assert_eq!(track.position_at(100).unwrap(), (600, 10_000));

        runtime
            .ingest_snapshot_chunk(chunk(
                1,
                2,
                4,
                vec![
                    player(1, 10_000, 10_000, 0, 0, true),
                    friendly_projectile(50, 5, 0, 400, 6_000),
                ],
            ))
            .unwrap();
        let confirmed = runtime.local_projectiles_at(166)[0];
        assert_eq!(confirmed.authoritative_entity_id, Some(50));
        assert_eq!(confirmed.position_at(166).unwrap(), (1_000, 10_000));

        runtime
            .ingest_snapshot_chunk(chunk(2, 4, 5, vec![player(1, 10_000, 10_000, 0, 0, true)]))
            .unwrap();
        assert!(runtime.local_projectiles_at(200).is_empty());
    }

    #[test]
    fn bevy_adapter_consumes_complete_snapshots_and_records_correction_diagnostics() {
        let arena = arena();
        let movement = PlayerMovementState::at_arena_spawn(&arena).unwrap();
        let runtime = RemoteClientRuntime::new(1, arena.clone(), movement);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(LoadedArena(arena))
            .insert_resource(NativeNetworkPresentation::new(runtime));
        configure(&mut app);
        app.world_mut().spawn((LocalPlayer, Transform::default()));
        app.world_mut()
            .resource_mut::<RemoteSnapshotInbox>()
            .push(chunk(1, 1, 0, vec![player(1, 10_200, 10_000, 0, 0, true)]));
        app.update();
        let diagnostics = app.world().resource::<NetworkCorrectionDiagnostics>();
        assert_eq!(diagnostics.noticeable_corrections, 1);
        assert_eq!(diagnostics.debug_metrics, 1);
        assert!(!diagnostics.network_warning);
        assert_eq!(
            app.world()
                .resource::<RemoteSnapshotInbox>()
                .pending_count(),
            0
        );
    }

    #[allow(clippy::cast_possible_truncation)]
    fn to_milli(value: f32) -> i32 {
        (value * 1_000.0).round() as i32
    }
}
