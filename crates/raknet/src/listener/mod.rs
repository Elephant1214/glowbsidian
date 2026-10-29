pub(crate) mod offline;
mod socket;

use crate::config::Config;
use crate::event::{Command, Event};
use crate::listener::offline::{
    Counts, ExistingSession, OfflineContext, OfflineHandler, OfflineOutcome, ip_bucket,
};
use crate::listener::socket::{bind_v4, bind_v6, recv};
use crate::peer::PeerId;
use crate::proto::offline::{OfflineCodec, OfflineMessage};
use crate::proto::reliability::Reliability;
use crate::session::{Action, Session, SessionConfig};
use bytes::{Bytes, BytesMut};
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time;

pub struct Listener {
    events: mpsc::UnboundedReceiver<Event>,
    commands: mpsc::UnboundedSender<Command>,
    local_v4: Option<SocketAddr>,
    local_v6: Option<SocketAddr>,
    guid: u64,
}

impl Listener {
    pub async fn bind(cfg: Config) -> io::Result<Self> {
        cfg.validate()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let v4 = match cfg.ipv4 {
            Some(addr) => Some(bind_v4(addr).await?),
            None => None,
        };
        let v6 = cfg.ipv6.map(bind_v6).transpose()?;
        let local_v4 = v4.as_ref().map(|s| s.local_addr()).transpose()?;
        let local_v6 = v6.as_ref().map(|s| s.local_addr()).transpose()?;

        let (event_tx, events) = mpsc::unbounded_channel();
        let (commands, command_rx) = mpsc::unbounded_channel();

        let task = Task {
            v4,
            v6,
            recv_len: (cfg.max_mtu as usize).max(1500),
            max_pending: cfg.max_pending,
            codec: OfflineCodec {
                cookies: cfg.cookies,
            },
            session_config: SessionConfig::from(&cfg),
            offline: OfflineHandler::new(&cfg),
            sessions: HashMap::new(),
            peers: HashMap::new(),
            next_peer: 0,
            event_tx,
            command_rx,
        };
        let guid = cfg.guid;
        tokio::spawn(task.run());

        Ok(Self {
            events,
            commands,
            local_v4,
            local_v6,
            guid,
        })
    }

    pub fn local_v4(&self) -> Option<SocketAddr> {
        self.local_v4
    }

    pub fn local_v6(&self) -> Option<SocketAddr> {
        self.local_v6
    }

    pub fn guid(&self) -> u64 {
        self.guid
    }

    pub async fn next_event(&mut self) -> Option<Event> {
        self.events.recv().await
    }

    pub fn send(&self, peer: PeerId, data: Bytes, reliability: Reliability) {
        let _ = self.commands.send(Command::Send {
            peer,
            data,
            reliability,
        });
    }

    pub fn disconnect(&self, peer: PeerId) {
        let _ = self.commands.send(Command::Disconnect(peer));
    }

    pub fn set_pong(&self, motd: String) {
        let _ = self.commands.send(Command::SetPong(motd));
    }
}

struct Task {
    v4: Option<UdpSocket>,
    v6: Option<UdpSocket>,
    recv_len: usize,
    max_pending: usize,
    codec: OfflineCodec,
    /// Owns cfg-derived policy, cookie secret, pong bytes
    offline: OfflineHandler,
    session_config: SessionConfig,
    sessions: HashMap<SocketAddr, Session>,
    /// Commands come in by PeerId
    peers: HashMap<PeerId, SocketAddr>,
    next_peer: u32,
    event_tx: mpsc::UnboundedSender<Event>,
    command_rx: mpsc::UnboundedReceiver<Command>,
}

impl Task {
    async fn run(mut self) {
        let mut buf4 = vec![0u8; if self.v4.is_some() { self.recv_len } else { 0 }];
        let mut buf6 = vec![0u8; if self.v6.is_some() { self.recv_len } else { 0 }];

        let mut tick = time::interval(Duration::from_millis(10));
        tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        tracing::info!("Server listening");
        loop {
            tokio::select! {
                r = recv(&self.v4, &mut buf4), if self.v4.is_some() => self.on_recv(r, &buf4).await,
                r = recv(&self.v6, &mut buf6), if self.v6.is_some() => self.on_recv(r, &buf6).await,
                cmd = self.command_rx.recv() => match cmd {
                    Some(cmd) => self.on_command(cmd).await,
                    None => break,
                },
                _ = tick.tick() => self.on_tick().await,
            }
        }
    }

    async fn on_recv(&mut self, result: io::Result<(usize, SocketAddr)>, buf: &[u8]) {
        match result {
            Ok((n, from)) => self.on_datagram(&buf[..n], from).await,
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
            Err(e) => tracing::debug!(error = %e, "raknet recv failed"),
        }
    }

