use serde::{Deserialize, Serialize};

/// Response structure for Weibo blog list API
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyblogResponse {
    pub data: Option<MyblogData>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyblogData {
    pub list: Vec<BlogItem>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlogItem {
    pub page_info: Option<PageInfo>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub object_type: Option<String>,
    pub object_id: Option<String>,
}

/// Response structure for Weibo live room API
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveRoomResponse {
    pub data: Option<LiveRoomData>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveRoomData {
    pub user_info: Option<UserInfo>,
    pub item: Option<ItemInfo>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub name: String,
    pub id: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    pub status: Option<i32>,
    pub desc: Option<String>,
    pub stream_info: Option<StreamInfo>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    pub pull: Option<PullInfo>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullInfo {
    pub live_origin_hls_url: Option<String>,
    pub live_origin_flv_url: Option<String>,
}
