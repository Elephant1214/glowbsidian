use crate::proto::byte_ext::{RakBuf, RakBufMut};
use crate::proto::messages::{OfflineId, OfflineMessage};
use crate::proto::{DecodeError, IP_UDP_HEADER_LEN};
use bytes::{Buf, BufMut, BytesMut};

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
