mod engine;

use std::sync::Arc;
use tauri::{State, Manager};
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
            let manager = manager_for_setup;
            // Bind the app handle so we can stream progress events to frontend windows
            tokio::spawn(async move {
                manager.set_app_handle(handle).await;
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
