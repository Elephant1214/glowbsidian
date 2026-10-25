use crate::data_types::U24;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum Reliability {
    Unreliable = 0,
    UnreliableSequenced = 1,
    Reliable = 2,
    ReliableOrdered = 3,
}

impl Reliability {
    pub fn is_reliable(&self) -> bool {
        matches!(self, Self::Reliable | Self::ReliableOrdered)
    }

    pub fn is_sequenced(&self) -> bool {
        matches!(self, Self::UnreliableSequenced)
    }

    /// Carries an ordering index + channel.
    pub fn has_order(&self) -> bool {
        matches!(self, Self::UnreliableSequenced | Self::ReliableOrdered)
    }

    /// RakNet sends split messages reliably, so unreliable ones get upgraded.
    pub fn for_split(self) -> Self {
        match self {
            Self::Unreliable => Self::Reliable,
            Self::UnreliableSequenced => Self::ReliableOrdered,
            r => r,
        }
    }
}

#[derive(Default)]
pub struct ReliableWindow {
    /// Everything below this has been received.
    base: U24,
    /// Received out of order, above `base`.
    above: HashSet<U24>,
}

/// Ignored indexes absurdly far ahead.
const WINDOW: u32 = 4096;

impl ReliableWindow {
    /// true = first time seen, deliver it. false = duplicate or absurdly far ahead, drop it.
    pub fn insert(&mut self, index: U24) -> bool {
        let ahead = index - self.base; // wrapping forward distance
        if ahead >= WINDOW {
            return false; // behind base (already delivered) or bogus
        }
        if ahead != 0 {
            return self.above.insert(index); // false if already there
        }
        self.base += 1;
        while self.above.remove(&self.base) {
            self.base += 1;
        }
        true
    }
}
