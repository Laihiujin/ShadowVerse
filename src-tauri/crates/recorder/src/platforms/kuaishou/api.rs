use super::response::{LiveStreamResponse, UserFollowCountResponse};
use crate::account::Account;
use crate::errors::RecorderError;
use chrono::Utc;
use rand::Rng;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const WEB_RATE_LIMIT_COOLDOWN_SECS: i64 = 10;
const WEB_RATE_LIMIT_RETRY_SECS: u64 = 10;

static WEB_COOLDOWN_UNTIL: AtomicI64 = AtomicI64::new(0);
 
fn is_rate_limit_message(message: &str) -> bool {
    let trimmed = message.trim();
    !trimmed.is_empty()
        && (trimmed.contains("\u{64cd}\u{4f5c}\u{592a}\u{5feb}")
            || trimmed.contains("\u{8bbf}\u{95ee}\u{8fc7}\u{4e8e}\u{9891}\u{7e41}")
            || trimmed.contains("\u{8bbf}\u{95ee}\u{9891}\u{7e41}")
            || trimmed.contains("\u{8bbf}\u{95ee}\u{592a}\u{5feb}")
            || trimmed.contains("\u{8bf7}\u{7a0d}\u{540e}\u{518d}\u{8bd5}")
            || trimmed.contains("\u{8bf7}\u{7a0d}\u{5019}\u{518d}\u{8bd5}")
            || trimmed.contains("\u{7a0d}\u{5019}\u{518d}\u{8bd5}")
            || trimmed.contains("\u{7a0d}\u{540e}\u{518d}\u{8bd5}")
            || trimmed.contains("\u{8bf7}\u{6c42}\u{8fc7}\u{4e8e}\u{9891}\u{7e41}"))
}

fn is_room_disabled_message(message: &str) -> bool {
    let trimmed = message.trim();
    !trimmed.is_empty() && trimmed.contains("\u{672a}\u{542f}\u{7528}")
}


fn set_web_cooldown(reason: &str) {
    let until = Utc::now().timestamp() + WEB_RATE_LIMIT_COOLDOWN_SECS;
    WEB_COOLDOWN_UNTIL.store(until, Ordering::Relaxed);
    log::info!(
        "[Kuaishou] Web cooldown set ({}s): {}",
        WEB_RATE_LIMIT_COOLDOWN_SECS,
        reason
    );
}


fn web_api_allowed() -> bool {
    let now = Utc::now().timestamp();
    now >= WEB_COOLDOWN_UNTIL.load(Ordering::Relaxed)
}

pub fn is_rate_limited_error(error: &RecorderError) -> bool {
    match error {
        RecorderError::ApiError { error } => is_rate_limit_message(error),
        _ => false,
    }
}

pub fn is_room_disabled_error(error: &RecorderError) -> bool {
    match error {
        RecorderError::ApiError { error } => is_room_disabled_message(error),
        _ => false,
    }
}

fn decode_json_string(raw: &str) -> Option<String> {
    serde_json::from_str::<String>(&format!("\"{raw}\""))
        .ok()
        .or_else(|| {
            let decoded = raw
                .replace("\\u002F", "/")
                .replace("\\u0026", "&")
                .replace("\\u003D", "=");
            if decoded == raw {
                None
            } else {
                Some(decoded)
            }
        })
}

fn extract_rate_limit_message_from_body(body: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        let keys = ["error_msg", "errorMsg", "errMsg", "message", "msg"];
        for key in keys {
            if let Some(msg) = value.get(key).and_then(|v| v.as_str()) {
                if is_rate_limit_message(msg) {
                    return Some(msg.to_string());
                }
            }
        }
    }

    if is_rate_limit_message(body) {
        return Some(body.trim().to_string());
    }

    None
}

fn extract_hls_play_url(html_str: &str) -> Option<String> {
    let regex = Regex::new(r#""hlsPlayUrl"\s*:\s*"([^"]+)""#).ok()?;
    let raw = regex.captures(html_str)?.get(1)?.as_str();
    let decoded = decode_json_string(raw)?;
    if decoded.contains(".m3u8") {
        Some(decoded)
    } else {
        None
    }
}

fn extract_initial_state(html_str: &str) -> Option<String> {
    let patterns = [
        r#"(?s)window\.__INITIAL_STATE__\s*=\s*(\{.*?\});\s*\(function"#,
        r#"(?s)window\.__INITIAL_STATE__\s*=\s*(\{.*?\})\s*;\s*</script>"#,
        r#"(?s)window\['__INITIAL_STATE__'\]\s*=\s*(\{.*?\})\s*;\s*</script>"#,
        r#"(?s)window\.__INITIAL_STATE__\s*=\s*(\{.*?\})\s*;\s*window\.__"#,
        r#"(?s)__INITIAL_STATE__\s*=\s*(\{.*?\})\s*;\s*\(function"#,
        r#"(?s)__INITIAL_STATE__\s*=\s*(\{.*?\})\s*;\s*<"#,
    ];

    for (i, pattern) in patterns.iter().enumerate() {
        if let Ok(regex) = Regex::new(pattern) {
            if let Some(captures) = regex.captures(html_str) {
                if let Some(value) = captures.get(1) {
                    let json_str = value.as_str().trim();
                    log::debug!("[Kuaishou] Extracted __INITIAL_STATE__ using pattern #{}", i);
                    return Some(json_str.to_string());
                }
            }
        }
    }

    log::warn!(
        "[Kuaishou] Failed to extract __INITIAL_STATE__ with any pattern, HTML size: {} bytes",
        html_str.len()
    );
    // Log a snippet of the HTML for debugging (first 500 chars)
    let snippet_len = html_str.len().min(500);
    if snippet_len > 0 {
        log::debug!("[Kuaishou] HTML snippet: {}...", &html_str[..snippet_len]);
    }

    None
}

fn extract_metadata_from_html(html_str: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut title = None;
    let mut cover = None;
    let mut avatar = None;

    // Try finding title
    let title_patterns = [
        r#"<title>(.*?)</title>"#,
        r#""caption"\s*:\s*"([^"]+)""#,
        r#""title"\s*:\s*"([^"]+)""#,
    ];
    for p in title_patterns {
        if let Some(m) = Regex::new(p).ok().and_then(|re| re.captures(html_str)).and_then(|c| c.get(1)) {
            let t = m
                .as_str()
                .replace(" - \u{5feb}\u{624b}\u{76f4}\u{64ad}", "")
                .trim()
                .to_string();
            if !t.is_empty()
                && !t.contains("\u{9519}\u{8bef}\u{4ee3}\u{7801}")
                && !t.contains("Error Code")
            {
                title = Some(t);
                break;
            }
        }
    }

    // Try finding avatar
    let avatar_patterns = [
        r#""headUrl"\s*:\s*"([^"]+)""#,
        r#""avatar"\s*:\s*"([^"]+)""#,
        r#""avatarUrl"\s*:\s*"([^"]+)""#,
    ];
    for p in avatar_patterns {
        if let Some(m) = Regex::new(p).ok().and_then(|re| re.captures(html_str)).and_then(|c| c.get(1)) {
            if let Some(decoded) = decode_json_string(m.as_str()) {
                avatar = Some(normalize_image_url(&decoded));
                break;
            }
        }
    }

    // Try finding cover
    let cover_patterns = [
        r#""poster"\s*:\s*"([^"]+)""#,
        r#""coverUrl"\s*:\s*"([^"]+)""#,
        r#""cover"\s*:\s*"([^"]+)""#,
        r#""snapshot"\s*:\s*"([^"]+)""#,
    ];
    for p in cover_patterns {
        if let Some(m) = Regex::new(p).ok().and_then(|re| re.captures(html_str)).and_then(|c| c.get(1)) {
            if let Some(decoded) = decode_json_string(m.as_str()) {
                cover = Some(normalize_image_url(&decoded));
                break;
            }
        }
    }

    (title, cover, avatar)
}

fn find_live_stream_response(value: &Value) -> Option<LiveStreamResponse> {
    match value {
        Value::Object(map) => {
            if map.contains_key("liveStream") || map.contains_key("live_stream") {
                let mut cloned = map.clone();
                if !cloned.contains_key("liveStream") {
                    if let Some(v) = cloned.remove("live_stream") {
                        cloned.insert("liveStream".to_string(), v);
                    }
                }
                if let Ok(response) = serde_json::from_value::<LiveStreamResponse>(Value::Object(cloned)) {
                    // Check if this looks like a valid response with metadata
                    // Prioritize ones that have author info
                    if (response.live_stream.is_some() && response.author.is_some()) || response.error_type.is_some() {
                        return Some(response);
                    }
                }
            }

            for child in map.values() {
                if let Some(response) = find_live_stream_response(child) {
                    return Some(response);
                }
            }

            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(response) = find_live_stream_response(value) {
                    return Some(response);
                }
            }
            None
        }
        _ => None,
    }
}

