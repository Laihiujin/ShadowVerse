mod messages;

use std::{
    io::Read,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use flate2::read::GzDecoder;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use rand::{distr::Alphanumeric, Rng};
use regex::Regex;
use reqwest::cookie::CookieStore;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;
use prost::Message;
use tokio::{
    sync::{mpsc, RwLock},
    time::sleep,
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::{
    provider::{DanmuMessageType, DanmuProvider},
    DanmuMessage, DanmuStreamError,
};

use messages::{
    CompressionType, CsHeartbeat, CsWebEnterRoom, PayloadType, ScWebFeedPush, SocketMessage,
};

type WsReadType = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

type WsWriteType = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsMessage,
>;

const KUAISHOU_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const KUAISHOU_ACCEPT_LANGUAGE: &str = "zh-CN,zh;q=0.9,en;q=0.8";
const KUAISHOU_SEC_CH_UA: &str = "\"Not)A;Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\"";
const KUAISHOU_SEC_CH_UA_PLATFORM: &str = "\"Windows\"";
const HEARTBEAT_INTERVAL_SECS: u64 = 20;

#[derive(Clone)]
struct KuaishouRoomInit {
    token: String,
    live_stream_id: String,
    websocket_urls: Vec<String>,
}

pub struct KuaishouDanmu {
    client: reqwest::Client,
    room_id: String,
    cookie: String,
    cookie_jar: Arc<reqwest::cookie::Jar>,
    stop: Arc<RwLock<bool>>,
    write: Arc<RwLock<Option<WsWriteType>>>,
}

#[async_trait]
impl DanmuProvider for KuaishouDanmu {
    async fn new(cookie: &str, room_id: &str) -> Result<Self, DanmuStreamError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "User-Agent",
            HeaderValue::from_static(KUAISHOU_USER_AGENT),
        );
        headers.insert("Accept", HeaderValue::from_static("*/*"));
        headers.insert(
            "Accept-Language",
            HeaderValue::from_static(KUAISHOU_ACCEPT_LANGUAGE),
        );
        headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
        headers.insert("Pragma", HeaderValue::from_static("no-cache"));
        headers.insert("Sec-CH-UA", HeaderValue::from_static(KUAISHOU_SEC_CH_UA));
        headers.insert("Sec-CH-UA-Mobile", HeaderValue::from_static("?0"));
        headers.insert(
            "Sec-CH-UA-Platform",
            HeaderValue::from_static(KUAISHOU_SEC_CH_UA_PLATFORM),
        );
        let mut cookie_string = cookie.to_string();
        if !cookie_string.contains("did=") {
            let did = gen_web_did();
            let didv = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            if !cookie_string.is_empty() {
                cookie_string.push_str("; ");
            }
            cookie_string.push_str(&format!("did={}; didv={}", did, didv));
        }

        let cookie_jar = Arc::new(reqwest::cookie::Jar::default());
        let cookie_url = url::Url::parse("https://live.kuaishou.com/").map_err(|e| {
            DanmuStreamError::MessageParseError {
                err: format!("Invalid cookie base url: {e}"),
            }
        })?;
        if !cookie_string.trim().is_empty() {
            cookie_jar.add_cookie_str(&cookie_string, &cookie_url);
        }

        let client = reqwest::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .cookie_provider(cookie_jar.clone())
            .build()?;

        Ok(Self {
            client,
            room_id: room_id.to_string(),
            cookie: cookie_string.clone(),
            cookie_jar,
            stop: Arc::new(RwLock::new(false)),
            write: Arc::new(RwLock::new(None)),
        })
    }

    async fn start(
        &self,
        tx: mpsc::UnboundedSender<DanmuMessageType>,
    ) -> Result<(), DanmuStreamError> {
        let mut retry_count = 0;
        const RETRY_DELAY: Duration = Duration::from_secs(5);
        info!(
            "Kuaishou WebSocket connection started, room_id: {}",
            self.room_id
        );

        loop {
            if *self.stop.read().await {
                info!(
                    "Kuaishou WebSocket connection stopped, room_id: {}",
                    self.room_id
                );
                break;
            }

            match self.connect_and_handle(tx.clone()).await {
                Ok(_) => {
                    info!(
                        "Kuaishou WebSocket connection closed normally, room_id: {}",
                        self.room_id
                    );
                    retry_count = 0;
                }
                Err(e) => {
                    error!(
                        "Kuaishou WebSocket connection error, room_id: {}, error: {}",
                        self.room_id, e
                    );
                    retry_count += 1;
                }
            }

            info!(
                "Retrying connection in {} seconds... (Attempt {}), room_id: {}",
                RETRY_DELAY.as_secs(),
                retry_count,
                self.room_id
            );
            sleep(RETRY_DELAY).await;
        }

        Ok(())
    }

    async fn stop(&self) -> Result<(), DanmuStreamError> {
        *self.stop.write().await = true;
        if let Some(mut write) = self.write.write().await.take() {
            if let Err(e) = write.close().await {
                error!("Failed to close Kuaishou WebSocket connection: {}", e);
            }
        }
        Ok(())
    }
}

