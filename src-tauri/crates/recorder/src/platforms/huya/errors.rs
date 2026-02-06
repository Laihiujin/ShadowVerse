use thiserror::Error;

#[derive(Error, Debug)]
pub enum HuyaClientError {
    #[error("Invalid response")]
    InvalidResponse,
    #[error("Client init error")]
    InitClientError,
    #[error("Invalid response status: {status}")]
    InvalidResponseStatus { status: reqwest::StatusCode },
    #[error("Invalid response json: {resp}")]
    InvalidResponseJson { resp: serde_json::Value },
    #[error("Invalid message code: {code}")]
    InvalidMessageCode { code: u64 },
    #[error("Invalid value")]
    InvalidValue,
    #[error("Invalid url")]
    InvalidUrl,
    #[error("Invalid stream format")]
    InvalidFormat,
    #[error("Invalid stream")]
    InvalidStream,
    #[error("Invalid cookie")]
    InvalidCookie,
    #[error("Upload error: {err}")]
    UploadError { err: String },
    #[error("Upload was cancelled by user")]
    UploadCancelled,
    #[error("Empty cache")]
    EmptyCache,
    #[error("Client error: {0}")]
    ClientError(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Security control error")]
    SecurityControlError,
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Format not found: {0}")]
    FormatNotFound(String),
    #[error("Codec not found: {0}")]
    CodecNotFound(String),
    #[error("Extractor error: {0}")]
    ExtractorError(String),
}

impl From<HuyaClientError> for String {
    fn from(err: HuyaClientError) -> Self {
        err.to_string()
    }
}

pub fn is_guest_cookie_block_error(err: &HuyaClientError) -> bool {
    match err {
        HuyaClientError::InvalidCookie => true,
        HuyaClientError::SecurityControlError => true,
        HuyaClientError::InvalidResponseStatus { status } => {
            matches!(status.as_u16(), 401 | 403 | 429)
        }
        HuyaClientError::ApiError(error) | HuyaClientError::UploadError { err: error } => {
            let lower = error.to_ascii_lowercase();
            lower.contains("cookie")
                || lower.contains("login")
                || lower.contains("invalid")
                || lower.contains("expired")
                || lower.contains("token")
                || lower.contains("blocked")
                || lower.contains("forbidden")
                || lower.contains("rate")
                || lower.contains("too many")
                || lower.contains("captcha")
        }
        _ => false,
    }
}
