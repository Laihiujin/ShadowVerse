use crate::account::Account;
use crate::errors::RecorderError;
use crate::utils::user_agent_generator;
use regex::Regex;
use reqwest::{Client, Proxy};

use super::response::DouyinRoomInfoResponse;
use super::params;
use crate::reverse_generate::abogus;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use reqwest::header::SET_COOKIE;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use std::fs;
use base64::{engine::general_purpose, Engine as _};
use serde_json::json;
use toml::Value as TomlValue;
const DOUYIN_PASSPORT_UA_DEFAULT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct DouyinBasicRoomInfo {
    pub room_id_str: String,
    pub room_title: String,
    pub cover: Option<String>,
    pub status: i64,
    pub hls_url: String,
    pub stream_data: String,
    // user related
    pub user_name: String,
    pub user_avatar: String,
    pub sec_user_id: String,
}

fn generate_a_bogus(params: &str) -> String {
    abogus::encode_abogus(params)
}

fn generate_ms_token() -> String {
    params::gen_false_ms_token()
}


fn douyin_proxy_url_from_env() -> Option<String> {
    // Force non-TikTok platforms to bypass proxy.
    None
}

fn build_douyin_proxy_client(proxy_url: &str) -> Result<Client, RecorderError> {
    let proxy = Proxy::all(proxy_url).map_err(|_| RecorderError::ApiError {
        error: "Invalid proxy URL".to_string(),
    })?;
    Client::builder()
        .proxy(proxy)
        .build()
        .map_err(|_| RecorderError::ApiError {
            error: "Failed to build proxy client".to_string(),
        })
}

pub fn generate_user_agent_header() -> reqwest::header::HeaderMap {
    let user_agent = user_agent_generator::UserAgentGenerator::new().generate(false);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("user-agent", user_agent.parse().unwrap());
    headers
}

fn passport_user_agent() -> String {
    read_env("DOUYIN_PASSPORT_USER_AGENT")
        .or_else(|| read_env("DOUYIN_USER_AGENT"))
        .unwrap_or_else(|| DOUYIN_PASSPORT_UA_DEFAULT.to_string())
}

fn generate_passport_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("user-agent", passport_user_agent().parse().unwrap());
    apply_douyin_browser_headers(&mut headers, None);
    headers
}

fn ttwid_check_payload(overrides: Option<&HashMap<String, String>>) -> serde_json::Value {
    let aid = param_value(overrides, &["aid"], "DOUYIN_AID", "6383");
    let service = param_value(
        overrides,
        &["ttwid_service", "service"],
        "DOUYIN_TTWID_SERVICE",
        "www.douyin.com",
    );
    json!({
        "aid": aid.parse::<i64>().unwrap_or(6383),
        "service": service,
        "union": false,
        "unionHost": "",
        "needFid": false,
        "fid": "",
        "migrate_priority": 0
    })
}

fn build_douyin_challenge_params(
    overrides: Option<&HashMap<String, String>>,
) -> Vec<(String, String)> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let mut params = Vec::new();
    push_param(
        &mut params,
        "passport_jssdk_version",
        param_value(
            overrides,
            &["passport_jssdk_version"],
            "DOUYIN_PASSPORT_JSSDK_VERSION",
            "3.1.3",
        ),
    );
    push_param(
        &mut params,
        "passport_jssdk_type",
        param_value(
            overrides,
            &["passport_jssdk_type"],
            "DOUYIN_PASSPORT_JSSDK_TYPE",
            "normal",
        ),
    );
    push_param(
        &mut params,
        "is_from_ttaccountsdk",
        param_value(
            overrides,
            &["is_from_ttaccountsdk"],
            "DOUYIN_IS_FROM_TTACCOUNTSDK",
            "1",
        ),
    );
    push_param(
        &mut params,
        "aid",
        param_value(overrides, &["aid"], "DOUYIN_AID", "6383"),
    );
    push_param(
        &mut params,
        "language",
        param_value(overrides, &["language"], "DOUYIN_LANGUAGE", "zh"),
    );
    push_param(
        &mut params,
        "account_app_language",
        param_value(
            overrides,
            &["account_app_language"],
            "DOUYIN_ACCOUNT_APP_LANGUAGE",
            "zh-CN",
        ),
    );
    push_param(
        &mut params,
        "ts",
        param_value(overrides, &["ts"], "DOUYIN_TS", &now_secs),
    );
    push_param(
        &mut params,
        "request_host",
        param_value(
            overrides,
            &["request_host"],
            "DOUYIN_REQUEST_HOST",
            "https://www.douyin.com",
        ),
    );
    push_param(
        &mut params,
        "skip_c",
        param_value(overrides, &["skip_c"], "DOUYIN_SKIP_C", "1"),
    );
    push_param(&mut params, "p_ca", env_or("DOUYIN_P_CA", "4.0.17"));
    push_param(
        &mut params,
        "p_ca_real",
        param_value(
            overrides,
            &["p_ca_real"],
            "DOUYIN_P_CA_REAL",
            "1.0.0.729",
        ),
    );
    push_param(
        &mut params,
        "account_sdk_source",
        param_value(
            overrides,
            &["account_sdk_source"],
            "DOUYIN_ACCOUNT_SDK_SOURCE",
            "web",
        ),
    );
    push_param(
        &mut params,
        "account_sdk_source_info",
        override_value(overrides, &["account_sdk_source_info"])
            .or_else(|| read_env("DOUYIN_ACCOUNT_SDK_SOURCE_INFO"))
            .unwrap_or_default(),
    );
    push_param(&mut params, "p_js_v", env_or("DOUYIN_P_JS_V", "3.1.3"));
    push_param(&mut params, "p_js_t", env_or("DOUYIN_P_JS_T", "pro"));
    push_param(&mut params, "p_zt", env_or("DOUYIN_P_ZT", "3.3.10"));
    push_param(&mut params, "p_ver", env_or("DOUYIN_P_VER", "1.1.3"));
    push_param(&mut params, "p_ver_real", env_or("DOUYIN_P_VER_REAL", "0"));
    push_param(
        &mut params,
        "p_bd",
        param_value(overrides, &["p_bd"], "DOUYIN_P_BD", "1.0.1.19-fix.01"),
    );
    push_param(
        &mut params,
        "p_ts",
        param_value(overrides, &["p_ts"], "DOUYIN_P_TS", &now_ms),
    );
    push_param(
        &mut params,
        "p_no",
        override_value(overrides, &["p_no"])
            .or_else(|| read_env("DOUYIN_P_NO"))
            .unwrap_or_else(|| now_secs.clone()),
    );
    push_param(
        &mut params,
        "biz_trace_id",
        override_value(overrides, &["biz_trace_id"])
            .or_else(|| read_env("DOUYIN_BIZ_TRACE_ID"))
            .unwrap_or_else(|| now_ms.clone()),
    );
    push_param(
        &mut params,
        "device_platform",
        param_value(
            overrides,
            &["device_platform"],
            "DOUYIN_DEVICE_PLATFORM",
            "web_app",
        ),
    );
    let ms_token = override_value(overrides, &["msToken", "ms_token"])
        .or_else(|| read_env("DOUYIN_MS_TOKEN"))
        .unwrap_or_else(|| generate_ms_token());
    push_param(&mut params, "msToken", ms_token);
    if let Some(sign) = override_value(overrides, &["sign"]).or_else(|| read_env("DOUYIN_SIGN")) {
        push_param(&mut params, "sign", sign);
    }
    if let Some(qs) = override_value(overrides, &["qs"]).or_else(|| read_env("DOUYIN_QS")) {
        push_param(&mut params, "qs", qs);
    }
    let param_str = build_query_string_owned(&params);
    let a_bogus = override_value(overrides, &["a_bogus", "a-bogus"])
        .or_else(|| read_env("DOUYIN_A_BOGUS"))
        .unwrap_or_else(|| generate_a_bogus(&param_str));
    push_param(&mut params, "a_bogus", a_bogus);
    params
}

