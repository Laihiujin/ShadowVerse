pub mod api;
pub mod response;

use crate::account::Account;
use crate::core::flv_recorder::FlvRecorder;
use crate::core::hls_recorder::{construct_stream_from_variant, HlsRecorder};
use crate::core::rtmp_recorder::RtmpRecorder;
use crate::core::{Codec, Format};
use crate::danmu::DanmuStorage;
use crate::errors::RecorderError;
use crate::events::RecorderEvent;
use crate::platforms::PlatformType;
use crate::traits::RecorderTrait;
use crate::{CachePath, Recorder, RoomInfo, UserInfo};
use async_trait::async_trait;
use chrono::Utc;
use danmu_stream::danmu_stream::DanmuStream;
use danmu_stream::provider::ProviderType;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{atomic, Arc};
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};

const KUAISHOU_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const KUAISHOU_MOBILE_USER_AGENT: &str =
    "ios/7.830 (ios 17.0; ; iPhone 15 (A2846/A3089/A3090/A3092))";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KuaishouProtocol {
    Hls,
    Flv,
    Rtmp,
}

impl KuaishouProtocol {
    fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "hls" | "m3u8" => Some(Self::Hls),
            "flv" => Some(Self::Flv),
            "rtmp" => Some(Self::Rtmp),
            _ => None,
        }
    }

    fn matches_url(self, url: &str) -> bool {
        match self {
            Self::Hls => url.contains(".m3u8"),
            Self::Flv => url.contains(".flv"),
            Self::Rtmp => url.starts_with("rtmp://") || url.starts_with("rtmps://"),
        }
    }
}

#[derive(Clone)]
pub struct KuaishouExtra {
    stream_url: Arc<RwLock<Option<String>>>,
    stream_list: Arc<RwLock<Vec<api::StreamInfo>>>,
    pre_live_id: Arc<RwLock<Option<String>>>,
    should_continue: Arc<AtomicBool>,
    last_error_ts: Arc<AtomicI64>,
}

pub type KuaishouRecorder = Recorder<KuaishouExtra>;

impl KuaishouRecorder {
    pub async fn new(
        room_id: &str,
        account: &Account,
        cache_dir: PathBuf,
        event_channel: broadcast::Sender<RecorderEvent>,
        update_interval: Arc<atomic::AtomicU64>,
        enabled: bool,
    ) -> Result<Self, RecorderError> {
        let mut default_headers = reqwest::header::HeaderMap::new();
        default_headers.insert("Referer", "https://live.kuaishou.com/".parse().unwrap());
        default_headers.insert(
            "User-Agent",
            KUAISHOU_USER_AGENT.parse().unwrap(),
        );

        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .no_proxy()
            .build()
            .map_err(|e| RecorderError::ApiError{ error: e.to_string() })?;
        let extra = KuaishouExtra {
            stream_url: Arc::new(RwLock::new(None)),
            stream_list: Arc::new(RwLock::new(Vec::new())),
            pre_live_id: Arc::new(RwLock::new(None)),
            should_continue: Arc::new(AtomicBool::new(false)),
            last_error_ts: Arc::new(AtomicI64::new(0)),
        };

        let recorder = Self {
            platform: PlatformType::Kuaishou,
            room_id: room_id.to_string(),
            account: account.clone(),
            client,
            event_channel,
            cache_dir,
            quit: Arc::new(atomic::AtomicBool::new(false)),
            enabled: Arc::new(atomic::AtomicBool::new(enabled)),
            update_interval,
            is_recording: Arc::new(atomic::AtomicBool::new(false)),
            room_info: Arc::new(RwLock::new(RoomInfo::default())),
            user_info: Arc::new(RwLock::new(UserInfo::default())),
            platform_live_id: Arc::new(RwLock::new(String::new())),
            live_id: Arc::new(RwLock::new(String::new())),
            danmu_storage: Arc::new(RwLock::new(None)),
            last_update: Arc::new(atomic::AtomicI64::new(Utc::now().timestamp())),
            last_sequence: Arc::new(atomic::AtomicU64::new(0)),
            danmu_task: Arc::new(Mutex::new(None)),
            record_task: Arc::new(Mutex::new(None)),
            extra,
        };

        log::info!("[Kuaishou][{}]Recorder created", room_id);

        Ok(recorder)
    }

    fn log_info(&self, message: &str) {
        log::info!("[Kuaishou][{}]{}", self.room_id, message);
    }

    fn log_error(&self, message: &str) {
        log::error!("[Kuaishou][{}]{}", self.room_id, message);
    }

