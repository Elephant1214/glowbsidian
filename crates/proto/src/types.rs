use serde::{Deserialize, Deserializer, de};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Xuid(pub u64);

impl FromStr for Xuid {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Xuid)
    }
}

impl<'de> Deserialize<'de> for Xuid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <&str>::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}
