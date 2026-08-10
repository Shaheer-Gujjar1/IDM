use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::io::{Write, Seek};
use serde::{Serialize, Deserialize};
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::fs::OpenOptions;
use futures_util::StreamExt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed(String),
    Trash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadChunk {
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub id: String,
    pub filename: String,
    pub save_path: String,
    pub total_size: u64,
    pub downloaded: u64,
    pub speed: f64, // bytes/sec
    pub eta: String,
    pub status: DownloadStatus,
    pub file_exists: bool,
    pub speed_limited: bool,
}

pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub save_path: String,
    pub cookie: String,
    pub referrer: String,
    pub user_agent: String,
    pub total_size: AtomicU64,
    pub downloaded: Arc<AtomicU64>,
    pub status: Arc<std::sync::Mutex<DownloadStatus>>,
    pub abort_tx: Option<broadcast::Sender<()>>,
    pub chunks: std::sync::Mutex<Vec<DownloadChunk>>,
    pub speed: Arc<std::sync::Mutex<f64>>,
    pub eta: Arc<std::sync::Mutex<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PersistentTask {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub save_path: String,
    pub cookie: String,
    pub referrer: String,
    #[serde(default)]
    pub user_agent: String,
    pub total_size: u64,
    pub downloaded: u64,
    pub status: DownloadStatus,
    pub chunks: Vec<DownloadChunk>,
}

pub struct DownloadManager {
    pub tasks: RwLock<HashMap<String, Arc<DownloadTask>>>,
    pub app_handle: Mutex<Option<tauri::AppHandle>>,
    pub client: reqwest::Client,
    pub theme_mode: Mutex<String>,
    pub speed_limit_bps: AtomicU64,
    pub max_chunks: AtomicU64,
    pub intercept_downloads: std::sync::atomic::AtomicBool,
}

impl DownloadManager {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .redirect(reqwest::redirect::Policy::limited(10))
            .pool_max_idle_per_host(64)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .tcp_nodelay(true)
            .connect_timeout(std::time::Duration::from_secs(30))
            .read_timeout(std::time::Duration::from_secs(120))
            .connection_verbose(false)
            .build()
            .unwrap_or_default();

