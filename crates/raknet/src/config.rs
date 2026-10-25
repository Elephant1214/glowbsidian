use crate::proto::version::RakNetVersion;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;
use thiserror::Error;

pub const MIN_MTU: u16 = 576;
pub const MAX_MTU: u16 = 1492;

#[derive(Debug, Clone)]
pub struct Config {
    /// IPv4 socket. `None` disables IPv4 and clients using protocols <= 7.
    pub ipv4: Option<SocketAddrV4>,
    /// IPv6 socket, bound with `IPV6_V6ONLY`. `None` disables IPv6.
    pub ipv6: Option<SocketAddrV6>,
    /// A random number generated for each run and sent in pongs and handshake replies.
    pub guid: u64,
    /// Accepted RakNet protocols. Connections from other protocols get
    /// `INCOMPATIBLE_PROTOCOL_VERSION` with the highest supported protocol. This cannot be empty.
    pub protocols: Vec<u8>,
    /// Ceiling for each session's negotiated MTU.
    /// This is allowed to be set above the maximum that the client supports to enable something
    /// like a proxy in the future.
    ///
    /// ### Allowed values
    /// Min: [`MIN_MTU`]\
    /// Max: [`u16::MAX`]
    pub max_mtu: u16,
    /// Hard ceiling on connected sessions (after `CONNECTION_REQUEST`).
    /// Excess gets `NO_FREE_INCOMING_CONNECTIONS`.
    ///
    /// ### Allowed values
    /// `>= 1`
    pub max_sessions: usize,
    /// Half-open sessions (after `OPEN_CONNECTION_REPLY_2`, before `CONNECTION_REQUEST`).
    /// When full, the oldest one is evicted to make room.
    ///
    /// ### Allowed values
    /// `>= 1`
    pub max_pending: usize,
    /// Pending + connected sessions per IP (per /64 block for IPv6).
    /// Excess gets `NO_FREE_INCOMING_CONNECTIONS`.
    ///
    /// ### Allowed values
    /// `>= 1`
    pub max_per_ip: usize,
    /// A list of IP addresses that bypass `max_per_ip`.
    pub trusted_ips: Vec<IpAddr>,
    /// How long a half-open session will wait for `CONNECTION_REQUEST`. Non-zero.
    pub pending_timeout: Duration,
    /// The time with no reply allowed before a connected session is dropped. Non-zero.
    pub timeout: Duration,
    /// Anti-spoof cookie in `OPEN_CONNECTION_REPLY_1`.
    pub cookies: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ipv4: Some(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 19132)),
            ipv6: Some(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 19133, 0, 0)),
            guid: rand::random(),
            protocols: vec![8],
            max_mtu: 1492,
            max_sessions: 25,
            max_pending: 32,
            max_per_ip: 5,
            trusted_ips: Vec::new(),
            pending_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(10),
            cookies: true,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.protocols.is_empty() {
            return Err(ConfigError::NoProtocols);
        }

        let v6_capable = self
            .protocols
            .iter()
            .any(|&p| RakNetVersion::for_protocol(p).is_some_and(|v| v.ipv6));
        if self.ipv4.is_none() && self.ipv6.is_none() {
            return Err(ConfigError::NoAddress);
        }
        if self.ipv4.is_none() && !v6_capable {
            return Err(ConfigError::Ipv6OnlyWithoutV6Protocol);
        }

        if !(MIN_MTU..=MAX_MTU).contains(&self.max_mtu) {
            return Err(ConfigError::MtuOutOfRange(self.max_mtu));
        }

        for (field, value) in [
            ("max_sessions", self.max_sessions),
            ("max_pending", self.max_pending),
            ("max_per_ip", self.max_per_ip),
        ] {
            if value == 0 {
                return Err(ConfigError::InvalidValue {
                    field,
                    value: value.to_string(),
                    expected: "at least 1",
                });
            }
        }

        for (field, value) in [
            ("pending_timeout", self.pending_timeout),
            ("timeout", self.timeout),
        ] {
            if value.is_zero() {
                return Err(ConfigError::InvalidValue {
                    field,
                    value: format!("{value:?}"),
                    expected: "non-zero",
                });
            }
        }

        if let Some(&p) = self
            .protocols
            .iter()
            .find(|&&p| RakNetVersion::for_protocol(p).is_none())
        {
            return Err(ConfigError::UnsupportedProtocol(p));
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("no IPv4 or IPv6 address configured")]
    NoAddress,
    #[error("IPv6-only enabled, but no configured protocol supports IPv6 (needs 8+)")]
    Ipv6OnlyWithoutV6Protocol,
    #[error("no RakNet protocols configured")]
    NoProtocols,
    #[error("unsupported RakNet protocol {0}")]
    UnsupportedProtocol(u8),
    #[error("max MTU {0} is outside valid range: {MIN_MTU}-{MAX_MTU}")]
    MtuOutOfRange(u16),
    #[error("invalid {field}: {value} (expected {expected})")]
    InvalidValue {
        field: &'static str,
        value: String,
        expected: &'static str,
    },
}
