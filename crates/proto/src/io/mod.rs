use crate::io::error::DecodeError;
use bytes::{Bytes, BytesMut};

pub mod buf;
pub mod error;
pub mod zlib;

pub trait Packet: Sized {
    const ID: u8;
    fn decode(buf: &mut Bytes) -> Result<Self, DecodeError>;
    fn encode(&self, buf: &mut BytesMut);
}
