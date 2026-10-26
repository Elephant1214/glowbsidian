use crate::proto::buf::{RakBuf, RakBufMut};
use crate::proto::{DecodeError, IP_UDP_HEADER_LEN};
use bytes::{Buf, BufMut, Bytes, BytesMut};
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

pub struct OfflineCodec {
    pub cookies: bool,
}

impl OfflineCodec {
    pub fn decode(&self, data: &[u8]) -> Result<OfflineMessage, DecodeError> {
        let mut reader = data;
        match OfflineId::try_from(reader.try_get_u8()?)? {
            id @ (OfflineId::UnconnectedPing | OfflineId::UnconnectedPingOpenConnections) => {
                let open_only = id == OfflineId::UnconnectedPingOpenConnections;
                let time = reader.try_get_u64()?;
                if !reader.try_get_magic()? {
                    return Err(DecodeError::BadMagic);
                }
                Ok(OfflineMessage::UnconnectedPing {
                    time,
                    client_guid: reader.try_get_u64()?,
                    open_only,
                })
            }
            OfflineId::OpenConnectionRequest1 => {
                if !reader.try_get_magic()? {
                    return Err(DecodeError::BadMagic);
                }
                let protocol = reader.try_get_u8()?;
                if !reader.iter().all(|&b| b == 0) {
                    return Err(DecodeError::Malformed("request 1 padding isn't zero"));
                }

                let mtu = (data.len() + IP_UDP_HEADER_LEN).min(u16::MAX as usize) as u16;
                Ok(OfflineMessage::OpenConnectionRequest1 { protocol, mtu })
            }
            OfflineId::OpenConnectionRequest2 => {
                if !reader.try_get_magic()? {
                    return Err(DecodeError::BadMagic);
                }
                let cookie = if self.cookies {
                    let cookie = reader.try_get_u32()?;
                    // This is for RakNet's encryption, libcat, which Mojang chose not to use
                    if reader.try_get_bool()? {
                        return Err(DecodeError::Malformed("request 2 has a security challenge"));
                    }
                    Some(cookie)
                } else {
                    None
                };
                let server_addr = reader.try_get_address()?;
                let mtu = reader.try_get_u16()?;
                let client_guid = reader.try_get_u64()?;
                Ok(OfflineMessage::OpenConnectionRequest2 {
                    cookie,
                    server_addr,
                    mtu,
                    client_guid,
                })
            }
            other => Err(DecodeError::UnknownId(other.into())),
        }
    }

    pub fn encode(&self, msg: &OfflineMessage, out: &mut BytesMut) {
        out.put_u8(msg.id().into());
        match msg {
            OfflineMessage::UnconnectedPong {
                time,
                server_guid,
                data,
            } => {
                out.put_u64(*time);
                out.put_u64(*server_guid);
                out.put_magic();
                out.put_u16(data.len() as u16);
                out.put_slice(data);
            }
            OfflineMessage::OpenConnectionReply1 {
                server_guid,
                cookie,
                mtu,
            } => {
                out.put_magic();
                out.put_u64(*server_guid);
                match cookie {
                    Some(c) => {
                        out.put_bool(true);
                        out.put_u32(*c);
                    }
                    None => out.put_bool(false),
                }
                out.put_u16(*mtu);
            }
            OfflineMessage::OpenConnectionReply2 {
                server_guid,
                client_addr,
                mtu,
            } => {
                out.put_magic();
                out.put_u64(*server_guid);
                out.put_address(client_addr);
                out.put_u16(*mtu);
                // true here would enable libcat encryption, which is unsupported
                out.put_bool(false);
            }
            OfflineMessage::IncompatibleProtocol {
                protocol,
                server_guid,
            } => {
                out.put_u8(*protocol);
                out.put_magic();
                out.put_u64(*server_guid);
            }
            OfflineMessage::NoFreeIncomingConnections { server_guid } => {
                out.put_magic();
                out.put_u64(*server_guid);
            }
            _ => unreachable!("client-bound only"),
        }
    }
}
