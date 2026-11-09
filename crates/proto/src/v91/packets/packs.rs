use crate::io::Packet;
use crate::io::buf::{McBuf, McBufMut};
use crate::io::error::DecodeError;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use uuid::Uuid;

#[derive(Debug)]
pub struct PackInfoEntry {
    pub id: Uuid,
    pub version: String,
    pub size: u64,
}

impl PackInfoEntry {
    fn decode(buf: &mut impl Buf) -> Result<Self, DecodeError> {
        let id = Uuid::parse_str(&buf.try_get_string()?)
            .map_err(|_| DecodeError::Malformed("bad pack UUID"))?;
        Ok(Self {
            id,
            version: buf.try_get_string()?,
            size: buf.try_get_u64_le()?,
        })
    }

    fn encode(&self, buf: &mut impl BufMut) {
        buf.put_string(&self.id.hyphenated().to_string());
        buf.put_string(&self.version);
        buf.put_u64_le(self.size);
    }
}

#[derive(Debug)]
pub struct ResourcePacksInfo {
    pub must_accept: bool,
    pub behavior_packs: Vec<PackInfoEntry>,
    pub resource_packs: Vec<PackInfoEntry>,
}

impl Packet for ResourcePacksInfo {
    const ID: u8 = 0x07;

    fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        Ok(Self {
            must_accept: buf.try_get_bool()?,
            behavior_packs: buf.try_get_list_u16(PackInfoEntry::decode)?,
            resource_packs: buf.try_get_list_u16(PackInfoEntry::decode)?,
        })
    }

    fn encode(&self, buf: &mut BytesMut) {
        buf.put_bool(self.must_accept);
        buf.put_list_u16(&self.behavior_packs, |b, e| e.encode(b));
        buf.put_list_u16(&self.resource_packs, |b, e| e.encode(b));
    }
}

#[derive(Debug)]
pub struct PackStackEntry {
    pub id: Uuid,
    pub version: String,
}

impl PackStackEntry {
    fn decode(buf: &mut impl Buf) -> Result<Self, DecodeError> {
        let id = Uuid::parse_str(&buf.try_get_string()?)
            .map_err(|_| DecodeError::Malformed("bad pack UUID"))?;
        Ok(Self {
            id,
            version: buf.try_get_string()?,
        })
    }

    fn encode(&self, buf: &mut impl BufMut) {
        buf.put_string(&self.id.hyphenated().to_string());
        buf.put_string(&self.version);
    }
}

#[derive(Debug)]
pub struct ResourcePackStack {
    pub must_accept: bool,
    pub behavior_packs: Vec<PackStackEntry>,
    pub resource_packs: Vec<PackStackEntry>,
}

impl Packet for ResourcePackStack {
    const ID: u8 = 0x08;

    fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        Ok(Self {
            must_accept: buf.try_get_bool()?,
            behavior_packs: buf.try_get_list_u16(PackStackEntry::decode)?,
            resource_packs: buf.try_get_list_u16(PackStackEntry::decode)?,
        })
    }

    fn encode(&self, buf: &mut BytesMut) {
        buf.put_bool(self.must_accept);
        buf.put_list_u16(&self.behavior_packs, |b, e| e.encode(b));
        buf.put_list_u16(&self.resource_packs, |b, e| e.encode(b));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum PackResponseStatus {
    Refused = 1,
    SendPacks = 2,
    HaveAllPacks = 3,
    Completed = 4,
}

#[derive(Debug)]
pub struct ResourcePackClientResponse {
    pub status: PackResponseStatus,
    pub pack_ids: Vec<Uuid>,
}

impl Packet for ResourcePackClientResponse {
    const ID: u8 = 0x09;

    fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        let status = PackResponseStatus::try_from(buf.try_get_u8()?)
            .map_err(|_| DecodeError::Malformed("invalid pack response status"))?;
        let pack_ids = buf.try_get_list_u16(|buf| {
            Uuid::parse_str(&buf.try_get_string()?)
                .map_err(|_| DecodeError::Malformed("bad pack UUID"))
        })?;

        Ok(Self { status, pack_ids })
    }

    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u8(self.status.into());
        buf.put_list_u16(&self.pack_ids, |buf, id| buf.put_string(&id.to_string()));
    }
}
