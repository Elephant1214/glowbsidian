use crate::config::{Config, MIN_MTU};
use crate::proto::messages::OfflineMessage;
use crate::proto::version::RakNetVersion;
use bytes::Bytes;
use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

const HANDSHAKE_TTL: Duration = Duration::from_secs(10);
const MAX_HANDSHAKES: usize = 4096;

/// What the handler needs to know about the current state.
pub struct OfflineContext {
    pub counts: Counts,
    /// The existing session from this address, if any.
    pub existing: Option<ExistingSession>,
}

#[derive(Clone, Copy)]
pub struct ExistingSession {
    pub guid: u64,
    /// Reached Connected. These are never replaced by an offline handshake.
    pub connected: bool,
}

#[derive(Default)]
pub struct Counts {
    pub pending: usize,
    pub connected: usize,
    pub from_ip: usize,
}

pub enum OfflineOutcome {
    /// Send this and change nothing
    Reply(OfflineMessage),
    /// Create or replace the session for this address, then send `reply`.
    CreateSession {
        guid: u64,
        mtu: u16,
        protocol: u8,
        version: RakNetVersion,
        reply: OfflineMessage,
    },
    /// Refuse: log `reason`, send `reply` (0x14 or 0x19)
    Refuse {
        reason: RefusalReason,
        reply: OfflineMessage,
    },
    /// Drops the packet
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    BadProtocol(u8),
    TooManyPending,
    TooManyFromIp,
    ServerFull,
}

pub struct OfflineHandler {
    guid: u64,
    protocols: Vec<u8>,
    max_mtu: u16,
    max_sessions: usize,
    max_pending: usize,
    max_per_ip: usize,
    trusted_ips: Vec<IpAddr>,
    cookies: bool,
    /// Protocol from each address's last REQUEST_1, until its REQUEST_2.
    handshakes: HashMap<SocketAddr, (u8, Instant)>,
    /// Keyed SipHash, seeded randomly per run, for the cookie.
    cookie_key: RandomState,
    /// The full MOTD string
    pong: Bytes,
}

impl OfflineHandler {
    pub fn new(cfg: &Config) -> Self {
        Self {
            guid: cfg.guid,
            protocols: cfg.protocols.clone(),
            max_mtu: cfg.max_mtu,
            max_sessions: cfg.max_sessions,
            max_pending: cfg.max_pending,
            max_per_ip: cfg.max_per_ip,
            trusted_ips: cfg.trusted_ips.clone(),
            cookies: cfg.cookies,
            handshakes: HashMap::new(),
            cookie_key: RandomState::new(),
            pong: Bytes::new(),
        }
    }

    pub(crate) fn handle(
        &mut self,
        from: SocketAddr,
        msg: OfflineMessage,
        ctx: &OfflineContext,
        now: Instant,
    ) -> OfflineOutcome {
        use OfflineMessage as M;

        match msg {
            M::UnconnectedPing {
                time, open_only, ..
            } => {
                // 0x02 should only be responded to if there's room
                if self.pong.is_empty() || (open_only && ctx.counts.connected >= self.max_sessions)
                {
                    return OfflineOutcome::Ignore;
                }
                tracing::trace!(%from, "responding to an offline ping");
                OfflineOutcome::Reply(M::UnconnectedPong {
                    time,
                    server_guid: self.guid,
                    data: self.pong.clone(),
                })
            }

            M::OpenConnectionRequest1 { protocol, mtu } => {
                let supported = self.protocols.contains(&protocol)
                    && RakNetVersion::for_protocol(protocol)
                        .is_some_and(|v| v.ipv6 || from.is_ipv4());
                if !supported {
                    return OfflineOutcome::Refuse {
                        reason: RefusalReason::BadProtocol(protocol),
                        reply: M::IncompatibleProtocol {
                            protocol: self.highest_protocol(),
                            server_guid: self.guid,
                        },
                    };
                }
                self.remember_handshake(from, protocol, now);
                tracing::trace!(with = %from, "connection stage 1");
                OfflineOutcome::Reply(M::OpenConnectionReply1 {
                    server_guid: self.guid,
                    cookie: self.cookies.then(|| self.cookie(from)),
                    mtu: mtu.min(self.max_mtu),
                })
            }

            M::OpenConnectionRequest2 {
                cookie,
                mtu,
                client_guid,
                ..
            } => {
                // Stay silent for spoofed or stale clients
                if self.cookies && cookie != Some(self.cookie(from)) {
                    return OfflineOutcome::Ignore;
                }
                if mtu < MIN_MTU {
                    return OfflineOutcome::Ignore;
                }
                let mtu = mtu.min(self.max_mtu);
                let reply = M::OpenConnectionReply2 {
                    server_guid: self.guid,
                    client_addr: from,
                    mtu,
                };
                match ctx.existing {
                    Some(e) if e.guid == client_guid => {
                        tracing::trace!(with = %from, "resending connection stage 2");
                        return OfflineOutcome::Reply(reply); // The original REPLY_2 was lost
                    }
                    // The old session has to timeout for a new session to be created
                    // This is to stop a new separate handshake from kicking players from the game
                    Some(e) if e.connected => return OfflineOutcome::Ignore,
                    _ => {}
                }
                let Some((protocol, at)) = self.handshakes.get(&from).copied() else {
                    return OfflineOutcome::Ignore; // no REQUEST_1 seen
                };
                if now.saturating_duration_since(at) > HANDSHAKE_TTL {
                    self.handshakes.remove(&from);
                    return OfflineOutcome::Ignore;
                }
                if let Some(reason) = self.refusal_reason(from, ctx) {
                    return OfflineOutcome::Refuse {
                        reason,
                        reply: M::NoFreeIncomingConnections {
                            server_guid: self.guid,
                        },
                    };
                }

                self.handshakes.remove(&from);
                let version = RakNetVersion::for_protocol(protocol)
                    .expect("protocols are validated in Config::validate");
                tracing::trace!(with = %from, "connection stage 2");
                OfflineOutcome::CreateSession {
                    guid: client_guid,
                    mtu,
                    protocol,
                    version,
                    reply,
                }
            }

            // Server-bound only
            _ => OfflineOutcome::Ignore,
        }
    }

    fn refusal_reason(&self, from: SocketAddr, ctx: &OfflineContext) -> Option<RefusalReason> {
        let counts = &ctx.counts;
        let trusted = self.trusted_ips.contains(&from.ip().to_canonical());

        if !trusted && counts.from_ip >= self.max_per_ip {
            Some(RefusalReason::TooManyFromIp)
        } else if counts.pending >= self.max_pending {
            Some(RefusalReason::TooManyPending)
        } else if counts.connected + counts.pending >= self.max_sessions {
            Some(RefusalReason::ServerFull)
        } else {
            None
        }
    }

    fn remember_handshake(&mut self, from: SocketAddr, protocol: u8, now: Instant) {
        if self.handshakes.len() >= MAX_HANDSHAKES {
            self.handshakes
                .retain(|_, (_, at)| now.saturating_duration_since(*at) <= HANDSHAKE_TTL);
            if self.handshakes.len() >= MAX_HANDSHAKES {
                return; // flooded: drop new ones until old ones expire
            }
        }
        self.handshakes.insert(from, (protocol, now));
    }

    fn cookie(&self, from: SocketAddr) -> u32 {
        self.cookie_key.hash_one(from) as u32
    }

    fn highest_protocol(&self) -> u8 {
        *self.protocols.iter().max().expect("validated non-empty")
    }

    pub fn set_pong(&mut self, pong: Bytes) {
        self.pong = pong;
    }
}
