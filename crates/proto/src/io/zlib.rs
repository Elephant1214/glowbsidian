use crate::io::error::DecodeError;
use bytes::Bytes;
use flate2::{Compression, read, write};
use std::io::{Read, Write};

/// Upper bound for anything the client sends (after inflation).
pub const MAX_INFLATED: usize = 2 * 1024 * 1024;

pub fn inflate(data: &[u8], limit: usize) -> Result<Bytes, DecodeError> {
    let mut out = Vec::new();
    read::ZlibDecoder::new(data)
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| DecodeError::Malformed("bad zlib stream"))?;
    if out.len() > limit {
        return Err(DecodeError::Malformed("inflated data too large"));
    }
    Ok(out.into())
}

pub fn deflate(data: &[u8]) -> Bytes {
    let mut encoder =
        write::ZlibEncoder::new(Vec::with_capacity(data.len() / 2), Compression::default());
    encoder
        .write_all(data)
        .expect("writing to a Vec can't fail");
    encoder
        .finish()
        .expect("writing to a Vec can't fail")
        .into()
}
