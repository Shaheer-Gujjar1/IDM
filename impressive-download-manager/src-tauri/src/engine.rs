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
    pub filename: String,
    pub save_path: String,
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

pub struct DownloadManager {
    pub tasks: RwLock<HashMap<String, Arc<DownloadTask>>>,
    pub app_handle: Mutex<Option<tauri::AppHandle>>,
    pub client: reqwest::Client,
    pub theme_mode: Mutex<String>,
    pub speed_limit_bps: AtomicU64,
    pub max_chunks: AtomicU64,
    pub intercept_downloads: std::sync::atomic::AtomicBool,
    pub minimize_to_tray: std::sync::atomic::AtomicBool,
}

impl DownloadManager {
    pub fn new() -> Self {
        // We use redirect policy "none" to handle redirects manually, but wait, 
        // reqwest auto redirects are fine if we get final_url in `start_download`. 
        // Then workers request final_url which shouldn't redirect further.
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .pool_max_idle_per_host(32)
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
                    filename: task.filename.clone(),
                    save_path: task.save_path.clone(),
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
                                filename: p_task.filename,
                                save_path: p_task.save_path,
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
        filename: String,
        save_path: String,
        cookie: String,
        referrer: String,
        user_agent: String,
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();

        let mut size_req = self.client.get(&url)
            .header(reqwest::header::RANGE, "bytes=0-0");
        if !cookie.is_empty() {
            size_req = size_req.header(reqwest::header::COOKIE, &cookie);
        }
        if !referrer.is_empty() {
            size_req = size_req.header(reqwest::header::REFERER, &referrer);
        }
        if !user_agent.is_empty() {
            size_req = size_req.header(reqwest::header::USER_AGENT, &user_agent);
        }

        let mut total_size = 0u64;
        let mut accept_ranges = false;
        let mut final_url = url.clone();

        if let Ok(res) = tokio::time::timeout(
            Duration::from_secs(10),
            size_req.send()
        ).await {
            if let Ok(res) = res {
                final_url = res.url().to_string();
                let status = res.status();
                if status == reqwest::StatusCode::PARTIAL_CONTENT {
                    accept_ranges = true;
                    if let Some(cr_val) = res.headers().get("Content-Range").and_then(|h| h.to_str().ok()) {
                        if let Some(slash_idx) = cr_val.rfind('/') {
                            if let Ok(s) = cr_val[slash_idx + 1..].trim().parse::<u64>() {
                                total_size = s;
                            }
                        }
                    }
                } else if status.is_success() {
                    total_size = res.headers()
                        .get(reqwest::header::CONTENT_LENGTH)
                        .and_then(|val| val.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                }
            }
        }

        let (abort_tx, _) = broadcast::channel(1);
        let task = Arc::new(DownloadTask {
            id: id.clone(),
            original_url: url.clone(),
            final_url: Mutex::new(final_url),
            filename: filename.clone(),
            save_path: save_path.clone(),
            cookie: cookie.clone(),
            referrer: referrer.clone(),
            user_agent: user_agent.clone(),
            total_size: AtomicU64::new(total_size),
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
            if let Err(e) = manager_clone.run_task(task, accept_ranges).await {
                eprintln!("Download failed: {}", e);
            }
        });

        Ok(id)
    }

    pub async fn run_task(self: &Arc<Self>, task: Arc<DownloadTask>, accept_ranges: bool) -> Result<(), String> {
        *task.status.lock().unwrap() = DownloadStatus::Downloading;

        let is_new = task.downloaded.load(Ordering::Relaxed) == 0;
        
        if let Some(parent) = std::path::Path::new(&task.save_path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let total_size_val = task.total_size.load(Ordering::Relaxed);

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
        let should_initialize = is_new || chunks_empty || !temp_dir_exists;

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
            if total_size_val > 0 {
                chunks.push(ActiveChunk {
                    id: uuid::Uuid::new_v4().to_string(),
                    start: 0,
                    end: Arc::new(AtomicU64::new(total_size_val - 1)),
                    downloaded: Arc::new(AtomicU64::new(0)),
                    active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                });
            } else {
                chunks.push(ActiveChunk {
                    id: uuid::Uuid::new_v4().to_string(),
                    start: 0,
                    end: Arc::new(AtomicU64::new(0)),
                    downloaded: Arc::new(AtomicU64::new(0)),
                    active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                });
            }
        } else {
            if !temp_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&temp_dir) {
                    return Err(format!("Could not create temp directory: {}", e));
                }
            }
        }

