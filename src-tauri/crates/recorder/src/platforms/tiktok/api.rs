use super::response::{RoomInfo as SigiRoomInfo, SigiStateResponse, StreamUrl as SigiStreamUrl};
use crate::account::Account;
use crate::errors::RecorderError;
use chrono::Utc;
use rand::Rng;
use regex::Regex;
use reqwest::header::{HeaderMap, LOCATION};
use reqwest::redirect::Policy;
use reqwest::{Client, Proxy, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::sync::atomic::{AtomicI64, Ordering};
use url::form_urlencoded::Serializer;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
const DEFAULT_COOKIE: &str = "1%7Cz7FKki38aKyy7i-BC9rEDwcrVvjcLcFEL6QIeqldoy4%7C1761302831%7C6c1461e9f1f980cbe0404c51905177d5d53bbd822e1bf66128887d942c9c3e2f";
const TIKTOK_COOLDOWN_SECS: i64 = 120;
const TIKTOK_MSSDK_URL: &str = "https://mssdk.tiktokw.us/web/report?msToken=1Ab-7YxR9lUHSem0PraI_XzdKmpHb6j50L8AaXLAd2aWTdoJCYLfX_67rVQFE4UwwHVHmyG_NfIipqrlLT3kCXps-5PYlNAqtdwEg7TrDyTAfCKyBrOLmhMUjB55oW8SPZ4_EkNxNFUdV7MquA==";
const TIKTOK_MSSDK_MAGIC: i64 = 538969122;
const TIKTOK_MSSDK_VERSION: i64 = 1;
const TIKTOK_MSSDK_DATA_TYPE: i64 = 8;
const TIKTOK_MSSDK_STR_DATA: &str = "3BvqYbNXLLOcZehvxZVbjpAu7vq82RoWmFSJHLFwzDwJIZevE0AeilQfP55LridxmdGGjknoksqIsLqlMHMif0IFK/Br7JWqxOHnYuMwVCnttFc0Y4MFvdVWM5FECiEulJC0Dc+eeVsNSrFnAc9K7fazqdglyJgGLSfXIJmgyCvvQ4pg0u5HBVVugLSWs242X42fjoWymaUCLZJQo6vi6WLyuV7l5IC3Mg+lelr5xBQD6Q7hBIFEw8zzxJ1n2DyA4xLbOHTQdKvEtsK7XzyWwjpRnojPTbBl69Zosnuru+lOBIl+tFu/+hCQ1m0jYZwTP4rVE75L3Du6+KZ5v/9TyFYjq7y3y9bGLP4d7yQueJbF90G1yrZ6htElrZ2vqZKDrIqBVbmOZr/nph12k2JKrITtN0R/pMsp0sJ4gesQnXxcD/pLOFAINHk7umgbe6LzJ7+TLUdGuO4M7xiEg/jCqhjgJX1izZ4NPoBDp35zRxj6Y6OrcstlTN/cv5sz663+Nco/mEwhGq2VwrL4gAIAPycndIsb48dPdtngmLqNDNN0ZyVRjgqVIDXXrxigXCkR9CH89Dlrrb7QQqWVgRXz9/k5ihEM43BR3sd3mMU/XgFLN1Aoxf6GzzdxP2QPBI75/ZoHoAmu54v8gTmA3ntCGlEF0zgaFGTdpkGdb+oZgyQM4pw1aAyxmFINXkpD3IKKoGev9kD9gTFnhiQMGCMemhZS7ZYdbuGu0Cb+lQKaL/QTt80FMyGmW8kzVy9xW/ja9BcdEJYRoaufuFRkBFG5ay8x4WHLR6hEapXqQial/cREbLL4sQytpjtmnndFqvT7xN5DhgsLY2Z7451MJhD6NJXKNrMafGZSbItzQWY=";
const TIKTOK_TTWID_CHECK_URL: &str = "https://www.tiktok.com/ttwid/check/";
const TIKTOK_TTWID_CHECK_DATA: &str = "{\"aid\":1988,\"service\":\"www.tiktok.com\",\"union\":false,\"unionHost\":\"\",\"needFid\":false,\"fid\":\"\",\"migrate_priority\":0}";

static TIKTOK_COOLDOWN_UNTIL: AtomicI64 = AtomicI64::new(0);

fn tiktok_api_allowed() -> bool {
    let now = Utc::now().timestamp();
    now >= TIKTOK_COOLDOWN_UNTIL.load(Ordering::Relaxed)
}

fn set_tiktok_cooldown(reason: &str) {
    let until = Utc::now().timestamp() + TIKTOK_COOLDOWN_SECS;
    TIKTOK_COOLDOWN_UNTIL.store(until, Ordering::Relaxed);
    log::info!("[TikTok] API cooldown set ({}s): {}", TIKTOK_COOLDOWN_SECS, reason);
}

#[derive(Clone, Debug)]
pub struct RoomInfo {
    pub live_status: bool,
    pub room_title: String,
    pub room_cover_url: String,
    pub user_id: String,
    pub user_name: String,
    pub user_avatar: String,
}

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub hls_url: Option<String>,
    pub rtmp_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TikTokQrInfo {
    pub oauth_key: String,
    pub url: String,
    pub image: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TikTokQrStatus {
    pub code: u8,
    pub cookies: String,
    pub message: String,
}

pub fn proxy_url_from_env() -> Option<String> {
    for key in ["TIKTOK_PROXY_URL", "tiktok_proxy_url"] {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

pub fn build_proxy_client(proxy_url: &str) -> Result<Client, RecorderError> {
    let proxy = Proxy::all(proxy_url).map_err(|_| RecorderError::ApiError {
        error: "Invalid proxy URL".to_string(),
    })?;
    Client::builder()
        .proxy(proxy)
        .http1_only()
        .build()
        .map_err(|_| RecorderError::ApiError {
            error: "Failed to build proxy client".to_string(),
        })
}

fn extract_script_json(html_str: &str, script_id: &str) -> Option<String> {
    let pattern = format!(
        r#"(?s)<script[^>]*id=['"]{script_id}['"][^>]*>(.*?)</script>"#
    );
    let regex = Regex::new(&pattern).ok()?;
    regex
        .captures(html_str)
        .and_then(|cap| cap.get(1))
        .map(|value| value.as_str().to_string())
}

fn parse_first_json_value(raw: &str) -> Option<Value> {
    let start = raw
        .find('{')
        .or_else(|| raw.find('['))?;
    let candidate = &raw[start..];
    let mut deserializer = serde_json::Deserializer::from_str(candidate);
    Value::deserialize(&mut deserializer).ok()
}

fn extract_json_after_marker(html_str: &str, marker: &str) -> Option<Value> {
    for (index, _) in html_str.match_indices(marker) {
        let slice = &html_str[index + marker.len()..];
        if let Some(value) = parse_first_json_value(slice) {
            return Some(value);
        }
    }
    None
}

fn extract_state_value(html_str: &str) -> Option<Value> {
    let script_ids = ["SIGI_STATE", "__UNIVERSAL_DATA_FOR_REHYDRATION__", "__NEXT_DATA__"];
    for script_id in script_ids {
        if let Some(json_str) = extract_script_json(html_str, script_id) {
            if let Some(parsed) = parse_first_json_value(&json_str) {
                return Some(parsed);
            }
        }
    }

    for marker in script_ids {
        if let Some(parsed) = extract_json_after_marker(html_str, marker) {
            return Some(parsed);
        }
    }

    // Try regex for window assignments
    let patterns = [
        r#"(?s)window\['SIGI_STATE'\]\s*=\s*(.*?);\s*window"#,
        r#"(?s)window\['SIGI_STATE'\]\s*=\s*(.*?);\s*</script>"#,
        r#"(?s)window\.SIGI_STATE\s*=\s*(.*?);\s*window"#,
        r#"(?s)window\.__UNIVERSAL_DATA_FOR_REHYDRATION__\s*=\s*(.*?);\s*window"#,
        r#"(?s)window\.__UNIVERSAL_DATA_FOR_REHYDRATION__\s*=\s*(.*?);\s*</script>"#,
    ];

    for pattern in patterns {
        if let Ok(regex) = Regex::new(pattern) {
             if let Some(cap) = regex.captures(html_str) {
                 if let Some(json_str) = cap.get(1) {
                     if let Some(parsed) = parse_first_json_value(json_str.as_str()) {
                         return Some(parsed);
                     }
                 }
             }
        }
    }

    None
}

fn decode_json_string(raw: &str) -> Option<String> {
    serde_json::from_str::<String>(&format!("\"{raw}\""))
        .ok()
        .or_else(|| {
            let decoded = raw
                .replace("\\u002F", "/")
                .replace("\\/", "/")
                .replace("\\u0026", "&")
                .replace("\\u003D", "=");
            if decoded == raw {
                None
            } else {
                Some(decoded)
            }
        })
}

fn extract_m3u8_from_html(html_str: &str) -> Option<String> {
    let regex = Regex::new(r#"(https?:\\?/\\?/[^"'\\s]+\\.m3u8[^"'\\s]*)"#).ok()?;
    let raw = regex.captures(html_str)?.get(1)?.as_str();
    let decoded = decode_json_string(raw).unwrap_or_else(|| raw.to_string());
    if decoded.contains(".m3u8") {
        Some(decoded)
    } else {
        None
    }
}

fn gen_device_id() -> String {
    let mut rng = rand::rng();
    (0..19)
        .map(|_| char::from(b'0' + rng.random_range(0..10)))
        .collect()
}

fn build_qr_params(
    token: Option<&str>,
    device_id: &str,
    verify_fp: &str,
    ms_token: &str,
) -> Vec<(String, String)> {
    let mut params = vec![
        ("next".to_string(), "https://www.tiktok.com".to_string()),
        ("multi_login".to_string(), "1".to_string()),
        ("did".to_string(), device_id.to_string()),
        ("locale".to_string(), "zh-Hans".to_string()),
        ("app_language".to_string(), "zh".to_string()),
        ("aid".to_string(), "1459".to_string()),
        ("account_sdk_source".to_string(), "web".to_string()),
        ("sdk_version".to_string(), "2.1.11-tiktokbeta.3".to_string()),
        ("language".to_string(), "zh-Hant".to_string()),
        ("verifyFp".to_string(), verify_fp.to_string()),
        ("target_aid".to_string(), "".to_string()),
        ("standalone_aid".to_string(), "".to_string()),
        ("msToken".to_string(), ms_token.to_string()),
    ];
    if let Some(token) = token {
        params.push(("token".to_string(), token.to_string()));
    }

    let shark_extra = serde_json::json!({
        "aid": 1459,
        "app_name": "Tik_Tok_Login",
        "channel": "tiktok_web",
        "device_platform": "web_pc",
        "device_id": device_id,
        "region": "TW",
        "priority_region": "",
        "os": "windows",
        "referer": "https://www.tiktok.com/",
        "cookie_enabled": true,
        "screen_width": 2560,
        "screen_height": 1440,
        "browser_language": "zh-CN",
        "browser_platform": "Win32",
        "browser_name": "Mozilla",
        "browser_version": "5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36",
        "browser_online": true,
        "verifyFp": verify_fp,
        "app_language": "zh-Hans",
        "webcast_language": "zh-Hans",
        "tz_name": "Asia/Shanghai",
        "is_page_visible": true,
        "focus_state": true,
        "is_fullscreen": false,
        "history_len": 2,
        "user_is_login": false,
        "data_collection_enabled": true
    });
    params.push(("shark_extra".to_string(), shark_extra.to_string()));

    params
}

fn build_query(params: &[(String, String)]) -> String {
    let mut serializer = Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn collect_cookie_map(headers: &reqwest::header::HeaderMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for value in headers.get_all(reqwest::header::SET_COOKIE).iter() {
        if let Ok(raw) = value.to_str() {
            if let Some((pair, _)) = raw.split_once(';') {
                if let Some((name, val)) = pair.split_once('=') {
                    map.insert(name.trim().to_string(), val.trim().to_string());
                }
            }
        }
    }
    map
}

fn merge_cookie_maps(target: &mut HashMap<String, String>, extra: HashMap<String, String>) {
    for (key, value) in extra {
        target.insert(key, value);
    }
}

fn build_cookie_string(map: &HashMap<String, String>) -> String {
    let mut pairs: Vec<String> = map
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    pairs.sort();
    pairs.join("; ")
}

fn has_login_cookie(map: &HashMap<String, String>) -> bool {
    map.contains_key("sessionid")
        || map.contains_key("sessionid_ss")
        || map.contains_key("sid_tt")
        || map.contains_key("sid_guard")
}

fn parse_cookie_header(header: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in header.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((key, value)) = pair.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    map
}

fn mssdk_url() -> String {
    env::var("TIKTOK_MSSDK_URL").unwrap_or_else(|_| TIKTOK_MSSDK_URL.to_string())
}

fn apply_tiktok_extra_headers(headers: &mut HeaderMap) {
    let mappings = [
        ("TIKTOK_X_MSSDK_INFO", "x-mssdk-info"),
        ("TIKTOK_TT_TICKET_GUARD_CLIENT_DATA", "tt-ticket-guard-client-data"),
        ("TIKTOK_TT_TICKET_GUARD_ITERATION_VERSION", "tt-ticket-guard-iteration-version"),
        ("TIKTOK_TT_TICKET_GUARD_PUBLIC_KEY", "tt-ticket-guard-public-key"),
        ("TIKTOK_TT_TICKET_GUARD_VERSION", "tt-ticket-guard-version"),
        ("TIKTOK_TT_TICKET_GUARD_WEB_VERSION", "tt-ticket-guard-web-version"),
    ];
    for (env_key, header_name) in mappings {
        if let Ok(value) = env::var(env_key) {
            let value = value.trim();
            if !value.is_empty() {
                if let Ok(parsed) = value.parse() {
                    headers.insert(header_name, parsed);
                }
            }
        }
    }
}

async fn fetch_ms_token(client: &Client) -> String {
    let payload = serde_json::json!({
        "magic": TIKTOK_MSSDK_MAGIC,
        "version": TIKTOK_MSSDK_VERSION,
        "dataType": TIKTOK_MSSDK_DATA_TYPE,
        "strData": TIKTOK_MSSDK_STR_DATA,
        "tspFromClient": chrono::Utc::now().timestamp_millis(),
    });
    let resp = client
        .post(mssdk_url())
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await;
    if let Ok(resp) = resp {
        if let Some(token) = collect_cookie_map(resp.headers()).get("msToken").cloned() {
            return token;
        }
    }
    crate::platforms::douyin::params::gen_false_ms_token()
}

async fn fetch_ttwid(client: &Client, cookie_header: &str) -> Option<String> {
    let resp = client
        .post(TIKTOK_TTWID_CHECK_URL)
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "text/plain")
        .header("Cookie", cookie_header)
        .body(TIKTOK_TTWID_CHECK_DATA)
        .send()
        .await
        .ok()?;
    collect_cookie_map(resp.headers()).get("ttwid").cloned()
}

async fn bootstrap_tiktok_cookies(
    client: &Client,
    verify_fp: &str,
    ms_token_hint: Option<&str>,
) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    if let Ok(resp) = client
        .get("https://www.tiktok.com/")
        .header("User-Agent", USER_AGENT)
        .header("Referer", "https://www.tiktok.com/")
        .send()
        .await
    {
        merge_cookie_maps(&mut cookies, collect_cookie_map(resp.headers()));
    }

    if !verify_fp.is_empty() {
        cookies.insert("s_v_web_id".to_string(), verify_fp.to_string());
    }

    let ms_token = if let Some(ms_token) = ms_token_hint {
        ms_token.to_string()
    } else if let Some(ms_token) = cookies.get("msToken").cloned() {
        ms_token
    } else {
        fetch_ms_token(client).await
    };
    cookies.insert("msToken".to_string(), ms_token);

    if !cookies.contains_key("ttwid") {
        let cookie_header = build_cookie_string(&cookies);
        if let Some(ttwid) = fetch_ttwid(client, &cookie_header).await {
            cookies.insert("ttwid".to_string(), ttwid);
        }
    }

    cookies
}

fn build_passport_headers(cookies: &HashMap<String, String>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", USER_AGENT.parse().unwrap());
    headers.insert("referer", "https://www.tiktok.com/".parse().unwrap());
    let cookie_header = build_cookie_string(cookies);
    if !cookie_header.is_empty() {
        headers.insert("cookie", cookie_header.parse().unwrap());
    }
    if let Some(csrf) = cookies.get("passport_csrf_token") {
        headers.insert("x-tt-passport-csrf-token", csrf.parse().unwrap());
    }
    headers
}

fn build_auth_broadcast_query(device_id: &str, verify_fp: &str, ms_token: &str) -> String {
    let now = chrono::Utc::now().timestamp();
    let params = vec![
        ("WebIdLastTime".to_string(), now.to_string()),
        ("aid".to_string(), "1988".to_string()),
        ("app_language".to_string(), "zh-Hans".to_string()),
        ("app_name".to_string(), "tiktok_web".to_string()),
        ("browser_language".to_string(), "zh-CN".to_string()),
        ("browser_name".to_string(), "Mozilla".to_string()),
        ("browser_online".to_string(), "true".to_string()),
        ("browser_platform".to_string(), "Win32".to_string()),
        ("browser_version".to_string(), USER_AGENT.to_string()),
        ("channel".to_string(), "tiktok_web".to_string()),
        ("cookie_enabled".to_string(), "true".to_string()),
        ("data_collection_enabled".to_string(), "true".to_string()),
        ("device_id".to_string(), device_id.to_string()),
        ("device_platform".to_string(), "web_pc".to_string()),
        ("focus_state".to_string(), "false".to_string()),
        ("history_len".to_string(), "6".to_string()),
        ("is_fullscreen".to_string(), "false".to_string()),
        ("is_page_visible".to_string(), "true".to_string()),
        ("os".to_string(), "windows".to_string()),
        ("region".to_string(), "TW".to_string()),
        ("screen_height".to_string(), "1440".to_string()),
        ("screen_width".to_string(), "2560".to_string()),
        ("tz_name".to_string(), "Asia/Shanghai".to_string()),
        ("user_is_login".to_string(), "false".to_string()),
        ("verifyFp".to_string(), verify_fp.to_string()),
        ("webcast_language".to_string(), "zh-Hans".to_string()),
        ("msToken".to_string(), ms_token.to_string()),
    ];
    build_query(&params)
}

async fn fetch_auth_broadcast_cookies(
    client: &Client,
    headers: &HeaderMap,
    base_cookies: &HashMap<String, String>,
    device_id: &str,
    verify_fp: &str,
    ms_token: &str,
) -> Result<HashMap<String, String>, RecorderError> {
    let query = build_auth_broadcast_query(device_id, verify_fp, ms_token);
    let hosts = [
        "https://web-va.tiktok.com",
        "https://login-no1a.www.tiktok.com",
        "https://us.tiktok.com",
    ];
    let mut merged = base_cookies.clone();
    for host in hosts {
        let url = format!("{host}/passport/web/auth_broadcast/?{query}");
        let resp = client.post(url).headers(headers.clone()).send().await?;
        let extra = collect_cookie_map(resp.headers());
        if !extra.is_empty() {
            log::info!(
                "[TikTok] auth_broadcast cookies from {}: {}",
                host,
                extra.len()
            );
        } else {
            log::warn!("[TikTok] auth_broadcast no cookies from {}", host);
        }
        merge_cookie_maps(&mut merged, extra);
    }
    Ok(merged)
}

fn extract_next_url(target: &str) -> Option<String> {
    let parsed = Url::parse(target).ok()?;
    for (key, value) in parsed.query_pairs() {
        if key == "next_url" {
            return Some(value.to_string());
        }
    }
    None
}

async fn fetch_cookies_with_redirects(
    headers: &HeaderMap,
    start_url: &str,
) -> Result<HashMap<String, String>, RecorderError> {
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .map_err(|_| RecorderError::ApiError {
            error: "Failed to build TikTok login client".to_string(),
        })?;
    let mut current = start_url.to_string();
    let mut collected = HashMap::new();
    if let Some(cookie_header) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
        merge_cookie_maps(&mut collected, parse_cookie_header(cookie_header));
    }

    for _ in 0..6 {
        let mut request_headers = headers.clone();
        let cookie_string = build_cookie_string(&collected);
        if !cookie_string.is_empty() {
            request_headers.insert("cookie", cookie_string.parse().unwrap());
        }
        let resp = client.get(&current).headers(request_headers).send().await?;
        merge_cookie_maps(&mut collected, collect_cookie_map(resp.headers()));

        if resp.status().is_redirection() {
            if let Some(location) = resp.headers().get(LOCATION) {
                let location = location.to_str().unwrap_or_default();
                if location.is_empty() {
                    break;
                }
                if let Ok(next_url) = Url::parse(&current).and_then(|base| base.join(location))
                {
                    current = next_url.to_string();
                    continue;
                }
                if let Ok(next_url) = Url::parse(location) {
                    current = next_url.to_string();
                    continue;
                }
            }
        }
        break;
    }

    Ok(collected)
}

async fn follow_login_chain(
    headers: &HeaderMap,
    target: &str,
) -> Result<String, RecorderError> {
    let mut cookie_map = fetch_cookies_with_redirects(headers, target).await?;
    if let Some(next_url) = extract_next_url(target) {
        let next_map = fetch_cookies_with_redirects(headers, &next_url).await?;
        merge_cookie_maps(&mut cookie_map, next_map);
    }
    Ok(build_cookie_string(&cookie_map))
}

pub async fn get_qr_login(client: &Client) -> Result<TikTokQrInfo, RecorderError> {
    let device_id = gen_device_id();
    let verify_fp = crate::platforms::douyin::params::gen_verify_fp();
    let mut cookies = bootstrap_tiktok_cookies(client, &verify_fp, None).await;
    let mut ms_token = cookies
        .get("msToken")
        .cloned()
        .unwrap_or_else(crate::platforms::douyin::params::gen_false_ms_token);

    let params = build_qr_params(None, &device_id, &verify_fp, &ms_token);
    let query = build_query(&params);
    let url = format!(
        "https://www.tiktok.com/passport/web/get_qrcode/?{query}"
    );

    let headers = build_passport_headers(&cookies);
    let resp = client.get(url).headers(headers.clone()).send().await?;
    if let Some(ms_header) = resp.headers().get("x-ms-token") {
        if let Ok(value) = ms_header.to_str() {
            ms_token = value.to_string();
            cookies.insert("msToken".to_string(), ms_token.clone());
        }
    }
    let json: serde_json::Value = resp.json().await?;
    log::info!("TikTok get_qrcode response: {}", json);
    let data = json.get("data").ok_or_else(|| RecorderError::ApiError {
        error: "TikTok QR: missing data".to_string(),
    })?;
    let token = data
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let qrcode_index_url = data
        .get("qrcode_index_url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let ttwid_ticket = data
        .get("ttwid_migration_ticket")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let qrcode = data
        .get("qrcode")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let (url, image) = if !qrcode.is_empty() {
        (qrcode_index_url, Some(qrcode))
    } else if !qrcode_index_url.is_empty() {
        (String::new(), Some(qrcode_index_url))
    } else {
        (String::new(), None)
    };

    let mut oauth_key = format!("{token}|{device_id}|{verify_fp}|{ms_token}");
    if !ttwid_ticket.is_empty() {
        oauth_key.push('|');
        oauth_key.push_str(&ttwid_ticket);
    }

    Ok(TikTokQrInfo {
        oauth_key,
        url,
        image,
    })
}

pub async fn get_qr_login_status(
    client: &Client,
    token_key: &str,
) -> Result<TikTokQrStatus, RecorderError> {
    if !tiktok_api_allowed() {
        return Ok(TikTokQrStatus {
            code: 1,
            cookies: String::new(),
            message: "访问太频繁，请稍后再试".to_string(),
        });
    }
    let mut parts = token_key.split('|');
    let token = parts.next().unwrap_or_default();
    let device_id = parts.next().unwrap_or_default();
    let verify_fp = parts.next().unwrap_or_default();
    let ms_token = parts.next().unwrap_or_default();
    let ttwid_ticket = parts.next();

    let cookies = bootstrap_tiktok_cookies(client, verify_fp, Some(ms_token)).await;
    let params = build_qr_params(Some(token), device_id, verify_fp, ms_token);
    let query = build_query(&params);
    let mut headers = build_passport_headers(&cookies);
    if let Some(ticket) = ttwid_ticket {
        if !ticket.is_empty() {
            if let Ok(value) = ticket.parse() {
                headers.insert("x-tt-passport-ttwid-ticket", value);
            }
        }
    }
    apply_tiktok_extra_headers(&mut headers);
    let hosts = [
        "https://login-no1a.www.tiktok.com",
        "https://web-va.tiktok.com",
    ];
    let mut response_cookies = cookies.clone();
    let mut json: Option<serde_json::Value> = None;
    for host in hosts {
        let url = format!("{host}/passport/web/check_qrconnect/?{query}");
        let resp = client.get(url).headers(headers.clone()).send().await?;
        merge_cookie_maps(&mut response_cookies, collect_cookie_map(resp.headers()));
        let current: serde_json::Value = resp.json().await?;
        let is_rate_limited = current.get("message").and_then(Value::as_str) == Some("error")
            && current
                .get("data")
                .and_then(|data| data.get("error_code"))
                .and_then(Value::as_i64)
                == Some(7);
        if is_rate_limited {
            log::warn!("[TikTok] QR rate limited on {}", host);
            json = Some(current);
            continue;
        }
        json = Some(current);
        break;
    }
    let json = json.unwrap_or_else(|| serde_json::json!({}));
    if let (Some(status_code), Some(sub_status_code)) = (
        json.get("status_code").and_then(Value::as_i64),
        json.get("sub_status_code").and_then(Value::as_i64),
    ) {
        log::info!(
            "[TikTok] QR status_code: {}, sub_status_code: {}",
            status_code,
            sub_status_code
        );
    }

    if json.get("message").and_then(Value::as_str) == Some("error") {
        let description = json
            .get("data")
            .and_then(|data| data.get("description"))
            .and_then(value_to_string)
            .unwrap_or_else(|| "TikTok QR error".to_string());
        let is_rate_limited = json
            .get("data")
            .and_then(|data| data.get("error_code"))
            .and_then(Value::as_i64)
            == Some(7)
            || description.contains("\u{8bbf}\u{95ee}\u{592a}\u{9891}\u{7e41}")
            || description.contains("\u{8bf7}\u{7a0d}\u{540e}\u{518d}\u{8bd5}")
            || description.to_ascii_lowercase().contains("rate");
        if is_rate_limited {
            set_tiktok_cooldown("rate limited");
        }
        return Ok(TikTokQrStatus {
            code: if is_rate_limited { 1 } else { 3 },
            cookies: String::new(),
            message: description,
        });
    }

    let status_value = match json.get("data") {
        Some(Value::Array(items)) => items.first().and_then(|item| {
            let status = item
                .get("status")
                .and_then(value_to_string);
            let target = item
                .get("target")
                .and_then(value_to_string);
            status.map(|s| (s, target))
        }),
        Some(Value::Object(obj)) => {
            let status = obj.get("status").and_then(value_to_string);
            let target = obj.get("target").and_then(value_to_string);
            status.map(|s| (s, target))
        }
        _ => None,
    };

    let authnext = json
        .get("data")
        .and_then(|data| data.get("authnext").or_else(|| data.get("auth_next")))
        .and_then(value_to_string);
    if let Some(authnext_url) = authnext {
        log::info!("[TikTok] QR authnext received");
        match follow_login_chain(&headers, &authnext_url).await {
            Ok(cookies) if !cookies.is_empty() => {
                return Ok(TikTokQrStatus {
                    code: 0,
                    cookies,
                    message: "ok".to_string(),
                });
            }
            Ok(_) => {
                log::warn!("[TikTok] QR login cookie empty after authnext");
            }
            Err(err) => {
                log::warn!("[TikTok] QR authnext chain failed: {}", err);
            }
        }
    }

    let redirect_url = json
        .get("data")
        .and_then(|data| data.get("redirect_url"))
        .and_then(value_to_string);
    if let Some(redirect_url) = redirect_url {
        log::info!("[TikTok] QR redirect_url received");
        match follow_login_chain(&headers, &redirect_url).await {
            Ok(cookies) if !cookies.is_empty() => {
                return Ok(TikTokQrStatus {
                    code: 0,
                    cookies,
                    message: "ok".to_string(),
                });
            }
            Ok(_) => {
                log::warn!("[TikTok] QR login cookie empty after redirect_url");
            }
            Err(err) => {
                log::warn!("[TikTok] QR redirect_url chain failed: {}", err);
            }
        }
    }

    if json.get("status_code").and_then(Value::as_i64) == Some(0)
        && json.get("sub_status_code").and_then(Value::as_i64) == Some(2001)
    {
        if has_login_cookie(&response_cookies) {
            return Ok(TikTokQrStatus {
                code: 0,
                cookies: build_cookie_string(&response_cookies),
                message: "ok".to_string(),
            });
        }
        log::info!("[TikTok] QR check pass, try auth_broadcast");
        match fetch_auth_broadcast_cookies(
            client,
            &headers,
            &response_cookies,
            device_id,
            verify_fp,
            ms_token,
        )
        .await
        {
            Ok(cookies) if has_login_cookie(&cookies) => {
                return Ok(TikTokQrStatus {
                    code: 0,
                    cookies: build_cookie_string(&cookies),
                    message: "ok".to_string(),
                });
            }
            Ok(cookies) => {
                log::warn!("[TikTok] auth_broadcast returned empty cookies");
                if !cookies.is_empty() {
                    log::warn!(
                        "[TikTok] auth_broadcast cookies missing sessionid ({} total)",
                        cookies.len()
                    );
                }
            }
            Err(err) => {
                log::warn!("[TikTok] auth_broadcast failed: {}", err);
            }
        }
        return Ok(TikTokQrStatus {
            code: 2,
            cookies: String::new(),
            message: "waiting_confirm".to_string(),
        });
    }

    if let Some((status, target)) = status_value {
        log::info!("[TikTok] QR status: {}", status);
        if let Some(target_url) = target {
            match follow_login_chain(&headers, &target_url).await {
                Ok(cookies) if !cookies.is_empty() => {
                    return Ok(TikTokQrStatus {
                        code: 0,
                        cookies,
                        message: "ok".to_string(),
                    });
                }
                Ok(_) => {
                    log::warn!("[TikTok] QR login cookie empty after status {}", status);
                }
                Err(err) => {
                    log::warn!("[TikTok] QR login chain failed: {}", err);
                }
            }
        }
        if status == "success" || status == "confirm" || status == "scan" || status == "scanned" {
            return Ok(TikTokQrStatus {
                code: 2,
                cookies: String::new(),
                message: "waiting_confirm".to_string(),
            });
        }
        if status == "new" {
            return Ok(TikTokQrStatus {
                code: 1,
                cookies: String::new(),
                message: status,
            });
        }
        if status == "expired" || status == "cancel" || status == "failed" {
            return Ok(TikTokQrStatus {
                code: 3,
                cookies: String::new(),
                message: status,
            });
        }
        return Ok(TikTokQrStatus {
            code: 1,
            cookies: String::new(),
            message: status,
        });
    }

    log::warn!("TikTok QR status: unexpected response: {}", json);
    Ok(TikTokQrStatus {
        code: 1,
        cookies: String::new(),
        message: "unknown".to_string(),
    })
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn find_m3u8_in_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => {
            let decoded = decode_json_string(value).unwrap_or_else(|| value.to_string());
            if decoded.contains(".m3u8") && decoded.starts_with("http") {
                Some(decoded)
            } else {
                None
            }
        }
        Value::Object(map) => {
            for child in map.values() {
                if let Some(url) = find_m3u8_in_value(child) {
                    return Some(url);
                }
            }
            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(url) = find_m3u8_in_value(value) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_username_from_url(url: &str) -> String {
    let url_no_query = url.split('?').next().unwrap_or(url);
    let segments = url_no_query.split('/').filter(|part| !part.is_empty());
    for segment in segments {
        if let Some(stripped) = segment.strip_prefix('@') {
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }
    }
    String::new()
}

fn get_string_field(map: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = map.get(*key) {
            match value {
                Value::String(value) if !value.is_empty() => return Some(value.clone()),
                Value::Number(value) => return Some(value.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn get_i64_field(map: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    for key in keys {
        if let Some(value) = map.get(*key) {
            match value {
                Value::Number(value) => return value.as_i64(),
                Value::String(value) => {
                    if let Ok(parsed) = value.parse::<i64>() {
                        return Some(parsed);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn parse_stream_data_value(raw: &str) -> Option<Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .or_else(|| decode_json_string(raw).and_then(|decoded| serde_json::from_str(&decoded).ok()))
}

fn append_codec(url: &str, codec: &str) -> String {
    if codec.is_empty() || url.contains("codec=") {
        return url.to_string();
    }
    let separator = if url.contains('?') { "&" } else { "?" };
    format!("{url}{separator}codec={codec}")
}

#[derive(Clone, Debug)]
struct StreamCandidate {
    hls_url: Option<String>,
    flv_url: Option<String>,
    bitrate: i64,
    width: i64,
    height: i64,
}

fn extract_stream_candidates(live_room_info: &Value) -> Vec<StreamCandidate> {
    let stream_data_raw = match live_room_info
        .get("liveRoom")
        .and_then(|value| value.get("streamData"))
        .and_then(|value| value.get("pull_data"))
        .and_then(|value| value.get("stream_data"))
        .and_then(|value| value.as_str())
    {
        Some(value) => value,
        None => return Vec::new(),
    };

    let stream_data_value = match parse_stream_data_value(stream_data_raw) {
        Some(value) => value,
        None => return Vec::new(),
    };
    let data = match stream_data_value.get("data").and_then(|value| value.as_object()) {
        Some(value) => value,
        None => return Vec::new(),
    };

    let mut candidates = Vec::new();

    for entry in data.values() {
        let main = entry.get("main").and_then(|value| value.as_object());
        let Some(main) = main else { continue };
        let sdk_params_raw = main
            .get("sdk_params")
            .and_then(|value| value.as_str())
            .unwrap_or("{}");
        let sdk_params = serde_json::from_str::<Value>(sdk_params_raw).unwrap_or(Value::Null);
        let bitrate = sdk_params
            .get("vbitrate")
            .and_then(|value| value.as_i64())
            .or_else(|| {
                sdk_params
                    .get("vbitrate")
                    .and_then(|value| value.as_str())
                    .and_then(|value| value.parse::<i64>().ok())
            })
            .unwrap_or(0);
        let resolution = sdk_params
            .get("resolution")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let (width, height) = resolution
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<i64>().ok()?, h.parse::<i64>().ok()?)))
            .unwrap_or((0, 0));
        let vcodec = sdk_params
            .get("VCodec")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        let hls_url = main
            .get("hls")
            .and_then(|value| value.as_str())
            .map(|url| append_codec(url, vcodec));
        let flv_url = main
            .get("flv")
            .and_then(|value| value.as_str())
            .map(|url| append_codec(url, vcodec));

        if hls_url.is_none() && flv_url.is_none() {
            continue;
        }

        candidates.push(StreamCandidate {
            hls_url,
            flv_url,
            bitrate,
            width,
            height,
        });
    }

    candidates.sort_by(|a, b| {
        b.bitrate
            .cmp(&a.bitrate)
            .then_with(|| b.width.cmp(&a.width))
            .then_with(|| b.height.cmp(&a.height))
    });

    candidates
}

fn extract_stream_from_live_room(live_room_info: &Value) -> Option<StreamInfo> {
    let candidates = extract_stream_candidates(live_room_info);
    let best = candidates.first()?;
    Some(StreamInfo {
        hls_url: best.hls_url.clone(),
        rtmp_url: best.flv_url.clone(),
    })
}

async fn check_url_accessible(client: &Client, headers: &HeaderMap, url: &str) -> bool {
    if url.contains(".m3u8") {
        return check_hls_stream_accessible(client, headers, url).await;
    }
    let mut request = client.get(url).headers(headers.clone());
    request = request.header("Range", "bytes=0-1");
    match request.send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

fn extract_first_media_uri(m3u8_text: &str) -> Option<String> {
    for line in m3u8_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return Some(trimmed.to_string());
    }
    None
}

fn resolve_uri(base: &str, uri: &str) -> Option<String> {
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return Some(uri.to_string());
    }
    let base_url = Url::parse(base).ok()?;
    base_url.join(uri).ok().map(|url| url.to_string())
}

async fn check_hls_stream_accessible(
    client: &Client,
    headers: &HeaderMap,
    m3u8_url: &str,
) -> bool {
    let response = match client.get(m3u8_url).headers(headers.clone()).send().await {
        Ok(resp) => resp,
        Err(_) => return false,
    };
    if !response.status().is_success() {
        return false;
    }
    let text = match response.text().await {
        Ok(text) => text,
        Err(_) => return false,
    };

    let first_uri = match extract_first_media_uri(&text) {
        Some(uri) => uri,
        None => return false,
    };

    let resolved = match resolve_uri(m3u8_url, &first_uri) {
        Some(url) => url,
        None => return false,
    };

    if resolved.contains(".m3u8") {
        let response = match client.get(&resolved).headers(headers.clone()).send().await {
            Ok(resp) => resp,
            Err(_) => return false,
        };
        if !response.status().is_success() {
            return false;
        }
        let text = match response.text().await {
            Ok(text) => text,
            Err(_) => return false,
        };
        let first_segment = match extract_first_media_uri(&text) {
            Some(uri) => uri,
            None => return false,
        };
        let segment_url = match resolve_uri(&resolved, &first_segment) {
            Some(url) => url,
            None => return false,
        };
        let response = match client
            .get(&segment_url)
            .headers(headers.clone())
            .header("Range", "bytes=0-1")
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(_) => return false,
        };
        return response.status().is_success();
    }

    let response = match client
        .get(&resolved)
        .headers(headers.clone())
        .header("Range", "bytes=0-1")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(_) => return false,
    };
    response.status().is_success()
}

async fn select_accessible_stream(
    client: &Client,
    headers: &HeaderMap,
    candidates: &[StreamCandidate],
) -> Option<StreamInfo> {
    for candidate in candidates {
        if let Some(hls_url) = candidate.hls_url.as_deref() {
            if check_url_accessible(client, headers, hls_url).await {
                return Some(StreamInfo {
                    hls_url: candidate.hls_url.clone(),
                    rtmp_url: candidate.flv_url.clone(),
                });
            }
        }

        if let Some(flv_url) = candidate.flv_url.as_deref() {
            if check_url_accessible(client, headers, flv_url).await {
                return Some(StreamInfo {
                    hls_url: None,
                    rtmp_url: Some(flv_url.to_string()),
                });
            }
        }
    }

    candidates.first().map(|candidate| StreamInfo {
        hls_url: candidate.hls_url.clone(),
        rtmp_url: candidate.flv_url.clone(),
    })
}

async fn verify_live_stream(
    client: &Client,
    headers: &HeaderMap,
    room_stream: Option<&SigiStreamUrl>,
    sigi_value: &Value,
    html_str: &str,
) -> bool {
    let mut candidates: Vec<String> = Vec::new();

    if let Some(stream) = room_stream {
        if let Some(url) = stream.hls_pull_url.as_ref() {
            candidates.push(url.clone());
        }
        if let Some(url) = stream.rtmp_pull_url.as_ref() {
            candidates.push(url.clone());
        }
    }

    if candidates.is_empty() {
        if let Some(stream) = find_stream_url(sigi_value) {
            if let Some(url) = stream.hls_pull_url.or(stream.rtmp_pull_url) {
                candidates.push(url);
            }
        }
    }

    if candidates.is_empty() {
        if let Some(live_room_info) = extract_live_room_user_info(sigi_value) {
            if let Some(stream_info) = extract_stream_from_live_room(&live_room_info) {
                if let Some(url) = stream_info.hls_url.or(stream_info.rtmp_url) {
                    candidates.push(url);
                }
            }
        }
    }

    if candidates.is_empty() {
        if let Some(url) = find_m3u8_in_value(sigi_value)
            .or_else(|| extract_m3u8_from_html(html_str))
        {
            candidates.push(url);
        }
    }

    let mut seen = HashSet::new();
    for url in candidates.into_iter().filter(|u| seen.insert(u.clone())) {
        if check_url_accessible(client, headers, &url).await {
            return true;
        }
    }

    false
}

fn extract_live_room_user_info(value: &Value) -> Option<Value> {
    match value {
        Value::Object(map) => {
            if let Some(live_room) = map.get("LiveRoom") {
                if let Some(info) = live_room.get("liveRoomUserInfo") {
                    return Some(info.clone());
                }
            }
            if let Some(info) = map.get("liveRoomUserInfo") {
                return Some(info.clone());
            }

            for child in map.values() {
                if let Some(info) = extract_live_room_user_info(child) {
                    return Some(info);
                }
            }

            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(info) = extract_live_room_user_info(value) {
                    return Some(info);
                }
            }
            None
        }
        Value::String(value) => {
            parse_first_json_value(value).and_then(|parsed| extract_live_room_user_info(&parsed))
        }
        _ => None,
    }
}

fn extract_first_url(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => {
            if let Some(url) = map.get("url").and_then(|v| v.as_str()) {
                Some(url.to_string())
            } else if let Some(list) = map
                .get("url_list")
                .or_else(|| map.get("urlList"))
                .or_else(|| map.get("urls"))
                .and_then(|v| v.as_array())
            {
                list.first().and_then(|v| v.as_str()).map(|s| s.to_string())
            } else {
                None
            }
        }
        Value::Array(list) => list.first().and_then(|v| v.as_str()).map(|s| s.to_string()),
        _ => None
    }
}

fn normalize_image_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("//") {
        format!("https:{}", trimmed)
    } else {
        trimmed.to_string()
    }
}

fn find_cover_url(value: &Value) -> Option<String> {
    const COVER_KEYS: [&str; 10] = [
        "cover",
        "coverUrl",
        "cover_url",
        "coverImage",
        "coverImageUrl",
        "roomCover",
        "roomCoverUrl",
        "liveRoomCover",
        "shareCover",
        "shareImage",
    ];
    match value {
        Value::Object(map) => {
            for key in COVER_KEYS {
                if let Some(value) = map.get(key) {
                    if let Some(url) = extract_first_url(value) {
                        return Some(normalize_image_url(&url));
                    }
                }
            }
            for (key, value) in map {
                if key.to_ascii_lowercase().contains("cover") {
                    if let Some(url) = extract_first_url(value) {
                        return Some(normalize_image_url(&url));
                    }
                }
            }
            for child in map.values() {
                if let Some(url) = find_cover_url(child) {
                    return Some(url);
                }
            }
            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(url) = find_cover_url(value) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

fn find_avatar_url(value: &Value) -> Option<String> {
    const AVATAR_KEYS: [&str; 9] = [
        "avatarThumb",
        "avatar_thumb",
        "avatar",
        "avatarUrl",
        "avatarMedium",
        "avatarLarge",
        "headUrl",
        "head_url",
        "profileImage",
    ];
    match value {
        Value::Object(map) => {
            for key in AVATAR_KEYS {
                if let Some(value) = map.get(key) {
                    if let Some(url) = extract_first_url(value) {
                        return Some(normalize_image_url(&url));
                    }
                }
            }
            for child in map.values() {
                if let Some(url) = find_avatar_url(child) {
                    return Some(url);
                }
            }
            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(url) = find_avatar_url(value) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_room_info_from_live_room(
    live_room_info: &Value,
    account: &Account,
    url: &str,
) -> Option<RoomInfo> {
    let live_room_map = live_room_info.as_object()?;
    let user_map = live_room_map.get("user").and_then(|value| value.as_object());
    let live_room_map = live_room_map.get("liveRoom").and_then(|value| value.as_object());

    let user_name = user_map
        .and_then(|map| get_string_field(map, &["nickname", "nickName", "name", "userName"]))
        .unwrap_or_default();
    let user_id = user_map
        .and_then(|map| get_string_field(map, &["uniqueId", "userId", "id", "uid"]))
        .unwrap_or_default();
    let status = user_map.and_then(|map| get_i64_field(map, &["status", "liveStatus"]));
    let live_room_status =
        live_room_map.and_then(|map| get_i64_field(map, &["status", "liveStatus"]));
    let title = live_room_map
        .and_then(|map| get_string_field(map, &["title", "roomTitle"]))
        .unwrap_or_default();
    
    let user_avatar = user_map
        .and_then(|map| map.get("avatarThumb"))
        .and_then(|v| extract_first_url(v))
        .map(|url| normalize_image_url(&url))
        .unwrap_or_default();
    let room_cover_url = live_room_map
        .and_then(|map| find_cover_url(&Value::Object(map.clone())))
        .unwrap_or_default();

    let status_flag = status.or(live_room_status);
    let live_status = if let Some(flag) = status_flag {
        flag == 2
    } else {
        extract_stream_from_live_room(live_room_info)
            .and_then(|stream| stream.hls_url.or(stream.rtmp_url))
            .is_some()
    };

    let extracted_name = extract_username_from_url(url);
    let final_user_name = if !user_name.is_empty() {
        user_name
    } else if !account.name.is_empty() {
        account.name.clone()
    } else if !extracted_name.is_empty() {
        extracted_name.clone()
    } else {
        "TikTok Live".to_string()
    };
    let final_user_id = if !user_id.is_empty() {
        user_id
    } else if !account.id.is_empty() {
        account.id.clone()
    } else {
        extracted_name
    };

    Some(RoomInfo {
        live_status,
        room_title: if title.is_empty() {
            format!("{}'s live", final_user_name)
        } else {
            title
        },
        room_cover_url: if room_cover_url.is_empty() {
            user_avatar.clone()
        } else {
            room_cover_url
        },
        user_id: final_user_id,
        user_name: final_user_name,
        user_avatar,
    })
}

fn looks_like_room_info(map: &Map<String, Value>) -> bool {
    if !(map.contains_key("status")
        || map.contains_key("liveStatus")
        || map.contains_key("live_status"))
    {
        return false;
    }
    let has_owner = map.contains_key("owner")
        || map.contains_key("ownerInfo")
        || map.contains_key("user")
        || map.contains_key("userInfo")
        || map.contains_key("host")
        || map.contains_key("author");
    let has_stream = map.contains_key("streamUrl")
        || map.contains_key("stream_url")
        || map.contains_key("streamUrlInfo")
        || map.contains_key("stream_url_info");
    let has_title = map.contains_key("title")
        || map.contains_key("roomId")
        || map.contains_key("room_id")
        || map.contains_key("liveRoomId")
        || map.contains_key("live_room_id");
    (has_owner || has_stream) && has_title
}

fn find_room_info(value: &Value) -> Option<SigiRoomInfo> {
    match value {
        Value::Object(map) => {
            if let Some(room_info_value) = map.get("roomInfo") {
                if let Ok(room_info) = serde_json::from_value::<SigiRoomInfo>(
                    room_info_value.clone(),
                ) {
                    return Some(room_info);
                }
            }

            if looks_like_room_info(map) {
                if let Ok(room_info) =
                    serde_json::from_value::<SigiRoomInfo>(Value::Object(map.clone()))
                {
                    return Some(room_info);
                }
            }

            for child in map.values() {
                if let Some(room_info) = find_room_info(child) {
                    return Some(room_info);
                }
            }

            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(room_info) = find_room_info(value) {
                    return Some(room_info);
                }
            }
            None
        }
        Value::String(value) => {
            parse_first_json_value(value).and_then(|parsed| find_room_info(&parsed))
        }
        _ => None,
    }
}

fn looks_like_stream_url(map: &Map<String, Value>) -> bool {
    map.contains_key("hlsPullUrl")
        || map.contains_key("hls_pull_url")
        || map.contains_key("hlsPlayUrl")
        || map.contains_key("rtmpPullUrl")
        || map.contains_key("rtmp_pull_url")
        || map.contains_key("rtmpPlayUrl")
        || map.contains_key("flvPullUrl")
}

fn find_stream_url(value: &Value) -> Option<SigiStreamUrl> {
    match value {
        Value::Object(map) => {
            if looks_like_stream_url(map) {
                if let Ok(stream_url) =
                    serde_json::from_value::<SigiStreamUrl>(Value::Object(map.clone()))
                {
                    return Some(stream_url);
                }
            }

            for child in map.values() {
                if let Some(stream_url) = find_stream_url(child) {
                    return Some(stream_url);
                }
            }

            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(stream_url) = find_stream_url(value) {
                    return Some(stream_url);
                }
            }
            None
        }
        Value::String(value) => {
            parse_first_json_value(value).and_then(|parsed| find_stream_url(&parsed))
        }
        _ => None,
    }
}

/// Get room information from TikTok page
/// Note: TikTok requires proxy to access in most regions
pub async fn get_room_info(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<RoomInfo, RecorderError> {
    if !tiktok_api_allowed() {
        return Err(RecorderError::ApiError {
            error: "TikTok API cooldown".to_string(),
        });
    }
    let proxy_url = proxy_url_from_env();
    let request_client = if let Some(proxy_url) = proxy_url.as_deref() {
        build_proxy_client(proxy_url)?
    } else {
        client.clone()
    };
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert("referer", "https://www.tiktok.com/".parse().unwrap());
    headers.insert("accept-language", "en-US,en;q=0.9".parse().unwrap());

    let cookie = if account.cookies.is_empty() {
        DEFAULT_COOKIE
    } else {
        &account.cookies
    };
    headers.insert("cookie", cookie.parse().unwrap());

    // Retry up to 3 times
    for attempt in 0..3 {
        let response = request_client
            .get(url)
            .headers(headers.clone())
            .send()
            .await?;
        let status = response.status();
        let html_str = response.text().await?;
        if !status.is_success() {
            if status == reqwest::StatusCode::FORBIDDEN
                || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            {
                set_tiktok_cooldown(&format!("response status {}", status));
            }
            return Err(RecorderError::ApiError {
                error: format!("TikTok response status: {}", status),
            });
        }

        // Check for region block
        if html_str.contains("We regret to inform you that we have discontinued operating TikTok") {
            return Err(RecorderError::ApiError {
                error: "TikTok is not available in this region. Please use a different proxy.".to_string(),
            });
        }

        // Check for unexpected EOF
        if html_str.contains("UNEXPECTED_EOF_WHILE_READING") {
            if attempt < 2 {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                continue;
            } else {
                return Err(RecorderError::ApiError {
                    error: "Failed to load page after 3 attempts".to_string(),
                });
            }
        }

        let sigi_value = match extract_state_value(&html_str) {
            Some(value) => value,
            None => {
                if attempt < 2 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
                return Err(RecorderError::ApiError {
                    error: "Please check if your network can access TikTok normally. Failed to extract page state JSON.".to_string(),
                });
            }
        };

        let mut extracted_room_info = serde_json::from_value::<SigiStateResponse>(sigi_value.clone())
            .ok()
            .and_then(|state| state.room_store)
            .and_then(|store| store.room_info);

        if extracted_room_info.is_none() {
            extracted_room_info = find_room_info(&sigi_value);
        }

        if let Some(room_info) = extracted_room_info {
            let mut live_status = match room_info.status {
                Some(status) => status == 2,
                None => room_info
                    .stream_url
                    .as_ref()
                    .map(|stream| stream.hls_pull_url.is_some() || stream.rtmp_pull_url.is_some())
                    .unwrap_or(false),
            };
            if live_status
                && !verify_live_stream(
                    &request_client,
                    &headers,
                    room_info.stream_url.as_ref(),
                    &sigi_value,
                    &html_str,
                )
                .await
            {
                live_status = false;
            }

            let user_id = room_info
                .owner
                .as_ref()
                .and_then(|o| o.id.clone())
                .unwrap_or_default();

            let user_name = room_info
                .owner
                .as_ref()
                .and_then(|o| o.nickname.clone())
                .filter(|n| !n.is_empty())
                .or_else(|| {
                    room_info.owner.as_ref().and_then(|o| o.unique_id.clone())
                })
                .or_else(|| {
                    room_info.owner.as_ref().and_then(|o| o.id.clone())
                })
                .unwrap_or_default();

            let mut user_avatar = room_info
                .owner
                .as_ref()
                .and_then(|o| o.avatar_thumb.as_ref())
                .and_then(|v| extract_first_url(v))
                .map(|url| normalize_image_url(&url))
                .unwrap_or_default();
            if user_avatar.is_empty() {
                if let Some(found) = find_avatar_url(&sigi_value) {
                    user_avatar = found;
                }
            }
            let mut room_cover_url =
                find_cover_url(&sigi_value).unwrap_or_else(|| String::new());
            if room_cover_url.is_empty() {
                room_cover_url = user_avatar.clone();
            }

            return Ok(RoomInfo {
                live_status,
                room_title: room_info.title.unwrap_or_default(),
                room_cover_url,
                user_id,
                user_name,
                user_avatar,
            });
        }

        if let Some(live_room_info) = extract_live_room_user_info(&sigi_value) {
            if let Some(mut room_info) =
                extract_room_info_from_live_room(&live_room_info, account, url)
            {
                if room_info.live_status
                    && !verify_live_stream(
                        &request_client,
                        &headers,
                        None,
                        &sigi_value,
                        &html_str,
                    )
                    .await
                {
                    room_info.live_status = false;
                }
                return Ok(room_info);
            }
        }

        let fallback_stream = find_stream_url(&sigi_value)
            .and_then(|stream| stream.hls_pull_url.or(stream.rtmp_pull_url))
            .or_else(|| find_m3u8_in_value(&sigi_value))
            .or_else(|| extract_m3u8_from_html(&html_str));

        if let Some(fallback_stream) = fallback_stream {
            let user_avatar = find_avatar_url(&sigi_value).unwrap_or_default();
            let extracted_name = extract_username_from_url(url);
            let user_name = if !account.name.is_empty() {
                account.name.clone()
            } else if !extracted_name.is_empty() {
                extracted_name.clone()
            } else {
                "TikTok Live".to_string()
            };
            let user_id = if !account.id.is_empty() {
                account.id.clone()
            } else {
                extracted_name
            };

            let live_status =
                check_url_accessible(&request_client, &headers, &fallback_stream).await;

            return Ok(RoomInfo {
                live_status,
                room_title: format!("{}'s live", user_name),
                room_cover_url: user_avatar.clone(),
                user_id,
                user_name,
                user_avatar,
            });
        }

        return Err(RecorderError::ApiError {
            error: "Failed to extract room info from page data".to_string(),
        });
    }

    Err(RecorderError::ApiError {
        error: "Failed to fetch TikTok page after retries".to_string(),
    })
}

/// Get stream URL from TikTok page
pub async fn get_stream_url(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<StreamInfo, RecorderError> {
    if !tiktok_api_allowed() {
        return Err(RecorderError::ApiError {
            error: "TikTok API cooldown".to_string(),
        });
    }
    let proxy_url = proxy_url_from_env();
    let request_client = if let Some(proxy_url) = proxy_url.as_deref() {
        build_proxy_client(proxy_url)?
    } else {
        client.clone()
    };
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert("referer", "https://www.tiktok.com/".parse().unwrap());
    headers.insert("accept-language", "en-US,en;q=0.9".parse().unwrap());

    let cookie = if account.cookies.is_empty() {
        DEFAULT_COOKIE
    } else {
        &account.cookies
    };
    headers.insert("cookie", cookie.parse().unwrap());

    // Retry up to 3 times
    for attempt in 0..3 {
        let response = request_client
            .get(url)
            .headers(headers.clone())
            .send()
            .await?;
        let status = response.status();
        let html_str = response.text().await?;
        if !status.is_success() {
            if status == reqwest::StatusCode::FORBIDDEN
                || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            {
                set_tiktok_cooldown(&format!("response status {}", status));
            }
            return Err(RecorderError::ApiError {
                error: format!("TikTok response status: {}", status),
            });
        }

        // Check for unexpected EOF
        if html_str.contains("UNEXPECTED_EOF_WHILE_READING") {
            if attempt < 2 {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                continue;
            } else {
                return Err(RecorderError::ApiError {
                    error: "Failed to load page after 3 attempts".to_string(),
                });
            }
        }

        let sigi_value = match extract_state_value(&html_str) {
            Some(value) => value,
            None => {
                if attempt < 2 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
                return Err(RecorderError::ApiError {
                    error: "Failed to extract page state JSON".to_string(),
                });
            }
        };

        if let Some(live_room_info) = extract_live_room_user_info(&sigi_value) {
            let candidates = extract_stream_candidates(&live_room_info);
            if let Some(stream_info) =
                select_accessible_stream(&request_client, &headers, &candidates).await
            {
                return Ok(stream_info);
            }
        }

        let mut stream_url = serde_json::from_value::<SigiStateResponse>(sigi_value.clone())
            .ok()
            .and_then(|state| state.room_store)
            .and_then(|store| store.room_info)
            .and_then(|room_info| room_info.stream_url);

        if stream_url.is_none() {
            stream_url = find_room_info(&sigi_value).and_then(|room_info| room_info.stream_url);
        }

        if stream_url.is_none() {
            stream_url = find_stream_url(&sigi_value);
        }

        if let Some(stream_url) = stream_url {
            let mut info = StreamInfo {
                hls_url: stream_url.hls_pull_url,
                rtmp_url: stream_url.rtmp_pull_url,
            };
            if let Some(hls_url) = info.hls_url.as_deref() {
                if !check_hls_stream_accessible(&request_client, &headers, hls_url).await {
                    info.hls_url = None;
                }
            }
            if info.hls_url.is_none() && info.rtmp_url.is_none() {
                return Err(RecorderError::ApiError {
                    error: "No available stream provided".to_string(),
                });
            }
            return Ok(info);
        }

        if let Some(m3u8_url) = find_m3u8_in_value(&sigi_value)
            .or_else(|| extract_m3u8_from_html(&html_str))
        {
            return Ok(StreamInfo {
                hls_url: Some(m3u8_url),
                rtmp_url: None,
            });
        }

        return Err(RecorderError::ApiError {
            error: "Failed to extract stream URL from page data".to_string(),
        });
    }

    Err(RecorderError::ApiError {
        error: "Failed to fetch TikTok page after retries".to_string(),
    })
}

/// Get user information from TikTok
pub async fn get_user_info(
    client: &Client,
    account: &Account,
) -> Result<crate::UserInfo, RecorderError> {
    let mut headers = HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "Accept-Language",
        "zh-CN,zh;q=0.8,zh-TW;q=0.7,zh-HK;q=0.5,en-US;q=0.3,en;q=0.2"
            .parse()
            .unwrap(),
    );

    if !account.cookies.is_empty() {
        headers.insert("Cookie", account.cookies.parse().unwrap());
    }

    let proxy_url = proxy_url_from_env();
    let request_client = if let Some(proxy_url) = proxy_url.as_deref() {
        build_proxy_client(proxy_url)?
    } else {
        client.clone()
    };

    // Access TikTok homepage to get user info from state
    let response = request_client
        .get("https://www.tiktok.com/")
        .headers(headers.clone())
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(RecorderError::ApiError {
            error: format!("Failed to fetch TikTok, status: {}", response.status()),
        });
    }

    let html_str = response.text().await?;

    // Check for region block
    if html_str.contains("We regret to inform you that we have discontinued operating TikTok") {
        return Err(RecorderError::ApiError {
            error: "TikTok is not available in this region. Please use a different proxy.".to_string(),
        });
    }

    // 1. Try Passport API first (more reliable JSON API)
    if let Ok(info) = get_user_info_from_passport(&request_client, &headers).await {
        return Ok(info);
    }

    let state = extract_state_value(&html_str).ok_or(RecorderError::ApiError {
        error: "Failed to extract TikTok state - please check if your network can access TikTok normally (proxy might be needed).".to_string(),
    })?;

    // Try to find current user in SigiState
    if let Some(user_info) = find_current_user_info(&state) {
        return Ok(user_info);
    }

    // Fallback: extract from specific path if known
    // SIGI_STATE -> UserModule -> users -> [username]
    if let Some(user_module) = state.get("UserModule").and_then(|u| u.get("users")) {
        if let Some(obj) = user_module.as_object() {
            if let Some((_, user_val)) = obj.iter().next() {
                let user_id = get_string_field(user_val.as_object().unwrap_or(&Map::new()), &["id", "secUid"]).unwrap_or_default();
                let user_name = get_string_field(user_val.as_object().unwrap_or(&Map::new()), &["nickname", "uniqueId"]).unwrap_or_default();
                let user_avatar = find_avatar_url(user_val).unwrap_or_default();
                
                if !user_id.is_empty() && !user_name.is_empty() {
                    return Ok(crate::UserInfo {
                        user_id,
                        user_name,
                        user_avatar,
                    });
                }
            }
        }
    }

    Err(RecorderError::ApiError {
        error: "Could not find user info in TikTok page".to_string(),
    })
}

fn find_current_user_info(value: &Value) -> Option<crate::UserInfo> {
    // 1. In SIGI_STATE, the current user is often under "AppContext" or "UserModule"
    if let Some(user) = value.get("AppContext").and_then(|a| a.get("appContext")).and_then(|c| c.get("user")) {
        let user_id = get_string_field(user.as_object().unwrap_or(&Map::new()), &["uid", "id"]).unwrap_or_default();
        let user_name = get_string_field(user.as_object().unwrap_or(&Map::new()), &["nickname", "uniqueId"]).unwrap_or_default();
        let user_avatar = find_avatar_url(user).unwrap_or_default();
        
        if !user_id.is_empty() {
            return Some(crate::UserInfo {
                user_id: user_id.clone(),
                user_name: if user_name.is_empty() { user_id.clone() } else { user_name },
                user_avatar,
            });
        }
    }

    // 2. In __UNIVERSAL_DATA_FOR_REHYDRATION__, it's under webapp.user-info
    if let Some(obj) = value.as_object() {
        for val in obj.values() {
            if let Some(user) = val.get("__DEFAULT_SCOPE__")
                .and_then(|s| s.get("webapp.user-info"))
                .and_then(|u| u.get("data"))
                .and_then(|d| d.get("user")) {
                let user_id = get_string_field(user.as_object().unwrap_or(&Map::new()), &["uid", "id", "secUid"]).unwrap_or_default();
                let user_name = get_string_field(user.as_object().unwrap_or(&Map::new()), &["nickname", "uniqueId"]).unwrap_or_default();
                let user_avatar = find_avatar_url(user).unwrap_or_default();
                
                if !user_id.is_empty() {
                    return Some(crate::UserInfo {
                        user_id: user_id.clone(),
                        user_name: if user_name.is_empty() { user_id.clone() } else { user_name },
                        user_avatar,
                    });
                }
            }
        }
    }

    None
}

/// Get user info from TikTok passport API
async fn get_user_info_from_passport(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
) -> Result<crate::UserInfo, RecorderError> {
    let response = client
        .get("https://www.tiktok.com/passport/web/user/info/")
        .headers(headers.clone())
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(RecorderError::ApiError {
            error: format!("Failed to fetch TikTok passport info, status: {}", response.status()),
        });
    }

    let json: Value = response.json().await?;
    if let Some(data) = json.get("data").and_then(|d| d.as_object()) {
        let user_id = get_string_field(data, &["user_id", "uid", "id"]).unwrap_or_default();
        let user_name = get_string_field(data, &["nickname", "unique_id"]).unwrap_or_default();
        let user_avatar = find_avatar_url(&json).unwrap_or_default();

        if !user_id.is_empty() {
            let final_name = if user_name.is_empty() {
                user_id.clone()
            } else {
                user_name
            };
            return Ok(crate::UserInfo {
                user_id,
                user_name: final_name,
                user_avatar,
            });
        }
    }

    Err(RecorderError::ApiError {
        error: "User info not found in passport response".to_string(),
    })
}

/// Download file from URL to local path
pub async fn download_file(client: &Client, url: &str, path: &std::path::Path) -> Result<(), RecorderError> {
    if url.is_empty() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| RecorderError::IoError(e))?;
        }
    }

    let response = client.get(url).send().await?;
    let bytes = response.bytes().await?;
    let mut file = tokio::fs::File::create(&path).await?;
    let mut content = std::io::Cursor::new(bytes);
    tokio::io::copy(&mut content, &mut file).await?;
    Ok(())
}
