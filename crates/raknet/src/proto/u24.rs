use std::ops::{Add, AddAssign, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct U24(u32);

impl U24 {
    pub const MASK: u32 = 0xFF_FFFF;

    pub fn new(value: u32) -> Self {
        Self(value & Self::MASK)
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn next(self) -> Self {
        Self((self.0 + 1) & Self::MASK)
    }

    /// How far `self` is ahead of `other`, wrapping.
    pub fn ahead_of(self, other: Self) -> u32 {
        self.0.wrapping_sub(other.0) & Self::MASK
    }

    /// True if `self` comes after `other` (within half the range).
    pub fn is_after(self, other: Self) -> bool {
        let d = self.ahead_of(other);
        d != 0 && d < 0x80_0000
    }
}

#[expect(
    clippy::suspicious_arithmetic_impl,
    reason = "the mask is the 24-bit wrap"
)]
impl Add<u32> for U24 {
    type Output = U24;
    fn add(self, rhs: u32) -> U24 {
        U24(self.0.wrapping_add(rhs) & U24::MASK)
    }
}

impl AddAssign<u32> for U24 {
    fn add_assign(&mut self, rhs: u32) {
        *self = *self + rhs;
    }
}

/// Distance from `rhs` forward to `self`, wrapping (same as `ahead_of`).
#[expect(
    clippy::suspicious_arithmetic_impl,
    reason = "the mask is the 24-bit wrap"
)]
impl Sub for U24 {
    type Output = u32;
    fn sub(self, rhs: U24) -> u32 {
        self.0.wrapping_sub(rhs.0) & U24::MASK
    }
}

#[cfg(test)]
mod tests;
