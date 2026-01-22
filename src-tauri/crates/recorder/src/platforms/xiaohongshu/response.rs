use serde::{Deserialize, Serialize};

/// Response structure for Xiaohongshu (小红书) live stream data
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialStateResponse {
    #[serde(rename = "liveStream")]
    pub live_stream: Option<LiveStream>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStream {
    #[serde(rename = "liveStatus")]
    pub live_status: Option<String>,
    #[serde(rename = "roomData")]
    pub room_data: Option<RoomData>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomData {
    #[serde(rename = "roomInfo")]
    pub room_info: Option<RoomInfo>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomInfo {
    #[serde(rename = "roomTitle")]
    pub room_title: Option<String>,
    pub deeplink: Option<String>,
    pub room_id: Option<String>,
}
