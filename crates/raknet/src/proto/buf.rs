use crate::proto::DecodeError;
use crate::proto::u24::U24;
use crate::proto::version::RakNetVersion;
use bytes::{Buf, BufMut, TryGetError};
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};

pub const MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];

pub trait RakBuf: Buf {
    fn try_get_bool(&mut self) -> Result<bool, TryGetError> {
        Ok(self.try_get_u8()? != 0)
    }

    fn try_get_u24_le(&mut self) -> Result<U24, TryGetError> {
        Ok(U24::new(self.try_get_uint_le(3)? as u32))
    }

    fn try_get_magic(&mut self) -> Result<bool, TryGetError> {
        let mut m = [0u8; 16];
        self.try_copy_to_slice(&mut m)?;
        Ok(m == MAGIC)
    }

    fn try_get_address_for(&mut self, version: RakNetVersion) -> Result<SocketAddr, DecodeError> {
        let addr = self.try_get_address()?;
        if addr.is_ipv6() && !version.ipv6 {
            return Err(DecodeError::Malformed(
                "ipv6 address on an ipv4-only protocol",
            ));
        }
        Ok(addr)
    }

    fn try_get_address(&mut self) -> Result<SocketAddr, DecodeError> {
        match self.try_get_u8()? {
            4 => {
                let mut ip = [0u8; 4];
                self.try_copy_to_slice(&mut ip)?;
                let port = self.try_get_u16()?;
                Ok(SocketAddr::V4(SocketAddrV4::new(
                    ip.map(|b| !b).into(),
                    port,
                )))
            }
            6 => {
                self.try_get_u16_le()?; // OS-specific family, ignored
                let port = self.try_get_u16()?;
                let flowinfo = self.try_get_u32()?;
                let mut ip = [0u8; 16];
                self.try_copy_to_slice(&mut ip)?;
                let scope_id = self.try_get_u32_le()?;
                Ok(SocketAddr::V6(SocketAddrV6::new(
                    ip.into(),
                    port,
                    flowinfo,
                    scope_id,
                )))
            }
            _ => Err(DecodeError::Malformed("bad address version")),
        }
    }
}

impl<T: Buf + ?Sized> RakBuf for T {}

pub trait RakBufMut: BufMut {
    fn put_id(&mut self, id: impl Into<u8>) {
        self.put_u8(id.into());
    }
    fn put_bool(&mut self, value: bool) {
        self.put_u8(value as u8);
    }

    fn put_u24_le(&mut self, value: U24) {
        self.put_uint_le(value.get() as u64, 3);
    }

    fn put_magic(&mut self) {
        self.put_slice(&MAGIC);
    }

    fn put_address(&mut self, addr: &SocketAddr) {
        match addr {
            SocketAddr::V4(v4) => {
                self.put_u8(4);
                self.put_slice(&v4.ip().octets().map(|b| !b));
                self.put_u16(v4.port());
            }
            SocketAddr::V6(v6) => {
                self.put_u8(6);
                self.put_u16_le(10);
                self.put_u16(v6.port());
                self.put_u32(v6.flowinfo());
                self.put_slice(&v6.ip().octets());
                self.put_u32_le(v6.scope_id());
            }
        }
    }
}

impl<T: BufMut + ?Sized> RakBufMut for T {}
