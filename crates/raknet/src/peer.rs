/// A handle for a connected client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerId(u32);

impl PeerId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectReason {
    /// Client violated RakNet rules
    Protocol,
    /// Client sent DISCONNECTION_NOTIFICATION (0x15)
    ClientDisconnected,
    /// Nothing received for ~10s
    TimedOut,
    /// Server disconnected the client
    Kicked,
}