    async fn on_datagram(&mut self, data: &[u8], from: SocketAddr) {
        let Some(&id) = data.first() else { return };

        if id & 0x80 != 0 {
            let Some(session) = self.sessions.get_mut(&from) else {
                return;
            };
            let actions = session.handle_datagram(data, Instant::now());
            self.apply(from, actions).await;
            return;
        }

        let msg = match self.codec.decode(data) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::trace!(%from, ?e, "raknet bad offline packet");
                return;
            }
        };

        let ctx = OfflineContext {
            counts: self.counts(from.ip()),
            existing: self.sessions.get(&from).map(|s| ExistingSession {
                guid: s.guid(),
                connected: s.was_connected(),
            }),
        };

        match self.offline.handle(from, msg, &ctx, Instant::now()) {
            OfflineOutcome::Reply(reply) => {
                self.send_offline(from, &reply).await;
            }
            OfflineOutcome::CreateSession { negotiated, reply } => {
                tracing::trace!(for = %from, "creating a new session");
                let peer = self.next_peer_id();
                let session =
                    Session::new(peer, from, negotiated, self.session_config, Instant::now());

                if ctx.counts.pending >= self.max_pending && !self.sessions.contains_key(&from) {
                    let oldest = self
                        .sessions
                        .iter()
                        .filter(|(_, s)| !s.was_connected())
                        .min_by_key(|(_, s)| s.created())
                        .map(|(addr, _)| *addr);
                    if let Some(addr) = oldest
                        && let Some(old) = self.sessions.remove(&addr)
                    {
                        self.peers.remove(&old.peer());
                        tracing::debug!(evicted = %addr, for = %from, "pending pool full, evicted oldest");
                    }
                }

                if let Some(old) = self.sessions.insert(from, session) {
                    self.peers.remove(&old.peer());
                }
                self.peers.insert(peer, from);
                self.send_offline(from, &reply).await;
            }
            OfflineOutcome::Refuse { reason, reply } => {
                tracing::debug!(%from, ?reason, "raknet refused");
                self.send_offline(from, &reply).await;
            }
            OfflineOutcome::Ignore => {
                tracing::trace!(%from, "ignored a datagram")
            }
        }
    }

    async fn on_command(&mut self, cmd: Command) {
        match cmd {
            Command::SetPong(pong) => self.offline.set_pong(pong.into()),
            Command::Send {
                peer,
                data,
                reliability,
            } => {
                if let Some(s) = self.peers.get(&peer).and_then(|a| self.sessions.get_mut(a)) {
                    s.send(data, reliability);
                }
            }
            Command::Disconnect(peer) => {
                if let Some(s) = self.peers.get(&peer).and_then(|a| self.sessions.get_mut(a)) {
                    s.disconnect(Instant::now());
                }
            }
        }
    }

    async fn on_tick(&mut self) {
        let now = Instant::now();
        let due: Vec<(SocketAddr, Vec<Action>)> = self
            .sessions
            .iter_mut()
            .map(|(addr, s)| (*addr, s.tick(now)))
            .filter(|(_, a)| !a.is_empty())
            .collect();
        for (addr, actions) in due {
            self.apply(addr, actions).await;
        }
    }

    async fn apply(&mut self, addr: SocketAddr, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send(bytes) => self.send(addr, &bytes).await,
                Action::Emit(event) => {
                    let _ = self.event_tx.send(event);
                }
                Action::Close(reason) => {
                    if let Some(s) = self.sessions.remove(&addr) {
                        self.peers.remove(&s.peer());
                        if s.was_connected() {
                            let _ = self.event_tx.send(Event::Disconnected {
                                peer: s.peer(),
                                reason,
                            });
                        }
                    }
                    return;
                }
            }
        }
    }

    async fn send(&self, to: SocketAddr, bytes: &[u8]) {
        let sock = if to.is_ipv4() { &self.v4 } else { &self.v6 };
        if let Some(s) = sock {
            let result = s.send_to(bytes, to).await;
            tracing::trace!(?result, "send_to done")
        }
    }

    async fn send_offline(&self, to: SocketAddr, msg: &OfflineMessage) {
        let mut out = BytesMut::new();
        self.codec.encode(msg, &mut out);
        self.send(to, &out).await;
    }

    fn counts(&self, ip: IpAddr) -> Counts {
        let bucket = ip_bucket(ip);
        let mut counts = Counts::default();
        for session in self.sessions.values() {
            if session.was_connected() {
                counts.connected += 1
            } else {
                counts.pending += 1
            }
            if ip_bucket(session.addr().ip()) == bucket {
                counts.from_ip += 1
            }
        }
        counts
    }

    fn next_peer_id(&mut self) -> PeerId {
        self.next_peer = self.next_peer.wrapping_add(1);
        PeerId::new(self.next_peer)
    }
}
