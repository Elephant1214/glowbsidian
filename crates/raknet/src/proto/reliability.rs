use num_enum::{IntoPrimitive, TryFromPrimitive};

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
