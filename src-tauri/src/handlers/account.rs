use std::str::FromStr;

use crate::database::account::AccountRow;
use crate::config::{Config, DefaultAccountConfig};
use crate::database::Database;
use crate::state::State;
use crate::state_type;
use crate::utils::browser::BrowserCookieCollector;
use chrono::Utc;
use recorder::platforms::{
    bilibili, douyin, huya, kuaishou, tiktok, weibo, xiaohongshu, PlatformType,
};
use recorder::UserInfo;
use serde::{Deserialize, Serialize};

use hyper::header::HeaderValue;
#[cfg(feature = "gui")]
use tauri::State as TauriState;

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

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn add_account(
    state: state_type!(),
    platform: String,
    cookies: &str,
) -> Result<(), String> {
    let account = build_account_row(&platform, cookies).await?;
    state.db.add_account(&account).await?;
    Ok(())
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn update_default_account(
    state: state_type!(),
    platform: String,
    cookies: String,
) -> Result<(), String> {
    if cookies.trim().is_empty() {
        return Err("Empty cookies".to_string());
    }
    let _ = build_account_row(&platform, &cookies).await?;
    let mut old_cookies: Option<String> = None;
    {
        let mut config = state.config.write().await;
        if let Some(entry) = config
            .default_accounts
            .iter_mut()
            .find(|entry| entry.platform == platform)
        {
            if entry.cookies.trim() != cookies.trim() && !entry.cookies.trim().is_empty() {
                old_cookies = Some(entry.cookies.clone());
            }
            entry.cookies = cookies.clone();
        } else {
            config.default_accounts.push(DefaultAccountConfig {
                platform: platform.clone(),
                cookies: cookies.clone(),
            });
        }
        config.save();
    }
    if let Some(old_cookies) = old_cookies {
        remove_accounts_by_platform_cookies(&state.db, &platform, &old_cookies).await;
    }
    Ok(())
}

pub async fn ensure_default_accounts(db: &Database, config: &Config) {
    if !config.use_default_accounts {
        return;
    }
    if config.default_accounts.is_empty() {
        return;
    }

    for entry in &config.default_accounts {
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
            if let Err(e) = build_account_row(platform.as_str(), cookies).await {
                log::warn!(
                    "Default account invalid for {}: {}, reimporting",
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
        match build_account_row(platform.as_str(), cookies).await {
            Ok(account) => {
                if let Err(e) = db.add_account(&account).await {
                    log::warn!(
                        "Failed to add default account for {}: {}",
                        platform.as_str(),
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to build default account for {}: {}",
                    platform.as_str(),
                    e
                );
            }
        }
    }
}

pub async fn remove_default_accounts(db: &Database, config: &Config) {
    if config.default_accounts.is_empty() {
        return;
    }

    for entry in &config.default_accounts {
        remove_accounts_by_platform_cookies(db, &entry.platform, &entry.cookies).await;
    }
}

async fn remove_accounts_by_platform_cookies(db: &Database, platform: &str, cookies: &str) {
    let cookies = cookies.trim();
    if cookies.is_empty() {
        return;
    }
    let accounts = match db.get_accounts().await {
        Ok(accounts) => accounts,
        Err(e) => {
            log::warn!("Failed to load accounts for cleanup: {}", e);
            return;
        }
    };
    for account in accounts
        .iter()
        .filter(|account| account.platform == platform && account.cookies == cookies)
    {
        if let Err(e) = db.remove_account(platform, &account.uid).await {
            log::warn!(
                "Failed to remove default account for {}: {}",
                platform,
                e
            );
        }
    }
}

async fn build_account_row(platform: &str, cookies: &str) -> Result<AccountRow, String> {
    if let Err(e) = cookies.parse::<HeaderValue>() {
        return Err(format!("Invalid cookies: {e}"));
    }

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

    let client = reqwest::Client::new();
    let user_info = match platform {
        PlatformType::BiliBili => {
            if csrf.is_none() {
                return Err("Invalid bilibili cookies".to_string());
            }
            let uid = get_item_from_cookies("DedeUserID", cookies)?;
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid,
                name: String::new(),
                avatar: String::new(),
                csrf: csrf.clone().unwrap(),
                cookies: cookies.into(),
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
                Err(e) => return Err(e.to_string()),
            }
        }
        PlatformType::Douyin => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.into(),
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
                Err(e) => return Err(format!("Failed to get Douyin user info: {e}")),
            }
        }
        PlatformType::Huya => {
            let user_id = get_item_from_cookies("yyuid", cookies)?;

            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: user_id,
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.into(),
                created_at: Utc::now().to_rfc3339(),
            };

            match huya::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => UserInfo {
                    user_id: user_info.user_id,
                    user_name: user_info.user_name,
                    user_avatar: user_info.user_avatar,
                },
                Err(e) => return Err(format!("Failed to get Huya user info: {e}")),
            }
        }
        PlatformType::Kuaishou => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.into(),
                created_at: Utc::now().to_rfc3339(),
            };
            match kuaishou::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => user_info,
                Err(e) => return Err(format!("Failed to get Kuaishou user info: {e}")),
            }
        }
        PlatformType::Xiaohongshu => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.into(),
                created_at: Utc::now().to_rfc3339(),
            };
            match xiaohongshu::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => user_info,
                Err(e) => return Err(format!("Failed to get Xiaohongshu user info: {e}")),
            }
        }
        PlatformType::TikTok => {
            let tmp_account = AccountRow {
                platform: platform.as_str().to_string(),
                uid: "".into(),
                name: String::new(),
                avatar: String::new(),
                csrf: "".into(),
                cookies: cookies.into(),
                created_at: Utc::now().to_rfc3339(),
            };
            match tiktok::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => user_info,
                Err(e) => {
                    if let Some(uid) = extract_tiktok_uid(cookies) {
                        UserInfo {
                            user_id: uid,
                            user_name: "TikTok".to_string(),
                            user_avatar: String::new(),
                        }
                    } else {
                        return Err(format!("Failed to get TikTok user info: {e}"));
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
                cookies: cookies.into(),
                created_at: Utc::now().to_rfc3339(),
            };
            match weibo::api::get_user_info(&client, &tmp_account.to_account()).await {
                Ok(user_info) => user_info,
                Err(e) => return Err(format!("Failed to get Weibo user info: {e}")),
            }
        }
        PlatformType::Youtube => {
            return Err("Unsupported platform".to_string());
        }
    };

    Ok(AccountRow {
        platform: platform.as_str().to_string(),
        uid: user_info.user_id,
        name: user_info.user_name,
        avatar: user_info.user_avatar,
        csrf: csrf.unwrap(),
        cookies: cookies.into(),
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
        let client = reqwest::Client::new();
        let _ = bilibili::api::logout(&client, &account.to_account()).await;
    }
    Ok(state.db.remove_account(&platform, &uid).await?)
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn get_account_count(state: state_type!()) -> Result<u64, String> {
    Ok(state.db.get_accounts().await?.len() as u64)
}

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

fn collect_browser_cookies_for_platform(platform: &str) -> Option<String> {
    let domain = domain_for_platform(platform)?;
    let mut cookie_sets = Vec::new();
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

    if cookie_sets.is_empty() {
        None
    } else {
        Some(cookie_sets.join("; "))
    }
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn get_browser_cookies(
    _state: state_type!(),
    platform: String,
) -> Result<String, String> {
    match collect_browser_cookies_for_platform(&platform) {
        Some(cookies) => Ok(cookies),
        None => Err("No browser cookies found".to_string()),
    }
}

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn get_qr_status(
    _state: state_type!(),
    platform: String,
    qrcode_key: &str,
) -> Result<PlatformQrStatus, String> {
    log::warn!("[Account] get_qr_status platform={}, key_len={}", platform, qrcode_key.len());
    let client = reqwest::Client::new();
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

#[cfg_attr(feature = "gui", tauri::command)]
pub async fn get_qr(_state: state_type!(), platform: String) -> Result<PlatformQrInfo, String> {
    log::warn!("[Account] get_qr platform={}", platform);
    let client = reqwest::Client::new();
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
