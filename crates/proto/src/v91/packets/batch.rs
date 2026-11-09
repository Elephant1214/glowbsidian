use crate::io::Packet;
use crate::io::buf::{McBuf, McBufMut};
use crate::io::error::DecodeError;
use crate::io::zlib::{MAX_INFLATED, deflate, inflate};
use bytes::{Buf, Bytes, BytesMut};

pub struct Batch {
    /// Zlib compressed bytes
    pub payload: Bytes,
}

impl Batch {
    /// Each entry is a single packet, ID + body
    pub fn unpack(&self) -> Result<Vec<Bytes>, DecodeError> {
        let mut data = inflate(&self.payload, MAX_INFLATED)?;
        let mut packets = Vec::new();
        while data.has_remaining() {
            packets.push(data.try_get_vbytes()?);
        }
        Ok(packets)
    }

    /// `packets` are already encoded as ID + body
    pub fn pack(packets: &[Bytes]) -> Self {
        let mut raw = BytesMut::new();
        for packet in packets {
            raw.put_vbytes(packet);
        }
        Self {
            payload: deflate(&raw),
        }
    }
}

impl Packet for Batch {
    const ID: u8 = 0x06;

    fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        Ok(Self {
            payload: buf.try_get_vbytes()?,
        })
    }

    fn encode(&self, buf: &mut BytesMut) {
        buf.put_vbytes(&self.payload);
    }
}