    fn parse_bool_env(value: &str) -> Option<bool> {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    }

    fn prefer_flv_env() -> Option<bool> {
        std::env::var("BSR_KUAISHOU_PREFER_FLV")
            .ok()
            .and_then(|v| Self::parse_bool_env(&v))
    }

    fn prefer_protocol() -> KuaishouProtocol {
        if let Ok(value) = std::env::var("BSR_KUAISHOU_PREFER_PROTOCOL") {
            if let Some(protocol) = KuaishouProtocol::from_str(&value) {
                return protocol;
            }
        }
        if let Some(prefer_flv) = Self::prefer_flv_env() {
            if prefer_flv {
                return KuaishouProtocol::Flv;
            }
        }
        KuaishouProtocol::Hls
    }

    fn select_stream_url(
        streams: &[api::StreamInfo],
        prefer: KuaishouProtocol,
    ) -> Option<String> {
        let mut selected = streams
            .iter()
            .find(|stream| prefer.matches_url(&stream.url))
            .map(|stream| stream.url.clone());

        if selected.is_none() {
            selected = streams
                .iter()
                .find(|stream| KuaishouProtocol::Hls.matches_url(&stream.url))
                .map(|stream| stream.url.clone());
        }

        if selected.is_none() {
            selected = streams
                .iter()
                .find(|stream| KuaishouProtocol::Flv.matches_url(&stream.url))
                .map(|stream| stream.url.clone());
        }

        if selected.is_none() {
            selected = streams
                .iter()
                .find(|stream| KuaishouProtocol::Rtmp.matches_url(&stream.url))
                .map(|stream| stream.url.clone());
        }

        if selected.is_none() {
            selected = streams.first().map(|stream| stream.url.clone());
        }

        selected
    }

    pub async fn reset(&self) {
        *self.extra.stream_url.write().await = None;
        self.last_update
            .store(Utc::now().timestamp(), atomic::Ordering::Relaxed);
        *self.danmu_storage.write().await = None;
        *self.platform_live_id.write().await = String::new();
        *self.live_id.write().await = String::new();
        self.extra
            .last_error_ts
            .store(0, atomic::Ordering::Relaxed);
        if let Some(danmu_task) = self.danmu_task.lock().await.take() {
            danmu_task.abort();
            let _ = danmu_task.await;
            self.log_info("Danmu task aborted");
        }
    }

    async fn check_status(&self) -> bool {
        let pre_live_status = self.room_info.read().await.status;

        // Construct full URL from room_id
        let url = if self.room_id.starts_with("http") {
            self.room_id.clone()
        } else {
            format!("https://live.kuaishou.com/u/{}", self.room_id)
        };

        match api::get_room_info(&self.client, &self.account, &url).await {
            Ok(room_info) => {
                let prev_room = self.room_info.read().await.clone();
                let prev_user = self.user_info.read().await.clone();
                let final_title = if room_info.room_title.is_empty() {
                    prev_room.room_title.clone()
                } else {
                    room_info.room_title.clone()
                };
                let final_cover = if room_info.room_cover_url.is_empty() {
                    prev_room.room_cover.clone()
                } else {
                    room_info.room_cover_url.clone()
                };
                let final_user_name = if room_info.user_name.is_empty() {
                    prev_user.user_name.clone()
                } else {
                    room_info.user_name.clone()
                };
                let final_avatar = if room_info.user_avatar.is_empty() {
                    prev_user.user_avatar.clone()
                } else {
                    room_info.user_avatar.clone()
                };

                *self.room_info.write().await = RoomInfo {
                    platform: "kuaishou".to_string(),
                    room_id: self.room_id.to_string(),
                    room_title: final_title,
                    room_cover: final_cover,
                    status: room_info.live_status,
                };

                // Update user info
                if self.user_info.read().await.user_id != room_info.user_id {
                    *self.user_info.write().await = UserInfo {
                        user_id: room_info.user_id.to_string(),
                        user_name: final_user_name,
                        user_avatar: final_avatar,
                    }
                }

                let live_status = room_info.live_status;
                if pre_live_status != live_status {
                    self.log_info(&format!(
                        "Live status changed to {}, enabled: {}",
                        live_status,
                        self.enabled.load(atomic::Ordering::Relaxed)
                    ));

                    if live_status {
                        let _ = self.event_channel.send(RecorderEvent::LiveStart {
                            recorder: self.info().await,
                        });
                    } else {
                        let _ = self.event_channel.send(RecorderEvent::LiveEnd {
                            platform: PlatformType::Kuaishou,
                            room_id: self.room_id.to_string(),
                            recorder: self.info().await,
                        });
                        *self.live_id.write().await = String::new();
                    }

                    self.reset().await;
                }

                *self.platform_live_id.write().await = Utc::now().timestamp().to_string();

                if !live_status {
                    return false;
                }

                // No need to poll aggressively if should not record.
                if !self.should_record().await {
                    return false;
                }

                // Get stream URLs
                let new_stream = api::get_stream_urls(&self.client, &self.account, &url).await;

                match new_stream {
                    Ok(streams) => {
                        let prefer = Self::prefer_protocol();
                        let selected_url = Self::select_stream_url(&streams, prefer);

                        if let Some(url) = selected_url {
                            *self.extra.stream_list.write().await = streams.clone();
                            let pre_stream = self.extra.stream_url.read().await.clone();
                            *self.extra.stream_url.write().await = Some(url.clone());
                            self.last_update
                                .store(Utc::now().timestamp(), atomic::Ordering::Relaxed);

                            self.log_info(&format!(
                                "Update to new stream: {:?} => {}",
                                pre_stream, url
                            ));

                            true
                        } else {
                            self.log_error("No stream URLs found");
                            false
                        }
                    }
                    Err(e) => {
                        self.log_error(&format!("Fetch stream failed: {}", e));
                        true
                    }
                }
            }
            Err(e) => {
                if api::is_rate_limited_error(&e) {
                    self.log_info("Rate limited, backing off");
                    self.extra
                        .last_error_ts
                        .store(Utc::now().timestamp(), atomic::Ordering::Relaxed);
                    return false;
                }
                if api::is_room_disabled_error(&e) {
                    self.log_info("Room not enabled, skipping polling");
                    self.extra
                        .last_error_ts
                        .store(Utc::now().timestamp(), atomic::Ordering::Relaxed);
                    return false;
                }
                self.log_error(&format!("Update room status failed: {}", e));
                self.extra
                    .last_error_ts
                    .store(Utc::now().timestamp(), atomic::Ordering::Relaxed);
                pre_live_status
            }
        }
    }

