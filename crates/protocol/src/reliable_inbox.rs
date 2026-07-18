//! Bounded contiguous delivery for reliable events received on independent QUIC streams.
//!
//! The canonical GDD owns reconnect-safe reliable authority (`TECH-015`, `TECH-021`-`023`), the
//! Content Production Specification fixes the closed Core route that consumes these events, and
//! the Development Roadmap requires restart/replay evidence for the complete M03 private loop.
//! QUIC makes each stream reliable but does not make application messages on different streams
//! arrive in one shared order, so clients buffer a small gap and publish only a contiguous prefix.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::ReliableEventFrame;

pub const RELIABLE_EVENT_REORDER_CAPACITY: usize = 64;
const RELIABLE_EVENT_REORDER_WINDOW: u32 = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReliableEventInbox {
    last_delivered_sequence: u32,
    pending: BTreeMap<u32, ReliableEventFrame>,
}

impl Default for ReliableEventInbox {
    fn default() -> Self {
        Self::new()
    }
}

impl ReliableEventInbox {
    #[must_use]
    pub const fn new() -> Self {
        Self::resume_after(0)
    }

    /// Starts a fresh transport inbox after a logical session's last contiguous delivery.
    /// Pending frames from a lost transport are intentionally not carried across connections.
    #[must_use]
    pub const fn resume_after(last_delivered_sequence: u32) -> Self {
        Self {
            last_delivered_sequence,
            pending: BTreeMap::new(),
        }
    }

    /// Accepts one validated frame and returns only the newly contiguous delivery prefix.
    pub fn push(
        &mut self,
        frame: ReliableEventFrame,
    ) -> Result<Vec<ReliableEventFrame>, ReliableEventInboxError> {
        frame
            .validate()
            .map_err(|_| ReliableEventInboxError::InvalidFrame)?;
        let expected = self
            .last_delivered_sequence
            .checked_add(1)
            .ok_or(ReliableEventInboxError::SequenceExhausted)?;
        if frame.sequence < expected {
            return Err(ReliableEventInboxError::StaleSequence {
                expected,
                received: frame.sequence,
            });
        }
        if frame.sequence > expected {
            let gap = frame.sequence - expected;
            if gap > RELIABLE_EVENT_REORDER_WINDOW {
                return Err(ReliableEventInboxError::GapTooLarge {
                    expected,
                    received: frame.sequence,
                });
            }
            if let Some(existing) = self.pending.get(&frame.sequence) {
                return if existing == &frame {
                    Ok(Vec::new())
                } else {
                    Err(ReliableEventInboxError::ConflictingSequence(frame.sequence))
                };
            }
            if self.pending.len() == RELIABLE_EVENT_REORDER_CAPACITY {
                return Err(ReliableEventInboxError::Saturated);
            }
            self.pending.insert(frame.sequence, frame);
            return Ok(Vec::new());
        }

        let mut ready = Vec::with_capacity(self.pending.len().saturating_add(1));
        self.deliver(frame, &mut ready);
        while let Some(next) = self.next_expected_sequence()
            && let Some(frame) = self.pending.remove(&next)
        {
            self.deliver(frame, &mut ready);
        }
        Ok(ready)
    }

    fn deliver(&mut self, frame: ReliableEventFrame, ready: &mut Vec<ReliableEventFrame>) {
        self.last_delivered_sequence = frame.sequence;
        ready.push(frame);
    }

    #[must_use]
    pub const fn last_delivered_sequence(&self) -> u32 {
        self.last_delivered_sequence
    }

    #[must_use]
    pub const fn next_expected_sequence(&self) -> Option<u32> {
        self.last_delivered_sequence.checked_add(1)
    }

    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    #[must_use]
    pub fn has_gap(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ReliableEventInboxError {
    #[error("reliable event frame is invalid")]
    InvalidFrame,
    #[error("reliable event sequence {received} precedes expected sequence {expected}")]
    StaleSequence { expected: u32, received: u32 },
    #[error("reliable event sequence {0} was reused with different content")]
    ConflictingSequence(u32),
    #[error("reliable event gap from {expected} to {received} exceeds the bounded window")]
    GapTooLarge { expected: u32, received: u32 },
    #[error("reliable event reorder inbox is saturated")]
    Saturated,
    #[error("reliable event sequence is exhausted")]
    SequenceExhausted,
}

#[cfg(test)]
mod tests {
    use crate::{ActionResultCode, ReliableEvent};

    use super::*;

    fn frame(sequence: u32, action_sequence: u32) -> ReliableEventFrame {
        ReliableEventFrame {
            sequence,
            server_tick: u64::from(sequence),
            event: ReliableEvent::ActionResult {
                action_sequence,
                code: ActionResultCode::Accepted,
            },
        }
    }

    #[test]
    fn out_of_order_frames_publish_only_one_contiguous_prefix() {
        let mut inbox = ReliableEventInbox::new();
        assert!(inbox.push(frame(2, 2)).unwrap().is_empty());
        assert!(inbox.has_gap());
        assert_eq!(inbox.pending_count(), 1);
        assert!(inbox.push(frame(2, 2)).unwrap().is_empty());
        assert!(matches!(
            inbox.push(frame(2, 99)),
            Err(ReliableEventInboxError::ConflictingSequence(2))
        ));

        let ready = inbox.push(frame(1, 1)).unwrap();
        assert_eq!(
            ready.iter().map(|value| value.sequence).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(inbox.last_delivered_sequence(), 2);
        assert_eq!(inbox.next_expected_sequence(), Some(3));
        assert!(!inbox.has_gap());
        assert_eq!(inbox.pending_count(), 0);
        assert!(matches!(
            inbox.push(frame(1, 1)),
            Err(ReliableEventInboxError::StaleSequence {
                expected: 3,
                received: 1
            })
        ));

        let mut resumed = ReliableEventInbox::resume_after(41);
        assert!(resumed.push(frame(43, 43)).unwrap().is_empty());
        assert_eq!(
            resumed
                .push(frame(42, 42))
                .unwrap()
                .iter()
                .map(|value| value.sequence)
                .collect::<Vec<_>>(),
            vec![42, 43]
        );
    }

    #[test]
    fn invalid_oversized_and_exhausted_sequences_fail_closed() {
        let mut inbox = ReliableEventInbox::new();
        assert!(matches!(
            inbox.push(frame(0, 1)),
            Err(ReliableEventInboxError::InvalidFrame)
        ));
        assert!(matches!(
            inbox.push(frame(RELIABLE_EVENT_REORDER_WINDOW + 2, 1)),
            Err(ReliableEventInboxError::GapTooLarge {
                expected: 1,
                received
            }) if received == RELIABLE_EVENT_REORDER_WINDOW + 2
        ));

        inbox.last_delivered_sequence = u32::MAX - 1;
        let ready = inbox.push(frame(u32::MAX, 1)).unwrap();
        assert_eq!(ready[0].sequence, u32::MAX);
        assert_eq!(inbox.next_expected_sequence(), None);
        assert!(matches!(
            inbox.push(frame(u32::MAX, 1)),
            Err(ReliableEventInboxError::SequenceExhausted)
        ));
    }
}
