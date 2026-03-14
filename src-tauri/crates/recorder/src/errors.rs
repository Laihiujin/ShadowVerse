use super::platforms::bilibili::api::BiliStream;
use super::platforms::douyin::stream_info::DouyinStream;
use thiserror::Error;

#[derive(Debug, Clone)]
pub enum Stream {
    BiliBili(BiliStream),
    Douyin(DouyinStream),
}

#[derive(Error, Debug)]
pub enum RecorderError {
    #[error("Index not found: {url}")]
    IndexNotFound { url: String },
    #[error("Can not delete current stream: {live_id}")]
    ArchiveInUse { live_id: String },
    #[error("Cache is empty")]
    EmptyCache,
    #[error("Parse m3u8 content failed: {content}")]
    M3u8ParseFailed { content: String },
    #[error("No available stream provided")]
    NoStreamAvailable,
    #[error("Stream is freezed: {stream:#?}")]
    FreezedStream { stream: Stream },
    #[error("Stream is nearly expired: {expire}")]
    StreamExpired { expire: i64 },
    #[error("No room info provided")]
    NoRoomInfo,
    #[error("Invalid stream: {stream:#?}")]
    InvalidStream { stream: Stream },
    #[error("Stream is too slow: {stream:#?}")]
    SlowStream { stream: Stream },
    #[error("Header url is empty")]
    EmptyHeader,
    #[error("Header timestamp is invalid")]
    InvalidTimestamp,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Danmu stream error: {0}")]
    DanmuStreamError(#[from] danmu_stream::DanmuStreamError),
    #[error("Subtitle not found: {live_id}")]
    SubtitleNotFound { live_id: String },
    #[error("Subtitle generation failed: {error}")]
    SubtitleGenerationFailed { error: String },
    #[error("Resolution changed: {err}")]
    ResolutionChanged { err: String },
    #[error("Ffmpeg error: {0}")]
    FfmpegError(String),
    #[error("Format not found: {format}")]
    FormatNotFound { format: String },
    #[error("Codec not found: {codecs}")]
    CodecNotFound { codecs: String },
    #[error("Invalid cookies")]
    InvalidCookies,
    #[error("API error: {error}")]
    ApiError { error: String },
    #[error("Invalid value")]
    InvalidValue,
    #[error("Invalid response")]
    InvalidResponse,
    #[error("Invalid response json: {resp}")]
    InvalidResponseJson { resp: serde_json::Value },
    #[error("Invalid response status: {status}")]
    InvalidResponseStatus { status: reqwest::StatusCode },
    #[error("Upload cancelled")]
    UploadCancelled,
    #[error("Upload error: {err}")]
    UploadError { err: String },
    #[error("Client error: {0}")]
    ClientError(#[from] reqwest::Error),
    #[error("Security control error")]
    SecurityControlError,
    #[error("JavaScript runtime error: {0}")]
    JsRuntimeError(String),
    #[error("Update timeout")]
    UpdateTimeout,
    #[error("Unsupported stream")]
    UnsupportedStream,
    #[error("Empty record")]
    EmptyRecord,
    #[error("Not live")]
    NotLive,
}

pub fn is_guest_cookie_block_error(err: &RecorderError) -> bool {
    match err {
        RecorderError::InvalidCookies => true,
        RecorderError::SecurityControlError => true,
        RecorderError::InvalidResponseStatus { status } => {
            matches!(status.as_u16(), 401 | 403 | 429)
        }
        RecorderError::ApiError { error } | RecorderError::UploadError { err: error } => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_messages() {
        let e = RecorderError::IndexNotFound {
            url: "https://example.com".to_string(),
        };
        assert_eq!(format!("{}", e), "Index not found: https://example.com");
        assert_eq!(format!("{}", RecorderError::EmptyCache), "Cache is empty");
        assert_eq!(
            format!("{}", RecorderError::NoStreamAvailable),
            "No available stream provided"
        );
        assert_eq!(format!("{}", RecorderError::NoRoomInfo), "No room info provided");
        assert_eq!(format!("{}", RecorderError::EmptyHeader), "Header url is empty");
        assert_eq!(
            format!("{}", RecorderError::InvalidTimestamp),
            "Header timestamp is invalid"
        );
        assert_eq!(format!("{}", RecorderError::InvalidCookies), "Invalid cookies");
        assert_eq!(format!("{}", RecorderError::InvalidValue), "Invalid value");
        assert_eq!(format!("{}", RecorderError::InvalidResponse), "Invalid response");
        assert_eq!(format!("{}", RecorderError::UploadCancelled), "Upload cancelled");
        assert_eq!(
            format!("{}", RecorderError::SecurityControlError),
            "Security control error"
        );
        assert_eq!(format!("{}", RecorderError::UpdateTimeout), "Update timeout");
        assert_eq!(
            format!("{}", RecorderError::UnsupportedStream),
            "Unsupported stream"
        );
        assert_eq!(format!("{}", RecorderError::EmptyRecord), "Empty record");
        assert_eq!(format!("{}", RecorderError::NotLive), "Not live");
    }

    #[test]
    fn test_error_display_with_fields() {
        let e = RecorderError::ArchiveInUse {
            live_id: "abc123".to_string(),
        };
        assert!(format!("{}", e).contains("abc123"));
        assert!(format!(
            "{}",
            RecorderError::M3u8ParseFailed {
                content: "bad content".to_string(),
            }
        )
        .contains("bad content"));
        assert!(format!("{}", RecorderError::StreamExpired { expire: 1700000000 })
            .contains("1700000000"));
        assert!(format!(
            "{}",
            RecorderError::ApiError {
                error: "rate limited".to_string(),
            }
        )
        .contains("rate limited"));
        assert!(format!("{}", RecorderError::FfmpegError("codec error".to_string()))
            .contains("codec error"));
        assert!(format!(
            "{}",
            RecorderError::SubtitleNotFound {
                live_id: "live1".to_string(),
            }
        )
        .contains("live1"));
        assert!(format!(
            "{}",
            RecorderError::UploadError {
                err: "timeout".to_string(),
            }
        )
        .contains("timeout"));
        assert!(format!("{}", RecorderError::JsRuntimeError("eval failed".to_string()))
            .contains("eval failed"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let e: RecorderError = io_err.into();
        assert!(format!("{}", e).contains("file not found"));
    }
}