fn challenge_body_override(overrides: Option<&HashMap<String, String>>) -> Option<String> {
    override_value(overrides, &["challenge_body", "challenge_body_raw"])
        .or_else(|| read_env("DOUYIN_PASSPORT_CHALLENGE_BODY"))
}

fn challenge_content_type_override(overrides: Option<&HashMap<String, String>>) -> String {
    override_value(
        overrides,
        &["challenge_content_type", "challenge_content-type"],
    )
    .or_else(|| read_env("DOUYIN_PASSPORT_CHALLENGE_CONTENT_TYPE"))
    .unwrap_or_else(|| "application/x-www-form-urlencoded".to_string())
}

fn normalize_params_raw(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(map) = parse_structured_kv(trimmed) {
        if map.is_empty() {
            return trimmed.to_string();
        }
        let mut params = Vec::with_capacity(map.len());
        for (k, v) in map {
            params.push((k, v));
        }
        return build_query_string_owned(&params);
    }
    trimmed.to_string()
}

fn normalize_body(raw: &str, content_type: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let is_json = content_type.to_ascii_lowercase().contains("json");
    if let Some(map) = parse_structured_kv(trimmed) {
        if map.is_empty() {
            return trimmed.to_string();
        }
        if is_json {
            return serde_json::to_string(&map).unwrap_or_else(|_| trimmed.to_string());
        }
        let mut params = Vec::with_capacity(map.len());
        for (k, v) in map {
            params.push((k, v));
        }
        return build_query_string_owned(&params);
    }
    trimmed.to_string()
}

fn parse_structured_kv(raw: &str) -> Option<HashMap<String, String>> {
    let mut map = HashMap::new();
    if raw.starts_with('{') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(obj) = json.as_object() {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        map.insert(k.clone(), s.to_string());
                    } else if v.is_number() || v.is_boolean() {
                        map.insert(k.clone(), v.to_string());
                    }
                }
                return Some(map);
            }
        }
    }
    if let Ok(toml) = raw.parse::<TomlValue>() {
        if toml.is_table() {
            collect_toml_kv(&toml, &mut map);
            if !map.is_empty() {
                return Some(map);
            }
        }
    }
    if raw.trim_start().starts_with('{') {
        let wrapped = format!("data = {raw}");
        if let Ok(toml) = wrapped.parse::<TomlValue>() {
            if let Some(data) = toml.get("data") {
                if data.is_table() {
                    collect_toml_kv(data, &mut map);
                    if !map.is_empty() {
                        return Some(map);
                    }
                }
            }
        }
    }
    None
}

async fn fetch_douyin_passport_challenge(
    client: &Client,
    headers: &reqwest::header::HeaderMap,
    cookies: &mut HashMap<String, String>,
    overrides: Option<&HashMap<String, String>>,
) {
    let raw_query = override_value(overrides, &["challenge_params_raw"])
        .or_else(|| read_env("DOUYIN_PASSPORT_CHALLENGE_PARAMS_RAW"));
    let url = if let Some(raw) = raw_query {
        let param_str = normalize_params_raw(&raw);
        format!("https://login.douyin.com/passport/web/challenge/?{param_str}")
    } else {
        let params = build_douyin_challenge_params(overrides);
        let param_str = build_query_string_owned(&params);
        format!("https://login.douyin.com/passport/web/challenge/?{param_str}")
    };
    let mut req_headers = headers.clone();
    let content_type = challenge_content_type_override(overrides);
    if let Ok(parsed) = content_type.parse() {
        req_headers.insert("content-type", parsed);
    }
    let cookie_header = format_cookie_header(cookies);
    if !cookie_header.is_empty() {
        req_headers.insert("Cookie", cookie_header.parse().unwrap());
    }
    let body = challenge_body_override(overrides)
        .map(|raw| normalize_body(&raw, &content_type))
        .unwrap_or_default();
    let resp = client
        .post(&url)
        .headers(req_headers)
        .body(body)
        .send()
        .await;
    let Ok(resp) = resp else {
        return;
    };
    let resp_headers = resp.headers().clone();
    let json: serde_json::Value = resp.json().await.unwrap_or_default();
    if let Some(data) = json.get("data") {
        let passportiv_len = data
            .get("passportiv")
            .and_then(|v| v.as_str())
            .map(|v| v.len())
            .unwrap_or(0);
        let template_len = data
            .get("template")
            .and_then(|v| v.as_str())
            .map(|v| v.len())
            .unwrap_or(0);
        log::info!(
            "[Douyin] challenge ok: passportiv_len={}, template_len={}",
            passportiv_len,
            template_len
        );
    } else {
        log::warn!("[Douyin] challenge response: {}", json);
    }
    let resp_cookies = parse_set_cookies(&resp_headers);
    for (k, v) in resp_cookies {
        cookies.insert(k, v);
    }
}

async fn fetch_ttwid_check(
    client: &Client,
    headers: &reqwest::header::HeaderMap,
    cookies: &mut HashMap<String, String>,
    overrides: Option<&HashMap<String, String>>,
) {
    let payload = ttwid_check_payload(overrides);
    let mut ttwid_headers = headers.clone();
    let ttwid_accept = override_value(overrides, &["ttwid_accept"])
        .unwrap_or_else(|| "application/json, text/plain, */*".to_string());
    if let Ok(parsed) = ttwid_accept.parse() {
        ttwid_headers.insert("accept", parsed);
    }
    ttwid_headers.insert("content-type", "application/json".parse().unwrap());
    let cookie_header = format_cookie_header(cookies);
    if !cookie_header.is_empty() {
        ttwid_headers.insert("Cookie", cookie_header.parse().unwrap());
    }
    let resp = client
        .post("https://login.douyin.com/ttwid/check/")
        .headers(ttwid_headers)
        .json(&payload)
        .send()
        .await;
    let Ok(resp) = resp else {
        return;
    };
    let resp_headers = resp.headers().clone();
    let _ = resp.json::<serde_json::Value>().await;
    let resp_cookies = parse_set_cookies(&resp_headers);
    for (k, v) in resp_cookies {
        cookies.insert(k, v);
    }
}

