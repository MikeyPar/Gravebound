//! One reliable-event sequence and serialized write boundary per Core transport.
//!
//! The canonical GDD owns reliable replay and reconnect authority (`TECH-015`, `TECH-021`-`023`),
//! the Content Production Specification fixes the closed Core route, and the Development Roadmap
//! requires restart/replay proof for the ordinary M03 loop. Request responses, route-state pushes,
//! and terminal completion pushes must therefore share this writer instead of allocating sequence
//! numbers independently.

use protocol::{ReliableEvent, ReliableEventFrame};
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;
use tokio::sync::{Mutex, MutexGuard};

use crate::{ServerTransportError, send_server_reliable_event, write_reliable_response};

pub const CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE: u32 = 0x103;
const CORE_RELIABLE_WRITE_UNCERTAIN_REASON: &[u8] = b"reliable delivery uncertain";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CoreReliableSequence {
    last_sequence: u32,
}

/// One connection's sequence and write authority. The mutex is acquired only after domain work
/// has produced a concrete event, then retained through the transport write. No producer can
/// reserve a sequence while it is waiting on persistence or another domain owner.
#[derive(Debug)]
pub struct CoreReliableWriter {
    connection: quinn::Connection,
    sequence: Mutex<CoreReliableSequence>,
    available: AtomicBool,
}

/// Owns an assigned sequence until its bytes have reached QUIC successfully. Dropping an armed
/// permit models task cancellation or an unwound write: the transport becomes unusable because
/// delivery of that sequence is no longer knowable.
struct CoreReliableWritePermit<'a> {
    connection: &'a quinn::Connection,
    available: &'a AtomicBool,
    _sequence: MutexGuard<'a, CoreReliableSequence>,
    armed: bool,
}

#[cfg(test)]
pub(crate) struct CoreReliableSequenceTestGuard<'a> {
    _sequence: MutexGuard<'a, CoreReliableSequence>,
}

#[derive(Debug, Error)]
pub enum CoreReliableWriterError {
    #[error("Core reliable event is invalid")]
    InvalidEvent,
    #[error("Core reliable sequence is exhausted")]
    SequenceExhausted,
    #[error("Core reliable writer is unavailable after retirement or uncertain delivery")]
    Unavailable,
    #[error("Core reliable transport write failed")]
    Transport(#[source] ServerTransportError),
}

impl CoreReliableSequence {
    fn frame(
        &mut self,
        server_tick: u64,
        event: ReliableEvent,
    ) -> Result<ReliableEventFrame, CoreReliableWriterError> {
        let next_sequence = self
            .last_sequence
            .checked_add(1)
            .ok_or(CoreReliableWriterError::SequenceExhausted)?;
        let frame = ReliableEventFrame {
            sequence: next_sequence,
            server_tick,
            event,
        };
        frame
            .validate()
            .map_err(|_| CoreReliableWriterError::InvalidEvent)?;
        self.last_sequence = next_sequence;
        Ok(frame)
    }
}

impl CoreReliableWritePermit<'_> {
    fn commit(mut self) -> Result<(), CoreReliableWriterError> {
        self.armed = false;
        if self.available.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(CoreReliableWriterError::Unavailable)
        }
    }
}

impl Drop for CoreReliableWritePermit<'_> {
    fn drop(&mut self) {
        if self.armed && self.available.swap(false, Ordering::AcqRel) {
            self.connection.close(
                CORE_RELIABLE_WRITE_UNCERTAIN_CLOSE_CODE.into(),
                CORE_RELIABLE_WRITE_UNCERTAIN_REASON,
            );
        }
    }
}

impl CoreReliableWriter {
    #[must_use]
    pub fn new(connection: quinn::Connection) -> Self {
        Self {
            connection,
            sequence: Mutex::new(CoreReliableSequence::default()),
            available: AtomicBool::new(true),
        }
    }

    #[must_use]
    pub(crate) const fn connection(&self) -> &quinn::Connection {
        &self.connection
    }

    #[must_use]
    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

    /// Retires this writer before an authoritative transport handoff becomes externally visible.
    /// Any in-flight write observes retirement during commit and is replayed by its domain owner.
    pub(crate) fn retire(&self, close_code: u32, reason: &'static [u8]) -> bool {
        let retired = self.available.swap(false, Ordering::AcqRel);
        if retired {
            self.connection.close(close_code.into(), reason);
        }
        retired
    }

    async fn prepare_write(
        &self,
        server_tick: u64,
        event: ReliableEvent,
    ) -> Result<(ReliableEventFrame, CoreReliableWritePermit<'_>), CoreReliableWriterError> {
        if !self.is_available() {
            return Err(CoreReliableWriterError::Unavailable);
        }
        let mut sequence = self.sequence.lock().await;
        if !self.is_available() {
            return Err(CoreReliableWriterError::Unavailable);
        }
        let frame = sequence.frame(server_tick, event)?;
        let permit = CoreReliableWritePermit {
            connection: &self.connection,
            available: &self.available,
            _sequence: sequence,
            armed: true,
        };
        Ok((frame, permit))
    }

