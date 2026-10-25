use crate::peer::{DisconnectReason, PeerId};
use crate::proto::reliable::Reliability;
use bytes::Bytes;
use std::net::SocketAddr;

#[derive(Debug)]
pub enum Event {
    /// Client sent NEW_INCOMING_CONNECTION (0x13)
    Connected {
        peer: PeerId,
        addr: SocketAddr,
        protocol: u8,
    },
    /// A complete message with any splits reassembled
    Message { peer: PeerId, data: Bytes },
    Disconnected {
        peer: PeerId,
        reason: DisconnectReason,
    },
}

pub enum Command {
    Send {
        peer: PeerId,
        data: Bytes,
        reliability: Reliability,
    },
    Disconnect(PeerId),
    SetPong(String),
}