pub async fn get_room_info(
    client: &Client,
    account: &Account,
    room_id: &str,
    sec_user_id: &str,
) -> Result<DouyinBasicRoomInfo, RecorderError> {
    let mut headers = generate_user_agent_header();
    headers.insert("Referer", "https://live.douyin.com/".parse().unwrap());
    headers.insert("Cookie", account.cookies.clone().parse().unwrap());
    let ms_token = generate_ms_token();
    let params = format!(
        "aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&enter_from=web_live&cookie_enabled=true&screen_width=1920&screen_height=1080&browser_language=zh-CN&browser_platform=MacIntel&browser_name=Chrome&browser_version=122.0.0.0&web_rid={room_id}&ms_token={ms_token}"
    );
    let a_bogus = generate_a_bogus(&params);
    // log::debug!("params: {params}");
    // log::debug!("user_agent: {user_agent}");
    // log::debug!("a_bogus: {a_bogus}");
    let url = format!(
            "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&enter_from=web_live&cookie_enabled=true&screen_width=1920&screen_height=1080&browser_language=zh-CN&browser_platform=MacIntel&browser_name=Chrome&browser_version=122.0.0.0&web_rid={room_id}&ms_token={ms_token}&a_bogus={a_bogus}"
        );

    let resp = client.get(&url).headers(headers.clone()).send().await?;

    let status = resp.status();
    let text = resp.text().await?;

    if text.is_empty() {
        log::debug!("Empty room info response, trying H5 API");
        return get_room_info_h5(client, account, room_id, sec_user_id).await;
    }

    if status.is_success() {
        if let Ok(data) = serde_json::from_str::<DouyinRoomInfoResponse>(&text) {
            if data.status_code != 0 {
                return Err(RecorderError::ApiError {
                    error: format!("Douyin API error status_code: {}", data.status_code),
                });
            }
            let room = match data.data.data.first() {
                Some(room) => room,
                None => {
                    return Err(RecorderError::ApiError {
                        error: "Douyin room info missing, possible rate limit or login redirect"
                            .to_string(),
                    });
                }
            };
            if let Some(enter_room_id) = data.data.enter_room_id.as_deref() {
                if !enter_room_id.is_empty()
                    && enter_room_id != room_id
                    && enter_room_id != room.id_str
                {
                    return Err(RecorderError::ApiError {
                        error: "Douyin room id mismatch, possible rate limit or login redirect"
                            .to_string(),
                    });
                }
            }

            let (user_name, user_avatar, owner_sec_uid) = if let Some(owner) = room.owner.as_ref()
            {
                (
                    owner.nickname.clone(),
                    owner
                        .avatar_thumb
                        .url_list
                        .first()
                        .cloned()
                        .unwrap_or_default(),
                    owner.sec_uid.clone(),
                )
            } else {
                if let Some(owner_user_id) = room.owner_user_id_str.as_deref() {
                    if !owner_user_id.is_empty() && owner_user_id != data.data.user.id_str {
                        return Err(RecorderError::ApiError {
                            error: "Douyin room owner mismatch, possible rate limit or login redirect"
                                .to_string(),
                        });
                    }
                }
                (
                    data.data.user.nickname.clone(),
                    data.data
                        .user
                        .avatar_thumb
                        .url_list
                        .first()
                        .cloned()
                        .unwrap_or_default(),
                    data.data.user.sec_uid.clone(),
                )
            };

            let cover = room
                .cover
                .as_ref()
                .and_then(|cover| cover.url_list.first().cloned());

            return Ok(DouyinBasicRoomInfo {
                room_id_str: room.id_str.clone(),
                sec_user_id: if owner_sec_uid.is_empty() {
                    sec_user_id.to_string()
                } else {
                    owner_sec_uid
                },
                cover,
                room_title: room.title.clone(),
                user_name,
                user_avatar,
                status: data.data.room_status,
                hls_url: room
                    .stream_url
                    .as_ref()
                    .map(|stream_url| stream_url.hls_pull_url.clone())
                    .unwrap_or_default(),
                stream_data: room
                    .stream_url
                    .as_ref()
                    .map(|s| s.live_core_sdk_data.pull_data.stream_data.clone())
                    .unwrap_or_default(),
            });
        }
        log::error!("Failed to parse room info response: {text}");
        return get_room_info_h5(client, account, room_id, sec_user_id).await;
    }

    log::error!("Failed to get room info: {status}");
    return get_room_info_h5(client, account, room_id, sec_user_id).await;
}

