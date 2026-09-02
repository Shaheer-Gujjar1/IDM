mod engine;
mod proxy;

use std::sync::Arc;
use tauri::{State, Manager};
use engine::DownloadManager;

#[tauri::command]
async fn start_proxy_server(
    proxy: State<'_, Arc<proxy::ProxyServer>>,
) -> Result<(), String> {
    proxy.start().await
}

#[tauri::command]
async fn stop_proxy_server(
    proxy: State<'_, Arc<proxy::ProxyServer>>,
) -> Result<(), String> {
    proxy.stop().await
}

#[tauri::command]
async fn start_download(
    url: String,
    filename: String,
    save_path: String,
    cookie: Option<String>,
    referrer: Option<String>,
    user_agent: Option<String>,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<String, String> {
    println!("[Tauri IPC] start_download called: {}", url);
    manager
        .start_download(
            url,
            filename,
            save_path,
            cookie.unwrap_or_default(),
            referrer.unwrap_or_default(),
            user_agent.unwrap_or_default(),
        )
        .await
}

#[tauri::command]
async fn pause_download(
    id: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.pause_download(&id).await
}

#[tauri::command]
async fn resume_download(
    id: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.resume_download(&id).await
}

#[tauri::command]
async fn cancel_download(
    id: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.cancel_download(&id).await
}

#[tauri::command]
async fn delete_task(
    id: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.delete_task(&id).await
}

#[tauri::command]
async fn trash_task(
    id: String,
    delete_file: bool,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.trash_task(&id, delete_file).await
}

#[tauri::command]
async fn restore_task(
    id: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.restore_task(&id).await
}

#[tauri::command]
async fn redownload_task(
    id: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.redownload_task(&id).await
}

#[tauri::command]
async fn refresh_download_link(
    id: String,
    app_handle: tauri::AppHandle,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    let tasks = manager.tasks.read().await;
    if let Some(task) = tasks.get(&id) {
        if let Some(ref tx) = task.abort_tx {
            let _ = tx.send(());
        }

        *task.status.lock().unwrap() = engine::DownloadStatus::Failed("REFRESHING".to_string());
        
        let browser_url = if !task.referrer.is_empty() {
            task.referrer.clone()
        } else {
            task.original_url.clone()
        };

        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(&browser_url)
                .spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(&["/C", "start", &browser_url])
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open")
                .arg(&browser_url)
                .spawn();
        }

        let refresh_url = format!("index.html#popup=refresh&id={}", id);
        let _ = tauri::WebviewWindowBuilder::new(
            &app_handle,
            format!("popup-refresh-{}", id),
            tauri::WebviewUrl::App(refresh_url.into()),
        )
        .title("Refreshing Link...")
        .inner_size(520.0, 300.0)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
        
        drop(tasks);
        let _ = manager.save_history().await;
    }
    Ok(())
}

#[tauri::command]
async fn open_file_dir(path: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let path_clone = path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let raw_p = std::path::Path::new(&path_clone);
            let p = raw_p.canonicalize().unwrap_or_else(|_| raw_p.to_path_buf());
            let canonical_str = p.to_string_lossy().to_string();

            let parent_dir = if p.is_dir() {
                p.clone()
            } else {
                p.parent().map(|parent| parent.to_path_buf()).unwrap_or_else(|| p.clone())
            };

            if !p.is_dir() && p.exists() {
                let file_uri = format!("file://{}", canonical_str);
                let gdbus_cmd = format!(
                    "gdbus call --session --dest org.freedesktop.FileManager1 --object-path /org/freedesktop/FileManager1 --method org.freedesktop.FileManager1.ShowItems \"['{}']\" \"\"",
                    file_uri
                );

                // 1. Freedesktop DBus ShowItems via sh -c gdbus (Highlights file in Nautilus/Zorin, Dolphin, Nemo, Deepin, Thunar, etc.)
                if let Ok(mut child) = std::process::Command::new("sh")
                    .args(["-c", &gdbus_cmd])
                    .spawn()
                {
                    if let Ok(status) = child.wait() {
                        if status.success() {
                            return;
                        }
                    }
                }

                // 2. GNOME / Zorin OS (nautilus --select)
                if std::process::Command::new("nautilus")
                    .args(["--select", &canonical_str])
                    .spawn()
                    .is_ok()
                {
                    return;
                }

                // 3. KDE (dolphin --select)
                if std::process::Command::new("dolphin")
                    .args(["--select", &canonical_str])
                    .spawn()
                    .is_ok()
                {
                    return;
                }

                // 4. Cinnamon (nemo --select)
                if std::process::Command::new("nemo")
                    .args(["--select", &canonical_str])
                    .spawn()
                    .is_ok()
                {
                    return;
                }
            }

            // 5. Desktop Fallback: Open containing folder directory with dde-file-manager or xdg-open
            if std::process::Command::new("dde-file-manager").arg(&parent_dir).spawn().is_ok() {
                return;
            }
            let _ = std::process::Command::new("xdg-open").arg(&parent_dir).spawn();
        })
        .await;
    }
    #[cfg(target_os = "windows")]
    {
        let path_clone = path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let raw_p = std::path::Path::new(&path_clone);
            let p = raw_p.canonicalize().unwrap_or_else(|_| raw_p.to_path_buf());
            if p.is_dir() {
                let _ = std::process::Command::new("explorer").arg(&p).spawn();
            } else {
                let win_path = p.to_string_lossy().replace('/', "\\");
                let _ = std::process::Command::new("explorer").args(["/select,", &win_path]).spawn();
            }
        })
        .await;
    }
    #[cfg(target_os = "macos")]
    {
        let path_clone = path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let raw_p = std::path::Path::new(&path_clone);
            let p = raw_p.canonicalize().unwrap_or_else(|_| raw_p.to_path_buf());
            if p.is_dir() {
                let _ = std::process::Command::new("open").arg(&p).spawn();
            } else {
                let _ = std::process::Command::new("open").args(["-R", &p.to_string_lossy()]).spawn();
            }
        })
        .await;
    }
    Ok(())
}

