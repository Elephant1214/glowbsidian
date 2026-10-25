pub mod recv;
pub mod send;

use crate::config::Config;
use crate::event::Event;
use crate::peer::{DisconnectReason, PeerId};
use crate::proto::connected::ConnectedCodec;
use crate::proto::datagram::Datagram;
use crate::proto::messages::ConnectedMessage;
use crate::proto::reliable::Reliability;
use crate::proto::version::RakNetVersion;
use crate::proto::{INTERNAL_PLACEHOLDER_V4, INTERNAL_PLACEHOLDER_V6};
use crate::session::recv::SessionRecv;
use crate::session::send::SessionSend;
use bytes::{Bytes, BytesMut};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const PING_INTERVAL: Duration = Duration::from_secs(5);
const CLOSE_LINGER: Duration = Duration::from_secs(1);

/// Something the session needs done that it can't do itself (it has no socket or channel).
pub enum Action {
    /// Send these bytes to this session's address (a datagram, ACK or NAK).
    Send(Bytes),
    /// Hand this to the server (Connected, Message).
    Emit(Event),
    /// Remove this session. The listener emits `Disconnected` if it had reached Connected.
    Close(DisconnectReason),
}

pub struct Session {
    peer: PeerId,
    addr: SocketAddr,
    guid: u64,
    mtu: u16,
    protocol: u8,
    version: RakNetVersion,
    config: SessionConfig,
    state: SessionState,
    created: Instant,
    /// The last packet
    last_recv: Instant,
    /// The last CONNECTED_PING
    last_ping: Instant,
    recv: SessionRecv,
    send: SessionSend,
}

impl Session {
    pub fn new(
        peer: PeerId,
        addr: SocketAddr,
        guid: u64,
        mtu: u16,
        protocol: u8,
        version: RakNetVersion,
        config: SessionConfig,
        now: Instant,
    ) -> Self {
        Self {
            peer,
            addr,
            guid,
            mtu,
            protocol,
            version,
            config,
            state: SessionState::Pending,
            created: now,
            last_recv: now,
            last_ping: now,
            recv: SessionRecv::default(),
            send: SessionSend::default(),
        }
    }

