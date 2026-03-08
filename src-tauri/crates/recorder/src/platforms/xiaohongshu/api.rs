use super::response::InitialStateResponse;
use crate::account::Account;
use crate::errors::RecorderError;
use regex::Regex;
use reqwest::Client;
use urlencoding::decode;

const USER_AGENT: &str = "ios/7.830 (ios 17.0; ; iPhone 15 (A2846/A3089/A3090/A3092))";

/// Get user information from cookies
pub async fn get_user_info(
    _client: &Client,
    account: &Account,
) -> Result<crate::UserInfo, RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "xy-common-params",
        "platform=iOS&sid=session.1722166379345546829388"
            .parse()
            .unwrap(),
    );
    headers.insert("referer", "https://app.xhs.cn/".parse().unwrap());

    if !account.cookies.is_empty() {
        headers.insert("Cookie", account.cookies.parse().unwrap());
    } else {
        return Err(RecorderError::ApiError {
            error: "Cookies are required for Xiaohongshu".to_string(),
        });
    }

    // Try to get user info from cookies (extract user ID)
    // Format: web_session=xxx; a1=xxx; webId=xxx
    let web_id_regex = Regex::new(r"webId=([^;]+)").map_err(|e| RecorderError::ApiError {
        error: format!("Failed to create webId regex: {}", e),
    })?;

    let user_id = web_id_regex
        .captures(&account.cookies)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract webId from cookies - please ensure cookies are valid"
                .to_string(),
        })?;

    // Use default user name and avatar since Xiaohongshu's profile API requires additional authentication
    // The actual validation happens when making requests with the cookies
    Ok(crate::UserInfo {
        user_id,
        user_name: "Xiaohongshu User".to_string(),
        user_avatar: String::new(),
    })
}

#[derive(Clone, Debug)]
pub struct RoomInfo {
    pub live_status: bool,
    pub room_title: String,
    pub room_cover_url: String,
    pub user_id: String,
    pub user_name: String,
}

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub flv_url: String,
    pub m3u8_url: String,
}

/// Extract parameter from URL query string
fn get_param(url: &str, param: &str) -> Option<String> {
    let query_regex = Regex::new(&format!(r"{}=([^&]+)", regex::escape(param))).ok()?;
    query_regex
        .captures(url)
        .and_then(|cap| cap.get(1))
        .and_then(|m| decode(m.as_str()).ok())
        .map(|s| s.into_owned())
}

/// Get room information from page
pub async fn get_room_info(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<RoomInfo, RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "xy-common-params",
        "platform=iOS&sid=session.1722166379345546829388"
            .parse()
            .unwrap(),
    );
    headers.insert("referer", "https://app.xhs.cn/".parse().unwrap());

    if !account.cookies.is_empty() {
        headers.insert("Cookie", account.cookies.parse().unwrap());
    }

    // Handle short link redirect
    let final_url = if url.contains("xhslink.com") {
        let response = client.get(url).headers(headers.clone()).send().await?;
        response.url().to_string()
    } else {
        url.to_string()
    };

    // Extract user_id
    let host_id = get_param(&final_url, "host_id");
    let user_id_regex =
        Regex::new(r"/user/profile/([^/?#]+)").map_err(|e| RecorderError::ApiError {
            error: format!("Failed to create regex: {}", e),
        })?;
    let user_id = user_id_regex
        .captures(&final_url)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .or(host_id.clone())
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract user_id".to_string(),
        })?;

    // Fetch page content
    let response = client
        .get(&final_url)
        .headers(headers.clone())
        .send()
        .await?;
    let html_str = response.text().await?;

    // Parse INITIAL_STATE from script tag
    let script_regex =
        Regex::new(r"<script>window.__INITIAL_STATE__=(.*?)</script>").map_err(|e| {
            RecorderError::ApiError {
                error: format!("Failed to create regex: {}", e),
            }
        })?;

    let json_str = script_regex
        .captures(&html_str)
        .and_then(|cap| cap.get(1))
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract INITIAL_STATE from page".to_string(),
        })?
        .as_str()
        .replace("undefined", "null");

    let state: InitialStateResponse =
        serde_json::from_str(&json_str).map_err(|e| RecorderError::ApiError {
            error: format!("Failed to parse JSON: {}", e),
        })?;

    // Check if live stream exists and is active
    let live_stream = state.live_stream.ok_or(RecorderError::NotLive)?;

    let live_status = live_stream
        .live_status
        .map(|s| s == "success")
        .unwrap_or(false);

    if !live_status {
        // Try to get anchor name from profile page
        let anchor_name = get_anchor_name_from_profile(client, &user_id, &headers)
            .await
            .unwrap_or_else(|_| String::from("Unknown"));

        return Ok(RoomInfo {
            live_status: false,
            room_title: String::new(),
            room_cover_url: String::new(),
            user_id,
            user_name: anchor_name,
        });
    }

    let room_data = live_stream.room_data.ok_or(RecorderError::ApiError {
        error: "No roomData in live stream".to_string(),
    })?;

    let room_info = room_data.room_info.ok_or(RecorderError::ApiError {
        error: "No roomInfo in room data".to_string(),
    })?;

    let room_title = room_info.room_title.unwrap_or_default();

    // Check if it's a replay (回放)
    if room_title.contains("回放") {
        return Err(RecorderError::NotLive);
    }

    let deeplink = room_info.deeplink.ok_or(RecorderError::ApiError {
        error: "No deeplink in room info".to_string(),
    })?;

    let anchor_name =
        get_param(&deeplink, "host_nickname").unwrap_or_else(|| String::from("Unknown"));

    Ok(RoomInfo {
        live_status: true,
        room_title,
        room_cover_url: String::new(),
        user_id,
        user_name: anchor_name,
    })
}