    /// Assigns the next sequence after domain dispatch and retains the writer lock through the
    /// response-stream write.
    pub(crate) async fn send_response(
        &self,
        send: quinn::SendStream,
        server_tick: u64,
        event: ReliableEvent,
    ) -> Result<ReliableEventFrame, CoreReliableWriterError> {
        let (frame, permit) = self.prepare_write(server_tick, event).await?;
        write_reliable_response(send, &frame)
            .await
            .map_err(CoreReliableWriterError::Transport)?;
        permit.commit()?;
        Ok(frame)
    }

    /// Sends a server-generated push through the same sequence/write critical section used by
    /// request responses.
    pub(crate) async fn send_event(
        &self,
        server_tick: u64,
        event: ReliableEvent,
    ) -> Result<ReliableEventFrame, CoreReliableWriterError> {
        let (frame, permit) = self.prepare_write(server_tick, event).await?;
        send_server_reliable_event(&self.connection, &frame)
            .await
            .map_err(CoreReliableWriterError::Transport)?;
        permit.commit()?;
        Ok(frame)
    }

    #[must_use]
    pub async fn last_sequence(&self) -> u32 {
        self.sequence.lock().await.last_sequence
    }

    #[cfg(test)]
    pub(crate) async fn hold_sequence_for_test(&self) -> CoreReliableSequenceTestGuard<'_> {
        CoreReliableSequenceTestGuard {
            _sequence: self.sequence.lock().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{ActionResultCode, ReliableEvent};
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::PrivatePkcs8KeyDer;

    use super::*;

    #[test]
    fn sequence_frames_are_contiguous_valid_and_exhaustion_safe() {
        let mut sequence = CoreReliableSequence::default();
        let first = sequence
            .frame(
                10,
                ReliableEvent::ActionResult {
                    action_sequence: 1,
                    code: ActionResultCode::Accepted,
                },
            )
            .unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(first.server_tick, 10);
        assert_eq!(sequence.last_sequence, 1);

        let mut exhausted = CoreReliableSequence {
            last_sequence: u32::MAX,
        };
        assert!(matches!(
            exhausted.frame(
                11,
                ReliableEvent::ActionResult {
                    action_sequence: 2,
                    code: ActionResultCode::Accepted,
                }
            ),
            Err(CoreReliableWriterError::SequenceExhausted)
        ));
    }

    async fn live_connection_pair() -> (
        quinn::Endpoint,
        quinn::Endpoint,
        quinn::Connection,
        quinn::Connection,
    ) {
        let rcgen::CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let certificate = cert.der().clone();
        let private_key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config =
            quinn::ServerConfig::with_single_cert(vec![certificate.clone()], private_key.into())
                .unwrap();
        let server_endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(client_config);
        let connecting = client_endpoint
            .connect(server_endpoint.local_addr().unwrap(), "localhost")
            .unwrap();
        let incoming = server_endpoint.accept().await.unwrap();
        let (client, server) = tokio::join!(connecting, incoming);
        (
            server_endpoint,
            client_endpoint,
            client.unwrap(),
            server.unwrap(),
        )
    }

    #[tokio::test]
    async fn cancelled_assigned_write_poisons_and_closes_transport() {
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;
        let writer = CoreReliableWriter::new(server);
        let (_frame, assigned_write) = writer
            .prepare_write(
                10,
                ReliableEvent::ActionResult {
                    action_sequence: 1,
                    code: ActionResultCode::Accepted,
                },
            )
            .await
            .unwrap();
        drop(assigned_write);

        assert!(!writer.is_available());
        assert!(matches!(
            writer
                .send_event(
                    11,
                    ReliableEvent::ActionResult {
                        action_sequence: 2,
                        code: ActionResultCode::Accepted,
                    },
                )
                .await,
            Err(CoreReliableWriterError::Unavailable)
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), client.closed())
            .await
            .unwrap();
        server_endpoint.wait_idle().await;
        client_endpoint.wait_idle().await;
    }

    #[tokio::test]
    async fn reset_response_stream_poisons_writer_before_any_later_sequence() {
        let (server_endpoint, client_endpoint, client, server) = live_connection_pair().await;
        let writer = CoreReliableWriter::new(server);
        let (mut client_send, mut client_receive) = client.open_bi().await.unwrap();
        client_send.write_all(&[1]).await.unwrap();
        client_send.finish().unwrap();
        let (server_send, mut server_receive) = writer.connection().accept_bi().await.unwrap();
        assert_eq!(server_receive.read_to_end(1).await.unwrap(), vec![1]);
        let reset_code = quinn::VarInt::from_u32(7);
        client_receive.stop(reset_code).unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), server_send.stopped())
                .await
                .unwrap()
                .unwrap(),
            Some(reset_code)
        );

        assert!(matches!(
            writer
                .send_response(
                    server_send,
                    10,
                    ReliableEvent::ActionResult {
                        action_sequence: 1,
                        code: ActionResultCode::Accepted,
                    },
                )
                .await,
            Err(CoreReliableWriterError::Transport(_))
        ));
        assert!(!writer.is_available());
        assert_eq!(writer.last_sequence().await, 1);
        assert!(matches!(
            writer
                .send_event(
                    11,
                    ReliableEvent::ActionResult {
                        action_sequence: 2,
                        code: ActionResultCode::Accepted,
                    },
                )
                .await,
            Err(CoreReliableWriterError::Unavailable)
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), client.closed())
            .await
            .unwrap();
        server_endpoint.wait_idle().await;
        client_endpoint.wait_idle().await;
    }
}