fn parse_live_stream_response(json_str: &str) -> Result<LiveStreamResponse, RecorderError> {
    let livestream_regex = Regex::new(r#"(?s)(\{"liveStream".*?),"gameInfo"#).map_err(|e| {
        RecorderError::ApiError {
            error: format!("Failed to create regex: {}", e),
        }
    })?;

    if let Some(cap) = livestream_regex.captures(json_str).and_then(|cap| cap.get(1)) {
        let full_json = format!("{}}}", cap.as_str());
        if let Ok(response) = serde_json::from_str::<LiveStreamResponse>(&full_json) {
            return Ok(response);
        }
    }

    let state: Value = serde_json::from_str(json_str).map_err(|e| RecorderError::ApiError {
        error: format!("Failed to parse JSON: {}", e),
    })?;

    find_live_stream_response(&state).ok_or(RecorderError::ApiError {
        error: "Failed to extract liveStream data".to_string(),
    })
}

fn normalize_cookie_header(cookies: &str) -> String {
    cookies
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

fn get_cookie_value_ci(cookies: &str, key: &str) -> Option<String> {
    let target = key.to_ascii_lowercase();
    for part in cookies.split(';').map(str::trim) {
        if let Some((k, v)) = part.split_once('=') {
            if k.trim().to_ascii_lowercase() == target {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn append_cookie_pair(mut header: String, key: &str, value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return header;
    }
    if !header.is_empty() {
        header.push_str("; ");
    }
    header.push_str(key);
    header.push('=');
    header.push_str(value);
    header
}

fn ensure_kuaishou_request_cookie(cookies: &str) -> String {
    let normalized = normalize_cookie_header(cookies);
    let filtered = filter_kuaishou_cookie_header(&normalized);
    let ensured = if filtered.is_empty() {
        let did = gen_web_did();
        let didv = Utc::now().timestamp_millis();
        format!("did={did}; didv={didv}")
    } else {
        ensure_kuaishou_base_cookies(&filtered)
    };

    ensured
}

fn ensure_kuaishou_base_cookies(cookies: &str) -> String {
    let mut normalized = filter_kuaishou_cookie_header(&normalize_cookie_header(cookies));
    if normalized.is_empty() {
        return normalized;
    }

    let did_existing = get_cookie_value_ci(&normalized, "did");
    if did_existing.is_none() {
        let fallback_did = get_cookie_value_ci(&normalized, "_did");
        let did_value = fallback_did.unwrap_or_else(gen_web_did);
        normalized = append_cookie_pair(normalized, "did", &did_value);
    }

    if get_cookie_value_ci(&normalized, "didv").is_none() {
        let didv = Utc::now().timestamp_millis().to_string();
        normalized = append_cookie_pair(normalized, "didv", &didv);
    }

    normalized
}

fn filter_kuaishou_cookie_header(cookies: &str) -> String {
    let allow = [
        "kwssectoken",
        "kuaishou.live.web_st",
        "kuaishou.live.web_ph",
        "did",
        "didv",
        "userid",
    ];
    let mut kept = Vec::new();
    for part in cookies.split(';').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let key_lower = k.trim().to_ascii_lowercase();
        if allow.iter().any(|item| *item == key_lower) {
            kept.push(format!("{}={}", k.trim(), v.trim()));
        }
    }
    kept.join("; ")
}

fn extract_user_id_from_url(url: &str) -> String {
    let url_no_fragment = url.split('#').next().unwrap_or(url);
    let url_no_query = url_no_fragment.split('?').next().unwrap_or(url_no_fragment);
    let trimmed = url_no_query.trim_end_matches('/');

    if let Some(pos) = trimmed.find("/u/") {
        return trimmed[(pos + 3)..].to_string();
    }
    if let Some(pos) = trimmed.find("/profile/") {
        return trimmed[(pos + 9)..].to_string();
    }

    if trimmed.contains("kuaishou.com") {
        if let Some(last) = trimmed.rsplit('/').next() {
            return last.to_string();
        }
    }

    String::new()
}

fn build_web_candidate_urls(url: &str) -> Vec<String> {
    let mut urls = Vec::new();
    if !url.is_empty() {
        urls.push(url.to_string());
    }

    if url.contains("live.kuaishou.com") {
        let www_url = url.replace("live.kuaishou.com", "www.kuaishou.com");
        if !urls.contains(&www_url) {
            urls.push(www_url);
        }
    } else if url.contains("www.kuaishou.com") {
        let live_url = url.replace("www.kuaishou.com", "live.kuaishou.com");
        if !urls.contains(&live_url) {
            urls.push(live_url);
        }
    }

    let eid = extract_user_id_from_url(url);
    if !eid.is_empty() {
        let candidates = [
            format!("https://live.kuaishou.com/u/{eid}"),
            format!("https://www.kuaishou.com/u/{eid}"),
            format!("https://www.kuaishou.com/live/u/{eid}"),
        ];
        for candidate in candidates {
            if !urls.contains(&candidate) {
                urls.push(candidate);
            }
        }
    }

    urls
}

async fn fetch_web_html(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<(String, reqwest::Url), RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "Accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
            .parse()
            .unwrap(),
    );
    headers.insert(
        "Accept-Language",
        "zh-CN,zh;q=0.8,zh-TW;q=0.7,zh-HK;q=0.5,en-US;q=0.3,en;q=0.2"
            .parse()
            .unwrap(),
    );

    let mut cookie_header = String::new();
    if !account.cookies.is_empty() {
        let cookie = normalize_cookie_header(&account.cookies);
        if !cookie.is_empty() {
            cookie_header = cookie;
            headers.insert("Cookie", cookie_header.parse().unwrap());
        }
    }

    for attempt in 0..2 {
        if !web_api_allowed() {
            log::info!(
                "[Kuaishou] Web rate limited, retrying after {}s (attempt {})",
                WEB_RATE_LIMIT_RETRY_SECS,
                attempt + 1
            );
            tokio::time::sleep(Duration::from_secs(WEB_RATE_LIMIT_RETRY_SECS)).await;
        }

        // Best-effort pre-warm: visit homepage before jumping to room URL.
        {
            let mut pre_headers = headers.clone();
            pre_headers.insert("Referer", "https://live.kuaishou.com/".parse().unwrap());
            match client
                .get("https://live.kuaishou.com/")
                .headers(pre_headers)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    let mut prewarm_map: HashMap<String, String> = HashMap::new();
                    for cookie_header in resp.headers().get_all("set-cookie") {
                        if let Ok(cookie_str) = cookie_header.to_str() {
                            if let Some(pair) = cookie_str.split(';').next() {
                                if let Some((name, value)) = pair.split_once('=') {
                                    let name = name.trim();
                                    let value = value.trim();
                                    if !name.is_empty() {
                                        prewarm_map.insert(name.to_string(), value.to_string());
                                    }
                                }
                            }
                        }
                    }
                    let body = resp.text().await.unwrap_or_default();
                    if !status.is_success() {
                        log::warn!(
                            "[Kuaishou] Homepage prewarm status: {}",
                            status
                        );
                    }
                    if let Some(msg) = extract_rate_limit_message_from_body(&body) {
                        set_web_cooldown(&msg);
                    }
                    if !prewarm_map.is_empty() {
                        let mut merged: HashMap<String, String> = HashMap::new();
                        for part in cookie_header.split(';').map(str::trim) {
                            if let Some((k, v)) = part.split_once('=') {
                                let key = k.trim();
                                if !key.is_empty() {
                                    merged.insert(key.to_string(), v.trim().to_string());
                                }
                            }
                        }
                        for (k, v) in prewarm_map {
                            merged.entry(k).or_insert(v);
                        }
                        let mut pairs: Vec<String> = merged
                            .into_iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect();
                        pairs.sort();
                        cookie_header = pairs.join("; ");
                    }
                }
                Err(err) => {
                    log::debug!("[Kuaishou] Homepage prewarm failed: {}", err);
                }
            }
        }

        let mut last_error: Option<RecorderError> = None;
        let mut last_small: Option<(String, reqwest::Url)> = None;

        for candidate in build_web_candidate_urls(url) {
            let referer = if candidate.contains("www.kuaishou.com") {
                "https://www.kuaishou.com/"
            } else {
                "https://live.kuaishou.com/"
            };
            let mut req_headers = headers.clone();
            if !cookie_header.is_empty() {
                req_headers.insert("Cookie", cookie_header.parse().unwrap());
            }
            req_headers.insert("Referer", referer.parse().unwrap());

            let response = client.get(&candidate).headers(req_headers).send().await?;
            let status = response.status();
            let final_url = response.url().clone();
            let html_str = response.text().await?;

            if !status.is_success() {
                log::warn!(
                    "[Kuaishou] Web response status: {}, url: {}",
                    status,
                    final_url
                );
                if let Some(msg) = extract_rate_limit_message_from_body(&html_str) {
                    set_web_cooldown(&msg);
                    last_error = Some(RecorderError::ApiError { error: msg });
                    continue;
                }
                let snippet_len = html_str.len().min(200);
                if snippet_len > 0 {
                    log::debug!(
                        "[Kuaishou] Web error snippet: {}...",
                        &html_str[..snippet_len]
                    );
                }
                last_error = Some(RecorderError::ApiError {
                    error: format!("Kuaishou web status: {}", status),
                });
                continue;
            }

            if html_str.len() <= 256 {
                log::warn!(
                    "[Kuaishou] Web response small ({} bytes), url: {}",
                    html_str.len(),
                    final_url
                );
                if let Some(msg) = extract_rate_limit_message_from_body(&html_str) {
                    set_web_cooldown(&msg);
                    last_error = Some(RecorderError::ApiError { error: msg });
                    continue;
                }
                let snippet_len = html_str.len().min(200);
                if snippet_len > 0 {
                    log::debug!(
                        "[Kuaishou] Web small snippet: {}...",
                        &html_str[..snippet_len]
                    );
                }
                last_small = Some((html_str, final_url));
                continue;
            }

            return Ok((html_str, final_url));
        }

        if let Some((html, final_url)) = last_small {
            return Ok((html, final_url));
        }

        let err = last_error.unwrap_or_else(|| RecorderError::ApiError {
            error: "Failed to fetch Kuaishou web page".to_string(),
        });

        if let RecorderError::ApiError { error } = &err {
            if is_rate_limit_message(error) && attempt == 0 {
                log::info!(
                    "[Kuaishou] Web rate limited, retrying after {}s",
                    WEB_RATE_LIMIT_RETRY_SECS
                );
                tokio::time::sleep(Duration::from_secs(WEB_RATE_LIMIT_RETRY_SECS)).await;
                continue;
            }
        }
        return Err(err);
    }

    Err(RecorderError::ApiError {
        error: "???????????".to_string(),
    })
}

