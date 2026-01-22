use super::response::{LiveRoomResponse, MyblogResponse};
use crate::account::Account;
use crate::errors::RecorderError;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:124.0) Gecko/20100101 Firefox/124.0";
const DEFAULT_COOKIE: &str = "XSRF-TOKEN=qAP-pIY5V4tO6blNOhA4IIOD";

/// QR code information for login
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrInfo {
    pub qrid: String,
    pub url: String,
}

/// QR code status for polling
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrStatus {
    pub code: u8,
    pub cookies: String,
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
    pub m3u8_url: String,
    pub flv_url: String,
}

/// Extract user ID from URL
fn extract_uid(url: &str) -> Option<String> {
    // URL format: https://weibo.com/u/5885340893
    let parts: Vec<&str> = url.split('?').collect();
    if let Some(path) = parts.first() {
        let segments: Vec<&str> = path.rsplitn(2, "/u/").collect();
        if segments.len() == 2 {
            return Some(segments[0].to_string());
        }
    }
    None
}

/// Extract room ID from URL
fn extract_room_id(url: &str) -> Option<String> {
    // URL format: https://weibo.com/l/wblive/p/show/1022:2321325026370190442592
    let parts: Vec<&str> = url.split('?').collect();
    if let Some(path) = parts.first() {
        let segments: Vec<&str> = path.split("/show/").collect();
        if segments.len() == 2 {
            return Some(segments[1].to_string());
        }
    }
    None
}

/// Get room ID from user's blog list
async fn get_room_id_from_uid(
    client: &Client,
    account: &Account,
    uid: &str,
) -> Result<String, RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "Accept-Language",
        "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"
            .parse()
            .unwrap(),
    );
    headers.insert("Referer", format!("https://weibo.com/u/{}", uid).parse().unwrap());

    if !account.cookies.is_empty() {
        headers.insert("Cookie", account.cookies.parse().unwrap());
    } else {
        headers.insert("Cookie", DEFAULT_COOKIE.parse().unwrap());
    }

    let api_url = format!(
        "https://weibo.com/ajax/statuses/mymblog?uid={}&page=1&feature=0",
        uid
    );

    let response = client.get(&api_url).headers(headers).send().await?;
    let myblog: MyblogResponse = response.json().await?;

    let data = myblog.data.ok_or(RecorderError::ApiError {
        error: "No data in myblog response".to_string(),
    })?;

    // Find live blog in the list
    for item in data.list {
        if let Some(page_info) = item.page_info {
            if page_info.object_type.as_deref() == Some("live") {
                if let Some(object_id) = page_info.object_id {
                    return Ok(object_id);
                }
            }
        }
    }

    Err(RecorderError::NotLive)
}

/// Get room information from room ID
pub async fn get_room_info(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<RoomInfo, RecorderError> {
    // Try to extract room_id directly from URL
    let room_id = if let Some(rid) = extract_room_id(url) {
        rid
    } else if let Some(uid) = extract_uid(url) {
        // Get room_id from user's blog list
        get_room_id_from_uid(client, account, &uid).await?
    } else {
        return Err(RecorderError::ApiError {
            error: "Failed to extract uid or room_id from URL".to_string(),
        });
    };

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "Accept-Language",
        "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"
            .parse()
            .unwrap(),
    );
    headers.insert("Referer", "https://weibo.com/".parse().unwrap());

    if !account.cookies.is_empty() {
        headers.insert("Cookie", account.cookies.parse().unwrap());
    } else {
        headers.insert("Cookie", DEFAULT_COOKIE.parse().unwrap());
    }

    let api_url = format!("https://weibo.com/l/pc/anchor/live?live_id={}", room_id);

    let response = client.get(&api_url).headers(headers).send().await?;
    let live_room: LiveRoomResponse = response.json().await?;

    let data = live_room.data.ok_or(RecorderError::ApiError {
        error: "No data in live room response".to_string(),
    })?;

    let user_info = data.user_info.ok_or(RecorderError::ApiError {
        error: "No user_info in live room data".to_string(),
    })?;

    let item = data.item.ok_or(RecorderError::ApiError {
        error: "No item in live room data".to_string(),
    })?;

    let live_status = item.status.unwrap_or(0) == 1;

    Ok(RoomInfo {
        live_status,
        room_title: item.desc.unwrap_or_default(),
        room_cover_url: String::new(),
        user_id: user_info.id.unwrap_or_default(),
        user_name: user_info.name,
    })
}

