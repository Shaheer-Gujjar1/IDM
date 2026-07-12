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
}

pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub save_path: String,
    pub cookie: String,
    pub referrer: String,
    pub total_size: u64,
    pub downloaded: Arc<AtomicU64>,
    pub status: Arc<RwLock<DownloadStatus>>,
    pub abort_tx: Option<broadcast::Sender<()>>,
    pub chunks: std::sync::Mutex<Vec<DownloadChunk>>,
    pub speed: Arc<RwLock<f64>>,
    pub eta: Arc<RwLock<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PersistentTask {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub save_path: String,
    pub cookie: String,
    pub referrer: String,
    pub total_size: u64,
    pub downloaded: u64,
    pub status: DownloadStatus,
    pub chunks: Vec<DownloadChunk>,
}

pub struct DownloadManager {
    pub tasks: RwLock<HashMap<String, Arc<DownloadTask>>>,
    pub app_handle: Mutex<Option<tauri::AppHandle>>,
    pub client: reqwest::Client,
}

impl DownloadManager {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            // tcp_nodelay DISABLED: disabling Nagle's algorithm helps latency but HURTS bulk download throughput
            .pool_max_idle_per_host(32)
            .connection_verbose(false)
            .build()
            .unwrap_or_default();

        Self {
            tasks: RwLock::new(HashMap::new()),
            app_handle: Mutex::new(None),
            client,
        }
    }

    pub async fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.lock().await = Some(handle);
    }

    pub async fn save_history(&self) -> Result<(), String> {
        let app_handle_guard = self.app_handle.lock().await;
        if let Some(ref handle) = *app_handle_guard {
            use tauri::Manager;
            let app_dir = handle.path().app_data_dir().map_err(|e| e.to_string())?;
            let _ = tokio::fs::create_dir_all(&app_dir).await;
            let history_path = app_dir.join("history.json");
            
            let tasks = self.tasks.read().await;
            let mut persistent_tasks = Vec::new();
            for task in tasks.values() {
                let downloaded = task.downloaded.load(Ordering::Relaxed);
                let status = task.status.read().await.clone();
                let chunks = task.chunks.lock().unwrap().clone();
                persistent_tasks.push(PersistentTask {
                    id: task.id.clone(),
                    url: task.url.clone(),
                    filename: task.filename.clone(),
                    save_path: task.save_path.clone(),
                    cookie: task.cookie.clone(),
                    referrer: task.referrer.clone(),
                    total_size: task.total_size,
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
        let app_handle_guard = self.app_handle.lock().await;
        if let Some(ref handle) = *app_handle_guard {
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
                                total_size: p_task.total_size,
                                downloaded: Arc::new(AtomicU64::new(p_task.downloaded)),
                                status: Arc::new(RwLock::new(status)),
                                abort_tx: None,
                                chunks: std::sync::Mutex::new(p_task.chunks),
                                speed: Arc::new(RwLock::new(0.0)),
                                eta: Arc::new(RwLock::new("---".to_string())),
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
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        
        // Reuse shared manager client — avoids redundant TLS handshakes
        let mut head_req = self.client.head(&url);
        if !cookie.is_empty() {
            head_req = head_req.header(reqwest::header::COOKIE, &cookie);
        }
        if !referrer.is_empty() {
            head_req = head_req.header(reqwest::header::REFERER, &referrer);
        }

        let mut total_size = 0;
        let mut accept_ranges = false;

        // Try HEAD request to resolve size/ranges, but fallback to 0/dynamic on fail
        if let Ok(res) = head_req.send().await {
            if res.status().is_success() {
                total_size = res
                    .headers()
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|val| val.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);

                accept_ranges = res
                    .headers()
                    .get(reqwest::header::ACCEPT_RANGES)
                    .and_then(|val| val.to_str().ok())
                    .map(|s| s == "bytes")
                    .unwrap_or(false);
            }
        }

        let (abort_tx, _) = broadcast::channel(1);
        let task = Arc::new(DownloadTask {
            id: id.clone(),
            url: url.clone(),
            filename: filename.clone(),
            save_path: save_path.clone(),
            cookie: cookie.clone(),
            referrer: referrer.clone(),
            total_size,
            downloaded: Arc::new(AtomicU64::new(0)),
            status: Arc::new(RwLock::new(DownloadStatus::Queued)),
            abort_tx: Some(abort_tx),
            chunks: std::sync::Mutex::new(vec![]),
            speed: Arc::new(RwLock::new(0.0)),
            eta: Arc::new(RwLock::new("---".to_string())),
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
        *task.status.write().await = DownloadStatus::Downloading;

        let is_new = task.downloaded.load(Ordering::Relaxed) == 0;
        let num_chunks = if accept_ranges && task.total_size > 0 { 8 } else { 1 };

        if let Some(parent) = std::path::Path::new(&task.save_path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let file_exists = tokio::fs::metadata(&task.save_path).await.is_ok();
        let should_initialize = is_new || !file_exists;

        if should_initialize {
            task.downloaded.store(0, Ordering::Relaxed);
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&task.save_path)
                .await
                .map_err(|e| format!("Failed to create file: {}", e))?;
            if task.total_size > 0 {
                let _ = file.set_len(task.total_size).await;
            }

            // Initialize connection chunks
            let mut chunks = vec![];
            if task.total_size > 0 {
                let chunk_size = task.total_size / num_chunks;
                for i in 0..num_chunks {
                    let start = i * chunk_size;
                    let end = if i == num_chunks - 1 {
                        task.total_size - 1
                    } else {
                        (i + 1) * chunk_size - 1
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
        let total_size = task.total_size;
        let downloaded_counter = task.downloaded.clone();
        let status_ref = task.status.clone();
        let mut abort_rx = abort_tx.subscribe();
        let app_handle_opt = self.app_handle.lock().await.clone();
        let task_speed = task.speed.clone();
        let task_eta = task.eta.clone();
        let manager_for_save = self.clone();

        // Speed & Progress reporting loop
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(800));
            let mut last_bytes = downloaded_counter.load(Ordering::Relaxed);
            let mut last_check = Instant::now();
            let mut save_ticks = 0;

            loop {
                tokio::select! {
                    _ = abort_rx.recv() => break,
                    _ = interval.tick() => {
                        let current_bytes = downloaded_counter.load(Ordering::Relaxed);
                        let now = Instant::now();
                        let duration = now.duration_since(last_check).as_secs_f64();
                        let raw_speed = if duration > 0.0 {
                            (current_bytes - last_bytes) as f64 / duration
                        } else {
                            0.0
                        };

                        let mut speed_guard = task_speed.write().await;
                        let last_speed = *speed_guard;
                        let speed = if last_speed <= 0.0 {
                            raw_speed
                        } else {
                            // 0.3 weight to current speed, 0.7 to historical trend
                            0.3 * raw_speed + 0.7 * last_speed
                        };
                        *speed_guard = speed;

                        let eta = if speed > 1024.0 && total_size > current_bytes {
                            let rem = total_size - current_bytes;
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

                        let status = status_ref.read().await.clone();
                        *task_eta.write().await = eta.clone();

                        let progress = DownloadProgress {
                            id: id_clone.clone(),
                            filename: filename_clone.clone(),
                            save_path: save_path_clone.clone(),
                            total_size,
                            downloaded: current_bytes,
                            speed,
                            eta,
                            status: status.clone(),
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

                        last_bytes = current_bytes;
                        last_check = now;
                    }
                }
            }
        });

        // Spawn segment download workers
        for (idx, chunk) in chunks_list.into_iter().enumerate() {
            let url = task.url.clone();
            let save_path = task.save_path.clone();
            let downloaded = task.downloaded.clone();
            let task_clone = task.clone();
            let mut task_abort_rx = abort_tx.subscribe();
            let client = self.client.clone();

            let worker = tokio::spawn(async move {
                let start_offset = chunk.start + chunk.downloaded;
                let end_offset = chunk.end;
                if end_offset > 0 && start_offset >= end_offset {
                    return;
                }

                let mut req = client.get(&url);
                if !task_clone.cookie.is_empty() {
                    req = req.header(reqwest::header::COOKIE, &task_clone.cookie);
                }
                if !task_clone.referrer.is_empty() {
                    req = req.header(reqwest::header::REFERER, &task_clone.referrer);
                }
                if num_chunks > 1 && end_offset > 0 {
                    req = req.header(reqwest::header::RANGE, format!("bytes={}-{}", start_offset, end_offset));
                }

                let res = match req.send().await {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("[Rust Engine] Worker request failed: {:?}", e);
                        return;
                    }
                };

                if !res.status().is_success() {
                    eprintln!("[Rust Engine] Worker HTTP request failed: {}", res.status());
                    return;
                }

                let mut stream = res.bytes_stream();
                let mut local_downloaded = chunk.downloaded;
                let mut last_saved_downloaded = chunk.downloaded;

                // --- Blocking writer thread ---
                // Open a *std* file handle so the writer thread never touches async I/O
                let std_file = match std::fs::OpenOptions::new().write(true).open(&save_path) {
                    Ok(f) => f,
                    Err(_) => return,
                };

                // Channel: async reader → blocking writer
                // Up to 64 chunks of 512 KB = 32 MB in flight before back-pressure
                let (tx, rx) = std::sync::mpsc::sync_channel::<(u64, Vec<u8>)>(64);
                let downloaded_for_writer = downloaded.clone();

                // Spawn a blocking OS thread that does all disk I/O
                // total_size cap prevents counter overflow if server ignores range headers
                let writer_total_size = total_size;
                let writer_handle = tokio::task::spawn_blocking(move || {
                    let mut f = std_file;
                    for (write_pos, buf) in rx {
                        // Hard cap: never write beyond total file size
                        if writer_total_size > 0 {
                            let already = downloaded_for_writer.load(Ordering::Relaxed);
                            if already >= writer_total_size {
                                break; // done, discard remaining
                            }
                            let allowed = (writer_total_size - already).min(buf.len() as u64) as usize;
                            let safe_buf = &buf[..allowed];
                            if f.seek(std::io::SeekFrom::Start(write_pos)).is_ok() {
                                if f.write_all(safe_buf).is_ok() {
                                    downloaded_for_writer.fetch_add(allowed as u64, Ordering::Relaxed);
                                }
                            }
                        } else {
                            if f.seek(std::io::SeekFrom::Start(write_pos)).is_ok() {
                                if f.write_all(&buf).is_ok() {
                                    downloaded_for_writer.fetch_add(buf.len() as u64, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                    // Ensure everything is flushed to OS buffers
                    let _ = f.flush();
                });

                // --- Async network reader --- reads at full line speed, no disk awaits ---
                let mut net_buffer = Vec::with_capacity(512 * 1024);

                loop {
                    tokio::select! {
                        biased; // Check abort first, always
                        _ = task_abort_rx.recv() => {
                            break;
                        }
                        bytes_chunk = stream.next() => {
                            match bytes_chunk {
                                Some(Ok(bytes)) => {
                                    let mut bytes_to_add = bytes.as_ref();
                                    if num_chunks > 1 && end_offset > 0 {
                                        let current_pos = chunk.start + local_downloaded + net_buffer.len() as u64;
                                        if current_pos >= end_offset + 1 {
                                            break;
                                        }
                                        let remaining = (end_offset + 1) - current_pos;
                                        if bytes_to_add.len() as u64 > remaining {
                                            bytes_to_add = &bytes_to_add[..remaining as usize];
                                        }
                                    }

                                    net_buffer.extend_from_slice(bytes_to_add);

                                    // Flush to writer thread every 512 KB
                                    if net_buffer.len() >= 512 * 1024 {
                                        let write_pos = chunk.start + local_downloaded;
                                        let flushed = std::mem::replace(&mut net_buffer, Vec::with_capacity(512 * 1024));
                                        local_downloaded += flushed.len() as u64;

                                        // Lock chunks to save progress only every 8 MB to eliminate lock contention
                                        if local_downloaded - last_saved_downloaded >= 8 * 1024 * 1024 {
                                            last_saved_downloaded = local_downloaded;
                                            let mut chunks_lock = task_clone.chunks.lock().unwrap();
                                            if idx < chunks_lock.len() {
                                                chunks_lock[idx].downloaded = local_downloaded;
                                            }
                                        }

                                        // If channel is full this will block briefly, which is fine
                                        if tx.send((write_pos, flushed)).is_err() {
                                            break; // writer died
                                        }
                                    }

                                    // Segment boundary reached
                                    if num_chunks > 1 && end_offset > 0 {
                                        let current_pos = chunk.start + local_downloaded + net_buffer.len() as u64;
                                        if current_pos >= end_offset + 1 {
                                            break;
                                        }
                                    }
                                }
                                _ => break,
                            }
                        }
                    }
                }

                // Flush any remaining bytes
                if !net_buffer.is_empty() {
                    let write_pos = chunk.start + local_downloaded;
                    local_downloaded += net_buffer.len() as u64;
                    let _ = tx.send((write_pos, net_buffer));
                }

                // Drop tx so the writer thread knows we're done, then wait for it
                drop(tx);
                let _ = writer_handle.await;

                // Update segment offset state when aborted or finished
                let mut chunks_lock = task_clone.chunks.lock().unwrap();
                if idx < chunks_lock.len() {
                    chunks_lock[idx].downloaded = local_downloaded;
                }
            });

            workers.push(worker);
        }

        // Wait for workers to exit
        for worker in workers {
            let _ = worker.await;
        }

        let is_aborted = {
            let s = task.status.read().await;
            *s == DownloadStatus::Paused
        };

        if !is_aborted {
            let downloaded_bytes = task.downloaded.load(Ordering::Relaxed);
            if task.total_size > 0 && downloaded_bytes >= task.total_size {
                *task.status.write().await = DownloadStatus::Completed;
            } else if task.total_size == 0 && downloaded_bytes > 0 {
                *task.status.write().await = DownloadStatus::Completed;
            } else {
                *task.status.write().await = DownloadStatus::Failed("Download incomplete".to_string());
            }

            let final_status = task.status.read().await.clone();
            let progress = DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size,
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "0s".to_string(),
                status: final_status,
            };
            if let Some(ref handle) = self.app_handle.lock().await.clone() {
                use tauri::Emitter;
                let _ = handle.emit("download-progress", progress);
            }
        }

        let _ = self.save_history().await;
        Ok(())
    }

    pub async fn pause_download(&self, id: &str) -> Result<(), String> {
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            {
                let mut status = task.status.write().await;
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
                total_size: task.total_size,
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: DownloadStatus::Paused,
            };
            if let Some(ref handle) = self.app_handle.lock().await.clone() {
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

    pub async fn resume_download(&self, id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(id) {
            {
                let mut status = task.status.write().await;
                if *status != DownloadStatus::Paused && !matches!(*status, DownloadStatus::Failed(_)) {
                    return Err("Task is not paused or failed".to_string());
                }
                *status = DownloadStatus::Queued;
            }

            let accept_ranges = task.chunks.lock().unwrap().len() > 1;
            let chunks_clone = task.chunks.lock().unwrap().clone();

            let (abort_tx, _) = broadcast::channel(1);
            let updated_task = Arc::new(DownloadTask {
                id: task.id.clone(),
                url: task.url.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                cookie: task.cookie.clone(),
                referrer: task.referrer.clone(),
                total_size: task.total_size,
                downloaded: task.downloaded.clone(),
                status: task.status.clone(),
                abort_tx: Some(abort_tx),
                chunks: std::sync::Mutex::new(chunks_clone),
                speed: task.speed.clone(),
                eta: task.eta.clone(),
            });

            tasks.insert(id.to_string(), updated_task.clone());

            let manager_clone = Arc::new(Self {
                tasks: RwLock::new(tasks.clone()),
                app_handle: Mutex::new(self.app_handle.lock().await.clone()),
                client: self.client.clone(),
            });

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
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            if let Some(ref tx) = task.abort_tx {
                let _ = tx.send(());
            }
            
            *task.status.write().await = DownloadStatus::Failed("Cancelled by user".to_string());
            
            let progress = DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size,
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: DownloadStatus::Failed("Cancelled by user".to_string()),
            };
            if let Some(ref handle) = self.app_handle.lock().await.clone() {
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
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            if let Some(ref tx) = task.abort_tx {
                let _ = tx.send(());
            }
            *task.status.write().await = DownloadStatus::Trash;
            
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
                total_size: task.total_size,
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: DownloadStatus::Trash,
            };
            if let Some(ref handle) = self.app_handle.lock().await.clone() {
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
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(id) {
            let final_status = {
                let mut status = task.status.write().await;
                if *status == DownloadStatus::Trash {
                    let downloaded = task.downloaded.load(Ordering::Relaxed);
                    if task.total_size > 0 && downloaded >= task.total_size {
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
                total_size: task.total_size,
                downloaded: task.downloaded.load(Ordering::Relaxed),
                speed: 0.0,
                eta: "---".to_string(),
                status: final_status,
            };
            if let Some(ref handle) = self.app_handle.lock().await.clone() {
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

    pub async fn redownload_task(&self, id: &str) -> Result<(), String> {
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
                total_size: task.total_size,
                downloaded: Arc::new(AtomicU64::new(0)),
                status: Arc::new(RwLock::new(DownloadStatus::Queued)),
                abort_tx: Some(abort_tx),
                chunks: std::sync::Mutex::new(vec![]),
                speed: Arc::new(RwLock::new(0.0)),
                eta: Arc::new(RwLock::new("---".to_string())),
            });

            tasks.insert(id.to_string(), updated_task.clone());

            let manager_clone = Arc::new(Self {
                tasks: RwLock::new(tasks.clone()),
                app_handle: Mutex::new(self.app_handle.lock().await.clone()),
                client: self.client.clone(),
            });

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
            let speed = *task.speed.read().await;
            let eta = task.eta.read().await.clone();
            let status = task.status.read().await.clone();
            
            Some(DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size,
                downloaded: current_bytes,
                speed,
                eta,
                status,
            })
        } else {
            None
        }
    }

    pub async fn get_all_progress(&self) -> Vec<DownloadProgress> {
        let tasks = self.tasks.read().await;
        let mut list = vec![];
        for task in tasks.values() {
            let current_bytes = task.downloaded.load(Ordering::Relaxed);
            let status = task.status.read().await.clone();
            let speed = *task.speed.read().await;
            let eta = task.eta.read().await.clone();
            list.push(DownloadProgress {
                id: task.id.clone(),
                filename: task.filename.clone(),
                save_path: task.save_path.clone(),
                total_size: task.total_size,
                downloaded: current_bytes,
                speed,
                eta,
                status,
            });
        }
        list
    }
}
