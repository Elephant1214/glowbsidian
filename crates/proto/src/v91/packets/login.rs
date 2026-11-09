use crate::io::Packet;
use crate::io::buf::{McBuf, McBufMut};
use crate::io::error::DecodeError;
use crate::io::zlib::{MAX_INFLATED, deflate, inflate};
use crate::types::Xuid;
use crate::v91::PROTOCOL;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::{Engine, alphabet};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_with::{NoneAsEmptyString, serde_as};
use uuid::Uuid;

/// JWTs use base64url without padding
const B64_URL: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::RequireNone),
);
/// x5u and SkinData use plain base64.
const B64: GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum Edition {
    Normal = 0,
    Education = 1,
}

#[derive(Debug)]
pub struct Login {
    pub protocol: i32,
    pub edition: Edition,
    pub request: Option<ConnectionRequest>,
}

#[derive(Debug)]
pub struct ConnectionRequest {
    /// Raw tokens needed for signature checks later
    pub chain: Vec<String>,
    pub client_data_jwt: String,
    pub identity_public_key: String,
    pub identity: Identity,
    pub client: ClientData,
}

#[serde_as]
#[derive(Debug, Deserialize)]
pub struct Identity {
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "identity")]
    pub uuid: Uuid,
    #[serde_as(as = "NoneAsEmptyString")]
    #[serde(rename = "XUID", default)]
    pub xuid: Option<Xuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ClientData {
    pub skin_id: String,
    #[serde(deserialize_with = "base64_bytes")]
    pub skin_data: Vec<u8>,
    pub client_random_id: i64,
    pub server_address: String,
}

impl Packet for Login {
    const ID: u8 = 0x01;

    fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        let protocol = buf.try_get_i32()?;
        if protocol != PROTOCOL {
            return Ok(Self {
                protocol,
                edition: Edition::Normal,
                request: None,
            });
        }
        let edition = Edition::try_from(buf.try_get_u8()?)
            .map_err(|_| DecodeError::Malformed("invalid game edition"))?;
        let payload = buf.try_get_vbytes()?;
        Ok(Self {
            protocol,
            edition,
            request: Some(ConnectionRequest::decode(&payload)?),
        })
    }

    fn encode(&self, buf: &mut BytesMut) {
        buf.put_i32(self.protocol);
        if let Some(req) = &self.request {
            buf.put_u8(self.edition.into());
            buf.put_vbytes(&req.encode());
        }
    }
}

impl ConnectionRequest {
    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut raw = inflate(payload, MAX_INFLATED)?;
        let chain_json = read_long_string(&mut raw)?;
        let client_data_jwt = String::from_utf8(read_long_string(&mut raw)?.into())
            .map_err(|_| DecodeError::BadUtf8)?;

        let chain = serde_json::from_slice::<ChainJson>(&chain_json)?.chain;

        let mut identity = None;
        let mut identity_public_key = None;
        for token in &chain {
            let claims: ChainClaims = jwt_claims(token)?;
            if claims.extra_data.is_some() {
                identity = claims.extra_data;
            }
            identity_public_key = Some(claims.identity_public_key);
        }

        Ok(Self {
            identity: identity.ok_or(DecodeError::Malformed("no extraData in chain"))?,
            identity_public_key: identity_public_key
                .ok_or(DecodeError::Malformed("empty chain"))?,
            client: jwt_claims(&client_data_jwt)?,
            chain,
            client_data_jwt,
        })
    }

    fn encode(&self) -> Bytes {
        let data = ChainJson {
            chain: self.chain.clone(),
        };
        let chain_json = serde_json::to_vec(&data).expect("serializing to strings can't fail");
        let mut raw = BytesMut::new();
        raw.put_i32_le(chain_json.len() as i32);
        raw.put_slice(&chain_json);
        raw.put_i32_le(self.client_data_jwt.len() as i32);
        raw.put_slice(self.client_data_jwt.as_bytes());
        deflate(&raw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive, IntoPrimitive)]
#[repr(i32)]
pub enum PlayStatusType {
    LoginSuccess = 0,
    ClientOutdated = 1,
    ServerOutdated = 2,
    PlayerSpawn = 3,
    EditionMismatch = 5,
}

#[derive(Debug)]
pub struct PlayStatus {
    pub status: PlayStatusType,
}

impl Packet for PlayStatus {
    const ID: u8 = 0x02;

    fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        let status = PlayStatusType::try_from(buf.try_get_i32()?)
            .map_err(|_| DecodeError::Malformed("invalid player status"))?;
        Ok(Self { status })
    }

    fn encode(&self, buf: &mut BytesMut) {
        buf.put_i32(self.status.into());
    }
}

#[derive(Debug)]
pub struct Disconnect {
    pub message: Option<String>,
}

impl Packet for Disconnect {
    const ID: u8 = 0x05;

    fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        let hide_screen = buf.try_get_bool()?;
        let message = if hide_screen {
            None
        } else {
            Some(buf.try_get_string()?)
        };
        Ok(Self { message })
    }

    fn encode(&self, buf: &mut BytesMut) {
        buf.put_bool(self.message.is_none());
        if let Some(msg) = &self.message {
            buf.put_string(msg);
        }
    }
}

#[derive(Deserialize, Serialize)]
struct ChainJson {
    chain: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChainClaims {
    identity_public_key: String,
    extra_data: Option<Identity>,
}

fn read_long_string(buf: &mut Bytes) -> Result<Bytes, DecodeError> {
    let len = usize::try_from(buf.try_get_i32_le()?)
        .map_err(|_| DecodeError::Malformed("negative length"))?;
    if len > buf.remaining() {
        return Err(DecodeError::Eof);
    }
    Ok(buf.copy_to_bytes(len))
}

fn jwt_claims<T: DeserializeOwned>(token: &str) -> Result<T, DecodeError> {
    let mut parts = token.split('.');
    let (Some(_header), Some(payload), Some(_sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(DecodeError::Malformed("bad JWT"));
    };
    Ok(serde_json::from_slice(&B64_URL.decode(payload)?)?)
}

fn base64_bytes<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    B64.decode(<&str>::deserialize(d)?)
        .map_err(de::Error::custom)
}