fn normalize_image_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("//") {
        format!("https:{}", trimmed)
    } else {
        trimmed.to_string()
    }
}

#[derive(Clone, Debug)]
struct FollowLiveInfo {
    caption: Option<String>,
    cover_url: Option<String>,
    user_name: Option<String>,
    user_id: Option<String>,
    user_avatar: Option<String>,
}

fn normalize_id(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn extract_kuaishou_kww(cookies: &str) -> Option<String> {
    for part in cookies.split(';').map(str::trim) {
        if let Some((key, value)) = part.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            if key == "kww" || key == "kwfv1" {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

async fn fetch_follow_live_info(
    client: &Client,
    account: &Account,
    room_id: &str,
    author_id: &str,
    author_name: &str,
) -> Option<FollowLiveInfo> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().ok()?);
    headers.insert("Accept", "application/json, text/plain, */*".parse().ok()?);
    headers.insert("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8".parse().ok()?);
    headers.insert("Referer", "https://live.kuaishou.com/".parse().ok()?);
    headers.insert("Origin", "https://live.kuaishou.com".parse().ok()?);

    let cookie_header = ensure_kuaishou_request_cookie(&account.cookies);
    if !cookie_header.is_empty() {
        headers.insert("Cookie", cookie_header.parse().ok()?);
    }
    if let Some(kww) = extract_kuaishou_kww(&cookie_header) {
        if let Ok(value) = kww.parse() {
            headers.insert("kww", value);
        }
    }

    let response = client
        .get("https://live.kuaishou.com/live_api/baseuser/userFollowCount")
        .headers(headers)
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        log::debug!("[Kuaishou] userFollowCount status: {}", response.status());
        return None;
    }

    let body = response.text().await.ok()?;
    let data: UserFollowCountResponse = serde_json::from_str(&body).ok()?;
    let follow_list = data.data?.follow;

    let room_id_norm = normalize_id(room_id);
    let author_id_norm = normalize_id(author_id);
    let author_name_norm = normalize_id(author_name);

    for item in follow_list {
        let Some(user) = item.user else {
            continue;
        };
        let principal_id = user
            .principal_id
            .as_deref()
            .map(normalize_id)
            .unwrap_or_default();
        let user_id = user
            .user_id
            .as_deref()
            .map(normalize_id)
            .unwrap_or_default();
        let user_name = user
            .user_name
            .as_deref()
            .map(normalize_id)
            .unwrap_or_default();

        let matches = (!room_id_norm.is_empty() && principal_id == room_id_norm)
            || (!author_id_norm.is_empty() && user_id == author_id_norm)
            || (!author_name_norm.is_empty() && user_name == author_name_norm);

        if matches {
            return Some(FollowLiveInfo {
                caption: item.caption,
                cover_url: item.cover_url,
                user_name: user.user_name,
                user_id: user.user_id,
                user_avatar: user.head_url,
            });
        }
    }

    None
}

async fn fetch_livedetail_info(
    client: &Client,
    account: &Account,
    room_id: &str,
) -> Option<FollowLiveInfo> {
    let cookie_header = ensure_kuaishou_request_cookie(&account.cookies);
    let did = get_cookie_value_ci(&cookie_header, "did")
        .or_else(|| get_cookie_value_ci(&cookie_header, "_did"))
        .unwrap_or_else(gen_web_did);
    let signer = crate::reverse_generate::kuaishou_sign::KuaishouSign::new(&did);
    let query_str = format!("principalId={}", room_id);
    let sign = signer.generate_sign(&query_str);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().ok()?);
    headers.insert("Accept", "application/json, text/plain, */*".parse().ok()?);
    headers.insert("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8".parse().ok()?);
    headers.insert("Referer", format!("https://live.kuaishou.com/u/{room_id}").parse().ok()?);
    headers.insert("Origin", "https://live.kuaishou.com".parse().ok()?);

    if !cookie_header.is_empty() {
        headers.insert("Cookie", cookie_header.parse().ok()?);
    }
    if let Some(kww) = extract_kuaishou_kww(&cookie_header) {
        if let Ok(value) = kww.parse() {
            headers.insert("kww", value);
        }
    }

    let response = client
        .get("https://live.kuaishou.com/live_api/liveroom/livedetail")
        .query(&[("principalId", room_id), ("sign", sign.as_str())])
        .headers(headers)
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        log::debug!("[Kuaishou] livedetail status: {}", response.status());
        return None;
    }

    let body = response.text().await.ok()?;
    let value: Value = serde_json::from_str(&body).ok()?;

    let caption = find_string_value(&value, &["caption", "title", "liveTitle", "streamTitle"]);
    let cover_url = find_image_url(&value, &["coverUrl", "cover", "poster", "snapshot"]);
    let user_info = find_user_info(&value);

    Some(FollowLiveInfo {
        caption,
        cover_url,
        user_name: user_info.as_ref().map(|u| u.user_name.clone()),
        user_id: user_info.as_ref().map(|u| u.user_id.clone()),
        user_avatar: user_info.as_ref().map(|u| u.user_avatar.clone()),
    })
}