        Self {
            tasks: RwLock::new(HashMap::new()),
            app_handle: Mutex::new(None),
            client,
            theme_mode: Mutex::new("dark".to_string()),
            speed_limit_bps: AtomicU64::new(0),
            max_chunks: AtomicU64::new(8),
            intercept_downloads: std::sync::atomic::AtomicBool::new(true),
        }
    }

    pub async fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.lock().await = Some(handle);
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
                let chunks = task.chunks.lock().unwrap().clone();
                persistent_tasks.push(PersistentTask {
                    id: task.id.clone(),
                    url: task.url.clone(),
                    filename: task.filename.clone(),
                    save_path: task.save_path.clone(),
                    cookie: task.cookie.clone(),
                    referrer: task.referrer.clone(),
                    user_agent: task.user_agent.clone(),
                    total_size: task.total_size.load(Ordering::Relaxed),
                    downloaded,
                    status,
                    chunks,
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
                            
                            tasks.insert(p_task.id.clone(), Arc::new(DownloadTask {
                                id: p_task.id,
                                url: p_task.url,
                                filename: p_task.filename,
                                save_path: p_task.save_path,
                                cookie: p_task.cookie,
                                referrer: p_task.referrer,
                                user_agent: p_task.user_agent,
                                total_size: AtomicU64::new(p_task.total_size),
                                downloaded: Arc::new(AtomicU64::new(p_task.downloaded)),
                                status: Arc::new(std::sync::Mutex::new(status)),
                                abort_tx: None,
                                chunks: std::sync::Mutex::new(p_task.chunks),
                                speed: Arc::new(std::sync::Mutex::new(0.0)),
                                eta: Arc::new(std::sync::Mutex::new("---".to_string())),
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
        filename: String,
        save_path: String,
        cookie: String,
        referrer: String,
        user_agent: String,
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let (abort_tx, _) = broadcast::channel(1);

        let task = Arc::new(DownloadTask {
            id: id.clone(),
            url: url.clone(),
            filename: filename.clone(),
            save_path: save_path.clone(),
            cookie: cookie.clone(),
            referrer: referrer.clone(),
            user_agent: user_agent.clone(),
            total_size: AtomicU64::new(0),
            downloaded: Arc::new(AtomicU64::new(0)),
            status: Arc::new(std::sync::Mutex::new(DownloadStatus::Queued)),
            abort_tx: Some(abort_tx),
            chunks: std::sync::Mutex::new(vec![]),
            speed: Arc::new(std::sync::Mutex::new(0.0)),
            eta: Arc::new(std::sync::Mutex::new("---".to_string())),
        });

        self.tasks.write().await.insert(id.clone(), task.clone());

        let manager_clone_save = self.clone();
        tokio::spawn(async move {
            let _ = manager_clone_save.save_history().await;
        });

        let manager_clone = self.clone();
        tokio::spawn(async move {
            if let Err(e) = manager_clone.run_task(task, true).await {
                eprintln!("Download failed: {}", e);
            }
        });

        Ok(id)
    }

    pub async fn run_task(self: &Arc<Self>, task: Arc<DownloadTask>, accept_ranges: bool) -> Result<(), String> {
        *task.status.lock().unwrap() = DownloadStatus::Downloading;

        if let Some(parent) = std::path::Path::new(&task.save_path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let is_new = task.downloaded.load(Ordering::Relaxed) == 0;
        let file_exists = tokio::fs::metadata(&task.save_path).await.is_ok();
        let should_initialize = is_new || !file_exists || task.chunks.lock().unwrap().is_empty();

        if should_initialize {
            task.downloaded.store(0, Ordering::Relaxed);

            let is_sourceforge = task.url.contains("sourceforge.net");
            let mut server_supports_ranges = false;
            let mut probed_total_size = task.total_size.load(Ordering::Relaxed);

            // SourceForge mirrors choke when probed with Range or requested via multi-chunks.
            // Force single-connection streaming for SourceForge.
            if accept_ranges && !is_sourceforge {
                let mut req = self.client.head(&task.url)
                    .header(reqwest::header::ACCEPT, "*/*")
                    .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9");

                if !task.cookie.is_empty() {
                    req = req.header(reqwest::header::COOKIE, &task.cookie);
                }
                if !task.referrer.is_empty() {
                    req = req.header(reqwest::header::REFERER, &task.referrer);
                }
                if !task.user_agent.is_empty() {
                    req = req.header(reqwest::header::USER_AGENT, &task.user_agent);
                }

                if let Ok(res) = req.send().await {
                    let status = res.status();
                    if status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT {
                        server_supports_ranges = true;
                        if let Some(cl) = res.headers().get(reqwest::header::CONTENT_LENGTH).and_then(|h| h.to_str().ok()).and_then(|s| s.parse::<u64>().ok()) {
                            if cl > 0 {
                                probed_total_size = cl;
                            }
                        }
                    }
                }
                // If HEAD fails or gives 0 size, fall back to GET Range 0-0 probe
                if probed_total_size == 0 {
                    let mut req_get = self.client.get(&task.url)
                        .header(reqwest::header::ACCEPT, "*/*")
                        .header(reqwest::header::RANGE, "bytes=0-0");
                    if !task.cookie.is_empty() { req_get = req_get.header(reqwest::header::COOKIE, &task.cookie); }
                    if !task.referrer.is_empty() { req_get = req_get.header(reqwest::header::REFERER, &task.referrer); }
                    if !task.user_agent.is_empty() { req_get = req_get.header(reqwest::header::USER_AGENT, &task.user_agent); }

                    if let Ok(res_get) = req_get.send().await {
                        if res_get.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                            server_supports_ranges = true;
                        }
                        if let Some(cr_val) = res_get.headers().get(reqwest::header::CONTENT_RANGE).and_then(|h| h.to_str().ok()) {
                            server_supports_ranges = true;
                            if let Some(slash_idx) = cr_val.rfind('/') {
                                if let Ok(s) = cr_val[slash_idx + 1..].trim().parse::<u64>() {
                                    if s > 0 { probed_total_size = s; }
                                }
                            }
                        }
                    }
                }
            }

            if probed_total_size > 0 {
                task.total_size.store(probed_total_size, Ordering::Relaxed);
            }

            let configured_max = (self.max_chunks.load(Ordering::Relaxed).clamp(1, 32)) as usize;
            let num_chunks = if accept_ranges && !is_sourceforge && (server_supports_ranges || probed_total_size > 0) {
                configured_max
            } else {
                1
            };

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&task.save_path)
                .await
                .map_err(|e| format!("Failed to create file: {}", e))?;

            if probed_total_size > 0 {
                let _ = file.set_len(probed_total_size).await;
            }

            // Initialize connection chunks
            let mut chunks = vec![];
            if num_chunks > 1 && probed_total_size > 0 {
                let num_chunks_u64 = num_chunks as u64;
                let chunk_size = probed_total_size / num_chunks_u64;
                for i in 0..num_chunks {
                    let start = (i as u64) * chunk_size;
                    let end = if i == num_chunks - 1 {
                        probed_total_size - 1
                    } else {
                        ((i as u64) + 1) * chunk_size - 1
                    };
                    chunks.push(DownloadChunk { start, end, downloaded: 0 });
                }
            } else {
                chunks.push(DownloadChunk { start: 0, end: 0, downloaded: 0 });
            }
            *task.chunks.lock().unwrap() = chunks;
        }

        let abort_tx = task.abort_tx.as_ref().unwrap();
        let mut workers = vec![];
        let chunks_list = task.chunks.lock().unwrap().clone();

        let id_clone = task.id.clone();
        let filename_clone = task.filename.clone();
        let save_path_clone = task.save_path.clone();
        let downloaded_counter = task.downloaded.clone();
        let status_ref = task.status.clone();
        let mut abort_rx = abort_tx.subscribe();
        let app_handle_opt = self.app_handle.lock().await.clone();
        let task_speed = task.speed.clone();
        let task_eta = task.eta.clone();
        let manager_for_save = self.clone();
        let manager_for_reporting = self.clone();
        let task_for_reporting = task.clone();

        // Speed & Progress reporting loop — uses a 3-sample sliding window for stable, accurate display
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(800));
            // Rolling 3-sample window: store (bytes_snapshot, instant) for last 3 ticks
            let mut samples: std::collections::VecDeque<(u64, Instant)> = std::collections::VecDeque::with_capacity(4);
            samples.push_back((downloaded_counter.load(Ordering::Relaxed), Instant::now()));
            let mut save_ticks = 0;

            loop {
                tokio::select! {
                    _ = abort_rx.recv() => break,
                    _ = interval.tick() => {
                        let current_bytes = downloaded_counter.load(Ordering::Relaxed);
                        let now = Instant::now();

                        // 3-sample sliding window: average speed over the last 3 ticks (~2.4s window)
                        samples.push_back((current_bytes, now));
                        if samples.len() > 3 {
                            samples.pop_front();
                        }
                        let raw_speed = if samples.len() >= 2 {
                            let (old_bytes, old_time) = samples.front().unwrap();
                            let duration = now.duration_since(*old_time).as_secs_f64();
                            if duration > 0.0 {
                                current_bytes.saturating_sub(*old_bytes) as f64 / duration
                            } else { 0.0 }
                        } else { 0.0 };

                        let mut speed_guard = task_speed.lock().unwrap();
                        let speed = raw_speed;
                        *speed_guard = speed;

                        let current_total_size = task_for_reporting.total_size.load(Ordering::Relaxed);
                        let eta = if speed > 1024.0 && current_total_size > current_bytes {
                            let rem = current_total_size - current_bytes;
                            let rem_secs = (rem as f64 / speed).round() as u64;
                            if rem_secs >= 3600 {
                                let h = rem_secs / 3600;
                                let m = (rem_secs % 3600) / 60;
                                let s = rem_secs % 60;
                                if m > 0 {
                                    format!("{}h {}m {}s", h, m, s)
                                } else {
                                    format!("{}h {}s", h, s)
                                }
                            } else if rem_secs >= 60 {
                                let m = rem_secs / 60;
                                let s = rem_secs % 60;
                                if s > 0 {
                                    format!("{} mins {} secs", m, s)
                                } else {
                                    format!("{} mins", m)
                                }
                            } else {
                                format!("{} secs", rem_secs)
                            }
                        } else {
                            "---".to_string()
                        };

                        let status = status_ref.lock().unwrap().clone();
                        *task_eta.lock().unwrap() = eta.clone();

                        let current_limit = manager_for_reporting.speed_limit_bps.load(Ordering::Relaxed);
                        let is_speed_limited = current_limit > 0 && speed > 0.0;

                        let progress = DownloadProgress {
                            id: id_clone.clone(),
                            filename: filename_clone.clone(),
                            save_path: save_path_clone.clone(),
                            total_size: current_total_size,
                            downloaded: current_bytes,
                            speed,
                            eta,
                            status: status.clone(),
                            file_exists: std::path::Path::new(&save_path_clone).exists(),
                            speed_limited: is_speed_limited,
                        };

                        if let Some(ref handle) = app_handle_opt {
                            use tauri::Emitter;
                            let _ = handle.emit("download-progress", progress);
                        }

                        // Periodic autosave every ~30 seconds — moved outside hot path
                        save_ticks += 1;
                        if save_ticks >= 37 {
                            save_ticks = 0;
                            let manager_clone = manager_for_save.clone();
                            tokio::spawn(async move {
                                let _ = manager_clone.save_history().await;
                            });
                        }

                        if status == DownloadStatus::Completed || matches!(status, DownloadStatus::Failed(_)) {
                            break;
                        }
                    }
                }
            }
        });

        let num_chunks = chunks_list.len();
        // Spawn segment download workers — each writes sequentially to its own .part_X file (IDM/XDM style)
        for (idx, chunk) in chunks_list.into_iter().enumerate() {
            let url = task.url.clone();
            let task_clone = task.clone();
            let mut task_abort_rx = abort_tx.subscribe();
            let client = self.client.clone();
            let manager_for_worker = self.clone();

            let worker = tokio::spawn(async move {
                let end_offset = chunk.end;
                let part_path = format!("{}.part_{}", task_clone.save_path, idx);

                let mut worker_file = match std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&part_path)
                {
                    Ok(f) => f,
                    Err(_) => return,
                };

                let mut local_downloaded = chunk.downloaded;
                let mut last_saved_downloaded = chunk.downloaded;

                if local_downloaded > 0 {
                    let _ = worker_file.seek(std::io::SeekFrom::Start(local_downloaded));
                }

                let mut limiter_window_start = Instant::now();
                let mut limiter_window_bytes = 0u64;

                let max_retries: u32 = 20;
                let mut retry_count: u32 = 0;

                'reconnect: loop {
                    let current_start = chunk.start + local_downloaded;
                    if end_offset > 0 && current_start >= end_offset + 1 {
                        break 'reconnect;
                    }
                    if retry_count >= max_retries {
                        break 'reconnect;
                    }

                    if retry_count > 0 {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                    }

                    let mut req = client.get(&url)
                        .header(reqwest::header::ACCEPT, "*/*")
                        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
                        .header(reqwest::header::CONNECTION, "keep-alive");

                    if !task_clone.cookie.is_empty() {
                        req = req.header(reqwest::header::COOKIE, &task_clone.cookie);
                    }
                    if !task_clone.referrer.is_empty() {
                        req = req.header(reqwest::header::REFERER, &task_clone.referrer);
                    }
                    if !task_clone.user_agent.is_empty() {
                        req = req.header(reqwest::header::USER_AGENT, &task_clone.user_agent);
                    }
                    let is_sourceforge = task_clone.url.contains("sourceforge.net");
                    if num_chunks > 1 && end_offset > 0 {
                        req = req.header(reqwest::header::RANGE, format!("bytes={}-{}", current_start, end_offset));
                    } else if local_downloaded > 0 && !is_sourceforge {
                        req = req.header(reqwest::header::RANGE, format!("bytes={}-", current_start));
                    }

                    let res = match req.send().await {
                        Ok(r) => r,
                        Err(_) => {
                            retry_count += 1;
                            continue 'reconnect;
                        }
                    };

                    if retry_count == 0 {
                        let content_type = res.headers()
                            .get(reqwest::header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .to_lowercase();

                        if content_type.contains("text/html") {
                            *task_clone.status.lock().unwrap() = DownloadStatus::Failed(
                                "landing page captured instead of file".to_string()
                            );
                            break 'reconnect;
                        }

                        if !res.status().is_success() && res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                            let code = res.status().as_u16();
                            *task_clone.status.lock().unwrap() = DownloadStatus::Failed(
                                format!("HTTP server returned error status {}", code)
                            );
                            break 'reconnect;
                        }

                        if let Some(cr_val) = res.headers().get("Content-Range").and_then(|h| h.to_str().ok()) {
                            if let Some(slash_idx) = cr_val.rfind('/') {
                                if let Ok(s) = cr_val[slash_idx + 1..].trim().parse::<u64>() {
                                    if s > 0 { task_clone.total_size.store(s, Ordering::Relaxed); }
                                }
                            }
                        }
                        if task_clone.total_size.load(Ordering::Relaxed) == 0 {
                            if let Some(cl) = res.headers()
                                .get(reqwest::header::CONTENT_LENGTH)
                                .and_then(|v| v.to_str().ok())
                                .and_then(|s| s.parse::<u64>().ok())
                            {
                                if cl > 0 { task_clone.total_size.store(cl, Ordering::Relaxed); }
                            }
                        }
                    }

                    let mut stream = res.bytes_stream();
                    let mut net_buffer = Vec::with_capacity(64 * 1024);

                    loop {
                        tokio::select! {
                            _ = task_abort_rx.recv() => {
                                if !net_buffer.is_empty() {
                                    local_downloaded += net_buffer.len() as u64;
                                    let _ = worker_file.write_all(&net_buffer);
                                    net_buffer.clear();
                                }
                                break 'reconnect;
                            }

                            chunk_res = tokio::time::timeout(Duration::from_secs(15), stream.next()) => {
                                match chunk_res {
                                    Err(_timeout) => {
                                        if !net_buffer.is_empty() {
                                            local_downloaded += net_buffer.len() as u64;
                                            let _ = worker_file.write_all(&net_buffer);
                                            net_buffer.clear();
                                        }
                                        retry_count += 1;
                                        continue 'reconnect;
                                    }

                                    Ok(None) => {
                                        if !net_buffer.is_empty() {
                                            local_downloaded += net_buffer.len() as u64;
                                            let _ = worker_file.write_all(&net_buffer);
                                            net_buffer.clear();
                                        }
                                        break 'reconnect;
                                    }

                                    Ok(Some(Err(_))) => {
                                        if !net_buffer.is_empty() {
                                            local_downloaded += net_buffer.len() as u64;
                                            let _ = worker_file.write_all(&net_buffer);
                                            net_buffer.clear();
                                        }
                                        retry_count += 1;
                                        continue 'reconnect;
                                    }

                                    Ok(Some(Ok(bytes))) => {
                                        retry_count = 0;
                                        let mut bytes_to_add = bytes.as_ref();

                                        if num_chunks > 1 && end_offset > 0 {
                                            let current_pos = chunk.start + local_downloaded + net_buffer.len() as u64;
                                            if current_pos >= end_offset + 1 {
                                                if !net_buffer.is_empty() {
                                                    local_downloaded += net_buffer.len() as u64;
                                                    let _ = worker_file.write_all(&net_buffer);
                                                    net_buffer.clear();
                                                }
                                                break 'reconnect;
                                            }
                                            let remaining = (end_offset + 1) - current_pos;
                                            if bytes_to_add.len() as u64 > remaining {
                                                bytes_to_add = &bytes_to_add[..remaining as usize];
                                            }
                                        }

                                        net_buffer.extend_from_slice(bytes_to_add);
                                        task_clone.downloaded.fetch_add(bytes_to_add.len() as u64, Ordering::Relaxed);

                                        let limit_bps = manager_for_worker.speed_limit_bps.load(Ordering::Relaxed);
                                        if limit_bps > 0 {
                                            let per_worker_limit = (limit_bps / (num_chunks as u64)).max(1024);
                                            limiter_window_bytes += bytes_to_add.len() as u64;
                                            let elapsed_sec = limiter_window_start.elapsed().as_secs_f64();
                                            let allowed_bytes = (per_worker_limit as f64 * elapsed_sec) as u64;
                                            if limiter_window_bytes > allowed_bytes {
                                                let excess = limiter_window_bytes - allowed_bytes;
                                                let sleep_sec = excess as f64 / per_worker_limit as f64;
                                                if sleep_sec >= 0.002 {
                                                    tokio::time::sleep(Duration::from_secs_f64(sleep_sec.min(0.5))).await;
                                                }
                                            }
                                            if elapsed_sec >= 0.5 {
                                                limiter_window_start = Instant::now();
                                                limiter_window_bytes = 0;
                                            }
                                        }

                                        if net_buffer.len() >= 64 * 1024 {
                                            let _ = worker_file.write_all(&net_buffer);
                                            local_downloaded += net_buffer.len() as u64;
                                            net_buffer.clear();

                                            if local_downloaded - last_saved_downloaded >= 4 * 1024 * 1024 {
                                                if let Ok(mut chunks_lock) = task_clone.chunks.try_lock() {
                                                    if idx < chunks_lock.len() {
                                                        chunks_lock[idx].downloaded = local_downloaded;
                                                        last_saved_downloaded = local_downloaded;
                                                    }
                                                }
                                            }
                                        }

                                        if num_chunks > 1 && end_offset > 0 {
                                            let current_pos = chunk.start + local_downloaded + net_buffer.len() as u64;
                                            if current_pos >= end_offset + 1 {
                                                if !net_buffer.is_empty() {
                                                    local_downloaded += net_buffer.len() as u64;
                                                    let _ = worker_file.write_all(&net_buffer);
                                                    net_buffer.clear();
                                                }
                                                break 'reconnect;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let _ = worker_file.flush();
                let mut chunks_lock = task_clone.chunks.lock().unwrap();
                if idx < chunks_lock.len() {
                    chunks_lock[idx].downloaded = local_downloaded;
                }
            });

            workers.push(worker);
        }

        for worker in workers {
            let _ = worker.await;
        }

        // --- Post-Download Assembly (IDM/XDM style) ---
        let is_deleted = !self.tasks.read().await.contains_key(&task.id);
        if is_deleted {
            for idx in 0..num_chunks {
                let _ = std::fs::remove_file(format!("{}.part_{}", task.save_path, idx));
            }
            return Ok(());
        }

        let is_aborted = {
            let s = task.status.lock().unwrap();
            *s == DownloadStatus::Paused
        };

        if !is_aborted {
            let downloaded_bytes = task.downloaded.load(Ordering::Relaxed);
            let total_size_val = task.total_size.load(Ordering::Relaxed);
            let is_success = (total_size_val > 0 && downloaded_bytes >= total_size_val)
                || (total_size_val > 0 && downloaded_bytes > 0 && ((downloaded_bytes as f64) / (total_size_val as f64)) >= 0.995)
                || (total_size_val == 0 && downloaded_bytes > 0);

            if is_success {
                // Merge .part_X files into final save_path
                let merge_res = tokio::task::spawn_blocking({
                    let save_path = task.save_path.clone();
                    move || -> Result<(), String> {
                        let mut final_file = std::fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(&save_path)
                            .map_err(|e| e.to_string())?;

                        for idx in 0..num_chunks {
                            let part_path = format!("{}.part_{}", save_path, idx);
                            if let Ok(mut part_file) = std::fs::File::open(&part_path) {
                                let _ = std::io::copy(&mut part_file, &mut final_file);
                            }
                            let _ = std::fs::remove_file(&part_path);
                        }
                        let _ = final_file.flush();
                        Ok(())
                    }
                }).await;

                if merge_res.is_ok() && merge_res.unwrap().is_ok() {
                    *task.status.lock().unwrap() = DownloadStatus::Completed;
                } else {
                    *task.status.lock().unwrap() = DownloadStatus::Failed("Failed to merge segment files".to_string());
                }
            } else if total_size_val == 0 && downloaded_bytes == 0 {
                *task.status.lock().unwrap() = DownloadStatus::Failed("No bytes received".to_string());
            } else {
                *task.status.lock().unwrap() = DownloadStatus::Failed(format!(
                    "Download incomplete ({}/{} bytes)",
                    downloaded_bytes, total_size_val
                ));
            }
        }

            let final_status = task.status.lock().unwrap().clone();
            let progress = DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "0s".to_string(),
                status: final_status,
                file_exists: std::path::Path::new(&task.save_path).exists(),
                speed_limited: false,
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
                if *status == DownloadStatus::Completed || matches!(*status, DownloadStatus::Failed(_)) {
                    return Ok(());
                }
                *status = DownloadStatus::Paused;
            }
            if let Some(ref tx) = task.abort_tx {
                let _ = tx.send(());
            }

            // Emit a progress update event immediately showing it was paused
            let progress = DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: DownloadStatus::Paused,
                file_exists: std::path::Path::new(&task.save_path).exists(),
                speed_limited: false,
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
            let chunks_clone = {
                let chunks_lock = task.chunks.lock().unwrap();
                chunks_lock.clone()
            };

            {
                let mut status_lock = task.status.lock().unwrap();
                if *status_lock == DownloadStatus::Completed || *status_lock == DownloadStatus::Downloading {
                    return Ok(());
                }
                *status_lock = DownloadStatus::Queued;
            }

            let accept_ranges = chunks_clone.len() > 1;

            let (abort_tx, _) = broadcast::channel(1);
            let updated_task = Arc::new(DownloadTask {
                id: task.id.clone(),
                url: task.url.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                cookie: task.cookie.clone(),
                referrer: task.referrer.clone(),
                user_agent: task.user_agent.clone(),
                total_size: AtomicU64::new(task.total_size.load(Ordering::Relaxed)),
                downloaded: task.downloaded.clone(),
                status: task.status.clone(),
                abort_tx: Some(abort_tx),
                chunks: std::sync::Mutex::new(chunks_clone),
                speed: task.speed.clone(),
                eta: task.eta.clone(),
            });

            tasks.insert(id.to_string(), updated_task.clone());

            let manager_clone = self.clone();

            tokio::spawn(async move {
                if let Err(e) = manager_clone.run_task(updated_task, accept_ranges).await {
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
            if let Some(ref tx) = task.abort_tx {
                let _ = tx.send(());
            }
            
            *task.status.lock().unwrap() = DownloadStatus::Paused;
            *task.speed.lock().unwrap() = 0.0;
            *task.eta.lock().unwrap() = "Paused".to_string();
            
            let progress = DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "Paused".to_string(),
                status: DownloadStatus::Paused,
                file_exists: std::path::Path::new(&task.save_path).exists(),
                speed_limited: false,
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

    pub async fn delete_task(&self, id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.remove(id) {
            if let Some(ref tx) = task.abort_tx {
                let _ = tx.send(());
            }
            drop(tasks);
            let _ = self.save_history().await;
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn trash_task(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let app_handle_opt = self.app_handle.lock().await.clone();
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            if let Some(ref tx) = task.abort_tx {
                let _ = tx.send(());
            }
            *task.status.lock().unwrap() = DownloadStatus::Trash;
            
            if delete_file {
                let path = task.save_path.clone();
                tokio::spawn(async move {
                    let _ = tokio::fs::remove_file(path).await;
                });
            }
            
            let progress = DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: DownloadStatus::Trash,
                file_exists: std::path::Path::new(&task.save_path).exists(),
                speed_limited: false,
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
            
            let progress = DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: final_status,
                file_exists: std::path::Path::new(&task.save_path).exists(),
                speed_limited: false,
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
            if let Some(ref tx) = task.abort_tx {
                let _ = tx.send(());
            }

            let path = task.save_path.clone();
            let _ = tokio::fs::remove_file(path).await;

            let accept_ranges = task.chunks.lock().unwrap().len() > 1;

            let (abort_tx, _) = broadcast::channel(1);
            let updated_task = Arc::new(DownloadTask {
                id: task.id.clone(),
                url: task.url.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                cookie: task.cookie.clone(),
                referrer: task.referrer.clone(),
                user_agent: task.user_agent.clone(),
                total_size: AtomicU64::new(task.total_size.load(Ordering::Relaxed)),
                downloaded: Arc::new(AtomicU64::new(0)),
                status: Arc::new(std::sync::Mutex::new(DownloadStatus::Queued)),
                abort_tx: Some(abort_tx),
                chunks: std::sync::Mutex::new(vec![]),
                speed: Arc::new(std::sync::Mutex::new(0.0)),
                eta: Arc::new(std::sync::Mutex::new("---".to_string())),
            });

            tasks.insert(id.to_string(), updated_task.clone());

            let manager_clone = self.clone();

            tokio::spawn(async move {
                if let Err(e) = manager_clone.run_task(updated_task, accept_ranges).await {
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
            let current_bytes = task.downloaded.load(Ordering::Relaxed);
            let speed = *task.speed.lock().unwrap();
            let eta = task.eta.lock().unwrap().clone();
            let status = task.status.lock().unwrap().clone();
            
            let current_limit = self.speed_limit_bps.load(Ordering::Relaxed);
            let is_speed_limited = current_limit > 0 && speed > 0.0;

            Some(DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: current_bytes,
                speed,
                eta,
                status,
                file_exists: std::path::Path::new(&task.save_path).exists(),
                speed_limited: is_speed_limited,
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
            let current_bytes = task.downloaded.load(Ordering::Relaxed);
            let status = task.status.lock().unwrap().clone();
            let speed = *task.speed.lock().unwrap();
            let eta = task.eta.lock().unwrap().clone();
            let is_speed_limited = current_limit > 0 && speed > 0.0;
            list.push(DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: current_bytes,
                speed,
                eta,
                status,
                file_exists: std::path::Path::new(&task.save_path).exists(),
                speed_limited: is_speed_limited,
            });
        }
        list
    }
}