impl KuaishouDanmu {
    async fn connect_and_handle(
        &self,
        tx: mpsc::UnboundedSender<DanmuMessageType>,
    ) -> Result<(), DanmuStreamError> {
        let room_init = self.room_init().await?;
        let ws_url = room_init
            .websocket_urls
            .first()
            .ok_or(DanmuStreamError::WebsocketError {
                err: "No websocket URL available".to_string(),
            })?
            .to_string();
        info!("Kuaishou danmu ws url selected: {}", ws_url);

        let (conn, _) = connect_async(&ws_url).await.map_err(|e| {
            DanmuStreamError::WebsocketError {
                err: e.to_string(),
            }
        })?;

        let (write, read) = conn.split();
        *self.write.write().await = Some(write);

        self.send_enter_room(&room_init).await?;

        tokio::select! {
            v = Self::send_heartbeat_packets(Arc::clone(&self.write)) => v,
            v = Self::recv(read, tx, self.room_id.clone(), Arc::clone(&self.stop)) => v
        }?;

        Ok(())
    }

    async fn send_enter_room(&self, room_init: &KuaishouRoomInit) -> Result<(), DanmuStreamError> {
        let page_id = format!(
            "{}{}",
            rand::rng()
                .sample_iter(&Alphanumeric)
                .take(16)
                .map(char::from)
                .collect::<String>(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let payload = CsWebEnterRoom {
            token: room_init.token.clone(),
            live_stream_id: room_init.live_stream_id.clone(),
            reconnect_count: 0,
            last_error_code: 0,
            exp_tag: String::new(),
            attach: String::new(),
            page_id,
        }
        .encode_to_vec();

        let msg = SocketMessage {
            payload_type: PayloadType::CsEnterRoom as i32,
            compression_type: CompressionType::None as i32,
            payload,
        };

        if let Some(write) = self.write.write().await.as_mut() {
            write
                .send(WsMessage::binary(msg.encode_to_vec()))
                .await
                .map_err(|e| DanmuStreamError::WebsocketError { err: e.to_string() })?;
        }

        Ok(())
    }

    async fn send_heartbeat_packets(
        write: Arc<RwLock<Option<WsWriteType>>>,
    ) -> Result<(), DanmuStreamError> {
        loop {
            let payload = CsHeartbeat {
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            }
            .encode_to_vec();
            let msg = SocketMessage {
                payload_type: PayloadType::CsHeartbeat as i32,
                compression_type: CompressionType::None as i32,
                payload,
            };

            if let Some(write) = write.write().await.as_mut() {
                write
                .send(WsMessage::binary(msg.encode_to_vec()))
                    .await
                    .map_err(|e| DanmuStreamError::WebsocketError { err: e.to_string() })?;
            }
            sleep(Duration::from_secs(HEARTBEAT_INTERVAL_SECS)).await;
        }
    }

    async fn recv(
        mut read: WsReadType,
        tx: mpsc::UnboundedSender<DanmuMessageType>,
        room_id: String,
        stop: Arc<RwLock<bool>>,
    ) -> Result<(), DanmuStreamError> {
        while let Some(msg_result) = read.next().await {
            if *stop.read().await {
                info!("Stopping Kuaishou danmu stream");
                break;
            }

            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    warn!("Kuaishou WS read error: {}", e);
                    break;
                }
            };

            // Only process binary frames; skip text/ping/pong/close
            if !msg.is_binary() {
                if msg.is_close() {
                    info!("Kuaishou WS closed by server");
                    break;
                }
                continue;
            }

            let data = msg.into_data();
            if data.is_empty() {
                continue;
            }

            // Protobuf decode failure → skip this message, don't crash the loop
            let socket_msg = match SocketMessage::decode(&*data) {
                Ok(m) => m,
                Err(e) => {
                    warn!("Kuaishou danmu: failed to decode SocketMessage: {}", e);
                    continue;
                }
            };

            let payload = match CompressionType::try_from(socket_msg.compression_type).ok() {
                Some(CompressionType::None) | Some(CompressionType::Unknown) => socket_msg.payload,
                Some(CompressionType::Gzip) => match gunzip(&socket_msg.payload) {
                    Ok(decompressed) => decompressed,
                    Err(e) => {
                        warn!("Kuaishou danmu: gzip decompress failed: {}", e);
                        continue;
                    }
                },
                Some(CompressionType::Aes) => {
                    warn!("Kuaishou payload uses AES compression, skipping");
                    continue;
                }
                None => socket_msg.payload,
            };

            if PayloadType::try_from(socket_msg.payload_type).ok() == Some(PayloadType::ScFeedPush)
            {
                let feed = match ScWebFeedPush::decode(&*payload) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("Kuaishou danmu: failed to decode ScWebFeedPush: {}", e);
                        continue;
                    }
                };
                for comment in feed.comment_feeds {
                    let user = comment.user.unwrap_or_default();
                    let user_id = user.principal_id.parse::<u64>().unwrap_or(0);
                    let color = parse_color(&comment.color);
                    let ts = if comment.time > 0 {
                        comment.time as i64
                    } else {
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64
                    };
                    let danmu = DanmuMessage {
                        room_id: room_id.clone(),
                        user_id,
                        user_name: user.user_name,
                        message: comment.content,
                        color,
                        timestamp: ts,
                    };
                    if tx.send(DanmuMessageType::DanmuMessage(danmu)).is_err() {
                        // Receiver dropped, stop
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }

    async fn room_init(&self) -> Result<KuaishouRoomInit, DanmuStreamError> {
        let referer = format!("https://live.kuaishou.com/u/{}", self.room_id);

        let mut cookie_for_danmu = self.cookie.clone();
        let mut html_from_page: Option<String> = None;
        if let Ok(html) = self.fetch_web_html().await {
            html_from_page = Some(html);
            // If we sent no cookie, pick up any set-cookie from the page request
            if cookie_for_danmu.trim().is_empty() {
                if let Some(jar_cookie) = self.get_cookie_from_jar() {
                    cookie_for_danmu = jar_cookie;
                }
            }
        }
        let hxfalcon = html_from_page
            .as_deref()
            .and_then(|html| extract_hxfalcon(html));
        let kww_for_danmu = extract_kww(&cookie_for_danmu);

        let has_web_st = cookie_for_danmu.contains("kuaishou.live.web_st=");
        info!(
            "Kuaishou danmu room_init: room={}, has_web_st={}, has_kwscode={}, has_kwfv1={}, cookie_len={}",
            self.room_id,
            has_web_st,
            cookie_for_danmu.contains("kwscode="),
            cookie_for_danmu.contains("kwfv1="),
            cookie_for_danmu.len(),
        );

        // ── Step 1: Parse liveStreamId from HTML playList, then call websocketinfo ──
        if let Some(html) = html_from_page.as_deref() {
            if let Some(livedetail) = extract_livedetail_from_html(html) {
                if let Some(id) = extract_live_stream_id(&livedetail) {
                    if !id.is_empty() {
                        match self
                            .fetch_websocketinfo_no_sign(
                                &referer,
                                &id,
                                &cookie_for_danmu,
                                kww_for_danmu.as_deref(),
                                hxfalcon.as_deref(),
                            )
                            .await
                        {
                            Ok((token, urls)) if !token.is_empty() && !urls.is_empty() => {
                                info!("Kuaishou danmu: got token+urls via HTML→websocketinfo, live_stream_id={}", id);
                                return Ok(KuaishouRoomInit {
                                    token,
                                    live_stream_id: id,
                                    websocket_urls: urls,
                                });
                            }
                            Ok(_) => {
                                info!("Kuaishou danmu: websocketinfo returned empty token/urls for id={}, trying fallbacks", id);
                            }
                            Err(e) => {
                                info!("Kuaishou danmu: websocketinfo failed for id={}: {}", id, e);
                            }
                        }
                    }
                }
            }
        }

        // ── Step 2: Try to extract token + urls directly from __INITIAL_STATE__ ──
        let mut token = String::new();
        let mut websocket_urls: Vec<String> = Vec::new();
        let mut live_stream_id = String::new();

        if let Some(html) = html_from_page.as_deref() {
            if let Some(state) = self.try_parse_initial_state(html) {
                if token.is_empty() {
                    token = extract_ws_token(&state).unwrap_or_default();
                }
                if websocket_urls.is_empty() {
                    websocket_urls = extract_websocket_urls(&state);
                }
                if live_stream_id.is_empty() {
                    live_stream_id = extract_live_stream_id(&state).unwrap_or_default();
                }
            }
            // Regex fallback on raw HTML
            if token.is_empty() {
                token = extract_ws_token_from_html(html).unwrap_or_default();
            }
            if websocket_urls.is_empty() {
                websocket_urls = extract_ws_urls_from_text(html);
            }
            if live_stream_id.is_empty() {
                live_stream_id = extract_live_stream_id_from_html(html).unwrap_or_default();
            }
        }

        // Fallback: liveStreamId from original cookie
        if live_stream_id.is_empty() {
            if let Some(id) = extract_live_stream_id_from_cookie(&self.cookie) {
                info!("Kuaishou danmu: liveStreamId from cookie");
                live_stream_id = id;
            }
        }

        if !token.is_empty() && !websocket_urls.is_empty() {
            info!("Kuaishou danmu: got token+urls from __INITIAL_STATE__/HTML");
            return Ok(KuaishouRoomInit {
                token,
                live_stream_id,
                websocket_urls,
            });
        }

        // ── Step 3: Call livedetail via principalId (works for both guest and login) ──
        // This is the most reliable way to get liveStreamId when HTML extraction fails.
        {
            info!("Kuaishou danmu: calling livedetail via principalId={}", self.room_id);
            if let Ok(resp) = self
                .client
                .get("https://live.kuaishou.com/live_api/liveroom/livedetail")
                .query(&[("principalId", self.room_id.as_str())])
                .header("Referer", referer.clone())
                .header("Origin", "https://live.kuaishou.com")
                .header("Accept", "application/json, text/plain, */*")
                .header("Accept-Language", KUAISHOU_ACCEPT_LANGUAGE)
                .header("Sec-Fetch-Dest", "empty")
                .header("Sec-Fetch-Mode", "cors")
                .header("Sec-Fetch-Site", "same-origin")
                .apply_header("Kww", kww_for_danmu.as_deref())
                .header("Cookie", cookie_for_danmu.clone())
                .send()
                .await
            {
                let ld_status = resp.status();
                let ld_text = resp.text().await.unwrap_or_default();
                if let Ok(data) = parse_response_data(&ld_text) {
                    if token.is_empty() {
                        token = extract_ws_token(&data).unwrap_or_default();
                    }
                    if websocket_urls.is_empty() {
                        websocket_urls = extract_websocket_urls(&data);
                    }
                    if live_stream_id.is_empty() {
                        live_stream_id = extract_live_stream_id(&data).unwrap_or_default();
                    }
                }
                if token.is_empty() {
                    token = extract_ws_token_from_text(&ld_text).unwrap_or_default();
                }
                if websocket_urls.is_empty() {
                    websocket_urls = extract_ws_urls_from_text(&ld_text);
                }
                info!(
                    "Kuaishou danmu livedetail (status {}): token_present={}, ws_urls={}, liveStreamId_present={}",
                    ld_status,
                    !token.is_empty(),
                    websocket_urls.len(),
                    !live_stream_id.is_empty()
                );
            }
        }

        // ── Step 4: Try websocketinfo again if we now have a live_stream_id ──
        if (token.is_empty() || websocket_urls.is_empty()) && !live_stream_id.is_empty() {
            info!("Kuaishou danmu: retrying websocketinfo with id={}", live_stream_id);
            if let Ok((t, urls)) = self
                .fetch_websocketinfo_no_sign(
                    &referer,
                    &live_stream_id,
                    &cookie_for_danmu,
                    kww_for_danmu.as_deref(),
                    hxfalcon.as_deref(),
                )
                .await
            {
                if !t.is_empty() {
                    token = t;
                }
                if !urls.is_empty() {
                    websocket_urls = urls;
                }
            }
        }

        info!(
            "Kuaishou danmu room_init result: token_present={}, ws_urls={}, live_stream_id_present={}",
            !token.is_empty(),
            websocket_urls.len(),
            !live_stream_id.is_empty(),
        );

        // ── Step 5: Environment variable overrides ──
        if token.is_empty() {
            if let Ok(env_token) = std::env::var("KUAISHOU_WS_TOKEN") {
                let trimmed = env_token.trim();
                if !trimmed.is_empty() {
                    token = trimmed.to_string();
                }
            }
        }

        // ── Step 6: WebSocket URL fallback list (gifshow + kuaishou) ──
        if websocket_urls.is_empty() {
            if let Ok(urls) = std::env::var("KUAISHOU_WS_URLS") {
                for raw in urls.split(',') {
                    push_ws_url(&mut websocket_urls, raw.trim());
                }
            } else if let Ok(url) = std::env::var("KUAISHOU_WS_URL") {
                push_ws_url(&mut websocket_urls, url.trim());
            } else {
                // gifshow.com (legacy) + kuaishou.com (current)
                for group in 1..=9 {
                    push_ws_url(
                        &mut websocket_urls,
                        &format!("wss://livejs-ws-group{group}.gifshow.com/websocket"),
                    );
                }
                for group in 1..=9 {
                    push_ws_url(
                        &mut websocket_urls,
                        &format!("wss://live-ws-pg-group{group}.kuaishou.com/websocket"),
                    );
                }
            }
        }

        if live_stream_id.is_empty() {
            return Err(DanmuStreamError::MessageParseError {
                err: "Kuaishou liveStreamId missing (room not live?)".to_string(),
            });
        }
        if token.is_empty() || websocket_urls.is_empty() {
            return Err(DanmuStreamError::MessageParseError {
                err: "Kuaishou websocket token or URL missing after all fallbacks".to_string(),
            });
        }

        Ok(KuaishouRoomInit {
            token,
            live_stream_id,
            websocket_urls,
        })
    }

    async fn fetch_web_html(&self) -> Result<String, DanmuStreamError> {
        let url = format!("https://live.kuaishou.com/u/{}", self.room_id);
        let response = self
            .client
            .get(&url)
            .header("Referer", "https://live.kuaishou.com/")
            .header("Origin", "https://live.kuaishou.com")
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", KUAISHOU_ACCEPT_LANGUAGE)
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Site", "none")
            .header("Sec-Fetch-User", "?1")
            .header("Upgrade-Insecure-Requests", "1")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            info!(
                "Kuaishou danmu fetch web html status: {} for {}",
                status,
                url
            );
        }
        Ok(response.text().await?)
    }

    fn try_parse_initial_state(&self, html: &str) -> Option<Value> {
        let state_str = extract_initial_state(html)?;
        serde_json::from_str::<Value>(&state_str)
            .or_else(|_| {
                let cleaned = clean_json_state(&state_str);
                serde_json::from_str::<Value>(&cleaned)
            })
            .ok()
    }
}

impl KuaishouDanmu {
    fn get_cookie_from_jar(&self) -> Option<String> {
        let url = url::Url::parse("https://live.kuaishou.com/").ok()?;
        self.cookie_jar
            .cookies(&url)
            .and_then(|v| v.to_str().ok().map(|s| s.to_string()))
    }