pub async fn get_room_info_h5(
    client: &Client,
    account: &Account,
    room_id: &str,
    sec_user_id: &str,
) -> Result<DouyinBasicRoomInfo, RecorderError> {
    // 参考biliup实现，构建完整的URL参数
    let room_id_str = room_id.to_string();
    // https://webcast.amemv.com/webcast/room/reflow/info/?type_id=0&live_id=1&version_code=99.99.99&app_id=1128&room_id=10000&sec_user_id=MS4wLjAB&aid=6383&device_platform=web&browser_language=zh-CN&browser_platform=Win32&browser_name=Mozilla&browser_version=5.0
    let url_params = [
        ("type_id", "0"),
        ("live_id", "1"),
        ("version_code", "99.99.99"),
        ("app_id", "1128"),
        ("room_id", &room_id_str),
        ("sec_user_id", sec_user_id),
        ("aid", "6383"),
        ("device_platform", "web"),
    ];

    // 构建URL
    let query_string = url_params
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("https://webcast.amemv.com/webcast/room/reflow/info/?{query_string}");

    let mut headers = generate_user_agent_header();
    headers.insert("Referer", "https://live.douyin.com/".parse().unwrap());
    headers.insert("Cookie", account.cookies.clone().parse().unwrap());

    let resp = client.get(&url).headers(headers).send().await?;

    let status = resp.status();
    let text = resp.text().await?;

    if status.is_success() {
        // Try to parse as H5 response format
        if let Ok(h5_data) =
            serde_json::from_str::<super::response::DouyinH5RoomInfoResponse>(&text)
        {
            // Extract RoomBasicInfo from H5 response
            let room = &h5_data.data.room;
            let owner = &room.owner;

            let cover = room
                .cover
                .as_ref()
                .and_then(|c| c.url_list.first().cloned());
            let hls_url = room
                .stream_url
                .as_ref()
                .map(|s| s.hls_pull_url.clone())
                .unwrap_or_default();

            return Ok(DouyinBasicRoomInfo {
                room_id_str: room.id_str.clone(),
                room_title: room.title.clone(),
                cover,
                status: if room.status == 2 { 0 } else { 1 },
                hls_url,
                user_name: owner.nickname.clone(),
                user_avatar: owner
                    .avatar_thumb
                    .url_list
                    .first()
                    .unwrap_or(&String::new())
                    .clone(),
                sec_user_id: owner.sec_uid.clone(),
                stream_data: room
                    .stream_url
                    .as_ref()
                    .map(|s| s.live_core_sdk_data.pull_data.stream_data.clone())
                    .unwrap_or_default(),
            });
        }

        // If that fails, try to parse as a generic JSON to see what we got
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&text) {
            // Check if it's an error response
            if let Some(status_code) = json_value
                .get("status_code")
                .and_then(serde_json::Value::as_i64)
            {
                if status_code != 0 {
                    let error_msg = json_value
                        .get("data")
                        .and_then(|v| v.get("message").and_then(|v| v.as_str()))
                        .unwrap_or("Unknown error");

                    if status_code == 10011 {
                        return Err(RecorderError::ApiError {
                            error: error_msg.to_string(),
                        });
                    }

                    return Err(RecorderError::ApiError {
                        error: format!(
                            "API returned error status_code: {status_code} - {error_msg}"
                        ),
                    });
                }
            }

            // 检查是否是"invalid session"错误
            if let Some(status_message) = json_value.get("status_message").and_then(|v| v.as_str())
            {
                if status_message.contains("invalid session") {
                    return Err(RecorderError::ApiError { error:
                            "Invalid session - please check your cookies. Make sure you have valid sessionid, passport_csrf_token, and other authentication cookies from douyin.com".to_string(),
                        });
                }
            }

            return Err(RecorderError::ApiError {
                error: format!("Failed to parse h5 room info response: {text}"),
            });
        }
        log::error!("Failed to parse h5 room info response: {text}");
        return Err(RecorderError::ApiError {
            error: format!("Failed to parse h5 room info response: {text}"),
        });
    }

    log::error!("Failed to get h5 room info: {status}");
    Err(RecorderError::ApiError {
        error: format!("Failed to get h5 room info: {status} {text}"),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DouyinQrInfo {
    pub oauth_key: String,
    pub url: String,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DouyinQrStatus {
    pub code: u8,
    pub cookies: String,
    pub message: String,
}

fn parse_set_cookies(headers: &reqwest::header::HeaderMap) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for value in headers.get_all(SET_COOKIE).iter() {
        if let Ok(raw) = value.to_str() {
            if let Some((pair, _)) = raw.split_once(';') {
                if let Some((name, val)) = pair.split_once('=') {
                    cookies.insert(name.trim().to_string(), val.trim().to_string());
                }
            }
        }
    }
    cookies
}

fn parse_cookie_header(cookie_header: &str) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for part in cookie_header.split(';') {
        if let Some((name, value)) = part.trim().split_once('=') {
            if !name.is_empty() {
                cookies.insert(name.to_string(), value.trim().to_string());
            }
        }
    }
    cookies
}

fn format_cookie_header(cookies: &HashMap<String, String>) -> String {
    cookies
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn has_login_cookie(cookies: &HashMap<String, String>) -> bool {
    for key in cookies.keys() {
        let lower = key.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "sessionid"
                | "sessionid_ss"
                | "sid_guard"
                | "sid_tt"
                | "uid_tt"
                | "uid_tt_ss"
                | "login_token"
                | "passport_csrf_token"
                | "passport_csrf_token_default"
        ) {
            return true;
        }
    }
    false
}

fn build_query_string_owned(params: &[(String, String)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in params {
        serializer.append_pair(k, v);
    }
    serializer.finish()
}

fn read_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn env_or(key: &str, default: &str) -> String {
    read_env(key).unwrap_or_else(|| default.to_string())
}

fn reverse_generate_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let root = if cwd.file_name().and_then(|s| s.to_str()) == Some("src-tauri") {
        cwd.parent().unwrap_or(&cwd).to_path_buf()
    } else {
        cwd
    };
    Some(
        root.join("src-tauri")
            .join("crates")
            .join("recorder")
            .join("src")
            .join("ReverseGenerate"),
    )
}

fn douyin_overrides_path() -> Option<PathBuf> {
    if let Some(path) = read_env("DOUYIN_PASSPORT_OVERRIDES_FILE") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    let root = reverse_generate_root()?;
    Some(root.join("qr_login.toml"))
}

fn push_param(params: &mut Vec<(String, String)>, key: &str, value: String) {
    if !value.is_empty() {
        params.push((key.to_string(), value));
    }
}

fn apply_passport_cookie_overrides(
    overrides: &mut HashMap<String, String>,
    cookies: &mut HashMap<String, String>,
) {
    if !overrides.contains_key("verifyFp") {
        if let Some(value) = cookies
            .get("s_v_web_id")
            .or_else(|| cookies.get("verifyFp"))
            .or_else(|| cookies.get("verify_fp"))
        {
            overrides.insert("verifyFp".to_string(), value.to_string());
        } else {
            let generated = params::gen_verify_fp();
            overrides.insert("verifyFp".to_string(), generated.clone());
            cookies.entry("s_v_web_id".to_string()).or_insert(generated);
        }
    }
    if !overrides.contains_key("fp") {
        if let Some(value) = overrides.get("verifyFp") {
            overrides.insert("fp".to_string(), value.to_string());
        }
    }
    if !overrides.contains_key("msToken") {
        if let Some(value) = cookies.get("msToken") {
            overrides.insert("msToken".to_string(), value.to_string());
        } else {
            let generated = generate_ms_token();
            overrides.insert("msToken".to_string(), generated.clone());
            cookies.entry("msToken".to_string()).or_insert(generated);
        }
    }
}

fn override_value(overrides: Option<&HashMap<String, String>>, keys: &[&str]) -> Option<String> {
    let map = overrides?;
    for key in keys {
        if let Some(value) = map.get(*key) {
            if !value.is_empty() {
                return Some(value.clone());
            }
        }
    }
    None
}

fn param_value(
    overrides: Option<&HashMap<String, String>>,
    keys: &[&str],
    env_key: &str,
    default: &str,
) -> String {
    override_value(overrides, keys).unwrap_or_else(|| env_or(env_key, default))
}

fn build_douyin_passport_params(
    include_next: bool,
    overrides: Option<&HashMap<String, String>>,
) -> Vec<(String, String)> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let mut params = Vec::new();
    push_param(
        &mut params,
        "passport_jssdk_version",
        param_value(
            overrides,
            &["passport_jssdk_version"],
            "DOUYIN_PASSPORT_JSSDK_VERSION",
            "3.1.3",
        ),
    );
    push_param(
        &mut params,
        "cookie_enabled",
        param_value(
            overrides,
            &["cookie_enabled"],
            "DOUYIN_COOKIE_ENABLED",
            "true",
        ),
    );
    push_param(
        &mut params,
        "passport_jssdk_type",
        param_value(
            overrides,
            &["passport_jssdk_type"],
            "DOUYIN_PASSPORT_JSSDK_TYPE",
            "normal",
        ),
    );
    push_param(
        &mut params,
        "is_from_ttaccountsdk",
        param_value(
            overrides,
            &["is_from_ttaccountsdk"],
            "DOUYIN_IS_FROM_TTACCOUNTSDK",
            "1",
        ),
    );
    push_param(
        &mut params,
        "aid",
        param_value(overrides, &["aid"], "DOUYIN_AID", "6383"),
    );
    push_param(
        &mut params,
        "browser_language",
        param_value(
            overrides,
            &["browser_language"],
            "DOUYIN_BROWSER_LANGUAGE",
            "zh-CN",
        ),
    );
    push_param(
        &mut params,
        "browser_platform",
        param_value(
            overrides,
            &["browser_platform"],
            "DOUYIN_BROWSER_PLATFORM",
            "Win32",
        ),
    );
    push_param(
        &mut params,
        "browser_name",
        param_value(
            overrides,
            &["browser_name"],
            "DOUYIN_BROWSER_NAME",
            "Chrome",
        ),
    );
    push_param(
        &mut params,
        "browser_version",
        param_value(
            overrides,
            &["browser_version"],
            "DOUYIN_BROWSER_VERSION",
            "138.0.0.0",
        ),
    );
    push_param(
        &mut params,
        "browser_online",
        param_value(
            overrides,
            &["browser_online"],
            "DOUYIN_BROWSER_ONLINE",
            "true",
        ),
    );
    push_param(
        &mut params,
        "screen_width",
        param_value(
            overrides,
            &["screen_width"],
            "DOUYIN_SCREEN_WIDTH",
            "2560",
        ),
    );
    push_param(
        &mut params,
        "screen_height",
        param_value(
            overrides,
            &["screen_height"],
            "DOUYIN_SCREEN_HEIGHT",
            "1440",
        ),
    );
    push_param(
        &mut params,
        "os_name",
        param_value(overrides, &["os_name"], "DOUYIN_OS_NAME", "Windows"),
    );
    push_param(
        &mut params,
        "os_version",
        param_value(overrides, &["os_version"], "DOUYIN_OS_VERSION", "10"),
    );
    push_param(
        &mut params,
        "platform",
        param_value(overrides, &["platform"], "DOUYIN_PLATFORM", "PC"),
    );
    push_param(
        &mut params,
        "language",
        param_value(overrides, &["language"], "DOUYIN_LANGUAGE", "zh"),
    );
    push_param(
        &mut params,
        "account_app_language",
        param_value(
            overrides,
            &["account_app_language"],
            "DOUYIN_ACCOUNT_APP_LANGUAGE",
            "zh-CN",
        ),
    );
    push_param(
        &mut params,
        "ts",
        param_value(overrides, &["ts"], "DOUYIN_TS", &now_secs),
    );
    if include_next {
        push_param(
            &mut params,
            "next",
            param_value(
                overrides,
                &["next"],
                "DOUYIN_NEXT",
                "https://www.douyin.com",
            ),
        );
    }
    push_param(
        &mut params,
        "need_short_url",
        param_value(
            overrides,
            &["need_short_url"],
            "DOUYIN_NEED_SHORT_URL",
            "true",
        ),
    );
    push_param(
        &mut params,
        "need_logo",
        param_value(
            overrides,
            &["need_logo"],
            "DOUYIN_NEED_LOGO",
            "false",
        ),
    );
    push_param(
        &mut params,
        "is_new_login",
        param_value(
            overrides,
            &["is_new_login"],
            "DOUYIN_IS_NEW_LOGIN",
            "1",
        ),
    );
    push_param(
        &mut params,
        "is_from_iesaccountsaas",
        param_value(
            overrides,
            &["is_from_iesaccountsaas"],
            "DOUYIN_IS_FROM_IESACCOUNTSAAS",
            "1",
        ),
    );
    push_param(&mut params, "p_ui", env_or("DOUYIN_P_UI", "2.1.4"));
    push_param(&mut params, "p_ca", env_or("DOUYIN_P_CA", "4.0.17"));
    push_param(
        &mut params,
        "p_ca_real",
        param_value(
            overrides,
            &["p_ca_real"],
            "DOUYIN_P_CA_REAL",
            "1.0.0.729",
        ),
    );
    push_param(
        &mut params,
        "account_sdk_source",
        param_value(
            overrides,
            &["account_sdk_source"],
            "DOUYIN_ACCOUNT_SDK_SOURCE",
            "web",
        ),
    );
    push_param(
        &mut params,
        "account_sdk_source_info",
        override_value(overrides, &["account_sdk_source_info"])
            .or_else(|| read_env("DOUYIN_ACCOUNT_SDK_SOURCE_INFO"))
            .unwrap_or_default(),
    );
    push_param(
        &mut params,
        "service",
        param_value(
            overrides,
            &["service"],
            "DOUYIN_SERVICE",
            "https://www.douyin.com",
        ),
    );
    push_param(&mut params, "p_js_v", env_or("DOUYIN_P_JS_V", "3.1.3"));
    push_param(&mut params, "p_js_t", env_or("DOUYIN_P_JS_T", "pro"));
    push_param(&mut params, "p_zt", env_or("DOUYIN_P_ZT", "3.3.10"));
    push_param(&mut params, "p_ver", env_or("DOUYIN_P_VER", "1.1.3"));
    push_param(&mut params, "p_ver_real", env_or("DOUYIN_P_VER_REAL", "0"));
    push_param(
        &mut params,
        "request_host",
        param_value(
            overrides,
            &["request_host"],
            "DOUYIN_REQUEST_HOST",
            "https://www.douyin.com",
        ),
    );
    push_param(
        &mut params,
        "p_bd",
        param_value(overrides, &["p_bd"], "DOUYIN_P_BD", "1.0.1.19-fix.01"),
    );
    push_param(
        &mut params,
        "p_ts",
        param_value(overrides, &["p_ts"], "DOUYIN_P_TS", &now_ms),
    );
    push_param(
        &mut params,
        "p_no",
        override_value(overrides, &["p_no"])
            .or_else(|| read_env("DOUYIN_P_NO"))
            .unwrap_or_else(|| now_secs.clone()),
    );
    push_param(
        &mut params,
        "biz_trace_id",
        override_value(overrides, &["biz_trace_id"])
            .or_else(|| read_env("DOUYIN_BIZ_TRACE_ID"))
            .unwrap_or_else(|| now_ms.clone()),
    );
    push_param(
        &mut params,
        "device_platform",
        param_value(
            overrides,
            &["device_platform"],
            "DOUYIN_DEVICE_PLATFORM",
            "web_app",
        ),
    );
    let verify_fp = override_value(overrides, &["verifyFp", "verify_fp"])
        .or_else(|| read_env("DOUYIN_VERIFY_FP"))
        .unwrap_or_else(|| params::gen_verify_fp());
    let fp = override_value(overrides, &["fp"])
        .or_else(|| read_env("DOUYIN_FP"))
        .unwrap_or_else(|| verify_fp.clone());
    push_param(&mut params, "verifyFp", verify_fp);
    push_param(&mut params, "fp", fp);
    let ms_token = override_value(overrides, &["msToken", "ms_token"])
        .or_else(|| read_env("DOUYIN_MS_TOKEN"))
        .unwrap_or_else(|| generate_ms_token());
    push_param(&mut params, "msToken", ms_token);
    if let Some(sign) = override_value(overrides, &["sign"]).or_else(|| read_env("DOUYIN_SIGN")) {
        push_param(&mut params, "sign", sign);
    }
    if let Some(qs) = override_value(overrides, &["qs"]).or_else(|| read_env("DOUYIN_QS")) {
        push_param(&mut params, "qs", qs);
    }
    let param_str = build_query_string_owned(&params);
    let a_bogus = override_value(overrides, &["a_bogus", "a-bogus"])
        .or_else(|| read_env("DOUYIN_A_BOGUS"))
        .unwrap_or_else(|| generate_a_bogus(&param_str));
    push_param(&mut params, "a_bogus", a_bogus);
    params
}

