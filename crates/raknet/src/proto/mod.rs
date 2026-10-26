mod buf;
pub mod connected;
pub mod datagram;
pub mod frame;
pub mod offline;
pub mod reliability;
pub mod u24;
pub mod version;

use crate::proto::offline::OfflineId;
use bytes::TryGetError;
use num_enum::TryFromPrimitiveError;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use thiserror::Error;

/// What 0x10's internal addresses are filled with (IPv4)
pub const INTERNAL_PLACEHOLDER_V4: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, 0));

/// What 0x10's internal addresses are filled with (IPv6)
pub const INTERNAL_PLACEHOLDER_V6: SocketAddr =
    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0));

pub const IP_UDP_HEADER_LEN: usize = 28;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unknown id {0:#04x}")]
    UnknownId(u8),
    #[error("unexpected end of packet")]
    Eof,
    #[error("bad offline magic")]
    BadMagic,
    #[error("bad reliable id: {0}")]
    BadReliability(u8),
    #[error("malformed data: {0}")]
    Malformed(&'static str),
}

impl From<TryFromPrimitiveError<OfflineId>> for DecodeError {
    fn from(value: TryFromPrimitiveError<OfflineId>) -> Self {
        DecodeError::UnknownId(value.number)
    }
}

impl From<TryGetError> for DecodeError {
    fn from(_: TryGetError) -> Self {
        DecodeError::Eof
    }
}
