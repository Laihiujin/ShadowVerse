use crate::core::{Codec as CoreCodec, Format as CoreFormat, HlsStream};
use crate::errors::RecorderError;
use std::fmt::Debug;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quality {
    Origin,
    BluRay4K,
    BluRay,
    UltraHD,
    HD,
    SD,
    Smooth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    HLS,
    FLV,
    RTMP,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    AVC,
    HEVC,
}

#[derive(Debug, Clone)]
pub struct CdnNode {
    pub host: String,
    pub priority: u8,
}

#[derive(Debug, Clone)]
pub struct StreamVariant {
    pub url: String,
    pub format: Format,
    pub codec: Codec,
    pub quality: Quality,
    pub bitrate: Option<u64>,
}

impl StreamVariant {
    pub fn to_hls_stream(
        &self,
        live_id: String,
        cdn_node: Option<&CdnNode>,
    ) -> Result<HlsStream, RecorderError> {
        if self.format != Format::HLS {
            return Err(RecorderError::ApiError {
                error: "Stream is not HLS format".to_string(),
            });
        }

        let url = if let Some(node) = cdn_node {
            self.url.replace(&extract_host(&self.url)?, &node.host)
        } else {
            self.url.clone()
        };

        let parsed = url::Url::parse(&url).map_err(|e| RecorderError::ApiError {
            error: format!("Invalid URL: {e}"),
        })?;

        let host = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));
        let mut base = parsed.path().to_string();
        let extra = parsed.query().unwrap_or("").to_string();
        if !extra.is_empty() {
            base.push('?');
        }

        let expire = parsed
            .query_pairs()
            .find(|(k, _)| k == "expire" || k == "expires")
            .and_then(|(_, v)| v.parse::<i64>().ok())
            .unwrap_or(0);

        let core_format = match self.codec {
            Codec::AVC => CoreFormat::TS,
            Codec::HEVC => CoreFormat::FMP4,
        };
        let core_codec = match self.codec {
            Codec::AVC => CoreCodec::Avc,
            Codec::HEVC => CoreCodec::Hevc,
        };

        Ok(HlsStream::new(
            live_id,
            host,
            base,
            extra,
            core_format,
            core_codec,
            expire,
        ))
    }

    pub fn to_flv_url(&self) -> Result<String, RecorderError> {
        if self.format != Format::FLV && self.format != Format::RTMP {
            return Err(RecorderError::ApiError {
                error: "Stream is not FLV or RTMP format".to_string(),
            });
        }
        Ok(self.url.clone())
    }

    pub fn to_recorder_type(
        &self,
        live_id: String,
        cdn_node: Option<&CdnNode>,
    ) -> Result<RecorderType, RecorderError> {
        match self.format {
            Format::HLS => Ok(RecorderType::Hls(Arc::new(
                self.to_hls_stream(live_id, cdn_node)?,
            ))),
            Format::FLV | Format::RTMP => Ok(RecorderType::Flv(self.to_flv_url()?)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RecorderType {
    Hls(Arc<HlsStream>),
    Flv(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformType {
    Bilibili,
    Douyin,
    Kuaishou,
    Huya,
    TikTok,
}

impl PlatformType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlatformType::Bilibili => "bilibili",
            PlatformType::Douyin => "douyin",
            PlatformType::Kuaishou => "kuaishou",
            PlatformType::Huya => "huya",
            PlatformType::TikTok => "tiktok",
        }
    }
}

pub trait PlatformStreamInfo: Clone + Send + Sync + Debug {
    fn primary_variant(&self) -> Result<StreamVariant, RecorderError>;
    fn all_variants(&self) -> Vec<StreamVariant>;
    fn expires_at(&self) -> Option<i64>;
    fn cdn_nodes(&self) -> Vec<CdnNode>;
    fn platform(&self) -> PlatformType;

    fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            now >= expires_at
        } else {
            false
        }
    }
}

fn extract_host(url: &str) -> Result<String, RecorderError> {
    url::Url::parse(url)
        .map(|u| u.host_str().unwrap_or("").to_string())
        .map_err(|e| RecorderError::ApiError {
            error: format!("Invalid URL: {e}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_variant_to_flv_url_ok() {
        let stream = StreamVariant {
            url: "rtmp://live.example.com/stream".to_string(),
            format: Format::FLV,
            codec: Codec::AVC,
            quality: Quality::Origin,
            bitrate: Some(5000),
        };
        assert_eq!(stream.to_flv_url().unwrap(), "rtmp://live.example.com/stream");
    }

    #[test]
    fn test_stream_variant_to_flv_url_hls_fails() {
        let stream = StreamVariant {
            url: "https://cdn.example.com/live/stream.m3u8".to_string(),
            format: Format::HLS,
            codec: Codec::AVC,
            quality: Quality::Origin,
            bitrate: None,
        };
        assert!(stream.to_flv_url().is_err());
    }

    #[test]
    fn test_stream_variant_to_hls_stream() {
        let stream = StreamVariant {
            url: "https://cdn.example.com/live/stream.m3u8?expire=9999999999&token=abc"
                .to_string(),
            format: Format::HLS,
            codec: Codec::AVC,
            quality: Quality::BluRay,
            bitrate: Some(3000),
        };
        let hls = stream.to_hls_stream("live_123".to_string(), None).unwrap();
        let index = hls.index();
        assert!(index.contains("cdn.example.com"));
        assert!(index.contains("stream.m3u8"));
    }

    #[test]
    fn test_stream_variant_to_hls_stream_with_cdn_node() {
        let stream = StreamVariant {
            url: "https://cdn1.example.com/live/stream.m3u8?token=abc".to_string(),
            format: Format::HLS,
            codec: Codec::AVC,
            quality: Quality::Origin,
            bitrate: None,
        };
        let cdn = CdnNode {
            host: "cdn2.example.com".to_string(),
            priority: 1,
        };
        let hls = stream
            .to_hls_stream("live_123".to_string(), Some(&cdn))
            .unwrap();
        assert!(hls.index().contains("cdn2.example.com"));
    }

    #[test]
    fn test_stream_variant_to_recorder_type_hls() {
        let stream = StreamVariant {
            url: "https://cdn.example.com/live/stream.m3u8?token=abc".to_string(),
            format: Format::HLS,
            codec: Codec::AVC,
            quality: Quality::Origin,
            bitrate: None,
        };
        assert!(matches!(
            stream.to_recorder_type("live_123".to_string(), None).unwrap(),
            RecorderType::Hls(_)
        ));
    }

    #[test]
    fn test_platform_type_as_str() {
        assert_eq!(PlatformType::Bilibili.as_str(), "bilibili");
        assert_eq!(PlatformType::Douyin.as_str(), "douyin");
        assert_eq!(PlatformType::Kuaishou.as_str(), "kuaishou");
        assert_eq!(PlatformType::Huya.as_str(), "huya");
        assert_eq!(PlatformType::TikTok.as_str(), "tiktok");
    }

    #[test]
    fn test_extract_host() {
        assert_eq!(
            extract_host("https://cdn.example.com/path").unwrap(),
            "cdn.example.com"
        );
        assert!(extract_host("not-a-url").is_err());
    }
}