/// Get stream URL from room ID
pub async fn get_stream_url(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<StreamInfo, RecorderError> {
    // Try to extract room_id directly from URL
    let room_id = if let Some(rid) = extract_room_id(url) {
        rid
    } else if let Some(uid) = extract_uid(url) {
        // Get room_id from user's blog list
        get_room_id_from_uid(client, account, &uid).await?
    } else {
        return Err(RecorderError::ApiError {
            error: "Failed to extract uid or room_id from URL".to_string(),
        });
    };

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "Accept-Language",
        "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"
            .parse()
            .unwrap(),
    );
    headers.insert("Referer", "https://weibo.com/".parse().unwrap());

    if !account.cookies.is_empty() {
        headers.insert("Cookie", account.cookies.parse().unwrap());
    } else {
        headers.insert("Cookie", DEFAULT_COOKIE.parse().unwrap());
    }

    let api_url = format!("https://weibo.com/l/pc/anchor/live?live_id={}", room_id);

    let response = client.get(&api_url).headers(headers).send().await?;
    let live_room: LiveRoomResponse = response.json().await?;

    let data = live_room.data.ok_or(RecorderError::ApiError {
        error: "No data in live room response".to_string(),
    })?;

    let item = data.item.ok_or(RecorderError::ApiError {
        error: "No item in live room data".to_string(),
    })?;

    let stream_info = item.stream_info.ok_or(RecorderError::ApiError {
        error: "No stream_info in item".to_string(),
    })?;

    let pull = stream_info.pull.ok_or(RecorderError::ApiError {
        error: "No pull info in stream_info".to_string(),
    })?;

    let m3u8_url = pull.live_origin_hls_url.ok_or(RecorderError::ApiError {
        error: "No HLS URL in pull info".to_string(),
    })?;

    let flv_url = pull.live_origin_flv_url.ok_or(RecorderError::ApiError {
        error: "No FLV URL in pull info".to_string(),
    })?;

    Ok(StreamInfo { m3u8_url, flv_url })
}

/// Get user information from cookies
pub async fn get_user_info(client: &Client, account: &Account) -> Result<crate::UserInfo, RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "Accept-Language",
        "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"
            .parse()
            .unwrap(),
    );
    headers.insert("Referer", "https://weibo.com/".parse().unwrap());

    if !account.cookies.is_empty() {
        headers.insert("Cookie", account.cookies.parse().unwrap());
    } else {
        return Err(RecorderError::ApiError {
            error: "Cookies are required".to_string(),
        });
    }

    // Get user info from profile API
    let response = client
        .get("https://weibo.com/ajax/profile/info")
        .headers(headers)
        .send()
        .await?;

    #[derive(Deserialize)]
    struct ProfileResponse {
        data: Option<ProfileData>,
    }

    #[derive(Deserialize)]
    struct ProfileData {
        user: Option<WeiboUser>,
    }

    #[derive(Deserialize)]
    struct WeiboUser {
        id: Option<i64>,
        #[serde(rename = "screen_name")]
        screen_name: Option<String>,
        #[serde(rename = "profile_image_url")]
        profile_image_url: Option<String>,
    }

    let profile: ProfileResponse = response.json().await.map_err(|e| RecorderError::ApiError {
        error: format!("Failed to parse profile response: {} - please check if cookies are valid", e),
    })?;

    let data = profile.data.ok_or(RecorderError::ApiError {
        error: "No data in profile response - cookies may be invalid".to_string(),
    })?;

    let user = data.user.ok_or(RecorderError::ApiError {
        error: "No user info in profile data - cookies may be invalid".to_string(),
    })?;

    Ok(crate::UserInfo {
        user_id: user.id.map(|id| id.to_string()).unwrap_or_default(),
        user_name: user.screen_name.unwrap_or_default(),
        user_avatar: user.profile_image_url.unwrap_or_default(),
    })
}

/// Get QR code for login
pub async fn get_qr(client: &Client) -> Result<QrInfo, RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert("Referer", "https://weibo.com/".parse().unwrap());

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let url = format!(
        "https://login.sina.com.cn/sso/qrcode/image?entry=weibo&size=180&callback=STK_{}",
        timestamp
    );

    let response = client.get(&url).headers(headers).send().await?;
    let text = response.text().await?;

    // Extract qrid from JSONP response
    let qrid_regex = Regex::new(r#""qrid":"([^"]+)""#).map_err(|e| RecorderError::ApiError {
        error: format!("Failed to create qrid regex: {}", e),
    })?;

    let qrid = qrid_regex
        .captures(&text)
        .and_then(|cap| cap.get(1))
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract qrid from response".to_string(),
        })?
        .as_str()
        .to_string();

    // Extract image URL from JSONP response
    let img_regex =
        Regex::new(r#""image":"([^"]+)""#).map_err(|e| RecorderError::ApiError {
            error: format!("Failed to create image regex: {}", e),
        })?;

    let img_url = img_regex
        .captures(&text)
        .and_then(|cap| cap.get(1))
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract image URL from response".to_string(),
        })?
        .as_str()
        .replace(r"\/", "/"); // Unescape forward slashes

    Ok(QrInfo {
        qrid,
        url: img_url,
    })
}

