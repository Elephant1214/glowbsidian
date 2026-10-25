use crate::proto::version::RakNetVersion;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;
use thiserror::Error;

pub const MIN_MTU: u16 = 576;
pub const MAX_MTU: u16 = 1492;

#[derive(Debug, Clone)]
pub struct Config {
    /// IPv4 socket. `None` disables IPv4 (and with it protocol-7 clients).
    pub ipv4: Option<SocketAddrV4>,
    /// IPv6 socket, bound with `IPV6_V6ONLY`. `None` disables IPv6.
    pub ipv6: Option<SocketAddrV6>,
    /// Random per run; sent in pongs and handshake replies.
    pub guid: u64,
    /// Accepted RakNet protocols; others get `0x19` with the highest. Non-empty.
    pub protocols: Vec<u8>,
    /// Ceiling for each session's negotiated MTU. [`MIN_MTU`]..=[`MAX_MTU`].
    /// This is allowed to be set above the maximum that the client supports to enable something
    /// like a proxy in the future.
    pub max_mtu: u16,
    /// Hard ceiling on connected sessions (after `0x09`). Excess gets `0x14`.
    pub max_sessions: usize,
    /// Half-open sessions (after `REPLY_2`, before `0x09`). 1..=10. Excess gets `0x14`.
    pub max_pending: usize,
    /// Pending + connected sessions per IP. Excess gets `0x14`.
    pub max_per_ip: usize,
    /// IP address that are allowed to bypass the connection limit set by max_per_ip.
    pub trusted_ips: Vec<IpAddr>,
    /// How long a half-open session may wait for `0x09`.
    pub pending_timeout: Duration,
    /// Silence before a connected session is dropped.
    pub timeout: Duration,
    /// Anti-spoof cookie in `REPLY_1`.
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
            max_pending: 10,
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

        if self.max_pending == 0 {
            return Err(ConfigError::InvalidValue {
                field: "max_pending",
                value: self.max_pending.to_string(),
                expected: "at least 1",
            });
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
