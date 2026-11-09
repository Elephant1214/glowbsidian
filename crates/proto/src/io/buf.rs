use crate::io::error::DecodeError;
use bytes::{Buf, BufMut, Bytes};

pub trait McBuf: Buf {
    fn try_get_bool(&mut self) -> Result<bool, DecodeError> {
        Ok(self.try_get_u8()? != 0)
    }

    fn try_get_uvarint(&mut self) -> Result<u32, DecodeError> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let b = self.try_get_u8()?;
            value |= ((b & 0x7F) as u32) << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(DecodeError::VarIntTooLong)
    }

    fn try_get_uvarint64(&mut self) -> Result<u64, DecodeError> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let b = self.try_get_u8()?;
            value |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(DecodeError::VarIntTooLong)
    }

    fn try_get_varint(&mut self) -> Result<i32, DecodeError> {
        let v = self.try_get_uvarint()?;
        Ok((v >> 1) as i32 ^ -((v & 1) as i32))
    }

    fn try_get_vbytes(&mut self) -> Result<Bytes, DecodeError> {
        let len = self.try_get_uvarint()? as usize;
        if len > self.remaining() {
            return Err(DecodeError::Eof);
        }
        Ok(self.copy_to_bytes(len))
    }

    fn try_get_string(&mut self) -> Result<String, DecodeError> {
        let raw = self.try_get_vbytes()?;
        String::from_utf8(raw.into()).map_err(|_| DecodeError::BadUtf8)
    }

    fn try_get_list_u16<T>(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Vec<T>, DecodeError> {
        let n = self.try_get_u16_le()? as usize;
        let mut out = Vec::with_capacity(n.min(self.remaining()));
        for _ in 0..n {
            out.push(f(self)?);
        }
        Ok(out)
    }

    fn try_get_list<T>(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Vec<T>, DecodeError> {
        let n = self.try_get_uvarint()? as usize;
        let mut out = Vec::with_capacity(n.min(self.remaining()));
        for _ in 0..n {
            out.push(f(self)?);
        }
        Ok(out)
    }
}

impl<T: Buf + ?Sized> McBuf for T {}

pub trait McBufMut: BufMut {
    fn put_bool(&mut self, value: bool) {
        self.put_u8(value as u8);
    }

    fn put_uvarint(&mut self, mut value: u32) {
        while value >= 0x80 {
            self.put_u8(value as u8 | 0x80);
            value >>= 7;
        }
        self.put_u8(value as u8);
    }

    fn put_uvarint64(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.put_u8(value as u8 | 0x80);
            value >>= 7;
        }
        self.put_u8(value as u8);
    }

    fn put_varint(&mut self, value: i32) {
        self.put_uvarint(((value << 1) ^ (value >> 31)) as u32);
    }

    fn put_vbytes(&mut self, bytes: &[u8]) {
        self.put_uvarint(bytes.len() as u32);
        self.put_slice(bytes);
    }

    fn put_string(&mut self, string: &str) {
        self.put_vbytes(string.as_bytes())
    }

    fn put_list_u16<T>(&mut self, items: &[T], mut f: impl FnMut(&mut Self, &T)) {
        self.put_u16_le(items.len() as u16);
        for item in items {
            f(self, item);
        }
    }

    fn put_list<T>(&mut self, items: &[T], mut f: impl FnMut(&mut Self, &T)) {
        self.put_uvarint(items.len() as u32);
        for item in items {
            f(self, item);
        }
    }
}

impl<T: BufMut + ?Sized> McBufMut for T {}
