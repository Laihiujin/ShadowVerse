use std::env;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use crate::database::account::AccountRow;
use crate::config::{
    load_accounts_file_or_example, resolve_accounts_file_write_path, write_accounts_file, AccountsFile,
    Config, DefaultAccountConfig,
};
use crate::database::Database;
use crate::state::State;
use crate::state_type;
use crate::utils::browser::BrowserCookieCollector;
use chrono::Utc;
use rand::Rng;
use recorder::platforms::{
    bilibili, douyin, huya, kuaishou, tiktok, weibo, xiaohongshu, PlatformType,
};
use recorder::UserInfo;
use serde::{Deserialize, Serialize};
use serde_json::json;

use hyper::header::HeaderValue;
use reqwest::header::{HeaderMap, USER_AGENT};
#[cfg(feature = "gui")]
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
#[cfg(feature = "gui")]
use tokio::time::Duration;
#[cfg(feature = "gui")]
use url::Url;

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn get_accounts(state: state_type!()) -> Result<super::AccountInfo, String> {
    let account_info = super::AccountInfo {
        accounts: state.db.get_accounts().await?,
    };
    Ok(account_info)
}

fn get_item_from_cookies(name: &str, cookies: &str) -> Result<String, String> {
    Ok(cookies
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix(format!("{name}=").as_str()))
        .ok_or_else(|| format!("Invalid cookies: missing {name}").to_string())?
        .to_string())
}

fn normalize_cookie_input(raw: &str) -> String {
    let replaced = raw
        .replace('；', ";")
        .replace('，', ",")
        .replace('＝', "=");
    let parts: Vec<&str> = replaced
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    parts.join("; ")
}

fn sanitize_cookie_header(raw: &str) -> Result<String, String> {
    let normalized = normalize_cookie_input(raw);
    if HeaderValue::from_str(&normalized).is_ok() {
        return Ok(normalized);
    }
    let ascii_only: String = normalized.chars().filter(|c| c.is_ascii()).collect();
    if HeaderValue::from_str(&ascii_only).is_ok() {
        return Ok(ascii_only);
    }
    Err("Invalid cookies".to_string())
}

fn normalize_cookie_string_for_compare(cookies: &str) -> String {
    let mut pairs: Vec<String> = cookies
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some(format!("{}={}", key.trim(), value.trim()))
        })
        .collect();
    pairs.sort();
    pairs.join("; ")
}

fn remove_cookie_keys_ci(
    cookie_map: &mut std::collections::HashMap<String, String>,
    keys: &[&str],
) {
    let mut remove_keys = Vec::new();
    for key in keys {
        let target = key.to_ascii_lowercase();
        for existing in cookie_map.keys() {
            if existing.to_ascii_lowercase() == target {
                remove_keys.push(existing.clone());
            }
        }
    }
    for key in remove_keys {
        cookie_map.remove(&key);
    }
}

fn get_item_from_cookies_ci(name: &str, cookies: &str) -> Result<String, String> {
    let target = name.to_ascii_lowercase();
    for cookie in cookies.split(';').map(str::trim) {
        if let Some((key, value)) = cookie.split_once('=') {
            if key.trim().to_ascii_lowercase() == target {
                return Ok(value.to_string());
            }
        }
    }
    Err(format!("Invalid cookies: missing {name}").to_string())
}

fn extract_numeric_after_marker(text: &str, marker: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let needle = marker.as_bytes();
    if needle.is_empty() || bytes.len() < needle.len() {
        return None;
    }
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            let max = (j + 200).min(bytes.len());
            while j < max && !bytes[j].is_ascii_digit() {
                j += 1;
            }
            let start = j;
            while j < max && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > start {
                return Some(String::from_utf8_lossy(&bytes[start..j]).to_string());
            }
        }
        i += 1;
    }
    None
}

fn gen_kuaishou_web_did() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        hex.push_str(&format!("{:02x}", byte));
    }
    format!("web_{hex}")
}

fn ensure_kuaishou_base_cookies(cookie_map: &mut std::collections::HashMap<String, String>) {
    if !cookie_map.contains_key("did") {
        cookie_map.insert("did".to_string(), gen_kuaishou_web_did());
    }
    if !cookie_map.contains_key("didv") {
        cookie_map.insert("didv".to_string(), Utc::now().timestamp_millis().to_string());
    }
    if !cookie_map.contains_key("kwpsecproductname") {
        cookie_map.insert("kwpsecproductname".to_string(), "PCLive".to_string());
    }
}

async fn fetch_huya_uid_from_cookie(client: &reqwest::Client, cookies: &str) -> Option<String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            .parse()
            .ok()?,
    );
    headers.insert("cookie", HeaderValue::from_str(cookies).ok()?);
    let urls = ["https://i.huya.com/", "https://www.huya.com/"];
    for url in urls {
        let response = client.get(url).headers(headers.clone()).send().await.ok()?;
        let body = response.text().await.ok()?;
        for key in ["yyuid", "udb_uid", "uid"] {
            if let Some(uid) = extract_numeric_after_marker(&body, key) {
                if uid.len() >= 5 {
                    return Some(uid);
                }
            }
        }
    }
    None
}

fn extract_tiktok_uid(cookies: &str) -> Option<String> {
    for cookie in cookies.split(';').map(str::trim) {
        if let Some((name, _)) = cookie.split_once('=') {
            if let Some(uid) = name.strip_prefix("__user_") {
                if !uid.is_empty() {
                    return Some(uid.to_string());
                }
            }
        }
    }
    get_item_from_cookies("sessionid", cookies)
        .or_else(|_| get_item_from_cookies("uid_tt", cookies))
        .ok()
}

fn extract_douyin_uid(cookies: &str) -> Option<String> {
    get_item_from_cookies("uid_tt", cookies)
        .or_else(|_| get_item_from_cookies("uid_tt_ss", cookies))
        .or_else(|_| get_item_from_cookies("sessionid", cookies))
        .ok()
}

fn fallback_uid_from_cookies(cookies: &str) -> String {
    // Try to extract known user ID keys
    let keys = ["userId", "user_id", "uid", "sec_uid", "yyuid"];
    for key in keys {
        if let Ok(val) = get_item_from_cookies(key, cookies) {
            if !val.trim().is_empty() {
                return val;
            }
        }
    }
    // Fallback to MD5 hash if no known key found
    format!("cookie_{:x}", md5::compute(cookies))
}

#[derive(Debug, Clone, Serialize)]
struct CookieItem {
    name: String,
    value: String,
}

#[derive(Debug, Clone, Serialize)]
struct CookieInfo {
    cookies: Vec<CookieItem>,
}

#[derive(Debug, Clone, Serialize)]
struct AccountExtra {
    cookie_info: CookieInfo,
    token_info: serde_json::Value,
    platform_tokens: serde_json::Value,
    user_info: serde_json::Value,
    login_info: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebviewCookieResult {
    pub cookies: String,
    pub extra: String,
}

const GUEST_REFRESH_MIN_INTERVAL_SECS: i64 = 120;
const GUEST_REFRESH_FORCE_MIN_INTERVAL_SECS: i64 = 30;

static GUEST_REFRESH_INFLIGHT: AtomicBool = AtomicBool::new(false);
static GUEST_REFRESH_LAST_TS: AtomicI64 = AtomicI64::new(0);

struct GuestRefreshGuard;

impl Drop for GuestRefreshGuard {
    fn drop(&mut self) {
        GUEST_REFRESH_INFLIGHT.store(false, Ordering::SeqCst);
    }
}

fn try_begin_guest_refresh(force: bool, reason: Option<&str>) -> Option<GuestRefreshGuard> {
    let now = Utc::now().timestamp();
    let last = GUEST_REFRESH_LAST_TS.load(Ordering::Relaxed);
    let min_interval = if force {
        GUEST_REFRESH_FORCE_MIN_INTERVAL_SECS
    } else {
        GUEST_REFRESH_MIN_INTERVAL_SECS
    };
    if now.saturating_sub(last) < min_interval {
        log::info!(
            "[Account] Skip guest refresh (cooldown {}s, last {}s ago){}",
            min_interval,
            now.saturating_sub(last),
            reason.map(|r| format!(", reason: {r}")).unwrap_or_default()
        );
        return None;
    }
    if GUEST_REFRESH_INFLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!(
            "[Account] Skip guest refresh (already running){}",
            reason.map(|r| format!(", reason: {r}")).unwrap_or_default()
        );
        return None;
    }
    GUEST_REFRESH_LAST_TS.store(now, Ordering::Relaxed);
    Some(GuestRefreshGuard)
}

fn cookie_list_from_header(cookies: &str) -> Vec<CookieItem> {
    cookies
        .split(';')
        .map(str::trim)
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some(CookieItem {
                name: name.to_string(),
                value: value.trim().to_string(),
            })
        })
        .collect()
}

fn build_account_extra_json(cookie_list: Vec<CookieItem>, user_info: &UserInfo) -> String {
    let extra = AccountExtra {
        cookie_info: CookieInfo { cookies: cookie_list },
        token_info: json!({}),
        platform_tokens: json!({}),
        user_info: json!({
            "user_id": user_info.user_id.clone(),
            "username": user_info.user_name.clone(),
            "avatar": user_info.user_avatar.clone(),
        }),
        login_info: json!({
            "user_id": user_info.user_id.clone(),
            "username": user_info.user_name.clone(),
            "avatar": user_info.user_avatar.clone(),
        }),
    };
    serde_json::to_string(&extra).unwrap_or_default()
}

async fn build_webview_cookie_result(
    platform: &str,
    cookie_str: String,
    cookie_list: Vec<CookieItem>,
) -> Result<WebviewCookieResult, String> {
    let account = build_account_row(platform, &cookie_str, None, None).await?;
    let user_info = UserInfo {
        user_id: account.uid.clone(),
        user_name: account.name.clone(),
        user_avatar: account.avatar.clone(),
    };
    let extra_json = build_account_extra_json(cookie_list, &user_info);
    Ok(WebviewCookieResult {
        cookies: cookie_str,
        extra: extra_json,
    })
}


fn default_douyin_webview_ua() -> &'static str {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
}