fn extract_image_url(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(map) => {
            if let Some(url) = map.get("url").and_then(|v| v.as_str()) {
                Some(url.to_string())
            } else if let Some(list) = map
                .get("urlList")
                .or_else(|| map.get("url_list"))
                .or_else(|| map.get("urls"))
                .and_then(|v| v.as_array())
            {
                list.first().and_then(|v| v.as_str()).map(|s| s.to_string())
            } else {
                None
            }
        }
        Value::Array(list) => list.first().and_then(|v| v.as_str()).map(|s| s.to_string()),
        _ => None,
    }
}

fn find_image_url(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key) {
                    if let Some(url) = extract_image_url(value) {
                        return Some(normalize_image_url(&url));
                    }
                }
            }

            for (key, value) in map {
                if key.to_ascii_lowercase().contains("cover")
                    || key.to_ascii_lowercase().contains("avatar")
                {
                    if let Some(url) = extract_image_url(value) {
                        return Some(normalize_image_url(&url));
                    }
                }
            }

            for child in map.values() {
                if let Some(url) = find_image_url(child, keys) {
                    return Some(url);
                }
            }

            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(url) = find_image_url(value, keys) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

fn quality_rank(label: &str) -> i64 {
    let lower = label.trim().to_ascii_lowercase();
    
    // Explicit resolutions
    if lower.contains("4k") || lower.contains("2160") || lower.contains("uhd") {
        return 20000;
    }
    if lower.contains("2k") || lower.contains("1440") || lower.contains("qhd") {
        return 15000;
    }

    // Kuaishou specific high quality
    if lower.contains("质臻") {
        return 12000; // Premium 1080p+ / High bitrate
    }
    
    // Source / Original
    if lower.contains("original") || lower.contains("source") || lower.contains("原画") {
        return 30000; // Highest priority
    }

    // Blu-ray variants
    if lower.contains("蓝光") || lower.contains("blue") {
        if lower.contains("8m") {
            return 8500;
        }
        if lower.contains("4m") {
            return 4500;
        }
        return 4200; // Default Blu-ray (slightly above standard 1080p)
    }

    if lower.contains("1080") || lower.contains("fhd") {
        return 4000;
    }

    if lower.contains("超清") {
        return 2500; // Super Clear (usually > 720p)
    }

    if lower.contains("720") || lower.contains("hd") {
        return 2000;
    }

    if lower.contains("高清") {
        return 1500; // High Clear
    }

    if lower.contains("540") {
        return 1200;
    }

    if lower.contains("480") || lower.contains("sd") || lower.contains("标清") {
        return 1000;
    }

    if lower.contains("360") || lower.contains("ld") || lower.contains("流畅") {
        return 600;
    }

    // Extract digits as fallback (e.g., just "1080", "720")
    // Only if the string is mostly digits to avoid matching "mp4" or similar blindly if we were less careful
    // But simplistic extraction is explicitly requested in previous versions, so we keep a robust version.
    let mut digits = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
             // Stop at first non-digit after finding digits to handle things like "720p"
            break;
        }
    }
    if let Ok(val) = digits.parse::<i64>() {
        if val > 100 { // Avoid parsing small numbers like "5" as quality
             return val;
        }
    }
    
    0
}


/// QR code information for login
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrInfo {
    pub qr_login_token: String,
    pub qr_login_signature: String,
    pub image_data: String,
    pub qr_cookie: String,
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
    pub url: String,
    pub quality: String,
    pub bitrate: Option<i64>,
    pub cookie: Option<String>,
}

/// QR code status for polling
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrStatus {
    pub code: u8,
    pub cookies: String,
    pub message: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub user_avatar: Option<String>,
}

fn gen_web_did() -> String {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        hex.push_str(&format!("{:02x}", byte));
    }
    format!("web_{hex}")
}



fn extract_qr_message(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            let keys = ["error_msg", "errorMsg", "errMsg", "message", "msg", "prompt"];
            for key in keys {
                if let Some(Value::String(msg)) = map.get(key) {
                    let trimmed = msg.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for child in map.values() {
                if let Some(msg) = extract_qr_message(child) {
                    return Some(msg);
                }
            }
            None
        }
        Value::Array(values) => {
            for child in values {
                if let Some(msg) = extract_qr_message(child) {
                    return Some(msg);
                }
            }
            None
        }
        _ => None,
    }
}

fn has_captcha_signal(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                let key_lower = key.to_ascii_lowercase();
                if key_lower.contains("captcha")
                    || key_lower.contains("verify")
                    || key_lower.contains("risk")
                    || key_lower.contains("slider")
                {
                    return true;
                }
                if let Value::String(text) = val {
                    let lower = text.to_ascii_lowercase();
                    if lower.contains("captcha")
                        || lower.contains("verify")
                        || lower.contains("slider")
                        || text.contains("\u{6ed1}\u{5757}")
                    {
                        return true;
                    }
                }
            }
            for child in map.values() {
                if has_captcha_signal(child) {
                    return true;
                }
            }
            false
        }
        Value::Array(values) => values.iter().any(has_captcha_signal),
        _ => false,
    }
}

fn extract_qr_scan_user(value: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let user = value.get("user");
    let user_id = user
        .and_then(|v| v.get("user_id").or_else(|| v.get("userId")))
        .and_then(|v| v.as_i64().map(|n| n.to_string()).or_else(|| v.as_str().map(|s| s.to_string())))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let user_name = user
        .and_then(|v| v.get("user_name").or_else(|| v.get("userName")).or_else(|| v.get("name")))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let user_avatar = user
        .and_then(|v| v.get("headurl").or_else(|| v.get("headUrl")).or_else(|| v.get("avatar")))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (user_id, user_name, user_avatar)
}