/// Poll QR code status and get cookies after successful login
pub async fn get_qr_status(client: &Client, qrid: &str) -> Result<QrStatus, RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert("Referer", "https://weibo.com/".parse().unwrap());

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let check_url = format!(
        "https://login.sina.com.cn/sso/qrcode/check?entry=sso&qrid={}&callback=STK_{}",
        qrid, timestamp
    );

    let response = client.get(&check_url).headers(headers.clone()).send().await?;
    let text = response.text().await?;

    // Extract retcode from JSONP response
    let retcode_regex =
        Regex::new(r#""retcode":(\d+)"#).map_err(|e| RecorderError::ApiError {
            error: format!("Failed to create retcode regex: {}", e),
        })?;

    let retcode = retcode_regex
        .captures(&text)
        .and_then(|cap| cap.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(50000000);

    // retcode: 20000000 = success, 50114001 = not scanned, 50114002 = scanned but not confirmed
    if retcode != 20000000 {
        let code = if retcode == 50114002 { 2 } else { 1 };
        return Ok(QrStatus {
            code,
            cookies: String::new(),
        });
    }

    // Extract alt token from response
    let alt_regex = Regex::new(r#""alt":"([^"]+)""#).map_err(|e| RecorderError::ApiError {
        error: format!("Failed to create alt regex: {}", e),
    })?;

    let alt = alt_regex
        .captures(&text)
        .and_then(|cap| cap.get(1))
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract alt token from response".to_string(),
        })?
        .as_str();

    // Get cross-domain URLs
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let login_url = format!(
        "https://login.sina.com.cn/sso/login.php?entry=qrcodesso&returntype=TEXT&crossdomain=1&cdult=3&domain=weibo.com&alt={}&savestate=30&callback=STK_{}",
        alt, timestamp
    );

    let login_response = client
        .get(&login_url)
        .headers(headers.clone())
        .send()
        .await?;
    let login_text = login_response.text().await?;

    // Extract cross-domain URLs from response
    let url_regex =
        Regex::new(r#""crossDomainUrlList":\[(.*?)\]"#).map_err(|e| RecorderError::ApiError {
            error: format!("Failed to create URL list regex: {}", e),
        })?;

    let url_list_str = url_regex
        .captures(&login_text)
        .and_then(|cap| cap.get(1))
        .ok_or(RecorderError::ApiError {
            error: "Failed to extract cross-domain URLs".to_string(),
        })?
        .as_str();

    // Parse individual URLs
    let single_url_regex = Regex::new(r#""([^"]+)""#).map_err(|e| RecorderError::ApiError {
        error: format!("Failed to create single URL regex: {}", e),
    })?;

    let mut all_cookies = Vec::new();
    let urls: Vec<String> = single_url_regex
        .captures_iter(url_list_str)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().replace(r"\/", "/")))
        .collect();

    // Visit cross-domain URLs to collect cookies
    for (idx, mut url) in urls.into_iter().enumerate() {
        // First two URLs need &action=login appended
        if idx < 2 && !url.contains("action=login") {
            url.push_str("&action=login");
        }

        if let Ok(cookie_response) = client.get(&url).headers(headers.clone()).send().await {
            for cookie_header in cookie_response.headers().get_all("set-cookie") {
                if let Ok(cookie_str) = cookie_header.to_str() {
                    if let Some(cookie_part) = cookie_str.split(';').next() {
                        all_cookies.push(cookie_part.to_string());
                    }
                }
            }
        }
    }

    // Deduplicate cookies by name
    let mut cookie_map = std::collections::HashMap::new();
    for cookie in all_cookies {
        if let Some(name) = cookie.split('=').next() {
            cookie_map.insert(name.to_string(), cookie);
        }
    }

    let cookies: Vec<String> = cookie_map.into_values().collect();
    let cookies_str = cookies.join("; ");

    if cookies_str.is_empty() {
        return Err(RecorderError::ApiError {
            error: "Failed to extract cookies from cross-domain responses".to_string(),
        });
    }

    Ok(QrStatus {
        code: 0,
        cookies: cookies_str,
    })
}