#[cfg_attr(feature = "gui", tauri::command)]
pub async fn add_account(
    state: state_type!(),
    platform: String,
    cookies: &str,
    extra: Option<String>,
) -> Result<(), String> {
    let account = build_account_row(&platform, cookies, extra, Some("manual")).await?;
    state.db.add_account(&account).await?;
    Ok(())
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn update_login_account(
    state: state_type!(),
    platform: String,
    cookies: String,
    extra: Option<String>,
) -> Result<(), String> {
    if cookies.trim().is_empty() {
        return Err("Empty cookies".to_string());
    }
    let _ = build_account_row(&platform, &cookies, None, None).await?;
    let mut old_cookies: Option<String> = None;
    {
        let mut config = state.config.write().await;
        if let Some(entry) = config
            .login_accounts
            .iter_mut()
            .find(|entry| entry.platform == platform)
        {
            if entry.cookies.trim() != cookies.trim() && !entry.cookies.trim().is_empty() {
                old_cookies = Some(entry.cookies.clone());
            }
            entry.cookies = cookies.clone();
            if let Some(extra) = extra.as_ref() {
                entry.extra = extra.clone();
            }
        } else {
            config.login_accounts.push(DefaultAccountConfig {
                platform: platform.clone(),
                cookies: cookies.clone(),
                extra: extra.clone().unwrap_or_default(),
            });
        }
        config.save();
    }
    {
        let (mut accounts, _) =
            load_accounts_file_or_example().unwrap_or_else(|| (AccountsFile::default(), resolve_accounts_file_write_path()));
        if let Some(entry) = accounts
            .login_accounts
            .iter_mut()
            .find(|entry| entry.platform == platform)
        {
            entry.cookies = cookies.clone();
            if let Some(extra) = extra.as_ref() {
                entry.extra = extra.clone();
            }
        } else {
            accounts.login_accounts.push(DefaultAccountConfig {
                platform: platform.clone(),
                cookies: cookies.clone(),
                extra: extra.clone().unwrap_or_default(),
            });
        }
        let path = resolve_accounts_file_write_path();
        if let Err(err) = write_accounts_file(&path, &accounts) {
            log::warn!("Failed to write accounts file {:?}: {}", path, err);
        }
    }
    if let Some(old_cookies) = old_cookies {
        remove_accounts_by_platform_cookies(&state.db, &platform, &old_cookies).await;
    }
    if platform == "tiktok" {
        let config_snapshot = state.config.read().await.clone();
        sync_tiktok_webview_cookies(&state.db, &config_snapshot).await;
    }
    Ok(())
}

pub async fn ensure_login_accounts(db: &Database, config: &Config) {
    if !config.use_login_accounts {
        return;
    }
    if config.login_accounts.is_empty() {
        return;
    }

    for entry in &config.login_accounts {
        let cookies = entry.cookies.trim();
        if cookies.is_empty() {
            continue;
        }
        let platform = match PlatformType::from_str(&entry.platform) {
            Ok(platform) => platform,
            Err(_) => {
                log::warn!("Skip default account with invalid platform: {}", entry.platform);
                continue;
            }
        };
        let accounts = match db.get_accounts().await {
            Ok(accounts) => accounts,
            Err(e) => {
                log::warn!("Failed to load accounts for validation: {}", e);
                continue;
            }
        };
        let existing = accounts.iter().find(|account| {
            account.platform == platform.as_str() && account.cookies == cookies
        });
        if let Some(existing) = existing {
            if let Err(e) = build_account_row(platform.as_str(), cookies, None, Some("login")).await {
                log::warn!(
                    "Login account invalid for {}: {}, reimporting",
                    platform.as_str(),
                    e
                );
                if let Err(e) = db.remove_account(platform.as_str(), &existing.uid).await {
                    log::warn!(
                        "Failed to remove invalid default account for {}: {}",
                        platform.as_str(),
                        e
                    );
                }
            } else {
                continue;
            }
        }
        match build_account_row(platform.as_str(), cookies, None, Some("login")).await {
            Ok(account) => {
                if let Err(e) = db.add_account(&account).await {
                    log::warn!(
                        "Failed to add login account for {}: {}",
                        platform.as_str(),
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to build login account for {}: {}",
                    platform.as_str(),
                    e
                );
            }
        }
    }

    // Ensure Kuaishou Danmu Cookie is loaded into env and visible in DB
    /* Kuaishou Danmu Cookie logic removed */
}

pub async fn sync_tiktok_webview_cookies(db: &Database, config: &Config) {
    let mut cookies = None;
    if config.use_guest_accounts {
        cookies = config
            .guest_accounts
            .iter()
            .find(|entry| entry.platform == "tiktok" && !entry.cookies.trim().is_empty())
            .map(|entry| entry.cookies.clone());
    } else if config.use_login_accounts {
        cookies = config
            .login_accounts
            .iter()
            .find(|entry| entry.platform == "tiktok" && !entry.cookies.trim().is_empty())
            .map(|entry| entry.cookies.clone());
    }
    if cookies.is_none() {
        if let Ok(account) = db.get_account_by_platform("tiktok").await {
            if !account.cookies.trim().is_empty() {
                cookies = Some(account.cookies);
            }
        }
    }
    if let Some(cookies) = cookies {
        env::set_var("TIKTOK_WEBVIEW_COOKIES", cookies);
    } else {
        env::remove_var("TIKTOK_WEBVIEW_COOKIES");
    }
}

#[cfg_attr(feature = "headless", allow(dead_code))]
pub async fn remove_login_accounts(db: &Database, config: &Config) {
    if config.login_accounts.is_empty() {
        return;
    }

    for entry in &config.login_accounts {
        remove_accounts_by_platform_cookies(db, &entry.platform, &entry.cookies).await;
    }
}

#[cfg_attr(feature = "headless", allow(dead_code))]
pub async fn remove_guest_accounts(db: &Database, config: &Config) {
    // First, remove accounts that match the config's guest_accounts entries
    if !config.guest_accounts.is_empty() {
        for entry in &config.guest_accounts {
            remove_accounts_by_platform_cookies(db, &entry.platform, &entry.cookies).await;
        }
    }
    
    // Additionally, remove ALL accounts that look like guest accounts
    // This handles cases where guest cookies were auto-generated (e.g., Kuaishou's did=web_...)
    let accounts = match db.get_accounts().await {
        Ok(accounts) => accounts,
        Err(e) => {
            log::warn!("Failed to load accounts for guest cleanup: {}", e);
            return;
        }
    };
    
    for account in accounts.iter() {
        let is_guest_account = match account.platform.as_str() {
            "kuaishou" => {
                // Check if this is an auto-generated guest cookie (did=web_...)
                account.cookies.contains("did=web_") || account.uid.starts_with("cookie")
            }
            "douyin" | "tiktok" => {
                // UID starts with "cookie" usually means it's a fallback guest account
                account.uid.starts_with("cookie")
            }
            "bilibili" => {
                // Similar check for other platforms
                account.uid.starts_with("cookie")
            }
            _ => false,
        };
        
        if is_guest_account {
            log::info!("Removing guest account: {} ({})", account.platform, account.uid);
            if let Err(e) = db.remove_account(&account.platform, &account.uid).await {
                log::warn!(
                    "Failed to remove guest account for {} ({}): {}",
                    account.platform,
                    account.uid,
                    e
                );
            }
        }
    }
}

async fn remove_accounts_by_platform_cookies(db: &Database, platform: &str, cookies: &str) {
    let cookies = cookies.trim();
    if cookies.is_empty() {
        return;
    }
    let normalized_target = normalize_cookie_string_for_compare(cookies);
    let accounts = match db.get_accounts().await {
        Ok(accounts) => accounts,
        Err(e) => {
            log::warn!("Failed to load accounts for cleanup: {}", e);
            return;
        }
    };
    for account in accounts
        .iter()
        .filter(|account| account.platform == platform)
    {
        let normalized_account = normalize_cookie_string_for_compare(&account.cookies);
        if normalized_account != normalized_target {
            continue;
        }
        if let Err(e) = db.remove_account(platform, &account.uid).await {
            log::warn!(
                "Failed to remove default account for {}: {}",
                platform,
                e
            );
        }
    }
}

async fn build_account_row(
    platform: &str,
    cookies: &str,
    extra: Option<String>,
    uid_prefix: Option<&str>,
) -> Result<AccountRow, String> {
    let cookies = sanitize_cookie_header(cookies)
        .map_err(|e| format!("Invalid cookies: {e}"))?;

    let platform = PlatformType::from_str(platform).map_err(|_| "Invalid platform".to_string())?;

    let csrf = match platform {
        PlatformType::BiliBili => cookies
            .split(';')
            .map(str::trim)
            .find_map(|cookie| {
                if cookie.starts_with("bili_jct=") {
                    let var_name = &"bili_jct=";
                    Some(cookie[var_name.len()..].to_string())
                } else {
                    None
                }
            }),
        _ => Some(String::new()),
    };

    let client = crate::utils::http::no_proxy_client();
    let mut user_info = match platform {
        PlatformType::BiliBili => {
            let uid = get_item_from_cookies("DedeUserID", &cookies)
                .unwrap_or_else(|_| fallback_uid_from_cookies(&cookies));
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid,
                name: String::new(),
                avatar: String::new(),
                csrf: csrf.clone().unwrap_or_default(),
                cookies: cookies.clone(),
                extra: String::new(),
                created_at: Utc::now().to_rfc3339(),
            };
            match bilibili::api::get_user_info(&client, &tmp_account.to_account(), &tmp_account.uid)
                .await
            {
                Ok(user_info) => UserInfo {
                    user_id: user_info.user_id,
                    user_name: user_info.user_name,
                    user_avatar: user_info.user_avatar_url,
                },
                Err(e) => {
                    log::warn!(
                        "BiliBili user info unavailable, using fallback uid: {}, error: {}",
                        tmp_account.uid,
                        e
                    );
                    UserInfo {
                        user_id: tmp_account.uid.clone(),
                        user_name: "BiliBili".to_string(),
                        user_avatar: String::new(),
                    }
                }
            }
        }
        PlatformType::Douyin => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.clone(),
                extra: String::new(),
                created_at: Utc::now().to_rfc3339(),
            };

            match douyin::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => {
                    let avatar_url = user_info
                        .avatar_thumb
                        .url_list
                        .first()
                        .cloned()
                        .unwrap_or_default();

                    UserInfo {
                        user_id: user_info.sec_uid,
                        user_name: user_info.nickname,
                        user_avatar: avatar_url,
                    }
                }
                Err(e) => {
                    let uid = extract_douyin_uid(&cookies)
                        .unwrap_or_else(|| fallback_uid_from_cookies(&cookies));
                    log::warn!(
                        "Douyin user info unavailable, fallback uid from cookies: {}, error: {}",
                        uid,
                        e
                    );
                    UserInfo {
                        user_id: uid,
                        user_name: "Douyin".to_string(),
                        user_avatar: String::new(),
                    }
                }
            }
        }
        PlatformType::Huya => {
            let user_id = get_item_from_cookies("yyuid", &cookies)
                .or_else(|_| get_item_from_cookies("udb_uid", &cookies))
                .or_else(|_| get_item_from_cookies("uid", &cookies))
                .or_else(|_| get_item_from_cookies_ci("yyuid", &cookies))
                .or_else(|_| get_item_from_cookies_ci("udb_uid", &cookies))
                .or_else(|_| get_item_from_cookies_ci("uid", &cookies));
            let (user_id, has_real_uid) = match user_id {
                Ok(user_id) => (user_id, true),
                Err(_) => {
                    if let Some(uid) = fetch_huya_uid_from_cookie(&client, &cookies).await {
                        (uid, true)
                    } else {
                        let fallback = fallback_uid_from_cookies(&cookies);
                        (fallback, false)
                    }
                }
            };

            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: user_id.clone(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.clone(),
                extra: String::new(),
                created_at: Utc::now().to_rfc3339(),
            };

            if has_real_uid {
                match huya::api::get_user_info(&client, &tmp_account.to_account()).await {
                    Ok(user_info) => UserInfo {
                        user_id: user_info.user_id,
                        user_name: user_info.user_name,
                        user_avatar: user_info.user_avatar,
                    },
                    Err(e) => {
                        log::warn!(
                            "Huya user info unavailable, using fallback uid: {}, error: {}",
                            user_id,
                            e
                        );
                        UserInfo {
                            user_id: user_id.clone(),
                            user_name: "Huya".to_string(),
                            user_avatar: String::new(),
                        }
                    }
                }
            } else {
                log::warn!(
                    "Huya cookies missing yyuid, using fallback uid: {}",
                    user_id
                );
                UserInfo {
                    user_id: user_id.clone(),
                    user_name: "Huya".to_string(),
                    user_avatar: String::new(),
                }
            }
        }
        PlatformType::Kuaishou => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.clone(),
                extra: String::new(),
                created_at: Utc::now().to_rfc3339(),
            };
            match kuaishou::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => user_info,
                Err(e) => {
                    log::warn!(
                        "Kuaishou user info unavailable, using fallback uid, error: {}",
                        e
                    );
                    UserInfo {
                        user_id: fallback_uid_from_cookies(&cookies),
                        user_name: "Kuaishou".to_string(),
                        user_avatar: String::new(),
                    }
                }
            }
        }
        PlatformType::Xiaohongshu => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.clone(),
                extra: String::new(),
                created_at: Utc::now().to_rfc3339(),
            };
            match xiaohongshu::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => user_info,
                Err(e) => {
                    log::warn!(
                        "Xiaohongshu user info unavailable, using fallback uid, error: {}",
                        e
                    );
                    UserInfo {
                        user_id: fallback_uid_from_cookies(&cookies),
                        user_name: "Xiaohongshu".to_string(),
                        user_avatar: String::new(),
                    }
                }
            }
        }
        PlatformType::TikTok => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.clone(),
                extra: String::new(),
                created_at: Utc::now().to_rfc3339(),
            };
            match tiktok::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => user_info,
                Err(e) => {
                    if let Some(uid) = extract_tiktok_uid(&cookies) {
                        UserInfo {
                            user_id: uid,
                            user_name: "TikTok".to_string(),
                            user_avatar: String::new(),
                        }
                    } else {
                        log::warn!(
                            "TikTok user info unavailable, fallback uid from cookies: {}",
                            e
                        );
                        UserInfo {
                            user_id: fallback_uid_from_cookies(&cookies),
                            user_name: "TikTok".to_string(),
                            user_avatar: String::new(),
                        }
                    }
                }
            }
        }
        PlatformType::Weibo => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.clone(),
                extra: String::new(),
                created_at: Utc::now().to_rfc3339(),
            };
            match weibo::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => user_info,
                Err(e) => {
                    log::warn!(
                        "Weibo user info unavailable, using fallback uid, error: {}",
                        e
                    );
                    UserInfo {
                        user_id: fallback_uid_from_cookies(&cookies),
                        user_name: "Weibo".to_string(),
                        user_avatar: String::new(),
                    }
                }
            }
        }
        PlatformType::Youtube => {
            return Err("Unsupported platform".to_string());
        }
    };

    let extra_json = extra.unwrap_or_else(|| {
        build_account_extra_json(cookie_list_from_header(&cookies), &user_info)
    });

    let mut uid = user_info.user_id;
    if let Some(prefix) = uid_prefix {
        uid = format!("{}:{}", prefix, uid);
        if prefix == "guest" {
            user_info.user_name = match platform {
                PlatformType::BiliBili => "Bilibili",
                PlatformType::Douyin => "Douyin",
                PlatformType::Huya => "Huya",
                PlatformType::Kuaishou => "Kuaishou",
                PlatformType::Xiaohongshu => "Xiaohongshu",
                PlatformType::TikTok => "TikTok",
                PlatformType::Weibo => "Weibo",
                PlatformType::Youtube => "Youtube",
            }.to_string();
            user_info.user_avatar = String::new();
        }
    }

    Ok(AccountRow {
        platform: platform.as_str().to_string(),
        uid,
        name: user_info.user_name,
        avatar: user_info.user_avatar,
        csrf: csrf.unwrap_or_default(),
        cookies: cookies.into(),
        extra: extra_json,
        created_at: Utc::now().to_rfc3339(),
    })
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn remove_account(
    state: state_type!(),
    platform: String,
    uid: String,
) -> Result<(), String> {
    if platform == "bilibili" {
        let account = state.db.get_account(&platform, &uid).await?;
        let client = crate::utils::http::no_proxy_client();
        let _ = bilibili::api::logout(&client, &account.to_account()).await;
    }
    Ok(state.db.remove_account(&platform, &uid).await?)
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn get_account_count(state: state_type!()) -> Result<u64, String> {
    Ok(state.db.get_accounts().await?.len() as u64)
}

#[cfg_attr(feature = "headless", allow(dead_code))]
fn domain_for_platform(platform: &str) -> Option<&'static str> {
    match platform {
        "bilibili" => Some("bilibili.com"),
        "douyin" => Some("douyin.com"),
        "huya" => Some("huya.com"),
        "kuaishou" => Some("kuaishou.com"),
        "xiaohongshu" => Some("xiaohongshu.com"),
        "tiktok" => Some("tiktok.com"),
        "weibo" => Some("weibo.com"),
        "youtube" => Some("youtube.com"),
        _ => None,
    }
}

