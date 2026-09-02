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
}

pub struct DownloadTask {
    pub id: String,
    pub original_url: String,
    pub final_url: Mutex<String>,
    pub filename: Arc<std::sync::Mutex<String>>,
    pub save_path: Arc<std::sync::Mutex<String>>,
    pub cookie: String,
    pub referrer: String,
    pub user_agent: String,
    pub total_size: AtomicU64,
    pub downloaded: Arc<AtomicU64>,
    pub network_downloaded: Arc<AtomicU64>,
    pub speed_limiter: Mutex<Instant>,
    pub status: Arc<std::sync::Mutex<DownloadStatus>>,
    pub abort_tx: Option<broadcast::Sender<()>>,
    pub chunks: std::sync::Mutex<Vec<ActiveChunk>>,
    pub speed: Arc<std::sync::Mutex<f64>>,
    pub eta: Arc<std::sync::Mutex<String>>,
}

impl DownloadTask {
    pub fn get_filename(&self) -> String {
        self.filename.lock().unwrap().clone()
    }

    pub fn get_save_path(&self) -> String {
        self.save_path.lock().unwrap().clone()
    }

    pub fn update_filename_and_path(&self, new_name: &str) {
        let mut fn_guard = self.filename.lock().unwrap();
        let mut sp_guard = self.save_path.lock().unwrap();
        *fn_guard = new_name.to_string();
        let path = std::path::Path::new(&*sp_guard);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                *sp_guard = parent.join(new_name).to_string_lossy().to_string();
            } else {
                *sp_guard = new_name.to_string();
            }
        } else {
            *sp_guard = new_name.to_string();
        }
    }

    pub async fn throttle_if_needed(&self, bytes_len: u64, limit_bps: u64) {
        if limit_bps == 0 || bytes_len == 0 {
            return;
        }

        // Target cap at 99.5% to guarantee measured UI speed never touches or exceeds limit
        let target_bps = (limit_bps as f64 * 0.995).max(1.0);

        let mut virtual_time = self.speed_limiter.lock().await;
        let now = Instant::now();
        
        if *virtual_time < now {
            *virtual_time = now;
        }
        
        let duration = Duration::from_secs_f64(bytes_len as f64 / target_bps);
        *virtual_time += duration;
        
        if let Some(sleep_duration) = virtual_time.checked_duration_since(now) {
            if !sleep_duration.is_zero() {
                tokio::time::sleep(sleep_duration).await;
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
    pub cookie: String,
    pub referrer: String,
    #[serde(default)]
    pub user_agent: String,
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
}

impl DownloadManager {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
            .pool_max_idle_per_host(32)
            .redirect(reqwest::redirect::Policy::limited(10))
            .tcp_keepalive(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(30))
            .http2_adaptive_window(true)
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
            minimize_to_tray: std::sync::atomic::AtomicBool::new(true),
            scheduler_config: Arc::new(tokio::sync::RwLock::new(SchedulerConfig::default())),
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
                    cookie: task.cookie.clone(),
                    referrer: task.referrer.clone(),
                    user_agent: task.user_agent.clone(),
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
                            
                            tasks.insert(p_task.id.clone(), Arc::new(DownloadTask {
                                id: p_task.id,
                                original_url: p_task.original_url,
                                final_url: Mutex::new(p_task.final_url),
                                filename: Arc::new(std::sync::Mutex::new(p_task.filename)),
                                save_path: Arc::new(std::sync::Mutex::new(p_task.save_path)),
                                cookie: p_task.cookie,
                                referrer: p_task.referrer,
                                user_agent: p_task.user_agent,
                                total_size: AtomicU64::new(p_task.total_size),
                                downloaded: Arc::new(AtomicU64::new(p_task.downloaded)),
                                network_downloaded: Arc::new(AtomicU64::new(p_task.downloaded)),
                                speed_limiter: Mutex::new(Instant::now()),
                                status: Arc::new(std::sync::Mutex::new(status)),
                                abort_tx: None,
                                chunks: std::sync::Mutex::new(active_chunks),
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
        mut filename: String,
        mut save_path: String,
        cookie: String,
        referrer: String,
        user_agent: String,
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("Unsupported URL protocol. Only http:// and https:// URLs can be downloaded over the network. In-memory URLs (e.g. blob:) are generated locally in your browser and saved directly by the browser.".to_string());
        }

        if crate::proxy::is_generic_filename(&filename) || filename.is_empty() {
            if let Ok(parsed) = reqwest::Url::parse(&url) {
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

        let (abort_tx, _) = broadcast::channel(1);
        let task = Arc::new(DownloadTask {
            id: id.clone(),
            original_url: url.clone(),
            final_url: Mutex::new(url.clone()),
            filename: Arc::new(std::sync::Mutex::new(filename)),
            save_path: Arc::new(std::sync::Mutex::new(save_path)),
            cookie,
            referrer,
            user_agent,
            total_size: AtomicU64::new(0),
            downloaded: Arc::new(AtomicU64::new(0)),
            network_downloaded: Arc::new(AtomicU64::new(0)),
            speed_limiter: Mutex::new(Instant::now()),
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
            if let Err(e) = manager_clone.run_task(task, false).await {
                eprintln!("Download error: {}", e);
            }
        });

        Ok(id)
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

        let temp_dir_exists = temp_dir.exists();
        let chunks_empty = task.chunks.lock().unwrap().is_empty();
        let should_initialize = !is_resume || chunks_empty || !temp_dir_exists;

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
        } else {
            if !temp_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&temp_dir) {
                    return Err(format!("Could not create temp directory: {}", e));
                }
            }
        }

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

        // Speed & Progress reporting loop
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(800));
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
                        };

                        if let Some(ref handle) = app_handle_opt {
                            use tauri::Emitter;
                            let _ = handle.emit("download-progress", progress);
                        }

                        save_ticks += 1;
                        if save_ticks >= 37 {
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
            let can_multi_segment = accept_ranges_flag.load(Ordering::SeqCst) && current_total_size > 2 * 1024 * 1024;
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

            // Find an inactive chunk or dynamically split the largest chunk
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

                // 2. Dynamic split of the largest remaining chunk (if range requests supported)
                if chunk_to_spawn.is_none() && can_multi_segment {
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

                    if max_rem > 1024 * 1024 { // Minimum split size 1MB
                        if let Some(idx) = best_idx {
                            let end = chunks[idx].end.load(Ordering::SeqCst);
                            let downloaded = chunks[idx].downloaded.load(Ordering::SeqCst);
                            let start = chunks[idx].start;

                            let rem = (end + 1) - (start + downloaded);
                            let split_len = rem / 2;
                            let new_start = end + 1 - split_len;

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

            if let Some(chunk) = chunk_to_spawn {
                let task_clone = task.clone();
                let client = self.client.clone();
                let temp_dir_clone = temp_dir.clone();
                let downloaded_counter_clone = task.downloaded.clone();
                let manager_for_worker = self.clone();
                let completed_tx = worker_completed_tx.clone();
                let chunk_id = chunk.id.clone();
                let mut worker_abort_rx = abort_tx.subscribe();
                let is_first_req_clone = is_first_request.clone();
                let accept_ranges_flag_clone = accept_ranges_flag.clone();

                let worker = tokio::spawn(async move {
                    let max_retries: u32 = 5;
                    let mut retry_count: u32 = 0;
                    let mut retry_with_no_range: bool = false;
                    let mut last_error_msg = String::new();
                    let mut net_buffer = Vec::with_capacity(64 * 1024);

                    let clean_header_val = |v: &str| -> Option<reqwest::header::HeaderValue> {
                        let sanitized: String = v.chars()
                            .filter(|c| !c.is_control() && (*c as u32) < 256)
                            .collect();
                        reqwest::header::HeaderValue::from_str(&sanitized).ok()
                    };

                    'reconnect: loop {
                        use tokio::io::{AsyncSeekExt, AsyncWriteExt};
                        net_buffer.clear();

                        let chunk_path = temp_dir_clone.join(&chunk_id);
                        let mut file = match tokio::fs::OpenOptions::new().write(true).create(true).open(&chunk_path).await {
                            Ok(f) => f,
                            Err(e) => {
                                last_error_msg = format!("Disk file error: {}", e);
                                retry_count += 1;
                                if retry_count >= max_retries {
                                    *task_clone.status.lock().unwrap() = DownloadStatus::Failed(last_error_msg);
                                    break 'reconnect;
                                }
                                tokio::time::sleep(Duration::from_millis(1000)).await;
                                continue 'reconnect;
                            }
                        };

                        let local_downloaded = chunk.downloaded.load(Ordering::SeqCst);
                        if let Err(e) = file.seek(std::io::SeekFrom::Start(local_downloaded)).await {
                            last_error_msg = format!("Disk seek error: {}", e);
                            retry_count += 1;
                            if retry_count >= max_retries {
                                *task_clone.status.lock().unwrap() = DownloadStatus::Failed(last_error_msg);
                                break 'reconnect;
                            }
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                            continue 'reconnect;
                        }

                        let current_end = chunk.end.load(Ordering::SeqCst);
                        let current_start = chunk.start + local_downloaded;

                        if current_end > 0 && current_end >= chunk.start && current_start > current_end {
                            break 'reconnect;
                        }

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
                        let mut req = client.get(&worker_url)
                            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7")
                            .header(reqwest::header::ACCEPT_ENCODING, "identity")
                            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
                            .header("sec-ch-ua", "\"Chromium\";v=\"124\", \"Google Chrome\";v=\"124\", \"Not-A.Brand\";v=\"99\"")
                            .header("sec-ch-ua-mobile", "?0")
                            .header("sec-ch-ua-platform", "\"Windows\"")
                            .header("sec-fetch-dest", "document")
                            .header("sec-fetch-mode", "navigate")
                            .header("sec-fetch-site", "cross-site")
                            .header("sec-fetch-user", "?1")
                            .header("upgrade-insecure-requests", "1")
                            .header(reqwest::header::CONNECTION, "keep-alive");

                        if let Some(ua) = clean_header_val(&task_clone.user_agent) {
                            req = req.header(reqwest::header::USER_AGENT, ua);
                        } else {
                            req = req.header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36");
                        }

                        if !task_clone.cookie.is_empty() {
                            if let Some(hv) = clean_header_val(&task_clone.cookie) {
                                req = req.header(reqwest::header::COOKIE, hv);
                            }
                        }
                        if !task_clone.referrer.is_empty() {
                            if let Some(hv) = clean_header_val(&task_clone.referrer) {
                                req = req.header(reqwest::header::REFERER, hv);
                            }
                        }

                        // Send Range header: unless fallback without Range requested
                        if !retry_with_no_range {
                            if current_end > 0 && current_end >= chunk.start {
                                req = req.header(reqwest::header::RANGE, format!("bytes={}-{}", current_start, current_end));
                            } else {
                                req = req.header(reqwest::header::RANGE, format!("bytes={}-", current_start));
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
                        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
                            last_error_msg = format!("HTTP {}", status);
                            eprintln!("[Worker Error] HTTP status {} for chunk {}", status, chunk_id);

                            // If Range failed on Chunk 0 with 416, 400, 403, or 405, immediately retry without Range header!
                            if is_first_req_clone.load(Ordering::SeqCst) && !retry_with_no_range && (status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE || status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::METHOD_NOT_ALLOWED) {
                                eprintln!("[Worker] Range header rejected with status {}. Retrying without Range...", status);
                                retry_with_no_range = true;
                                continue 'reconnect;
                            }

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

                            // If ranges supported and file > 2MB, dynamically split segments for parallel workers!
                            let max_c = manager_for_worker.max_chunks.load(Ordering::Relaxed).max(1) as usize;
                            if is_partial && probed_total_size > 2 * 1024 * 1024 && max_c > 1 {
                                let num_workers = ((probed_total_size / (1024 * 1024)) as usize).clamp(1, max_c);
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
                                        if !net_buffer.is_empty() {
                                            let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(64 * 1024));
                                            if let Ok(_) = file.write_all(&flushed).await {
                                                chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                            }
                                        }
                                        break 'reconnect;
                                    }
                                }
                                bytes_chunk_res = tokio::time::timeout(Duration::from_secs(60), stream.next()) => {
                                    let bytes_chunk = match bytes_chunk_res {
                                        Ok(b) => b,
                                        Err(_) => {
                                            if !net_buffer.is_empty() {
                                                let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(64 * 1024));
                                                if let Ok(_) = file.write_all(&flushed).await {
                                                    chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                    downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                                }
                                            }
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
                                                    if !net_buffer.is_empty() {
                                                        let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(64 * 1024));
                                                        if let Ok(_) = file.write_all(&flushed).await {
                                                            chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                            downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                                        }
                                                    }
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
                                                let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(64 * 1024));
                                                if let Err(_) = file.write_all(&flushed).await {
                                                    retry_count += 1;
                                                    continue 'reconnect;
                                                }
                                                chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                            }
                                        }
                                        Some(Err(_)) => {
                                            if !net_buffer.is_empty() {
                                                let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(64 * 1024));
                                                if let Ok(_) = file.write_all(&flushed).await {
                                                    chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                    downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                                }
                                            }
                                            retry_count += 1;
                                            continue 'reconnect;
                                        }
                                        None => {
                                            // Stream reached EOF cleanly
                                            if !net_buffer.is_empty() {
                                                let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(64 * 1024));
                                                if let Ok(_) = file.write_all(&flushed).await {
                                                    chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                    downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                                }
                                            }
                                            break 'reconnect;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let final_dl = chunk.downloaded.load(Ordering::SeqCst);
                    let chunk_path = temp_dir_clone.join(&chunk_id);
                    if let Ok(f) = tokio::fs::OpenOptions::new().write(true).open(&chunk_path).await {
                        f.set_len(final_dl).await.ok();
                    }
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
                *task.status.lock().unwrap() = DownloadStatus::Assembling;

                let cur_filename = task.get_filename();
                let cur_save_path = task.get_save_path();

                let progress = DownloadProgress {
                    id: task.id.clone(),
                    url: task.original_url.clone(),
                    filename: cur_filename,
                    save_path: cur_save_path.clone(),
                    total_size: total_size_val,
                    downloaded: downloaded_bytes,
                    speed: 0.0,
                    eta: "Assembling...".to_string(),
                    status: DownloadStatus::Assembling,
                    file_exists: std::path::Path::new(&cur_save_path).exists(),
                    speed_limited: false,
                };
                if let Some(ref handle) = self.app_handle.lock().await.clone() {
                    use tauri::Emitter;
                    let _ = handle.emit("download-progress", progress);
                }

                let temp_dir_clone = temp_dir.clone();
                let save_path_clone = cur_save_path.clone();
                let chunks_clone = task.chunks.lock().unwrap().clone();

                let assembly_result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let mut out_file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&save_path_clone)
                        .map_err(|e| format!("Failed to open final file: {}", e))?;

                    let mut sorted_chunks = chunks_clone;
                    sorted_chunks.sort_by_key(|c| c.start);

                    let mut buf = vec![0u8; 1024 * 1024 * 2]; // 2MB assembly buffer

                    for chunk in sorted_chunks {
                        let part_path = temp_dir_clone.join(&chunk.id);
                        if let Ok(mut part_file) = std::fs::File::open(&part_path) {
                            use std::io::Read;
                            use std::io::Write;

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
                    Ok(())
                }).await;

                match assembly_result {
                    Ok(Ok(_)) => {
                        *task.status.lock().unwrap() = DownloadStatus::Completed;
                        let _ = std::fs::remove_dir_all(&temp_dir);
                    },
                    Ok(Err(e)) => {
                        *task.status.lock().unwrap() = DownloadStatus::Failed(format!("Assembly failed: {}", e));
                    },
                    Err(_) => {
                        *task.status.lock().unwrap() = DownloadStatus::Failed("Assembly panicked".to_string());
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
                cookie: task.cookie.clone(),
                referrer: task.referrer.clone(),
                user_agent: task.user_agent.clone(),
                total_size: AtomicU64::new(task.total_size.load(Ordering::Relaxed)),
                downloaded: task.downloaded.clone(),
                network_downloaded: Arc::new(AtomicU64::new(task.downloaded.load(Ordering::Relaxed))),
                speed_limiter: Mutex::new(Instant::now()),
                status: task.status.clone(),
                abort_tx: Some(abort_tx),
                chunks: std::sync::Mutex::new(chunks_clone),
                speed: task.speed.clone(),
                eta: task.eta.clone(),
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
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }
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
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }
            *task.status.lock().unwrap() = DownloadStatus::Trash;
            
            let temp_dir = if let Some(ref handle) = app_handle_opt {
                let mut p = handle.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                p.push("temp"); p.push(&task.id); p
            } else { std::path::PathBuf::from(format!("./temp/{}", task.id)) };

            let path = task.get_save_path();
            tokio::spawn(async move {
                if delete_file { let _ = tokio::fs::remove_file(path).await; }
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
            let _ = tokio::fs::remove_file(path).await;

            let (abort_tx, _) = broadcast::channel(1);
            let updated_task = Arc::new(DownloadTask {
                id: task.id.clone(),
                original_url: task.original_url.clone(),
                final_url: Mutex::new(task.original_url.clone()),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                cookie: task.cookie.clone(),
                referrer: task.referrer.clone(),
                user_agent: task.user_agent.clone(),
                total_size: AtomicU64::new(0),
                downloaded: Arc::new(AtomicU64::new(0)),
                network_downloaded: Arc::new(AtomicU64::new(0)),
                speed_limiter: Mutex::new(Instant::now()),
                status: Arc::new(std::sync::Mutex::new(DownloadStatus::Queued)),
                abort_tx: Some(abort_tx),
                chunks: std::sync::Mutex::new(vec![]),
                speed: Arc::new(std::sync::Mutex::new(0.0)),
                eta: Arc::new(std::sync::Mutex::new("---".to_string())),
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
