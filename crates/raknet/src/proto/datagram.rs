use crate::data_types::U24;
use crate::proto::DecodeError;
use crate::proto::byte_ext::{RakBuf, RakBufMut};
use crate::proto::frame::Frame;
use bytes::{Buf, BufMut, Bytes, BytesMut};

/// Flags + u24 sequence
pub const DATAGRAM_HEADER_LEN: usize = 4;
const DATA_FLAGS: u8 = 0x84;
const ACK: u8 = 0xC0;
const NAK: u8 = 0xA0;

pub enum Datagram {
    Data { seq: U24, frames: Vec<Frame> },
    Ack(Vec<(U24, U24)>),
    Nak(Vec<(U24, U24)>),
}

impl Datagram {
    pub fn decode(mut buf: Bytes) -> Result<Self, DecodeError> {
        let flags = buf.try_get_u8()?;
        if flags & 0x40 != 0 {
            if flags & 0x20 != 0 {
                // Unused arrival rate provided by RakNet
                buf.try_get_f32()?;
            }
            return Ok(Datagram::Ack(decode_ranges(&mut buf)?));
        }
        if flags & 0x20 != 0 {
            return Ok(Datagram::Nak(decode_ranges(&mut buf)?));
        }
        let seq = buf.try_get_u24_le()?;
        let mut frames = Vec::new();

        while buf.has_remaining() {
            // A bad frame ends parsing but keeps the ones before it and the packet is still ACKed
            // so that the client doesn't spam a malformed frame forever
            match Frame::decode(&mut buf) {
                Ok(frame) => frames.push(frame),
                Err(_) => break,
            }
        }
        Ok(Datagram::Data { seq, frames })
    }
}

pub fn put_data_header(out: &mut BytesMut, seq: U24) {
    out.put_u8(DATA_FLAGS);
    out.put_u24_le(seq);
}

pub fn encode_ranges(nak: bool, ranges: &[(U24, U24)], out: &mut BytesMut) {
    out.put_u8(if nak { NAK } else { ACK });
    out.put_u16(ranges.len() as u16);

    for &(min, max) in ranges {
        if min == max {
            out.put_bool(true);
            out.put_u24_le(min);
        } else {
            out.put_bool(false);
            out.put_u24_le(min);
            out.put_u24_le(max);
        }
    }
}

fn decode_ranges(buf: &mut Bytes) -> Result<Vec<(U24, U24)>, DecodeError> {
    let count = buf.try_get_u16()?;

    let mut ranges = Vec::with_capacity(count.min(256) as usize);
    for _ in 0..count {
        let single = buf.try_get_u8()? != 0;
        let min = buf.try_get_u24_le()?;
        let max = if single { min } else { buf.try_get_u24_le()? };
        ranges.push((min, max));
    }

    Ok(ranges)
}

/// Collapses sequence numbers into (min, max) runs
pub fn to_ranges(mut seqs: Vec<U24>) -> Vec<(U24, U24)> {
    seqs.sort_unstable_by_key(|s| s.get());
    seqs.dedup();

    let mut out = Vec::<(U24, U24)>::new();
    for s in seqs {
        match out.last_mut() {
            Some((_, max)) if max.next() == s => *max = s,
            _ => out.push((s, s)),
        }
    }
    out
}
