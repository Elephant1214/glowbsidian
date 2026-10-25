use crate::data_types::U24;
use crate::proto::DecodeError;
use crate::proto::byte_ext::{RakBuf, RakBufMut};
use crate::proto::reliable::Reliability;
use bytes::{Buf, BufMut, Bytes, BytesMut};

#[derive(Debug, Clone)]
pub struct Frame {
    pub reliability: Reliability,
    /// Present if reliability is reliable or reliable ordered (u24)
    pub reliable_index: Option<U24>,
    /// Present if reliability is unreliable sequenced (u24)
    pub sequence_index: Option<U24>,
    /// (ordering index, channel) if reliability is unreliable sequenced or reliable ordered
    pub order: Option<(U24, u8)>,
    pub split: Option<SplitInfo>,
    pub body: Bytes,
}

#[derive(Debug, Clone, Copy)]
pub struct SplitInfo {
    pub count: u32,
    pub id: u16,
    pub index: u32,
}

impl Frame {
    pub fn decode(buf: &mut Bytes) -> Result<Frame, DecodeError> {
        let flags = buf.try_get_u8()?;
        let bits = flags >> 5;
        let reliability =
            Reliability::try_from(bits).map_err(|_| DecodeError::BadReliability(bits))?;

        let len = (buf.try_get_u16()? as usize).div_ceil(8); // length in bits
        if len == 0 {
            return Err(DecodeError::Malformed("empty frame"));
        }

        let reliable_index = if reliability.is_reliable() {
            Some(buf.try_get_u24_le()?)
        } else {
            None
        };

        let sequence_index = if reliability.is_sequenced() {
            Some(buf.try_get_u24_le()?)
        } else {
            None
        };

        let order = if reliability.has_order() {
            let index = buf.try_get_u24_le()?;
            let channel = buf.try_get_u8()?;
            if channel >= 32 {
                return Err(DecodeError::Malformed("channel >= 32"));
            }
            Some((index, channel))
        } else {
            None
        };

        let split = if flags & 0x10 != 0 {
            let count = buf.try_get_u32()?;
            let id = buf.try_get_u16()?;
            let index = buf.try_get_u32()?;
            if index >= count {
                return Err(DecodeError::Malformed("split index >= count"));
            }
            Some(SplitInfo { count, id, index })
        } else {
            None
        };

        if buf.remaining() < len {
            return Err(DecodeError::Eof);
        }
        let body = buf.split_to(len);
        Ok(Frame {
            reliability,
            reliable_index,
            sequence_index,
            order,
            split,
            body,
        })
    }

    pub fn encode(&self, out: &mut BytesMut) {
        let split_flag = if self.split.is_some() { 0x10 } else { 0x00 };
        out.put_u8((u8::from(self.reliability) << 5) | split_flag);
        out.put_u16((self.body.len() * 8) as u16);
        if let Some(i) = self.reliable_index {
            out.put_u24_le(i);
        }
        if let Some(i) = self.sequence_index {
            out.put_u24_le(i);
        }
        if let Some((i, channel)) = self.order {
            out.put_u24_le(i);
            out.put_u8(channel);
        }
        if let Some(split) = self.split {
            out.put_u32(split.count);
            out.put_u16(split.id);
            out.put_u32(split.index);
        }
        out.put_slice(&self.body);
    }

    /// Bytes this frame takes in a datagram so that `send` can pack datagrams up to the MTU.
    pub fn encoded_len(&self) -> usize {
        3 // flags + u16 bit len
            + self.reliable_index.map_or(0, |_| 3)
            + self.sequence_index.map_or(0, |_| 3)
            + self.order.map_or(0, |_| 4)
            + self.split.map_or(0, |_| 10)
            + self.body.len()
    }
}
