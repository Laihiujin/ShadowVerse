use chrono::Local;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{self, AtomicU64};
use std::sync::Arc;

#[cfg(target_os = "windows")]
use winreg::enums::HKEY_CURRENT_USER;
#[cfg(target_os = "windows")]
use winreg::RegKey;

use crate::{danmu2ass::Danmu2AssOptions, recorder_manager::ClipRangeParams};

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub cache: String,
    pub output: String,
    pub live_start_notify: bool,
    pub live_end_notify: bool,
    pub clip_notify: bool,
    pub post_notify: bool,
    #[serde(default = "default_auto_subtitle")]
    pub auto_subtitle: bool,
    #[serde(default = "default_subtitle_generator_type")]
    pub subtitle_generator_type: String,
    #[serde(default = "default_whisper_model")]
    pub whisper_model: String,
    #[serde(default = "default_whisper_prompt")]
    pub whisper_prompt: String,
    #[serde(default = "default_openai_api_endpoint")]
    pub openai_api_endpoint: String,
    #[serde(default = "default_openai_api_key")]
    pub openai_api_key: String,
    #[serde(default = "default_clip_name_format")]
    pub clip_name_format: String,
    #[serde(default = "default_auto_generate_config")]
    pub auto_generate: AutoGenerateConfig,
    #[serde(default = "default_status_check_interval")]
    pub status_check_interval: u64,
    #[serde(skip)]
    pub config_path: String,
    #[serde(default = "default_whisper_language")]
    pub whisper_language: String,
    #[serde(default = "default_webhook_url")]
    pub webhook_url: String,
    #[serde(default = "default_danmu_ass_options")]
    pub danmu_ass_options: Danmu2AssOptions,
    #[serde(skip)]
    pub update_interval: Arc<AtomicU64>,
    #[serde(default = "default_powerlive_key")]
    pub powerlive_key: String,
    #[serde(default = "default_douyin_passport")]
    pub douyin_passport: DouyinPassportConfig,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct AutoGenerateConfig {
    pub enabled: bool,
    pub encode_danmu: bool,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct DouyinPassportConfig {
    pub provider_url: String,
    pub sign: String,
    pub qs: String,
    pub passport_jssdk_version: String,
    pub passport_jssdk_type: String,
    pub is_from_ttaccountsdk: String,
    pub aid: String,
    pub language: String,
    pub account_app_language: String,
    pub next: String,
    pub need_short_url: String,
    pub need_logo: String,
    pub is_new_login: String,
    pub is_from_iesaccountsaas: String,
    pub account_sdk_source: String,
    pub account_sdk_source_info: String,
    pub p_ui: String,
    pub p_ca: String,
    pub p_ca_real: String,
    pub p_js_v: String,
    pub p_js_t: String,
    pub p_zt: String,
    pub p_ver: String,
    pub p_ver_real: String,
    pub request_host: String,
    pub p_bd: String,
    pub p_ts: String,
    pub p_no: String,
    pub biz_trace_id: String,
    pub device_platform: String,
    pub ms_token: String,
    pub a_bogus: String,
    pub x_tt_passport_csrf_token: String,
    pub x_tt_passport_aid_sign: String,
    pub x_tt_passport_trace_id: String,
    pub x_tt_passport_verify_portrait: String,
    pub x_tt_session_dtrait: String,
    pub qr_origin: String,
    pub qr_referer: String,
}

fn default_danmu_ass_options() -> Danmu2AssOptions {
    Danmu2AssOptions::default()
}

fn default_auto_subtitle() -> bool {
    false
}

fn default_subtitle_generator_type() -> String {
    "whisper".to_string()
}

fn default_whisper_model() -> String {
    "whisper_model.bin".to_string()
}

fn default_whisper_prompt() -> String {
    "这是一段中文 你们好".to_string()
}

fn default_openai_api_endpoint() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_openai_api_key() -> String {
    String::new()
}

fn default_clip_name_format() -> String {
    "[{room_id}][{live_id}][{title}][{created_at}].mp4".to_string()
}

fn default_auto_generate_config() -> AutoGenerateConfig {
    AutoGenerateConfig {
        enabled: false,
        encode_danmu: false,
    }
}

fn default_status_check_interval() -> u64 {
    30
}

fn default_whisper_language() -> String {
    "auto".to_string()
}

fn default_webhook_url() -> String {
    String::new()
}

fn default_powerlive_key() -> String {
    String::new()
}

fn default_douyin_passport() -> DouyinPassportConfig {
    DouyinPassportConfig {
        provider_url: String::new(),
        sign: String::new(),
        qs: String::new(),
        passport_jssdk_version: "3.1.3".to_string(),
        passport_jssdk_type: "normal".to_string(),
        is_from_ttaccountsdk: "1".to_string(),
        aid: "6383".to_string(),
        language: "zh".to_string(),
        account_app_language: "zh-CN".to_string(),
        next: "https://live.douyin.com".to_string(),
        need_short_url: "true".to_string(),
        need_logo: "false".to_string(),
        is_new_login: "1".to_string(),
        is_from_iesaccountsaas: "1".to_string(),
        account_sdk_source: "web".to_string(),
        account_sdk_source_info: String::new(),
        p_ui: "2.1.4".to_string(),
        p_ca: "4.0.17".to_string(),
        p_ca_real: "1.0.0.729".to_string(),
        p_js_v: "3.1.3".to_string(),
        p_js_t: "pro".to_string(),
        p_zt: "3.3.10".to_string(),
        p_ver: "1.1.3".to_string(),
        p_ver_real: "0".to_string(),
        request_host: "https://live.douyin.com".to_string(),
        p_bd: "1.0.1.19-fix.01".to_string(),
        p_ts: String::new(),
        p_no: String::new(),
        biz_trace_id: String::new(),
        device_platform: "web_app".to_string(),
        ms_token: String::new(),
        a_bogus: String::new(),
        x_tt_passport_csrf_token: String::new(),
        x_tt_passport_aid_sign: String::new(),
        x_tt_passport_trace_id: String::new(),
        x_tt_passport_verify_portrait: String::new(),
        x_tt_session_dtrait: String::new(),
        qr_origin: "https://live.douyin.com".to_string(),
        qr_referer: "https://live.douyin.com/".to_string(),
    }
}

fn normalize_proxy_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut value = trimmed.to_string();
    if trimmed.contains('=') {
        let mut selected = None;
        for entry in trimmed.split(';') {
            let entry = entry.trim();
            if let Some(stripped) = entry.strip_prefix("https=") {
                selected = Some(stripped.trim());
                break;
            }
            if let Some(stripped) = entry.strip_prefix("http=") {
                selected = Some(stripped.trim());
            }
        }
        if let Some(selected) = selected {
            value = selected.to_string();
        }
    }

    if value.contains("://") {
        Some(value)
    } else {
        Some(format!("http://{value}"))
    }
}