        let abort_tx = task.abort_tx.as_ref().unwrap();
        
        let id_clone = task.id.clone();
        let filename_clone = task.filename.clone();
        let save_path_clone = task.save_path.clone();
        let status_ref = task.status.clone();
        let mut abort_rx = abort_tx.subscribe();
        let app_handle_opt = self.app_handle.lock().await.clone();
        let task_speed = task.speed.clone();
        let task_eta = task.eta.clone();
        let manager_for_save = self.clone();
        let manager_for_reporting = self.clone();
        let task_for_reporting = task.clone();

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

                        let progress = DownloadProgress {
                            id: id_clone.clone(),
                            url: task_for_reporting.original_url.clone(),
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

        // Shared Blocking Writer Thread removed in favor of Part File Assembly

        // XDM Dynamic Chunk Spawner Logic
        let (worker_completed_tx, mut worker_completed_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut active_tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

        let target_workers = if accept_ranges && task.total_size.load(Ordering::Relaxed) > 0 { 
            self.max_chunks.load(Ordering::Relaxed) as usize 
        } else { 
            1 
        };

        // Initialize active flags to false
        {
            let chunks = task.chunks.lock().unwrap();
            for c in chunks.iter() {
                c.active.store(false, Ordering::SeqCst);
            }
        }

        let mut loop_abort_rx = abort_tx.subscribe();
        
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

            if active_tasks.len() >= target_workers {
                // Wait for a worker to finish or abort
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

            // Find an inactive chunk or split the largest active chunk
            let mut chunk_to_spawn = None;
            {
                let mut chunks = task.chunks.lock().unwrap();
                
                // 1. Find inactive chunk
                for chunk in chunks.iter() {
                    if !chunk.active.load(Ordering::SeqCst) {
                        let end = chunk.end.load(Ordering::SeqCst);
                        let downloaded = chunk.downloaded.load(Ordering::SeqCst);
                        if end > 0 && chunk.start + downloaded <= end {
                            chunk.active.store(true, Ordering::SeqCst);
                            chunk_to_spawn = Some(chunk.clone());
                            break;
                        }
                    }
                }
                
                // 2. If no inactive chunks, try to split the largest one
                if chunk_to_spawn.is_none() && accept_ranges {
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
                            
                            // Reduce the end of the existing active chunk. The active worker will naturally stop!
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
                let accept_ranges_clone = accept_ranges;
                
                let worker = tokio::spawn(async move {
                    let max_retries: u32 = 20;
                    let mut retry_count: u32 = 0;
                    
                    let mut net_buffer = Vec::with_capacity(256 * 1024);

                    'reconnect: loop {
                        use tokio::io::{AsyncSeekExt, AsyncWriteExt};
                        net_buffer.clear();

                        let chunk_path = temp_dir_clone.join(&chunk_id);
                        let mut file = match tokio::fs::OpenOptions::new().write(true).create(true).open(&chunk_path).await {
                            Ok(f) => f,
                            Err(_) => { retry_count += 1; tokio::time::sleep(Duration::from_millis(1000)).await; continue 'reconnect; }
                        };
                        
                        let local_downloaded = chunk.downloaded.load(Ordering::SeqCst);
                        if let Err(_) = file.seek(std::io::SeekFrom::Start(local_downloaded)).await {
                            retry_count += 1; tokio::time::sleep(Duration::from_millis(1000)).await; continue 'reconnect;
                        }
                        
                        let current_end = chunk.end.load(Ordering::SeqCst);
                        let local_downloaded = chunk.downloaded.load(Ordering::SeqCst);
                        let current_start = chunk.start + local_downloaded;
                        
                        if accept_ranges_clone && current_end > 0 && current_start > current_end {
                            break 'reconnect;
                        }
                        
                        if retry_count >= max_retries {
                            *task_clone.status.lock().unwrap() = DownloadStatus::Failed("Max retries exceeded".to_string());
                            break 'reconnect;
                        }

                        if retry_count > 0 { tokio::time::sleep(Duration::from_millis(1000)).await; }

                        let worker_url = task_clone.final_url.lock().await.clone();
                        let mut req = client.get(&worker_url)
                            .header(reqwest::header::ACCEPT, "*/*")
                            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
                            .header(reqwest::header::CONNECTION, "keep-alive");

                        if !task_clone.cookie.is_empty() { req = req.header(reqwest::header::COOKIE, &task_clone.cookie); }
                        if !task_clone.referrer.is_empty() { req = req.header(reqwest::header::REFERER, &task_clone.referrer); }
                        if !task_clone.user_agent.is_empty() { req = req.header(reqwest::header::USER_AGENT, &task_clone.user_agent); }

                        if accept_ranges_clone && current_end > 0 {
                            req = req.header(reqwest::header::RANGE, format!("bytes={}-{}", current_start, current_end));
                        } else if local_downloaded > 0 {
                            req = req.header(reqwest::header::RANGE, format!("bytes={}-", current_start));
                        }

                        let res = match req.send().await {
                            Ok(r) => r,
                            Err(_) => { retry_count += 1; continue 'reconnect; }
                        };

                        if !res.status().is_success() && res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                            retry_count += 1; continue 'reconnect;
                        }

                        if retry_count == 0 {
                            if let Some(cr_val) = res.headers().get("Content-Range").and_then(|h| h.to_str().ok()) {
                                if let Some(slash_idx) = cr_val.rfind('/') {
                                    if let Ok(s) = cr_val[slash_idx + 1..].trim().parse::<u64>() {
                                        if s > task_clone.total_size.load(Ordering::Relaxed) {
                                            task_clone.total_size.store(s, Ordering::Relaxed);
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
                                            let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(1024 * 1024));
                                            if let Ok(_) = file.write_all(&flushed).await {
                                                chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                            }
                                        }
                                        break 'reconnect;
                                    }
                                }
                                bytes_chunk_res = tokio::time::timeout(Duration::from_secs(15), stream.next()) => {
                                    let bytes_chunk = match bytes_chunk_res {
                                        Ok(b) => b,
                                        Err(_) => {
                                            if !net_buffer.is_empty() {
                                                let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(1024 * 1024));
                                                if let Ok(_) = file.write_all(&flushed).await {
                                                    chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                    downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                                }
                                            }
                                            retry_count += 1;
                                            continue 'reconnect; // timeout, force reconnect
                                        }
                                    };
                                    match bytes_chunk {
                                        Some(Ok(bytes)) => {
                                            retry_count = 0;
                                            let mut bytes_to_add = bytes.as_ref();

                                            // Re-evaluate end in case of dynamic split
                                            let dynamic_end = chunk.end.load(Ordering::SeqCst);
                                            let current_downloaded = chunk.downloaded.load(Ordering::SeqCst);
                                            let current_pos = chunk.start + current_downloaded + net_buffer.len() as u64;

                                            if accept_ranges_clone && dynamic_end > 0 {
                                                if current_pos >= dynamic_end + 1 {
                                                    if !net_buffer.is_empty() {
                                                        let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(1024 * 1024));
                                                        if let Ok(_) = file.write_all(&flushed).await {
                                                            chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                            downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                                        }
                                                    }
                                                    break 'reconnect; // Reached end
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

                                            if net_buffer.len() >= 1024 * 1024 { // 1MB buffer
                                                let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(1024 * 1024));
                                                if let Err(_) = file.write_all(&flushed).await {
                                                    retry_count += 1; continue 'reconnect;
                                                }
                                                chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                            }

                                            if accept_ranges_clone && dynamic_end > 0 {
                                                let current_dl = chunk.downloaded.load(Ordering::SeqCst);
                                                let current_pos = chunk.start + current_dl + net_buffer.len() as u64;
                                                if current_pos >= dynamic_end + 1 {
                                                    if !net_buffer.is_empty() {
                                                        let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(1024 * 1024));
                                                        if let Ok(_) = file.write_all(&flushed).await {
                                                            chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                            downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                                        }
                                                    }
                                                    break 'reconnect;
                                                }
                                            }
                                        }
                                        Some(Err(_)) => {
                                            if !net_buffer.is_empty() {
                                                let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(1024 * 1024));
                                                if let Ok(_) = file.write_all(&flushed).await {
                                                    chunk.downloaded.fetch_add(flushed.len() as u64, Ordering::SeqCst);
                                                    downloaded_counter_clone.fetch_add(flushed.len() as u64, Ordering::Relaxed);
                                                }
                                            }
                                            retry_count += 1; continue 'reconnect;
                                        }
                                        None => {
                                            if !net_buffer.is_empty() {
                                                let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(1024 * 1024));
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

                    // net_buffer is already guaranteed empty when breaking loop, 
                    // or flushed before break, because we replaced tx_clone with file.write_all
                    
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
                // If we couldn't spawn a new chunk, and active_tasks is 0, we are done
                if active_tasks.is_empty() { break; }
                
                // Wait for someone to finish
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
            let total_size_val = task.total_size.load(Ordering::Relaxed);
            
            let is_success = (total_size_val > 0 && downloaded_bytes >= total_size_val)
                || (total_size_val == 0 && downloaded_bytes > 0);

            if is_success {
                *task.status.lock().unwrap() = DownloadStatus::Assembling;

                // Emit Assembling progress
                let progress = DownloadProgress {
                    id: task.id.clone(),
                    url: task.original_url.clone(),
                    filename: task.filename.clone(),
                    save_path: task.save_path.clone(),
                    total_size: total_size_val,
                    downloaded: downloaded_bytes,
                    speed: 0.0,
                    eta: "Assembling...".to_string(),
                    status: DownloadStatus::Assembling,
                    file_exists: std::path::Path::new(&task.save_path).exists(),
                    speed_limited: false,
                };
                if let Some(ref handle) = self.app_handle.lock().await.clone() {
                    use tauri::Emitter;
                    let _ = handle.emit("download-progress", progress);
                }

                // Assembling Phase
                let temp_dir_clone = temp_dir.clone();
                let save_path_clone = task.save_path.clone();
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
                            let expected_len = if chunk_end >= chunk.start {
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
                    // keep original error
                } else {
                    *task.status.lock().unwrap() = DownloadStatus::Failed("No bytes received".to_string());
                }
            } else {
                if let DownloadStatus::Failed(_) = *task.status.lock().unwrap() {
                    // keep original error
                } else {
                    *task.status.lock().unwrap() = DownloadStatus::Failed(format!("Download incomplete ({}/{} bytes)", downloaded_bytes, total_size_val));
                }
            }
        }

        let final_status = task.status.lock().unwrap().clone();
        let progress = DownloadProgress {
            id: task.id.clone(),
            url: task.original_url.clone(),
            filename: task.filename.clone(),
            save_path: task.save_path.clone(),
            total_size: task.total_size.load(Ordering::Relaxed),
            downloaded: task.network_downloaded.load(Ordering::Relaxed),
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
                if *status == DownloadStatus::Completed || matches!(*status, DownloadStatus::Failed(_)) { return Ok(()); }
                *status = DownloadStatus::Paused;
            }
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }

            let progress = DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.network_downloaded.load(Ordering::Relaxed),
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
            let chunks_clone = { task.chunks.lock().unwrap().clone() };

            {
                let mut status_lock = task.status.lock().unwrap();
                if *status_lock == DownloadStatus::Completed || *status_lock == DownloadStatus::Downloading { return Ok(()); }
                *status_lock = DownloadStatus::Queued;
            }

            let accept_ranges = chunks_clone.len() > 1 || {
                let sz = task.total_size.load(Ordering::Relaxed);
                sz > 0
            };

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
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }
            
            *task.status.lock().unwrap() = DownloadStatus::Paused;
            *task.speed.lock().unwrap() = 0.0;
            *task.eta.lock().unwrap() = "Paused".to_string();
            
            let progress = DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.network_downloaded.load(Ordering::Relaxed),
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

            let path = task.save_path.clone();
            tokio::spawn(async move {
                if delete_file { let _ = tokio::fs::remove_file(path).await; }
                let _ = tokio::fs::remove_dir_all(temp_dir).await;
            });
            
            let progress = DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.network_downloaded.load(Ordering::Relaxed),
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
                url: task.original_url.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size.load(Ordering::Relaxed),
                downloaded: task.network_downloaded.load(Ordering::Relaxed),
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
            if let Some(ref tx) = task.abort_tx { let _ = tx.send(()); }

            let path = task.save_path.clone();
            let _ = tokio::fs::remove_file(path).await;

            let accept_ranges = task.chunks.lock().unwrap().len() > 1 || task.total_size.load(Ordering::Relaxed) > 0;

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
            let current_bytes = task.network_downloaded.load(Ordering::Relaxed);
            let speed = *task.speed.lock().unwrap();
            let eta = task.eta.lock().unwrap().clone();
            let status = task.status.lock().unwrap().clone();
            
            let current_limit = self.speed_limit_bps.load(Ordering::Relaxed);
            let is_speed_limited = current_limit > 0 && speed > 0.0;

            Some(DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
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
            let current_bytes = task.network_downloaded.load(Ordering::Relaxed);
            let status = task.status.lock().unwrap().clone();
            let speed = *task.speed.lock().unwrap();
            let eta = task.eta.lock().unwrap().clone();
            let is_speed_limited = current_limit > 0 && speed > 0.0;
            list.push(DownloadProgress {
                id: task.id.clone(),
                url: task.original_url.clone(),
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
