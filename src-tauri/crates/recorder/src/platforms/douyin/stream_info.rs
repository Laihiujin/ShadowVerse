use serde_derive::Deserialize;
use serde_derive::Serialize;

use crate::core::stream_info::{
    CdnNode, Codec, Format, PlatformStreamInfo, PlatformType, Quality, StreamVariant,
};
use crate::errors::RecorderError;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DouyinStream {
    pub data: Data,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    pub origin: Origin,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Ld {
    pub main: Main,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Main {
    pub flv: String,
    pub hls: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Md {
    pub main: Main,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub main: Main,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Sd {
    pub main: Main,
}
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Hd {
    pub main: Main,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Ao {
    pub main: Main,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Uhd {
    pub main: Main,
}

impl PlatformStreamInfo for DouyinStream {
    fn primary_variant(&self) -> Result<StreamVariant, RecorderError> {
        Ok(StreamVariant {
            url: self.data.origin.main.hls.clone(),
            format: Format::HLS,
            codec: Codec::AVC,
            quality: Quality::Origin,
            bitrate: None,
        })
    }

    fn all_variants(&self) -> Vec<StreamVariant> {
        self.primary_variant().into_iter().collect()
    }

    fn expires_at(&self) -> Option<i64> {
        None
    }

    fn cdn_nodes(&self) -> Vec<CdnNode> {
        Vec::new()
    }

    fn platform(&self) -> PlatformType {
        PlatformType::Douyin
    }
}