    async fn fetch_websocketinfo_no_sign(
        &self,
        referer: &str,
        live_stream_id: &str,
        cookie: &str,
        kww: Option<&str>,
        hxfalcon: Option<&str>,
    ) -> Result<(String, Vec<String>), DanmuStreamError> {
        let mut ws_builder = self
            .client
            .get("https://live.kuaishou.com/live_api/liveroom/websocketinfo")
            .query(&[("caver", "2"), ("liveStreamId", live_stream_id)]);
        if let Some(hxfalcon) = hxfalcon {
            ws_builder = ws_builder.query(&[("__NS_hxfalcon", hxfalcon)]);
        }
        let ws_resp = ws_builder
            .header("Referer", referer)
            .header("Origin", "https://live.kuaishou.com")
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", KUAISHOU_ACCEPT_LANGUAGE)
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-origin")
            .apply_header("Kww", kww)
            .header("Cookie", cookie)
            .send()
            .await?;
        let ws_status = ws_resp.status();
        let ws_info = ws_resp.text().await?;
        // Use lenient parser: result=2 is guest/basic-access and may still contain valid data
        let ws_data = parse_response_data(&ws_info).unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
        let mut token = extract_ws_token(&ws_data).unwrap_or_default();
        let mut websocket_urls = extract_websocket_urls(&ws_data);
        if token.is_empty() {
            token = extract_ws_token_from_text(&ws_info).unwrap_or_default();
        }
        if websocket_urls.is_empty() {
            websocket_urls = extract_ws_urls_from_text(&ws_info);
        }
        if token.is_empty() || websocket_urls.is_empty() {
            let snippet_len = ws_info.len().min(300);
            if snippet_len > 0 {
                info!(
                    "Kuaishou danmu websocketinfo(no-sign) snippet (status {}): {}...",
                    ws_status,
                    &ws_info[..snippet_len]
                );
            }
        }
        Ok((token, websocket_urls))
    }
}


fn extract_kww(cookie: &str) -> Option<String> {
    if cookie.trim().is_empty() {
        return None;
    }
    let re = Regex::new(r"(?i)(?:kww|kwfv1)=([^;]+)").ok()?;
    re.captures(cookie)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

fn extract_hxfalcon(html: &str) -> Option<String> {
    let patterns = [
        r#"__NS_hxfalcon=([A-Za-z0-9_\-$.]+)"#,
        r#""__NS_hxfalcon"\s*:\s*"([^"]+)""#,
        r#"__NS_hxfalcon"\s*=\s*"([^"]+)""#,
    ];
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(cap) = re.captures(html).and_then(|c| c.get(1)) {
                let value = cap.as_str().trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn extract_livedetail_from_html(html: &str) -> Option<Value> {
    let re =
        Regex::new(r#""playList"\s*:\s*\[([\s\S]*?)\](?=,\s*"loading"|$)"#).ok()?;
    let raw = re
        .captures(html)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())?;
    let cleaned = raw.replace("undefined", "null");
    if let Ok(value) = serde_json::from_str::<Value>(&cleaned) {
        if let Some(first) = value.as_array().and_then(|arr| arr.first()).cloned() {
            return Some(first);
        }
        return Some(value);
    }
    let wrapped = format!("[{}]", cleaned);
    serde_json::from_str::<Value>(&wrapped)
        .ok()
        .and_then(|value| value.as_array().and_then(|arr| arr.first()).cloned())
}

fn parse_response_data(text: &str) -> Result<Value, DanmuStreamError> {
    let root: Value = serde_json::from_str(text).map_err(|e| DanmuStreamError::MessageParseError {
        err: e.to_string(),
    })?;

    let data = root.get("data").unwrap_or(&root);

    if let Some(result) = data.get("result").and_then(|v| v.as_i64()) {
        // result = 1: 正常
        // result = 2: 访客模式（基础权限）
        // result = 671/677: 直播间未开播
        if result != 1 && result != 2 && result != 671 && result != 677 {
            return Err(DanmuStreamError::MessageParseError {
                err: format!("Kuaishou API error: {result}"),
            });
        }
        if result == 671 || result == 677 {
            return Err(DanmuStreamError::MessageParseError {
                err: format!("Kuaishou room is not live: {result}"),
            });
        }
    }

    Ok(root)
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

    for pattern in patterns {
        if let Ok(regex) = Regex::new(pattern) {
            if let Some(captures) = regex.captures(html_str) {
                if let Some(value) = captures.get(1) {
                    let json_str = value.as_str().trim();
                    return Some(json_str.to_string());
                }
            }
        }
    }

    None
}

fn clean_json_state(raw: &str) -> String {
    let mut trimmed = raw.trim().trim_end_matches(';').trim().to_string();
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if end > start {
            trimmed = trimmed[start..=end].to_string();
        }
    }
    if trimmed.contains("undefined") {
        trimmed = trimmed.replace("undefined", "null");
    }
    trimmed
}

fn extract_live_stream_id_from_html(html_str: &str) -> Option<String> {
    let patterns = [
        r#""liveStreamId"\s*:\s*"([^"]+)""#,
        r#""liveStreamId"\s*:\s*(\d+)"#,
    ];
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(cap) = re.captures(html_str).and_then(|c| c.get(1)) {
                let value = cap.as_str().trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn extract_ws_token_from_html(html_str: &str) -> Option<String> {
    let patterns = [
        r#""websocketToken"\s*:\s*"([^"]+)""#,
        r#""webSocketToken"\s*:\s*"([^"]+)""#,
        r#""websocket_token"\s*:\s*"([^"]+)""#,
        r#""token"\s*:\s*"([^"]+)""#,
    ];
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(cap) = re.captures(html_str).and_then(|c| c.get(1)) {
                let value = cap.as_str().trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}


fn extract_live_stream_id_from_cookie(cookie: &str) -> Option<String> {
    let re = Regex::new(r"(?i)(?:^|;\s*)liveStreamId=([^;]+)").ok()?;
    re.captures(cookie)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|v| !v.is_empty())
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => {
            if s.trim().is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn extract_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = value.get(*key) {
            if let Some(s) = value_to_string(v) {
                return Some(s);
            }
        }
    }
    None
}

fn extract_live_stream_id(data: &Value) -> Option<String> {
    let direct_keys = ["liveStreamId", "live_stream_id", "liveStreamID"];
    if let Some(id) = extract_string_field(data, &direct_keys) {
        return Some(id);
    }

    let live_stream = data.get("liveStream").or_else(|| data.get("live_stream"))?;
    let nested_keys = ["id", "liveStreamId", "live_stream_id"];
    extract_string_field(live_stream, &nested_keys)
        .or_else(|| find_string_by_keys(data, &direct_keys))
}

fn extract_ws_token(data: &Value) -> Option<String> {
    let token_keys = ["token", "websocketToken", "webSocketToken", "websocket_token"];

    if let Some(token) = extract_string_field(data, &token_keys) {
        return Some(token);
    }

    for key in ["websocketInfo", "webSocketInfo", "websocket_info"] {
        if let Some(info) = data.get(key) {
            if let Some(token) = extract_string_field(info, &token_keys) {
                return Some(token);
            }
        }
    }

    if let Some(live_stream) = data.get("liveStream").or_else(|| data.get("live_stream")) {
        if let Some(token) = extract_string_field(live_stream, &token_keys) {
            return Some(token);
        }
        for key in ["websocketInfo", "webSocketInfo", "websocket_info"] {
            if let Some(info) = live_stream.get(key) {
                if let Some(token) = extract_string_field(info, &token_keys) {
                    return Some(token);
                }
            }
        }
    }

    find_string_by_keys(data, &token_keys)
}

fn extract_ws_token_from_text(text: &str) -> Option<String> {
    let named_re = Regex::new(
        r#"(?i)"(?:token|websocketToken|webSocketToken|websocket_token|authToken|accessToken)"\s*:\s*"([^"]{50,})""#,
    )
    .ok()?;
    if let Some(token) = named_re
        .captures(text)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .filter(|v| !v.trim().is_empty())
    {
        return Some(token);
    }
    let base64_re = Regex::new(r#""([A-Za-z0-9+/]{80,}={0,2})""#).ok()?;
    let token = base64_re
        .captures_iter(text)
        .filter_map(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .find(|token| token.len() >= 80);
    token
}

fn extract_ws_urls_from_text(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let re = Regex::new(r#"(?i)wss://(?:livejs-ws-group\d+\.gifshow\.com|live-ws-pg-group\d+\.kuaishou\.com)/websocket"#).ok();
    if let Some(re) = re {
        for cap in re.captures_iter(text) {
            if let Some(m) = cap.get(0) {
                push_ws_url(&mut urls, m.as_str());
            }
        }
    }
    urls
}

fn push_ws_url(urls: &mut Vec<String>, url: &str) {
    if url.trim().is_empty() {
        return;
    }
    if !urls.iter().any(|u| u == url) {
        urls.push(url.to_string());
    }
}

fn collect_ws_urls_from_list(list: &[Value], urls: &mut Vec<String>) {
    for item in list {
        if let Some(url) = item.as_str() {
            push_ws_url(urls, url);
            continue;
        }
        if let Some(url) = item.get("url").and_then(|v| v.as_str()) {
            push_ws_url(urls, url);
        }
        if let Some(url) = item.get("wsUrl").and_then(|v| v.as_str()) {
            push_ws_url(urls, url);
        }
        if let Some(url) = item.get("websocketUrl").and_then(|v| v.as_str()) {
            push_ws_url(urls, url);
        }
        if let Some(url) = item.get("webSocketUrl").and_then(|v| v.as_str()) {
            push_ws_url(urls, url);
        }
    }
}

fn find_string_by_keys(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(v) = map.get(*key) {
                    if let Some(s) = value_to_string(v) {
                        return Some(s);
                    }
                }
            }
            for v in map.values() {
                if let Some(s) = find_string_by_keys(v, keys) {
                    return Some(s);
                }
            }
            None
        }
        Value::Array(list) => {
            for v in list {
                if let Some(s) = find_string_by_keys(v, keys) {
                    return Some(s);
                }
            }
            None
        }
        _ => None,
    }
}

fn collect_ws_urls_recursive(value: &Value, urls: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if matches!(
                    k.as_str(),
                    "webSocketAddresses"
                        | "websocketUrls"
                        | "webSocketUrls"
                        | "websocketUrl"
                        | "webSocketUrl"
                ) {
                    if let Some(list) = v.as_array() {
                        collect_ws_urls_from_list(list, urls);
                    } else if let Some(url) = v.as_str() {
                        push_ws_url(urls, url);
                    }
                }
                collect_ws_urls_recursive(v, urls);
            }
        }
        Value::Array(list) => {
            for v in list {
                collect_ws_urls_recursive(v, urls);
            }
        }
        Value::String(s) => {
            if s.starts_with("ws://") || s.starts_with("wss://") {
                push_ws_url(urls, s);
            }
        }
        _ => {}
    }
}

fn extract_websocket_urls(data: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    let mut candidates = Vec::new();
    candidates.push(data);
    if let Some(live_stream) = data.get("liveStream").or_else(|| data.get("live_stream")) {
        candidates.push(live_stream);
    }
    for key in ["websocketInfo", "webSocketInfo", "websocket_info"] {
        if let Some(info) = data.get(key) {
            candidates.push(info);
        }
    }
    if let Some(live_stream) = data.get("liveStream").or_else(|| data.get("live_stream")) {
        for key in ["websocketInfo", "webSocketInfo", "websocket_info"] {
            if let Some(info) = live_stream.get(key) {
                candidates.push(info);
            }
        }
    }

    for candidate in candidates {
        if let Some(list) = candidate.as_array() {
            collect_ws_urls_from_list(list, &mut urls);
        }
        if let Some(url) = candidate.as_str() {
            push_ws_url(&mut urls, url);
        }
        if let Some(list) = candidate.get("webSocketAddresses").and_then(|v| v.as_array()) {
            collect_ws_urls_from_list(list, &mut urls);
        }
        if let Some(list) = candidate.get("websocketUrls").and_then(|v| v.as_array()) {
            collect_ws_urls_from_list(list, &mut urls);
        }
        if let Some(list) = candidate.get("webSocketUrls").and_then(|v| v.as_array()) {
            collect_ws_urls_from_list(list, &mut urls);
        }
        if let Some(url) = candidate.get("websocketUrl").and_then(|v| v.as_str()) {
            push_ws_url(&mut urls, url);
        }
        if let Some(url) = candidate.get("webSocketUrl").and_then(|v| v.as_str()) {
            push_ws_url(&mut urls, url);
        }
    }

    if urls.is_empty() {
        collect_ws_urls_recursive(data, &mut urls);
    }

    urls
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, DanmuStreamError> {
    let mut decoder = GzDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| DanmuStreamError::MessageParseError {
            err: e.to_string(),
        })?;
    Ok(out)
}

fn parse_color(color: &str) -> u32 {
    let trimmed = color.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let hex = trimmed.trim_start_matches('#');
    u32::from_str_radix(hex, 16).unwrap_or(0)
}

trait HeaderApply {
    fn apply_header(self, name: &str, value: Option<&str>) -> Self;
}

impl HeaderApply for reqwest::RequestBuilder {
    fn apply_header(self, name: &str, value: Option<&str>) -> Self {
        if let Some(value) = value {
            if !value.is_empty() {
                return self.header(name, value);
            }
        }
        self
    }
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
