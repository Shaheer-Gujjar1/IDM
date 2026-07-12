mod engine;

use std::sync::Arc;
use tauri::{State, Manager};
use engine::DownloadManager;

#[tauri::command]
async fn start_download(
    url: String,
    filename: String,
    save_path: String,
    cookie: Option<String>,
    referrer: Option<String>,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<String, String> {
    manager.start_download(url, filename, save_path, cookie.unwrap_or_default(), referrer.unwrap_or_default()).await
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
async fn open_progress_window(app_handle: tauri::AppHandle, id: String) -> Result<(), String> {
    let progress_url = format!("/index.html?popup=progress&id={}", id);
    
    // Spawn the progress window
    let _ = tauri::WebviewWindowBuilder::new(
        &app_handle,
        format!("popup-progress-{}", id),
        tauri::WebviewUrl::App(progress_url.into()),
    )
    .title("Downloading...")
    .inner_size(520.0, 300.0)
    .center()
    .resizable(false)
    .build()
    .map_err(|e| e.to_string())?;

    // Close the original Add window if open
    if let Some(add_win) = app_handle.get_webview_window("popup-add") {
        let _ = add_win.close();
    }
    Ok(())
}

#[tauri::command]
async fn open_complete_window(app_handle: tauri::AppHandle, filename: String) -> Result<(), String> {
    let complete_url = format!("/index.html?popup=complete&filename={}", urlencoding::encode(&filename));
    
    // Spawn the complete window
    let _ = tauri::WebviewWindowBuilder::new(
        &app_handle,
        format!("popup-complete-{}", uuid::Uuid::new_v4()),
        tauri::WebviewUrl::App(complete_url.into()),
    )
    .title("Download Finished")
    .inner_size(500.0, 240.0)
    .center()
    .resizable(false)
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

/// Resume a paused download AND immediately open/focus its progress popup window
#[allow(dead_code)]
#[tauri::command]
async fn resume_and_open_progress(
    id: String,
    app_handle: tauri::AppHandle,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.resume_download(&id).await?;

    let progress_url = format!("/index.html?popup=progress&id={}", id);
    let window_label = format!("popup-progress-{}", id);

    // If the window already exists, just focus it; otherwise create it
    if let Some(win) = app_handle.get_webview_window(&window_label) {
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        let _ = tauri::WebviewWindowBuilder::new(
            &app_handle,
            &window_label,
            tauri::WebviewUrl::App(progress_url.into()),
        )
        .title("Downloading...")
        .inner_size(520.0, 360.0)
        .center()
        .resizable(false)
        .build()
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn parse_content_disposition(value: &str) -> Option<String> {
    if let Some(pos) = value.find("filename=") {
        let mut filename = value[pos + 9..].trim().to_string();
        if let Some(semi) = filename.find(';') {
            filename = filename[..semi].trim().to_string();
        }
        if filename.starts_with('"') && filename.ends_with('"') {
            filename = filename[1..filename.len() - 1].to_string();
        }
        return Some(filename);
    }
    if let Some(pos) = value.find("filename*=") {
        let mut filename = value[pos + 10..].trim().to_string();
        if let Some(semi) = filename.find(';') {
            filename = filename[..semi].trim().to_string();
        }
        if filename.to_lowercase().starts_with("utf-8''") {
            filename = filename[7..].to_string();
        }
        if let Ok(decoded) = urlencoding::decode(&filename) {
            return Some(decoded.to_string());
        }
    }
    None
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let download_manager = Arc::new(DownloadManager::new());
    let manager_for_setup = download_manager.clone();

    tauri::Builder::default()
        .manage(download_manager)
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label().to_string();
                if label == "main" {
                    // Keep daemon alive — hide instead of exit
                    let _ = window.hide();
                    api.prevent_close();
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
            let handle = app.handle().clone();
            let handle_for_server = app.handle().clone();
            let manager = manager_for_setup;
            
            // Spawn app handle binding on Tauri's async runtime
            tauri::async_runtime::spawn(async move {
                manager.set_app_handle(handle).await;
            });

            // Spawn local capture server on Tauri's async runtime
            tauri::async_runtime::spawn(async move {
                let listener = match tokio::net::TcpListener::bind("127.0.0.1:9600").await {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("Failed to bind extension capture server to port 9600: {}", e);
                        return;
                    }
                };

                loop {
                    if let Ok((mut stream, _)) = listener.accept().await {
                        let app_handle = handle_for_server.clone();
                        tauri::async_runtime::spawn(async move {
                            use tokio::io::{AsyncReadExt, AsyncWriteExt};
                            let mut buffer = vec![0; 4096];
                            if let Ok(n) = stream.read(&mut buffer).await {
                                let req_str = String::from_utf8_lossy(&buffer[..n]);
                                
                                // Handle CORS Preflight request
                                if req_str.starts_with("OPTIONS") {
                                    let response = "HTTP/1.1 204 No Content\r\n\
                                                    Access-Control-Allow-Origin: *\r\n\
                                                    Access-Control-Allow-Methods: POST, OPTIONS\r\n\
                                                    Access-Control-Allow-Headers: Content-Type\r\n\
                                                    Access-Control-Max-Age: 86400\r\n\r\n";
                                    let _ = stream.write_all(response.as_bytes()).await;
                                    return;
                                }

                                // Check if it is a POST to /download
                                if req_str.starts_with("POST /download") || req_str.contains("POST /download") {
                                    if let Some(body_start) = req_str.find("\r\n\r\n") {
                                        let body = &req_str[body_start + 4..];
                                        
                                        #[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
                                        struct DownloadPayload {
                                            url: String,
                                            filename: String,
                                            cookie: Option<String>,
                                            referrer: Option<String>,
                                        }

                                        let body_clean = body.trim_end_matches('\0').trim();
                                        if let Ok(payload) = serde_json::from_str::<DownloadPayload>(body_clean) {
                                            // Resolve real filename via fast HTTP HEAD inspection
                                            let mut filename = payload.filename.clone();
                                            let client = reqwest::Client::builder()
                                                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                                                .redirect(reqwest::redirect::Policy::limited(10))
                                                .build()
                                                .unwrap_or_default();

                                            if let Ok(res) = client.head(&payload.url).timeout(std::time::Duration::from_secs(2)).send().await {
                                                if let Some(cd_val) = res.headers().get(reqwest::header::CONTENT_DISPOSITION).and_then(|h| h.to_str().ok()) {
                                                    if let Some(parsed) = parse_content_disposition(cd_val) {
                                                        filename = parsed;
                                                    }
                                                }

                                                let has_hash_filename = filename.chars().all(|c| c.is_numeric() || c.is_ascii_lowercase()) && filename.len() > 10;
                                                if has_hash_filename || filename == "download" || filename == "captured_download" {
                                                    let final_url = res.url().as_str();
                                                    if let Ok(parsed_url) = reqwest::Url::parse(final_url) {
                                                        if let Some(last_seg) = parsed_url.path_segments().and_then(|s| s.last()) {
                                                            if !last_seg.is_empty() && last_seg != "download" {
                                                                filename = last_seg.to_string();
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            // Spawn native popup-add window pre-filled with payload
                                            let add_url = format!(
                                                "/index.html?popup=add&url={}&filename={}&cookie={}&referrer={}",
                                                urlencoding::encode(&payload.url),
                                                urlencoding::encode(&filename),
                                                urlencoding::encode(&payload.cookie.unwrap_or_default()),
                                                urlencoding::encode(&payload.referrer.unwrap_or_default())
                                            );
                                            
                                            let _ = tauri::WebviewWindowBuilder::new(
                                                &app_handle,
                                                "popup-add",
                                                tauri::WebviewUrl::App(add_url.into()),
                                            )
                                            .title("New Download")
                                            .inner_size(520.0, 360.0)
                                            .center()
                                            .resizable(false)
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
                                }

                                let response = "HTTP/1.1 400 Bad Request\r\n\
                                                Access-Control-Allow-Origin: *\r\n\r\n";
                                let _ = stream.write_all(response.as_bytes()).await;
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
            cancel_download,
            select_folder,
            open_progress_window,
            open_complete_window,
            close_window,
            get_download_progress,
            get_all_downloads
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