fn detect_env_proxy() -> Option<String> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = env::var(key) {
            if let Some(proxy_url) = normalize_proxy_url(&value) {
                return Some(proxy_url);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn detect_windows_proxy() -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").ok()?;
    if enabled != 1 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    normalize_proxy_url(&server)
}

impl Config {
    pub fn load(
        config_path: &PathBuf,
        default_cache: &Path,
        default_output: &Path,
    ) -> Result<Self, String> {
        if let Ok(content) = std::fs::read_to_string(config_path) {
            if let Ok(mut config) = toml::from_str::<Config>(&content) {
                config.config_path = config_path.to_str().unwrap().into();
                config.update_interval = Arc::new(AtomicU64::new(config.status_check_interval));
                return Ok(config);
            }
        }

        if let Some(dir_path) = PathBuf::from(config_path).parent() {
            if let Err(e) = std::fs::create_dir_all(dir_path) {
                return Err(format!("Failed to create config dir: {e}"));
            }
        }

        let config = Config {
            cache: default_cache.to_str().unwrap().into(),
            output: default_output.to_str().unwrap().into(),
            live_start_notify: true,
            live_end_notify: true,
            clip_notify: true,
            post_notify: true,
            auto_subtitle: false,
            subtitle_generator_type: default_subtitle_generator_type(),
            whisper_model: default_whisper_model(),
            whisper_prompt: default_whisper_prompt(),
            openai_api_endpoint: default_openai_api_endpoint(),
            openai_api_key: default_openai_api_key(),
            clip_name_format: default_clip_name_format(),
            auto_generate: default_auto_generate_config(),
            status_check_interval: default_status_check_interval(),
            config_path: config_path.to_str().unwrap().into(),
            whisper_language: default_whisper_language(),
            webhook_url: default_webhook_url(),
            danmu_ass_options: default_danmu_ass_options(),
            update_interval: Arc::new(AtomicU64::new(default_status_check_interval())),
            powerlive_key: default_powerlive_key(),
            douyin_passport: default_douyin_passport(),
        };

        config.save();

        Ok(config)
    }

    pub fn save(&self) {
        let content = toml::to_string(&self).unwrap();
        if let Err(e) = std::fs::write(self.config_path.clone(), content) {
            log::error!("Failed to save config: {} {}", e, self.config_path);
        }
    }

    #[allow(dead_code)]
    pub fn set_cache_path(&mut self, path: &str) {
        self.cache = path.to_string();
        self.save();
    }

    #[allow(dead_code)]
    pub fn set_output_path(&mut self, path: &str) {
        self.output = path.into();
        self.save();
    }

    #[allow(dead_code)]
    pub fn set_whisper_language(&mut self, language: &str) {
        self.whisper_language = language.to_string();
        self.save();
    }

    #[allow(dead_code)]
    pub fn set_danmu_ass_options(&mut self, options: Danmu2AssOptions) {
        self.danmu_ass_options = options;
        self.save();
    }

    pub fn generate_clip_name(&self, params: &ClipRangeParams) -> PathBuf {
        // get format config
        // filter special characters from title to make sure file name is valid
        let title = params
            .title
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>();
        let format_config = self.clip_name_format.clone();
        let format_config = format_config.replace("{title}", &title);
        let format_config = format_config.replace("{platform}", &params.platform);
        let format_config = format_config.replace("{room_id}", &params.room_id.to_string());
        let format_config = format_config.replace("{live_id}", &params.live_id);
        let format_config = format_config.replace("{note}", &params.note);
        let format_config = format_config.replace(
            "{x}",
            &params
                .ranges
                .first()
                .map_or("0".to_string(), |r| r.start.to_string()),
        );
        let format_config = format_config.replace(
            "{y}",
            &params
                .ranges
                .last()
                .map_or("0".to_string(), |r| r.end.to_string()),
        );
        let format_config = format_config.replace(
            "{created_at}",
            &Local::now().format("%Y-%m-%d_%H-%M-%S").to_string(),
        );
        let duration = params.ranges.iter().map(|r| r.duration()).sum::<f64>();
        let format_config = format_config.replace("{length}", &duration.to_string());

        let sanitized = sanitize_filename::sanitize(&format_config);
        let output = self.output.clone();

        Path::new(&output).join(&sanitized)
    }

    pub fn set_status_check_interval(&mut self, interval: u64) {
        self.status_check_interval = interval;
        self.update_interval
            .store(interval, atomic::Ordering::Relaxed);
        self.save();
    }

    pub fn apply_proxy_env(&self) {
        let detected = detect_env_proxy().or_else(|| {
            #[cfg(target_os = "windows")]
            {
                detect_windows_proxy()
            }
            #[cfg(not(target_os = "windows"))]
            {
                None
            }
        });
        if let Some(proxy_url) = detected {
            std::env::set_var("TIKTOK_PROXY_URL", proxy_url);
        } else {
            std::env::remove_var("TIKTOK_PROXY_URL");
        }
    }

    pub fn apply_douyin_passport_env(&self) {
        let cfg = &self.douyin_passport;
        std::env::set_var("DOUYIN_PASSPORT_PROVIDER_URL", &cfg.provider_url);
        std::env::set_var("DOUYIN_SIGN", &cfg.sign);
        std::env::set_var("DOUYIN_QS", &cfg.qs);
        std::env::set_var("DOUYIN_PASSPORT_JSSDK_VERSION", &cfg.passport_jssdk_version);
        std::env::set_var("DOUYIN_PASSPORT_JSSDK_TYPE", &cfg.passport_jssdk_type);
        std::env::set_var("DOUYIN_IS_FROM_TTACCOUNTSDK", &cfg.is_from_ttaccountsdk);
        std::env::set_var("DOUYIN_AID", &cfg.aid);
        std::env::set_var("DOUYIN_LANGUAGE", &cfg.language);
        std::env::set_var("DOUYIN_ACCOUNT_APP_LANGUAGE", &cfg.account_app_language);
        std::env::set_var("DOUYIN_NEXT", &cfg.next);
        std::env::set_var("DOUYIN_NEED_SHORT_URL", &cfg.need_short_url);
        std::env::set_var("DOUYIN_NEED_LOGO", &cfg.need_logo);
        std::env::set_var("DOUYIN_IS_NEW_LOGIN", &cfg.is_new_login);
        std::env::set_var("DOUYIN_IS_FROM_IESACCOUNTSAAS", &cfg.is_from_iesaccountsaas);
        std::env::set_var("DOUYIN_ACCOUNT_SDK_SOURCE", &cfg.account_sdk_source);
        std::env::set_var("DOUYIN_ACCOUNT_SDK_SOURCE_INFO", &cfg.account_sdk_source_info);
        std::env::set_var("DOUYIN_P_UI", &cfg.p_ui);
        std::env::set_var("DOUYIN_P_CA", &cfg.p_ca);
        std::env::set_var("DOUYIN_P_CA_REAL", &cfg.p_ca_real);
        std::env::set_var("DOUYIN_P_JS_V", &cfg.p_js_v);
        std::env::set_var("DOUYIN_P_JS_T", &cfg.p_js_t);
        std::env::set_var("DOUYIN_P_ZT", &cfg.p_zt);
        std::env::set_var("DOUYIN_P_VER", &cfg.p_ver);
        std::env::set_var("DOUYIN_P_VER_REAL", &cfg.p_ver_real);
        std::env::set_var("DOUYIN_REQUEST_HOST", &cfg.request_host);
        std::env::set_var("DOUYIN_P_BD", &cfg.p_bd);
        std::env::set_var("DOUYIN_P_TS", &cfg.p_ts);
        std::env::set_var("DOUYIN_P_NO", &cfg.p_no);
        std::env::set_var("DOUYIN_BIZ_TRACE_ID", &cfg.biz_trace_id);
        std::env::set_var("DOUYIN_DEVICE_PLATFORM", &cfg.device_platform);
        std::env::set_var("DOUYIN_MS_TOKEN", &cfg.ms_token);
        std::env::set_var("DOUYIN_A_BOGUS", &cfg.a_bogus);
        std::env::set_var("DOUYIN_X_TT_PASSPORT_CSRF_TOKEN", &cfg.x_tt_passport_csrf_token);
        std::env::set_var("DOUYIN_X_TT_PASSPORT_AID_SIGN", &cfg.x_tt_passport_aid_sign);
        std::env::set_var("DOUYIN_X_TT_PASSPORT_TRACE_ID", &cfg.x_tt_passport_trace_id);
        std::env::set_var(
            "DOUYIN_X_TT_PASSPORT_VERIFY_PORTRAIT",
            &cfg.x_tt_passport_verify_portrait,
        );
        std::env::set_var("DOUYIN_X_TT_SESSION_DTRAIT", &cfg.x_tt_session_dtrait);
        std::env::set_var("DOUYIN_QR_ORIGIN", &cfg.qr_origin);
        std::env::set_var("DOUYIN_QR_REFERER", &cfg.qr_referer);
    }
}