#[cfg_attr(feature = "headless", allow(dead_code))]
fn collect_browser_cookies_for_platform(platform: &str) -> Option<String> {
    let mut domains = vec![domain_for_platform(platform)?];
    if platform == "huya" {
        // Huya login cookies often live on yy.com (udb.yy.com / passport.yy.com).
        domains.push("yy.com");
    }
    let mut cookie_sets = Vec::new();
    for domain in domains {
        if let Some(collector) = BrowserCookieCollector::new_chrome() {
            if let Ok(cookies) = collector.get_cookies_as_string(domain) {
                if !cookies.is_empty() {
                    cookie_sets.push(cookies);
                }
            }
        }
        if let Some(collector) = BrowserCookieCollector::new_edge() {
            if let Ok(cookies) = collector.get_cookies_as_string(domain) {
                if !cookies.is_empty() {
                    cookie_sets.push(cookies);
                }
            }
        }
    }

    if cookie_sets.is_empty() {
        None
    } else {
        Some(cookie_sets.join("; "))
    }
}

#[cfg_attr(feature = "gui", tauri::command)]
#[cfg_attr(feature = "headless", allow(dead_code))]
pub async fn get_browser_cookies(
    _state: state_type!(),
    platform: String,
) -> Result<String, String> {
    match collect_browser_cookies_for_platform(&platform) {
        Some(cookies) => Ok(cookies),
        None => Err("No browser cookies found".to_string()),
    }
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn open_tiktok_login_window(
    state: state_type!(),
    user_agent: Option<String>,
) -> Result<(), String> {
    let label = "tiktok-login";
    if let Some(window) = state.app_handle.get_webview_window(label) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let url = Url::parse("https://www.tiktok.com/login")
        .map_err(|e| format!("Invalid login URL: {e}"))?;
    let mut builder = WebviewWindowBuilder::new(
        &state.app_handle,
        label,
        WebviewUrl::External(url),
    )
    .title("TikTok 登录")
    .inner_size(1100.0, 800.0);

    let fallback_ua = std::env::var("TIKTOK_WEBVIEW_USER_AGENT")
        .or_else(|_| std::env::var("DOUYIN_PASSPORT_USER_AGENT"))
        .unwrap_or_default();
    let ua = user_agent
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let trimmed = fallback_ua.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
    if let Some(ua) = ua {
        builder = builder.user_agent(ua);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to open login window: {e}"))?;
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn get_tiktok_webview_cookies(state: state_type!()) -> Result<WebviewCookieResult, String> {
    let label = "tiktok-login";
    let window = state
        .app_handle
        .get_webview_window(label)
        .ok_or_else(|| "未找到内置浏览器窗口，请先打开登录窗口".to_string())?;
    let mut urls: Vec<String> = vec![
        "https://www.tiktok.com",
        "https://www.tiktok.com/passport/",
        "https://web-va.tiktok.com",
    ]
    .into_iter()
    .map(|url| url.to_string())
    .collect();
    if let Ok(extra) = std::env::var("TIKTOK_WEBVIEW_COOKIE_URLS") {
        for raw in extra.split(',') {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                urls.push(trimmed.to_string());
            }
        }
    }
    let mut cookie_map: HashMap<String, String> = HashMap::new();
    let mut cookie_map_lower: HashMap<String, String> = HashMap::new();

    for raw in urls {
        let url = Url::parse(&raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            let name = cookie.name().to_string();
            let lower = name.to_ascii_lowercase();
            cookie_map.insert(name, value.to_string());
            cookie_map_lower.insert(lower, value.to_string());
        }
    }

    let cookie_list = cookie_map
        .iter()
        .map(|(name, value)| CookieItem {
            name: name.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    // Filter out device IDs for guest mode
    remove_cookie_keys_ci(
        &mut cookie_map,
        &["buvid3", "buvid4", "buvid_fp", "buvid_fp_plain", "_uuid", "b_lsid"],
    );

    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");

    if cookie_str.is_empty() {
        return Err("未读取到 Cookie，请先在内置浏览器完成登录".to_string());
    }

    let has_login = cookie_map.contains_key("sessionid")
        || cookie_map.contains_key("sessionid_ss")
        || cookie_map_lower.contains_key("sessionid")
        || cookie_map_lower.contains_key("sessionid_ss");
    if !has_login {
        log::warn!(
            "[Account] TikTok webview cookies missing core login tokens (sessionid)."
        );
        return Err("未检测到 TikTok 登录状态。请在内置浏览器完成登录（出现个人头像）后再导入。".to_string());
    }

    let required_keys = [
        "sessionid",
        "sessionid_ss",
        "sid_tt",
        "sid_guard",
        "uid_tt",
        "uid_tt_ss",
        "ttwid",
        "msToken",
        "passport_csrf_token",
        "tt_csrf_token",
        "s_v_web_id",
        "tt-target-idc",
        "store-idc",
        "store-country-code",
        "_ttp",
        "_waftokenid",
    ];
    let mut missing = Vec::new();
    for key in required_keys {
        if !(cookie_map.contains_key(key) || cookie_map_lower.contains_key(&key.to_ascii_lowercase()))
        {
            missing.push(key);
        }
    }
    if !missing.is_empty() {
        log::warn!(
            "[Account] TikTok webview cookies missing keys: {}",
            missing.join(", ")
        );
    }

    std::env::set_var("TIKTOK_WEBVIEW_COOKIES", &cookie_str);
    std::env::set_var("TIKTOK_WEBVIEW_REFRESH_NEEDED", "0");
    {
        let config = state.config.write().await;
        config.save();
    }
    return build_webview_cookie_result("tiktok", cookie_str, cookie_list).await;
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn open_douyin_login_window(
    state: state_type!(),
    user_agent: Option<String>,
) -> Result<(), String> {
    let label = "douyin-login";
    if let Some(window) = state.app_handle.get_webview_window(label) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let login_url = std::env::var("DOUYIN_LOGIN_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://live.douyin.com".to_string());
    let url = Url::parse(&login_url)
        .map_err(|e| format!("Invalid login URL: {e}"))?;
    let mut builder = WebviewWindowBuilder::new(
        &state.app_handle,
        label,
        WebviewUrl::External(url),
    )
    .title("抖音 登录")
    .inner_size(1100.0, 800.0);

    let fallback_ua = std::env::var("DOUYIN_PASSPORT_USER_AGENT")
        .or_else(|_| std::env::var("DOUYIN_USER_AGENT"))
        .unwrap_or_default();
    let ua = user_agent
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let trimmed = fallback_ua.trim();
            if trimmed.is_empty() {
                Some(default_douyin_webview_ua())
            } else {
                Some(trimmed)
            }
        });
    if let Some(ua) = ua {
        builder = builder.user_agent(ua);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to open login window: {e}"))?;
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn get_douyin_webview_cookies(state: state_type!()) -> Result<WebviewCookieResult, String> {
    let label = "douyin-login";
    let window = state
        .app_handle
        .get_webview_window(label)
        .ok_or_else(|| "未找到内置浏览器窗口，请先打开登录窗口".to_string())?;
    let urls = [
        "https://douyin.com",
        "https://www.douyin.com",
        "https://m.douyin.com",
        "http://douyin.com",
        "http://www.douyin.com",
        "http://m.douyin.com",
        "https://live.douyin.com",
        "https://www.iesdouyin.com",
        "https://sso.douyin.com",
        "https://www.douyin.com/passport/",
        "https://www.douyin.com/passport/web/account/info/",
        "https://www.douyin.com/user/",
    ];
    let mut cookie_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut cookie_map_lower: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for raw in urls {
        let url = Url::parse(raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            let name = cookie.name().to_string();
            let lower = name.to_ascii_lowercase();
            cookie_map.insert(name, value.to_string());
            cookie_map_lower.insert(lower, value.to_string());
        }
    }

    for (name, value) in cookie_map_lower.iter() {
        if !cookie_map.contains_key(name) {
            cookie_map.insert(name.clone(), value.clone());
        }
    }

    if !cookie_map.contains_key("sessionid") && !cookie_map.contains_key("sessionid_ss") {
        if let Some(value) = cookie_map_lower
            .get("sessionid")
            .or_else(|| cookie_map_lower.get("sessionid_ss"))
            .cloned()
        {
            cookie_map.insert("sessionid".to_string(), value);
        }
    }

    let cookie_list = cookie_map
        .iter()
        .map(|(name, value)| CookieItem {
            name: name.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");

    if cookie_str.is_empty() {
        return Err("未读取到 Cookie，请先在内置浏览器完成登录".to_string());
    }

    let has_login = cookie_map.contains_key("sessionid")
        || cookie_map.contains_key("sessionid_ss")
        || cookie_map_lower.contains_key("sessionid")
        || cookie_map_lower.contains_key("sessionid_ss");
    if !has_login {
        log::warn!(
            "[Account] Douyin webview cookies missing core login tokens (sessionid)."
        );
        return Err("未检测到抖音登录状态。请在内置浏览器完成登录后再导入。".to_string());
    }

    return build_webview_cookie_result("douyin", cookie_str, cookie_list).await;
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn open_kuaishou_login_window(
    state: state_type!(),
    user_agent: Option<String>,
) -> Result<(), String> {
    let label = "kuaishou-login";
    if let Some(window) = state.app_handle.get_webview_window(label) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let url = Url::parse("https://live.kuaishou.com")
        .map_err(|e| format!("Invalid login URL: {e}"))?;
    let mut builder = WebviewWindowBuilder::new(
        &state.app_handle,
        label,
        WebviewUrl::External(url),
    )
    .title("快手 登录")
    .inner_size(1100.0, 800.0);

    let fallback_ua = std::env::var("KUAISHOU_WEBVIEW_USER_AGENT")
        .or_else(|_| std::env::var("KUAISHOU_USER_AGENT"))
        .unwrap_or_default();
    let ua = user_agent
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let trimmed = fallback_ua.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
    if let Some(ua) = ua {
        builder = builder.user_agent(ua);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to open login window: {e}"))?;
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn get_kuaishou_webview_cookies(state: state_type!()) -> Result<WebviewCookieResult, String> {
    let label = "kuaishou-login";
    let window = state
        .app_handle
        .get_webview_window(label)
        .ok_or_else(|| "未找到内置浏览器窗口，请先打开登录窗口".to_string())?;
    let urls = [
        "https://live.kuaishou.com",
        "https://live.kuaishou.com/",
        "https://live.kuaishou.com/live",
        "https://live.kuaishou.com/u/1",
        "https://www.kuaishou.com",
        "https://www.kuaishou.com/",
        "https://www.kuaishou.com/live",
        "https://www.kuaishou.com/u/1",
        "https://www.kuaishou.com/profile/1",
        "https://m.kuaishou.com",
        "https://m.kuaishou.com/",
        "https://id.kuaishou.com",
        "https://id.kuaishou.com/",
        "https://s.kuaishou.com",
        "https://c.kuaishou.com",
        "https://v.kuaishou.com",
    ];
    let mut cookie_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut cookie_map_lower: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for raw in urls {
        let url = Url::parse(raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            let name = cookie.name().to_string();
            let lower = name.to_ascii_lowercase();
            cookie_map.insert(name, value.to_string());
            cookie_map_lower.insert(lower, value.to_string());
        }
    }
    for (name, value) in cookie_map_lower.iter() {
        if !cookie_map.contains_key(name) {
            cookie_map.insert(name.clone(), value.clone());
        }
    }

    ensure_kuaishou_base_cookies(&mut cookie_map);

    let cookie_list = cookie_map
        .iter()
        .map(|(name, value)| CookieItem {
            name: name.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");

    if cookie_str.is_empty() {
        return Err("未读取到 Cookie，请先在内置浏览器完成登录".to_string());
    }
    let has_login = cookie_map.contains_key("userId")
        || cookie_map.contains_key("kuaishou.live.web_st")
        || cookie_map.contains_key("kuaishou.live.web_ph")
        || cookie_map.contains_key("kwssectoken")
        || cookie_map_lower.contains_key("userid")
        || cookie_map_lower.contains_key("kuaishou.live.web_st")
        || cookie_map_lower.contains_key("kuaishou.live.web_ph")
        || cookie_map_lower.contains_key("kwssectoken");
    if !has_login {
        log::warn!("[Account] Kuaishou webview cookies missing login tokens.");
    }
    let required_keys = [
        "userId",
        "kuaishou.live.web_st",
        "kuaishou.live.web_ph",
        "kwssectoken",
        "clientid",
        "client_key",
        "kpn",
        "did",
        "didv",
        "kwpsecproductname",
        "kwfv1",
        "kwscode",
    ];
    let mut missing = Vec::new();
    for key in required_keys {
        if !cookie_map.contains_key(key) && !cookie_map_lower.contains_key(&key.to_ascii_lowercase()) {
            missing.push(key);
        }
    }
    if !missing.is_empty() {
        log::warn!(
            "[Account] Kuaishou webview cookies missing keys: {}",
            missing.join(",")
        );
    }

    return build_webview_cookie_result("kuaishou", cookie_str, cookie_list).await;
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn open_huya_login_window(
    state: state_type!(),
    user_agent: Option<String>,
) -> Result<(), String> {
    let label = "huya-login";
    if let Some(window) = state.app_handle.get_webview_window(label) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let url = Url::parse("https://www.huya.com")
        .map_err(|e| format!("Invalid login URL: {e}"))?;
    let mut builder = WebviewWindowBuilder::new(
        &state.app_handle,
        label,
        WebviewUrl::External(url),
    )
    .title("虎牙 登录")
    .inner_size(1100.0, 800.0);

    let fallback_ua = std::env::var("HUYA_WEBVIEW_USER_AGENT")
        .or_else(|_| std::env::var("HUYA_USER_AGENT"))
        .unwrap_or_default();
    let ua = user_agent
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let trimmed = fallback_ua.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
    if let Some(ua) = ua {
        builder = builder.user_agent(ua);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to open login window: {e}"))?;
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn get_huya_webview_cookies(state: state_type!()) -> Result<WebviewCookieResult, String> {
    let label = "huya-login";
    let window = state
        .app_handle
        .get_webview_window(label)
        .ok_or_else(|| "未找到内置浏览器窗口，请先打开登录窗口".to_string())?;
    let urls = [
        "http://www.huya.com/",
        "http://i.huya.com/",
        "https://www.huya.com/",
        "https://www.huya.com/index.html",
        "https://www.huya.com/g",
        "https://www.huya.com/0",
        "https://www.huya.com/u/",
        "https://i.huya.com/",
        "https://i.huya.com/setting",
        "https://m.huya.com/",
        "https://m.huya.com/room/1",
        "https://udblgn.huya.com/",
        "https://udb.yy.com/",
        "https://passport.yy.com/",
        "https://www.yy.com/",
    ];
    let mut cookie_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut cookie_map_lower: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for raw in urls {
        let url = Url::parse(raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            let name = cookie.name().to_string();
            let lower = name.to_ascii_lowercase();
            cookie_map.insert(name, value.to_string());
            cookie_map_lower.insert(lower, value.to_string());
        }
    }

    if !cookie_map.contains_key("yyuid") {
        if let Some(value) = cookie_map_lower
            .get("yyuid")
            .or_else(|| cookie_map_lower.get("yyuid_u"))
            .or_else(|| cookie_map_lower.get("udb_uid"))
            .or_else(|| cookie_map_lower.get("uid"))
            .cloned()
        {
            cookie_map.insert("yyuid".to_string(), value);
        }
    }

    let cookie_list = cookie_map
        .iter()
        .map(|(name, value)| CookieItem {
            name: name.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");

    if cookie_str.is_empty() {
        return Err("未读取到完整 Cookie，请在内置浏览器完成登录并访问 www.huya.com / i.huya.com".to_string());
    }
    if !cookie_map.contains_key("yyuid") {
        log::warn!("[Account] Huya webview cookies missing yyuid.");
    }

    return build_webview_cookie_result("huya", cookie_str, cookie_list).await;
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn open_bilibili_login_window(
    state: state_type!(),
    user_agent: Option<String>,
) -> Result<(), String> {
    let label = "bilibili-login";
    if let Some(window) = state.app_handle.get_webview_window(label) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let url = Url::parse("https://www.bilibili.com/")
        .map_err(|e| format!("Invalid login URL: {e}"))?;
    let mut builder = WebviewWindowBuilder::new(
        &state.app_handle,
        label,
        WebviewUrl::External(url),
    )
    .title("B站 登录")
    .inner_size(1100.0, 800.0);

    let fallback_ua = std::env::var("BILIBILI_WEBVIEW_USER_AGENT")
        .or_else(|_| std::env::var("BILIBILI_USER_AGENT"))
        .unwrap_or_default();
    let ua = user_agent
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let trimmed = fallback_ua.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
    if let Some(ua) = ua {
        builder = builder.user_agent(ua);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to open login window: {e}"))?;
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn get_bilibili_webview_cookies(state: state_type!()) -> Result<WebviewCookieResult, String> {
    let label = "bilibili-login";
    let window = state
        .app_handle
        .get_webview_window(label)
        .ok_or_else(|| "未找到内置浏览器窗口，请先打开登录窗口".to_string())?;
    let urls = [
        "http://www.bilibili.com",
        "http://passport.bilibili.com",
        "https://www.bilibili.com",
        "https://passport.bilibili.com",
        "https://passport.bilibili.com/login",
        "https://passport.bilibili.com/web/",
        "https://api.bilibili.com",
        "https://space.bilibili.com",
        "https://live.bilibili.com",
        "https://m.bilibili.com",
        "https://t.bilibili.com",
        "https://account.bilibili.com",
    ];
    let mut cookie_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for raw in urls {
        let url = Url::parse(raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            cookie_map.insert(cookie.name().to_string(), value.to_string());
        }
    }

    let cookie_list = cookie_map
        .iter()
        .map(|(name, value)| CookieItem {
            name: name.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");

    if cookie_str.is_empty() {
        return Err("未读取到 Cookie，请先在内置浏览器完成登录".to_string());
    }

    let has_uid = cookie_map.contains_key("DedeUserID");
    let has_csrf = cookie_map.contains_key("bili_jct");
    if !has_uid || !has_csrf {
        log::warn!("[Account] BiliBili webview cookies missing DedeUserID/bili_jct.");
    }

    return build_webview_cookie_result("bilibili", cookie_str, cookie_list).await;
}





#[cfg(feature = "gui")]
#[allow(dead_code)]
async fn get_tiktok_webview_guest_cookies(
    state: &crate::state::State,
    label: &str,
) -> Result<String, String> {
    let mut created = false;
    if state.app_handle.get_webview_window(label).is_none() {
        let url = Url::parse("https://live.tiktok.com/")
            .map_err(|e| format!("Invalid guest URL: {e}"))?;
        let mut builder = WebviewWindowBuilder::new(
            &state.app_handle,
            label,
            WebviewUrl::External(url),
        )
        .title("TikTok Guest")
        .inner_size(900.0, 700.0)
        .visible(false)
        .skip_taskbar(true)
        .initialization_script(r#"
            (function() {
                // Mute audio context if possible
                if (window.AudioContext || window.webkitAudioContext) {
                    try {
                        const AudioContext = window.AudioContext || window.webkitAudioContext;
                        const ctx = new AudioContext();
                        ctx.suspend();
                    } catch (e) {}
                }
                
                // Mute all media elements periodically
                const muteAll = () => {
                    document.querySelectorAll('video, audio').forEach(el => {
                        el.muted = true;
                        el.pause();
                        el.volume = 0;
                    });
                };
                
                setInterval(muteAll, 500);
                document.addEventListener('DOMContentLoaded', muteAll);
                window.addEventListener('load', muteAll);
                
                // Override media play
                const originalPlay = HTMLMediaElement.prototype.play;
                HTMLMediaElement.prototype.play = function() {
                    this.muted = true;
                    return Promise.resolve(); // Fake success or originalPlay.apply(this, arguments);
                };
            })();
        "#);

        let fallback_ua = std::env::var("TIKTOK_WEBVIEW_USER_AGENT")
            .or_else(|_| std::env::var("TIKTOK_USER_AGENT"))
            .unwrap_or_default();
        let ua = fallback_ua.trim();
        if !ua.is_empty() {
            builder = builder.user_agent(ua);
        }

        builder
            .build()
            .map_err(|e| format!("Failed to open guest window: {e}"))?;
        created = true;
    }

    let window = state
        .app_handle
        .get_webview_window(label)
        .ok_or_else(|| "未找到内置浏览器窗口，请先打开 TikTok 页面".to_string())?;
    if created {
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    let mut urls: Vec<String> = vec!["https://live.tiktok.com/", "https://www.tiktok.com/"]
        .into_iter()
        .map(|url| url.to_string())
        .collect();
    if let Ok(extra) = std::env::var("TIKTOK_WEBVIEW_COOKIE_URLS") {
        for raw in extra.split(',') {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                urls.push(trimmed.to_string());
            }
        }
    }

    let mut cookie_map: HashMap<String, String> = HashMap::new();
    let mut cookie_map_lower: HashMap<String, String> = HashMap::new();
    for raw in urls {
        let url = Url::parse(&raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            let name = cookie.name().to_string();
            let lower = name.to_ascii_lowercase();
            cookie_map.insert(name, value.to_string());
            cookie_map_lower.insert(lower, value.to_string());
        }
    }
    for (name, value) in cookie_map_lower.iter() {
        if !cookie_map.contains_key(name) {
            cookie_map.insert(name.clone(), value.clone());
        }
    }

    // Filter out login tokens and device IDs for guest mode
    remove_cookie_keys_ci(
        &mut cookie_map,
        &[
            "sessionid",
            "sessionid_ss",
            "sid_tt",
            "sid_guard",
            "uid_tt",
            "uid_tt_ss",
            "s_v_web_id",
            "ttwid",
            "tt_webid",
            "tt_webid_v2",
            "device_id",
        ],
    );

    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    if cookie_str.is_empty() {
        if created {
            let _ = window.close();
        }
        return Err("未读取到 Cookie，请先在内置浏览器访问 TikTok 页面".to_string());
    }
    if cookie_map.is_empty() && !cookie_map_lower.is_empty() {
        log::warn!("[Account] TikTok guest cookies only available in lowercase map.");
    }
    if created {
        let _ = window.close();
    }
    return Ok(cookie_str);
}

#[cfg(feature = "gui")]
#[allow(dead_code)]
async fn get_douyin_webview_guest_cookies(
    state: &crate::state::State,
    label: &str,
) -> Result<String, String> {
    let mut created = false;
    if state.app_handle.get_webview_window(label).is_none() {
        let url = Url::parse("https://live.douyin.com/")
            .map_err(|e| format!("Invalid guest URL: {e}"))?;
        let mut builder = WebviewWindowBuilder::new(
            &state.app_handle,
            label,
            WebviewUrl::External(url),
        )
        .title("Douyin Guest")
        .inner_size(900.0, 700.0)
        .visible(false)
        .skip_taskbar(true)
        .initialization_script(r#"
            (function() {
                // Mute audio context if possible
                if (window.AudioContext || window.webkitAudioContext) {
                    try {
                        const AudioContext = window.AudioContext || window.webkitAudioContext;
                        const ctx = new AudioContext();
                        ctx.suspend();
                    } catch (e) {}
                }
                
                // Mute all media elements periodically
                const muteAll = () => {
                    document.querySelectorAll('video, audio').forEach(el => {
                        el.muted = true;
                        el.pause();
                        el.volume = 0;
                    });
                };
                
                setInterval(muteAll, 500);
                document.addEventListener('DOMContentLoaded', muteAll);
                window.addEventListener('load', muteAll);
                
                // Override media play
                const originalPlay = HTMLMediaElement.prototype.play;
                HTMLMediaElement.prototype.play = function() {
                    this.muted = true;
                    return Promise.resolve(); // Fake success or originalPlay.apply(this, arguments);
                };
            })();
        "#);

        let fallback_ua = std::env::var("DOUYIN_WEBVIEW_USER_AGENT")
            .or_else(|_| std::env::var("DOUYIN_PASSPORT_USER_AGENT"))
            .unwrap_or_default();
        let ua = fallback_ua.trim();
        if !ua.is_empty() {
            builder = builder.user_agent(ua);
        }

        builder
            .build()
            .map_err(|e| format!("Failed to open guest window: {e}"))?;
        created = true;
    }

    let window = state
        .app_handle
        .get_webview_window(label)
        .ok_or_else(|| "未找到内置浏览器窗口，请先打开抖音页面".to_string())?;
    if created {
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    let mut urls: Vec<String> = vec!["https://live.douyin.com/"]
        .into_iter()
        .map(|url| url.to_string())
        .collect();
    if let Ok(extra) = std::env::var("DOUYIN_WEBVIEW_COOKIE_URLS") {
        for raw in extra.split(',') {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                urls.push(trimmed.to_string());
            }
        }
    }

    let mut cookie_map: HashMap<String, String> = HashMap::new();
    let mut cookie_map_lower: HashMap<String, String> = HashMap::new();
    for raw in urls {
        let url = Url::parse(&raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            let name = cookie.name().to_string();
            let lower = name.to_ascii_lowercase();
            cookie_map.insert(name, value.to_string());
            cookie_map_lower.insert(lower, value.to_string());
        }
    }
    for (name, value) in cookie_map_lower.iter() {
        if !cookie_map.contains_key(name) {
            cookie_map.insert(name.clone(), value.clone());
        }
    }
    if !cookie_map.contains_key("sessionid") {
        if let Some(value) = cookie_map_lower
            .get("sessionid")
            .or_else(|| cookie_map_lower.get("sessionid_ss"))
        {
            cookie_map.insert("sessionid".to_string(), value.clone());
        }
    }

    // Filter out login tokens and device IDs for guest mode
    remove_cookie_keys_ci(
        &mut cookie_map,
        &[
            "sessionid",
            "sessionid_ss",
            "sid_tt",
            "sid_guard",
            "uid_tt",
            "uid_tt_ss",
            "s_v_web_id",
            "ttwid",
            "tt_webid",
            "tt_webid_v2",
            "device_id",
        ],
    );

    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    if cookie_str.is_empty() {
        if created {
            let _ = window.close();
        }
        return Err("未读取到 Cookie，请先在内置浏览器访问抖音页面".to_string());
    }
    if created {
        let _ = window.close();
    }
    return Ok(cookie_str);
}



#[cfg(feature = "gui")]
#[allow(dead_code)]
async fn get_huya_webview_guest_cookies(
    state: &crate::state::State,
    label: &str,
) -> Result<String, String> {
    let mut created = false;
    if state.app_handle.get_webview_window(label).is_none() {
        let url = Url::parse("https://www.huya.com/")
            .map_err(|e| format!("Invalid guest URL: {e}"))?;
        let mut builder = WebviewWindowBuilder::new(
            &state.app_handle,
            label,
            WebviewUrl::External(url),
        )
        .title("Huya Guest")
        .inner_size(900.0, 700.0)
        .visible(false)
        .skip_taskbar(true)
        .initialization_script(r#"
            (function() {
                // Mute audio context if possible
                if (window.AudioContext || window.webkitAudioContext) {
                    try {
                        const AudioContext = window.AudioContext || window.webkitAudioContext;
                        const ctx = new AudioContext();
                        ctx.suspend();
                    } catch (e) {}
                }
                
                // Mute all media elements periodically
                const muteAll = () => {
                    document.querySelectorAll('video, audio').forEach(el => {
                        el.muted = true;
                        el.pause();
                        el.volume = 0;
                    });
                };
                
                setInterval(muteAll, 500);
                document.addEventListener('DOMContentLoaded', muteAll);
                window.addEventListener('load', muteAll);
                
                // Override media play
                const originalPlay = HTMLMediaElement.prototype.play;
                HTMLMediaElement.prototype.play = function() {
                    this.muted = true;
                    return Promise.resolve(); // Fake success or originalPlay.apply(this, arguments);
                };
            })();
        "#);

        let fallback_ua = std::env::var("HUYA_WEBVIEW_USER_AGENT")
            .or_else(|_| std::env::var("HUYA_USER_AGENT"))
            .unwrap_or_default();
        let ua = fallback_ua.trim();
        if !ua.is_empty() {
            builder = builder.user_agent(ua);
        }

        if let Err(e) = builder.build() {
            return Err(format!("Failed to open guest window: {e}"));
        }
        created = true;
    }

    let window = state.app_handle.get_webview_window(label);
    if window.is_none() {
         return Err("未找到内置浏览器窗口".to_string());
    }
    let window = window.unwrap();
    if created {
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    let mut urls: Vec<String> = vec!["https://www.huya.com/"]
        .into_iter()
        .map(|url| url.to_string())
        .collect();
    if let Ok(extra) = std::env::var("HUYA_WEBVIEW_COOKIE_URLS") {
        for raw in extra.split(',') {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                urls.push(trimmed.to_string());
            }
        }
    }

    let mut cookie_map: HashMap<String, String> = HashMap::new();
    let mut cookie_map_lower: HashMap<String, String> = HashMap::new();
    for raw in urls {
        let url = Url::parse(&raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            let name = cookie.name().to_string();
            let lower = name.to_ascii_lowercase();
            cookie_map.insert(name, value.to_string());
            cookie_map_lower.insert(lower, value.to_string());
        }
    }
    for (name, value) in cookie_map_lower.iter() {
        if !cookie_map.contains_key(name) {
            cookie_map.insert(name.clone(), value.clone());
        }
    }

    // Filter out device IDs for guest mode
    remove_cookie_keys_ci(&mut cookie_map, &["guid"]);

    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    if cookie_str.is_empty() {
        if created {
            let _ = window.close();
        }
        return Err("未读取到 Cookie，请先在内置浏览器访问虎牙页面".to_string());
    }

    if created {
        let _ = window.close();
    }

    return Ok(cookie_str);
}

#[cfg(feature = "gui")]
#[allow(dead_code)]
async fn get_bilibili_webview_guest_cookies(
    state: &crate::state::State,
    label: &str,
) -> Result<String, String> {
    let mut created = false;
    if state.app_handle.get_webview_window(label).is_none() {
        let url = Url::parse("https://live.bilibili.com/")
            .map_err(|e| format!("Invalid guest URL: {e}"))?;
        let mut builder = WebviewWindowBuilder::new(
            &state.app_handle,
            label,
            WebviewUrl::External(url),
        )
        .title("Bilibili Guest")
        .inner_size(900.0, 700.0)
        .visible(false)
        .skip_taskbar(true)
        .initialization_script(r#"
            (function() {
                // Mute audio context if possible
                if (window.AudioContext || window.webkitAudioContext) {
                    try {
                        const AudioContext = window.AudioContext || window.webkitAudioContext;
                        const ctx = new AudioContext();
                        ctx.suspend();
                    } catch (e) {}
                }
                
                // Mute all media elements periodically
                const muteAll = () => {
                    document.querySelectorAll('video, audio').forEach(el => {
                        el.muted = true;
                        el.pause();
                        el.volume = 0;
                    });
                };
                
                setInterval(muteAll, 500);
                document.addEventListener('DOMContentLoaded', muteAll);
                window.addEventListener('load', muteAll);
                
                // Override media play
                const originalPlay = HTMLMediaElement.prototype.play;
                HTMLMediaElement.prototype.play = function() {
                    this.muted = true;
                    return Promise.resolve(); // Fake success or originalPlay.apply(this, arguments);
                };
            })();
        "#);

        let fallback_ua = std::env::var("BILIBILI_WEBVIEW_USER_AGENT")
            .or_else(|_| std::env::var("BILIBILI_USER_AGENT"))
            .unwrap_or_default();
        let ua = fallback_ua.trim();
        if !ua.is_empty() {
            builder = builder.user_agent(ua);
        }

        if let Err(e) = builder.build() {
            return Err(format!("Failed to open guest window: {e}"));
        }
        created = true;
    }

    let window = state.app_handle.get_webview_window(label);
    if window.is_none() {
         return Err("未找到内置浏览器窗口".to_string());
    }
    let window = window.unwrap();
    if created {
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    let mut urls: Vec<String> = vec!["https://live.bilibili.com/"]
        .into_iter()
        .map(|url| url.to_string())
        .collect();
    if let Ok(extra) = std::env::var("BILIBILI_WEBVIEW_COOKIE_URLS") {
        for raw in extra.split(',') {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                urls.push(trimmed.to_string());
            }
        }
    }

    let mut cookie_map: HashMap<String, String> = HashMap::new();
    for raw in urls {
        let url = Url::parse(&raw).map_err(|e| format!("Invalid cookie URL: {e}"))?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|e| format!("读取 Cookie 失败: {e}"))?;
        for cookie in cookies {
            let value = cookie.value();
            if value.is_empty() {
                continue;
            }
            cookie_map.insert(cookie.name().to_string(), value.to_string());
        }
    }

    let cookie_str = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    if cookie_str.is_empty() {
        if created {
            let _ = window.close();
        }
        return Err("未读取到 Cookie，请先在内置浏览器访问 BiliBili 页面".to_string());
    }
    if created {
        let _ = window.close();
    }
    return Ok(cookie_str);
}

#[cfg(feature = "gui")]
#[allow(dead_code)]
async fn get_kuaishou_webview_guest_cookies(
    state: &crate::state::State,
    label: &str,
) -> Result<String, String> {
    let mut created = false;
    if state.app_handle.get_webview_window(label).is_none() {
        let url = Url::parse("https://live.kuaishou.com/")
            .map_err(|e| format!("Invalid guest URL: {e}"))?;
        let mut builder = WebviewWindowBuilder::new(
            &state.app_handle,
            label,
            WebviewUrl::External(url),
        )
        .title("Kuaishou Guest")
        .inner_size(900.0, 700.0)
        .visible(false)
        .skip_taskbar(true)
        .initialization_script(r#"
            (function() {
                // Mute audio context if possible
                if (window.AudioContext || window.webkitAudioContext) {
                    try {
                        const AudioContext = window.AudioContext || window.webkitAudioContext;
                        const ctx = new AudioContext();
                        ctx.suspend();
                    } catch (e) {}
                }
                
                // Mute all media elements periodically
                const muteAll = () => {
                    document.querySelectorAll('video, audio').forEach(el => {
                        el.muted = true;
                        el.pause();
                        el.volume = 0;
                    });
                };
                
                setInterval(muteAll, 500);
                document.addEventListener('DOMContentLoaded', muteAll);
                window.addEventListener('load', muteAll);
                
                // Override media play
                const originalPlay = HTMLMediaElement.prototype.play;
                HTMLMediaElement.prototype.play = function() {
                    this.muted = true;
                    return Promise.resolve(); // Fake success or originalPlay.apply(this, arguments);
                };
            })();
        "#);

        let fallback_ua = std::env::var("KUAISHOU_WEBVIEW_USER_AGENT")
            .or_else(|_| std::env::var("KUAISHOU_USER_AGENT"))
            .unwrap_or_default();
        let ua = fallback_ua.trim();
        if !ua.is_empty() {
            builder = builder.user_agent(ua);
        }

        if let Err(e) = builder.build() {
            return Err(format!("Failed to open guest window: {e}"));
        }
        created = true;
    }

    let window = state.app_handle.get_webview_window(label);
    if window.is_none() {
         return Err("未找到内置浏览器窗口".to_string());
    }
    let window = window.unwrap();
    
    if created {
        tokio::time::sleep(Duration::from_millis(3000)).await;
    }

    let mut urls: Vec<String> = vec![
        "https://live.kuaishou.com/".to_string(),
        "https://www.kuaishou.com/".to_string(),
        "https://kuaishou.com/".to_string(),
    ];
    
    if let Ok(extra) = std::env::var("KUAISHOU_WEBVIEW_COOKIE_URLS") {
        for raw in extra.split(',') {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                urls.push(trimmed.to_string());
            }
        }
    }

    let mut cookie_map: HashMap<String, String> = HashMap::new();
    for raw in &urls {
        if let Ok(url) = Url::parse(&raw) {
            if let Ok(cookies) = window.cookies_for_url(url) {
                for cookie in cookies {
                    let value = cookie.value();
                    if value.is_empty() {
                        continue;
                    }
                    cookie_map.insert(cookie.name().to_string(), value.to_string());
                }
            }
        }
    }

    if created {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Remove device identifiers for guest mode
    remove_cookie_keys_ci(
        &mut cookie_map,
        &["did", "didv", "kwpsecproductname"],
    );

    let mut cookie_pairs: Vec<String> = cookie_map
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    cookie_pairs.sort();
    let cookie_str = cookie_pairs.join("; ");
        
    if cookie_str.is_empty() {
        if created {
             let _ = window.close();
        }
        return Err("未读取到 Kuaishou Cookie".to_string());
    }
    
    if created {
        let _ = window.close();
    }
    return Ok(cookie_str);
}


#[cfg(not(feature = "gui"))]
async fn get_huya_webview_guest_cookies(
    _state: &crate::state::State,
    _label: &str,
) -> Result<String, String> {
    Err("Guest cookies require GUI mode".to_string())
}

#[cfg(not(feature = "gui"))]
async fn get_tiktok_webview_guest_cookies(
    _state: &crate::state::State,
    _label: &str,
) -> Result<String, String> {
    Err("Guest cookies require GUI mode".to_string())
}

#[cfg(not(feature = "gui"))]
async fn get_douyin_webview_guest_cookies(
    _state: &crate::state::State,
    _label: &str,
) -> Result<String, String> {
    Err("Guest cookies require GUI mode".to_string())
}

#[cfg(not(feature = "gui"))]
async fn get_kuaishou_webview_guest_cookies(
    _state: &crate::state::State,
    _label: &str,
) -> Result<String, String> {
    Err("Guest cookies require GUI mode".to_string())
}

#[cfg(not(feature = "gui"))]
async fn get_bilibili_webview_guest_cookies(
    _state: &crate::state::State,
    _label: &str,
) -> Result<String, String> {
    Err("Guest cookies require GUI mode".to_string())
}

/// Internal version for direct state usage (e.g., from background timers)

#[cfg(feature = "gui")]
async fn refresh_guest_accounts_inner(
    state: &crate::state::State,
    reason: Option<&str>,
    force: bool,
) -> Result<(), String> {
    let _guard = match try_begin_guest_refresh(force, reason) {
        Some(guard) => guard,
        None => return Ok(()),
    };

    if let Some(reason) = reason {
        log::warn!("[Account] Guest refresh triggered: {}", reason);
    }

    let old_entries_snapshot = state.config.read().await.guest_accounts.clone();
    let mut updates: Vec<(String, String)> = Vec::new();
    let updates_final: Vec<(String, String)>;
    let mut attempt: u8 = 0;
    let max_attempts: u8 = 2;
    let mut last_guest_labels: Vec<String> = Vec::new();

    loop {
        attempt = attempt.saturating_add(1);
        updates.clear();
        last_guest_labels.clear();

        // Clear and close any existing guest webviews to avoid reusing stale session data.
        let active_windows = state.app_handle.webview_windows();
        for (label, window) in active_windows {
            if label.contains("-guest") {
                let _ = window.clear_all_browsing_data();
                let _ = window.close();
            }
        }

        let refresh_id = Utc::now().timestamp_millis();
        let douyin_label = format!("douyin-guest-{refresh_id}");
        let tiktok_label = format!("tiktok-guest-{refresh_id}");
        let huya_label = format!("huya-guest-{refresh_id}");
        let bilibili_label = format!("bilibili-guest-{refresh_id}");
        let kuaishou_label = format!("kuaishou-guest-{refresh_id}");

        // Helper function to create or get headless webview for guest mode
        let ensure_guest_webview = |label: &str, url: &str| -> Result<(), String> {
            if state.app_handle.get_webview_window(label).is_none() {
                log::info!("Creating headless guest webview: {} -> {}", label, url);
                let mut builder = WebviewWindowBuilder::new(
                    &state.app_handle,
                    label,
                    WebviewUrl::External(Url::parse(url).map_err(|e| e.to_string())?),
                )
                .title(format!("Guest - {}", label))
                .visible(false) // headless mode
                .skip_taskbar(true)
                .initialization_script(r#"
                    (function() {
                        // Mute audio context if possible
                        if (window.AudioContext || window.webkitAudioContext) {
                            try {
                                const AudioContext = window.AudioContext || window.webkitAudioContext;
                                const ctx = new AudioContext();
                                ctx.suspend();
                            } catch (e) {}
                        }
                        
                        // Mute all media elements periodically
                        const muteAll = () => {
                            document.querySelectorAll('video, audio').forEach(el => {
                                el.muted = true;
                                el.pause();
                                el.volume = 0;
                            });
                        };
                        
                        setInterval(muteAll, 500);
                        document.addEventListener('DOMContentLoaded', muteAll);
                        window.addEventListener('load', muteAll);
                        
                        // Override media play
                        const originalPlay = HTMLMediaElement.prototype.play;
                        HTMLMediaElement.prototype.play = function() {
                            this.muted = true;
                            return Promise.resolve();
                        };
                    })();
                "#);

                if label.starts_with("tiktok-") {
                    let fallback_ua = std::env::var("TIKTOK_WEBVIEW_USER_AGENT")
                        .or_else(|_| std::env::var("TIKTOK_USER_AGENT"))
                        .unwrap_or_default();
                    let ua = fallback_ua.trim();
                    if !ua.is_empty() {
                        builder = builder.user_agent(ua);
                    }
                }

                match builder.build() {
                    Ok(_window) => {
                        log::info!("Successfully created headless webview: {}", label);
                        Ok(())
                    }
                    Err(e) => {
                        log::error!("Failed to create headless webview {}: {}", label, e);
                        Err(format!("Failed to create webview: {}", e))
                    }
                }
            } else {
                Ok(())
            }
        };

        // Ensure webviews for platforms that need real browser cookies
        // Kuaishou uses headless webview cookies plus rotated did/didv for guest refresh
        let guest_platforms: Vec<(String, &'static str)> = vec![
            (douyin_label.clone(), "https://live.douyin.com/"),
            (tiktok_label.clone(), "https://live.tiktok.com/"),
            (huya_label.clone(), "https://www.huya.com/"),
            (bilibili_label.clone(), "https://live.bilibili.com/"),
            (kuaishou_label.clone(), "https://live.kuaishou.com/"),
        ];

        for (label, url) in &guest_platforms {
            if let Err(e) = ensure_guest_webview(label, url) {
                log::warn!("Cannot ensure webview {}: {}", label, e);
            } else {
                log::info!("Webview {} ensured successfully", label);
            }
        }

        last_guest_labels.extend(guest_platforms.iter().map(|(label, _)| label.clone()));

        // Wait for webviews to fully load and populate cookies
        log::info!("Waiting 2 seconds for guest webviews to load...");
        tokio::time::sleep(Duration::from_secs(2)).await;
        log::info!("Starting cookie collection from guest webviews");

        // Douyin
        {
            log::info!("Collecting Douyin guest cookies");
            match get_douyin_webview_guest_cookies(state, &douyin_label).await {
                Ok(cookie_str) if !cookie_str.is_empty() => {
                    log::info!("Douyin guest cookie collected: {} chars", cookie_str.len());
                    updates.push(("douyin".to_string(), cookie_str));
                }
                Ok(_) => log::warn!("Douyin cookie is empty"),
                Err(e) => log::warn!("Failed to get Douyin guest cookies: {}", e),
            }
        }

        // Kuaishou
        {
            log::info!("Collecting Kuaishou guest cookies");
            match get_kuaishou_webview_guest_cookies(state, &kuaishou_label).await {
                Ok(cookie_str) if !cookie_str.is_empty() => {
                    log::info!("Kuaishou guest cookie collected: {} chars", cookie_str.len());
                    updates.push(("kuaishou".to_string(), cookie_str));
                }
                Ok(_) => log::warn!("Kuaishou cookie is empty"),
                Err(e) => log::warn!("Failed to get Kuaishou guest cookies: {}", e),
            }
        }

        // TikTok
        {
            log::info!("Collecting TikTok guest cookies");
            match get_tiktok_webview_guest_cookies(state, &tiktok_label).await {
                Ok(cookie_str) if !cookie_str.is_empty() => {
                    log::info!("TikTok guest cookie collected: {} chars", cookie_str.len());
                    updates.push(("tiktok".to_string(), cookie_str));
                }
                Ok(_) => log::warn!("TikTok cookie is empty"),
                Err(e) => log::warn!("Failed to get TikTok guest cookies: {}", e),
            }
        }

        // Huya
        {
            log::info!("Collecting Huya guest cookies");
            match get_huya_webview_guest_cookies(state, &huya_label).await {
                Ok(cookie_str) if !cookie_str.is_empty() => {
                    log::info!("Huya guest cookie collected: {} chars", cookie_str.len());
                    updates.push(("huya".to_string(), cookie_str));
                }
                Ok(_) => log::warn!("Huya cookie is empty"),
                Err(e) => log::warn!("Failed to get Huya guest cookies: {}", e),
            }
        }

        // Bilibili
        {
            log::info!("Collecting Bilibili guest cookies");
            match get_bilibili_webview_guest_cookies(state, &bilibili_label).await {
                Ok(cookie_str) if !cookie_str.is_empty() => {
                    log::info!("Bilibili guest cookie collected: {} chars", cookie_str.len());
                    updates.push(("bilibili".to_string(), cookie_str));
                }
                Ok(_) => log::warn!("Bilibili cookie is empty"),
                Err(e) => log::warn!("Failed to get Bilibili guest cookies: {}", e),
            }
        }

        // Close guest webviews created in this attempt
        for label in &last_guest_labels {
            if let Some(window) = state.app_handle.get_webview_window(label) {
                let _ = window.close();
            }
        }

        // Double check: close any active guest windows
        let active_windows = state.app_handle.webview_windows();
        for (label, window) in active_windows {
            if label.contains("-guest") {
                let _ = window.close();
            }
        }

        if updates.is_empty() {
            if attempt < max_attempts {
                log::warn!(
                    "[Account] Guest cookie refresh returned empty result, retrying (attempt {}/{})",
                    attempt,
                    max_attempts
                );
                tokio::time::sleep(Duration::from_millis(800)).await;
                continue;
            }
            return Err("??????????? Cookie????????".to_string());
        }

        let mut changed_updates = Vec::new();
        for (platform, cookies) in updates.iter() {
            let changed = old_entries_snapshot
                .iter()
                .find(|entry| entry.platform == *platform)
                .map(|entry| {
                    normalize_cookie_string_for_compare(&entry.cookies)
                        != normalize_cookie_string_for_compare(cookies)
                })
                .unwrap_or(true);
            if changed {
                changed_updates.push((platform.clone(), cookies.clone()));
            }
        }

        if changed_updates.is_empty() {
            if attempt < max_attempts {
                log::warn!(
                    "[Account] Guest cookies unchanged, retrying (attempt {}/{})",
                    attempt,
                    max_attempts
                );
                tokio::time::sleep(Duration::from_millis(800)).await;
                continue;
            }
            return Err("?? Cookie ?????????".to_string());
        }

        updates_final = changed_updates;
        break;
    }

    let updated_entries = {
        let mut config = state.config.write().await;
        let old_entries = config.guest_accounts.clone();
        let mut next = config.guest_accounts.clone();
        for (platform, cookies) in &updates_final {
            // Remove existing entries to prevent duplicates and ensure single entry per platform
            next.retain(|entry| entry.platform != *platform);
            next.push(DefaultAccountConfig {
                platform: platform.clone(),
                cookies: cookies.clone(),
                extra: String::new(),
            });
        }
        config.guest_accounts = next.clone();
        config.save();
        (next, old_entries)
    };
    let (updated_entries, old_entries) = updated_entries;

    let mut accounts =
        load_accounts_file_or_example().map(|(accounts, _)| accounts).unwrap_or_default();
    accounts.guest_accounts = updated_entries.clone();
    let path = resolve_accounts_file_write_path();
    if let Err(err) = write_accounts_file(&path, &accounts) {
        log::warn!("Failed to write accounts file {:?}: {}", path, err);
    }

    for old in old_entries {
        let still_exists = updated_entries.iter().any(|entry| {
            entry.platform == old.platform && entry.cookies.trim() == old.cookies.trim()
        });
        if !still_exists {
            remove_accounts_by_platform_cookies(&state.db, &old.platform, &old.cookies).await;
        }
    }

    let config_snapshot = state.config.read().await.clone();
    ensure_guest_accounts(&state.db, &config_snapshot).await;
    sync_tiktok_webview_cookies(&state.db, &config_snapshot).await;

    // Double check: close any active guest windows
    let active_windows = state.app_handle.webview_windows();
    for (label, window) in active_windows {
        if label.contains("-guest") {
            log::info!("Force closing stray guest window: {}", label);
            let _ = window.close();
        }
    }

    Ok(())
}
#[cfg(not(feature = "gui"))]
async fn refresh_guest_accounts_inner(
    _state: &crate::state::State,
    _reason: Option<&str>,
    _force: bool,
) -> Result<(), String> {
    Err("Guest cookie refresh requires GUI mode".to_string())
}

pub async fn refresh_guest_accounts(state: state_type!()) -> Result<(), String> {
    refresh_guest_accounts_inner(&state, None, false).await
}

pub async fn refresh_guest_accounts_on_demand(state: State, reason: String) -> Result<(), String> {
    let reason_ref = reason.as_str();
    refresh_guest_accounts_inner(&state, Some(reason_ref), true).await
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn get_qr_status(
    _state: state_type!(),
    platform: String,
    qrcode_key: &str,
) -> Result<PlatformQrStatus, String> {
    log::warn!("[Account] get_qr_status platform={}, key_len={}", platform, qrcode_key.len());
    let client = crate::utils::http::no_proxy_client();
    match platform.as_str() {
        "bilibili" => match bilibili::api::get_qr_status(&client, qrcode_key).await {
            Ok(qr_status) => {
                log::warn!(
                    "[Account] bilibili qr_status code={}, cookies_len={}",
                    qr_status.code,
                    qr_status.cookies.len()
                );
                Ok(PlatformQrStatus {
                    code: qr_status.code,
                    cookies: qr_status.cookies,
                    message: None,
                })
            }
            Err(e) => Err(e.to_string()),
        },
        "douyin" => match douyin::api::get_qr_login_status(&client, qrcode_key).await {
            Ok(qr_status) => {
                log::warn!(
                    "[Account] douyin qr_status code={}, msg={}, cookies_len={}",
                    qr_status.code,
                    qr_status.message,
                    qr_status.cookies.len()
                );
                Ok(PlatformQrStatus {
                    code: qr_status.code,
                    cookies: qr_status.cookies,
                    message: Some(qr_status.message),
                })
            }
            Err(e) => Err(e.to_string()),
        },
        "kuaishou" => {
            let mut parts = qrcode_key.split('|');
            let token = parts.next().ok_or_else(|| "Invalid Kuaishou QR key".to_string())?;
            let signature = parts.next().ok_or_else(|| "Invalid Kuaishou QR key".to_string())?;
            let cookie = parts.next();
            match kuaishou::api::get_qr_status(&client, token, signature, cookie).await {
                Ok(qr_status) => {
                    log::warn!(
                        "[Account] kuaishou qr_status code={}, cookies_len={}",
                        qr_status.code,
                        qr_status.cookies.len()
                    );
                    Ok(PlatformQrStatus {
                        code: qr_status.code,
                        cookies: qr_status.cookies,
                        message: qr_status.message,
                    })
                }
                Err(e) => Err(e.to_string()),
            }
        }
        "tiktok" => match tiktok::api::get_qr_login_status(&client, qrcode_key).await {
            Ok(qr_status) => {
                log::warn!(
                    "[Account] tiktok qr_status code={}, msg={}, cookies_len={}",
                    qr_status.code,
                    qr_status.message,
                    qr_status.cookies.len()
                );
                Ok(PlatformQrStatus {
                    code: qr_status.code,
                    cookies: qr_status.cookies,
                    message: Some(qr_status.message),
                })
            }
            Err(e) => Err(e.to_string()),
        },
        _ => Err("Invalid platform".to_string()),
    }
}

pub async fn ensure_guest_accounts(db: &Database, config: &Config) {
    if !config.use_guest_accounts {
        return;
    }
    if config.guest_accounts.is_empty() {
        return;
    }

    // First, clean up any old guest accounts that are not in the current config
    let all_accounts = match db.get_accounts().await {
        Ok(acc) => acc,
        Err(e) => {
            log::warn!("Failed to load accounts for cleanup: {}", e);
            Vec::new()
        }
    };
    
    // Identify accounts to keep (those present in config)
    let valid_guest_cookies: Vec<(String, String)> = config.guest_accounts
        .iter()
        .map(|entry| (entry.platform.clone(), entry.cookies.trim().to_string()))
        .collect();

    // Track which (platform, cookie) pairs we have already seen in the DB to remove duplicates
    let mut seen_valid_guests = std::collections::HashSet::new();

    for account in all_accounts {
        // Only target guest accounts (uid starts with "guest:" or legacy "cookie_")
        // Also strictly treat auto-generated-looking cookies as guests if necessary
        let is_guest_marker = account.uid.starts_with("guest:") || account.uid.starts_with("cookie_");
        
        if is_guest_marker {
            let matches_config = valid_guest_cookies.iter().any(|(p, c)| {
                *p == account.platform && *c == account.cookies.trim()
            });

            if !matches_config {
                log::info!("Removing outdated guest account: {} ({})", account.platform, account.uid);
                if let Err(e) = db.remove_account(&account.platform, &account.uid).await {
                    log::warn!("Failed to remove outdated guest account: {}", e);
                }
            } else {
                // It matches config, but is it a duplicate in the DB?
                let key = (account.platform.clone(), account.cookies.trim().to_string());
                if seen_valid_guests.contains(&key) {
                    log::info!("Removing duplicate guest account: {} ({})", account.platform, account.uid);
                    if let Err(e) = db.remove_account(&account.platform, &account.uid).await {
                        log::warn!("Failed to remove duplicate guest account: {}", e);
                    }
                } else {
                    seen_valid_guests.insert(key);
                }
            }
        }
    }

    for entry in &config.guest_accounts {
        let cookies = entry.cookies.trim();
        if cookies.is_empty() {
            continue;
        }
        let platform = match PlatformType::from_str(&entry.platform) {
            Ok(platform) => platform,
            Err(_) => {
                log::warn!("Skip guest account with invalid platform: {}", entry.platform);
                continue;
            }
        };
        let accounts = match db.get_accounts().await {
            Ok(accounts) => accounts,
            Err(e) => {
                log::warn!("Failed to load accounts for validation: {}", e);
                continue;
            }
        };
        let existing = accounts.iter().find(|account| {
            account.platform == platform.as_str() && account.cookies.trim() == cookies
        });

        if let Some(existing) = existing {
            // Update cookies if they differ only by whitespace to keep DB clean
            if existing.cookies != cookies {
                 let mut updated = existing.clone();
                 updated.cookies = cookies.to_string();
                 if let Err(e) = db.add_account(&updated).await {
                     log::warn!("Failed to update guest account cookies: {}", e);
                 }
            }

            let expected_name = match platform {
                PlatformType::BiliBili => "Bilibili",
                PlatformType::Douyin => "Douyin",
                PlatformType::Huya => "Huya",
                PlatformType::Kuaishou => "Kuaishou",
                PlatformType::Xiaohongshu => "Xiaohongshu",
                PlatformType::TikTok => "TikTok",
                PlatformType::Weibo => "Weibo",
                PlatformType::Youtube => "Youtube",
            };
            if existing.name == expected_name && existing.avatar.is_empty() {
                continue;
            }
        }

        match build_account_row(platform.as_str(), cookies, None, Some("guest")).await {
            Ok(account) => {
                if let Err(e) = db.add_account(&account).await {
                    log::warn!(
                        "Failed to add guest account for {}: {}",
                        platform.as_str(),
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to build guest account for {}: {}",
                    platform.as_str(),
                    e
                );
                if let Some(existing) = existing {
                    if let Err(e) = db.remove_account(platform.as_str(), &existing.uid).await {
                        log::warn!(
                            "Failed to remove invalid guest account for {}: {}",
                            platform.as_str(),
                            e
                        );
                    }
                }
            }
        }
    }
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn check_tiktok_proxy(_state: state_type!()) -> Result<tiktok::api::TikTokProxyCheck, String> {
    let client = crate::utils::http::no_proxy_client();
    tiktok::api::check_proxy_available(&client)
        .await
        .map_err(|e| e.to_string())
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn check_tiktok_cookie(state: state_type!()) -> Result<tiktok::api::TikTokCookieCheck, String> {
    let account = state
        .db
        .get_account_by_platform("tiktok")
        .await
        .map_err(|_| "TikTok account not found".to_string())?;
    let client = crate::utils::http::no_proxy_client();
    tiktok::api::check_cookie_available(&client, &account.to_account())
        .await
        .map_err(|e| e.to_string())
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn get_qr(_state: state_type!(), platform: String) -> Result<PlatformQrInfo, String> {
    log::warn!("[Account] get_qr platform={}", platform);
    let client = crate::utils::http::no_proxy_client();
    match platform.as_str() {
        "bilibili" => match bilibili::api::get_qr(&client).await {
            Ok(qr_info) => {
                log::warn!(
                    "[Account] bilibili get_qr key_len={}, url_len={}",
                    qr_info.oauth_key.len(),
                    qr_info.url.len()
                );
                Ok(PlatformQrInfo {
                    oauth_key: qr_info.oauth_key,
                    url: Some(qr_info.url),
                    image: None,
                })
            }
            Err(e) => Err(e.to_string()),
        },
        "douyin" => match douyin::api::get_qr_login(&client).await {
            Ok(qr_info) => {
                log::warn!(
                    "[Account] douyin get_qr key_len={}, url_len={}, image={}",
                    qr_info.oauth_key.len(),
                    qr_info.url.len(),
                    qr_info.image.is_some()
                );
                Ok(PlatformQrInfo {
                    oauth_key: qr_info.oauth_key,
                    url: if qr_info.url.is_empty() {
                        None
                    } else {
                        Some(qr_info.url)
                    },
                    image: qr_info.image.map(|raw| {
                        if raw.starts_with("data:image") {
                            raw
                        } else {
                            format!("data:image/png;base64,{raw}")
                        }
                    }),
                })
            }
            Err(e) => Err(e.to_string()),
        },
        "kuaishou" => match kuaishou::api::get_qr(&client).await {
            Ok(qr_info) => {
                log::warn!(
                    "[Account] kuaishou get_qr key_len={}, image_len={}",
                    qr_info
                        .qr_login_token
                        .len()
                        .saturating_add(qr_info.qr_login_signature.len()),
                    qr_info.image_data.len()
                );
                Ok(PlatformQrInfo {
                    oauth_key: format!(
                        "{}|{}|{}",
                        qr_info.qr_login_token, qr_info.qr_login_signature, qr_info.qr_cookie
                    ),
                    url: None,
                    image: Some(format!("data:image/png;base64,{}", qr_info.image_data)),
                })
            }
            Err(e) => Err(e.to_string()),
        },
        "tiktok" => match tiktok::api::get_qr_login(&client).await {
            Ok(qr_info) => {
                log::warn!(
                    "[Account] tiktok get_qr key_len={}, url_len={}, image={}",
                    qr_info.oauth_key.len(),
                    qr_info.url.len(),
                    qr_info.image.is_some()
                );
                Ok(PlatformQrInfo {
                    oauth_key: qr_info.oauth_key,
                    url: if qr_info.url.is_empty() {
                        None
                    } else {
                        Some(qr_info.url)
                    },
                    image: qr_info.image.map(|raw| {
                        if raw.starts_with("data:image") {
                            raw
                        } else if raw.starts_with("http://") || raw.starts_with("https://") {
                            raw
                        } else {
                            format!("data:image/png;base64,{raw}")
                        }
                    }),
                })
            }
            Err(e) => Err(e.to_string()),
        },
        _ => Err("Invalid platform".to_string()),
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformQrInfo {
    pub oauth_key: String,
    pub url: Option<String>,
    pub image: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformQrStatus {
    pub code: u8,
    pub cookies: String,
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_item_from_cookies() {
        let cookies = "DedeUserID=1234567890; bili_jct=1234567890; yyuid=1234567890";
        let uid = get_item_from_cookies("DedeUserID", cookies).unwrap();
        assert_eq!(uid, "1234567890");
        let uid = get_item_from_cookies("yyuid", cookies).unwrap();
        assert_eq!(uid, "1234567890");
        let uid = get_item_from_cookies("bili_jct", cookies).unwrap();
        assert_eq!(uid, "1234567890");
        let uid = get_item_from_cookies("unknown", cookies).unwrap_err();
        assert_eq!(uid, "Invalid cookies: missing unknown");
    }
}
#[cfg(all(feature = "gui", not(feature = "headless")))]
use std::collections::HashMap;


/// 列出所有打开的 webview 窗口
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn list_webview_windows(state: state_type!()) -> Result<Vec<String>, String> {
    let login_labels = vec![
        "tiktok-login",
        "douyin-login",
        "kuaishou-login",
        "huya-login",
        "bilibili-login",
    ];
    
    let mut open_windows = Vec::new();
    for label in login_labels {
        if state.app_handle.get_webview_window(label).is_some() {
            open_windows.push(label.to_string());
        }
    }
    
    Ok(open_windows)
}

/// 关闭指定的 webview 窗口
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn close_webview_window(state: state_type!(), label: String) -> Result<(), String> {
    if let Some(window) = state.app_handle.get_webview_window(&label) {
        window.close().map_err(|e| format!("关闭窗口失败: {e}"))?;
        Ok(())
    } else {
        Err(format!("未找到窗口: {}", label))
    }
}

/// 关闭所有登录窗口
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn close_all_login_windows(state: state_type!()) -> Result<Vec<String>, String> {
    let login_labels = vec![
        "tiktok-login",
        "douyin-login",
        "kuaishou-login",
        "huya-login",
        "bilibili-login",
    ];
    
    let mut closed_windows = Vec::new();
    for label in login_labels {
        if let Some(window) = state.app_handle.get_webview_window(label) {
            if window.close().is_ok() {
                closed_windows.push(label.to_string());
            }
        }
    }
    
    Ok(closed_windows)
}

/// Start a background task that periodically refreshes guest cookies.
/// This should be called once during app startup in GUI mode.
/// The timer will check every 30 minutes and refresh cookies if guest mode is enabled.
#[cfg(feature = "gui")]
pub fn start_guest_cookie_refresh_timer(state: crate::state::State) {
    use tokio::time::{interval, Duration};
    
    tokio::spawn(async move {
        // Wait 2 minutes before first refresh to let the app fully initialize
        tokio::time::sleep(Duration::from_secs(120)).await;
        
        let mut interval = interval(Duration::from_secs(30 * 60)); // 30 minutes
        loop {
            interval.tick().await;
            
            // Check if guest mode is enabled
            let use_guest = state.config.read().await.use_guest_accounts;
            if !use_guest {
                log::debug!("[Account] Guest mode disabled, skipping cookie refresh");
                continue;
            }
            
            log::info!("[Account] Starting periodic guest cookie refresh...");
            let start = std::time::Instant::now();
            match refresh_guest_accounts_inner(&state, Some("timer"), false).await {
                Ok(()) => {
                    log::info!("[Account] Periodic guest cookie refresh completed successfully in {:?}", start.elapsed());
                }
                Err(e) => {
                    log::warn!("[Account] Periodic guest cookie refresh failed: {}", e);
                }
            }
        }
    });
}

#[cfg(not(feature = "gui"))]
pub fn start_guest_cookie_refresh_timer(_state: crate::state::State) {
    // Guest cookie refresh requires GUI mode for webview access
    log::info!("[Account] Guest cookie refresh timer not available in headless mode");
}