#[tauri::command]
async fn open_file(path: String) -> Result<(), String> {
    let path_clone = path.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let raw_p = std::path::Path::new(&path_clone);
        let p = raw_p.canonicalize().unwrap_or_else(|_| raw_p.to_path_buf());
        let canonical_str = p.to_string_lossy().to_string();

        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&canonical_str).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let win_path = canonical_str.replace('/', "\\");
            let _ = std::process::Command::new("cmd").args(["/C", "start", "", &win_path]).spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&canonical_str).spawn();
        }
    })
    .await;
    Ok(())
}

#[tauri::command]
async fn select_folder() -> Result<String, String> {
    let folder = rfd::AsyncFileDialog::new()
        .pick_folder()
        .await;
    
    if let Some(path) = folder {
        Ok(path.path().to_string_lossy().to_string())
    } else {
        Err("Cancelled".to_string())
    }
}

#[tauri::command]
async fn get_default_download_dir() -> Result<String, String> {
    if let Some(dir) = dirs::download_dir() {
        Ok(dir.to_string_lossy().to_string())
    } else if let Some(home) = dirs::home_dir() {
        Ok(home.join("Downloads").to_string_lossy().to_string())
    } else {
        Ok("Downloads".to_string())
    }
}

#[tauri::command]
async fn toggle_autostart(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let desktop_path = std::path::Path::new(&home)
                .join(".config")
                .join("autostart")
                .join("impressive-download-manager.desktop");
            if enabled {
                if let Ok(current_exe) = std::env::current_exe().and_then(|p| p.canonicalize()) {
                    let exe_str = current_exe.to_string_lossy().to_string();
                    let desktop_content = format!(
                        "[Desktop Entry]\n\
                         Type=Application\n\
                         Exec=\"{}\" --background\n\
                         Hidden=false\n\
                         NoDisplay=false\n\
                         X-GNOME-Autostart-enabled=true\n\
                         Name=Impressive Download Manager\n\
                         Comment=Start Impressive Download Manager in the background\n",
                        exe_str
                    );
                    if std::fs::write(&desktop_path, desktop_content).is_ok() {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&desktop_path, std::fs::Permissions::from_mode(0o755));
                    }
                }
            } else {
                let _ = std::fs::remove_file(desktop_path);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let plist_path = std::path::Path::new(&home)
                .join("Library")
                .join("LaunchAgents")
                .join("com.impressive.idm.plist");
            if enabled {
                if let Ok(current_exe) = std::env::current_exe().and_then(|p| p.canonicalize()) {
                    let exe_str = current_exe.to_string_lossy().to_string();
                    let launch_agents_dir = std::path::Path::new(&home).join("Library").join("LaunchAgents");
                    let _ = std::fs::create_dir_all(&launch_agents_dir);
                    let plist_content = format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                         <plist version=\"1.0\">\n\
                         <dict>\n\
                           <key>Label</key><string>com.impressive.idm</string>\n\
                           <key>ProgramArguments</key>\n\
                           <array>\n\
                             <string>{}</string>\n\
                             <string>--background</string>\n\
                           </array>\n\
                           <key>RunAtLoad</key><true/>\n\
                         </dict>\n\
                         </plist>\n",
                        exe_str
                    );
                    let _ = std::fs::write(plist_path, plist_content);
                }
            } else {
                let _ = std::fs::remove_file(plist_path);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if enabled {
            if let Ok(current_exe) = std::env::current_exe() {
                let exe_str = current_exe.to_string_lossy().to_string();
                let cmd_str = format!("\"{}\" --background", exe_str);
                let _ = std::process::Command::new("reg")
                    .args(&[
                        "add",
                        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                        "/v",
                        "ImpressiveDownloadManager",
                        "/t",
                        "REG_SZ",
                        "/d",
                        &cmd_str,
                        "/f"
                    ])
                    .spawn();
            }
        } else {
            let _ = std::process::Command::new("reg")
                .args(&[
                    "delete",
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                    "/v",
                    "ImpressiveDownloadManager",
                    "/f"
                ])
                .spawn();
        }
    }
    Ok(())
}