fn apply_douyin_passport_headers(
    headers: &mut reqwest::header::HeaderMap,
    passport_csrf: &str,
    overrides: Option<&HashMap<String, String>>,
) {
    let origin = override_value(overrides, &["qr_origin", "origin"])
        .unwrap_or_else(|| env_or("DOUYIN_QR_ORIGIN", "https://www.douyin.com"));
    let referer = override_value(overrides, &["qr_referer", "referer"])
        .unwrap_or_else(|| env_or("DOUYIN_QR_REFERER", "https://www.douyin.com/"));
    headers.insert("Origin", origin.parse().unwrap());
    headers.insert("Referer", referer.parse().unwrap());
    let csrf = override_value(
        overrides,
        &["x_tt_passport_csrf_token", "x-tt-passport-csrf-token"],
    )
    .or_else(|| read_env("DOUYIN_X_TT_PASSPORT_CSRF_TOKEN"))
    .unwrap_or_else(|| passport_csrf.to_string());
    if !csrf.is_empty() {
        headers.insert("x-tt-passport-csrf-token", csrf.parse().unwrap());
    }
    if let Some(value) = override_value(
        overrides,
        &["x_tt_passport_aid_sign", "x-tt-passport-aid-sign"],
    )
    .or_else(|| read_env("DOUYIN_X_TT_PASSPORT_AID_SIGN"))
    {
        headers.insert("x-tt-passport-aid-sign", value.parse().unwrap());
    }
    if let Some(value) = override_value(
        overrides,
        &["x_tt_passport_trace_id", "x-tt-passport-trace-id"],
    )
    .or_else(|| read_env("DOUYIN_X_TT_PASSPORT_TRACE_ID"))
    {
        headers.insert("x-tt-passport-trace-id", value.parse().unwrap());
    }
    if let Some(value) = override_value(
        overrides,
        &["x_tt_passport_verify_portrait", "x-tt-passport-verify-portrait"],
    )
    .or_else(|| read_env("DOUYIN_X_TT_PASSPORT_VERIFY_PORTRAIT"))
    {
        headers.insert("x-tt-passport-verify-portrait", value.parse().unwrap());
    }
    if let Some(value) = override_value(
        overrides,
        &["x_tt_session_dtrait", "x-tt-session-dtrait"],
    )
    .or_else(|| read_env("DOUYIN_X_TT_SESSION_DTRAIT"))
    {
        headers.insert("x-tt-session-dtrait", value.parse().unwrap());
    }
}

