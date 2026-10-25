use bytes::Bytes;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq)]
pub enum OfflineMessage {
    UnconnectedPing {
        time: u64,
        client_guid: u64,
        open_only: bool,
    },
    UnconnectedPong {
        time: u64,
        server_guid: u64,
        data: Bytes,
    },
    OpenConnectionRequest1 {
        protocol: u8,
        mtu: u16,
    },
    OpenConnectionReply1 {
        server_guid: u64,
        cookie: Option<u32>,
        mtu: u16,
    },
    OpenConnectionRequest2 {
        cookie: Option<u32>,
        server_addr: SocketAddr,
        mtu: u16,
        client_guid: u64,
    },
    OpenConnectionReply2 {
        server_guid: u64,
        client_addr: SocketAddr,
        mtu: u16,
    },
    IncompatibleProtocol {
        protocol: u8,
        server_guid: u64,
    },
    NoFreeIncomingConnections {
        server_guid: u64,
    },
}

impl OfflineMessage {
    pub fn id(&self) -> OfflineId {
        match self {
            Self::UnconnectedPing {
                open_only: false, ..
            } => OfflineId::UnconnectedPing,
            Self::UnconnectedPing {
                open_only: true, ..
            } => OfflineId::UnconnectedPingOpenConnections,
            Self::OpenConnectionRequest1 { .. } => OfflineId::OpenConnectionRequest1,
            Self::OpenConnectionReply1 { .. } => OfflineId::OpenConnectionReply1,
            Self::OpenConnectionRequest2 { .. } => OfflineId::OpenConnectionRequest2,
            Self::OpenConnectionReply2 { .. } => OfflineId::OpenConnectionReply2,
            Self::NoFreeIncomingConnections { .. } => OfflineId::NoFreeIncomingConnections,
            Self::IncompatibleProtocol { .. } => OfflineId::IncompatibleProtocol,
            Self::UnconnectedPong { .. } => OfflineId::UnconnectedPong,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum OfflineId {
    UnconnectedPing = 0x01,
    UnconnectedPingOpenConnections = 0x02,
    OpenConnectionRequest1 = 0x05,
    OpenConnectionReply1 = 0x06,
    OpenConnectionRequest2 = 0x07,
    OpenConnectionReply2 = 0x08,
    NoFreeIncomingConnections = 0x14,
    IncompatibleProtocol = 0x19,
    UnconnectedPong = 0x1C,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectedMessage {
    ConnectedPing {
        time: u64,
    },
    ConnectedPong {
        ping_time: u64,
        pong_time: u64,
    },
    ConnectionRequest {
        client_guid: u64,
        time: u64,
    },
    ConnectionRequestAccepted {
        client_addr: SocketAddr,
        internal: Vec<SocketAddr>,
        request_time: u64,
        time: u64,
    },
    NewIncomingConnection {
        server_addr: SocketAddr,
        internal: Vec<SocketAddr>,
        ping_time: u64,
        pong_time: u64,
    },
    DetectLostConnections,
    Disconnect,
    Game(Bytes),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum ConnectedId {
    ConnectedPing = 0x00,
    ConnectedPong = 0x03,
    DetectLostConnections = 0x04,
    ConnectionRequest = 0x09,
    ConnectionRequestAccepted = 0x10,
    NewIncomingConnection = 0x13,
    Disconnect = 0x15,
}