#[tauri::command]
async fn set_speed_limit(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    limit_bps: u64,
) -> Result<(), String> {
    manager.speed_limit_bps.store(limit_bps, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn set_intercept_downloads(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    enabled: bool,
) -> Result<(), String> {
    manager.intercept_downloads.store(enabled, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn set_minimize_to_tray(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    enabled: bool,
) -> Result<(), String> {
    manager.minimize_to_tray.store(enabled, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn set_max_chunks(
    manager: tauri::State<'_, Arc<DownloadManager>>,
    max_chunks: u64,
) -> Result<(), String> {
    let clamped = max_chunks.clamp(1, 32);
    manager.max_chunks.store(clamped, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn get_scheduler_config(
    manager: tauri::State<'_, Arc<DownloadManager>>,
) -> Result<engine::SchedulerConfig, String> {
    Ok(manager.get_scheduler_config().await)
}

#[tauri::command]
async fn set_scheduler_config(
    config: engine::SchedulerConfig,
    manager: tauri::State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.save_scheduler_config(config).await
}

#[tauri::command]
async fn open_progress_window(app_handle: tauri::AppHandle, id: String) -> Result<(), String> {
    let progress_url = format!("index.html#popup=progress&id={}", id);
    
    // Spawn the progress window
    let _ = tauri::WebviewWindowBuilder::new(
        &app_handle,
        format!("popup-progress-{}", id),
        tauri::WebviewUrl::App(progress_url.into()),
    )
    .title("Downloading...")
    .inner_size(520.0, 340.0)
    .center()
    .build()
    .map_err(|e| e.to_string())?;

    // Close the original Add window if open
    if let Some(add_win) = app_handle.get_webview_window("popup-add") {
        let _ = add_win.close();
    }
    Ok(())
}

#[tauri::command]
async fn open_complete_window(app_handle: tauri::AppHandle, filename: String, save_path: String) -> Result<(), String> {
    let complete_url = format!(
        "index.html#popup=complete&filename={}&save_path={}",
        urlencoding::encode(&filename),
        urlencoding::encode(&save_path),
    );

    // Spawn the complete window
    let _ = tauri::WebviewWindowBuilder::new(
        &app_handle,
        format!("popup-complete-{}", uuid::Uuid::new_v4()),
        tauri::WebviewUrl::App(complete_url.into()),
    )
    .title("Download Finished")
    .inner_size(520.0, 360.0)
    .center()
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn close_window(window: tauri::Window) -> Result<(), String> {
    let _ = window.close();
    Ok(())
}

#[tauri::command]
async fn get_download_progress(
    id: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<Option<engine::DownloadProgress>, String> {
    Ok(manager.get_progress(&id).await)
}

#[tauri::command]
async fn get_all_downloads(
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<Vec<engine::DownloadProgress>, String> {
    Ok(manager.get_all_progress().await)
}

#[tauri::command]
async fn resume_and_open_progress(
    id: String,
    app_handle: tauri::AppHandle,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    let _ = manager.resume_download(&id).await;

    let progress_url = format!("index.html#popup=progress&id={}", id);
    let window_label = format!("popup-progress-{}", id);

    if let Some(win) = app_handle.get_webview_window(&window_label) {
        let _ = win.close();
    }

    let _ = tauri::WebviewWindowBuilder::new(
        &app_handle,
        &window_label,
        tauri::WebviewUrl::App(progress_url.into()),
    )
    .title("Downloading...")
    .inner_size(520.0, 340.0)
    .center()
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn sync_theme_mode(
    theme_mode: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    *manager.theme_mode.lock().await = theme_mode;
    Ok(())
}

#[tauri::command]
async fn redownload_and_open_progress(
    id: String,
    app_handle: tauri::AppHandle,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    let _ = manager.redownload_task(&id).await;

    let progress_url = format!("index.html#popup=progress&id={}", id);
    let window_label = format!("popup-progress-{}", id);

    if let Some(win) = app_handle.get_webview_window(&window_label) {
        let _ = win.close();
    }

    let _ = tauri::WebviewWindowBuilder::new(
        &app_handle,
        &window_label,
        tauri::WebviewUrl::App(progress_url.into()),
    )
    .title("Downloading...")
    .inner_size(520.0, 340.0)
    .center()
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn extract_sourceforge_mirror_url(html_body: &str) -> Option<String> {
    if let Some(pos) = html_body.find("https://downloads.sourceforge.net/project/") {
        let tail = &html_body[pos..];
        let end_idx = tail.find(|c: char| c == '"' || c == '\'' || c == ' ' || c == '<' || c == '\n' || c == '\r').unwrap_or(tail.len());
        return Some(tail[..end_idx].to_string());
    }
    if let Some(pos) = html_body.find(".dl.sourceforge.net/project/") {
        let head = &html_body[..pos];
        if let Some(scheme_pos) = head.rfind("http") {
            let tail = &html_body[scheme_pos..];
            let end_idx = tail.find(|c: char| c == '"' || c == '\'' || c == ' ' || c == '<' || c == '\n' || c == '\r').unwrap_or(tail.len());
            return Some(tail[..end_idx].to_string());
        }
    }
    None
}

#[allow(dead_code)]
pub fn parse_content_disposition(value: &str) -> Option<String> {
    proxy::parse_content_disposition(value)
}

#[allow(dead_code)]
pub fn sanitize_filename(name: &str) -> String {
    proxy::sanitize_filename(name)
}

#[allow(dead_code)]
pub fn is_generic_filename(name: &str) -> bool {
    proxy::is_generic_filename(name)
}

#[allow(dead_code)]
fn parse_content_range(value: &str) -> Option<u64> {
    if let Some(slash_idx) = value.rfind('/') {
        let total_str = value[slash_idx + 1..].trim();
        return total_str.parse::<u64>().ok();
    }
    None
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let download_manager = Arc::new(DownloadManager::new());
    let proxy_server = Arc::new(proxy::ProxyServer::new(8765));
    let manager_for_setup = download_manager.clone();
    let proxy_for_setup = proxy_server.clone();

    tauri::Builder::default()
        .manage(download_manager)
        .manage(proxy_server)
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_updater::Builder::new()
                .header(
                    "User-Agent",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
                )
                .expect("valid user-agent header")
                .header("Cache-Control", "no-cache, no-store, must-revalidate")
                .expect("valid cache-control header")
                .header("Pragma", "no-cache")
                .expect("valid pragma header")
                .build()
        )
        .plugin(tauri_plugin_process::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label().to_string();
                if label == "main" {
                    let minimize = if let Some(mgr) = window.app_handle().try_state::<Arc<DownloadManager>>() {
                        mgr.minimize_to_tray.load(std::sync::atomic::Ordering::Relaxed)
                    } else {
                        true
                    };

                    if minimize {
                        let _ = window.hide();
                        api.prevent_close();
                    } else {
                        // Minimize to tray is turned OFF: exit process completely like killall command
                        std::process::exit(0);
                    }
                } else if label.starts_with("popup-progress-") {
                    // Auto-pause the download when the progress popup is closed
                    let task_id = label.trim_start_matches("popup-progress-").to_string();
                    let app = window.app_handle().clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(mgr) = app.try_state::<std::sync::Arc<DownloadManager>>() {
                            let _ = mgr.pause_download(&task_id).await;
                        }
                    });
                }
            }
        })
        .setup(move |app| {
            // Bind port 9600 synchronously for single-instance check and reuse for capture server
            let std_listener = match std::net::TcpListener::bind("127.0.0.1:9600") {
                Ok(l) => l,
                Err(_) => {
                    // Another instance is already running. Notify it to show the main window.
                    if let Ok(mut stream) = std::net::TcpStream::connect("127.0.0.1:9600") {
                        use std::io::Write;
                        let _ = stream.write_all(b"POST /show-main HTTP/1.1\r\n\r\n");
                    }
                    // Exit this second instance immediately
                    std::process::exit(0);
                }
            };

            std_listener.set_nonblocking(true).ok();

            let handle = app.handle().clone();
            proxy_for_setup.set_app_handle(app.handle().clone());

            let proxy_start = proxy_for_setup.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = proxy_start.start().await {
                    eprintln!("Failed to start local HTTP proxy server: {}", e);
                }
            });

            let handle_for_server = app.handle().clone();
            let manager_for_server = manager_for_setup;
            
            let manager_for_init = manager_for_server.clone();
            // Spawn app handle binding on Tauri's async runtime
            tauri::async_runtime::spawn(async move {
                manager_for_init.set_app_handle(handle).await;
                let _ = manager_for_init.load_history().await;
                let _ = manager_for_init.load_scheduler_config().await;
                manager_for_init.start_scheduler_loop();

                // Write Native Messaging Host manifests for Linux browsers (Chrome, Brave, Chromium, Edge, Opera, Vivaldi, Firefox)
                #[cfg(target_os = "linux")]
                {
                    if let Ok(home) = std::env::var("HOME") {
                        if let Ok(current_exe) = std::env::current_exe().and_then(|p| p.canonicalize()) {
                            let exe_str = current_exe.to_string_lossy().to_string();
                            let browser_dirs = [
                                std::path::Path::new(&home).join(".config").join("BraveSoftware").join("Brave-Browser").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join(".config").join("google-chrome").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join(".config").join("chromium").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join(".config").join("microsoft-edge").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join(".config").join("opera").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join(".config").join("vivaldi").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join(".mozilla").join("native-messaging-hosts"),
                            ];

                            let native_manifest = format!(
                                "{{\n  \"name\": \"com.impressive.idm\",\n  \"description\": \"Impressive Download Manager Host\",\n  \"path\": \"{}\",\n  \"type\": \"stdio\",\n  \"allowed_origins\": [\"chrome-extension://*\", \"moz-extension://*\"],\n  \"allowed_extensions\": [\"*@*\"]\n}}\n",
                                exe_str
                            );

                            for b_dir in &browser_dirs {
                                if std::fs::create_dir_all(b_dir).is_ok() {
                                    let manifest_file = b_dir.join("com.impressive.idm.json");
                                    let _ = std::fs::write(manifest_file, &native_manifest);
                                }
                            }
                        }
                    }
                }

                // Windows Autostart & Native Messaging setup
                #[cfg(target_os = "windows")]
                {
                    if let Ok(current_exe) = std::env::current_exe() {
                        let exe_str = current_exe.to_string_lossy().to_string();
                        let cmd_str = format!("\"{}\" --background", exe_str);
                        let _ = std::process::Command::new("reg")
                            .args(&["add", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", "/v", "ImpressiveDownloadManager", "/t", "REG_SZ", "/d", &cmd_str, "/f"])
                            .spawn();

                        if let Ok(app_data) = std::env::var("APPDATA") {
                            let manifest_dir = std::path::Path::new(&app_data).join("ImpressiveDownloadManager");
                            if std::fs::create_dir_all(&manifest_dir).is_ok() {
                                let manifest_path = manifest_dir.join("com.impressive.idm.json");
                                let json_path_str = manifest_path.to_string_lossy().to_string();
                                let escaped_exe = exe_str.replace('\\', "\\\\");
                                let native_manifest = format!(
                                    "{{\n  \"name\": \"com.impressive.idm\",\n  \"description\": \"Impressive Download Manager Host\",\n  \"path\": \"{}\",\n  \"type\": \"stdio\",\n  \"allowed_origins\": [\"chrome-extension://*\", \"moz-extension://*\"],\n  \"allowed_extensions\": [\"*@*\"]\n}}\n",
                                    escaped_exe
                                );
                                if std::fs::write(&manifest_path, native_manifest).is_ok() {
                                    for reg_path in &[
                                        "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.impressive.idm",
                                        "HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.impressive.idm",
                                        "HKCU\\Software\\Mozilla\\NativeMessagingHosts\\com.impressive.idm",
                                    ] {
                                        let _ = std::process::Command::new("reg")
                                            .args(&["add", reg_path, "/ve", "/t", "REG_SZ", "/d", &json_path_str, "/f"])
                                            .spawn();
                                    }
                                }
                            }
                        }
                    }
                }

                // macOS Autostart LaunchAgent & Native Messaging setup
                #[cfg(target_os = "macos")]
                {
                    if let Ok(home) = std::env::var("HOME") {
                        if let Ok(current_exe) = std::env::current_exe().and_then(|p| p.canonicalize()) {
                            let exe_str = current_exe.to_string_lossy().to_string();
                            let launch_agents_dir = std::path::Path::new(&home).join("Library").join("LaunchAgents");
                            let _ = std::fs::create_dir_all(&launch_agents_dir);
                            let plist_path = launch_agents_dir.join("com.impressive.idm.plist");
                            let plist_content = format!(
                                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                                 <plist version=\"1.0\">\n\
                                 <dict>\n\
                                   <key>Label</key><string>com.impressive.idm</string>\n\
                                   <key>ProgramArguments</key>\n\
                                   <array>\n\
                                     <string>{}</string>\n\
                                     <string>--background</string>\n\
                                   </array>\n\
                                   <key>RunAtLoad</key><true/>\n\
                                 </dict>\n\
                                 </plist>\n",
                                exe_str
                            );
                            let _ = std::fs::write(plist_path, plist_content);

                            let browser_dirs = [
                                std::path::Path::new(&home).join("Library").join("Application Support").join("Google").join("Chrome").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join("Library").join("Application Support").join("BraveSoftware").join("Brave-Browser").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join("Library").join("Application Support").join("Microsoft Edge").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join("Library").join("Application Support").join("com.operasoftware.Opera").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join("Library").join("Application Support").join("Vivaldi").join("NativeMessagingHosts"),
                                std::path::Path::new(&home).join("Library").join("Application Support").join("Mozilla").join("NativeMessagingHosts"),
                            ];

                            let native_manifest = format!(
                                "{{\n  \"name\": \"com.impressive.idm\",\n  \"description\": \"Impressive Download Manager Host\",\n  \"path\": \"{}\",\n  \"type\": \"stdio\",\n  \"allowed_origins\": [\"chrome-extension://*\", \"moz-extension://*\"],\n  \"allowed_extensions\": [\"*@*\"]\n}}\n",
                                exe_str
                            );

                            for b_dir in &browser_dirs {
                                if std::fs::create_dir_all(b_dir).is_ok() {
                                    let manifest_file = b_dir.join("com.impressive.idm.json");
                                    let _ = std::fs::write(manifest_file, &native_manifest);
                                }
                            }
                        }
                    }
                }
            });

            // Parse --background arg and hide window if launched in background
            let args: Vec<String> = std::env::args().collect();
            if args.contains(&"--background".to_string()) {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.hide();
                }
            }

            // Spawn local capture server on Tauri's async runtime reusing the bound listener
            tauri::async_runtime::spawn(async move {
                let listener = match tokio::net::TcpListener::from_std(std_listener) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("Failed to convert std TcpListener to Tokio TcpListener: {}", e);
                        return;
                    }
                };

                loop {
                    if let Ok((mut stream, _)) = listener.accept().await {
                        let app_handle = handle_for_server.clone();
                        let manager = manager_for_server.clone();
                        tauri::async_runtime::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buffer = Vec::new();
                            let mut chunk = [0u8; 4096];
                            let max_limit = 2 * 1024 * 1024; // 2 MB max

                            loop {
                                let n = match stream.read(&mut chunk).await {
                                    Ok(0) => break,
                                    Ok(n) => n,
                                    Err(_) => break,
                                };
                                buffer.extend_from_slice(&chunk[..n]);
                                if buffer.len() > max_limit {
                                    break;
                                }

                                if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                                    let headers_str = String::from_utf8_lossy(&buffer[..pos]);
                                    let body_bytes = &buffer[pos + 4..];

                                    let content_length = headers_str
                                        .lines()
                                        .find(|l| l.to_lowercase().starts_with("content-length:"))
                                        .and_then(|l| l.split(':').nth(1))
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                        .unwrap_or(0);

                                    if content_length == 0 || body_bytes.len() >= content_length {
                                        break;
                                    }
                                }
                            }

                            let req_str = String::from_utf8_lossy(&buffer);
                            
                            // Handle CORS Preflight request
                            if req_str.starts_with("OPTIONS") {
                                let theme = manager.theme_mode.lock().await.clone();
                                let response = format!("HTTP/1.1 204 No Content\r\n\
                                                Access-Control-Allow-Origin: *\r\n\
                                                Access-Control-Allow-Methods: POST, OPTIONS\r\n\
                                                Access-Control-Allow-Headers: Content-Type\r\n\
                                                Access-Control-Expose-Headers: X-App-Theme\r\n\
                                                X-App-Theme: {}\r\n\
                                                Access-Control-Max-Age: 86400\r\n\r\n", theme);
                                let _ = stream.write_all(response.as_bytes()).await;
                                return;
                            }

                            // Check if it is a request to show the main window from a second instance
                            if req_str.starts_with("POST /show-main") || req_str.contains("POST /show-main") {
                                if let Some(main_win) = app_handle.get_webview_window("main") {
                                    let _ = main_win.show();
                                    let _ = main_win.set_focus();
                                    #[cfg(target_os = "linux")]
                                    {
                                        let _ = main_win.set_resizable(false);
                                        let _ = main_win.set_resizable(true);
                                    }
                                }
                                let response = "HTTP/1.1 200 OK\r\n\
                                                Access-Control-Allow-Origin: *\r\n\
                                                Content-Type: text/plain\r\n\r\n\
                                                ok";
                                let _ = stream.write_all(response.as_bytes()).await;
                                return;
                            }

                            // Check status
                            if req_str.starts_with("GET /status") || req_str.contains("GET /status") {
                                let theme = manager.theme_mode.lock().await.clone();
                                let enabled = manager.intercept_downloads.load(std::sync::atomic::Ordering::Relaxed);
                                let body = format!("{{\"enabled\":{},\"theme\":\"{}\"}}", enabled, theme);
                                let response = format!(
                                    "HTTP/1.1 200 OK\r\n\
                                     Access-Control-Allow-Origin: *\r\n\
                                     Content-Type: application/json\r\n\
                                     Content-Length: {}\r\n\r\n{}",
                                    body.len(),
                                    body
                                );
                                let _ = stream.write_all(response.as_bytes()).await;
                                return;
                            }

                            // Check if it is a POST to /download
                            if req_str.starts_with("POST /download") || req_str.contains("POST /download") {
                                if !manager.intercept_downloads.load(std::sync::atomic::Ordering::Relaxed) {
                                    let response = "HTTP/1.1 403 Forbidden\r\n\
                                                    Access-Control-Allow-Origin: *\r\n\
                                                    Content-Type: text/plain\r\n\r\n\
                                                    disabled";
                                    let _ = stream.write_all(response.as_bytes()).await;
                                    return;
                                }

                                if let Some(body_start) = req_str.find("\r\n\r\n") {
                                    let body = &req_str[body_start + 4..];
                                    
                                    #[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
                                    struct DownloadPayload {
                                        url: String,
                                        filename: String,
                                        cookie: Option<String>,
                                        referrer: Option<String>,
                                        size: Option<u64>,
                                        mime: Option<String>,
                                        #[serde(alias = "userAgent")]
                                        user_agent: Option<String>,
                                    }

                                    let body_clean = body.trim_end_matches('\0').trim();
                                    if let Ok(payload) = serde_json::from_str::<DownloadPayload>(body_clean) {
                                        let mut filename = proxy::sanitize_filename(&payload.filename);
                                        let target_download_url = payload.url.clone();
                                        let total_size = payload.size.unwrap_or(0);

                                        if !target_download_url.starts_with("http://") && !target_download_url.starts_with("https://") {
                                            let response = "HTTP/1.1 400 Bad Request\r\n\
                                                            Access-Control-Allow-Origin: *\r\n\
                                                            Content-Type: text/plain\r\n\r\n\
                                                            Unsupported protocol";
                                            let _ = stream.write_all(response.as_bytes()).await;
                                            return;
                                        }

                                        if proxy::is_generic_filename(&filename) {
                                            if let Ok(parsed_url) = reqwest::Url::parse(&target_download_url) {
                                                let mut found_in_query = false;
                                                for (k, v) in parsed_url.query_pairs() {
                                                    let k_lower = k.to_lowercase();
                                                    if (k_lower.contains("file") || k_lower.contains("name") || k_lower.contains("title")) && v.contains('.') {
                                                        if let Ok(decoded) = urlencoding::decode(&v) {
                                                            let sanitized = proxy::sanitize_filename(&decoded);
                                                            if !proxy::is_generic_filename(&sanitized) {
                                                                filename = sanitized;
                                                                found_in_query = true;
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                                if !found_in_query {
                                                    if let Some(last_seg) = parsed_url.path_segments().and_then(|s| s.last()) {
                                                        if !last_seg.is_empty() && !proxy::is_generic_filename(last_seg) {
                                                            if let Ok(decoded) = urlencoding::decode(last_seg) {
                                                                let sanitized = proxy::sanitize_filename(&decoded);
                                                                if !proxy::is_generic_filename(&sanitized) {
                                                                    filename = sanitized;
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        if proxy::is_generic_filename(&filename) {
                                            filename = "downloaded_file".to_string();
                                        }

                                        println!(
                                            "[Capture Server] Received download payload: URL={}, filename={}, size={}, cookie_len={}, referrer={}",
                                            target_download_url,
                                            filename,
                                            total_size,
                                            payload.cookie.as_ref().map(|c| c.len()).unwrap_or(0),
                                            payload.referrer.as_deref().unwrap_or("none")
                                        );

                                        let download_dir = dirs::download_dir()
                                            .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
                                            .unwrap_or_else(|| std::path::PathBuf::from("."));
                                        let save_path = download_dir.join(&filename).to_string_lossy().to_string();

                                        println!("[Capture Server] Starting engine with save_path: {}", save_path);

                                        // Check if there is an active task waiting for a refresh link
                                        let mut refresh_task_id = None;
                                        {
                                            let tasks = manager.tasks.read().await;
                                            for task in tasks.values() {
                                                let status = task.status.lock().unwrap();
                                                if let engine::DownloadStatus::Failed(ref msg) = *status {
                                                    if msg == "REFRESHING" {
                                                        refresh_task_id = Some(task.id.clone());
                                                        break;
                                                    }
                                                }
                                            }
                                        }

                                        if let Some(ref id) = refresh_task_id {
                                            {
                                                let mut tasks = manager.tasks.write().await;
                                                if let Some(task) = tasks.get_mut(id) {
                                                    let updated_task = std::sync::Arc::new(engine::DownloadTask {
                                                        id: task.id.clone(),
                                                        original_url: target_download_url.clone(),
                                                        final_url: tokio::sync::Mutex::new(target_download_url.clone()),
                                                        filename: task.filename.clone(),
                                                        save_path: task.save_path.clone(),
                                                        cookie: payload.cookie.clone().unwrap_or_default(),
                                                        referrer: payload.referrer.clone().unwrap_or_default(),
                                                        user_agent: payload.user_agent.clone().unwrap_or_default(),
                                                        total_size: std::sync::atomic::AtomicU64::new(task.total_size.load(std::sync::atomic::Ordering::Relaxed)),
                                                        downloaded: task.downloaded.clone(),
                                                        network_downloaded: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(task.downloaded.load(std::sync::atomic::Ordering::Relaxed))),
                                                        speed_limiter: tokio::sync::Mutex::new(std::time::Instant::now()),
                                                        status: std::sync::Arc::new(std::sync::Mutex::new(engine::DownloadStatus::Paused)),
                                                        abort_tx: None,
                                                        chunks: std::sync::Mutex::new(task.chunks.lock().unwrap().clone()),
                                                        speed: std::sync::Arc::new(std::sync::Mutex::new(0.0)),
                                                        eta: std::sync::Arc::new(std::sync::Mutex::new("---".to_string())),
                                                    });
                                                    tasks.insert(id.clone(), updated_task);
                                                }
                                            }

                                            let _ = manager.resume_download(id).await;

                                            if let Some(win) = app_handle.get_webview_window(&format!("popup-refresh-{}", id)) {
                                                let _ = win.close();
                                            }

                                            let progress_url = format!("index.html#popup=progress&id={}", id);
                                            let _ = tauri::WebviewWindowBuilder::new(
                                                &app_handle,
                                                format!("popup-progress-{}", id),
                                                tauri::WebviewUrl::App(progress_url.into()),
                                            )
                                            .title("Downloading...")
                                            .inner_size(520.0, 340.0)
                                            .center()
                                            .build();

                                            let response = "HTTP/1.1 200 OK\r\n\
                                                            Access-Control-Allow-Origin: *\r\n\
                                                            Content-Type: application/json\r\n\r\n\
                                                            {\"status\":\"ok\"}";
                                            let _ = stream.write_all(response.as_bytes()).await;
                                            return;
                                        }

                                        // Popup-based flow: Spawn native popup-add window pre-filled with payload
                                        let add_url = format!(
                                            "index.html#popup=add&url={}&filename={}&cookie={}&referrer={}&user_agent={}&size={}",
                                            urlencoding::encode(&target_download_url),
                                            urlencoding::encode(&filename),
                                            urlencoding::encode(&payload.cookie.unwrap_or_default()),
                                            urlencoding::encode(&payload.referrer.unwrap_or_default()),
                                            urlencoding::encode(&payload.user_agent.unwrap_or_default()),
                                            total_size
                                        );
                                        
                                        let _ = tauri::WebviewWindowBuilder::new(
                                            &app_handle,
                                            "popup-add",
                                            tauri::WebviewUrl::App(add_url.into()),
                                        )
                                        .title("New Download Captured")
                                        .inner_size(520.0, 370.0)
                                        .center()
                                        .build();
                                        
                                        let response = "HTTP/1.1 200 OK\r\n\
                                                        Access-Control-Allow-Origin: *\r\n\
                                                        Access-Control-Allow-Methods: POST, OPTIONS\r\n\
                                                        Access-Control-Allow-Headers: Content-Type\r\n\
                                                        Content-Type: application/json\r\n\r\n\
                                                        {\"status\":\"ok\"}";
                                        let _ = stream.write_all(response.as_bytes()).await;
                                        return;
                                    }
                                }

                                let response = "HTTP/1.1 400 Bad Request\r\n\
                                                Access-Control-Allow-Origin: *\r\n\r\n";
                                let _ = stream.write_all(response.as_bytes()).await;
                                return;
                            }
                        });
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_download,
            pause_download,
            resume_download,
            resume_and_open_progress,
            redownload_and_open_progress,
            cancel_download,
            delete_task,
            trash_task,
            restore_task,
            redownload_task,
            refresh_download_link,
            open_file,
            open_file_dir,
            select_folder,
            get_default_download_dir,
            toggle_autostart,
            open_progress_window,
            open_complete_window,
            close_window,
            get_download_progress,
            get_all_downloads,
            sync_theme_mode,
            set_speed_limit,
            set_intercept_downloads,
            set_minimize_to_tray,
            set_max_chunks,
            get_scheduler_config,
            set_scheduler_config,
            start_proxy_server,
            stop_proxy_server
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