fn apply_douyin_browser_headers(
    headers: &mut reqwest::header::HeaderMap,
    overrides: Option<&HashMap<String, String>>,
) {
    let defaults = [
        ("accept", "application/json, text/javascript"),
        ("accept-encoding", "gzip, deflate, br, zstd"),
        ("accept-language", "zh-CN,zh;q=0.9,en;q=0.8"),
        (
            "sec-ch-ua",
            "\"Not)A;Brand\";v=\"8\", \"Chromium\";v=\"138\", \"Google Chrome\";v=\"138\"",
        ),
        ("sec-ch-ua-mobile", "?0"),
        ("sec-ch-ua-platform", "\"Windows\""),
        ("sec-fetch-dest", "empty"),
        ("sec-fetch-mode", "cors"),
        ("sec-fetch-site", "same-site"),
    ];
    for (key, value) in defaults {
        let override_key = key.replace('-', "_");
        let picked = override_value(overrides, &[key, override_key.as_str()])
            .or_else(|| read_env(&format!("DOUYIN_{}", override_key.to_ascii_uppercase())));
        let value = picked.unwrap_or_else(|| value.to_string());
        if let Ok(parsed) = value.parse() {
            headers.insert(key, parsed);
        }
    }
}

async fn fetch_douyin_passport_overrides(
    client: &Client,
) -> Option<HashMap<String, String>> {
    let mut map = HashMap::new();

    if let Some(path) = douyin_overrides_path() {
        if let Ok(raw) = fs::read_to_string(&path) {
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let mut parsed = false;
            if ext == "json" || ext.is_empty() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                    let data = json.get("data").unwrap_or(&json);
                    if let Some(obj) = data.as_object() {
                        for (k, v) in obj {
                            if let Some(s) = v.as_str() {
                                map.insert(k.clone(), s.to_string());
                            } else if v.is_number() || v.is_boolean() {
                                map.insert(k.clone(), v.to_string());
                            } else if v.is_object() || v.is_array() {
                                map.insert(k.clone(), v.to_string());
                            }
                        }
                    }
                    parsed = true;
                }
            }
            if !parsed {
                if let Ok(toml) = raw.parse::<TomlValue>() {
                    if let Some(table) = toml.as_table() {
                        if let Some(douyin) = table.get("douyin_passport") {
                            collect_toml_kv(douyin, &mut map);
                        } else if let Some(douyin) = table.get("douyin") {
                            collect_toml_kv(douyin, &mut map);
                        } else if let Some(qr_login) = table.get("qr_login") {
                            if let Some(qr_table) = qr_login.as_table() {
                                if let Some(douyin) = qr_table.get("douyin_passport") {
                                    collect_toml_kv(douyin, &mut map);
                                } else if let Some(douyin) = qr_table.get("douyin") {
                                    collect_toml_kv(douyin, &mut map);
                                }
                            }
                        } else {
                            collect_toml_kv(&toml, &mut map);
                        }
                    }
                }
            }
        }
    }

    if let Some(url) = read_env("DOUYIN_PASSPORT_PROVIDER_URL") {
        if !url.trim().is_empty() {
            if let Ok(resp) = client.get(url).send().await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let data = json.get("data").unwrap_or(&json);
                    if let Some(obj) = data.as_object() {
                        for (k, v) in obj {
                            if let Some(s) = v.as_str() {
                                map.entry(k.clone()).or_insert_with(|| s.to_string());
                            } else if v.is_number() || v.is_boolean() {
                                map.entry(k.clone()).or_insert_with(|| v.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    if map.is_empty() { None } else { Some(map) }
}

fn collect_toml_kv(value: &TomlValue, map: &mut HashMap<String, String>) {
    let Some(table) = value.as_table() else {
        return;
    };
    for (k, v) in table {
        if let Some(s) = v.as_str() {
            map.insert(k.to_string(), s.to_string());
        } else if v.is_integer() || v.is_float() || v.is_bool() {
            map.insert(k.to_string(), v.to_string());
        } else if v.is_table() || v.is_array() {
            map.insert(k.to_string(), v.to_string());
        }
    }
}

async fn fetch_passport_csrf(
    client: &Client,
    headers: &reqwest::header::HeaderMap,
) -> Option<(String, HashMap<String, String>)> {
    let mut csrf_headers = headers.clone();
    if !csrf_headers.contains_key("Referer") {
        csrf_headers.insert("Referer", "https://www.douyin.com/".parse().unwrap());
    }
    let resp = client
        .get("https://www.douyin.com/")
        .headers(csrf_headers)
        .send()
        .await
        .ok()?;
    let cookies = parse_set_cookies(resp.headers());
    let csrf = cookies.get("passport_csrf_token").cloned().unwrap_or_default();
    Some((csrf, cookies))
}

pub async fn get_qr_login(client: &Client) -> Result<DouyinQrInfo, RecorderError> {
    let proxy_url = douyin_proxy_url_from_env();
    let request_client = if let Some(proxy_url) = proxy_url.as_deref() {
        build_douyin_proxy_client(proxy_url)?
    } else {
        client.clone()
    };
    let proxy_display = proxy_url.as_deref().unwrap_or("direct");
    log::info!(
        "[Douyin] QR get_qrcode request host=https://login.douyin.com, proxy={}",
        proxy_display
    );
    let mut headers = generate_passport_headers();

    let mut cookies = HashMap::new();
    let passport_csrf = fetch_passport_csrf(&request_client, &headers)
        .await
        .map(|(csrf, fetched)| {
            cookies = fetched;
            csrf
        })
        .unwrap_or_default();
    let mut overrides = fetch_douyin_passport_overrides(&request_client).await.unwrap_or_default();
    apply_passport_cookie_overrides(&mut overrides, &mut cookies);
    apply_douyin_browser_headers(&mut headers, Some(&overrides));
    apply_douyin_passport_headers(&mut headers, &passport_csrf, Some(&overrides));
    let cookie_header = format_cookie_header(&cookies);
    if !cookie_header.is_empty() {
        headers.insert("Cookie", cookie_header.parse().unwrap());
    }
    if env_or("DOUYIN_PASSPORT_CHALLENGE", "1") == "1" {
        fetch_douyin_passport_challenge(&request_client, &headers, &mut cookies, Some(&overrides))
            .await;
        let cookie_header = format_cookie_header(&cookies);
        if !cookie_header.is_empty() {
            headers.insert("Cookie", cookie_header.parse().unwrap());
        }
    }
    if env_or("DOUYIN_TTWID_CHECK", "1") == "1" {
        fetch_ttwid_check(&request_client, &headers, &mut cookies, Some(&overrides)).await;
        let cookie_header = format_cookie_header(&cookies);
        if !cookie_header.is_empty() {
            headers.insert("Cookie", cookie_header.parse().unwrap());
        }
    }

    let url = {
        let raw_query = override_value(Some(&overrides), &["params_raw"])
            .or_else(|| read_env("DOUYIN_PASSPORT_PARAMS_RAW"));
        let param_str = if let Some(raw) = raw_query {
            normalize_params_raw(&raw)
        } else {
            let query_params = build_douyin_passport_params(true, Some(&overrides));
            build_query_string_owned(&query_params)
        };
        format!(
            "https://login.douyin.com/passport/web/get_qrcode/?{param_str}"
        )
    };

    let resp = request_client.get(url).headers(headers.clone()).send().await?;
    let resp_headers = resp.headers().clone();
    let json: serde_json::Value = resp.json().await?;
    log::warn!("[Douyin] get_qr_login response: {}", json);
    let data = json
        .get("data")
        .ok_or_else(|| RecorderError::ApiError {
            error: "Douyin QR: missing data".to_string(),
        })?;
    let resp_cookies = parse_set_cookies(&resp_headers);
    for (k, v) in resp_cookies {
        cookies.insert(k, v);
    }
    let cookie_header = format_cookie_header(&cookies);
    let cookie_b64 = if cookie_header.is_empty() {
        String::new()
    } else {
        general_purpose::STANDARD.encode(cookie_header.as_bytes())
    };
    let token = data
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let qr_url = data
        .get("qrcode_index_url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let qr_image = data
        .get("qrcode")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string());

    Ok(DouyinQrInfo {
        oauth_key: if cookie_b64.is_empty() {
            format!("{token}|{passport_csrf}")
        } else {
            format!("{token}|{passport_csrf}|{cookie_b64}")
        },
        url: qr_url,
        image: qr_image,
    })
}

pub async fn get_qr_login_status(
    client: &Client,
    token_with_csrf: &str,
) -> Result<DouyinQrStatus, RecorderError> {
    let mut parts = token_with_csrf.split('|');
    let token = parts.next().unwrap_or_default();
    let passport_csrf = parts.next().unwrap_or_default();
    let cookie_header = parts
        .next()
        .and_then(|encoded| general_purpose::STANDARD.decode(encoded).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();

    let proxy_url = douyin_proxy_url_from_env();
    let request_client = if let Some(proxy_url) = proxy_url.as_deref() {
        build_douyin_proxy_client(proxy_url)?
    } else {
        client.clone()
    };
    let proxy_display = proxy_url.as_deref().unwrap_or("direct");
    log::info!(
        "[Douyin] QR status request host=https://login.douyin.com, proxy={}",
        proxy_display
    );
    let mut headers = generate_passport_headers();
    let mut cookie_map = parse_cookie_header(&cookie_header);
    let mut overrides = fetch_douyin_passport_overrides(&request_client).await.unwrap_or_default();
    apply_passport_cookie_overrides(&mut overrides, &mut cookie_map);
    apply_douyin_browser_headers(&mut headers, Some(&overrides));
    apply_douyin_passport_headers(&mut headers, passport_csrf, Some(&overrides));
    let merged_cookie_header = format_cookie_header(&cookie_map);
    if !merged_cookie_header.is_empty() {
        headers.insert("Cookie", merged_cookie_header.parse().unwrap());
    }
    if env_or("DOUYIN_PASSPORT_CHALLENGE", "1") == "1" {
        fetch_douyin_passport_challenge(&request_client, &headers, &mut cookie_map, Some(&overrides))
            .await;
        let merged_cookie_header = format_cookie_header(&cookie_map);
        if !merged_cookie_header.is_empty() {
            headers.insert("Cookie", merged_cookie_header.parse().unwrap());
        }
    }

    let url = {
        let raw_query = override_value(Some(&overrides), &["params_raw_status", "params_raw"])
            .or_else(|| read_env("DOUYIN_PASSPORT_PARAMS_RAW_STATUS"))
            .or_else(|| read_env("DOUYIN_PASSPORT_PARAMS_RAW"));
        let param_str = if let Some(raw) = raw_query {
            normalize_params_raw(&raw)
        } else {
            let mut query_params = build_douyin_passport_params(false, Some(&overrides));
            push_param(&mut query_params, "token", token.to_string());
            build_query_string_owned(&query_params)
        };
        format!(
            "https://login.douyin.com/passport/web/check_qrconnect/?{param_str}"
        )
    };

    let resp = request_client
        .post(&url)
        .headers(headers.clone())
        .form(&[("token", token)])
        .send()
        .await?;
    let mut resp_headers = resp.headers().clone();
    let mut json: serde_json::Value = resp.json().await?;
    log::warn!("[Douyin] get_qr_login_status response: {}", json);
    let mut error_code = json
        .get("data")
        .and_then(|v| v.get("error_code"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if error_code == 4031 && proxy_url.is_some() {
        log::warn!("[Douyin] QR status blocked via proxy, retrying direct");
        if let Ok(direct_resp) = client
            .post(&url)
            .headers(headers.clone())
            .form(&[("token", token)])
            .send()
            .await
        {
            resp_headers = direct_resp.headers().clone();
            if let Ok(direct_json) = direct_resp.json::<serde_json::Value>().await {
                json = direct_json;
                log::warn!(
                    "[Douyin] get_qr_login_status direct response: {}",
                    json
                );
                error_code = json
                    .get("data")
                    .and_then(|v| v.get("error_code"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
            }
        }
    }
    let data = json.get("data");
    let status = data
        .and_then(|v| v.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let error_code = data
        .and_then(|v| v.get("error_code"))
        .and_then(|v| v.as_i64())
        .unwrap_or(error_code);
    let description = data
        .and_then(|v| v.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let top_message = json
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if error_code != 0 || top_message == "error" {
        if error_code == 4031 {
            let missing_session = !headers.contains_key("x-tt-session-dtrait");
            let missing_portrait = !headers.contains_key("x-tt-passport-verify-portrait");
            if missing_session || missing_portrait {
                log::warn!(
                    "[Douyin] QR headers missing: session_dtrait={}, verify_portrait={}. Set DOUYIN_X_TT_SESSION_DTRAIT / DOUYIN_X_TT_PASSPORT_VERIFY_PORTRAIT or overrides file.",
                    missing_session,
                    missing_portrait
                );
            }
        }
        let msg = if !description.is_empty() {
            description.to_string()
        } else if error_code != 0 {
            format!("Douyin QR error_code={error_code}")
        } else {
            "Douyin QR error".to_string()
        };
        log::warn!(
            "[Douyin] QR status blocked: error_code={}, message='{}'",
            error_code,
            msg
        );
        return Ok(DouyinQrStatus {
            code: 2,
            cookies: String::new(),
            message: msg,
        });
    }

    if status == "3" || status == "success" {
        let resp_cookies = parse_set_cookies(&resp_headers);
        let mut merged_cookies = cookie_map.clone();
        merged_cookies.extend(resp_cookies);
        if has_login_cookie(&merged_cookies) {
            let cookie_str = format_cookie_header(&merged_cookies);
            log::info!("[Douyin] QR login cookies recovered from status ({} cookies)", merged_cookies.len());
            return Ok(DouyinQrStatus {
                code: 0,
                cookies: cookie_str,
                message: "ok".to_string(),
            });
        }
        let redirect_url = json
            .get("data")
            .and_then(|v| v.get("redirect_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !redirect_url.is_empty() {
            let merged_header = format_cookie_header(&merged_cookies);
            let mut redirect_req = request_client.get(redirect_url).headers(headers.clone());
            if !merged_header.is_empty() {
                redirect_req = redirect_req.header("Cookie", merged_header);
            }
            let redirect_resp = redirect_req.send().await?;
            let mut redirect_cookies = merged_cookies;
            redirect_cookies.extend(parse_set_cookies(redirect_resp.headers()));
            if has_login_cookie(&redirect_cookies) {
                let cookie_str = format_cookie_header(&redirect_cookies);
                log::info!(
                    "[Douyin] QR login cookies recovered from redirect ({} cookies)",
                    redirect_cookies.len()
                );
                return Ok(DouyinQrStatus {
                    code: 0,
                    cookies: cookie_str,
                    message: "ok".to_string(),
                });
            }
        }
        log::warn!("[Douyin] QR login success but cookies missing");
        return Ok(DouyinQrStatus {
            code: 2,
            cookies: String::new(),
            message: "login redirect missing cookies".to_string(),
        });
    }

    Ok(DouyinQrStatus {
        code: 1,
        cookies: String::new(),
        message: status.to_string(),
    })
}

pub async fn get_user_info(
    client: &Client,
    account: &Account,
) -> Result<super::response::User, RecorderError> {
    // Use the IM spotlight relation API to get user info
    let url = "https://www.douyin.com/aweme/v1/web/im/spotlight/relation/";
    let mut headers = generate_user_agent_header();
    headers.insert("Referer", "https://www.douyin.com/".parse().unwrap());
    headers.insert("Cookie", account.cookies.clone().parse().unwrap());

    let resp = client.get(url).headers(headers.clone()).send().await?;

    let status = resp.status();
    let text = resp.text().await?;

    if status.is_success() {
        if let Ok(data) = serde_json::from_str::<super::response::DouyinRelationResponse>(&text) {
            if data.status_code == 0 {
                let owner_sec_uid = &data.owner_sec_uid;

                // Find the user's own info in the followings list by matching sec_uid
                if let Some(followings) = &data.followings {
                    for following in followings {
                        if following.sec_uid == *owner_sec_uid {
                            let user = super::response::User {
                                id_str: following.uid.clone(),
                                sec_uid: following.sec_uid.clone(),
                                nickname: following.nickname.clone(),
                                avatar_thumb: following.avatar_thumb.clone(),
                                follow_info: super::response::FollowInfo::default(),
                                foreign_user: 0,
                                open_id_str: String::new(),
                            };
                            return Ok(user);
                        }
                    }
                }

                // If not found in followings, create a minimal user info from owner_sec_uid
                let user = super::response::User {
                    id_str: String::new(), // We don't have the numeric UID
                    sec_uid: owner_sec_uid.clone(),
                    nickname: "抖音用户".to_string(), // Default nickname
                    avatar_thumb: super::response::AvatarThumb { url_list: vec![] },
                    follow_info: super::response::FollowInfo::default(),
                    foreign_user: 0,
                    open_id_str: String::new(),
                };
                return Ok(user);
            }
        } else {
            log::error!("Failed to parse user info response: {text}");
            return Err(RecorderError::ApiError {
                error: format!("Failed to parse user info response: {text}"),
            });
        }
    }

    log::error!("Failed to get user info: {status}");

    Err(RecorderError::ApiError {
        error: format!("Failed to get user info: {status} {text}"),
    })
}

pub async fn get_room_owner_sec_uid(
    client: &Client,
    room_id: &str,
) -> Result<String, RecorderError> {
    let url = format!("https://live.douyin.com/{room_id}");
    let mut headers = generate_user_agent_header();
    headers.insert("Referer", "https://live.douyin.com/".parse().unwrap());
    let resp = client.get(url).headers(headers.clone()).send().await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(RecorderError::ApiError {
            error: format!("Failed to get room owner sec uid: {status} {text}"),
        });
    }
    // match to get sec_uid from text like \"sec_uid\":\"MS4wLjABAAAAdFmmud36bynPjXOvoMjatb42856_zryHsGmlkpIECDA\"
    let sec_uid = Regex::new(r#"\\"sec_uid\\":\\"(.*?)\\""#)
        .unwrap()
        .captures(&text)
        .and_then(|c| c.get(1))
        .ok_or_else(|| RecorderError::ApiError {
            error: "Failed to find sec_uid in room page".to_string(),
        })?
        .as_str()
        .to_string();
    Ok(sec_uid)
}

/// Download file from url to path
pub async fn download_file(client: &Client, url: &str, path: &Path) -> Result<(), RecorderError> {
    if !path.parent().unwrap().exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    let response = client.get(url).send().await?;
    let bytes = response.bytes().await?;
    let mut file = tokio::fs::File::create(&path).await?;
    let mut content = std::io::Cursor::new(bytes);
    tokio::io::copy(&mut content, &mut file).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_room_owner_sec_uid() {
        let client = crate::utils::no_proxy_client();
        let sec_uid = get_room_owner_sec_uid(&client, "200525029536")
            .await
            .unwrap();
        assert_eq!(
            sec_uid,
            "MS4wLjABAAAAdFmmud36bynPjXOvoMjatb42856_zryHsGmlkpIECDA"
        );
    }
}