    pub fn peer(&self) -> PeerId {
        self.peer
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn guid(&self) -> u64 {
        self.guid
    }

    pub fn was_connected(&self) -> bool {
        matches!(
            self.state,
            SessionState::Connected
                | SessionState::Closing {
                    was_connected: true,
                    ..
                }
        )
    }

    pub fn handle_datagram(&mut self, packet: &[u8], now: Instant) -> Vec<Action> {
        let mut out = Vec::new();
        let Ok(datagram) = Datagram::decode(Bytes::copy_from_slice(packet)) else {
            return out;
        };
        self.last_recv = now;

        match datagram {
            Datagram::Ack(ranges) => self.send.on_ack(&ranges, now),
            Datagram::Nak(ranges) => self.send.on_nak(&ranges),
            Datagram::Data { seq, frames } => {
                self.recv.note_seq(seq);
                let mut delivered = Vec::new();
                for frame in frames {
                    if let Err(why) = self.recv.accept(frame, self.config, &mut delivered) {
                        tracing::debug!(addr = %self.addr, why, "raknet protocol violation");
                        out.push(Action::Close(DisconnectReason::Protocol));
                        return out;
                    }
                }
                for body in delivered {
                    self.handle_message(body, now, &mut out);
                }
            }
        }
        out
    }

    /// Queues game data. It goes out on the next tick (at most 10 ms later).
    pub fn send(&mut self, data: Bytes, reliability: Reliability) {
        if self.state == SessionState::Connected {
            self.send.push(data, reliability, self.mtu);
        }
    }

    /// Queues 0x15 and closes once it has been ACKed or timed out
    pub fn disconnect(&mut self, now: Instant) {
        if matches!(self.state, SessionState::Closing { .. }) {
            return;
        }
        self.queue_message(&ConnectedMessage::Disconnect, Reliability::ReliableOrdered);
        self.state = SessionState::Closing {
            since: now,
            was_connected: self.state == SessionState::Connected,
        };
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Action> {
        let mut out = Vec::new();
        if let Some(reason) = self.expired(now) {
            out.push(Action::Close(reason));
            return out;
        }
        if self.state == SessionState::Connected
            && now.saturating_duration_since(self.last_ping) >= PING_INTERVAL
        {
            let time = self.clock(now);
            self.queue_message(
                &ConnectedMessage::ConnectedPing { time },
                Reliability::Unreliable,
            );
            self.last_ping = now;
        }
        self.recv.flush_acks(self.mtu, &mut out);
        self.send.resend_due(now);
        self.send.flush(self.mtu, now, &mut out);
        out
    }

    fn expired(&self, now: Instant) -> Option<DisconnectReason> {
        let since = |t: Instant| now.saturating_duration_since(t);

        match self.state {
            SessionState::Pending | SessionState::Handshaking => (since(self.created)
                > self.config.pending_timeout)
                .then_some(DisconnectReason::TimedOut),
            SessionState::Connected => {
                (since(self.last_recv) > self.config.timeout).then_some(DisconnectReason::TimedOut)
            }
            SessionState::Closing { since: t, .. } => {
                (self.send.is_idle() || since(t) > CLOSE_LINGER).then_some(DisconnectReason::Kicked)
            }
        }
    }

    fn handle_message(&mut self, body: Bytes, now: Instant, out: &mut Vec<Action>) {
        let Ok(msg) = ConnectedCodec::decode(&body, self.version) else {
            return;
        };
        use ConnectedMessage as M;
        use SessionState as S;

        match (self.state, msg) {
            // Only the first 0x09 is answered. 0x10 is reliable, so the send side resends it if it's
            // lost, and answering every copy would let one datagram trigger dozens of replies.
            (S::Pending, M::ConnectionRequest { client_guid, time }) => {
                if client_guid != self.guid {
                    out.push(Action::Close(DisconnectReason::Protocol));
                    return;
                }
                let internal = vec![
                    if self.addr.is_ipv4() {
                        INTERNAL_PLACEHOLDER_V4
                    } else {
                        INTERNAL_PLACEHOLDER_V6
                    };
                    self.version.internal_ids
                ];
                let accepted = M::ConnectionRequestAccepted {
                    client_addr: self.addr,
                    internal,
                    request_time: time,
                    time: self.clock(now),
                };
                self.queue_message(&accepted, Reliability::ReliableOrdered);
                self.state = S::Handshaking;
            }
            (S::Handshaking, M::NewIncomingConnection { .. }) => {
                self.state = S::Connected;
                out.push(Action::Emit(Event::Connected {
                    peer: self.peer,
                    addr: self.addr,
                    protocol: self.protocol,
                }));
            }
            (S::Handshaking | S::Connected, M::ConnectedPing { time }) => {
                let pong = M::ConnectedPong {
                    ping_time: time,
                    pong_time: self.clock(now),
                };
                self.queue_message(&pong, Reliability::Unreliable);
            }
            (_, M::Disconnect) => out.push(Action::Close(DisconnectReason::ClientDisconnected)),
            (S::Connected, M::Game(data)) => {
                out.push(Action::Emit(Event::Message {
                    peer: self.peer,
                    data,
                }));
            }
            _ => {}
        }
    }

    fn queue_message(&mut self, msg: &ConnectedMessage, reliability: Reliability) {
        let mut buf = BytesMut::new();
        ConnectedCodec::encode(msg, &mut buf);
        self.send.push(buf.freeze(), reliability, self.mtu);
    }

    /// Milliseconds since the session started for RakNet's time fields
    fn clock(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.created).as_millis() as u64
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SessionConfig {
    /// Pending/handshaking sessions are dropped after this long without reaching connected.
    pub pending_timeout: Duration,
    /// Connected sessions are dropped after this long without receiving anything.
    pub timeout: Duration,
    /// Largest split count accepted.
    pub max_split_parts: u32,
    /// How many split messages may be reassembling at once,
    pub max_concurrent_splits: usize,
}

impl From<&Config> for SessionConfig {
    fn from(cfg: &Config) -> Self {
        Self {
            pending_timeout: cfg.pending_timeout,
            timeout: cfg.timeout,
            max_split_parts: 256,
            max_concurrent_splits: 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// After REPLY_2, waiting for CONNECTION_REQUEST (0x09). Counts toward max_pending.
    Pending,
    /// Got 0x09, sent 0x10, waiting for NEW_INCOMING_CONNECTION (0x13).
    Handshaking,
    /// Got 0x13. Emitted `Connected`; game data flows. Counts toward max_sessions.
    Connected,
    /// Sent or received 0x15. Flushing the last reliable frames, then removed.
    Closing { since: Instant, was_connected: bool },
}
