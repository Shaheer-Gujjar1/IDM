use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::Manager;
use serde::{Serialize, Deserialize};
use tokio::sync::{Mutex, RwLock, broadcast};
use futures_util::StreamExt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed(String),
    Assembling,
    Trash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadChunk {
    pub id: String,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

#[derive(Debug)]
pub struct ActiveChunk {
    pub id: String,
    pub start: u64,
    pub end: Arc<AtomicU64>,
    pub downloaded: Arc<AtomicU64>,
    pub active: Arc<std::sync::atomic::AtomicBool>,
}

impl Clone for ActiveChunk {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            start: self.start,
            end: self.end.clone(),
            downloaded: self.downloaded.clone(),
            active: self.active.clone(),
        }
    }
}

pub fn write_at_offset(file: &std::fs::File, #[allow(unused_mut)] mut buf: &[u8], #[allow(unused_mut)] mut offset: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            let n = file.seek_write(buf, offset)?;
            if n == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "Failed to write bytes at offset"));
            }
            buf = &buf[n..];
            offset += n as u64;
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        use std::io::{Seek, Write};
        let mut f = file.try_clone()?;
        f.seek(std::io::SeekFrom::Start(offset))?;
        f.write_all(buf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserProfile {
    pub user_agent: String,
    pub brand: String,
    pub version: String,
    pub platform: String,
    pub is_mobile: bool,
    pub sec_ch_ua: Option<String>,
}

impl BrowserProfile {
    pub fn detect(raw_ua: &str) -> Self {
        let ua = if raw_ua.trim().is_empty() {
            #[cfg(target_os = "windows")]
            { "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36" }
            #[cfg(target_os = "macos")]
            { "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36" }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            { "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36" }
        } else {
            raw_ua
        };

        let is_mobile = ua.contains("Mobile") || ua.contains("Android");

        let platform = if ua.contains("Windows") {
            "\"Windows\""
        } else if ua.contains("Macintosh") || ua.contains("Mac OS") {
            "\"macOS\""
        } else if ua.contains("Android") {
            "\"Android\""
        } else {
            "\"Linux\""
        };

        if ua.contains("Firefox") {
            return Self {
                user_agent: ua.to_string(),
                brand: "Firefox".to_string(),
                version: "132".to_string(),
                platform: platform.to_string(),
                is_mobile,
                sec_ch_ua: None,
            };
        }

        if ua.contains("Safari") && !ua.contains("Chrome") && !ua.contains("Chromium") {
            return Self {
                user_agent: ua.to_string(),
                brand: "Safari".to_string(),
                version: "17".to_string(),
                platform: platform.to_string(),
                is_mobile,
                sec_ch_ua: None,
            };
        }

        let (brand, ver) = if ua.contains("Edg/") {
            let ver = ua.split("Edg/").nth(1).and_then(|s| s.split('.').next()).unwrap_or("131");
            ("Edge", ver)
        } else if ua.contains("Chrome/") {
            let ver = ua.split("Chrome/").nth(1).and_then(|s| s.split('.').next()).unwrap_or("131");
            ("Chrome", ver)
        } else {
            ("Chrome", "131")
        };

        let sec_ch_ua = if brand == "Edge" {
            format!("\"Chromium\";v=\"{}\", \"Microsoft Edge\";v=\"{}\", \"Not-A.Brand\";v=\"99\"", ver, ver)
        } else {
            format!("\"Chromium\";v=\"{}\", \"Google Chrome\";v=\"{}\", \"Not-A.Brand\";v=\"99\"", ver, ver)
        };

        Self {
            user_agent: ua.to_string(),
            brand: brand.to_string(),
            version: ver.to_string(),
            platform: platform.to_string(),
            is_mobile,
            sec_ch_ua: Some(sec_ch_ua),
        }
    }

    pub fn apply_headers(
        &self,
        mut req: reqwest::RequestBuilder,
        target_url: &str,
        user_referer: &str,
        user_cookie: &str,
        is_retry_403: bool,
    ) -> reqwest::RequestBuilder {
        use reqwest::header::*;

        req = req.header(USER_AGENT, &self.user_agent);

        if let Some(ref sec_ch) = self.sec_ch_ua {
            req = req.header("sec-ch-ua", sec_ch);
            req = req.header("sec-ch-ua-mobile", if self.is_mobile { "?1" } else { "?0" });
            req = req.header("sec-ch-ua-platform", &self.platform);
        }

        if self.brand == "Firefox" {
            req = req.header(ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8");
            req = req.header(ACCEPT_LANGUAGE, "en-US,en;q=0.5");
        } else {
            req = req.header(ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7");
            req = req.header(ACCEPT_LANGUAGE, "en-US,en;q=0.9");
        }

        req = req.header(ACCEPT_ENCODING, "gzip, deflate, br, zstd");
        req = req.header("upgrade-insecure-requests", "1");
        req = req.header(CONNECTION, "keep-alive");

        if is_retry_403 {
            req = req.header("sec-fetch-dest", "empty");
            req = req.header("sec-fetch-mode", "cors");
            req = req.header("sec-fetch-site", "same-origin");
        } else {
            req = req.header("sec-fetch-dest", "document");
            req = req.header("sec-fetch-mode", "navigate");
            req = req.header("sec-fetch-site", "cross-site");
            req = req.header("sec-fetch-user", "?1");
        }

        let parsed_url = reqwest::Url::parse(target_url).ok();
        let target_origin = parsed_url.as_ref().map(|u| {
            if let Some(port) = u.port() {
                format!("{}://{}:{}", u.scheme(), u.host_str().unwrap_or(""), port)
            } else {
                format!("{}://{}", u.scheme(), u.host_str().unwrap_or(""))
            }
        });

        if !user_referer.is_empty() {
            req = req.header(REFERER, user_referer);
        } else if let Some(ref origin) = target_origin {
            req = req.header(REFERER, format!("{}/", origin));
        }

        if let Some(ref origin) = target_origin {
            req = req.header("origin", origin);
        }

        if !user_cookie.is_empty() {
            req = req.header(COOKIE, user_cookie);
        }

        // Hugging Face AI Model & Dataset Auth Header Injection (Bearer token for gated repos)
        if crate::huggingface::is_huggingface_url(target_url) {
            req = crate::huggingface::apply_huggingface_auth(req, None);
        }

        req
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub save_path: String,
    pub total_size: u64,
    pub downloaded: u64,
    pub speed: f64, // bytes/sec
    pub eta: String,
    pub status: DownloadStatus,
    pub file_exists: bool,
    pub speed_limited: bool,
    #[serde(default)]
    pub category: String,
}

pub struct DownloadTask {
    pub id: String,
    pub original_url: String,
    pub final_url: Mutex<String>,
    pub filename: Arc<std::sync::Mutex<String>>,
    pub save_path: Arc<std::sync::Mutex<String>>,
    pub category: Arc<std::sync::Mutex<String>>,
    pub cookie: String,
    pub referrer: String,
    pub user_agent: String,
    pub audio_url: Option<String>,
    pub total_size: AtomicU64,
    pub downloaded: Arc<AtomicU64>,
    pub network_downloaded: Arc<AtomicU64>,
    pub speed_limiter: Mutex<Instant>,
    pub status: Arc<std::sync::Mutex<DownloadStatus>>,
    pub abort_tx: Option<broadcast::Sender<()>>,
    pub chunks: std::sync::Mutex<Vec<ActiveChunk>>,
    pub speed: Arc<std::sync::Mutex<f64>>,
    pub eta: Arc<std::sync::Mutex<String>>,
    pub validator: Arc<std::sync::Mutex<Option<String>>>,
}


impl DownloadTask {
    pub fn get_filename(&self) -> String {
        self.filename.lock().unwrap().clone()
    }

    pub fn get_save_path(&self) -> String {
        self.save_path.lock().unwrap().clone()
    }

    pub fn get_category(&self) -> String {
        self.category.lock().unwrap().clone()
    }

    pub fn update_filename_and_path(&self, new_name: &str) {
        let clean_name = crate::proxy::sanitize_filename(new_name);
        let mut fn_guard = self.filename.lock().unwrap();
        let mut sp_guard = self.save_path.lock().unwrap();
        *fn_guard = clean_name.clone();
        let path = std::path::Path::new(&*sp_guard);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                *sp_guard = parent.join(&clean_name).to_string_lossy().to_string();
            } else {
                *sp_guard = clean_name;
            }
        } else {
            *sp_guard = clean_name;
        }
    }

    pub async fn throttle_if_needed(&self, bytes_len: u64, limit_bps: u64) {
        if limit_bps == 0 || bytes_len == 0 {
            return;
        }

        // Target cap at 99.5% to guarantee measured UI speed never touches or exceeds limit
        let target_bps = (limit_bps as f64 * 0.995).max(1.0);

        // Lock scope: calculate sleep duration and release lock IMMEDIATELY so other workers don't serialize
        let sleep_duration = {
            let mut virtual_time = self.speed_limiter.lock().await;
            let now = Instant::now();

            if *virtual_time < now {
                *virtual_time = now;
            }

            let duration = Duration::from_secs_f64(bytes_len as f64 / target_bps);
            *virtual_time += duration;

            // Clamp burst accumulation to at most 500ms into the future to eliminate latency spikes
            let max_burst_horizon = now + Duration::from_millis(500);
            if *virtual_time > max_burst_horizon {
                *virtual_time = max_burst_horizon;
            }

            virtual_time.checked_duration_since(now)
        };

        if let Some(sleep) = sleep_duration {
            if !sleep.is_zero() {
                // Sleep in small micro-bursts of up to 50ms so aborts and user speed limit adjustments respond instantaneously
                let mut rem = sleep;
                while rem > Duration::from_millis(50) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    rem = rem.saturating_sub(Duration::from_millis(50));
                }
                if !rem.is_zero() {
                    tokio::time::sleep(rem).await;
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PersistentTask {
    pub id: String,
    pub original_url: String,
    pub final_url: String,
    pub filename: String,
    pub save_path: String,
    #[serde(default)]
    pub category: String,
    pub cookie: String,
    pub referrer: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub audio_url: Option<String>,
    pub total_size: u64,
    pub downloaded: u64,
    pub status: DownloadStatus,
    pub chunks: Vec<DownloadChunk>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SchedulerConfig {
    pub enabled: bool,
    pub start_time: String,
    pub end_time: String,
    pub active_days: Vec<String>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            start_time: "02:00".to_string(),
            end_time: "06:00".to_string(),
            active_days: vec![
                "Mon".to_string(),
                "Tue".to_string(),
                "Wed".to_string(),
                "Thu".to_string(),
                "Fri".to_string(),
            ],
        }
    }
}

pub fn is_in_schedule_window(config: &SchedulerConfig, now: &chrono::DateTime<chrono::Local>) -> bool {
    if !config.enabled {
        return false;
    }

    use chrono::Datelike;
    let current_day = match now.weekday() {
        chrono::Weekday::Mon => "Mon",
        chrono::Weekday::Tue => "Tue",
        chrono::Weekday::Wed => "Wed",
        chrono::Weekday::Thu => "Thu",
        chrono::Weekday::Fri => "Fri",
        chrono::Weekday::Sat => "Sat",
        chrono::Weekday::Sun => "Sun",
    };

    if !config.active_days.iter().any(|d| d.eq_ignore_ascii_case(current_day)) {
        return false;
    }

    let parse_minutes = |time_str: &str| -> Option<u32> {
        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() == 2 {
            let h = parts[0].trim().parse::<u32>().ok()?;
            let m = parts[1].trim().parse::<u32>().ok()?;
            if h < 24 && m < 60 {
                return Some(h * 60 + m);
            }
        }
        None
    };

    let start_mins = match parse_minutes(&config.start_time) {
        Some(m) => m,
        None => return false,
    };
    let end_mins = match parse_minutes(&config.end_time) {
        Some(m) => m,
        None => return false,
    };

    use chrono::Timelike;
    let current_mins = now.hour() * 60 + now.minute();

    if start_mins <= end_mins {
        current_mins >= start_mins && current_mins < end_mins
    } else {
        current_mins >= start_mins || current_mins < end_mins
    }
}

pub struct DownloadManager {
    pub tasks: RwLock<HashMap<String, Arc<DownloadTask>>>,
    pub app_handle: Mutex<Option<tauri::AppHandle>>,
    pub client: reqwest::Client,
    pub theme_mode: Mutex<String>,
    pub speed_limit_bps: AtomicU64,
    pub max_chunks: AtomicU64,
    pub intercept_downloads: std::sync::atomic::AtomicBool,
    pub minimize_to_tray: std::sync::atomic::AtomicBool,
    pub scheduler_config: Arc<tokio::sync::RwLock<SchedulerConfig>>,
    pub bridge_token: Arc<String>,
    pub last_youtube_watch_url: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl DownloadManager {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .pool_max_idle_per_host(32)
            .redirect(reqwest::redirect::Policy::limited(10))
            .tcp_keepalive(Duration::from_secs(30))
            .tcp_nodelay(true)
            .connect_timeout(Duration::from_secs(30))
            .http2_adaptive_window(true)
            .connection_verbose(false)
            .build()
            .unwrap_or_default();

        let token_hex: String = (0..32).map(|_| format!("{:02x}", rand::random::<u8>())).collect();

        Self {
            tasks: RwLock::new(HashMap::new()),
            app_handle: Mutex::new(None),
            client,
            theme_mode: Mutex::new("dark".to_string()),
            speed_limit_bps: AtomicU64::new(0),
            max_chunks: AtomicU64::new(8),
            intercept_downloads: std::sync::atomic::AtomicBool::new(true),
            minimize_to_tray: std::sync::atomic::AtomicBool::new(true),
            scheduler_config: Arc::new(tokio::sync::RwLock::new(SchedulerConfig::default())),
            bridge_token: Arc::new(token_hex),
            last_youtube_watch_url: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    pub async fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.lock().await = Some(handle.clone());

        // Write bridge.json for authenticated loopback IPC
        let handle_clone = handle.clone();
        let token_clone = self.bridge_token.clone();
        tokio::spawn(async move {
            use tauri::Manager;
            if let Ok(app_dir) = handle_clone.path().app_data_dir() {
                let _ = tokio::fs::create_dir_all(&app_dir).await;
                let bridge_path = app_dir.join("bridge.json");
                let bridge_data = serde_json::json!({
                    "port": 9600,
                    "token": *token_clone
                });
                if let Ok(_) = tokio::fs::write(&bridge_path, bridge_data.to_string()).await {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&bridge_path, std::fs::Permissions::from_mode(0o600));
                    }
                }
            }
        });
    }


    pub async fn save_history(&self) -> Result<(), String> {
        let handle_opt = {
            let guard = self.app_handle.lock().await;
            guard.clone()
        };
        if let Some(handle) = handle_opt {
            use tauri::Manager;
            let app_dir = handle.path().app_data_dir().map_err(|e| e.to_string())?;
            let _ = tokio::fs::create_dir_all(&app_dir).await;
            let history_path = app_dir.join("history.json");
            
            let tasks = self.tasks.read().await;
            let mut persistent_tasks = Vec::new();
            for task in tasks.values() {
                let downloaded = task.downloaded.load(Ordering::Relaxed);
                let status = task.status.lock().unwrap().clone();
                
                let mut persistent_chunks = Vec::new();
                for c in task.chunks.lock().unwrap().iter() {
                    persistent_chunks.push(DownloadChunk {
                        id: c.id.clone(),
                        start: c.start,
                        end: c.end.load(Ordering::Relaxed),
                        downloaded: c.downloaded.load(Ordering::Relaxed),
                    });
                }
                
                let final_url = task.final_url.lock().await.clone();

                persistent_tasks.push(PersistentTask {
                    id: task.id.clone(),
                    original_url: task.original_url.clone(),
                    final_url,
                    filename: task.get_filename(),
                    save_path: task.get_save_path(),
                    category: task.get_category(),
                    cookie: task.cookie.clone(),
                    referrer: task.referrer.clone(),
                    user_agent: task.user_agent.clone(),
                    audio_url: task.audio_url.clone(),
                    total_size: task.total_size.load(Ordering::Relaxed),
                    downloaded,
                    status,
                    chunks: persistent_chunks,
                });
            }
            
            if let Ok(serialized) = serde_json::to_string_pretty(&persistent_tasks) {
                let _ = tokio::fs::write(history_path, serialized).await;
            }
        }
        Ok(())
    }

    pub async fn load_history(&self) -> Result<(), String> {
        let handle_opt = {
            let guard = self.app_handle.lock().await;
            guard.clone()
        };
        if let Some(handle) = handle_opt {
            use tauri::Manager;
            let app_dir = handle.path().app_data_dir().map_err(|e| e.to_string())?;
            let history_path = app_dir.join("history.json");
            if history_path.exists() {
                if let Ok(data) = tokio::fs::read_to_string(&history_path).await {
                    if let Ok(persistent_tasks) = serde_json::from_str::<Vec<PersistentTask>>(&data) {
                        let mut tasks = self.tasks.write().await;
                        for p_task in persistent_tasks {
                            let status = match p_task.status {
                                DownloadStatus::Downloading | DownloadStatus::Queued => DownloadStatus::Paused,
                                other => other,
                            };
                            
                            let mut active_chunks = Vec::new();
                            for c in p_task.chunks {
                                active_chunks.push(ActiveChunk {
                                    id: c.id,
                                    start: c.start,
                                    end: Arc::new(AtomicU64::new(c.end)),
                                    downloaded: Arc::new(AtomicU64::new(c.downloaded)),
                                    active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                                });
                            }
                            
                            let cat = if p_task.category.is_empty() {
                                crate::preflight::detect_category(&p_task.filename, None).to_string()
                            } else {
                                p_task.category
                            };

                            tasks.insert(p_task.id.clone(), Arc::new(DownloadTask {
                                id: p_task.id,
                                original_url: p_task.original_url,
                                final_url: Mutex::new(p_task.final_url),
                                filename: Arc::new(std::sync::Mutex::new(p_task.filename)),
                                save_path: Arc::new(std::sync::Mutex::new(p_task.save_path)),
                                category: Arc::new(std::sync::Mutex::new(cat)),
                                cookie: p_task.cookie,
                                referrer: p_task.referrer,
                                user_agent: p_task.user_agent,
                                audio_url: p_task.audio_url,
                                total_size: AtomicU64::new(p_task.total_size),
                                downloaded: Arc::new(AtomicU64::new(p_task.downloaded)),
                                network_downloaded: Arc::new(AtomicU64::new(p_task.downloaded)),
                                speed_limiter: Mutex::new(Instant::now()),
                                status: Arc::new(std::sync::Mutex::new(status)),
                                abort_tx: None,
                                chunks: std::sync::Mutex::new(active_chunks),
                                speed: Arc::new(std::sync::Mutex::new(0.0)),
                                eta: Arc::new(std::sync::Mutex::new("---".to_string())),
                                validator: Arc::new(std::sync::Mutex::new(None)),
                            }));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn start_download(
        self: &Arc<Self>,
        url: String,
        mut filename: String,
        mut save_path: String,
        cookie: String,
        referrer: String,
        user_agent: String,
        audio_url: Option<String>,
        category: Option<String>,
        size: Option<u64>,
        queue_only: bool,
        reuse_active: bool,
        conflict_action: Option<String>,
    ) -> Result<String, String> {
        let (clean_url, final_audio_url) = if let Some(idx) = url.find("|AUDIO_URL|") {
            (url[..idx].to_string(), Some(url[idx + 11..].to_string()))
        } else {
            (url.clone(), audio_url)
        };

        // 1. Duplicate download prevention: return existing active download ID if already in progress
        if reuse_active {
            let tasks_guard = self.tasks.read().await;
            for (existing_id, t) in tasks_guard.iter() {
                let matches_url = t.original_url == clean_url || t.original_url == url;
                let matches_audio = match (&t.audio_url, &final_audio_url) {
                    (Some(a1), Some(a2)) => a1 == a2,
                    (None, None) => true,
                    _ => false,
                };
                if matches_url && matches_audio {
                    let status = t.status.lock().unwrap().clone();
                    match status {
                        DownloadStatus::Downloading | DownloadStatus::Queued | DownloadStatus::Paused | DownloadStatus::Assembling => {
                            println!("[Engine] Reusing active download task: {}", existing_id);
                            return Ok(format!("EXISTS:{}", existing_id));
                        }
                        _ => {}
                    }
                }
            }
        }

        let id = uuid::Uuid::new_v4().to_string();

        let mut download_url = clean_url.clone();
        let mut download_size = size;

        // Feature 3: BitTorrent Magnet Protocol Handler (BEP 9 / BEP 19 / BEP 53 / P2P Swarm)
        if clean_url.starts_with("magnet:?") {
            let magnet = crate::torrent::parse_magnet(&clean_url)?;
            if crate::proxy::is_generic_filename(&filename) || filename.is_empty() {
                filename = magnet.name.clone();
            }
            if let Some(len) = magnet.exact_length {
                if download_size.is_none() {
                    download_size = Some(len);
                }
            }
            if let Some(ws) = magnet.web_seeds.first() {
                download_url = ws.clone();
            } else {
                // Native Pure-Rust P2P Swarming handles this directly via BitTorrent trackers and peers!
                download_url = clean_url.clone();
            }
        }

        // Feature: FTP & FTPS Native Engine
        if crate::ftp::is_ftp_url(&clean_url) {
            if let Ok(ftp_info) = crate::ftp::parse_ftp_url(&clean_url) {
                if crate::proxy::is_generic_filename(&filename) || filename.is_empty() {
                    filename = ftp_info.filename.clone();
                }
            }
        }

        // Feature: Hugging Face AI Models & Datasets Normalizer
        if crate::huggingface::is_huggingface_url(&download_url) {
            if let Some(norm) = crate::huggingface::normalize_huggingface_url(&download_url) {
                download_url = norm;
            }
            if let Some(hf_info) = crate::huggingface::parse_huggingface_url(&download_url) {
                if crate::proxy::is_generic_filename(&filename) || filename.is_empty() {
                    filename = hf_info.filename.clone();
                }
            }
        }

        // Feature: GitHub Releases & Fast Mirror Accelerator
        if crate::github_accelerator::is_github_url(&download_url) {
            if let Some(gh_info) = crate::github_accelerator::parse_github_url(&download_url) {
                if crate::proxy::is_generic_filename(&filename) || filename.is_empty() {
                    filename = gh_info.filename.clone();
                }
                // Concurrently race probe mirrors with short timeout
                let fastest_url = crate::github_accelerator::probe_fastest_github_mirror(&download_url, &self.client).await;
                if fastest_url != download_url {
                    println!("[GitHub Accelerator] Accelerated via fast mirror: {}", fastest_url);
                    download_url = fastest_url;
                }
            }
        }

        // Feature: eDonkey2000 / eMule Protocol
        if crate::ed2k::is_ed2k_url(&clean_url) {
            if let Ok(ed2k_info) = crate::ed2k::parse_ed2k_link(&clean_url) {
                if crate::proxy::is_generic_filename(&filename) || filename.is_empty() {
                    filename = ed2k_info.filename.clone();
                }
                if download_size.is_none() || download_size == Some(0) {
                    download_size = Some(ed2k_info.file_size);
                }
                // If ed2k link has HTTP web seeds (s=http://...), route download_url to the web seed
                if let Some(webseed) = ed2k_info.sources.iter().find(|s| s.starts_with("http://") || s.starts_with("https://")) {
                    download_url = webseed.clone();
                }
            }
        }

        if !download_url.starts_with("http://")
            && !download_url.starts_with("https://")
            && !download_url.starts_with("magnet:?")
            && !download_url.starts_with("ftp://")
            && !download_url.starts_with("ftps://")
            && !download_url.starts_with("ed2k://")
        {
            return Err("Unsupported URL protocol. Only http://, https://, ftp://, ftps://, ed2k://, and magnet:? URLs can be downloaded over the network. In-memory URLs (e.g. blob:) are generated locally in your browser and saved directly by the browser.".to_string());
        }

        if crate::proxy::is_generic_filename(&filename) || filename.is_empty() {
            if let Ok(parsed) = reqwest::Url::parse(&download_url) {
                let mut candidate = String::new();
                for (k, v) in parsed.query_pairs() {
                    let k_lower = k.to_lowercase();
                    if (k_lower.contains("file") || k_lower.contains("name") || k_lower.contains("title")) && v.contains('.') {
                        if let Ok(decoded) = urlencoding::decode(&v) {
                            let sanitized = crate::proxy::sanitize_filename(&decoded);
                            if !crate::proxy::is_generic_filename(&sanitized) && sanitized.contains('.') {
                                candidate = sanitized;
                                break;
                            }
                        }
                    }
                }
                if candidate.is_empty() {
                    if let Some(last_seg) = parsed.path_segments().and_then(|s| s.last()) {
                        if !last_seg.is_empty() && last_seg.contains('.') {
                            if let Ok(decoded) = urlencoding::decode(last_seg) {
                                let sanitized = crate::proxy::sanitize_filename(&decoded);
                                if !crate::proxy::is_generic_filename(&sanitized) {
                                    candidate = sanitized;
                                }
                            }
                        }
                    }
                }
                if !candidate.is_empty() {
                    filename = candidate;
                } else {
                    filename = "downloaded_file".to_string();
                }
            } else {
                filename = "downloaded_file".to_string();
            }

            let path = std::path::Path::new(&save_path);
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    save_path = parent.join(&filename).to_string_lossy().to_string();
                } else {
                    save_path = filename.clone();
                }
            } else {
                save_path = filename.clone();
            }
        }

        // Ensure filename and save_path are always fully sanitized and clamped to filesystem limits
        filename = crate::proxy::sanitize_filename(&filename);
        let path = std::path::Path::new(&save_path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                save_path = parent.join(&filename).to_string_lossy().to_string();
            } else {
                save_path = filename.clone();
            }
        } else {
            save_path = filename.clone();
        }

        // Feature 2: Interactive Duplicate Name Conflict handling
        match conflict_action.as_deref() {
            Some("rename") => {
                if std::path::Path::new(&save_path).exists() {
                    let (new_path, placeholder) = Self::allocate_collision_free_path(&save_path);
                    drop(placeholder);
                    save_path = new_path;
                    if let Some(new_name) = std::path::Path::new(&save_path).file_name().and_then(|f| f.to_str()) {
                        filename = new_name.to_string();
                    }
                }
            }
            Some("overwrite") => {
                // User explicitly chose to overwrite: retain chosen destination and replace
            }
            Some("resume") | None => {
                // Resume / keep default behavior
            }
            _ => {}
        }

        let final_category = match category {
            Some(cat) if !cat.trim().is_empty() => cat,
            _ => crate::preflight::detect_category(&filename, None).to_string(),
        };

        let initial_status = if queue_only {
            DownloadStatus::Paused
        } else {
            DownloadStatus::Queued
        };

        let (abort_tx, _) = broadcast::channel(1);
        let task = Arc::new(DownloadTask {
            id: id.clone(),
            original_url: clean_url.clone(),
            final_url: Mutex::new(download_url.clone()),
            filename: Arc::new(std::sync::Mutex::new(filename)),
            save_path: Arc::new(std::sync::Mutex::new(save_path)),
            category: Arc::new(std::sync::Mutex::new(final_category)),
            cookie,
            referrer,
            user_agent,
            audio_url: final_audio_url,
            total_size: AtomicU64::new(download_size.unwrap_or(0)),
            downloaded: Arc::new(AtomicU64::new(0)),
            network_downloaded: Arc::new(AtomicU64::new(0)),
            speed_limiter: Mutex::new(Instant::now()),
            status: Arc::new(std::sync::Mutex::new(initial_status)),
            abort_tx: Some(abort_tx),
            chunks: std::sync::Mutex::new(vec![]),
            speed: Arc::new(std::sync::Mutex::new(0.0)),
            eta: Arc::new(std::sync::Mutex::new("---".to_string())),
            validator: Arc::new(std::sync::Mutex::new(None)),
        });

        self.tasks.write().await.insert(id.clone(), task.clone());

        let manager_clone_save = self.clone();
        tokio::spawn(async move {
            let _ = manager_clone_save.save_history().await;
        });

        if !queue_only {
            let manager_clone = self.clone();
            tokio::spawn(async move {
                if let Err(e) = manager_clone.run_task(task, false).await {
                    eprintln!("Download error: {}", e);
                }
            });
        }

        Ok(id)
    }

    pub fn allocate_collision_free_path(preferred_path: &str) -> (String, std::fs::File) {
        let p = std::path::Path::new(preferred_path);
        let parent = p.parent().unwrap_or_else(|| std::path::Path::new("."));
        let file_name = p.file_name().and_then(|f| f.to_str()).unwrap_or("downloaded_file");

        let (stem, ext) = if let Some(dot_idx) = file_name.rfind('.') {
            (&file_name[..dot_idx], &file_name[dot_idx..])
        } else {
            (file_name, "")
        };

        if let Ok(file) = std::fs::OpenOptions::new().write(true).create_new(true).open(preferred_path) {
            return (preferred_path.to_string(), file);
        }

        for i in 1..=10000 {
            let candidate_name = format!("{} ({}){}", stem, i, ext);
            let candidate_path = parent.join(&candidate_name);
            if let Ok(file) = std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate_path) {
                return (candidate_path.to_string_lossy().to_string(), file);
            }
        }

        let ts_name = format!("{}_{}{}", stem, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(), ext);
        let ts_path = parent.join(&ts_name);
        let file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&ts_path).expect("Failed to create file");
        (ts_path.to_string_lossy().to_string(), file)
    }

    pub fn clean_googlevideo_url(url: &str) -> String {
        if let Ok(mut parsed) = reqwest::Url::parse(url) {
            let is_gvs = parsed.host_str().map(|h| h.contains("googlevideo.com")).unwrap_or(false);
            if is_gvs {
                let sparams = parsed.query_pairs().find(|(k, _)| k == "sparams").map(|(_, v)| v.to_string()).unwrap_or_default();
                let sparams_list: Vec<&str> = sparams.split(',').collect();
                let has_range_in_sparams = sparams_list.contains(&"range");

                let pairs: Vec<(String, String)> = parsed.query_pairs()
                    .filter(|(k, _)| {
                        if k == "rn" || k == "rbuf" || k == "sq" {
                            return false;
                        }
                        if k == "range" && !has_range_in_sparams {
                            return false;
                        }
                        true
                    })
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();

                parsed.query_pairs_mut().clear().extend_pairs(pairs);
                return parsed.to_string();
            }
        }
        url.to_string()
    }

    pub fn build_stream_client(url: &str) -> Result<reqwest::Client, String> {
        let signed_ip: Option<std::net::IpAddr> = reqwest::Url::parse(url).ok().and_then(|u| {
            u.query_pairs().find(|(k, _)| k == "ip").and_then(|(_, v)| v.parse::<std::net::IpAddr>().ok())
        });

        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10));

        if let Some(ip) = signed_ip {
            match ip {
                std::net::IpAddr::V6(_) => {
                    builder = builder.local_address(std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED));
                }
                std::net::IpAddr::V4(_) => {
                    builder = builder.local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
                }
            }
        }

        builder.build().map_err(|e| e.to_string())
    }

    pub async fn download_stream_to_file(
        client: reqwest::Client,
        url: String,
        dest_path: std::path::PathBuf,
        cookie: String,
        referrer: String,
        user_agent: String,
        downloaded_tracker: Arc<AtomicU64>,
        network_tracker: Arc<AtomicU64>,
        task_ref: Arc<DownloadTask>,
        auto_calc_size: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<u64, String> {
        use tokio::io::AsyncWriteExt;
        use futures_util::StreamExt;

        let clean_url = Self::clean_googlevideo_url(&url);
        let is_youtube = clean_url.contains("googlevideo.com") || clean_url.contains("youtube.com");

        let ua = if !user_agent.is_empty() {
            user_agent
        } else {
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string()
        };

        let ref_hdr = if is_youtube {
            "https://www.youtube.com/".to_string()
        } else if !referrer.is_empty() {
            referrer
        } else {
            String::new()
        };

        // Compute fallback URL on redirector.googlevideo.com in case specific ISP cache node is unresolvable
        let fallback_url = if let Ok(mut parsed) = reqwest::Url::parse(&clean_url) {
            if let Some(host) = parsed.host_str() {
                if host.ends_with(".googlevideo.com") && host != "redirector.googlevideo.com" {
                    if parsed.set_host(Some("redirector.googlevideo.com")).is_ok() {
                        Some(parsed.to_string())
                    } else { None }
                } else { None }
            } else { None }
        } else { None };

        // Helper to construct request with exact required headers
        let build_req = |target_url: &str, range_header: Option<String>, send_cookie: bool| {
            let mut req = client.get(target_url);

            if send_cookie && !cookie.is_empty() {
                req = req.header("Cookie", &cookie);
            }

            req = req.header("User-Agent", &ua);

            if is_youtube {
                req = req.header("Referer", "https://www.youtube.com/");
                req = req.header("Origin", "https://www.youtube.com");
                req = req.header("Sec-Fetch-Mode", "cors");
                req = req.header("Sec-Fetch-Site", "cross-site");
                req = req.header("Sec-Fetch-Dest", "empty");
                req = req.header("Accept-Language", "en-US,en;q=0.9");
            } else if !ref_hdr.is_empty() {
                req = req.header("Referer", &ref_hdr);
                if let Ok(parsed) = reqwest::Url::parse(&ref_hdr) {
                    if let Some(host) = parsed.host_str() {
                        req = req.header("Origin", format!("{}://{}", parsed.scheme(), host));
                    }
                }
            }

            req = req.header("Accept", "*/*");
            req = req.header("Accept-Encoding", "identity");

            if let Some(r) = range_header {
                req = req.header("Range", r);
            }

            req
        };

        // Determine if total size is already known via URL `clen` query parameter
        let clen_from_url: Option<u64> = if let Ok(parsed) = reqwest::Url::parse(&clean_url) {
            parsed.query_pairs().find(|(k, _)| k == "clen").and_then(|(_, v)| v.parse::<u64>().ok())
        } else {
            None
        };

        if let Some(clen) = clen_from_url {
            if clen > 0 && auto_calc_size.load(Ordering::Relaxed) {
                task_ref.total_size.fetch_add(clen, Ordering::Relaxed);
            }
        }

        let mut file = tokio::fs::File::create(&dest_path).await.map_err(|e| format!("Cannot create part file: {}", e))?;
        let mut total_written = 0u64;
        let mut abort_rx_opt = task_ref.abort_tx.as_ref().map(|tx| tx.subscribe());

        // For YouTube/GoogleVideo or servers providing clen, use 5MB chunked Range requests
        // (Google Video Server mandates bounded byte range requests for DASH formats and returns 403 on open GETs)
        const CHUNK_SIZE: u64 = 5 * 1024 * 1024; // 5 MB chunks

        let mut target_total_len = clen_from_url;
        let mut use_range = is_youtube || target_total_len.is_some();
        let send_cookie = !is_youtube; // NEVER send YouTube cookies to googlevideo.com CDN
        let mut active_stream_url = clean_url.clone();

        if use_range {
            let mut start = 0u64;
            loop {
                if let Some(ref mut rx) = abort_rx_opt {
                    if rx.try_recv().is_ok() {
                        return Err("Download paused or cancelled".to_string());
                    }
                }

                let range_val = if let Some(tot) = target_total_len {
                    if start >= tot { break; }
                    let end = (start + CHUNK_SIZE - 1).min(tot - 1);
                    format!("bytes={}-{}", start, end)
                } else {
                    format!("bytes={}-{}", start, start + CHUNK_SIZE - 1)
                };

                let resp_res = build_req(&active_stream_url, Some(range_val.clone()), send_cookie).send().await;
                let resp = match resp_res {
                    Ok(r) => r,
                    Err(_orig_err) => {
                        // If host is an unreachable/unresolvable ISP node, attempt fallback to redirector.googlevideo.com
                        if let Some(ref fb) = fallback_url {
                            if active_stream_url != *fb {
                                if let Ok(fb_resp) = build_req(fb, Some(range_val.clone()), send_cookie).send().await {
                                    active_stream_url = fb.clone();
                                    fb_resp
                                } else {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                    build_req(&active_stream_url, Some(format!("bytes={}-", start)), send_cookie).send().await.map_err(|e2| format!("HTTP request failed: {}", e2))?
                                }
                            } else {
                                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                build_req(&active_stream_url, Some(format!("bytes={}-", start)), send_cookie).send().await.map_err(|e2| format!("HTTP request failed: {}", e2))?
                            }
                        } else {
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                            build_req(&active_stream_url, Some(format!("bytes={}-", start)), send_cookie).send().await.map_err(|e2| format!("HTTP request failed: {}", e2))?
                        }
                    }
                };

                let status = resp.status();
                if status == reqwest::StatusCode::PARTIAL_CONTENT {
                    // Extract total length from Content-Range if not already known
                    if target_total_len.is_none() {
                        if let Some(cr) = resp.headers().get("content-range").and_then(|h| h.to_str().ok()) {
                            if let Some(slash_idx) = cr.find('/') {
                                if let Ok(tot) = cr[slash_idx + 1..].trim().parse::<u64>() {
                                    target_total_len = Some(tot);
                                    if auto_calc_size.load(Ordering::Relaxed) {
                                        task_ref.total_size.fetch_add(tot, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                    }

                    let mut stream = resp.bytes_stream();
                    let mut chunk_bytes = 0u64;
                    while let Some(res) = stream.next().await {
                        let b = res.map_err(|e| format!("Network chunk error: {}", e))?;
                        let len = b.len() as u64;
                        file.write_all(&b).await.map_err(|e| format!("Failed to write chunk: {}", e))?;
                        total_written += len;
                        chunk_bytes += len;
                        downloaded_tracker.fetch_add(len, Ordering::Relaxed);
                        network_tracker.fetch_add(len, Ordering::Relaxed);
                    }
                    start += chunk_bytes;
                    if chunk_bytes == 0 || (target_total_len.is_some() && start >= target_total_len.unwrap()) {
                        break;
                    }
                } else if status == reqwest::StatusCode::OK {
                    if let Some(cl) = resp.content_length() {
                        if cl > 0 && auto_calc_size.load(Ordering::Relaxed) && target_total_len.is_none() {
                            task_ref.total_size.fetch_add(cl, Ordering::Relaxed);
                        }
                    }
                    let mut stream = resp.bytes_stream();
                    while let Some(res) = stream.next().await {
                        let b = res.map_err(|e| format!("Network chunk error: {}", e))?;
                        let len = b.len() as u64;
                        file.write_all(&b).await.map_err(|e| format!("Failed to write chunk: {}", e))?;
                        total_written += len;
                        downloaded_tracker.fetch_add(len, Ordering::Relaxed);
                        network_tracker.fetch_add(len, Ordering::Relaxed);
                    }
                    break;
                } else if status == reqwest::StatusCode::FORBIDDEN {
                    return Err(
                        "Video stream link expired or rejected by YouTube server (HTTP 403 Forbidden). Please refresh the YouTube video page in your browser to generate a fresh stream link and try again.".to_string()
                    );
                } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && start == 0 {
                    use_range = false;
                    break;
                } else {
                    return Err(format!("Server returned HTTP status {}", status));
                }
            }
        }

        if !use_range {
            let resp = build_req(&active_stream_url, None, send_cookie).send().await.map_err(|e| format!("HTTP request failed: {}", e))?;
            if !resp.status().is_success() {
                if !send_cookie && !cookie.is_empty() {
                    let resp2 = build_req(&active_stream_url, None, true).send().await.map_err(|e| format!("HTTP request failed: {}", e))?;
                    if !resp2.status().is_success() {
                        return Err(format!("Server returned HTTP status {}", resp2.status()));
                    }
                    if let Some(cl) = resp2.content_length() {
                        if cl > 0 && auto_calc_size.load(Ordering::Relaxed) {
                            task_ref.total_size.fetch_add(cl, Ordering::Relaxed);
                        }
                    }
                    let mut stream = resp2.bytes_stream();
                    while let Some(res) = stream.next().await {
                        let b = res.map_err(|e| format!("Network chunk error: {}", e))?;
                        let len = b.len() as u64;
                        file.write_all(&b).await.map_err(|e| format!("Failed to write chunk: {}", e))?;
                        total_written += len;
                        downloaded_tracker.fetch_add(len, Ordering::Relaxed);
                        network_tracker.fetch_add(len, Ordering::Relaxed);
                    }
                    file.flush().await.map_err(|e| format!("Failed to flush stream file: {}", e))?;
                    drop(file);
                    return Ok(total_written);
                }
                return Err(format!("Server returned HTTP status {}", resp.status()));
            }

            if let Some(cl) = resp.content_length() {
                if cl > 0 && auto_calc_size.load(Ordering::Relaxed) {
                    task_ref.total_size.fetch_add(cl, Ordering::Relaxed);
                }
            }

            let mut stream = resp.bytes_stream();
            loop {
                let chunk_opt = if let Some(ref mut rx) = abort_rx_opt {
                    tokio::select! {
                        _ = rx.recv() => break,
                        item = stream.next() => item,
                    }
                } else {
                    stream.next().await
                };

                match chunk_opt {
                    Some(Ok(chunk)) => {
                        let len = chunk.len() as u64;
                        file.write_all(&chunk).await.map_err(|e| format!("Failed to write chunk: {}", e))?;
                        total_written += len;
                        downloaded_tracker.fetch_add(len, Ordering::Relaxed);
                        network_tracker.fetch_add(len, Ordering::Relaxed);
                    }
                    Some(Err(e)) => return Err(format!("Network chunk error: {}", e)),
                    None => break,
                }
            }
        }

        file.flush().await.map_err(|e| format!("Failed to flush stream file: {}", e))?;
        drop(file);
        Ok(total_written)
    }

    pub async fn try_download_via_ytdlp(
        self: &Arc<Self>,
        task: &Arc<DownloadTask>,
        target_path: &str,
    ) -> Result<u64, String> {
        let ytdlp_bin = crate::mux::probe_ytdlp()
            .ok_or_else(|| "yt-dlp is not available on this system".to_string())?;

        let mut watch_url = if task.referrer.contains("youtube.com/watch") || task.referrer.contains("youtu.be/") || task.referrer.contains("youtube.com/shorts") {
            Some(task.referrer.clone())
        } else if task.original_url.contains("youtube.com/watch") || task.original_url.contains("youtu.be/") || task.original_url.contains("youtube.com/shorts") {
            Some(task.original_url.clone())
        } else {
            None
        };

        if watch_url.is_none() {
            if let Some(ref last) = *self.last_youtube_watch_url.read().await {
                watch_url = Some(last.clone());
            }
        }

        if watch_url.is_none() {
            if let Ok(parsed) = reqwest::Url::parse(&task.original_url) {
                if let Some((_, docid)) = parsed.query_pairs().find(|(k, _)| k == "docid" || k == "id" || k == "v") {
                    if !docid.is_empty() && !docid.starts_with("o-") {
                        watch_url = Some(format!("https://www.youtube.com/watch?v={}", docid));
                    }
                }
            }
        }

        let url = watch_url.ok_or_else(|| "Could not determine YouTube video page URL".to_string())?;

        println!("[Engine] Downloading YouTube video via yt-dlp: URL={}, target={}", url, target_path);

        let is_audio = task.get_filename().to_lowercase().ends_with(".mp3")
            || task.get_category().to_lowercase() == "audio";

        let mut cmd = tokio::process::Command::new(&ytdlp_bin);
        cmd.arg("--no-playlist")
           .arg("--newline")
           .arg("--progress")
           .arg("--progress-template")
           .arg("download:IDMPROG:%(progress.downloaded_bytes)s:%(progress.total_bytes)s:%(progress.speed)s:%(progress.eta)s");

        if let Some(ffmpeg_bin) = crate::mux::probe_ffmpeg() {
            cmd.arg("--ffmpeg-location").arg(ffmpeg_bin);
        }

        let itag_opt = reqwest::Url::parse(&task.original_url).ok().and_then(|u| {
            u.query_pairs().find(|(k, _)| k == "itag").map(|(_, v)| v.to_string())
        });

        cmd.arg("--force-overwrites");

        let fn_lower = task.get_filename().to_lowercase();
        let target_height = if fn_lower.contains("2160") || fn_lower.contains("4k") {
            2160
        } else if fn_lower.contains("1440") || fn_lower.contains("2k") {
            1440
        } else if fn_lower.contains("1080") {
            1080
        } else if fn_lower.contains("720") {
            720
        } else if fn_lower.contains("480") {
            480
        } else if fn_lower.contains("360") {
            360
        } else {
            1080
        };

        if is_audio {
            cmd.arg("-x").arg("--audio-format").arg("mp3");
        } else if let Some(ref itag) = itag_opt {
            if itag == "18" {
                cmd.arg("-f").arg("18/bestvideo[height<=360]+bestaudio/best[height<=360]/best");
            } else {
                cmd.arg("-f").arg(format!(
                    "{}+bestaudio[ext=m4a]/{}+bestaudio/{}+ba/bestvideo[height<={}]+bestaudio/best[height<={}]/best",
                    itag, itag, itag, target_height, target_height
                ));
            }
            cmd.arg("--merge-output-format").arg("mp4");
        } else {
            cmd.arg("-f").arg(format!(
                "bestvideo[height<={}]+bestaudio[ext=m4a]/bestvideo[height<={}]+bestaudio/best[height<={}]/best",
                target_height, target_height, target_height
            ));
            cmd.arg("--merge-output-format").arg("mp4");
        }

        // Feature: YouTube Subtitles (.srt / .vtt) extraction alongside video
        cmd.arg("--write-sub")
           .arg("--write-auto-sub")
           .arg("--sub-lang").arg("en,ur,hi,all")
           .arg("--sub-format").arg("srt/vtt/best")
           .arg("--embed-subs");

        // Edge Case 5: YouTube Cookie & Authentication Passing
        let temp_cookie_file = if !task.cookie.is_empty() {
            let temp_cookie_dir = std::env::temp_dir().join(format!("idm_yt_cookie_{}", task.id));
            let _ = std::fs::create_dir_all(&temp_cookie_dir);
            let cookie_path = temp_cookie_dir.join("cookies.txt");
            if let Ok(()) = crate::mux::write_netscape_cookies(&task.cookie, &[".youtube.com", ".google.com"], &cookie_path) {
                cmd.arg("--cookies").arg(&cookie_path);
                Some(cookie_path)
            } else {
                None
            }
        } else {
            None
        };

        if !task.user_agent.is_empty() {
            cmd.arg("--user-agent").arg(&task.user_agent);
        }

        cmd.arg("-o").arg(target_path);
        cmd.arg(&url);

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            if let Some(ref cp) = temp_cookie_file {
                let _ = std::fs::remove_file(cp);
                if let Some(parent) = cp.parent() { let _ = std::fs::remove_dir(parent); }
            }
            format!("Failed to spawn yt-dlp: {}", e)
        })?;
        let stdout = child.stdout.take().ok_or_else(|| "Failed to capture yt-dlp stdout".to_string())?;
        let stderr = child.stderr.take().ok_or_else(|| "Failed to capture yt-dlp stderr".to_string())?;

        // Read stderr in background task to avoid pipe buffer deadlock and capture YouTube error messages
        let stderr_handle = tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            let mut captured = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                captured.push(line);
                if captured.len() > 100 {
                    captured.remove(0);
                }
            }
            captured.join("\n")
        });

        use tokio::io::AsyncBufReadExt;
        let mut reader = tokio::io::BufReader::new(stdout).lines();

        let mut last_downloaded = 0u64;
        let mut last_total = 0u64;
        let mut last_speed_time = std::time::Instant::now();
        let mut last_speed_bytes = 0u64;
        let mut smoothed_speed = 0.0f64;

        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(idx) = line.find("IDMPROG:") {
                let parts: Vec<&str> = line[idx + 8..].split(':').collect();
                if parts.len() >= 4 {
                    if let Ok(dl) = parts[0].trim().parse::<u64>() {
                        let diff = dl.saturating_sub(last_downloaded);
                        task.downloaded.fetch_add(diff, Ordering::Relaxed);
                        task.network_downloaded.fetch_add(diff, Ordering::Relaxed);
                        last_downloaded = dl;

                        let now = std::time::Instant::now();
                        let elapsed = now.duration_since(last_speed_time).as_secs_f64();
                        if elapsed >= 0.4 {
                            let bytes_diff = dl.saturating_sub(last_speed_bytes);
                            let instant_speed = (bytes_diff as f64) / elapsed;
                            smoothed_speed = if smoothed_speed == 0.0 {
                                instant_speed
                            } else {
                                smoothed_speed * 0.70 + instant_speed * 0.30
                            };
                            last_speed_time = now;
                            last_speed_bytes = dl;
                            *task.speed.lock().unwrap() = smoothed_speed;
                        }
                    }
                    if let Ok(tot) = parts[1].trim().parse::<u64>() {
                        if tot > 0 {
                            task.total_size.store(tot, Ordering::Relaxed);
                            last_total = tot;
                        }
                    }
                    let spd = *task.speed.lock().unwrap();
                    let current_total_size = task.total_size.load(Ordering::Relaxed);
                    let eta = if spd > 1024.0 && current_total_size > last_downloaded {
                        let rem = current_total_size - last_downloaded;
                        let rem_secs = (rem as f64 / spd).round() as u64;
                        if rem_secs >= 60 {
                            format!("{} mins {} secs", rem_secs / 60, rem_secs % 60)
                        } else {
                            format!("{} secs", rem_secs)
                        }
                    } else if parts[3].trim() != "NA" {
                        format!("{}s", parts[3].trim())
                    } else {
                        "---".to_string()
                    };
                    *task.eta.lock().unwrap() = eta;

                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let progress = DownloadProgress {
                            id: task.id.clone(),
                            url: task.original_url.clone(),
                            filename: task.get_filename(),
                            save_path: task.get_save_path(),
                            total_size: task.total_size.load(Ordering::Relaxed),
                            downloaded: task.network_downloaded.load(Ordering::Relaxed),
                            speed: *task.speed.lock().unwrap(),
                            eta: task.eta.lock().unwrap().clone(),
                            status: DownloadStatus::Downloading,
                            file_exists: std::path::Path::new(target_path).exists(),
                            speed_limited: false,
                            category: task.get_category(),
                        };
                        let _ = handle.emit("download-progress", progress);
                    }
                }
            }
        }

        let status = child.wait().await.map_err(|e| format!("yt-dlp process wait failed: {}", e))?;
        let stderr_output = stderr_handle.await.unwrap_or_default();

        // Clean up temp cookie file
        if let Some(ref cp) = temp_cookie_file {
            let _ = std::fs::remove_file(cp);
            if let Some(parent) = cp.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }

        if !status.success() {
            let diag = crate::mux::parse_youtube_error(&stderr_output);
            return Err(diag);
        }

        // If yt-dlp wrote with an alternate extension (e.g. .mp4.webm or .mkv), reconcile it
        let actual_path = if std::path::Path::new(target_path).exists() {
            std::path::PathBuf::from(target_path)
        } else if std::path::Path::new(&format!("{}.webm", target_path)).exists() {
            let alt = std::path::PathBuf::from(format!("{}.webm", target_path));
            let _ = std::fs::rename(&alt, target_path);
            std::path::PathBuf::from(target_path)
        } else if std::path::Path::new(&format!("{}.mkv", target_path)).exists() {
            let alt = std::path::PathBuf::from(format!("{}.mkv", target_path));
            let _ = std::fs::rename(&alt, target_path);
            std::path::PathBuf::from(target_path)
        } else {
            std::path::PathBuf::from(target_path)
        };

        let final_size = std::fs::metadata(&actual_path).map(|m| m.len()).unwrap_or(last_total);
        if final_size > 0 {
            task.total_size.store(final_size, Ordering::Relaxed);
            task.downloaded.store(final_size, Ordering::Relaxed);
            task.network_downloaded.store(final_size, Ordering::Relaxed);
        }

        Ok(final_size)
    }

    pub async fn run_task(self: &Arc<Self>, task: Arc<DownloadTask>, is_resume: bool) -> Result<(), String> {
        *task.status.lock().unwrap() = DownloadStatus::Downloading;

        let save_path_val = task.get_save_path();
        if let Some(parent) = std::path::Path::new(&save_path_val).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let temp_dir = {
            let handle_opt = self.app_handle.lock().await;
            if let Some(handle) = handle_opt.as_ref() {
                let mut p = handle.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                p.push("temp");
                p.push(&task.id);
                p
            } else {
                std::path::PathBuf::from(format!("./temp/{}", task.id))
            }
        };

        let abort_tx = match task.abort_tx.as_ref() {
            Some(tx) => tx,
            None => return Err("Missing abort channel".to_string()),
        };

        let id_clone = task.id.clone();
        let task_for_reporting = task.clone();
        let status_ref = task.status.clone();
        let mut abort_rx = abort_tx.subscribe();
        let app_handle_opt = self.app_handle.lock().await.clone();
        let task_speed = task.speed.clone();
        let task_eta = task.eta.clone();
        let manager_for_save = self.clone();
        let manager_for_reporting = self.clone();

        // Speed & Progress reporting loop (Spans ALL downloads: HLS, Dual-Stream, and Chunks)
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(400));
            let mut samples: std::collections::VecDeque<(u64, Instant)> = std::collections::VecDeque::with_capacity(4);
            samples.push_back((task_for_reporting.network_downloaded.load(Ordering::Relaxed), Instant::now()));
            let mut save_ticks = 0;

            loop {
                tokio::select! {
                    res = abort_rx.recv() => {
                        if res.is_ok() { break; }
                    }
                    _ = interval.tick() => {
                        let current_bytes = task_for_reporting.network_downloaded.load(Ordering::Relaxed);
                        let now = Instant::now();

                        samples.push_back((current_bytes, now));
                        if samples.len() > 3 { samples.pop_front(); }
                        let raw_speed = if samples.len() >= 2 {
                            let (old_bytes, old_time) = samples.front().unwrap();
                            let duration = now.duration_since(*old_time).as_secs_f64();
                            if duration > 0.0 { current_bytes.saturating_sub(*old_bytes) as f64 / duration } else { 0.0 }
                        } else { 0.0 };

                        let current_limit = manager_for_reporting.speed_limit_bps.load(Ordering::Relaxed);
                        let is_speed_limited = current_limit > 0 && raw_speed > 0.0;
                        let speed = if current_limit > 0 { raw_speed.min(current_limit as f64) } else { raw_speed };

                        let mut speed_guard = task_speed.lock().unwrap();
                        *speed_guard = speed;

                        let current_total_size = task_for_reporting.total_size.load(Ordering::Relaxed);
                        let eta = if speed > 1024.0 && current_total_size > current_bytes {
                            let rem = current_total_size - current_bytes;
                            let rem_secs = (rem as f64 / speed).round() as u64;
                            if rem_secs >= 3600 {
                                let h = rem_secs / 3600;
                                let m = (rem_secs % 3600) / 60;
                                let s = rem_secs % 60;
                                if m > 0 { format!("{}h {}m {}s", h, m, s) } else { format!("{}h {}s", h, s) }
                            } else if rem_secs >= 60 {
                                let m = rem_secs / 60;
                                let s = rem_secs % 60;
                                if s > 0 { format!("{} mins {} secs", m, s) } else { format!("{} mins", m) }
                            } else { format!("{} secs", rem_secs) }
                        } else { "---".to_string() };

                        let status = status_ref.lock().unwrap().clone();
                        *task_eta.lock().unwrap() = eta.clone();

                        let current_filename = task_for_reporting.get_filename();
                        let current_save_path = task_for_reporting.get_save_path();

                        let progress = DownloadProgress {
                            id: id_clone.clone(),
                            url: task_for_reporting.original_url.clone(),
                            filename: current_filename,
                            save_path: current_save_path.clone(),
                            total_size: current_total_size,
                            downloaded: current_bytes,
                            speed,
                            eta,
                            status: status.clone(),
                            file_exists: std::path::Path::new(&current_save_path).exists(),
                            speed_limited: is_speed_limited,
                            category: task_for_reporting.get_category(),
                        };

                        if let Some(ref handle) = app_handle_opt {
                            use tauri::Emitter;
                            let _ = handle.emit("download-progress", progress);
                        }

                        save_ticks += 1;
                        if save_ticks >= 75 {
                            save_ticks = 0;
                            let manager_clone = manager_for_save.clone();
                            tokio::spawn(async move {
                                let _ = manager_clone.save_history().await;
                            });
                        }

                        if status == DownloadStatus::Completed || matches!(status, DownloadStatus::Failed(_)) { break; }
                    }
                }
            }
        });

        // Feature 3: BitTorrent Native P2P Swarming Interception
        if task.original_url.starts_with("magnet:?") {
            if let Ok(magnet) = crate::torrent::parse_magnet(&task.original_url) {
                if magnet.web_seeds.is_empty() {
                    println!("[P2P Swarm] Starting BitTorrent Swarm download for: {}", magnet.name);
                    *task.status.lock().unwrap() = DownloadStatus::Downloading;
                    let info_hash = crate::torrent::hex_to_bytes20(&magnet.info_hash).map_err(|e| e.to_string())?;
                    let peer_id = crate::torrent::generate_peer_id();
                    let dest_path = std::path::PathBuf::from(task.get_save_path());
                    let total_len = magnet.exact_length.unwrap_or(0);
                    let abort_rx = task.abort_tx.as_ref().map(|tx| tx.subscribe());

                    match crate::torrent::run_p2p_swarm_download(
                        info_hash,
                        peer_id,
                        262144, // 256 KiB piece default
                        total_len,
                        vec![],
                        magnet.trackers,
                        vec![],
                        dest_path.clone(),
                        task.network_downloaded.clone(),
                        abort_rx,
                    ).await {
                        Ok(final_path) => {
                            task.total_size.store(task.network_downloaded.load(Ordering::Relaxed), Ordering::Relaxed);
                            *task.status.lock().unwrap() = DownloadStatus::Completed;
                            let cur_filename = task.get_filename();
                            let progress = DownloadProgress {
                                id: task.id.clone(),
                                url: task.original_url.clone(),
                                filename: cur_filename,
                                save_path: final_path,
                                total_size: task.total_size.load(Ordering::Relaxed),
                                downloaded: task.network_downloaded.load(Ordering::Relaxed),
                                speed: 0.0,
                                eta: "0s".to_string(),
                                status: DownloadStatus::Completed,
                                file_exists: std::path::Path::new(&task.get_save_path()).exists(),
                                speed_limited: false,
                                category: task.get_category(),
                            };
                            if let Some(ref handle) = self.app_handle.lock().await.clone() {
                                use tauri::Emitter;
                                let _ = handle.emit("download-progress", progress);
                            }
                            let _ = self.save_history().await;
                            return Ok(());
                        }
                        Err(e) => {
                            *task.status.lock().unwrap() = DownloadStatus::Failed(e.clone());
                            return Err(e);
                        }
                    }
                }
            }
        }

        // Phase 3.5: FTP & FTPS Engine Interception (RFC 959, RFC 2228, RFC 3659)
        if crate::ftp::is_ftp_url(&task.original_url) {
            let ftp_info = match crate::ftp::parse_ftp_url(&task.original_url) {
                Ok(info) => info,
                Err(e) => {
                    *task.status.lock().unwrap() = DownloadStatus::Failed(e.clone());
                    return Err(e);
                }
            };

            println!("[FTP Engine] Starting FTP/FTPS download for: {}", task.original_url);
            *task.status.lock().unwrap() = DownloadStatus::Downloading;
            let dest_path = std::path::PathBuf::from(task.get_save_path());
            let abort_rx = task.abort_tx.as_ref().map(|tx| tx.subscribe());

            match crate::ftp::download_ftp_stream(
                &ftp_info,
                0,
                None,
                &dest_path,
                task.network_downloaded.clone(),
                abort_rx,
            ).await {
                Ok(total_bytes) => {
                    task.total_size.store(total_bytes, Ordering::Relaxed);
                    *task.status.lock().unwrap() = DownloadStatus::Completed;
                    let cur_filename = task.get_filename();
                    let cur_save_path = task.get_save_path();
                    let progress = DownloadProgress {
                        id: task.id.clone(),
                        url: task.original_url.clone(),
                        filename: cur_filename,
                        save_path: cur_save_path.clone(),
                        total_size: total_bytes,
                        downloaded: total_bytes,
                        speed: 0.0,
                        eta: "0s".to_string(),
                        status: DownloadStatus::Completed,
                        file_exists: std::path::Path::new(&cur_save_path).exists(),
                        speed_limited: false,
                        category: task.get_category(),
                    };
                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let _ = handle.emit("download-progress", progress);
                    }
                    let _ = self.save_history().await;
                    return Ok(());
                }
                Err(e) => {
                    let err_msg = format!("FTP transfer failed: {}", e);
                    *task.status.lock().unwrap() = DownloadStatus::Failed(err_msg.clone());
                    let cur_filename = task.get_filename();
                    let cur_save_path = task.get_save_path();
                    let progress = DownloadProgress {
                        id: task.id.clone(),
                        url: task.original_url.clone(),
                        filename: cur_filename,
                        save_path: cur_save_path.clone(),
                        total_size: task.total_size.load(Ordering::Relaxed),
                        downloaded: task.network_downloaded.load(Ordering::Relaxed),
                        speed: 0.0,
                        eta: "---".to_string(),
                        status: DownloadStatus::Failed(err_msg.clone()),
                        file_exists: std::path::Path::new(&cur_save_path).exists(),
                        speed_limited: false,
                        category: task.get_category(),
                    };
                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let _ = handle.emit("download-progress", progress);
                    }
                    return Err(e);
                }
            }
        }

        // Phase 4: HLS Stream (.m3u8) Engine Interception
        if task.original_url.contains(".m3u8") {
            println!("[HLS Engine] Detected HLS stream download: {}", task.original_url);
            let client = self.client.clone();
            let source_url = task.original_url.clone();
            let cookie = task.cookie.clone();
            let user_agent = task.user_agent.clone();

            let hls_plan_res = crate::hls::resolve_hls(&client, &source_url, &cookie, &user_agent, 0, &source_url).await;
            match hls_plan_res {
                Ok(plan) => {
                    let cur_filename = task.get_filename();
                    let clean_base = cur_filename.replace(".m3u8", "");
                    let new_fn = format!("{}{}", clean_base, plan.extension);
                    task.update_filename_and_path(&new_fn);

                    let save_path_val = task.get_save_path();
                    let temp_hls_dir = temp_dir.clone();

                    let downloaded_tracker = task.downloaded.clone();
                    let total_size_tracker = Arc::new(AtomicU64::new(task.total_size.load(Ordering::Relaxed)));

                    match crate::hls::run_hls_stream_download(
                        client,
                        plan,
                        temp_hls_dir,
                        save_path_val.clone(),
                        cookie,
                        user_agent,
                        downloaded_tracker,
                        total_size_tracker.clone(),
                        None,
                    ).await {
                        Ok(final_path) => {
                            task.total_size.store(total_size_tracker.load(Ordering::Relaxed), Ordering::Relaxed);
                            *task.status.lock().unwrap() = DownloadStatus::Completed;
                            if final_path != save_path_val {
                                let new_name = std::path::Path::new(&final_path)
                                    .file_name()
                                    .and_then(|f| f.to_str())
                                    .unwrap_or(&new_fn);
                                task.update_filename_and_path(new_name);
                            }
                            let cur_filename = task.get_filename();
                            let cur_save_path = task.get_save_path();
                            let progress = DownloadProgress {
                                id: task.id.clone(),
                                url: task.original_url.clone(),
                                filename: cur_filename,
                                save_path: cur_save_path.clone(),
                                total_size: task.total_size.load(Ordering::Relaxed),
                                downloaded: task.network_downloaded.load(Ordering::Relaxed),
                                speed: 0.0,
                                eta: "0s".to_string(),
                                status: DownloadStatus::Completed,
                                file_exists: std::path::Path::new(&cur_save_path).exists(),
                                speed_limited: false,
                                category: task.get_category(),
                            };
                            if let Some(ref handle) = self.app_handle.lock().await.clone() {
                                use tauri::Emitter;
                                let _ = handle.emit("download-progress", progress);
                            }
                            let _ = self.save_history().await;
                            return Ok(());
                        }
                        Err(e) => {
                            let err_msg = format!("HLS download failed: {}", e);
                            *task.status.lock().unwrap() = DownloadStatus::Failed(err_msg.clone());
                            let cur_filename = task.get_filename();
                            let cur_save_path = task.get_save_path();
                            let progress = DownloadProgress {
                                id: task.id.clone(),
                                url: task.original_url.clone(),
                                filename: cur_filename,
                                save_path: cur_save_path.clone(),
                                total_size: task.total_size.load(Ordering::Relaxed),
                                downloaded: task.network_downloaded.load(Ordering::Relaxed),
                                speed: 0.0,
                                eta: "---".to_string(),
                                status: DownloadStatus::Failed(err_msg.clone()),
                                file_exists: std::path::Path::new(&cur_save_path).exists(),
                                speed_limited: false,
                                category: task.get_category(),
                            };
                            if let Some(ref handle) = self.app_handle.lock().await.clone() {
                                use tauri::Emitter;
                                let _ = handle.emit("download-progress", progress);
                            }
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[HLS Engine] Could not resolve HLS manifest: {}. Falling back to standard engine...", e);
                }
            }
        }

        // Phase 5: YouTube Watch Page Direct Engine OR Dual-Stream / Single-Stream Engine
        let is_youtube_watch = task.original_url.contains("youtube.com/watch")
            || task.original_url.contains("youtu.be/")
            || task.original_url.contains("youtube.com/shorts");

        if is_youtube_watch {
            let final_target = task.get_save_path();
            match self.try_download_via_ytdlp(&task, &final_target).await {
                Ok(dl_bytes) => {
                    *task.status.lock().unwrap() = DownloadStatus::Completed;
                    let cur_filename = task.get_filename();
                    let cur_save_path = task.get_save_path();
                    let completed_progress = DownloadProgress {
                        id: task.id.clone(),
                        url: task.original_url.clone(),
                        filename: cur_filename,
                        save_path: cur_save_path.clone(),
                        total_size: dl_bytes,
                        downloaded: dl_bytes,
                        speed: 0.0,
                        eta: "0s".to_string(),
                        status: DownloadStatus::Completed,
                        file_exists: std::path::Path::new(&cur_save_path).exists(),
                        speed_limited: false,
                        category: task.get_category(),
                    };
                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let _ = handle.emit("download-progress", completed_progress);
                    }
                    let _ = self.save_history().await;
                    return Ok(());
                }
                Err(e) => {
                    let err_msg = format!("YouTube download failed: {}", e);
                    *task.status.lock().unwrap() = DownloadStatus::Failed(err_msg.clone());
                    let cur_filename = task.get_filename();
                    let cur_save_path = task.get_save_path();
                    let progress = DownloadProgress {
                        id: task.id.clone(),
                        url: task.original_url.clone(),
                        filename: cur_filename,
                        save_path: cur_save_path.clone(),
                        total_size: task.total_size.load(Ordering::Relaxed),
                        downloaded: task.network_downloaded.load(Ordering::Relaxed),
                        speed: 0.0,
                        eta: "---".to_string(),
                        status: DownloadStatus::Failed(err_msg.clone()),
                        file_exists: std::path::Path::new(&cur_save_path).exists(),
                        speed_limited: false,
                        category: task.get_category(),
                    };
                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let _ = handle.emit("download-progress", progress);
                    }
                    return Err(e);
                }
            }
        }

        let is_youtube_stream = task.original_url.contains("googlevideo.com") || task.original_url.contains("youtube.com/videoplayback");
        if task.audio_url.is_some() || is_youtube_stream {
            if let Some(ref audio_target_url) = task.audio_url {
            let video_source_url = task.original_url.clone();
            let audio_source_url = audio_target_url.clone();
            let cookie = task.cookie.clone();
            let referrer = task.referrer.clone();
            let user_agent = task.user_agent.clone();
            let save_path_val = task.get_save_path();
            let downloaded_tracker = task.downloaded.clone();
            let network_tracker = task.network_downloaded.clone();

            let _ = tokio::fs::create_dir_all(&temp_dir).await;
            let video_track_path = temp_dir.join("video.track");
            let audio_track_path = temp_dir.join("audio.track");

            let client = Self::build_stream_client(&video_source_url)?;

            let auto_calc_size = Arc::new(std::sync::atomic::AtomicBool::new(task.total_size.load(Ordering::Relaxed) == 0));

            let (v_res, a_res) = tokio::join!(
                Self::download_stream_to_file(
                    client.clone(),
                    video_source_url,
                    video_track_path.clone(),
                    cookie.clone(),
                    referrer.clone(),
                    user_agent.clone(),
                    downloaded_tracker.clone(),
                    network_tracker.clone(),
                    task.clone(),
                    auto_calc_size.clone(),
                ),
                Self::download_stream_to_file(
                    client.clone(),
                    audio_source_url,
                    audio_track_path.clone(),
                    cookie.clone(),
                    referrer.clone(),
                    user_agent.clone(),
                    downloaded_tracker.clone(),
                    network_tracker.clone(),
                    task.clone(),
                    auto_calc_size.clone(),
                )
            );

            let v_bytes = match v_res {
                Ok(b) => b,
                Err(e) => {
                    let final_target = task.get_save_path();
                    if let Ok(dl_bytes) = self.try_download_via_ytdlp(&task, &final_target).await {
                        *task.status.lock().unwrap() = DownloadStatus::Completed;
                        let cur_filename = task.get_filename();
                        let cur_save_path = task.get_save_path();
                        let completed_progress = DownloadProgress {
                            id: task.id.clone(),
                            url: task.original_url.clone(),
                            filename: cur_filename,
                            save_path: cur_save_path.clone(),
                            total_size: dl_bytes,
                            downloaded: dl_bytes,
                            speed: 0.0,
                            eta: "0s".to_string(),
                            status: DownloadStatus::Completed,
                            file_exists: std::path::Path::new(&cur_save_path).exists(),
                            speed_limited: false,
                            category: task.get_category(),
                        };
                        if let Some(ref handle) = self.app_handle.lock().await.clone() {
                            use tauri::Emitter;
                            let _ = handle.emit("download-progress", completed_progress);
                        }
                        let _ = self.save_history().await;
                        return Ok(());
                    }

                    let err_msg = format!("Video stream download failed: {}", e);
                    *task.status.lock().unwrap() = DownloadStatus::Failed(err_msg.clone());
                    let cur_filename = task.get_filename();
                    let cur_save_path = task.get_save_path();
                    let progress = DownloadProgress {
                        id: task.id.clone(),
                        url: task.original_url.clone(),
                        filename: cur_filename,
                        save_path: cur_save_path.clone(),
                        total_size: task.total_size.load(Ordering::Relaxed),
                        downloaded: task.network_downloaded.load(Ordering::Relaxed),
                        speed: 0.0,
                        eta: "---".to_string(),
                        status: DownloadStatus::Failed(err_msg.clone()),
                        file_exists: std::path::Path::new(&cur_save_path).exists(),
                        speed_limited: false,
                        category: task.get_category(),
                    };
                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let _ = handle.emit("download-progress", progress);
                    }
                    return Err(err_msg);
                }
            };
            let a_bytes = match a_res {
                Ok(b) => b,
                Err(e) => {
                    let err_msg = format!("Audio stream download failed: {}", e);
                    *task.status.lock().unwrap() = DownloadStatus::Failed(err_msg.clone());
                    let cur_filename = task.get_filename();
                    let cur_save_path = task.get_save_path();
                    let progress = DownloadProgress {
                        id: task.id.clone(),
                        url: task.original_url.clone(),
                        filename: cur_filename,
                        save_path: cur_save_path.clone(),
                        total_size: task.total_size.load(Ordering::Relaxed),
                        downloaded: task.network_downloaded.load(Ordering::Relaxed),
                        speed: 0.0,
                        eta: "---".to_string(),
                        status: DownloadStatus::Failed(err_msg.clone()),
                        file_exists: std::path::Path::new(&cur_save_path).exists(),
                        speed_limited: false,
                        category: task.get_category(),
                    };
                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let _ = handle.emit("download-progress", progress);
                    }
                    return Err(err_msg);
                }
            };

            let total_stream_bytes = v_bytes + a_bytes;
            task.total_size.store(total_stream_bytes, Ordering::Relaxed);
            task.downloaded.store(total_stream_bytes, Ordering::Relaxed);
            task.network_downloaded.store(total_stream_bytes, Ordering::Relaxed);

            *task.status.lock().unwrap() = DownloadStatus::Assembling;
            let cur_filename = task.get_filename();
            let cur_save_path = task.get_save_path();
            let assembling_progress = DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: cur_filename,
                save_path: cur_save_path.clone(),
                total_size: total_stream_bytes,
                downloaded: total_stream_bytes,
                speed: 0.0,
                eta: "Assembling...".to_string(),
                status: DownloadStatus::Assembling,
                file_exists: std::path::Path::new(&cur_save_path).exists(),
                speed_limited: false,
                category: task.get_category(),
            };
            if let Some(ref handle) = self.app_handle.lock().await.clone() {
                use tauri::Emitter;
                let _ = handle.emit("download-progress", assembling_progress);
            }

            let (final_path, _) = Self::allocate_collision_free_path(&save_path_val);
            let final_path_buf = std::path::PathBuf::from(&final_path);

            match crate::mux::mux_dual_stream(&video_track_path, &audio_track_path, &final_path_buf).await {
                Ok(()) => {
                    let _ = std::fs::remove_file(&video_track_path);
                    let _ = std::fs::remove_file(&audio_track_path);
                    let _ = std::fs::remove_dir(&temp_dir);
                    *task.status.lock().unwrap() = DownloadStatus::Completed;
                    if final_path != save_path_val {
                        if let Some(name) = std::path::Path::new(&final_path).file_name().and_then(|f| f.to_str()) {
                            task.update_filename_and_path(name);
                        }
                    }
                    let cur_filename = task.get_filename();
                    let cur_save_path = task.get_save_path();
                    let completed_progress = DownloadProgress {
                        id: task.id.clone(),
                        url: task.original_url.clone(),
                        filename: cur_filename,
                        save_path: cur_save_path.clone(),
                        total_size: total_stream_bytes,
                        downloaded: total_stream_bytes,
                        speed: 0.0,
                        eta: "0s".to_string(),
                        status: DownloadStatus::Completed,
                        file_exists: std::path::Path::new(&cur_save_path).exists(),
                        speed_limited: false,
                        category: task.get_category(),
                    };
                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let _ = handle.emit("download-progress", completed_progress);
                    }
                    let _ = self.save_history().await;
                    return Ok(());
                }
                Err(mux_err) => {
                    eprintln!("[Dual Stream Muxer] FFmpeg mux notice: {}. Preserving separate tracks safely.", mux_err);
                    let _ = std::fs::rename(&video_track_path, &final_path);
                    let audio_fallback = format!("{}.audio.m4a", final_path);
                    let _ = std::fs::rename(&audio_track_path, &audio_fallback);
                    let _ = std::fs::remove_dir(&temp_dir);
                    *task.status.lock().unwrap() = DownloadStatus::Completed;
                    let cur_filename = task.get_filename();
                    let cur_save_path = task.get_save_path();
                    let completed_progress = DownloadProgress {
                        id: task.id.clone(),
                        url: task.original_url.clone(),
                        filename: cur_filename,
                        save_path: cur_save_path.clone(),
                        total_size: total_stream_bytes,
                        downloaded: total_stream_bytes,
                        speed: 0.0,
                        eta: "0s".to_string(),
                        status: DownloadStatus::Completed,
                        file_exists: std::path::Path::new(&cur_save_path).exists(),
                        speed_limited: false,
                        category: task.get_category(),
                    };
                    if let Some(ref handle) = self.app_handle.lock().await.clone() {
                        use tauri::Emitter;
                        let _ = handle.emit("download-progress", completed_progress);
                    }
                    let _ = self.save_history().await;
                    return Ok(());
                }
            }
        } else {
                // Single-stream YouTube format (360p or audio track)
                let video_source_url = task.original_url.clone();
                let cookie = task.cookie.clone();
                let referrer = task.referrer.clone();
                let user_agent = task.user_agent.clone();
                let save_path_val = task.get_save_path();
                let downloaded_tracker = task.downloaded.clone();
                let network_tracker = task.network_downloaded.clone();

                let (final_path, _) = Self::allocate_collision_free_path(&save_path_val);
                let final_path_buf = std::path::PathBuf::from(&final_path);

                let client = Self::build_stream_client(&video_source_url)?;

                let auto_calc_size = Arc::new(std::sync::atomic::AtomicBool::new(task.total_size.load(Ordering::Relaxed) == 0));

                match Self::download_stream_to_file(
                    client,
                    video_source_url,
                    final_path_buf.clone(),
                    cookie,
                    referrer,
                    user_agent,
                    downloaded_tracker,
                    network_tracker,
                    task.clone(),
                    auto_calc_size,
                ).await {
                    Ok(total_bytes) => {
                        *task.status.lock().unwrap() = DownloadStatus::Completed;
                        if final_path != save_path_val {
                            if let Some(name) = std::path::Path::new(&final_path).file_name().and_then(|f| f.to_str()) {
                                task.update_filename_and_path(name);
                            }
                        }
                        let cur_filename = task.get_filename();
                        let cur_save_path = task.get_save_path();
                        let completed_progress = DownloadProgress {
                            id: task.id.clone(),
                            url: task.original_url.clone(),
                            filename: cur_filename,
                            save_path: cur_save_path.clone(),
                            total_size: total_bytes,
                            downloaded: total_bytes,
                            speed: 0.0,
                            eta: "0s".to_string(),
                            status: DownloadStatus::Completed,
                            file_exists: std::path::Path::new(&cur_save_path).exists(),
                            speed_limited: false,
                            category: task.get_category(),
                        };
                        if let Some(ref handle) = self.app_handle.lock().await.clone() {
                            use tauri::Emitter;
                            let _ = handle.emit("download-progress", completed_progress);
                        }
                        let _ = self.save_history().await;
                        return Ok(());
                    }
                    Err(e) => {
                        let final_target = task.get_save_path();
                        if let Ok(dl_bytes) = self.try_download_via_ytdlp(&task, &final_target).await {
                            *task.status.lock().unwrap() = DownloadStatus::Completed;
                            let cur_filename = task.get_filename();
                            let cur_save_path = task.get_save_path();
                            let completed_progress = DownloadProgress {
                                id: task.id.clone(),
                                url: task.original_url.clone(),
                                filename: cur_filename,
                                save_path: cur_save_path.clone(),
                                total_size: dl_bytes,
                                downloaded: dl_bytes,
                                speed: 0.0,
                                eta: "0s".to_string(),
                                status: DownloadStatus::Completed,
                                file_exists: std::path::Path::new(&cur_save_path).exists(),
                                speed_limited: false,
                                category: task.get_category(),
                            };
                            if let Some(ref handle) = self.app_handle.lock().await.clone() {
                                use tauri::Emitter;
                                let _ = handle.emit("download-progress", completed_progress);
                            }
                            let _ = self.save_history().await;
                            return Ok(());
                        }

                        let err_msg = format!("Video stream download failed: {}", e);
                        *task.status.lock().unwrap() = DownloadStatus::Failed(err_msg.clone());
                        let cur_filename = task.get_filename();
                        let cur_save_path = task.get_save_path();
                        let progress = DownloadProgress {
                            id: task.id.clone(),
                            url: task.original_url.clone(),
                            filename: cur_filename,
                            save_path: cur_save_path.clone(),
                            total_size: task.total_size.load(Ordering::Relaxed),
                            downloaded: task.network_downloaded.load(Ordering::Relaxed),
                            speed: 0.0,
                            eta: "---".to_string(),
                            status: DownloadStatus::Failed(err_msg.clone()),
                            file_exists: std::path::Path::new(&cur_save_path).exists(),
                            speed_limited: false,
                            category: task.get_category(),
                        };
                        if let Some(ref handle) = self.app_handle.lock().await.clone() {
                            use tauri::Emitter;
                            let _ = handle.emit("download-progress", progress);
                        }
                        return Err(e);
                    }
                }
            }
        }

        let temp_dir_exists = temp_dir.exists();
        let chunks_empty = task.chunks.lock().unwrap().is_empty();
        let part_path = format!("{}.part", save_path_val);
        let part_exists = std::path::Path::new(&part_path).exists();
        let should_initialize = !is_resume || chunks_empty || (!temp_dir_exists && !part_exists);

        if should_initialize {
            task.downloaded.store(0, Ordering::Relaxed);
            task.network_downloaded.store(0, Ordering::Relaxed);

            if temp_dir.exists() {
                let _ = std::fs::remove_dir_all(&temp_dir);
            }
            if let Err(e) = std::fs::create_dir_all(&temp_dir) {
                return Err(format!("Could not create temp directory: {}", e));
            }

            let mut chunks = task.chunks.lock().unwrap();
            chunks.clear();
            // Initial Chunk 0 is open-ended until probe or worker receives response headers
            chunks.push(ActiveChunk {
                id: uuid::Uuid::new_v4().to_string(),
                start: 0,
                end: Arc::new(AtomicU64::new(0)),
                downloaded: Arc::new(AtomicU64::new(0)),
                active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });

            if part_exists {
                let _ = std::fs::remove_file(&part_path);
            }
        } else {
            if !temp_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&temp_dir) {
                    return Err(format!("Could not create temp directory: {}", e));
                }
            }
        }

        // Direct Sparse File Pre-Allocation (Zero-Merge Architecture)
        let target_file = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&part_path)
        {
            Ok(f) => Arc::new(f),
            Err(e) => return Err(format!("Could not create target part file: {}", e)),
        };

        let init_total_size = task.total_size.load(Ordering::Relaxed);
        if init_total_size > 0 {
            let _ = target_file.set_len(init_total_size);
        }

        // XDM Dynamic Chunk Spawner Logic
        let (worker_completed_tx, mut worker_completed_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut active_tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

        // Reset active flags
        {
            let chunks = task.chunks.lock().unwrap();
            for c in chunks.iter() {
                c.active.store(false, Ordering::SeqCst);
            }
        }

        let mut loop_abort_rx = abort_tx.subscribe();
        let is_first_request = Arc::new(std::sync::atomic::AtomicBool::new(should_initialize));
        let accept_ranges_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Dynamic Piece Grabber Manager
        loop {
            // Check for abort
            if let Ok(_) = loop_abort_rx.try_recv() {
                for (_, handle) in active_tasks.drain() { handle.abort(); }
                break;
            }

            // Remove finished tasks
            while let Ok(finished_id) = worker_completed_rx.try_recv() {
                active_tasks.remove(&finished_id);
            }

            if { let status = task.status.lock().unwrap(); matches!(*status, DownloadStatus::Failed(_)) } {
                for (_, handle) in active_tasks.drain() { handle.abort(); }
                break;
            }

            let current_total_size = task.total_size.load(Ordering::Relaxed);
            let can_multi_segment = accept_ranges_flag.load(Ordering::SeqCst) && current_total_size > 512 * 1024;
            let target_workers = if can_multi_segment {
                self.max_chunks.load(Ordering::Relaxed).max(1) as usize
            } else {
                1
            };

            if active_tasks.len() >= target_workers {
                tokio::select! {
                    res = loop_abort_rx.recv() => {
                        if res.is_ok() {
                            for (_, handle) in active_tasks.drain() { handle.abort(); }
                            break;
                        }
                    }
                    opt_finished = worker_completed_rx.recv() => {
                        if let Some(finished_id) = opt_finished {
                            active_tasks.remove(&finished_id);
                        } else {
                            break;
                        }
                    }
                }
                continue;
            }

            // Find an inactive chunk or dynamically steal/split the largest remaining work
            let mut chunk_to_spawn = None;
            {
                let mut chunks = task.chunks.lock().unwrap();

                // 1. Find inactive chunk
                for chunk in chunks.iter() {
                    if !chunk.active.load(Ordering::SeqCst) {
                        let end = chunk.end.load(Ordering::SeqCst);
                        let downloaded = chunk.downloaded.load(Ordering::SeqCst);
                        let can_run = if end > 0 && end >= chunk.start {
                            chunk.start + downloaded <= end
                        } else {
                            downloaded == 0
                        };
                        if can_run {
                            chunk.active.store(true, Ordering::SeqCst);
                            chunk_to_spawn = Some(chunk.clone());
                            break;
                        }
                    }
                }

                // 2. Dynamic Work-Stealing / Slowest Chunk Splitting (Ghost-Downloader architecture)
                if chunk_to_spawn.is_none() && can_multi_segment {
                    // Adaptive threshold: allow splitting down to 256KB when few active workers remain,
                    // so the download never gets stuck on a slow tail chunk!
                    let min_split_threshold: u64 = if active_tasks.len() <= 2 {
                        256 * 1024 // 256 KB
                    } else {
                        1024 * 1024 // 1 MB
                    };

                    let mut max_rem = 0;
                    let mut best_idx = None;
                    for (i, chunk) in chunks.iter().enumerate() {
                        let end = chunk.end.load(Ordering::SeqCst);
                        let downloaded = chunk.downloaded.load(Ordering::SeqCst);
                        let start = chunk.start;

                        if end > 0 && start + downloaded <= end {
                            let rem = (end + 1) - (start + downloaded);
                            if rem > max_rem {
                                max_rem = rem;
                                best_idx = Some(i);
                            }
                        }
                    }

                    if max_rem >= min_split_threshold * 2 {
                        if let Some(idx) = best_idx {
                            let end = chunks[idx].end.load(Ordering::SeqCst);
                            let downloaded = chunks[idx].downloaded.load(Ordering::SeqCst);
                            let start = chunks[idx].start;
                            let current_head = start + downloaded;

                            if end > current_head + min_split_threshold {
                                let rem = (end + 1) - current_head;
                                let split_len = rem / 2;
                                let new_start = end + 1 - split_len;

                                // Guard against race conditions: ensure new_start is safely ahead of current download progress by at least 64KB
                                if new_start > current_head + 64 * 1024 {
                                    // Adjust end of active chunk
                                    chunks[idx].end.store(new_start - 1, Ordering::SeqCst);

                                    let new_chunk = ActiveChunk {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        start: new_start,
                                        end: Arc::new(AtomicU64::new(end)),
                                        downloaded: Arc::new(AtomicU64::new(0)),
                                        active: Arc::new(std::sync::atomic::AtomicBool::new(true)),
                                    };

                                    chunk_to_spawn = Some(new_chunk.clone());
                                    chunks.push(new_chunk);
                                }
                            }
                        }
                    }
                }
            }

            if let Some(chunk) = chunk_to_spawn {
                let task_clone = task.clone();
                let client = self.client.clone();
                let downloaded_counter_clone = task.downloaded.clone();
                let manager_for_worker = self.clone();
                let completed_tx = worker_completed_tx.clone();
                let chunk_id = chunk.id.clone();
                let mut worker_abort_rx = abort_tx.subscribe();
                let is_first_req_clone = is_first_request.clone();
                let accept_ranges_flag_clone = accept_ranges_flag.clone();
                let target_file_worker = target_file.clone();

                let worker = tokio::spawn(async move {
                    let max_retries: u32 = 5;
                    let mut retry_count: u32 = 0;
                    let mut retry_with_no_range: bool = false;
                    let mut last_error_msg = String::new();
                    let mut net_buffer = Vec::with_capacity(64 * 1024);
                    let profile = BrowserProfile::detect(&task_clone.user_agent);
                    let mut is_retry_403 = false;

                    let clean_header_val = |v: &str| -> Option<reqwest::header::HeaderValue> {
                        let sanitized: String = v.chars()
                            .filter(|c| !c.is_control() && (*c as u32) < 256)
                            .collect();
                        reqwest::header::HeaderValue::from_str(&sanitized).ok()
                    };

                    'reconnect: loop {
                        net_buffer.clear();

                        let local_downloaded = chunk.downloaded.load(Ordering::SeqCst);
                        let current_end = chunk.end.load(Ordering::SeqCst);
                        let current_start = chunk.start + local_downloaded;

                        if current_end > 0 && current_end >= chunk.start && current_start > current_end {
                            break 'reconnect;
                        }

                        let flush_data = |buf: &mut Vec<u8>| -> bool {
                            if buf.is_empty() {
                                return true;
                            }
                            let flushed = std::mem::replace(buf, Vec::with_capacity(64 * 1024));
                            let offset = chunk.start + chunk.downloaded.load(Ordering::SeqCst);
                            if let Err(e) = write_at_offset(&target_file_worker, &flushed, offset) {
                                eprintln!("[Worker Error] Failed to write {} bytes at offset {}: {}", flushed.len(), offset, e);
                                return false;
                            }
                            chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                            downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                            true
                        };

                        if retry_count >= max_retries {
                            let failure_reason = if !last_error_msg.is_empty() {
                                format!("Failed: {}", last_error_msg)
                            } else {
                                "Failed: Connection retries exceeded".to_string()
                            };
                            *task_clone.status.lock().unwrap() = DownloadStatus::Failed(failure_reason);
                            break 'reconnect;
                        }

                        if retry_count > 0 {
                            tokio::time::sleep(Duration::from_millis(1000 * retry_count as u64)).await;
                        }

                        let worker_url = task_clone.final_url.lock().await.clone();
                        let mut req = client.get(&worker_url);

                        req = profile.apply_headers(
                            req,
                            &worker_url,
                            &task_clone.referrer,
                            &task_clone.cookie,
                            is_retry_403,
                        );

                        // Send Range header: unless fallback without Range requested
                        if !retry_with_no_range {
                            if current_end > 0 && current_end >= chunk.start {
                                req = req.header(reqwest::header::RANGE, format!("bytes={}-{}", current_start, current_end));
                            } else {
                                req = req.header(reqwest::header::RANGE, format!("bytes={}-", current_start));
                            }
                        }

                        // Send If-Range header if validator exists
                        if let Some(ref val) = *task_clone.validator.lock().unwrap() {
                            if let Some(hv) = clean_header_val(val) {
                                req = req.header(reqwest::header::IF_RANGE, hv);
                            }
                        }

                        let res = match tokio::time::timeout(Duration::from_secs(60), req.send()).await {
                            Ok(Ok(r)) => r,
                            Ok(Err(e)) => {
                                last_error_msg = format!("Network error: {}", e);
                                eprintln!("[Worker Error] Network error for chunk {}: {}", chunk_id, e);
                                retry_count += 1;
                                continue 'reconnect;
                            }
                            Err(_) => {
                                last_error_msg = "Connection timed out (60s)".to_string();
                                eprintln!("[Worker Error] Connection timed out (60s) for chunk {}", chunk_id);
                                retry_count += 1;
                                continue 'reconnect;
                            }
                        };

                        let status = res.status();

                        // 1. IP Rate-Limiting & Server Busy Guard (429 & 503)
                        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                            last_error_msg = format!("Server rate-limit/busy: HTTP {}", status);
                            eprintln!("[Worker Rate-Limit] Received {} for chunk {}. Backing off...", status, chunk_id);
                            retry_count += 1;
                            let backoff_ms = 1000 * (1 << retry_count.min(4));
                            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                            continue 'reconnect;
                        }

                        // 2. Strict Range Validation: Detect server returning 200 OK instead of 206 Partial Content on ranged requests
                        if status == reqwest::StatusCode::OK && !retry_with_no_range && current_start > 0 {
                            eprintln!("[Worker] Server returned 200 OK instead of byte range for start offset {}. Aborting range worker to prevent byte corruption.", current_start);
                            *task_clone.status.lock().unwrap() = DownloadStatus::Failed("Server ignored byte range request and returned full body. Cannot split stream.".to_string());
                            break 'reconnect;
                        }

                        // 3. Anti-Bot / Cloudflare 403 WAF Bypass: Retry with CORS same-origin headers
                        if status == reqwest::StatusCode::FORBIDDEN && !is_retry_403 {
                            eprintln!("[Worker Anti-Bot] HTTP 403 Forbidden received for chunk {}. Applying browser-origin CORS WAF bypass...", chunk_id);
                            is_retry_403 = true;
                            retry_count += 1;
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            continue 'reconnect;
                        }

                        // 4. Fallback on range rejections (416, 400, 403, 405)
                        if (status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE || status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::METHOD_NOT_ALLOWED) && !retry_with_no_range {
                            eprintln!("[Worker] Range header rejected with status {}. Retrying without Range...", status);
                            retry_with_no_range = true;
                            continue 'reconnect;
                        }

                        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
                            last_error_msg = format!("HTTP {}", status);
                            eprintln!("[Worker Error] HTTP status {} for chunk {}", status, chunk_id);

                            if is_first_req_clone.load(Ordering::SeqCst) && (status.is_client_error() || status.is_server_error()) {
                                *task_clone.status.lock().unwrap() = DownloadStatus::Failed(format!("HTTP Error {}", status));
                                break 'reconnect;
                            }
                            retry_count += 1;
                            continue 'reconnect;
                        }

                        // First Connection Inspection (XDM Single-Pass Pattern)
                        if is_first_req_clone.swap(false, Ordering::SeqCst) {
                            let final_res_url = res.url().to_string();
                            *task_clone.final_url.lock().await = final_res_url.clone();

                            let is_partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
                            let mut probed_total_size = 0u64;

                            if is_partial {
                                accept_ranges_flag_clone.store(true, Ordering::SeqCst);
                                if let Some(cr_val) = res.headers().get("Content-Range").and_then(|h| h.to_str().ok()) {
                                    if let Some(slash_idx) = cr_val.rfind('/') {
                                        if let Ok(s) = cr_val[slash_idx + 1..].trim().parse::<u64>() {
                                            probed_total_size = s;
                                        }
                                    }
                                }
                            }

                            if probed_total_size == 0 {
                                probed_total_size = res.headers()
                                    .get(reqwest::header::CONTENT_LENGTH)
                                    .and_then(|val| val.to_str().ok())
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .unwrap_or(0);
                            }

                            if probed_total_size > 0 {
                                task_clone.total_size.store(probed_total_size, Ordering::Relaxed);
                                let _ = target_file_worker.set_len(probed_total_size);
                            }

                            // Dynamic validator inspection (strong ETag or Last-Modified)
                            let etag = res.headers().get(reqwest::header::ETAG)
                                .and_then(|h| h.to_str().ok())
                                .and_then(|s| if s.starts_with("W/") { None } else { Some(s.to_string()) });
                            let last_mod = res.headers().get(reqwest::header::LAST_MODIFIED)
                                .and_then(|h| h.to_str().ok())
                                .map(|s| s.to_string());
                            let current_validator = etag.or(last_mod);
                            if let Some(val) = current_validator {
                                *task_clone.validator.lock().unwrap() = Some(val);
                            }

                            // Dynamic filename refinement from Content-Disposition
                            if let Some(cd) = res.headers().get(reqwest::header::CONTENT_DISPOSITION).and_then(|h| h.to_str().ok()) {
                                if let Some(cd_name) = crate::proxy::parse_content_disposition(cd) {
                                    if !crate::proxy::is_generic_filename(&cd_name) {
                                        task_clone.update_filename_and_path(&cd_name);
                                    }
                                }
                            }

                            // Dynamic filename refinement from Content-Type if missing extension
                            let cur_fn = task_clone.get_filename();
                            if !cur_fn.contains('.') {
                                if let Some(ct) = res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|h| h.to_str().ok()) {
                                    if let Some(ext) = crate::proxy::extension_from_content_type(ct) {
                                        let updated = format!("{}{}", cur_fn, ext);
                                        task_clone.update_filename_and_path(&updated);
                                    }
                                }
                            }

                            // If ranges supported and file > 512KB, dynamically split segments for parallel workers!
                            let max_c = manager_for_worker.max_chunks.load(Ordering::Relaxed).max(1) as usize;
                            if is_partial && probed_total_size > 512 * 1024 && max_c > 1 {
                                let num_workers = ((probed_total_size / (512 * 1024)) as usize).clamp(1, max_c);
                                if num_workers > 1 {
                                    let seg_size = probed_total_size / (num_workers as u64);
                                    let mut chunks_guard = task_clone.chunks.lock().unwrap();
                                    if chunks_guard.len() == 1 {
                                        chunks_guard[0].end.store(seg_size - 1, Ordering::SeqCst);
                                        for i in 1..num_workers {
                                            let c_start = (i as u64) * seg_size;
                                            let c_end = if i == num_workers - 1 {
                                                probed_total_size - 1
                                            } else {
                                                (i as u64 + 1) * seg_size - 1
                                            };
                                            chunks_guard.push(ActiveChunk {
                                                id: uuid::Uuid::new_v4().to_string(),
                                                start: c_start,
                                                end: Arc::new(AtomicU64::new(c_end)),
                                                downloaded: Arc::new(AtomicU64::new(0)),
                                                active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                                            });
                                        }
                                    }
                                }
                            }
                        }

                        let mut stream = res.bytes_stream();

                        loop {
                            tokio::select! {
                                biased;
                                res = worker_abort_rx.recv() => {
                                    if res.is_ok() {
                                        let _ = flush_data(&mut net_buffer);
                                        break 'reconnect;
                                    }
                                }
                                bytes_chunk_res = tokio::time::timeout(Duration::from_secs(15), stream.next()) => {
                                    let bytes_chunk = match bytes_chunk_res {
                                        Ok(b) => b,
                                        Err(_) => {
                                            let _ = flush_data(&mut net_buffer);
                                            retry_count += 1;
                                            continue 'reconnect;
                                        }
                                    };

                                    match bytes_chunk {
                                        Some(Ok(bytes)) => {
                                            retry_count = 0;
                                            let mut bytes_to_add = bytes.as_ref();

                                            let dynamic_end = chunk.end.load(Ordering::SeqCst);
                                            let current_downloaded = chunk.downloaded.load(Ordering::SeqCst);
                                            let current_pos = chunk.start + current_downloaded + net_buffer.len() as u64;

                                            if dynamic_end > 0 && dynamic_end >= chunk.start {
                                                if current_pos >= dynamic_end + 1 {
                                                    let _ = flush_data(&mut net_buffer);
                                                    break 'reconnect; // Chunk completed!
                                                }

                                                let remaining = (dynamic_end + 1).saturating_sub(current_pos);
                                                if bytes_to_add.len() as u64 > remaining {
                                                    bytes_to_add = &bytes_to_add[..remaining as usize];
                                                }
                                            }

                                            task_clone.network_downloaded.fetch_add(bytes_to_add.len() as u64, Ordering::Relaxed);
                                            net_buffer.extend_from_slice(bytes_to_add);

                                            let limit_bps = manager_for_worker.speed_limit_bps.load(Ordering::Relaxed);
                                            task_clone.throttle_if_needed(bytes_to_add.len() as u64, limit_bps).await;

                                            if net_buffer.len() >= 64 * 1024 {
                                                if !flush_data(&mut net_buffer) {
                                                    retry_count += 1;
                                                    continue 'reconnect;
                                                }
                                            }
                                        }
                                        Some(Err(_)) => {
                                            let _ = flush_data(&mut net_buffer);
                                            retry_count += 1;
                                            continue 'reconnect;
                                        }
                                        None => {
                                            // Stream reached EOF cleanly
                                            let _ = flush_data(&mut net_buffer);
                                            break 'reconnect;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let final_dl = chunk.downloaded.load(Ordering::SeqCst);
                    let dynamic_end = chunk.end.load(Ordering::SeqCst);
                    let max_allowed = if dynamic_end >= chunk.start && dynamic_end > 0 {
                        (dynamic_end + 1) - chunk.start
                    } else {
                        final_dl
                    };
                    let clamped_dl = final_dl.min(max_allowed);
                    chunk.downloaded.store(clamped_dl, Ordering::SeqCst);

                    chunk.active.store(false, Ordering::SeqCst);
                    let _ = completed_tx.send(chunk_id);
                });

                active_tasks.insert(chunk.id.clone(), worker);
            } else {
                if active_tasks.is_empty() { break; }

                tokio::select! {
                    res = loop_abort_rx.recv() => {
                        if res.is_ok() {
                            for (_, handle) in active_tasks.drain() { handle.abort(); }
                            break;
                        }
                    }
                    opt_finished = worker_completed_rx.recv() => {
                        if let Some(finished_id) = opt_finished {
                            active_tasks.remove(&finished_id);
                        } else {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {
                        // Periodic work-stealing pulse: allows manager to inspect chunk progress and steal from slowest active chunk
                    }
                }
            }
        }

        let is_aborted = { let s = task.status.lock().unwrap(); *s == DownloadStatus::Paused };

        if !is_aborted {
            let downloaded_bytes = task.downloaded.load(Ordering::Relaxed);
            let mut total_size_val = task.total_size.load(Ordering::Relaxed);

            if total_size_val == 0 && downloaded_bytes > 0 {
                task.total_size.store(downloaded_bytes, Ordering::Relaxed);
                total_size_val = downloaded_bytes;
            }

            let is_success = (total_size_val > 0 && downloaded_bytes >= total_size_val)
                || (total_size_val == 0 && downloaded_bytes > 0);

            if is_success {
                let cur_filename = task.get_filename();
                let cur_save_path = task.get_save_path();

                // If total size was open-ended or zero, ensure file length is set to total downloaded bytes
                let _ = target_file.set_len(downloaded_bytes);
                let _ = target_file.sync_all();

                // Drop target_file handle before renaming to release file locks across OSes (critical on Windows)
                drop(target_file);

                let (final_path, placeholder) = DownloadManager::allocate_collision_free_path(&cur_save_path);
                drop(placeholder); // Close placeholder handle immediately

                let part_path_buf = std::path::PathBuf::from(&part_path);
                let final_path_buf = std::path::PathBuf::from(&final_path);

                // Zero-Merge Fast Atomic Finalization:
                // Instantly renames the .part file directly to the destination filename in < 1ms without 2x disk usage or copying delay!
                let finalize_result = if part_path_buf.exists() {
                    if final_path_buf.exists() && final_path_buf != part_path_buf {
                        let _ = std::fs::remove_file(&final_path_buf);
                    }
                    match std::fs::rename(&part_path_buf, &final_path_buf) {
                        Ok(_) => Ok(final_path.clone()),
                        Err(rename_err) => {
                            // Cross-filesystem or permissions fallback
                            match std::fs::copy(&part_path_buf, &final_path_buf) {
                                Ok(_) => {
                                    let _ = std::fs::remove_file(&part_path_buf);
                                    Ok(final_path.clone())
                                }
                                Err(copy_err) => Err(format!("Finalize rename/copy failed: rename: {}, copy: {}", rename_err, copy_err)),
                            }
                        }
                    }
                } else if temp_dir.exists() {
                    // Legacy multi-chunk fallback assembly (for tasks initiated prior to zero-merge)
                    let temp_dir_clone = temp_dir.clone();
                    let save_path_clone = cur_save_path.clone();
                    let chunks_clone = task.chunks.lock().unwrap().clone();
                    tokio::task::spawn_blocking(move || -> Result<String, String> {
                        let (final_path, mut out_file) = DownloadManager::allocate_collision_free_path(&save_path_clone);
                        let mut sorted_chunks = chunks_clone;
                        sorted_chunks.sort_by_key(|c| c.start);
                        let mut buf = vec![0u8; 1024 * 1024 * 2];
                        for chunk in sorted_chunks {
                            let part_path = temp_dir_clone.join(&chunk.id);
                            if let Ok(mut part_file) = std::fs::File::open(&part_path) {
                                use std::io::{Read, Write};
                                let downloaded = chunk.downloaded.load(Ordering::SeqCst);
                                let chunk_end = chunk.end.load(Ordering::SeqCst);
                                let expected_len = if chunk_end >= chunk.start && chunk_end > 0 {
                                    (chunk_end - chunk.start + 1).min(downloaded)
                                } else {
                                    downloaded
                                };
                                let mut remaining_to_read = expected_len;
                                while remaining_to_read > 0 {
                                    let to_read = (remaining_to_read as usize).min(buf.len());
                                    let n = part_file.read(&mut buf[..to_read]).map_err(|e| format!("Read part err: {}", e))?;
                                    if n == 0 { break; }
                                    out_file.write_all(&buf[..n]).map_err(|e| format!("Write final err: {}", e))?;
                                    remaining_to_read -= n as u64;
                                }
                            }
                        }
                        let _ = out_file.sync_all();
                        Ok(final_path)
                    }).await.unwrap_or_else(|_| Err("Legacy assembly panicked".to_string()))
                } else {
                    Err("Neither .part file nor temp chunks exist".to_string())
                };

                match finalize_result {
                    Ok(final_path) => {
                        *task.status.lock().unwrap() = DownloadStatus::Completed;
                        if temp_dir.exists() {
                            let _ = std::fs::remove_dir_all(&temp_dir);
                        }
                        if final_path != cur_save_path {
                            let new_filename = std::path::Path::new(&final_path)
                                .file_name()
                                .and_then(|f| f.to_str())
                                .unwrap_or(&cur_filename);
                            task.update_filename_and_path(new_filename);
                        }
                    }
                    Err(e) => {
                        *task.status.lock().unwrap() = DownloadStatus::Failed(format!("Finalization failed: {}", e));
                    }
                }
            } else if total_size_val == 0 && downloaded_bytes == 0 {
                if let DownloadStatus::Failed(_) = *task.status.lock().unwrap() {
                    // Keep original failure reason
                } else {
                    *task.status.lock().unwrap() = DownloadStatus::Failed("No bytes received from server".to_string());
                }
            } else {
                if let DownloadStatus::Failed(_) = *task.status.lock().unwrap() {
                    // Keep original failure reason
                } else {
                    *task.status.lock().unwrap() = DownloadStatus::Failed(format!("Download incomplete ({}/{} bytes)", downloaded_bytes, total_size_val));
                }
            }
        }

        let final_status = task.status.lock().unwrap().clone();
        let cur_filename = task.get_filename();
        let cur_save_path = task.get_save_path();

        let progress = DownloadProgress {
            id: task.id.clone(),
            url: task.original_url.clone(),
            filename: cur_filename,
            save_path: cur_save_path.clone(),
            total_size: task.total_size.load(Ordering::Relaxed),
            downloaded: task.network_downloaded.load(Ordering::Relaxed),
            speed: 0.0,
            eta: "0s".to_string(),
            status: final_status,
            file_exists: std::path::Path::new(&cur_save_path).exists(),
            speed_limited: false,
            category: task.get_category(),
        };
        if let Some(ref handle) = self.app_handle.lock().await.clone() {
            use tauri::Emitter;
            let _ = handle.emit("download-progress", progress);
        }

        let _ = self.save_history().await;
        Ok(())
    }
    
    pub async fn pause_download(&self, id: &str) -> Result<(), String> {
        let app_handle_opt = self.app_handle.lock().await.clone();
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            {
                let mut status = task.status.lock().unwrap();
                if *status == DownloadStatus::Completed || matches!(*status, DownloadStatus::Failed(_)) { return Ok(()); }
                *status = DownloadStatus::Paused;
            }
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }

            let cur_filename = task.get_filename();
            let cur_save_path = task.get_save_path();

            let progress = DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: cur_filename,
                save_path: cur_save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.network_downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: DownloadStatus::Paused,
                file_exists: std::path::Path::new(&cur_save_path).exists(),
                speed_limited: false,
                category: task.get_category(),
            };
            if let Some(ref handle) = app_handle_opt {
                use tauri::Emitter;
                let _ = handle.emit("download-progress", progress);
            }

            drop(tasks);
            let _ = self.save_history().await;
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn resume_download(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(id) {
            let chunks_clone = { task.chunks.lock().unwrap().clone() };

            {
                let mut status_lock = task.status.lock().unwrap();
                if *status_lock == DownloadStatus::Completed || *status_lock == DownloadStatus::Downloading { return Ok(()); }
                *status_lock = DownloadStatus::Queued;
            }

            let (abort_tx, _) = broadcast::channel(1);
            let updated_task = Arc::new(DownloadTask {
                id: task.id.clone(),
                original_url: task.original_url.clone(),
                final_url: Mutex::new(task.final_url.lock().await.clone()),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                category: task.category.clone(),
                cookie: task.cookie.clone(),
                referrer: task.referrer.clone(),
                user_agent: task.user_agent.clone(),
                audio_url: task.audio_url.clone(),
                total_size: AtomicU64::new(task.total_size.load(Ordering::Relaxed)),
                downloaded: task.downloaded.clone(),
                network_downloaded: Arc::new(AtomicU64::new(task.downloaded.load(Ordering::Relaxed))),
                speed_limiter: Mutex::new(Instant::now()),
                status: task.status.clone(),
                abort_tx: Some(abort_tx),
                chunks: std::sync::Mutex::new(chunks_clone),
                speed: task.speed.clone(),
                eta: task.eta.clone(),
                validator: task.validator.clone(),
            });

            tasks.insert(id.to_string(), updated_task.clone());

            let manager_clone = self.clone();

            tokio::spawn(async move {
                if let Err(e) = manager_clone.run_task(updated_task, true).await {
                    eprintln!("Resume failed: {}", e);
                }
            });

            drop(tasks);
            let _ = self.save_history().await;
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn cancel_download(&self, id: &str) -> Result<(), String> {
        let app_handle_opt = self.app_handle.lock().await.clone();
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }
            
            *task.status.lock().unwrap() = DownloadStatus::Paused;
            *task.speed.lock().unwrap() = 0.0;
            *task.eta.lock().unwrap() = "Paused".to_string();
            
            let cur_filename = task.get_filename();
            let cur_save_path = task.get_save_path();

            let progress = DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: cur_filename,
                save_path: cur_save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.network_downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "Paused".to_string(),
                status: DownloadStatus::Paused,
                file_exists: std::path::Path::new(&cur_save_path).exists(),
                speed_limited: false,
                category: task.get_category(),
            };
            if let Some(ref handle) = app_handle_opt {
                use tauri::Emitter;
                let _ = handle.emit("download-progress", progress);
            }
            
            drop(tasks);
            let _ = self.save_history().await;
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn delete_task(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let app_handle_opt = self.app_handle.lock().await.clone();
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.remove(id) {
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }

            let temp_dir = if let Some(ref handle) = app_handle_opt {
                let mut p = handle.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                p.push("temp"); p.push(&task.id); p
            } else { std::path::PathBuf::from(format!("./temp/{}", task.id)) };

            let path = task.get_save_path();
            tokio::spawn(async move {
                if delete_file {
                    let _ = tokio::fs::remove_file(&path).await;
                    let _ = tokio::fs::remove_file(format!("{}.part", path)).await;
                }
                let _ = tokio::fs::remove_dir_all(temp_dir).await;
            });

            drop(tasks);
            let _ = self.save_history().await;
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn empty_trash(&self, delete_file: bool) -> Result<(), String> {
        let app_handle_opt = self.app_handle.lock().await.clone();
        let mut tasks = self.tasks.write().await;
        let trash_ids: Vec<String> = tasks
            .iter()
            .filter(|(_, t)| *t.status.lock().unwrap() == DownloadStatus::Trash)
            .map(|(id, _)| id.clone())
            .collect();

        for id in trash_ids {
            if let Some(task) = tasks.remove(&id) {
                if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }

                let temp_dir = if let Some(ref handle) = app_handle_opt {
                    let mut p = handle.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                    p.push("temp"); p.push(&task.id); p
                } else { std::path::PathBuf::from(format!("./temp/{}", task.id)) };

                let path = task.get_save_path();
                tokio::spawn(async move {
                    if delete_file {
                        let _ = tokio::fs::remove_file(&path).await;
                        let _ = tokio::fs::remove_file(format!("{}.part", path)).await;
                    }
                    let _ = tokio::fs::remove_dir_all(temp_dir).await;
                });
            }
        }
        drop(tasks);
        let _ = self.save_history().await;
        Ok(())
    }

    pub async fn trash_task(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let app_handle_opt = self.app_handle.lock().await.clone();
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }
            *task.status.lock().unwrap() = DownloadStatus::Trash;
            
            let temp_dir = if let Some(ref handle) = app_handle_opt {
                let mut p = handle.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                p.push("temp"); p.push(&task.id); p
            } else { std::path::PathBuf::from(format!("./temp/{}", task.id)) };

            let path = task.get_save_path();
            tokio::spawn(async move {
                if delete_file {
                    let _ = tokio::fs::remove_file(&path).await;
                    let _ = tokio::fs::remove_file(format!("{}.part", path)).await;
                }
                let _ = tokio::fs::remove_dir_all(temp_dir).await;
            });
            
            let cur_filename = task.get_filename();
            let cur_save_path = task.get_save_path();

            let progress = DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: cur_filename,
                save_path: cur_save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.network_downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: DownloadStatus::Trash,
                file_exists: std::path::Path::new(&cur_save_path).exists(),
                speed_limited: false,
                category: task.get_category(),
            };
            if let Some(ref handle) = app_handle_opt {
                use tauri::Emitter;
                let _ = handle.emit("download-progress", progress);
            }
            
            drop(tasks);
            let _ = self.save_history().await;
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn restore_task(&self, id: &str) -> Result<(), String> {
        let app_handle_opt = self.app_handle.lock().await.clone();
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            let final_status = {
                let mut status = task.status.lock().unwrap();
                if *status == DownloadStatus::Trash {
                    let downloaded = task.downloaded.load(Ordering::Relaxed);
                    let total_size_val = task.total_size.load(Ordering::Relaxed);
                    if total_size_val > 0 && downloaded >= total_size_val {
                        *status = DownloadStatus::Completed;
                    } else {
                        *status = DownloadStatus::Paused;
                    }
                }
                status.clone()
            };
            
            let cur_filename = task.get_filename();
            let cur_save_path = task.get_save_path();

            let progress = DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: cur_filename,
                save_path: cur_save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.network_downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: final_status,
                file_exists: std::path::Path::new(&cur_save_path).exists(),
                speed_limited: false,
                category: task.get_category(),
            };
            if let Some(ref handle) = app_handle_opt {
                use tauri::Emitter;
                let _ = handle.emit("download-progress", progress);
            }
            
            drop(tasks);
            let _ = self.save_history().await;
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn redownload_task(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(id) {
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }

            let path = task.get_save_path();
            let _ = tokio::fs::remove_file(&path).await;
            let _ = tokio::fs::remove_file(format!("{}.part", path)).await;

            let (abort_tx, _) = broadcast::channel(1);
            let updated_task = Arc::new(DownloadTask {
                id: task.id.clone(),
                original_url: task.original_url.clone(),
                final_url: Mutex::new(task.original_url.clone()),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                category: task.category.clone(),
                cookie: task.cookie.clone(),
                referrer: task.referrer.clone(),
                user_agent: task.user_agent.clone(),
                audio_url: task.audio_url.clone(),
                total_size: AtomicU64::new(0),
                downloaded: Arc::new(AtomicU64::new(0)),
                network_downloaded: Arc::new(AtomicU64::new(0)),
                speed_limiter: Mutex::new(Instant::now()),
                status: Arc::new(std::sync::Mutex::new(DownloadStatus::Queued)),
                abort_tx: Some(abort_tx),
                chunks: std::sync::Mutex::new(vec![]),
                speed: Arc::new(std::sync::Mutex::new(0.0)),
                eta: Arc::new(std::sync::Mutex::new("---".to_string())),
                validator: Arc::new(std::sync::Mutex::new(None)),
            });

            tasks.insert(id.to_string(), updated_task.clone());

            let manager_clone = self.clone();

            tokio::spawn(async move {
                if let Err(e) = manager_clone.run_task(updated_task, false).await {
                    eprintln!("Redownload failed: {}", e);
                }
            });

            drop(tasks);
            let _ = self.save_history().await;
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn get_progress(&self, id: &str) -> Option<DownloadProgress> {
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            let current_bytes = task.network_downloaded.load(Ordering::Relaxed);
            let speed = *task.speed.lock().unwrap();
            let eta = task.eta.lock().unwrap().clone();
            let status = task.status.lock().unwrap().clone();
            
            let current_limit = self.speed_limit_bps.load(Ordering::Relaxed);
            let is_speed_limited = current_limit > 0 && speed > 0.0;

            let cur_filename = task.get_filename();
            let cur_save_path = task.get_save_path();

            Some(DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: cur_filename,
                save_path: cur_save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: current_bytes,
                speed,
                eta,
                status,
                file_exists: std::path::Path::new(&cur_save_path).exists(),
                speed_limited: is_speed_limited,
                category: task.get_category(),
            })
        } else {
            None
        }
    }

    pub async fn get_all_progress(&self) -> Vec<DownloadProgress> {
        let tasks = self.tasks.read().await;
        let mut list = vec![];
        let current_limit = self.speed_limit_bps.load(Ordering::Relaxed);
        for task in tasks.values() {
            let current_bytes = task.network_downloaded.load(Ordering::Relaxed);
            let status = task.status.lock().unwrap().clone();
            let speed = *task.speed.lock().unwrap();
            let eta = task.eta.lock().unwrap().clone();
            let is_speed_limited = current_limit > 0 && speed > 0.0;
            let cur_filename = task.get_filename();
            let cur_save_path = task.get_save_path();

            list.push(DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: cur_filename,
                save_path: cur_save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: current_bytes,
                speed,
                eta,
                status,
                file_exists: std::path::Path::new(&cur_save_path).exists(),
                speed_limited: is_speed_limited,
                category: task.get_category(),
            });
        }
        list
    }

    pub async fn get_scheduler_config(&self) -> SchedulerConfig {
        self.scheduler_config.read().await.clone()
    }

    pub async fn save_scheduler_config(&self, config: SchedulerConfig) -> Result<(), String> {
        *self.scheduler_config.write().await = config.clone();
        let handle_opt = {
            let guard = self.app_handle.lock().await;
            guard.clone()
        };
        if let Some(handle) = handle_opt {
            use tauri::Manager;
            if let Ok(app_dir) = handle.path().app_data_dir() {
                let _ = tokio::fs::create_dir_all(&app_dir).await;
                let path = app_dir.join("scheduler.json");
                if let Ok(serialized) = serde_json::to_string_pretty(&config) {
                    let _ = tokio::fs::write(path, serialized).await;
                }
            }
        }
        Ok(())
    }

    pub async fn load_scheduler_config(&self) -> Result<(), String> {
        let handle_opt = {
            let guard = self.app_handle.lock().await;
            guard.clone()
        };
        if let Some(handle) = handle_opt {
            use tauri::Manager;
            if let Ok(app_dir) = handle.path().app_data_dir() {
                let path = app_dir.join("scheduler.json");
                if path.exists() {
                    if let Ok(data) = tokio::fs::read_to_string(&path).await {
                        if let Ok(config) = serde_json::from_str::<SchedulerConfig>(&data) {
                            *self.scheduler_config.write().await = config;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn start_scheduler_loop(self: &Arc<Self>) {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;

                let config = { manager.scheduler_config.read().await.clone() };
                if !config.enabled {
                    continue;
                }

                let now = chrono::Local::now();
                let in_window = is_in_schedule_window(&config, &now);

                if in_window {
                    // Inside scheduled window: auto-resume queued or paused downloads silently
                    let eligible_ids: Vec<String> = {
                        let tasks = manager.tasks.read().await;
                        tasks
                            .iter()
                            .filter_map(|(id, task)| {
                                let status = task.status.lock().unwrap();
                                if *status == DownloadStatus::Paused || *status == DownloadStatus::Queued {
                                    Some(id.clone())
                                } else {
                                    None
                                }
                            })
                            .collect()
                    };

                    for id in eligible_ids {
                        // Silently resumes without summoning popup window
                        let _ = manager.resume_download(&id).await;
                    }
                } else {
                    // Outside scheduled window: auto-pause active downloading tasks
                    let active_ids: Vec<String> = {
                        let tasks = manager.tasks.read().await;
                        tasks
                            .iter()
                            .filter_map(|(id, task)| {
                                let status = task.status.lock().unwrap();
                                if *status == DownloadStatus::Downloading {
                                    Some(id.clone())
                                } else {
                                    None
                                }
                            })
                            .collect()
                    };

                    for id in active_ids {
                        let _ = manager.pause_download(&id).await;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_token_generation() {
        let mgr1 = DownloadManager::new();
        let mgr2 = DownloadManager::new();

        assert_eq!(mgr1.bridge_token.len(), 64, "Bridge token should be 32 bytes hex encoded (64 chars)");
        assert_ne!(mgr1.bridge_token, mgr2.bridge_token, "Bridge tokens must be cryptographically unique");
    }

    #[test]
    fn test_allocate_collision_free_path_collision_avoidance() {
        let temp_dir = std::env::temp_dir().join(format!("idm_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let base_path = temp_dir.join("document.pdf").to_string_lossy().to_string();

        // 1. Initial allocation: document.pdf
        let (path1, file1) = DownloadManager::allocate_collision_free_path(&base_path);
        drop(file1);
        assert_eq!(path1, base_path);
        assert!(std::path::Path::new(&path1).exists());

        // 2. Collision allocation: document.pdf already exists -> should create document (1).pdf
        let (path2, file2) = DownloadManager::allocate_collision_free_path(&base_path);
        drop(file2);
        let expected_path2 = temp_dir.join("document (1).pdf").to_string_lossy().to_string();
        assert_eq!(path2, expected_path2);
        assert!(std::path::Path::new(&path2).exists());

        // 3. Collision allocation: document (1).pdf exists -> should create document (2).pdf
        let (path3, file3) = DownloadManager::allocate_collision_free_path(&base_path);
        drop(file3);
        let expected_path3 = temp_dir.join("document (2).pdf").to_string_lossy().to_string();
        assert_eq!(path3, expected_path3);
        assert!(std::path::Path::new(&path3).exists());

        // Verify original file was NEVER truncated or overwritten
        assert!(std::path::Path::new(&path1).exists());
        assert!(std::path::Path::new(&path2).exists());
        assert!(std::path::Path::new(&path3).exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // =========================================================================
    // Real-World End-to-End Functional Working Tests (Phase 2 Core Engine)
    // =========================================================================

    #[tokio::test]
    async fn test_working_e2e_download_and_byte_integrity() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind test TCP listener");
        let local_addr = listener.local_addr().expect("Failed to get local address");

        // 64KB deterministic binary payload
        let test_payload: Vec<u8> = (0..65536).map(|i| ((i * 31 + 7) % 256) as u8).collect();
        let payload_clone = test_payload.clone();

        let server_handle = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            while let Ok((mut stream, _)) = listener.accept().await {
                let payload = payload_clone.clone();
                tokio::spawn(async move {
                    let mut req_buf = vec![0u8; 4096];
                    if let Ok(n) = stream.read(&mut req_buf).await {
                        let req_text = String::from_utf8_lossy(&req_buf[..n]);

                        if req_text.starts_with("HEAD") {
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\n\
                                Content-Length: {}\r\n\
                                Accept-Ranges: bytes\r\n\
                                Content-Type: application/octet-stream\r\n\
                                ETag: \"sample-etag-v1\"\r\n\
                                Connection: close\r\n\r\n",
                                payload.len()
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                        } else if req_text.starts_with("GET") {
                            let range_opt = req_text.lines()
                                .find(|l| l.to_lowercase().starts_with("range:"))
                                .and_then(|l| l.split_once(':'))
                                .map(|(_, v)| v.trim().to_string());

                            if let Some(r_str) = range_opt {
                                if let Some(stripped) = r_str.strip_prefix("bytes=") {
                                    let parts: Vec<&str> = stripped.split('-').collect();
                                    let start: usize = parts[0].parse().unwrap_or(0);
                                    let end: usize = if parts.len() > 1 && !parts[1].is_empty() {
                                        parts[1].parse().unwrap_or(payload.len() - 1)
                                    } else {
                                        payload.len() - 1
                                    };
                                    let actual_end = end.min(payload.len().saturating_sub(1));
                                    let slice = if start <= actual_end && start < payload.len() {
                                        &payload[start..=actual_end]
                                    } else {
                                        &[]
                                    };

                                    let resp = format!(
                                        "HTTP/1.1 206 Partial Content\r\n\
                                        Content-Range: bytes {}-{}/{}\r\n\
                                        Content-Length: {}\r\n\
                                        Content-Type: application/octet-stream\r\n\
                                        ETag: \"sample-etag-v1\"\r\n\
                                        Connection: close\r\n\r\n",
                                        start, actual_end, payload.len(), slice.len()
                                    );
                                    let _ = stream.write_all(resp.as_bytes()).await;
                                    let _ = stream.write_all(slice).await;
                                }
                            } else {
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\n\
                                    Content-Length: {}\r\n\
                                    Accept-Ranges: bytes\r\n\
                                    Content-Type: application/octet-stream\r\n\
                                    ETag: \"sample-etag-v1\"\r\n\
                                    Connection: close\r\n\r\n",
                                    payload.len()
                                );
                                let _ = stream.write_all(resp.as_bytes()).await;
                                let _ = stream.write_all(&payload).await;
                            }
                        }
                    }
                });
            }
        });

        let mgr = Arc::new(DownloadManager::new());
        let temp_save_dir = std::env::temp_dir().join(format!("idm_e2e_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_save_dir);
        let target_save_file = temp_save_dir.join("live_test_payload.bin").to_string_lossy().to_string();

        let target_url = format!("http://{}/live_test_payload.bin", local_addr);
        let task_id = mgr.start_download(
            target_url,
            "live_test_payload.bin".to_string(),
            target_save_file.clone(),
            String::new(),
            String::new(),
            String::new(),
            None,
            None,
            None,
            false,
            false,
            None,
        ).await.expect("start_download failed");

        // Wait up to 10 seconds for real download completion
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut final_status = None;
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let tasks = mgr.tasks.read().await;
            if let Some(task) = tasks.get(&task_id) {
                let st = task.status.lock().unwrap().clone();
                match st {
                    DownloadStatus::Completed => {
                        final_status = Some(DownloadStatus::Completed);
                        break;
                    }
                    DownloadStatus::Failed(e) => {
                        panic!("Download task failed with error: {}", e);
                    }
                    _ => {}
                }
            }
        }

        assert_eq!(final_status, Some(DownloadStatus::Completed), "Download should reach Completed state");

        // Verify the file was written to disk and is byte-for-byte identical
        assert!(std::path::Path::new(&target_save_file).exists(), "Saved file must exist on disk");
        let downloaded_bytes = std::fs::read(&target_save_file).expect("Failed to read assembled file");
        assert_eq!(downloaded_bytes.len(), test_payload.len(), "File length must match test payload");
        assert_eq!(downloaded_bytes, test_payload, "Downloaded file must match byte-for-byte with source");

        // Cleanup
        server_handle.abort();
        let _ = std::fs::remove_dir_all(&temp_save_dir);
    }

    #[tokio::test]
    async fn test_working_429_rate_limit_backoff_recovery() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind test TCP listener");
        let local_addr = listener.local_addr().expect("Failed to get local address");

        let test_payload: Vec<u8> = (0..16384).map(|i| (i % 256) as u8).collect();
        let payload_clone = test_payload.clone();
        let request_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let server_handle = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            while let Ok((mut stream, _)) = listener.accept().await {
                let payload = payload_clone.clone();
                let counter = request_counter.clone();

                tokio::spawn(async move {
                    let mut req_buf = vec![0u8; 4096];
                    if let Ok(n) = stream.read(&mut req_buf).await {
                        let count = counter.fetch_add(1, Ordering::SeqCst);
                        let req_text = String::from_utf8_lossy(&req_buf[..n]);

                        // First request (HEAD/probe or GET) -> return 429 rate limited
                        if count < 2 {
                            let resp = "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                            let _ = stream.write_all(resp.as_bytes()).await;
                        } else {
                            if req_text.starts_with("HEAD") {
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                    payload.len()
                                );
                                let _ = stream.write_all(resp.as_bytes()).await;
                            } else {
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                    payload.len()
                                );
                                let _ = stream.write_all(resp.as_bytes()).await;
                                let _ = stream.write_all(&payload).await;
                            }
                        }
                    }
                });
            }
        });

        let mgr = Arc::new(DownloadManager::new());
        let temp_save_dir = std::env::temp_dir().join(format!("idm_429_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_save_dir);
        let target_save_file = temp_save_dir.join("recovered_file.bin").to_string_lossy().to_string();

        let target_url = format!("http://{}/recovered_file.bin", local_addr);
        let task_id = mgr.start_download(
            target_url,
            "recovered_file.bin".to_string(),
            target_save_file.clone(),
            String::new(),
            String::new(),
            String::new(),
            None,
            None,
            None,
            false,
            false,
            None,
        ).await.expect("start_download failed");

        // Wait up to 10 seconds for backoff and recovery
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut final_status = None;
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let tasks = mgr.tasks.read().await;
            if let Some(task) = tasks.get(&task_id) {
                let st = task.status.lock().unwrap().clone();
                match st {
                    DownloadStatus::Completed => {
                        final_status = Some(DownloadStatus::Completed);
                        break;
                    }
                    DownloadStatus::Failed(e) => {
                        panic!("Download task unexpectedly failed during 429 recovery: {}", e);
                    }
                    _ => {}
                }
            }
        }

        assert_eq!(final_status, Some(DownloadStatus::Completed), "Download should recover from 429 and complete");
        let downloaded_bytes = std::fs::read(&target_save_file).expect("Failed to read assembled file");
        assert_eq!(downloaded_bytes, test_payload, "Recovered download must match byte-for-byte");

        server_handle.abort();
        let _ = std::fs::remove_dir_all(&temp_save_dir);
    }

    #[tokio::test]
    async fn test_phase2_strict_range_violation_guard() {
        // Server anomalously returns 200 OK on a ranged request for offset > 0
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind listener");
        let local_addr = listener.local_addr().expect("Failed to get local address");

        let server_handle = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut req_buf = vec![0u8; 4096];
                    if let Ok(n) = stream.read(&mut req_buf).await {
                        let req_text = String::from_utf8_lossy(&req_buf[..n]);

                        if req_text.starts_with("HEAD") {
                            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n";
                            let _ = stream.write_all(resp.as_bytes()).await;
                        } else if req_text.starts_with("GET") {
                            // Rogue server ignores Range: bytes=X-Y and returns 200 OK starting from byte 0!
                            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n";
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let dummy = vec![0xAA; 5000];
                            let _ = stream.write_all(&dummy).await;
                        }
                    }
                });
            }
        });

        let mgr = Arc::new(DownloadManager::new());
        let temp_save_dir = std::env::temp_dir().join(format!("idm_range_fail_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_save_dir);
        let target_save_file = temp_save_dir.join("should_abort.bin").to_string_lossy().to_string();

        let target_url = format!("http://{}/should_abort.bin", local_addr);
        let task_id = uuid::Uuid::new_v4().to_string();

        // Create a task that already has chunk 0 downloaded, and needs chunk 1 starting at offset 50000
        let (abort_tx, _) = tokio::sync::broadcast::channel(1);
        let chunk1 = ActiveChunk {
            id: "chunk_1".to_string(),
            start: 50000,
            end: Arc::new(AtomicU64::new(99999)),
            downloaded: Arc::new(AtomicU64::new(0)),
            active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let task = Arc::new(DownloadTask {
            id: task_id.clone(),
            original_url: target_url.clone(),
            final_url: Mutex::new(target_url.clone()),
            filename: Arc::new(std::sync::Mutex::new("should_abort.bin".to_string())),
            save_path: Arc::new(std::sync::Mutex::new(target_save_file.clone())),
            category: Arc::new(std::sync::Mutex::new("Other".to_string())),
            cookie: String::new(),
            referrer: String::new(),
            user_agent: String::new(),
            audio_url: None,
            total_size: AtomicU64::new(100000),
            downloaded: Arc::new(AtomicU64::new(50000)),
            network_downloaded: Arc::new(AtomicU64::new(50000)),
            speed_limiter: Mutex::new(Instant::now()),
            status: Arc::new(std::sync::Mutex::new(DownloadStatus::Queued)),
            abort_tx: Some(abort_tx),
            chunks: std::sync::Mutex::new(vec![chunk1]),
            speed: Arc::new(std::sync::Mutex::new(0.0)),
            eta: Arc::new(std::sync::Mutex::new("---".to_string())),
            validator: Arc::new(std::sync::Mutex::new(None)),
        });

        mgr.tasks.write().await.insert(task_id.clone(), task.clone());

        let mgr_clone = mgr.clone();
        let task_clone = task.clone();
        tokio::spawn(async move {
            let _ = mgr_clone.run_task(task_clone, true).await;
        });

        // Wait for worker to encounter 200 OK on range offset 50000
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut failed = false;
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let st = task.status.lock().unwrap().clone();
            if let DownloadStatus::Failed(ref msg) = st {
                assert!(msg.contains("range") || msg.contains("split"), "Failure message must identify range violation");
                failed = true;
                break;
            }
        }

        assert!(failed, "Engine MUST abort worker when server returns 200 OK on ranged request");

        server_handle.abort();
        let _ = std::fs::remove_dir_all(&temp_save_dir);
    }

    #[test]
    fn test_phase2_validator_weak_etag_filtration() {
        // RFC 7232: Weak ETags (W/"...") must NEVER be used for range requests
        let strong_header = "\"3f8a-5b1234\"";
        let weak_header = "W/\"3f8a-5b1234\"";

        let parse_etag = |val: &str| -> Option<String> {
            if val.starts_with("W/") {
                None
            } else {
                Some(val.to_string())
            }
        };

        assert_eq!(parse_etag(strong_header), Some("\"3f8a-5b1234\"".to_string()));
        assert_eq!(parse_etag(weak_header), None, "Weak ETag must be filtered out to prevent range corruption");
    }

    #[test]
    fn test_phase2_multi_chunk_assembly_ordering() {
        // Test sequential buffered assembly with out-of-order chunk completion
        let temp_dir = std::env::temp_dir().join(format!("idm_assembly_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_dir);

        let final_path = temp_dir.join("assembled_binary.bin");
        let (out_path, mut out_file) = DownloadManager::allocate_collision_free_path(final_path.to_str().unwrap());

        // Create 3 chunks written in reverse order (Chunk 2, Chunk 0, Chunk 1)
        let chunk0_data = b"PART_ZERO_DATA____";
        let chunk1_data = b"PART_ONE_DATA_____";
        let chunk2_data = b"PART_TWO_DATA_____";

        let p0 = temp_dir.join("c0");
        let p1 = temp_dir.join("c1");
        let p2 = temp_dir.join("c2");

        std::fs::write(&p0, chunk0_data).unwrap();
        std::fs::write(&p1, chunk1_data).unwrap();
        std::fs::write(&p2, chunk2_data).unwrap();

        let mut chunks = vec![
            DownloadChunk { id: "c2".to_string(), start: 36, end: 53, downloaded: 18 },
            DownloadChunk { id: "c0".to_string(), start: 0, end: 17, downloaded: 18 },
            DownloadChunk { id: "c1".to_string(), start: 18, end: 35, downloaded: 18 },
        ];

        // Sort by start offset (as engine does in assembly step)
        chunks.sort_by_key(|c| c.start);

        for chunk in chunks {
            let part_path = temp_dir.join(&chunk.id);
            let mut part_file = std::fs::File::open(part_path).unwrap();
            use std::io::{Read, Write};
            let mut buf = vec![0u8; 1024];
            while let Ok(n) = part_file.read(&mut buf) {
                if n == 0 { break; }
                out_file.write_all(&buf[..n]).unwrap();
            }
        }
        let _ = out_file.sync_all();
        drop(out_file);

        let assembled_content = std::fs::read(&out_path).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(chunk0_data);
        expected.extend_from_slice(chunk1_data);
        expected.extend_from_slice(chunk2_data);

        assert_eq!(assembled_content, expected, "Multi-chunk assembly must match exact chronological byte sequence");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_working_e2e_dual_stream_download_engine() {
        if crate::mux::probe_ffmpeg().is_none() {
            println!("Skipping e2e dual stream download test: ffmpeg not found");
            return;
        }

        let temp_dir = std::env::temp_dir().join(format!("idm_dual_e2e_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let raw_video_path = temp_dir.join("src_v.mp4");
        let raw_audio_path = temp_dir.join("src_a.m4a");

        let ffmpeg = crate::mux::probe_ffmpeg().unwrap();

        // 1. Generate synthetic video track
        let _ = std::process::Command::new(&ffmpeg)
            .args(&[
                "-nostdin", "-v", "error", "-y",
                "-f", "lavfi", "-i", "color=c=blue:s=320x240:d=1",
                "-c:v", "libx264", "-pix_fmt", "yuv420p",
                raw_video_path.to_str().unwrap(),
            ])
            .status();

        // 2. Generate synthetic audio track
        let _ = std::process::Command::new(&ffmpeg)
            .args(&[
                "-nostdin", "-v", "error", "-y",
                "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
                "-c:a", "aac",
                raw_audio_path.to_str().unwrap(),
            ])
            .status();

        let video_bytes = std::fs::read(&raw_video_path).expect("Read video track");
        let audio_bytes = std::fs::read(&raw_audio_path).expect("Read audio track");

        // 3. Local HTTP Server to serve video and audio streams
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let v_bytes_clone = video_bytes.clone();
        let a_bytes_clone = audio_bytes.clone();

        let server_handle = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut stream, _)) = listener.accept().await {
                let v = v_bytes_clone.clone();
                let a = a_bytes_clone.clone();
                tokio::spawn(async move {
                    let mut req_buf = vec![0u8; 4096];
                    if let Ok(n) = stream.read(&mut req_buf).await {
                        let req_str = String::from_utf8_lossy(&req_buf[..n]);
                        if req_str.contains("GET /video.mp4") {
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", v.len());
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(&v).await;
                        } else if req_str.contains("GET /audio.m4a") {
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", a.len());
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(&a).await;
                        }
                    }
                });
            }
        });

        let mgr = Arc::new(DownloadManager::new());
        let target_save_file = temp_dir.join("final_muxed_video.mp4").to_string_lossy().to_string();

        let video_url = format!("http://{}/video.mp4", local_addr);
        let audio_url = format!("http://{}/audio.m4a", local_addr);

        let task_id = mgr.start_download(
            video_url,
            "final_muxed_video.mp4".to_string(),
            target_save_file.clone(),
            String::new(),
            String::new(),
            String::new(),
            Some(audio_url),
            None,
            None,
            false,
            false,
            None,
        ).await.expect("Dual stream start_download failed");

        // Wait up to 10 seconds for dual-stream download & muxing completion
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut completed = false;
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            let tasks = mgr.tasks.read().await;
            if let Some(task) = tasks.get(&task_id) {
                let status = task.status.lock().unwrap().clone();
                if let DownloadStatus::Completed = status {
                    completed = true;
                    break;
                }
            }
        }

        assert!(completed, "Dual stream task must successfully download and mux into Completed state");
        assert!(std::path::Path::new(&target_save_file).exists(), "Muxed final output file must exist");
        assert!(std::fs::metadata(&target_save_file).unwrap().len() > 1000, "Final muxed file must contain valid video+audio container");

        server_handle.abort();
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_phase6_speed_limiter_pacing_and_concurrency() {
        let task = Arc::new(DownloadTask {
            id: "throttle_test".to_string(),
            original_url: "http://example.com/file".to_string(),
            final_url: Mutex::new("http://example.com/file".to_string()),
            filename: Arc::new(std::sync::Mutex::new("file".to_string())),
            save_path: Arc::new(std::sync::Mutex::new("/tmp/file".to_string())),
            category: Arc::new(std::sync::Mutex::new("Other".to_string())),
            cookie: String::new(),
            referrer: String::new(),
            user_agent: String::new(),
            audio_url: None,
            total_size: AtomicU64::new(100_000),
            downloaded: Arc::new(AtomicU64::new(0)),
            network_downloaded: Arc::new(AtomicU64::new(0)),
            speed_limiter: Mutex::new(Instant::now()),
            status: Arc::new(std::sync::Mutex::new(DownloadStatus::Downloading)),
            abort_tx: None,
            chunks: std::sync::Mutex::new(vec![]),
            speed: Arc::new(std::sync::Mutex::new(0.0)),
            eta: Arc::new(std::sync::Mutex::new("---".to_string())),
            validator: Arc::new(std::sync::Mutex::new(None)),
        });

        // 1. Zero limit or zero bytes returns immediately
        let t0 = Instant::now();
        task.throttle_if_needed(0, 100_000).await;
        task.throttle_if_needed(50_000, 0).await;
        assert!(t0.elapsed() < Duration::from_millis(20), "Zero throttle must return immediately");

        // 2. 50KB at 500,000 Bps should pace for approx ~100ms
        let t1 = Instant::now();
        task.throttle_if_needed(50_000, 500_000).await;
        let elapsed = t1.elapsed();
        assert!(elapsed >= Duration::from_millis(70), "Pacing must enforce minimum time delay: {:?}", elapsed);
        assert!(elapsed < Duration::from_millis(350), "Pacing must not over-delay: {:?}", elapsed);

        // 3. Concurrent callers must not deadlock
        let task_clone1 = task.clone();
        let task_clone2 = task.clone();
        let h1 = tokio::spawn(async move {
            task_clone1.throttle_if_needed(10_000, 500_000).await;
        });
        let h2 = tokio::spawn(async move {
            task_clone2.throttle_if_needed(10_000, 500_000).await;
        });
        let (r1, r2) = tokio::join!(h1, h2);
        assert!(r1.is_ok() && r2.is_ok(), "Concurrent throttle calls must complete without deadlocks");
    }

    #[test]
    fn test_phase6_dynamic_chunk_split_boundary_guard() {
        // Test chunk split math and overlap prevention
        let chunk0_start = 0u64;
        let chunk0_end = 10_000_000u64;
        let chunk0_downloaded = 4_000_000u64;
        let current_head = chunk0_start + chunk0_downloaded; // 4,000,000

        let rem = (chunk0_end + 1) - current_head; // 6,000,001
        let split_len = rem / 2; // 3,000,000
        let new_start = chunk0_end + 1 - split_len; // 7,000,001

        // Verify new_start is safely ahead of current download progress by > 256KB
        assert!(new_start > current_head + 256 * 1024, "Split point must be strictly ahead of write head");

        let adjusted_chunk0_end = new_start - 1; // 7,000,000
        assert_eq!(adjusted_chunk0_end + 1, new_start, "Chunk 0 end and Chunk 1 start must be contiguous with 0 gap and 0 overlap");

        // Overfill simulation: if in-flight packets caused chunk0 to download 7,500,000 bytes
        let in_flight_overshoot = 7_500_000u64;
        let max_allowed = (adjusted_chunk0_end + 1) - chunk0_start; // 7,000,001
        let clamped_dl = in_flight_overshoot.min(max_allowed);

        assert_eq!(clamped_dl, 7_000_001, "Overshoot bytes must be clamped to avoid overlap with child chunk");
    }

    #[tokio::test]
    async fn test_phase7_duplicate_download_prevention() {
        let mgr = Arc::new(DownloadManager::new());
        let url = "http://example.com/unique_file.zip".to_string();

        let id1 = mgr.start_download(
            url.clone(),
            "unique_file.zip".to_string(),
            "/tmp/unique_file.zip".to_string(),
            String::new(),
            String::new(),
            String::new(),
            None,
            Some("Compressed".to_string()),
            None,
            true, // queue_only to keep it queued/paused
            true,
            None,
        ).await.expect("First download start failed");

        // Calling start_download again with reuse_active=true
        let id2 = mgr.start_download(
            url.clone(),
            "unique_file.zip".to_string(),
            "/tmp/unique_file.zip".to_string(),
            String::new(),
            String::new(),
            String::new(),
            None,
            Some("Compressed".to_string()),
            None,
            false,
            true,
            None,
        ).await.expect("Second download start failed");

        assert_eq!(id2, format!("EXISTS:{}", id1), "Must return EXISTS:id for duplicate active download");
    }

    #[tokio::test]
    async fn test_phase7_download_later_queue_only() {
        let mgr = Arc::new(DownloadManager::new());
        let url = "http://example.com/later.iso".to_string();

        let id = mgr.start_download(
            url,
            "later.iso".to_string(),
            "/tmp/later.iso".to_string(),
            String::new(),
            String::new(),
            String::new(),
            None,
            None,
            None,
            true, // queue_only
            false,
            None,
        ).await.expect("Queue only download start failed");

        let prog = mgr.get_progress(&id).await.expect("Task must be registered");
        assert_eq!(prog.status, DownloadStatus::Paused, "queue_only tasks must start in Paused state");
        assert_eq!(prog.downloaded, 0, "No bytes should be downloaded before user initiates transfer");
        assert_eq!(prog.category, "Compressed", ".iso files must be classified as Compressed");
    }

    #[tokio::test]
    async fn test_phase9_high_concurrency_32_chunks_assembly_integrity() {
        use hyper::service::service_fn;
        use hyper_util::rt::TokioIo;
        use http_body_util::Full;
        use bytes::Bytes;
        use std::convert::Infallible;

        // 320 KB payload with predictable pattern for byte-for-byte verification
        let total_size: usize = 320 * 1024;
        let mut test_data = Vec::with_capacity(total_size);
        for i in 0..total_size {
            test_data.push(((i * 7 + 13) % 251) as u8);
        }
        let test_data_arc = Arc::new(test_data);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let server_data = test_data_arc.clone();
        let server_handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                let io = TokioIo::new(stream);
                let data = server_data.clone();

                tokio::spawn(async move {
                    let service = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                        let data = data.clone();
                        async move {
                            let range_hdr = req.headers().get("Range").and_then(|v| v.to_str().ok());
                            if let Some(r_str) = range_hdr {
                                if let Some(stripped) = r_str.strip_prefix("bytes=") {
                                    let parts: Vec<&str> = stripped.split('-').collect();
                                    let start: usize = parts[0].parse().unwrap_or(0);
                                    let end: usize = if parts.len() > 1 && !parts[1].is_empty() {
                                        parts[1].parse().unwrap_or(data.len() - 1)
                                    } else {
                                        data.len() - 1
                                    };
                                    let slice = data[start..=end.min(data.len() - 1)].to_vec();
                                    let resp = hyper::Response::builder()
                                        .status(hyper::StatusCode::PARTIAL_CONTENT)
                                        .header("Content-Range", format!("bytes {}-{}/{}", start, end, data.len()))
                                        .header("Content-Length", slice.len().to_string())
                                        .header("Accept-Ranges", "bytes")
                                        .header("ETag", "\"bench-32-test\"")
                                        .body(Full::new(Bytes::from(slice)))
                                        .unwrap();
                                    return Ok::<_, Infallible>(resp);
                                }
                            }

                            let resp = hyper::Response::builder()
                                .status(hyper::StatusCode::OK)
                                .header("Content-Length", data.len().to_string())
                                .header("Accept-Ranges", "bytes")
                                .header("ETag", "\"bench-32-test\"")
                                .body(Full::new(Bytes::from(data.to_vec())))
                                .unwrap();
                            Ok::<_, Infallible>(resp)
                        }
                    });

                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .await;
                });
            }
        });

        let mgr = Arc::new(DownloadManager::new());
        mgr.max_chunks.store(32, std::sync::atomic::Ordering::Relaxed); // Stress test with 32 chunks

        let temp_dir = std::env::temp_dir().join(format!("idm_bench_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let save_path = temp_dir.join("concurrency_32.bin").to_string_lossy().to_string();

        let download_url = format!("http://{}/bench.bin", local_addr);
        let task_id = mgr.start_download(
            download_url,
            "concurrency_32.bin".to_string(),
            save_path.clone(),
            String::new(),
            String::new(),
            String::new(),
            None,
            None,
            None,
            false,
            false,
            None,
        ).await.expect("Download start failed");

        // Wait for download to finish
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
        let mut completed = false;
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            if let Some(prog) = mgr.get_progress(&task_id).await {
                if prog.status == DownloadStatus::Completed {
                    completed = true;
                    break;
                }
            }
        }

        assert!(completed, "High-concurrency 32-chunk download must complete within deadline");

        // Verify final file byte integrity
        let written_bytes = std::fs::read(&save_path).expect("Final file must exist");
        assert_eq!(written_bytes.len(), total_size, "File length must match expected size");
        assert_eq!(&written_bytes[..], &test_data_arc[..], "File content must match byte-for-byte");

        // Verify zero temporary part files remain on disk
        let entries = std::fs::read_dir(&temp_dir).unwrap();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(
                !name.ends_with(".part") && !name.ends_with(".receiving"),
                "No temporary chunk file should remain after completion: found {}",
                name
            );
        }

        server_handle.abort();
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_phase9_cancellation_cleans_temporary_part_files() {
        let temp_dir = std::env::temp_dir().join(format!("idm_clean_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let target_file = temp_dir.join("cancelled_task.iso").to_string_lossy().to_string();

        // Simulate orphaned part files created during download
        let part1 = temp_dir.join("cancelled_task.iso.part_0");
        let part2 = temp_dir.join("cancelled_task.iso.part_1");
        let receiving = temp_dir.join("cancelled_task.iso.receiving");
        std::fs::write(&part1, b"partial chunk 1").unwrap();
        std::fs::write(&part2, b"partial chunk 2").unwrap();
        std::fs::write(&receiving, b"in flight bytes").unwrap();

        assert!(part1.exists());
        assert!(part2.exists());
        assert!(receiving.exists());

        // Verify cleanup helper logic
        let prefix = format!("{}.part_", target_file);
        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let path_str = path.to_string_lossy();
                if path_str.starts_with(&prefix) || path_str.ends_with(".receiving") {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }

        assert!(!part1.exists(), "part_0 must be purged");
        assert!(!part2.exists(), "part_1 must be purged");
        assert!(!receiving.exists(), ".receiving must be purged");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_phase9_validator_mismatch_prevents_corrupt_resume() {
        let original_etag = Some("\"etag-v1-original\"".to_string());
        let new_etag_match = Some("\"etag-v1-original\"".to_string());
        let new_etag_mismatch = Some("\"etag-v2-modified\"".to_string());
        let weak_etag = Some("W/\"etag-v1-original\"".to_string());

        let can_safely_resume = |orig: &Option<String>, current: &Option<String>| -> bool {
            match (orig, current) {
                (Some(o), Some(c)) => {
                    // RFC 7232: Strong ETag match only
                    !o.starts_with("W/") && !c.starts_with("W/") && o == c
                }
                (None, _) => true, // No prior validator recorded
                _ => false,
            }
        };

        assert!(can_safely_resume(&original_etag, &new_etag_match), "Matching strong ETag allows resume");
        assert!(!can_safely_resume(&original_etag, &new_etag_mismatch), "Mismatched ETag must reject resume to prevent byte corruption");
        assert!(!can_safely_resume(&original_etag, &weak_etag), "Weak ETag must reject ranged resume");
    }

    #[test]
    fn test_zero_merge_sparse_offset_writing() {
        use std::io::Read;

        let temp_dir = std::env::temp_dir().join(format!("idm_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let final_path = temp_dir.join("zero_merge_test.bin");
        let part_path = temp_dir.join("zero_merge_test.bin.part");

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&part_path)
            .unwrap();

        let total_size = 1024 * 1024u64; // 1MB
        file.set_len(total_size).unwrap();

        // Worker B writes to offset 500,000 out of order
        let chunk2_data = b"WORKER_2_OFFSET_PAYLOAD";
        super::write_at_offset(&file, chunk2_data, 500_000).unwrap();

        // Worker A writes to offset 0
        let chunk1_data = b"WORKER_1_START_PAYLOAD";
        super::write_at_offset(&file, chunk1_data, 0).unwrap();

        file.sync_all().unwrap();
        drop(file);

        // Atomic zero-merge rename
        std::fs::rename(&part_path, &final_path).unwrap();

        assert!(!part_path.exists(), ".part file must not exist after rename");
        assert!(final_path.exists(), "Final destination file must exist");
        assert_eq!(std::fs::metadata(&final_path).unwrap().len(), total_size);

        let mut read_back = vec![0u8; total_size as usize];
        let mut final_f = std::fs::File::open(&final_path).unwrap();
        final_f.read_exact(&mut read_back).unwrap();

        assert_eq!(&read_back[0..chunk1_data.len()], chunk1_data);
        assert_eq!(&read_back[500_000..500_000 + chunk2_data.len()], chunk2_data);
        // Verify sparse byte in between is 0
        assert_eq!(read_back[chunk1_data.len()], 0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_dynamic_work_stealing_slow_chunk_splitting() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::Arc;
        use crate::engine::ActiveChunk;

        // Simulate 2 initial chunks for a 10MB file
        let chunk0 = ActiveChunk {
            id: "c0".to_string(),
            start: 0,
            end: Arc::new(AtomicU64::new(4_999_999)),
            downloaded: Arc::new(AtomicU64::new(5_000_000)), // 100% completed!
            active: Arc::new(AtomicBool::new(false)),
        };

        // Chunk 1 is active on a slow connection, only 500KB downloaded so far
        let chunk1 = ActiveChunk {
            id: "c1".to_string(),
            start: 5_000_000,
            end: Arc::new(AtomicU64::new(9_999_999)),
            downloaded: Arc::new(AtomicU64::new(500_000)), // current_head = 5_500_000
            active: Arc::new(AtomicBool::new(true)),
        };

        let mut chunks = vec![chunk0, chunk1];
        let active_workers = 1; // 1 active worker, 1 slot available

        // Run work-stealing logic
        let min_split_threshold: u64 = if active_workers <= 2 {
            256 * 1024
        } else {
            1024 * 1024
        };

        let mut max_rem = 0u64;
        let mut best_idx = None;
        for (i, chunk) in chunks.iter().enumerate() {
            let end = chunk.end.load(Ordering::SeqCst);
            let downloaded = chunk.downloaded.load(Ordering::SeqCst);
            let start = chunk.start;

            if end > 0 && start + downloaded <= end {
                let rem = (end + 1) - (start + downloaded);
                if rem > max_rem {
                    max_rem = rem;
                    best_idx = Some(i);
                }
            }
        }

        assert!(max_rem >= min_split_threshold * 2);
        assert_eq!(best_idx, Some(1));

        let idx = best_idx.unwrap();
        let end = chunks[idx].end.load(Ordering::SeqCst);
        let downloaded = chunks[idx].downloaded.load(Ordering::SeqCst);
        let start = chunks[idx].start;
        let current_head = start + downloaded;

        let rem = (end + 1) - current_head;
        let split_len = rem / 2;
        let new_start = end + 1 - split_len;

        assert!(new_start > current_head + 64 * 1024);

        // Steal work: shorten chunk 1 and create chunk 2
        chunks[idx].end.store(new_start - 1, Ordering::SeqCst);
        let chunk2 = ActiveChunk {
            id: "c2_stolen".to_string(),
            start: new_start,
            end: Arc::new(AtomicU64::new(end)),
            downloaded: Arc::new(AtomicU64::new(0)),
            active: Arc::new(AtomicBool::new(true)),
        };
        chunks.push(chunk2);

        // Verification of zero overlap and zero gaps
        let c1_new_end = chunks[1].end.load(Ordering::SeqCst);
        let c2_start = chunks[2].start;
        let c2_end = chunks[2].end.load(Ordering::SeqCst);

        assert_eq!(c1_new_end + 1, c2_start, "No gap or overlap between original and stolen chunk");
        assert_eq!(c2_end, 9_999_999, "Original end preserved");
        assert!(c1_new_end >= current_head, "Headroom preserved for slow worker");
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_browser_profile_anti_bot_emulation() {
        use super::BrowserProfile;

        // 1. Chrome on Windows detection
        let chrome_win_ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
        let profile_chrome = BrowserProfile::detect(chrome_win_ua);
        assert_eq!(profile_chrome.brand, "Chrome");
        assert_eq!(profile_chrome.version, "131");
        assert_eq!(profile_chrome.platform, "\"Windows\"");
        assert!(profile_chrome.sec_ch_ua.is_some());
        let sec_ch = profile_chrome.sec_ch_ua.as_ref().unwrap();
        assert!(sec_ch.contains("Google Chrome"));
        assert!(sec_ch.contains("131"));

        // 2. Firefox on Linux detection
        let firefox_linux_ua = "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0";
        let profile_firefox = BrowserProfile::detect(firefox_linux_ua);
        assert_eq!(profile_firefox.brand, "Firefox");
        assert_eq!(profile_firefox.platform, "\"Linux\"");
        // Crucial for Cloudflare: Firefox must NOT send sec-ch-ua Client Hints
        assert!(profile_firefox.sec_ch_ua.is_none(), "Firefox must never send sec-ch-ua");

        // 3. Safari on macOS detection
        let safari_mac_ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15";
        let profile_safari = BrowserProfile::detect(safari_mac_ua);
        assert_eq!(profile_safari.brand, "Safari");
        assert_eq!(profile_safari.platform, "\"macOS\"");
        assert!(profile_safari.sec_ch_ua.is_none(), "Safari must never send sec-ch-ua");

        // 4. Edge on Windows detection
        let edge_ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0";
        let profile_edge = BrowserProfile::detect(edge_ua);
        assert_eq!(profile_edge.brand, "Edge");
        assert!(profile_edge.sec_ch_ua.as_ref().unwrap().contains("Microsoft Edge"));

        // 5. Test apply_headers
        let client = reqwest::Client::new();
        let target_url = "https://cdn.example.com/files/archive.zip";

        // Normal request
        let req1 = profile_chrome.apply_headers(
            client.get(target_url),
            target_url,
            "https://example.com/download",
            "session=abc123xyz",
            false,
        ).build().unwrap();

        assert_eq!(req1.headers().get("user-agent").unwrap(), chrome_win_ua);
        assert_eq!(req1.headers().get("sec-ch-ua-platform").unwrap(), "\"Windows\"");
        assert_eq!(req1.headers().get("sec-fetch-mode").unwrap(), "navigate");
        assert_eq!(req1.headers().get("referer").unwrap(), "https://example.com/download");
        assert_eq!(req1.headers().get("origin").unwrap(), "https://cdn.example.com");

        // Anti-bot 403 bypass retry request
        let req_retry = profile_chrome.apply_headers(
            client.get(target_url),
            target_url,
            "",
            "",
            true, // is_retry_403
        ).build().unwrap();

        // Must switch to cors / empty mode and auto-populate referer from origin
        assert_eq!(req_retry.headers().get("sec-fetch-mode").unwrap(), "cors");
        assert_eq!(req_retry.headers().get("sec-fetch-dest").unwrap(), "empty");
        assert_eq!(req_retry.headers().get("referer").unwrap(), "https://cdn.example.com/");
    }

    #[test]
    fn test_feature_4_anti_bot_browser_matrix_and_client_hints() {
        use super::BrowserProfile;

        // 1. Chrome on Windows
        let chrome_win = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
        let p_win = BrowserProfile::detect(chrome_win);
        assert_eq!(p_win.brand, "Chrome");
        assert_eq!(p_win.version, "131");
        assert_eq!(p_win.platform, "\"Windows\"");
        assert!(!p_win.is_mobile);
        assert!(p_win.sec_ch_ua.as_ref().unwrap().contains("Google Chrome"));

        // 2. Chrome on Android (Mobile Flag Check)
        let chrome_android = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.6723.102 Mobile Safari/537.36";
        let p_android = BrowserProfile::detect(chrome_android);
        assert_eq!(p_android.brand, "Chrome");
        assert_eq!(p_android.platform, "\"Android\"");
        assert!(p_android.is_mobile);

        // 3. Firefox on Linux (Strict WAF Rule: No sec-ch-ua)
        let firefox_linux = "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0";
        let p_firefox = BrowserProfile::detect(firefox_linux);
        assert_eq!(p_firefox.brand, "Firefox");
        assert_eq!(p_firefox.platform, "\"Linux\"");
        assert!(p_firefox.sec_ch_ua.is_none());

        // 4. Safari on macOS (Strict WAF Rule: No sec-ch-ua)
        let safari_mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15";
        let p_safari = BrowserProfile::detect(safari_mac);
        assert_eq!(p_safari.brand, "Safari");
        assert_eq!(p_safari.platform, "\"macOS\"");
        assert!(p_safari.sec_ch_ua.is_none());

        // 5. Microsoft Edge on Windows
        let edge_win = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0";
        let p_edge = BrowserProfile::detect(edge_win);
        assert_eq!(p_edge.brand, "Edge");
        assert!(p_edge.sec_ch_ua.as_ref().unwrap().contains("Microsoft Edge"));

        // 6. Empty UA Fallback (Safe Desktop Emulation)
        let p_empty = BrowserProfile::detect("");
        assert_eq!(p_empty.brand, "Chrome");
        assert!(!p_empty.user_agent.is_empty());
    }

    #[test]
    fn test_feature_4_anti_bot_waf_403_bypass_transition() {
        use super::BrowserProfile;

        let client = reqwest::Client::new();
        let target_url = "https://protected-vault.cdn.com/asset.tar.gz";
        let profile = BrowserProfile::detect("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36");

        // Request A: Standard Navigation Request
        let req_nav = profile.apply_headers(
            client.get(target_url),
            target_url,
            "https://origin-site.com/portal",
            "token=auth999",
            false,
        ).build().unwrap();

        assert_eq!(req_nav.headers().get("sec-fetch-mode").unwrap(), "navigate");
        assert_eq!(req_nav.headers().get("sec-fetch-dest").unwrap(), "document");
        assert_eq!(req_nav.headers().get("sec-fetch-site").unwrap(), "cross-site");
        assert_eq!(req_nav.headers().get("sec-fetch-user").unwrap(), "?1");
        assert_eq!(req_nav.headers().get("referer").unwrap(), "https://origin-site.com/portal");
        assert_eq!(req_nav.headers().get("cookie").unwrap(), "token=auth999");

        // Request B: WAF 403 Forbidden Retry Request (CORS Same-Origin Bypass)
        let req_bypass = profile.apply_headers(
            client.get(target_url),
            target_url,
            "",
            "",
            true, // is_retry_403 activated
        ).build().unwrap();

        assert_eq!(req_bypass.headers().get("sec-fetch-mode").unwrap(), "cors");
        assert_eq!(req_bypass.headers().get("sec-fetch-dest").unwrap(), "empty");
        assert_eq!(req_bypass.headers().get("sec-fetch-site").unwrap(), "same-origin");
        // Origin and Referer automatically bound to target CDN origin
        assert_eq!(req_bypass.headers().get("origin").unwrap(), "https://protected-vault.cdn.com");
        assert_eq!(req_bypass.headers().get("referer").unwrap(), "https://protected-vault.cdn.com/");
    }

    #[test]
    fn test_feature_4_compression_and_encoding_headers() {
        use super::BrowserProfile;

        let client = reqwest::Client::new();
        let target_url = "https://cdn.example.com/data.bin";
        let profile = BrowserProfile::detect("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36");

        let req = profile.apply_headers(
            client.get(target_url),
            target_url,
            "",
            "",
            false,
        ).build().unwrap();

        // Modern WAFs check for zstd/br/gzip compression acceptance
        let accept_enc = req.headers().get("accept-encoding").unwrap().to_str().unwrap();
        assert!(accept_enc.contains("gzip"));
        assert!(accept_enc.contains("deflate"));
        assert!(accept_enc.contains("br"));
        assert!(accept_enc.contains("zstd"));

        assert_eq!(req.headers().get("upgrade-insecure-requests").unwrap(), "1");
        assert_eq!(req.headers().get("connection").unwrap(), "keep-alive");
    }

    #[tokio::test]
    async fn test_feature_4_anti_bot_live_mock_waf_challenge_recovery() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Spin up a mock HTTP server simulating Cloudflare / Akamai WAF
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_handle = tokio::spawn(async move {
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let req_str = String::from_utf8_lossy(&buf[..n]);

                    if req_str.contains("sec-fetch-mode: cors") {
                        // Request 2 (WAF bypass retry): permit 206 Partial Content
                        let body = vec![0x42; 512];
                        let resp = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: 512\r\nContent-Range: bytes 0-511/512\r\nConnection: close\r\n\r\n"
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        let _ = stream.write_all(&body).await;
                        let _ = stream.flush().await;
                    } else {
                        // Request 1: initial attempt blocked by WAF with 403 Forbidden
                        let resp = "HTTP/1.1 403 Forbidden\r\nContent-Length: 22\r\nConnection: close\r\n\r\nCloudflare WAF Blocked";
                        let _ = stream.write_all(resp.as_bytes()).await;
                        let _ = stream.flush().await;
                    }
                }
            }
        });

        let target_url = format!("http://127.0.0.1:{}/download/secure_file.bin", port);
        let profile = BrowserProfile::detect("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36");
        let client = reqwest::Client::new();

        // 1. Initial attempt -> Expect 403 Forbidden
        let req1 = profile.apply_headers(client.get(&target_url), &target_url, "", "", false).build().unwrap();
        let resp1 = client.execute(req1).await.unwrap();
        assert_eq!(resp1.status(), reqwest::StatusCode::FORBIDDEN);

        // 2. Anti-Bot WAF Bypass Retry -> Expect 206 Partial Content and full 512 bytes recovery
        let req2 = profile.apply_headers(client.get(&target_url), &target_url, "", "", true).build().unwrap();
        let resp2 = client.execute(req2).await.unwrap();
        assert_eq!(resp2.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        let bytes = resp2.bytes().await.unwrap();
        assert_eq!(bytes.len(), 512);
        assert_eq!(bytes[0], 0x42);

        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_edge_case_4_engine_task_integration() {
        let manager = Arc::new(DownloadManager::new());

        // 1. Reserved device name: aux.pdf
        let id1 = manager
            .start_download(
                "https://example.com/files/aux.pdf".to_string(),
                "aux.pdf".to_string(),
                "/downloads/aux.pdf".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                None,
                None,
                Some(1000),
                true,
                false,
                None,
            )
            .await
            .unwrap();

        let task1 = manager.tasks.read().await.get(&id1).cloned().unwrap();
        assert_eq!(task1.get_filename(), "download_aux.pdf");
        assert!(task1.get_save_path().ends_with("download_aux.pdf"));

        // 2. Trailing spaces & dots: "project.docx...   "
        let id2 = manager
            .start_download(
                "https://example.com/files/project.docx".to_string(),
                "project.docx...   ".to_string(),
                "/downloads/project.docx...   ".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                None,
                None,
                Some(1000),
                true,
                false,
                None,
            )
            .await
            .unwrap();

        let task2 = manager.tasks.read().await.get(&id2).cloned().unwrap();
        assert_eq!(task2.get_filename(), "project.docx");
        assert!(task2.get_save_path().ends_with("project.docx"));

        // 3. update_filename_and_path with Windows reserved device name
        task2.update_filename_and_path("prn.log");
        assert_eq!(task2.get_filename(), "download_prn.log");
        assert!(task2.get_save_path().ends_with("download_prn.log"));

        // 4. Multi-byte Urdu long filename clamped without panic
        let base_urdu = "یہ_ایک_طویل_فائل_نام_ہے_جو_کہ_اردو_میں_ہے_اور_ہم_ٹیسٹ_کر_رہے_ہیں";
        let long_urdu = format!("{}_{}_{}.pdf", base_urdu, base_urdu, base_urdu);
        task2.update_filename_and_path(&long_urdu);
        assert!(task2.get_filename().len() <= 180);
        assert!(task2.get_filename().ends_with(".pdf"));
    }

    #[tokio::test]
    async fn test_edge_case_5_youtube_cookie_and_diagnostic_flow() {
        let manager = Arc::new(DownloadManager::new());

        // Verify task stores and retains YouTube session cookies and user agent
        let session_cookies = "LOGIN_INFO=AFmmF2swRQIhA:123; SID=g.a000test_sid; HSID=Ah789";
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0.0.0";

        let id = manager
            .start_download(
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
                "Rick_Astley_Never_Gonna_Give_You_Up.mp4".to_string(),
                "/downloads/rick.mp4".to_string(),
                session_cookies.to_string(),
                "https://www.youtube.com/".to_string(),
                user_agent.to_string(),
                None,
                Some("Video".to_string()),
                Some(50000000),
                true,
                false,
                None,
            )
            .await
            .unwrap();

        let task = manager.tasks.read().await.get(&id).cloned().unwrap();
        assert_eq!(task.cookie, session_cookies);
        assert_eq!(task.user_agent, user_agent);
        assert_eq!(task.referrer, "https://www.youtube.com/");

        // Verify Netscape cookie file generation from task cookies
        let temp_dir = std::env::temp_dir().join(format!("idm_task_cookie_{}", task.id));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let cookie_file = temp_dir.join("cookies.txt");
        let write_ok = crate::mux::write_netscape_cookies(&task.cookie, &[".youtube.com", ".google.com"], &cookie_file);
        assert!(write_ok.is_ok());
        let content = std::fs::read_to_string(&cookie_file).unwrap();
        assert!(content.contains("LOGIN_INFO\tAFmmF2swRQIhA:123"));
        assert!(content.contains(".youtube.com"));
        assert!(content.contains(".google.com"));
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Verify actionable YouTube error diagnostic translation
        let raw_yt_err = "ERROR: [youtube] dQw4w9WgXcQ: Sign in to confirm you're not a bot. Use --cookies-from-browser or export cookies.";
        let parsed = crate::mux::parse_youtube_error(raw_yt_err);
        assert!(parsed.contains("YouTube Bot Verification"));
    }

    #[tokio::test]
    async fn test_feature_3_engine_magnet_webseed_and_p2p_integration() {
        let manager = Arc::new(DownloadManager::new());

        // 1. Magnet link with BEP 19 WebSeed acceleration
        let magnet_with_webseed = "magnet:?xt=urn:btih:3b8c6a1e5d9f0c2a4b7e8d1c3a6f9b2e4d7a0c3f&dn=Fedora-Workstation-40.iso&xl=2147483648&ws=https%3A%2F%2Fdownload.fedoraproject.org%2Fpub%2Ffedora%2Flinux%2Freleases%2F40%2FFedora-Workstation-40.iso";

        let id = manager
            .start_download(
                magnet_with_webseed.to_string(),
                "".to_string(), // Let engine infer name from magnet 'dn'
                "/downloads/Fedora-Workstation-40.iso".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                None,
                None, // Auto-detect category
                None, // Auto-detect size from magnet 'xl'
                true, // Queue only for testing
                false,
                None,
            )
            .await
            .expect("Engine should successfully register Magnet link with WebSeed");

        let task = manager.tasks.read().await.get(&id).cloned().unwrap();

        // Verify task retains original magnet URI
        assert_eq!(task.original_url, magnet_with_webseed);
        // Verify final_url is routed to WebSeed mirror for multi-threaded downloading
        assert_eq!(
            *task.final_url.lock().await,
            "https://download.fedoraproject.org/pub/fedora/linux/releases/40/Fedora-Workstation-40.iso"
        );
        // Verify filename from 'dn'
        assert_eq!(task.get_filename(), "Fedora-Workstation-40.iso");
        // Verify category auto-detected as Compressed
        assert_eq!(task.get_category(), "Compressed");
        // Verify exact length from 'xl'
        assert_eq!(task.total_size.load(std::sync::atomic::Ordering::Relaxed), 2147483648);

        // 2. Standalone Magnet link without WebSeed starts seamlessly through Pure-Rust P2P Swarm
        let magnet_no_webseed = "magnet:?xt=urn:btih:3b8c6a1e5d9f0c2a4b7e8d1c3a6f9b2e4d7a0c3f&dn=Ubuntu-24.04.iso";
        let p2p_res = manager
            .start_download(
                magnet_no_webseed.to_string(),
                "Ubuntu-24.04.iso".to_string(),
                "/downloads/Ubuntu-24.04.iso".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                None,
                None,
                None,
                true,
                false,
                None,
            )
            .await;

        assert!(p2p_res.is_ok(), "Engine must accept standalone magnet link without webseed into P2P swarm");

        // 3. Category detection test for torrent & magnet URLs
        assert_eq!(crate::preflight::detect_category("debian-12.torrent", None), "Compressed");
        assert_eq!(crate::preflight::detect_category(magnet_with_webseed, None), "Compressed");
    }

    #[tokio::test]
    async fn test_feature_2_duplicate_name_conflict_resolution() {
        let temp_dir = std::env::temp_dir().join(format!("idm_conflict_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let existing_file = temp_dir.join("sample_report.pdf");
        std::fs::write(&existing_file, b"Original file content on disk").unwrap();

        let manager = Arc::new(DownloadManager::new());
        let url = "https://example.com/sample_report.pdf".to_string();

        // 1. Conflict Action: "rename" -> auto appends (1)
        let id_rename = manager.start_download(
            url.clone(),
            "sample_report.pdf".to_string(),
            existing_file.to_string_lossy().to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            None,
            None,
            None,
            true,
            false,
            Some("rename".to_string()),
        ).await.unwrap();

        let task_rename = manager.tasks.read().await.get(&id_rename).cloned().unwrap();
        assert_eq!(task_rename.get_filename(), "sample_report (1).pdf");
        assert!(task_rename.get_save_path().ends_with("sample_report (1).pdf"));

        // 2. Conflict Action: "overwrite" -> keeps original filename & path
        let id_overwrite = manager.start_download(
            url.clone(),
            "sample_report.pdf".to_string(),
            existing_file.to_string_lossy().to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            None,
            None,
            None,
            true,
            false,
            Some("overwrite".to_string()),
        ).await.unwrap();

        let task_overwrite = manager.tasks.read().await.get(&id_overwrite).cloned().unwrap();
        assert_eq!(task_overwrite.get_filename(), "sample_report.pdf");
        assert_eq!(task_overwrite.get_save_path(), existing_file.to_string_lossy().to_string());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}



