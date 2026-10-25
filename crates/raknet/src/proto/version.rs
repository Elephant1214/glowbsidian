/// Wire details that differ between RakNet protocol versions.
#[derive(Clone, Copy, Debug)]
pub struct RakNetVersion {
    pub internal_ids: usize,
    pub ipv6: bool,
}

impl RakNetVersion {
    pub fn for_protocol(protocol: u8) -> Option<Self> {
        match protocol {
            ..=7 => Some(Self {
                internal_ids: 10,
                ipv6: false,
            }),
            8 => Some(Self {
                internal_ids: 10,
                ipv6: true,
            }),
            _ => None,
        }
    }
}
