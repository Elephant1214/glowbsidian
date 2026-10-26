use crate::proto::u24::U24;
use std::collections::HashSet;

#[derive(Default)]
pub struct ReliableWindow {
    /// Everything below this has been received.
    base: U24,
    /// Received out of order, above `base`.
    above: HashSet<U24>,
}

/// Ignored indexes absurdly far ahead.
const RELIABLE_WINDOW: u32 = 4096;

impl ReliableWindow {
    /// true = first time seen, deliver it. false = duplicate or absurdly far ahead
    pub fn insert(&mut self, index: U24) -> bool {
        let ahead = index - self.base; // wrapping forward distance
        if ahead >= RELIABLE_WINDOW {
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

#[cfg(test)]
mod tests;