/// Get room information from web page
pub async fn get_room_info(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<RoomInfo, RecorderError> {
    let account_obj = ensure_guest_cookie(account);
    let account = &account_obj;
    let (html_str, _final_url) = fetch_web_html(client, account, url).await?;
    if is_rate_limit_message(&html_str) {
        let message = "访问太快，请稍后再试。";
        set_web_cooldown(message);
        return Err(RecorderError::ApiError {
            error: message.to_string(),
        });
    }
    let has_fallback_stream = extract_hls_play_url(&html_str).is_some();
    let fallback_user_id = extract_user_id_from_url(url);
    let (html_title, html_cover, html_avatar) = extract_metadata_from_html(&html_str);

    let fallback_user_name = html_title.clone().unwrap_or_else(|| "Kuaishou Live".to_string());

    let fallback_title = html_title
        .clone()
        .unwrap_or_else(|| format!("{}'s live", fallback_user_name));
    let mut fallback_room_info = RoomInfo {
        live_status: has_fallback_stream,
        room_title: fallback_title.clone(),
        room_cover_url: html_cover.clone().unwrap_or_default(),
        user_id: fallback_user_id.clone(),
        user_name: fallback_user_name.clone(),
        user_avatar: html_avatar.clone().unwrap_or_default(),
    };

    if fallback_room_info.room_title == fallback_title {
        let follow_info = match fetch_follow_live_info(
            client,
            account,
            &fallback_user_id,
            &fallback_room_info.user_id,
            &fallback_room_info.user_name,
        )
        .await
        {
            Some(info) => Some(info),
            None => fetch_livedetail_info(client, account, &fallback_user_id).await,
        };

        if let Some(info) = follow_info {
            if let Some(caption) = info.caption.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                fallback_room_info.room_title = caption.to_string();
            }
            if let Some(name) = info.user_name.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                fallback_room_info.user_name = name.to_string();
            }
            if let Some(uid) = info.user_id.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                fallback_room_info.user_id = uid.to_string();
            }
            if fallback_room_info.room_cover_url.is_empty() {
                if let Some(cover) = info.cover_url.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                    fallback_room_info.room_cover_url = normalize_image_url(cover);
                }
            }
            if fallback_room_info.user_avatar.is_empty() {
                if let Some(avatar) = info.user_avatar.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                    fallback_room_info.user_avatar = normalize_image_url(avatar);
                }
            }
        }
    }
    if fallback_room_info.room_title == fallback_title {
        if !fallback_room_info.user_name.trim().is_empty() {
            fallback_room_info.room_title = fallback_room_info.user_name.clone();
        }
    }

    // Parse JSON from script tag
    let json_str = match extract_initial_state(&html_str) {
        Some(json_str) => json_str,
        None => {
            return Ok(fallback_room_info.clone());
        }
    };




    let state_value = serde_json::from_str::<Value>(&json_str).ok();

    let live_data = match parse_live_stream_response(&json_str) {
        Ok(live_data) => live_data,
        Err(_) => {
            return Ok(fallback_room_info.clone());
        }
    };

    // Check for errors
    if let Some(error) = live_data.error_type {
        return Err(RecorderError::ApiError {
            error: format!("{}: {}", error.title, error.content),
        });
    }

    let live_stream = live_data.live_stream.ok_or(RecorderError::ApiError {
        error: "No liveStream found in response".to_string(),
    })?;

    let author = live_data.author.unwrap_or_default();
    let mut author_name = if author.name.is_empty() {
        fallback_user_name.clone()
    } else {
        author.name.clone()
    };
    let mut author_id = if author.id.is_empty() {
        fallback_user_id.clone()
    } else {
        author.id.clone()
    };
    let author_avatar = author
        .head_url
        .clone()
        .map(|url| normalize_image_url(&url))
        .filter(|url| !url.is_empty())
        .or_else(|| {
             state_value.as_ref().and_then(|value| {
                 find_image_url(value, &["headurl", "headUrl", "avatar", "avatarUrl", "portrait", "profilePic", "avatarThumb"])
             })
        })
        .unwrap_or_default();

    let is_live = live_stream.play_urls.is_some()
        && live_stream
            .play_urls
            .as_ref()
            .and_then(|p| p.h264.as_ref())
            .and_then(|h| h.adaptation_set.as_ref())
            .map(|a| !a.representation.is_empty())
            .unwrap_or(false);

    let cover_url = live_stream
        .cover_url
        .clone()
        .map(|url| normalize_image_url(&url))
        .filter(|url| !url.is_empty());
        
    let room_cover_url = if let Some(url) = cover_url {
        url
    } else {
        // Try finding cover recursively, BUT explicitly exclude avatar-like keys first
         if let Some(value) = state_value.as_ref() {
             find_image_url(value, &["cover", "coverUrl", "poster", "image"])
                .or_else(|| {
                     // Try regex fallback for poster/cover patterns in HTML
                     let patterns = [
                         r#""poster"\s*:\s*"([^"]+)""#,
                         r#""coverUrl"\s*:\s*"([^"]+)""#,
                         r#""cover"\s*:\s*"([^"]+)""#,
                     ];
                     for pattern in patterns {
                         if let Ok(re) = Regex::new(pattern) {
                             if let Some(cap) = re.captures(&json_str) {
                                  if let Some(m) = cap.get(1) {
                                      return decode_json_string(m.as_str());
                                  }
                             }
                         }
                     }
                     None
                })
                .unwrap_or_else(|| author_avatar.clone())
         } else {
             author_avatar.clone()
         }
    };

    let fallback_title = format!("{}'s live", author_name);
    let extra_title = state_value
        .as_ref()
        .and_then(|value| find_string_value(value, &["caption", "title", "liveTitle", "streamTitle"]))
        .filter(|t| is_title_useful(t, &author_name));

    let mut final_title = live_stream
        .caption
        .clone()
        .or_else(|| live_data.config.and_then(|c| c.caption))
        .filter(|s| !s.is_empty())
        .or(html_title)
        .or(extra_title)
        .unwrap_or_else(|| fallback_title.clone());

    let mut final_cover = if room_cover_url.is_empty() {
        html_cover.unwrap_or_default()
    } else {
        room_cover_url
    };

    let mut final_avatar = if author_avatar.is_empty() {
        html_avatar.unwrap_or_default()
    } else {
        author_avatar
    };

    if final_title == fallback_title {
        let follow_info = match fetch_follow_live_info(
            client,
            account,
            &fallback_user_id,
            &author_id,
            &author_name,
        )
        .await
        {
            Some(info) => Some(info),
            None => fetch_livedetail_info(client, account, &fallback_user_id).await,
        };
        if let Some(follow_info) = follow_info {
            if let Some(caption) = follow_info
                .caption
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                final_title = caption.to_string();
            }
            if author_name.is_empty() {
                if let Some(name) = follow_info.user_name {
                    if !name.trim().is_empty() {
                        author_name = name;
                    }
                }
            }
            if author_id.is_empty() {
                if let Some(id) = follow_info.user_id {
                    if !id.trim().is_empty() {
                        author_id = id;
                    }
                }
            }
            if final_cover.is_empty() {
                if let Some(cover) = follow_info.cover_url {
                    if !cover.trim().is_empty() {
                        final_cover = normalize_image_url(&cover);
                    }
                }
            }
            if final_avatar.is_empty() {
                if let Some(avatar) = follow_info.user_avatar {
                    if !avatar.trim().is_empty() {
                        final_avatar = normalize_image_url(&avatar);
                    }
                }
            }
        }
    }
    if final_title == fallback_title && !author_name.trim().is_empty() {
        final_title = author_name.clone();
    }

    Ok(RoomInfo {
        live_status: is_live,
        room_title: final_title,
        room_cover_url: final_cover,
        user_id: author_id,
        user_name: author_name,
        user_avatar: final_avatar,
    })
}
/// Get stream URLs from Kuaishou web page
pub async fn get_stream_urls(
    client: &Client,
    account: &Account,
    url: &str,
) -> Result<Vec<StreamInfo>, RecorderError> {
    let account_obj = ensure_guest_cookie(account);
    let account = &account_obj;
    let (html_str, _final_url) = fetch_web_html(client, account, url).await?;
    let fallback_hls = extract_hls_play_url(&html_str).map(|hls_url| StreamInfo {
        url: hls_url,
        quality: "蓝光质臻".to_string(), // HLS adaptive streaming, typically delivers 720p+
        bitrate: None,
        cookie: Some(account.cookies.clone()),
    });
    let mut urls = Vec::new();

    let json_str = match extract_initial_state(&html_str) {
        Some(json_str) => json_str,
        None => {
            if let Some(fallback) = fallback_hls.clone() {
                urls.push(fallback);
            }
            if !urls.is_empty() {
                return Ok(urls);
            }
            return Err(RecorderError::ApiError {
                error: "Failed to extract JSON data from page".to_string(),
            });
        }
    };

    let live_data = match parse_live_stream_response(&json_str) {
        Ok(live_data) => live_data,
        Err(e) => {
            if let Some(fallback) = fallback_hls.clone() {
                urls.push(fallback);
            }
            if !urls.is_empty() {
                return Ok(urls);
            }
            return Err(e);
        }
    };

    let live_stream = match live_data.live_stream {
        Some(live_stream) => live_stream,
        None => {
            if !urls.is_empty() {
                return Ok(urls);
            }
            return Err(RecorderError::ApiError {
                error: "No liveStream found in response".to_string(),
            });
        }
    };

    let play_urls = match live_stream.play_urls {
        Some(play_urls) => play_urls,
        None => {
            if !urls.is_empty() {
                return Ok(urls);
            }
            return Err(RecorderError::ApiError {
                error: "No playUrls found in response".to_string(),
            });
        }
    };

    let mut all_representations = Vec::new();

    if let Some(h264) = play_urls.h264 {
        if let Some(set) = h264.adaptation_set {
            all_representations.extend(set.representation);
        }
    }

    if let Some(h265) = play_urls.h265 {
        if let Some(set) = h265.adaptation_set {
            all_representations.extend(set.representation);
        }
    }

    if all_representations.is_empty() {
        if !urls.is_empty() {
            return Ok(urls);
        }
        return Err(RecorderError::ApiError {
            error: "No usable stream representations found".to_string(),
        });
    }

    all_representations.sort_by(|a, b| b.bitrate.unwrap_or(0).cmp(&a.bitrate.unwrap_or(0)));

    // Remove duplicates based on URL
    let mut seen_urls = std::collections::HashSet::new();

    urls.extend(all_representations.into_iter().filter_map(|rep| {
        if seen_urls.contains(&rep.url) {
            return None;
        }
        seen_urls.insert(rep.url.clone());
        Some(StreamInfo {
            url: rep.url,
            quality: rep.name.or(rep.quality_type).unwrap_or_default(),
            bitrate: rep.bitrate,
            cookie: Some(account.cookies.clone()),
        })
    }));

    if !urls.iter().any(|stream| stream.url.contains(".m3u8")) {
        if let Some(fallback) = fallback_hls.clone() {
            urls.insert(0, fallback);
        }
    }

    urls.sort_by(|a, b| {
        let a_m3u8 = a.url.contains(".m3u8");
        let b_m3u8 = b.url.contains(".m3u8");
        b_m3u8
            .cmp(&a_m3u8)
            .then_with(|| b.bitrate.unwrap_or(0).cmp(&a.bitrate.unwrap_or(0)))
            .then_with(|| quality_rank(&b.quality).cmp(&quality_rank(&a.quality)))
    });

    if !urls.iter().any(|stream| stream.url.contains(".m3u8")) {
        if let Some(flv_url) = urls
            .iter()
            .find(|stream| stream.url.contains(".flv"))
            .map(|stream| stream.url.clone())
        {
            let guessed_hls = flv_url.replacen(".flv", ".m3u8", 1);
            if guessed_hls != flv_url {
                log::info!("[Kuaishou] No m3u8 found, guessing HLS from FLV URL");
                urls.insert(
                    0,
                    StreamInfo {
                        url: guessed_hls,
                        quality: "蓝光质臻".to_string(), // HLS adaptive streaming, typically delivers 720p+
                        bitrate: None,
                        cookie: Some(account.cookies.clone()),
                    },
                );
            }
        }
    }

    // Log available stream qualities for debugging
    if !urls.is_empty() {
        log::info!("[Kuaishou] Found {} stream(s):", urls.len());
        for (i, stream) in urls.iter().enumerate() {
            log::info!(
                "  [{}] Quality: {}, Bitrate: {}, Format: {}",
                i,
                if stream.quality.is_empty() { "unknown" } else { &stream.quality },
                stream
                    .bitrate
                    .map_or("unknown".to_string(), |b| format!("{} kbps", b)),
                if stream.url.contains(".m3u8") {
                    "HLS"
                } else if stream.url.contains(".flv") {
                    "FLV"
                } else {
                    "other"
                }
            );
        }
    } else {
        log::warn!("[Kuaishou] No streams found");
    }

    Ok(urls)
}


