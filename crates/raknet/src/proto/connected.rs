use crate::proto::DecodeError;
use crate::proto::byte_ext::{RakBuf, RakBufMut};
use crate::proto::messages::{ConnectedId, ConnectedMessage};
use crate::proto::version::RakNetVersion;
use bytes::{Buf, BufMut, Bytes, BytesMut};

pub struct ConnectedCodec;

impl ConnectedCodec {
    pub fn decode(body: &Bytes, version: RakNetVersion) -> Result<ConnectedMessage, DecodeError> {
        let mut reader = &body[..];

        let msg = match reader.try_get_id::<ConnectedId>()? {
            ConnectedId::ConnectedPing => ConnectedMessage::ConnectedPing {
                time: reader.try_get_u64()?,
            },
            ConnectedId::ConnectedPong => {
                let ping_time = reader.try_get_u64()?;
                let pong_time = reader.try_get_u64()?;
                ConnectedMessage::ConnectedPong {
                    ping_time,
                    pong_time,
                }
            }
            ConnectedId::DetectLostConnections => ConnectedMessage::DetectLostConnections,
            ConnectedId::ConnectionRequest => {
                let client_guid = reader.try_get_u64()?;
                let time = reader.try_get_u64()?;
                if reader.try_get_bool()? {
                    return Err(DecodeError::Malformed(
                        "connection request asked for security",
                    ));
                }
                ConnectedMessage::ConnectionRequest { client_guid, time }
            }
            ConnectedId::NewIncomingConnection => {
                let server_addr = reader.try_get_address_for(version)?;
                let internal = (0..version.internal_ids)
                    .map(|_| reader.try_get_address_for(version))
                    .collect::<Result<Vec<_>, _>>()?;
                let ping_time = reader.try_get_u64()?;
                let pong_time = reader.try_get_u64()?;

                ConnectedMessage::NewIncomingConnection {
                    server_addr,
                    internal,
                    ping_time,
                    pong_time,
                }
            }
            ConnectedId::Disconnect => ConnectedMessage::Disconnect,
            // Slice removes the protocol prefix from the data that gets forwarded to an actual server
            ConnectedId::Game => return Ok(ConnectedMessage::Game(body.clone())),
            other => return Err(DecodeError::UnknownId(other.into())),
        };

        if !reader.is_empty() {
            return Err(DecodeError::Malformed("trailing bytes"));
        }

        Ok(msg)
    }

    pub fn encode(msg: &ConnectedMessage, out: &mut BytesMut) {
        match msg {
            ConnectedMessage::ConnectedPing { time } => {
                out.put_id(ConnectedId::ConnectedPing);
                out.put_u64(*time);
            }
            ConnectedMessage::ConnectedPong {
                ping_time,
                pong_time,
            } => {
                out.put_id(ConnectedId::ConnectedPong);
                out.put_u64(*ping_time);
                out.put_u64(*pong_time);
            }
            ConnectedMessage::ConnectionRequestAccepted {
                client_addr,
                internal,
                request_time,
                time,
            } => {
                out.put_id(ConnectedId::ConnectionRequestAccepted);
                out.put_address(client_addr);
                out.put_u16(0); // system index
                for addr in internal {
                    out.put_address(addr);
                }
                out.put_u64(*request_time);
                out.put_u64(*time);
            }
            ConnectedMessage::Disconnect => out.put_id(ConnectedId::Disconnect),
            ConnectedMessage::Game(data) => out.put_slice(data),
            _ => unreachable!("client-bound only"),
        }
    }
}
