use bytes::TryGetError;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("unexpected end of packet")]
    Eof,
    #[error("varint too long")]
    VarIntTooLong,
    #[error("invalid utf-8")]
    BadUtf8,
    #[error("unknown packet id {0:#04x}")]
    UnknownId(u32),
    #[error("malformed: {0}")]
    Malformed(&'static str),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Base64(#[from] base64::DecodeError),
}

impl From<TryGetError> for DecodeError {
    fn from(_: TryGetError) -> Self {
        Self::Eof
    }
}