    async fn danmu(&self) -> Result<(), RecorderError> {
        let cookies = self.account.cookies.clone();
        let room_id = self.room_id.clone();
        let danmu_stream = DanmuStream::new(ProviderType::Kuaishou, &cookies, &room_id).await;
        let danmu_stream = match danmu_stream {
            Ok(stream) => stream,
            Err(err) => {
                self.log_error(&format!("Failed to create danmu stream: {err}"));
                return Err(RecorderError::DanmuStreamError(err));
            }
        };

        let mut start_fut = Box::pin(danmu_stream.start());

        loop {
            tokio::select! {
                start_res = &mut start_fut => {
                    match start_res {
                        Ok(_) => {
                            self.log_info("Danmu stream finished");
                            return Ok(());
                        }
                        Err(err) => {
                            self.log_error(&format!("Danmu stream start error: {err}"));
                            return Err(RecorderError::DanmuStreamError(err));
                        }
                    }
                }
                recv_res = danmu_stream.recv() => {
                    match recv_res {
                        Ok(Some(msg)) => {
                            match msg {
                                danmu_stream::DanmuMessageType::DanmuMessage(danmu) => {
                                    let ts = Utc::now().timestamp_millis();
                                    let _ = self.event_channel.send(RecorderEvent::DanmuReceived {
                                        room: self.room_id.clone(),
                                        ts,
                                        content: danmu.message.clone(),
                                    });
                                    if let Some(storage) = self.danmu_storage.write().await.as_ref() {
                                        storage.add_line(ts, &danmu.message).await;
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            self.log_info("Danmu stream closed");
                            return Ok(());
                        }
                        Err(err) => {
                            self.log_error(&format!("Failed to receive danmu message: {err}"));
                            return Err(RecorderError::DanmuStreamError(err));
                        }
                    }
                }
            }
        }
    }

    /// Update entries for a new live
    async fn update_entries(&self, live_id: &str) -> Result<(), RecorderError> {
        let current_stream_url = self.extra.stream_url.read().await.clone();
        let Some(stream_url) = current_stream_url else {
            return Err(RecorderError::NoStreamAvailable);
        };
        let stream_list = self.extra.stream_list.read().await.clone();
        let fallback_hls = stream_list
            .iter()
            .find(|stream| stream.url.contains(".m3u8"))
            .map(|stream| stream.url.clone());
        let fallback_flv = stream_list
            .iter()
            .find(|stream| stream.url.contains(".flv"))
            .map(|stream| stream.url.clone());

        let work_dir = self.work_dir(live_id).await;
        self.log_info(&format!("New record started: {}", live_id));

        let _ = tokio::fs::create_dir_all(&work_dir.full_path()).await;

        // Download cover
        let room_info = self.room_info.read().await.clone();
        let cover_url = room_info.room_cover.clone();
        let cover_path = work_dir.with_filename("cover.jpg");
        let _ = api::download_file(&self.client, &cover_url, &cover_path.full_path()).await;

        let danmu_path = work_dir.with_filename("danmu.txt");
        *self.danmu_storage.write().await = DanmuStorage::new(&danmu_path.full_path()).await;

        *self.live_id.write().await = live_id.to_string();

        let self_clone = self.clone();
        self.log_info(&format!("Start fetching danmu for live {live_id}"));
        *self.danmu_task.lock().await = Some(tokio::spawn(async move {
            let _ = self_clone.danmu().await;
        }));

        // Send record start event
        let _ = self.event_channel.send(RecorderEvent::RecordStart {
            recorder: self.info().await,
        });

        self.is_recording.store(true, atomic::Ordering::Relaxed);

        let is_mobile_stream =
            stream_url.contains("auth_key=") || stream_url.contains("pull.yximgs.com");
        let mut headers = reqwest::header::HeaderMap::new();
        if is_mobile_stream {
            headers.insert("Referer", "https://www.kuaishou.com/".parse().unwrap());
            headers.insert("Origin", "https://www.kuaishou.com".parse().unwrap());
            headers.insert("User-Agent", KUAISHOU_MOBILE_USER_AGENT.parse().unwrap());
        } else {
            headers.insert("Referer", "https://live.kuaishou.com/".parse().unwrap());
            headers.insert("Origin", "https://live.kuaishou.com".parse().unwrap());
            headers.insert("User-Agent", KUAISHOU_USER_AGENT.parse().unwrap());
        }
        if !self.account.cookies.is_empty() {
            headers.insert("Cookie", self.account.cookies.parse().unwrap());
        }

        if stream_url.starts_with("rtmp://") || stream_url.starts_with("rtmps://") {
            self.log_info("Using RTMP recorder");
            let rtmp_recorder = RtmpRecorder::new(
                stream_url,
                work_dir.full_path(),
                self.enabled.clone(),
                self.event_channel.clone(),
                live_id.to_string(),
            );
            if let Err(e) = rtmp_recorder.start().await {
                self.log_error(&format!("Rtmp recorder quit with error: {}", e));
                return Err(e);
            }
            return Ok(());
        }
        if stream_url.contains(".flv") {
            self.log_info("Using FLV recorder");
            let flv_recorder = FlvRecorder::new(
                stream_url.clone(),
                headers.clone(),
                work_dir.full_path(),
                self.enabled.clone(),
                self.event_channel.clone(),
                live_id.to_string(),
            );
            if let Err(e) = flv_recorder.start().await {
                self.log_error(&format!("Flv recorder quit with error: {}", e));
                if let Some(hls_url) = fallback_hls {
                    self.log_info("FLV failed, fallback to HLS recorder");
                    return self
                        .run_hls_recorder(&hls_url, &headers, &work_dir, live_id)
                        .await;
                }
                return Err(e);
            }
            return Ok(());
        }

        // Create HLS stream
        // Kuaishou stream URLs are direct m3u8 URLs
        if let Err(e) = self
            .run_hls_recorder(&stream_url, &headers, &work_dir, live_id)
            .await
        {
            self.log_error(&format!("Hls recorder quit with error: {}", e));
            if let Some(flv_url) = fallback_flv {
                self.log_info("HLS failed, fallback to FLV recorder");
                let flv_recorder = FlvRecorder::new(
                    flv_url,
                    headers,
                    work_dir.full_path(),
                    self.enabled.clone(),
                    self.event_channel.clone(),
                    live_id.to_string(),
                );
                if let Err(err) = flv_recorder.start().await {
                    self.log_error(&format!("Flv recorder quit with error: {}", err));
                    return Err(err);
                }
                return Ok(());
            }
            return Err(e);
        }

        Ok(())
    }

    async fn run_hls_recorder(
        &self,
        stream_url: &str,
        headers: &reqwest::header::HeaderMap,
        work_dir: &CachePath,
        live_id: &str,
    ) -> Result<(), RecorderError> {
        let hls_stream = construct_stream_from_variant(
            live_id,
            stream_url,
            Format::TS,
            Codec::Avc,
        )
        .await
        .map_err(|_| RecorderError::NoStreamAvailable)?;

        let hls_recorder = HlsRecorder::new(
            self.room_id.to_string(),
            Arc::new(hls_stream),
            self.client.clone(),
            if self.account.cookies.is_empty() {
                None
            } else {
                Some(self.account.cookies.clone())
            },
            Some(headers.clone()),
            self.event_channel.clone(),
            work_dir.full_path(),
            self.enabled.clone(),
        )
        .await?;

        hls_recorder.start().await
    }
}

#[async_trait]
impl RecorderTrait<KuaishouExtra> for KuaishouRecorder {
    async fn run(&self) {
        let self_clone = self.clone();
        *self.record_task.lock().await = Some(tokio::spawn(async move {
            self_clone.log_info("Start running recorder");
            while !self_clone.quit.load(atomic::Ordering::Relaxed) {
                if self_clone.check_status().await {
                    // Live status is ok, start recording
                    if self_clone.should_record().await {
                        let live_id;
                        // If should continue with previous recording, use the same live id
                        if self_clone.extra.should_continue.load(Ordering::Relaxed)
                            && self_clone.extra.pre_live_id.read().await.is_some()
                        {
                            live_id = self_clone.extra.pre_live_id.read().await.clone().unwrap();
                            self_clone
                                .extra
                                .should_continue
                                .store(false, Ordering::Relaxed);
                        } else {
                            live_id = Utc::now().timestamp_millis().to_string();
                            self_clone
                                .extra
                                .pre_live_id
                                .write()
                                .await
                                .replace(live_id.clone());
                        }

                        if let Err(e) = self_clone.update_entries(&live_id).await {
                            match e {
                                RecorderError::StreamExpired { expire } => {
                                    self_clone
                                        .extra
                                        .should_continue
                                        .store(true, Ordering::Relaxed);
                                    self_clone
                                        .log_info(&format!("Stream expired at {}", expire));
                                }
                                _ => {
                                    self_clone.log_error(&format!("Update entries error: {}", e));
                                }
                            }
                        }

                        let _ = self_clone.event_channel.send(RecorderEvent::RecordEnd {
                            recorder: self_clone.info().await,
                        });
                    }

                    self_clone
                        .is_recording
                        .store(false, atomic::Ordering::Relaxed);

                    self_clone.reset().await;
                    // If should continue with previous recording, no need to sleep
                    if self_clone.extra.should_continue.load(Ordering::Relaxed) {
                        continue;
                    }
                    let error_backoff = std::env::var("BSR_KUAISHOU_ERROR_BACKOFF_SECS")
                        .ok()
                        .and_then(|v| v.trim().parse::<u64>().ok())
                        .unwrap_or(60);
                    let error_window = std::env::var("BSR_KUAISHOU_ERROR_WINDOW_SECS")
                        .ok()
                        .and_then(|v| v.trim().parse::<u64>().ok())
                        .unwrap_or(120);
                    let last_error_ts = self_clone.extra.last_error_ts.load(atomic::Ordering::Relaxed);
                    if last_error_ts > 0 {
                        let now = Utc::now().timestamp();
                        if now.saturating_sub(last_error_ts) <= error_window as i64 {
                            let interval = self_clone.update_interval.load(atomic::Ordering::Relaxed);
                            let mut sleep_secs = crate::utils::jitter_interval_secs(interval, 10);
                            sleep_secs = sleep_secs.max(interval);
                            sleep_secs = sleep_secs.saturating_add(error_backoff);
                            tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
                            continue;
                        }
                    }
                    // Go check status again after random 2-5 secs
                    let secs = rand::random::<u64>() % 4 + 2;
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                    continue;
                }

                let interval = self_clone.update_interval.load(atomic::Ordering::Relaxed);
                let mut sleep_secs = crate::utils::jitter_interval_secs(interval, 10);
                sleep_secs = sleep_secs.max(interval);
                let error_backoff = std::env::var("BSR_KUAISHOU_ERROR_BACKOFF_SECS")
                    .ok()
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(60);
                let error_window = std::env::var("BSR_KUAISHOU_ERROR_WINDOW_SECS")
                    .ok()
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(120);
                let last_error_ts = self_clone.extra.last_error_ts.load(atomic::Ordering::Relaxed);
                if last_error_ts > 0 {
                    let now = Utc::now().timestamp();
                    if now.saturating_sub(last_error_ts) <= error_window as i64 {
                        sleep_secs = sleep_secs.saturating_add(error_backoff);
                    }
                }
                tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
            }
        }));
    }
}
