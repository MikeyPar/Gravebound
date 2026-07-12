//! Deterministic protocol-frame impairment for `GB-M02-05`.
//!
//! This crate owns no sockets or gameplay. It schedules canonical encoded frames on an explicit
//! clock and reports delivery/link facts to the real client and server owners.

use std::collections::BTreeMap;

use protocol::{NetworkChannel, WireCodecError, WireMessage, decode_frame, encode_frame};
use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};
use thiserror::Error;

pub const BASIS_POINTS_DENOMINATOR: u16 = 10_000;
pub const MAX_OUTAGE_WINDOWS: usize = 8;
pub const MIN_OUTAGE_MICROS: u64 = 500_000;
pub const MAX_OUTAGE_MICROS: u64 = 5_000_000;
pub const MAX_QUEUED_FRAMES: usize = 65_536;
pub const MAX_QUEUED_BYTES: usize = 64 * 1_024 * 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdverseNetworkProfile {
    Baseline,
    FullySupported,
    FairPlay,
    Degraded,
    Severe,
    M02Exit,
}

impl AdverseNetworkProfile {
    #[must_use]
    pub const fn config(self) -> ImpairmentConfig {
        match self {
            Self::Baseline => ImpairmentConfig::profile(20, 0, 0, 0),
            Self::FullySupported => ImpairmentConfig::profile(80, 10, 50, 10),
            Self::FairPlay => ImpairmentConfig::profile(120, 20, 100, 50),
            Self::Degraded => ImpairmentConfig::profile(180, 40, 200, 100),
            Self::Severe => ImpairmentConfig::profile(250, 80, 500, 200),
            Self::M02Exit => ImpairmentConfig::profile(100, 20, 100, 0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutageWindow {
    pub start_micros: u64,
    pub duration_micros: u64,
}

impl OutageWindow {
    #[must_use]
    pub const fn end_micros(self) -> Option<u64> {
        self.start_micros.checked_add(self.duration_micros)
    }

    #[must_use]
    pub fn contains(self, micros: u64) -> bool {
        self.end_micros()
            .is_some_and(|end| micros >= self.start_micros && micros < end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpairmentConfig {
    pub round_trip_micros: u64,
    pub jitter_micros: u64,
    pub datagram_loss_basis_points: u16,
    pub datagram_duplication_basis_points: u16,
    pub datagram_reordering_basis_points: u16,
    pub outage_windows: Vec<OutageWindow>,
}

impl ImpairmentConfig {
    #[must_use]
    const fn profile(
        round_trip_millis: u64,
        jitter_millis: u64,
        datagram_loss_basis_points: u16,
        datagram_reordering_basis_points: u16,
    ) -> Self {
        Self {
            round_trip_micros: round_trip_millis * 1_000,
            jitter_micros: jitter_millis * 1_000,
            datagram_loss_basis_points,
            datagram_duplication_basis_points: 0,
            datagram_reordering_basis_points,
            outage_windows: Vec::new(),
        }
    }

    pub fn try_new(
        round_trip_millis: u64,
        jitter_millis: u64,
        datagram_loss_basis_points: u16,
        datagram_reordering_basis_points: u16,
    ) -> Result<Self, HarnessError> {
        let config = Self {
            round_trip_micros: round_trip_millis
                .checked_mul(1_000)
                .ok_or(HarnessError::TimeOverflow)?,
            jitter_micros: jitter_millis
                .checked_mul(1_000)
                .ok_or(HarnessError::TimeOverflow)?,
            datagram_loss_basis_points,
            datagram_duplication_basis_points: 0,
            datagram_reordering_basis_points,
            outage_windows: Vec::new(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), HarnessError> {
        for value in [
            self.datagram_loss_basis_points,
            self.datagram_duplication_basis_points,
            self.datagram_reordering_basis_points,
        ] {
            if value > BASIS_POINTS_DENOMINATOR {
                return Err(HarnessError::InvalidBasisPoints);
            }
        }
        if self.outage_windows.len() > MAX_OUTAGE_WINDOWS {
            return Err(HarnessError::TooManyOutages);
        }
        let mut prior_end = None;
        for window in &self.outage_windows {
            if !(MIN_OUTAGE_MICROS..=MAX_OUTAGE_MICROS).contains(&window.duration_micros) {
                return Err(HarnessError::InvalidOutageDuration);
            }
            let end = window.end_micros().ok_or(HarnessError::TimeOverflow)?;
            if prior_end.is_some_and(|prior| window.start_micros < prior) {
                return Err(HarnessError::OverlappingOrUnsortedOutages);
            }
            prior_end = Some(end);
        }
        Ok(())
    }

    #[must_use]
    pub const fn one_way_base_micros(&self) -> u64 {
        self.round_trip_micros / 2
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HarnessStats {
    pub submitted: u64,
    pub delivered: u64,
    pub probabilistically_lost: u64,
    pub outage_dropped: u64,
    pub duplicate_copies: u64,
    pub reordered: u64,
    pub queued_frames: usize,
    pub queued_bytes: usize,
    pub maximum_queued_frames: usize,
    pub maximum_queued_bytes: usize,
    pub delivery_latency_total_micros: u128,
    pub minimum_delivery_latency_micros: Option<u64>,
    pub maximum_delivery_latency_micros: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendDisposition {
    Scheduled { copies: u8 },
    ProbabilisticallyLost,
    OutageDropped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkTransition {
    pub at_micros: u64,
    pub state: LinkState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub direction: Direction,
    pub submitted_at_micros: u64,
    pub delivered_at_micros: u64,
    pub duplicate: bool,
    pub encoded_frame: Vec<u8>,
    pub message: WireMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessAdvance {
    pub transitions: Vec<LinkTransition>,
    pub deliveries: Vec<Delivery>,
}

#[derive(Debug, Clone)]
struct ScheduledFrame {
    direction: Direction,
    submitted_at_micros: u64,
    duplicate: bool,
    datagram: bool,
    encoded_frame: Vec<u8>,
}

#[derive(Debug)]
pub struct NetworkHarness {
    config: ImpairmentConfig,
    rng: ChaCha8Rng,
    now_micros: u64,
    next_ordinal: u64,
    queue: BTreeMap<(u64, u64), ScheduledFrame>,
    reliable_tail: [[u64; 7]; 2],
    stats: HarnessStats,
}

impl NetworkHarness {
    pub fn new(config: ImpairmentConfig, seed: u64) -> Result<Self, HarnessError> {
        config.validate()?;
        Ok(Self {
            config,
            rng: ChaCha8Rng::seed_from_u64(seed),
            now_micros: 0,
            next_ordinal: 1,
            queue: BTreeMap::new(),
            reliable_tail: [[0; 7]; 2],
            stats: HarnessStats::default(),
        })
    }

    #[must_use]
    pub const fn config(&self) -> &ImpairmentConfig {
        &self.config
    }

    #[must_use]
    pub const fn now_micros(&self) -> u64 {
        self.now_micros
    }

    #[must_use]
    pub const fn stats(&self) -> HarnessStats {
        self.stats
    }

    #[must_use]
    pub fn link_state_at(&self, at_micros: u64) -> LinkState {
        if self.outage_containing(at_micros).is_some() {
            LinkState::Down
        } else {
            LinkState::Up
        }
    }

    pub fn submit(
        &mut self,
        direction: Direction,
        message: &WireMessage,
    ) -> Result<SendDisposition, HarnessError> {
        self.stats.submitted = self
            .stats
            .submitted
            .checked_add(1)
            .ok_or(HarnessError::CounterOverflow)?;
        let encoded_frame = encode_frame(message)?;
        let datagram = message.uses_datagram();
        if datagram && self.link_state_at(self.now_micros) == LinkState::Down {
            self.stats.outage_dropped = self
                .stats
                .outage_dropped
                .checked_add(1)
                .ok_or(HarnessError::CounterOverflow)?;
            return Ok(SendDisposition::OutageDropped);
        }
        let (lost, duplicate, reordered) = self.datagram_decisions(datagram);
        if lost {
            self.stats.probabilistically_lost = self
                .stats
                .probabilistically_lost
                .checked_add(1)
                .ok_or(HarnessError::CounterOverflow)?;
            return Ok(SendDisposition::ProbabilisticallyLost);
        }

        let jittered_delay = self.jittered_delay()?;
        let mut deliver_at = self
            .now_micros
            .checked_add(jittered_delay)
            .ok_or(HarnessError::TimeOverflow)?;
        if reordered {
            let holdback = self
                .config
                .jitter_micros
                .checked_mul(2)
                .and_then(|value| value.checked_add(1_000))
                .ok_or(HarnessError::TimeOverflow)?;
            deliver_at = deliver_at
                .checked_add(holdback)
                .ok_or(HarnessError::TimeOverflow)?;
            self.stats.reordered = self
                .stats
                .reordered
                .checked_add(1)
                .ok_or(HarnessError::CounterOverflow)?;
        }
        let reliable_slot = if datagram {
            None
        } else {
            let direction_index = direction_index(direction);
            let channel_index = channel_index(message.channel());
            let tail = self.reliable_tail[direction_index][channel_index];
            deliver_at = deliver_at.max(tail.checked_add(1).ok_or(HarnessError::TimeOverflow)?);
            Some((direction_index, channel_index))
        };
        let copy_count = if duplicate { 2 } else { 1 };
        let queued_bytes = encoded_frame
            .len()
            .checked_mul(copy_count)
            .ok_or(HarnessError::CounterOverflow)?;
        self.ensure_schedule_capacity(copy_count, queued_bytes)?;
        if let Some((direction_index, channel_index)) = reliable_slot {
            self.reliable_tail[direction_index][channel_index] = deliver_at;
        }
        self.schedule(
            deliver_at,
            ScheduledFrame {
                direction,
                submitted_at_micros: self.now_micros,
                duplicate: false,
                datagram,
                encoded_frame: encoded_frame.clone(),
            },
        )?;
        let copies = if duplicate {
            self.stats.duplicate_copies = self
                .stats
                .duplicate_copies
                .checked_add(1)
                .ok_or(HarnessError::CounterOverflow)?;
            self.schedule(
                deliver_at
                    .checked_add(1)
                    .ok_or(HarnessError::TimeOverflow)?,
                ScheduledFrame {
                    direction,
                    submitted_at_micros: self.now_micros,
                    duplicate: true,
                    datagram,
                    encoded_frame,
                },
            )?;
            2
        } else {
            1
        };
        Ok(SendDisposition::Scheduled { copies })
    }

    pub fn advance_to(&mut self, target_micros: u64) -> Result<HarnessAdvance, HarnessError> {
        if target_micros < self.now_micros {
            return Err(HarnessError::ClockMovedBackward);
        }
        let transitions = self.transitions_between(self.now_micros, target_micros)?;
        let mut deliveries = Vec::new();
        while let Some((&key, _)) = self.queue.first_key_value() {
            if key.0 > target_micros {
                break;
            }
            let frame = self.queue.remove(&key).ok_or(HarnessError::QueueCorrupt)?;
            self.stats.queued_frames -= 1;
            self.stats.queued_bytes -= frame.encoded_frame.len();
            if let Some(outage) = self.outage_containing(key.0) {
                if frame.datagram {
                    self.stats.outage_dropped = self
                        .stats
                        .outage_dropped
                        .checked_add(1)
                        .ok_or(HarnessError::CounterOverflow)?;
                    continue;
                }
                let retry_at = outage
                    .end_micros()
                    .and_then(|end| end.checked_add(1))
                    .ok_or(HarnessError::TimeOverflow)?;
                self.schedule(retry_at, frame)?;
                continue;
            }
            let message = decode_frame(&frame.encoded_frame)?;
            let latency = key
                .0
                .checked_sub(frame.submitted_at_micros)
                .ok_or(HarnessError::TimeOverflow)?;
            self.stats.delivered = self
                .stats
                .delivered
                .checked_add(1)
                .ok_or(HarnessError::CounterOverflow)?;
            self.stats.delivery_latency_total_micros = self
                .stats
                .delivery_latency_total_micros
                .checked_add(u128::from(latency))
                .ok_or(HarnessError::CounterOverflow)?;
            self.stats.minimum_delivery_latency_micros = Some(
                self.stats
                    .minimum_delivery_latency_micros
                    .map_or(latency, |minimum| minimum.min(latency)),
            );
            self.stats.maximum_delivery_latency_micros =
                self.stats.maximum_delivery_latency_micros.max(latency);
            deliveries.push(Delivery {
                direction: frame.direction,
                submitted_at_micros: frame.submitted_at_micros,
                delivered_at_micros: key.0,
                duplicate: frame.duplicate,
                encoded_frame: frame.encoded_frame,
                message,
            });
        }
        self.now_micros = target_micros;
        Ok(HarnessAdvance {
            transitions,
            deliveries,
        })
    }

    fn schedule(&mut self, at_micros: u64, frame: ScheduledFrame) -> Result<(), HarnessError> {
        if self.stats.queued_frames >= MAX_QUEUED_FRAMES {
            return Err(HarnessError::QueueFrameLimit);
        }
        let next_bytes = self
            .stats
            .queued_bytes
            .checked_add(frame.encoded_frame.len())
            .ok_or(HarnessError::CounterOverflow)?;
        if next_bytes > MAX_QUEUED_BYTES {
            return Err(HarnessError::QueueByteLimit);
        }
        let ordinal = self.next_ordinal;
        self.next_ordinal = ordinal
            .checked_add(1)
            .ok_or(HarnessError::CounterOverflow)?;
        if self.queue.insert((at_micros, ordinal), frame).is_some() {
            return Err(HarnessError::QueueCorrupt);
        }
        self.stats.queued_frames += 1;
        self.stats.queued_bytes = next_bytes;
        self.stats.maximum_queued_frames = self
            .stats
            .maximum_queued_frames
            .max(self.stats.queued_frames);
        self.stats.maximum_queued_bytes =
            self.stats.maximum_queued_bytes.max(self.stats.queued_bytes);
        Ok(())
    }

    fn ensure_schedule_capacity(
        &self,
        additional_frames: usize,
        additional_bytes: usize,
    ) -> Result<(), HarnessError> {
        let frame_count = self
            .stats
            .queued_frames
            .checked_add(additional_frames)
            .ok_or(HarnessError::CounterOverflow)?;
        if frame_count > MAX_QUEUED_FRAMES {
            return Err(HarnessError::QueueFrameLimit);
        }
        let byte_count = self
            .stats
            .queued_bytes
            .checked_add(additional_bytes)
            .ok_or(HarnessError::CounterOverflow)?;
        if byte_count > MAX_QUEUED_BYTES {
            return Err(HarnessError::QueueByteLimit);
        }
        self.next_ordinal
            .checked_add(
                u64::try_from(additional_frames).map_err(|_| HarnessError::CounterOverflow)?,
            )
            .ok_or(HarnessError::CounterOverflow)?;
        Ok(())
    }

    fn draw_basis_points(&mut self, basis_points: u16) -> bool {
        self.rng.next_u32() % u32::from(BASIS_POINTS_DENOMINATOR) < u32::from(basis_points)
    }

    fn datagram_decisions(&mut self, datagram: bool) -> (bool, bool, bool) {
        if datagram {
            (
                self.draw_basis_points(self.config.datagram_loss_basis_points),
                self.draw_basis_points(self.config.datagram_duplication_basis_points),
                self.draw_basis_points(self.config.datagram_reordering_basis_points),
            )
        } else {
            (false, false, false)
        }
    }

    fn jittered_delay(&mut self) -> Result<u64, HarnessError> {
        let base = i128::from(self.config.one_way_base_micros());
        let jitter = self.config.jitter_micros;
        let span = jitter
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .ok_or(HarnessError::TimeOverflow)?;
        let offset = i128::from(self.rng.next_u64() % span) - i128::from(jitter);
        u64::try_from((base + offset).max(0)).map_err(|_| HarnessError::TimeOverflow)
    }

    fn outage_containing(&self, micros: u64) -> Option<OutageWindow> {
        self.config
            .outage_windows
            .iter()
            .copied()
            .find(|window| window.contains(micros))
    }

    fn transitions_between(
        &self,
        start_exclusive: u64,
        end_inclusive: u64,
    ) -> Result<Vec<LinkTransition>, HarnessError> {
        let mut transitions = Vec::new();
        for window in &self.config.outage_windows {
            if window.start_micros > start_exclusive && window.start_micros <= end_inclusive {
                transitions.push(LinkTransition {
                    at_micros: window.start_micros,
                    state: LinkState::Down,
                });
            }
            let end = window.end_micros().ok_or(HarnessError::TimeOverflow)?;
            if end > start_exclusive && end <= end_inclusive {
                transitions.push(LinkTransition {
                    at_micros: end,
                    state: LinkState::Up,
                });
            }
        }
        transitions.sort_by_key(|transition| transition.at_micros);
        Ok(transitions)
    }
}

const fn direction_index(direction: Direction) -> usize {
    match direction {
        Direction::ClientToServer => 0,
        Direction::ServerToClient => 1,
    }
}

const fn channel_index(channel: NetworkChannel) -> usize {
    match channel {
        NetworkChannel::Input => 0,
        NetworkChannel::Action => 1,
        NetworkChannel::Snapshot => 2,
        NetworkChannel::Pattern => 3,
        NetworkChannel::Mutation => 4,
        NetworkChannel::Control => 5,
        NetworkChannel::Social => 6,
    }
}

#[derive(Debug, Error)]
pub enum HarnessError {
    #[error("impairment probability must be within 0..=10000 basis points")]
    InvalidBasisPoints,
    #[error("impairment profile has more than {MAX_OUTAGE_WINDOWS} outage windows")]
    TooManyOutages,
    #[error("outage duration must be 500ms..=5s")]
    InvalidOutageDuration,
    #[error("outage windows must be sorted and nonoverlapping")]
    OverlappingOrUnsortedOutages,
    #[error("harness clock moved backward")]
    ClockMovedBackward,
    #[error("harness time arithmetic overflowed")]
    TimeOverflow,
    #[error("harness counter overflowed")]
    CounterOverflow,
    #[error("harness queue exceeded {MAX_QUEUED_FRAMES} frames")]
    QueueFrameLimit,
    #[error("harness queue exceeded {MAX_QUEUED_BYTES} bytes")]
    QueueByteLimit,
    #[error("harness queue invariant failed")]
    QueueCorrupt,
    #[error(transparent)]
    Codec(#[from] WireCodecError),
}

#[cfg(test)]
mod tests {
    use protocol::{ActionFrame, ActionKind, InputFrame};

    use super::*;

    fn input(sequence: u32) -> WireMessage {
        WireMessage::InputFrame(InputFrame {
            sequence,
            client_tick: u64::from(sequence),
            movement_x_milli: 1_000,
            movement_y_milli: 0,
            aim_x_milli: 1_000,
            aim_y_milli: 0,
            held_primary: false,
            primary_sequence: 0,
            ability_1_sequence: 0,
            ability_2_sequence: 0,
        })
    }

    fn action(sequence: u32) -> WireMessage {
        WireMessage::ActionFrame(ActionFrame {
            sequence,
            client_tick: u64::from(sequence),
            action: ActionKind::Ability1Press,
        })
    }

    #[test]
    fn qa_profiles_are_exact_and_valid() {
        let expected = [
            (AdverseNetworkProfile::Baseline, 20, 0, 0, 0),
            (AdverseNetworkProfile::FullySupported, 80, 10, 50, 10),
            (AdverseNetworkProfile::FairPlay, 120, 20, 100, 50),
            (AdverseNetworkProfile::Degraded, 180, 40, 200, 100),
            (AdverseNetworkProfile::Severe, 250, 80, 500, 200),
            (AdverseNetworkProfile::M02Exit, 100, 20, 100, 0),
        ];
        for (profile, rtt_ms, jitter_ms, loss, reorder) in expected {
            let config = profile.config();
            assert_eq!(config.round_trip_micros, rtt_ms * 1_000);
            assert_eq!(config.jitter_micros, jitter_ms * 1_000);
            assert_eq!(config.datagram_loss_basis_points, loss);
            assert_eq!(config.datagram_reordering_basis_points, reorder);
            assert!(config.validate().is_ok());
        }
    }

    #[test]
    fn identical_seed_profile_and_clock_produce_identical_deliveries() {
        fn replay() -> (Vec<HarnessAdvance>, HarnessStats) {
            let mut harness =
                NetworkHarness::new(AdverseNetworkProfile::FairPlay.config(), 0x0047_5241_5645)
                    .unwrap();
            for sequence in 1..=100 {
                harness
                    .submit(Direction::ClientToServer, &input(sequence))
                    .unwrap();
            }
            let first = harness.advance_to(100_000).unwrap();
            let second = harness.advance_to(500_000).unwrap();
            (vec![first, second], harness.stats())
        }
        assert_eq!(replay(), replay());
    }

    #[test]
    fn datagrams_can_drop_duplicate_and_reorder_while_reliable_stays_ordered() {
        let mut config = AdverseNetworkProfile::Baseline.config();
        config.datagram_loss_basis_points = 0;
        config.datagram_duplication_basis_points = BASIS_POINTS_DENOMINATOR;
        config.datagram_reordering_basis_points = BASIS_POINTS_DENOMINATOR;
        let mut harness = NetworkHarness::new(config, 7).unwrap();
        assert_eq!(
            harness
                .submit(Direction::ClientToServer, &input(1))
                .unwrap(),
            SendDisposition::Scheduled { copies: 2 }
        );
        for sequence in 1..=3 {
            harness
                .submit(Direction::ClientToServer, &action(sequence))
                .unwrap();
        }
        let advance = harness.advance_to(100_000).unwrap();
        let datagrams = advance
            .deliveries
            .iter()
            .filter(|delivery| delivery.message.uses_datagram())
            .collect::<Vec<_>>();
        assert_eq!(datagrams.len(), 2);
        assert!(!datagrams[0].duplicate);
        assert!(datagrams[1].duplicate);
        let reliable_sequences = advance
            .deliveries
            .iter()
            .filter_map(|delivery| match delivery.message {
                WireMessage::ActionFrame(ref frame) => Some(frame.sequence),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(reliable_sequences, vec![1, 2, 3]);

        let mut total_loss = AdverseNetworkProfile::Baseline.config();
        total_loss.datagram_loss_basis_points = BASIS_POINTS_DENOMINATOR;
        let mut harness = NetworkHarness::new(total_loss, 7).unwrap();
        assert_eq!(
            harness
                .submit(Direction::ClientToServer, &input(1))
                .unwrap(),
            SendDisposition::ProbabilisticallyLost
        );
        assert_eq!(harness.stats().probabilistically_lost, 1);
    }

    #[test]
    fn outage_transitions_are_exact_and_datagrams_fail_closed() {
        let mut config = AdverseNetworkProfile::Baseline.config();
        config.outage_windows = vec![OutageWindow {
            start_micros: 1_000_000,
            duration_micros: 500_000,
        }];
        let mut harness = NetworkHarness::new(config, 9).unwrap();
        harness.advance_to(1_000_000).unwrap();
        assert_eq!(harness.link_state_at(1_000_000), LinkState::Down);
        assert_eq!(
            harness
                .submit(Direction::ClientToServer, &input(1))
                .unwrap(),
            SendDisposition::OutageDropped
        );
        let advance = harness.advance_to(1_500_000).unwrap();
        assert_eq!(
            advance.transitions,
            vec![LinkTransition {
                at_micros: 1_500_000,
                state: LinkState::Up
            }]
        );
    }

    #[test]
    fn invalid_profiles_and_backward_clock_fail_without_mutation() {
        let mut config = AdverseNetworkProfile::Baseline.config();
        config.datagram_loss_basis_points = 10_001;
        assert!(matches!(
            NetworkHarness::new(config, 1),
            Err(HarnessError::InvalidBasisPoints)
        ));
        let mut harness = NetworkHarness::new(AdverseNetworkProfile::Baseline.config(), 1).unwrap();
        harness.advance_to(10).unwrap();
        assert!(matches!(
            harness.advance_to(9),
            Err(HarnessError::ClockMovedBackward)
        ));
        assert_eq!(harness.now_micros(), 10);
    }

    #[test]
    fn queue_frame_bound_fails_closed_at_the_exact_limit() {
        let mut harness =
            NetworkHarness::new(AdverseNetworkProfile::Baseline.config(), 11).unwrap();
        for _ in 0..MAX_QUEUED_FRAMES {
            harness
                .submit(Direction::ClientToServer, &input(1))
                .unwrap();
        }
        assert_eq!(harness.stats().queued_frames, MAX_QUEUED_FRAMES);
        assert!(matches!(
            harness.submit(Direction::ClientToServer, &input(1)),
            Err(HarnessError::QueueFrameLimit)
        ));
        assert_eq!(harness.stats().queued_frames, MAX_QUEUED_FRAMES);
    }

    #[test]
    fn outage_bounds_reject_duration_overlap_and_count_drift() {
        let mut config = AdverseNetworkProfile::Baseline.config();
        config.outage_windows = vec![OutageWindow {
            start_micros: 1,
            duration_micros: MIN_OUTAGE_MICROS - 1,
        }];
        assert!(matches!(
            config.validate(),
            Err(HarnessError::InvalidOutageDuration)
        ));
        config.outage_windows = vec![
            OutageWindow {
                start_micros: 1,
                duration_micros: MIN_OUTAGE_MICROS,
            },
            OutageWindow {
                start_micros: 2,
                duration_micros: MIN_OUTAGE_MICROS,
            },
        ];
        assert!(matches!(
            config.validate(),
            Err(HarnessError::OverlappingOrUnsortedOutages)
        ));
        config.outage_windows = (0..=MAX_OUTAGE_WINDOWS)
            .map(|index| OutageWindow {
                start_micros: u64::try_from(index).unwrap() * 1_000_000,
                duration_micros: MIN_OUTAGE_MICROS,
            })
            .collect();
        assert!(matches!(
            config.validate(),
            Err(HarnessError::TooManyOutages)
        ));
    }
}
