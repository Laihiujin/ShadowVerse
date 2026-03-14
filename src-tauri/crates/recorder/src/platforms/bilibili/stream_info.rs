use super::api::{BiliStream, UrlInfo};
use crate::core::stream_info::{
    CdnNode, Codec, Format, PlatformStreamInfo, PlatformType, Quality, StreamVariant,
};
use crate::errors::RecorderError;

#[derive(Clone, Debug)]
pub struct BiliStreamInfo {
    pub inner: BiliStream,
    pub quality: Quality,
}

impl BiliStreamInfo {
    pub fn new(stream: BiliStream, quality: Quality) -> Self {
        Self {
            inner: stream,
            quality,
        }
    }

    fn build_url(&self, url_info: &UrlInfo) -> String {
        if url_info.extra.is_empty() {
            format!("{}{}", url_info.host, self.inner.base_url)
        } else {
            format!("{}{}?{}", url_info.host, self.inner.base_url, url_info.extra)
        }
    }
}

impl PlatformStreamInfo for BiliStreamInfo {
    fn primary_variant(&self) -> Result<StreamVariant, RecorderError> {
        let url_info = self
            .inner
            .url_info
            .first()
            .ok_or(RecorderError::NoStreamAvailable)?;

        let format = match self.inner.format {
            crate::core::Format::TS | crate::core::Format::FMP4 => Format::HLS,
            crate::core::Format::Flv => Format::FLV,
        };
        let codec = match self.inner.codec {
            crate::core::Codec::Avc => Codec::AVC,
            crate::core::Codec::Hevc => Codec::HEVC,
        };

        Ok(StreamVariant {
            url: self.build_url(url_info),
            format,
            codec,
            quality: self.quality,
            bitrate: None,
        })
    }

    fn all_variants(&self) -> Vec<StreamVariant> {
        let format = match self.inner.format {
            crate::core::Format::TS | crate::core::Format::FMP4 => Format::HLS,
            crate::core::Format::Flv => Format::FLV,
        };
        let codec = match self.inner.codec {
            crate::core::Codec::Avc => Codec::AVC,
            crate::core::Codec::Hevc => Codec::HEVC,
        };

        self.inner
            .url_info
            .iter()
            .map(|url_info| StreamVariant {
                url: self.build_url(url_info),
                format,
                codec,
                quality: self.quality,
                bitrate: None,
            })
            .collect()
    }

    fn expires_at(&self) -> Option<i64> {
        self.inner
            .url_info
            .first()
            .map(UrlInfo::get_expire)
            .filter(|expire| *expire > 0)
    }

    fn cdn_nodes(&self) -> Vec<CdnNode> {
        self.inner
            .url_info
            .iter()
            .enumerate()
            .map(|(i, url_info)| CdnNode {
                host: url_info.host.clone(),
                priority: i as u8,
            })
            .collect()
    }

    fn platform(&self) -> PlatformType {
        PlatformType::Bilibili
    }
}