/// Get stream URL from page
pub async fn get_stream_url(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<StreamInfo, RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "xy-common-params",
        "platform=iOS&sid=session.1722166379345546829388"
            .parse()
            .unwrap(),
    );
    headers.insert("referer", "https://app.xhs.cn/".parse().unwrap());

    if !account.cookies.is_empty() {
        headers.insert("Cookie", account.cookies.parse().unwrap());
    }

    // Handle short link redirect
    let final_url = if url.contains("xhslink.com") {
        let response = client.get(url).headers(headers.clone()).send().await?;
        response.url().to_string()
    } else {
        url.to_string()
    };

    // Fetch page content
    let response = client
        .get(&final_url)
        .headers(headers.clone())
        .send()
        .await?;
    let html_str = response.text().await?;

    // Parse INITIAL_STATE from script tag
    let script_regex =
        Regex::new(r"<script>window.__INITIAL_STATE__=(.*?)</script>").map_err(|e| {
            RecorderError::ApiError {
                error: format!("Failed to create regex: {}", e),
            }
        })?;

    let json_str = script_regex
        .captures(&html_str)
        .and_then(|cap| cap.get(1))
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract INITIAL_STATE from page".to_string(),
        })?
        .as_str()
        .replace("undefined", "null");

    let state: InitialStateResponse =
        serde_json::from_str(&json_str).map_err(|e| RecorderError::ApiError {
            error: format!("Failed to parse JSON: {}", e),
        })?;

    let live_stream = state.live_stream.ok_or(RecorderError::NotLive)?;

    let room_data = live_stream.room_data.ok_or(RecorderError::ApiError {
        error: "No roomData in live stream".to_string(),
    })?;

    let room_info = room_data.room_info.ok_or(RecorderError::ApiError {
        error: "No roomInfo in room data".to_string(),
    })?;

    let deeplink = room_info.deeplink.ok_or(RecorderError::ApiError {
        error: "No deeplink in room info".to_string(),
    })?;

    let flv_url_encoded = get_param(&deeplink, "flvUrl").ok_or(RecorderError::ApiError {
        error: "No flvUrl in deeplink".to_string(),
    })?;

    // Extract room_id from flv_url
    let room_id_regex = Regex::new(r"live/([^.]+)\.").map_err(|e| RecorderError::ApiError {
        error: format!("Failed to create regex: {}", e),
    })?;

    let room_id = room_id_regex
        .captures(&flv_url_encoded)
        .and_then(|cap| cap.get(1))
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract room_id from flv_url".to_string(),
        })?
        .as_str();

    // Construct stream URLs
    let flv_url = format!("http://live-source-play.xhscdn.com/live/{}.flv", room_id);
    let m3u8_url = format!("http://live-source-play.xhscdn.com/live/{}.m3u8", room_id);

    Ok(StreamInfo { flv_url, m3u8_url })
}

/// Get anchor name from profile page
async fn get_anchor_name_from_profile(
    client: &Client,
    user_id: &str,
    headers: &reqwest::header::HeaderMap,
) -> Result<String, RecorderError> {
    let profile_url = format!("https://www.xiaohongshu.com/user/profile/{}", user_id);
    let response = client
        .get(&profile_url)
        .headers(headers.clone())
        .send()
        .await?;
    let html_str = response.text().await?;

    let title_regex =
        Regex::new(r"<title>@(.*?) 的个人主页</title>").map_err(|e| RecorderError::ApiError {
            error: format!("Failed to create regex: {}", e),
        })?;

    let anchor_name = title_regex
        .captures(&html_str)
        .and_then(|cap| cap.get(1))
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract anchor name from profile".to_string(),
        })?
        .as_str()
        .to_string();

    Ok(anchor_name)
}