fn ensure_guest_cookie(account: &crate::account::Account) -> crate::account::Account {
    let mut next = account.clone();
    let normalized = normalize_cookie_header(&next.cookies);
    let ensured = ensure_kuaishou_base_cookies(&normalized);
    if ensured != next.cookies {
        next.cookies = ensured;
    } else if normalized != next.cookies {
        next.cookies = normalized;
    }
    next
}

pub async fn fetch_guest_state(client: &Client) -> Result<String, RecorderError> {
    log::info!("[Kuaishou] Fetching guest state from homepage...");
    let url = "https://live.kuaishou.com/";
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    
    // Attempt fetch
    let response = client.get(url).headers(headers).send().await?;
    
    let mut map = HashMap::new();
    for value in response.headers().get_all(reqwest::header::SET_COOKIE).iter() {
        if let Ok(raw) = value.to_str() {
            if let Some((pair, _)) = raw.split_once(';') {
                if let Some((name, val)) = pair.split_once('=') {
                     map.insert(name.trim().to_string(), val.trim().to_string());
                }
            }
        }
    }
    
    if let Some(did) = map.get("did") {
        log::info!("[Kuaishou] Found device_id: {}", did);
        return Ok(did.to_string());
    }
    
    Ok(String::new())
}

/// Get QR code for login
pub async fn get_qr(client: &Client) -> Result<QrInfo, RecorderError> {
    // Check overrides
    let overrides = crate::reverse_generate::qr_login::fetch_kuaishou_overrides().unwrap_or_default();
    
    let mut headers = reqwest::header::HeaderMap::new();
    let user_agent = overrides.get("user_agent").filter(|v| !v.is_empty())
        .map(|v| v.as_str())
        .unwrap_or(USER_AGENT);
    
    headers.insert("User-Agent", user_agent.parse().unwrap());
    headers.insert(
        "Content-Type",
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    headers.insert("Referer", "https://live.kuaishou.com/".parse().unwrap());
    
    // Handle device_id (did)
    let mut did = overrides.get("device_id").filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .unwrap_or_default();
        
    if did.is_empty() {
        if let Ok(real_did) = fetch_guest_state(client).await {
            if !real_did.is_empty() {
                did = real_did;
                let _ = crate::reverse_generate::qr_login::update_kuaishou_config(Some(&did), None, true);
            }
        }
    }
    // Fallback to random if still empty
    if did.is_empty() {
        did = gen_web_did();
    }
    
    // Check custom cookie in overrides
    let mut qr_cookie = if let Some(cookie) = overrides.get("cookie").filter(|v| !v.is_empty()) {
        cookie.clone()
    } else {
        // Build cookie using did
        let didv = Utc::now().timestamp_millis();
        format!("did={did}; didv={didv}; kwpsecproductname=PCLive")
    };
    
    headers.insert("Cookie", qr_cookie.parse().unwrap());

    let response = client
        .post("https://id.kuaishou.com/rest/c/infra/ks/qr/start")
        .headers(headers)
        .body("sid=kuaishou.live.web&channelType=UNKNOWN&encryptHeaders=")
        .send()
        .await?;
    let response_headers = response.headers().clone();

    let data: serde_json::Value = response.json().await?;
    let mut cookie_map: HashMap<String, String> = HashMap::new();
    for cookie_part in qr_cookie.split(';') {
        if let Some((name, value)) = cookie_part.trim().split_once('=') {
            cookie_map.insert(name.trim().to_string(), value.trim().to_string());
        }
    }
    for cookie_header in response_headers.get_all("set-cookie") {
        if let Ok(cookie_str) = cookie_header.to_str() {
            if let Some(cookie_part) = cookie_str.split(';').next() {
                if let Some((name, value)) = cookie_part.split_once('=') {
                    cookie_map.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }
    }
    let mut cookie_pairs: Vec<String> = cookie_map
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    cookie_pairs.sort();
    qr_cookie = cookie_pairs.join("; ");

    Ok(QrInfo {
        qr_login_token: data["qrLoginToken"]
            .as_str()
            .ok_or(RecorderError::InvalidValue)?
            .to_string(),
        qr_login_signature: data["qrLoginSignature"]
            .as_str()
            .ok_or(RecorderError::InvalidValue)?
            .to_string(),
        image_data: data["imageData"]
            .as_str()
            .ok_or(RecorderError::InvalidValue)?
            .to_string(),
        qr_cookie,
    })
}
pub fn get_kuaishou_cookie_item(cookies: &str, key: &str) -> Option<String> {
    for cookie in cookies.split(';').map(str::trim) {
        if let Some((name, value)) = cookie.split_once('=') {
            if name.trim() == key {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}



fn find_string_value(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(Value::String(value)) = map.get(*key) {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for child in map.values() {
                if let Some(found) = find_string_value(child, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(values) => {
            for child in values {
                if let Some(found) = find_string_value(child, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn is_title_useful(title: &str, author_name: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return false;
    }
    if !author_name.trim().is_empty() && trimmed == author_name.trim() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("????") || lower.contains("error code") || lower.contains("????") {
        return false;
    }
    if lower.ends_with("'s live") || trimmed.ends_with("???") {
        return false;
    }
    true
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

fn user_info_from_map(map: &Map<String, Value>) -> Option<crate::UserInfo> {
    let user_id = get_string_field(map, &["user_id", "userId", "userIdStr", "uid"]);
    let user_name = get_string_field(map, &["user_name", "userName", "nickname", "nickName"]);
    if let (Some(user_id), Some(user_name)) = (user_id, user_name) {
        if user_id == "1" || user_name == "英雄联盟" {
            return None;
        }
        let user_avatar = get_string_field(
            map,
            &["headurl", "headUrl", "avatar", "avatarUrl", "portrait", "profilePic"],
        )
        .unwrap_or_default();
        return Some(crate::UserInfo {
            user_id,
            user_name,
            user_avatar,
        });
    }

    let user_id = get_string_field(map, &["id"]);
    let user_name = get_string_field(map, &["name"]);
    if let (Some(user_id), Some(user_name)) = (user_id, user_name) {
        if user_id == "1" || user_name == "英雄联盟" {
            return None;
        }
        let user_avatar = get_string_field(
            map,
            &["headurl", "headUrl", "avatar", "avatarUrl", "portrait", "profilePic"],
        )
        .unwrap_or_default();
        return Some(crate::UserInfo {
            user_id,
            user_name,
            user_avatar,
        });
    }

    None
}

fn find_user_info(value: &Value) -> Option<crate::UserInfo> {
    match value {
        Value::Object(map) => {
            if let Some(user_info) = user_info_from_map(map) {
                return Some(user_info);
            }
            for child in map.values() {
                if let Some(user_info) = find_user_info(child) {
                    return Some(user_info);
                }
            }
            None
        }
        Value::Array(values) => {
            for value in values {
                if let Some(user_info) = find_user_info(value) {
                    return Some(user_info);
                }
            }
            None
        }
        _ => None,
    }
}

async fn fetch_baseuser_info(
    client: &Client,
    account: &Account,
) -> Option<crate::UserInfo> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().ok()?);
    headers.insert("Accept", "application/json, text/plain, */*".parse().ok()?);
    headers.insert("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8".parse().ok()?);
    headers.insert("Referer", "https://live.kuaishou.com/".parse().ok()?);
    headers.insert("Origin", "https://live.kuaishou.com".parse().ok()?);

    let cookie_header = ensure_kuaishou_request_cookie(&account.cookies);
    if !cookie_header.is_empty() {
        headers.insert("Cookie", cookie_header.parse().ok()?);
    }
    if let Some(kww) = extract_kuaishou_kww(&cookie_header) {
        if let Ok(value) = kww.parse() {
            headers.insert("kww", value);
        }
    }

    let response = client
        .get("https://live.kuaishou.com/live_api/baseuser/userinfo")
        .headers(headers)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.text().await.ok()?;
    let value: Value = serde_json::from_str(&body).ok()?;
    if let Some(info) = find_user_info(&value) {
        return Some(info);
    }
    value
        .get("data")
        .and_then(find_user_info)
}

/// Get user information from cookies
pub async fn get_user_info(
    client: &Client,
    account: &Account,
) -> Result<crate::UserInfo, RecorderError> {
    let account = ensure_guest_cookie(account);
    let uid = get_cookie_value_ci(&account.cookies, "userId")
        .or_else(|| get_cookie_value_ci(&account.cookies, "user_id"))
        .or_else(|| get_cookie_value_ci(&account.cookies, "userIdStr"))
        .or_else(|| get_cookie_value_ci(&account.cookies, "uid"));

    if let Some(info) = fetch_baseuser_info(client, &account).await {
        return Ok(info);
    }

    let mut candidates = vec![
        "https://live.kuaishou.com/".to_string(),
        "https://www.kuaishou.com/".to_string(),
    ];

    if let Some(uid) = uid.as_ref() {
        candidates.push(format!("https://live.kuaishou.com/u/{uid}"));
        candidates.push(format!("https://www.kuaishou.com/u/{uid}"));
        candidates.push(format!("https://www.kuaishou.com/profile/{uid}"));
    }

    for url in candidates {
        let (html_str, _) = match fetch_web_html(client, &account, &url).await {
            Ok(result) => result,
            Err(_) => continue,
        };

        if let Some(json_str) = extract_initial_state(&html_str) {
            if let Ok(state) = serde_json::from_str::<Value>(&json_str) {
                if let Some(user_info) = find_user_info(&state) {
                    return Ok(user_info);
                }
            }

            #[derive(Deserialize)]
            struct KuaishouUser {
                #[serde(default, alias = "user_id", alias = "userId", alias = "id")]
                user_id: String,
                #[serde(default, alias = "user_name", alias = "userName", alias = "name")]
                user_name: String,
                #[serde(
                    default,
                    alias = "headurl",
                    alias = "headUrl",
                    alias = "avatar",
                    alias = "avatarUrl"
                )]
                head_url: String,
            }

            let user_regex =
                Regex::new(r#"(?s)"profile":\{"ownerCount".*?"user":(.*?),"currentWork"#)
                    .map_err(|e| RecorderError::ApiError {
                        error: format!("Failed to create user regex: {}", e),
                    })?;

            if let Some(user_str) = user_regex
                .captures(&json_str)
                .and_then(|cap| cap.get(1))
                .map(|m| m.as_str())
            {
                if let Ok(user) = serde_json::from_str::<KuaishouUser>(user_str) {
                    if !user.user_id.is_empty() || !user.user_name.is_empty() {
                        return Ok(crate::UserInfo {
                            user_id: user.user_id,
                            user_name: user.user_name,
                            user_avatar: user.head_url,
                        });
                    }
                }
            }
        }

        let (title, _cover, avatar) = extract_metadata_from_html(&html_str);
        if title.is_some() || avatar.is_some() {
            return Ok(crate::UserInfo {
                user_id: "".to_string(),
                user_name: title.unwrap_or_else(|| "Kuaishou".to_string()),
                user_avatar: avatar.unwrap_or_default(),
            });
        }
    }

    if let Some(uid) = uid {
        if let Some(info) = fetch_livedetail_info(client, &account, &uid).await {
            return Ok(crate::UserInfo {
                user_id: info.user_id.unwrap_or(uid),
                user_name: info.user_name.unwrap_or_else(|| "Kuaishou".to_string()),
                user_avatar: info.user_avatar.unwrap_or_default(),
            });
        }
    }

    Err(RecorderError::ApiError {
        error: "Failed to parse user info from page".to_string(),
    })
}

/// Poll QR code status and get cookies after successful login
pub async fn get_qr_status(
    client: &Client,
    qr_login_token: &str,
    qr_login_signature: &str,
    qr_cookie: Option<&str>,
) -> Result<QrStatus, RecorderError> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", USER_AGENT.parse().unwrap());
    headers.insert(
        "Content-Type",
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    headers.insert("Referer", "https://live.kuaishou.com/".parse().unwrap());
    if let Some(cookie) = qr_cookie {
        if !cookie.trim().is_empty() {
            headers.insert("Cookie", cookie.parse().unwrap());
        }
    }

    let payload = format!(
        "qrLoginToken={qr_login_token}&qrLoginSignature={qr_login_signature}&channelType=UNKNOWN&encryptHeaders=&sid=kuaishou.live.web"
    );

    // Step 1: Check scan status
    let scan_response = client
        .post("https://id.kuaishou.com/rest/c/infra/ks/qr/scanResult")
        .headers(headers.clone())
        .body(payload.clone())
        .send()
        .await?;

    let scan_data: serde_json::Value = scan_response.json().await?;
    log::warn!("[Kuaishou] QR scanResult: {}", scan_data);
    let (scan_user_id, scan_user_name, scan_user_avatar) = extract_qr_scan_user(&scan_data);

    // If not scanned yet, return pending status
    if scan_data["result"].as_u64().unwrap_or(1) != 1 {
        let message = extract_qr_message(&scan_data);
        return Ok(QrStatus {
            code: 1,
            cookies: String::new(),
            message,
            user_id: scan_user_id,
            user_name: scan_user_name,
            user_avatar: scan_user_avatar,
        });
    }

    // Step 2: Check accept status
    let accept_response = client
        .post("https://id.kuaishou.com/rest/c/infra/ks/qr/acceptResult")
        .headers(headers.clone())
        .body(payload)
        .send()
        .await?;

    let accept_data: serde_json::Value = accept_response.json().await?;
    log::warn!("[Kuaishou] QR acceptResult: {}", accept_data);

    // If not accepted yet, return pending status
    if accept_data["result"].as_u64().unwrap_or(1) != 1 {
        let message = extract_qr_message(&accept_data);
        return Ok(QrStatus {
            code: 2,
            cookies: String::new(),
            message,
            user_id: scan_user_id,
            user_name: scan_user_name,
            user_avatar: scan_user_avatar,
        });
    }

    // Step 3: Get qrToken and perform callback
    let qr_token = accept_data["qrToken"]
        .as_str()
        .ok_or(RecorderError::InvalidValue)?;

    let callback_response = client
        .post("https://id.kuaishou.com/pass/kuaishou/login/qr/callback")
        .headers(headers.clone())
        .body(format!("qrToken={qr_token}&sid=kuaishou.live.web"))
        .send()
        .await?;

    let callback_headers = callback_response.headers().clone();
    let callback_json: serde_json::Value = callback_response.json().await.unwrap_or_default();
    log::warn!("[Kuaishou] QR callback: {}", callback_json);
    let callback_message = extract_qr_message(&callback_json);

    let mut cookies_map: HashMap<String, String> = HashMap::new();
    for cookie_header in callback_headers.get_all("set-cookie") {
        if let Ok(cookie_str) = cookie_header.to_str() {
            if let Some(cookie_part) = cookie_str.split(';').next() {
                if let Some((name, value)) = cookie_part.split_once('=') {
                    cookies_map.insert(name.trim().to_string(), value.trim().to_string());
                }
            }
        }
    }

    if let Some(value) = callback_json.get("kuaishou.live.web_st").and_then(|v| v.as_str()) {
        if !value.is_empty() {
            cookies_map.insert("kuaishou.live.web_st".to_string(), value.to_string());
        }
    }
    if let Some(value) = callback_json.get("kuaishou.live.web.at").and_then(|v| v.as_str()) {
        if !value.is_empty() {
            cookies_map.insert("kuaishou.live.web.at".to_string(), value.to_string());
        }
    }
    if let Some(value) = callback_json.get("ssecurity").and_then(|v| v.as_str()) {
        if !value.is_empty() {
            cookies_map.insert("ssecurity".to_string(), value.to_string());
        }
    }
    if let Some(value) = callback_json.get("passToken").and_then(|v| v.as_str()) {
        if !value.is_empty() {
            cookies_map.insert("passToken".to_string(), value.to_string());
        }
    }
    if let Some(value) = callback_json.get("userId").and_then(|v| v.as_i64()) {
        cookies_map.insert("userId".to_string(), value.to_string());
    }

    let mut cookies_vec: Vec<String> = cookies_map
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    cookies_vec.sort();
    let cookies = cookies_vec.join("; ");

    if cookies.is_empty() {
        if let Some(message) = callback_message {
            return Ok(QrStatus {
                code: 2,
                cookies: String::new(),
                message: Some(message),
                user_id: scan_user_id,
                user_name: scan_user_name,
                user_avatar: scan_user_avatar,
            });
        }
        if has_captcha_signal(&callback_json) {
            return Ok(QrStatus {
                code: 2,
                cookies: String::new(),
                message: Some("\u{9700}\u{8981}\u{6ed1}\u{5757}\u{9a8c}\u{8bc1}".to_string()),
                user_id: scan_user_id,
                user_name: scan_user_name,
                user_avatar: scan_user_avatar,
            });
        }
        return Err(RecorderError::ApiError {
            error: "Failed to extract cookies from response".to_string(),
        });
    }

    Ok(QrStatus {
        code: 0,
        cookies,
        message: None,
        user_id: scan_user_id,
        user_name: scan_user_name,
        user_avatar: scan_user_avatar,
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
