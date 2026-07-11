mod engine;

use std::sync::Arc;
use tauri::State;
use engine::DownloadManager;

#[tauri::command]
async fn start_download(
    url: String,
    filename: String,
    save_path: String,
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<String, String> {
    manager.start_download(url, filename, save_path).await
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let download_manager = Arc::new(DownloadManager::new());
    let manager_for_setup = download_manager.clone();

    tauri::Builder::default()
        .manage(download_manager)
        .plugin(tauri_plugin_opener::init())
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
                                        
                                        #[derive(serde::Deserialize, serde::Serialize, Clone)]
                                        struct DownloadPayload {
                                            url: String,
                                            filename: String,
                                        }

                                        let body_clean = body.trim_end_matches('\0').trim();
                                        if let Ok(payload) = serde_json::from_str::<DownloadPayload>(body_clean) {
                                            // Emit download-intercepted event to trigger Popup 1 (Add Modal) on frontend
                                            use tauri::Emitter;
                                            let _ = app_handle.emit("download-intercepted", payload);
                                            
                                            let response = "HTTP/1.1 200 OK\r\n\
                                                            Access-Control-Allow-Origin: *\r\n\
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
            cancel_download
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
