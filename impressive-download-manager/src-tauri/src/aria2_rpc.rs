use std::sync::Arc;
use std::sync::atomic::Ordering;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use crate::engine::{DownloadManager, DownloadStatus};

pub const DEFAULT_ARIA2_PORT: u16 = 6800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default)]
    pub jsonrpc: Option<String>,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Option<Value>, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
            }),
        }
    }
}

/// Helper to sanitize and unpack parameters, ignoring optional "token:secret" prefix if present
fn unpack_params(params: &[Value]) -> &[Value] {
    if let Some(first) = params.first() {
        if let Some(s) = first.as_str() {
            if s.starts_with("token:") {
                return &params[1..];
            }
        }
    }
    params
}

/// Dispatches an individual JSON-RPC 2.0 request against IDM's DownloadManager
pub async fn dispatch_rpc(req: JsonRpcRequest, manager: &Arc<DownloadManager>) -> JsonRpcResponse {
    let id = req.id.clone();
    let effective_params = unpack_params(&req.params);

    match req.method.as_str() {
        "aria2.addUri" => {
            // params: [ [uris...], options? ]
            if effective_params.is_empty() {
                return JsonRpcResponse::err(id, -32602, "Invalid params: uris required");
            }

            let uris = match effective_params[0].as_array() {
                Some(arr) if !arr.is_empty() => arr,
                _ => return JsonRpcResponse::err(id, -32602, "Invalid params: uris array cannot be empty"),
            };

            let target_url = match uris[0].as_str() {
                Some(u) => u.to_string(),
                None => return JsonRpcResponse::err(id, -32602, "Invalid uri string"),
            };

            let mut out_filename = String::new();
            let mut custom_dir = String::new();
            let mut cookie_header = String::new();
            let mut referer_header = String::new();
            let mut user_agent_header = String::new();

            if let Some(options) = effective_params.get(1).and_then(|v| v.as_object()) {
                if let Some(out) = options.get("out").and_then(|v| v.as_str()) {
                    out_filename = out.to_string();
                }
                if let Some(dir) = options.get("dir").and_then(|v| v.as_str()) {
                    custom_dir = dir.to_string();
                }
                if let Some(headers) = options.get("header").and_then(|v| v.as_array()) {
                    for h in headers {
                        if let Some(h_str) = h.as_str() {
                            let lower = h_str.to_lowercase();
                            if lower.starts_with("cookie:") {
                                cookie_header = h_str[7..].trim().to_string();
                            } else if lower.starts_with("referer:") {
                                referer_header = h_str[8..].trim().to_string();
                            } else if lower.starts_with("user-agent:") {
                                user_agent_header = h_str[11..].trim().to_string();
                            }
                        }
                    }
                }
            }

            let base_dir = if !custom_dir.is_empty() {
                std::path::PathBuf::from(custom_dir)
            } else {
                dirs::download_dir()
                    .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
            };

            let effective_filename = if !out_filename.is_empty() {
                crate::proxy::sanitize_filename(&out_filename)
            } else {
                "download_file".to_string()
            };

            let save_path = base_dir.join(&effective_filename).to_string_lossy().to_string();

            match manager
                .start_download(
                    target_url,
                    effective_filename,
                    save_path,
                    cookie_header,
                    referer_header,
                    user_agent_header,
                    None,
                    None,
                    None,
                    false,
                    true,
                )
                .await
            {
                Ok(gid) => JsonRpcResponse::success(id, json!(gid)),
                Err(e) => JsonRpcResponse::err(id, -32000, &format!("Failed to add download: {}", e)),
            }
        }

        "aria2.tellStatus" => {
            // params: [ "gid", [keys...] ]
            let gid = match effective_params.get(0).and_then(|v| v.as_str()) {
                Some(g) => g,
                None => return JsonRpcResponse::err(id, -32602, "GID required"),
            };

            let tasks_guard = manager.tasks.read().await;
            if let Some(task) = tasks_guard.get(gid) {
                let status_obj = format_task_status(task);
                JsonRpcResponse::success(id, status_obj)
            } else {
                JsonRpcResponse::err(id, -32000, &format!("GID {} not found", gid))
            }
        }

        "aria2.tellActive" => {
            let tasks_guard = manager.tasks.read().await;
            let mut active = Vec::new();
            for task in tasks_guard.values() {
                let status = task.status.lock().unwrap().clone();
                match status {
                    DownloadStatus::Downloading | DownloadStatus::Assembling => {
                        active.push(format_task_status(task));
                    }
                    _ => {}
                }
            }
            JsonRpcResponse::success(id, json!(active))
        }

        "aria2.tellWaiting" => {
            let tasks_guard = manager.tasks.read().await;
            let mut waiting = Vec::new();
            for task in tasks_guard.values() {
                let status = task.status.lock().unwrap().clone();
                match status {
                    DownloadStatus::Queued | DownloadStatus::Paused => {
                        waiting.push(format_task_status(task));
                    }
                    _ => {}
                }
            }
            JsonRpcResponse::success(id, json!(waiting))
        }

        "aria2.tellStopped" => {
            let tasks_guard = manager.tasks.read().await;
            let mut stopped = Vec::new();
            for task in tasks_guard.values() {
                let status = task.status.lock().unwrap().clone();
                match status {
                    DownloadStatus::Completed | DownloadStatus::Failed(_) => {
                        stopped.push(format_task_status(task));
                    }
                    _ => {}
                }
            }
            JsonRpcResponse::success(id, json!(stopped))
        }

        "aria2.pause" | "aria2.forcePause" => {
            let gid = match effective_params.get(0).and_then(|v| v.as_str()) {
                Some(g) => g,
                None => return JsonRpcResponse::err(id, -32602, "GID required"),
            };
            match manager.pause_download(gid).await {
                Ok(_) => JsonRpcResponse::success(id, json!(gid)),
                Err(e) => JsonRpcResponse::err(id, -32000, &format!("Failed to pause: {}", e)),
            }
        }

        "aria2.unpause" => {
            let gid = match effective_params.get(0).and_then(|v| v.as_str()) {
                Some(g) => g,
                None => return JsonRpcResponse::err(id, -32602, "GID required"),
            };
            match manager.resume_download(gid).await {
                Ok(_) => JsonRpcResponse::success(id, json!(gid)),
                Err(e) => JsonRpcResponse::err(id, -32000, &format!("Failed to unpause: {}", e)),
            }
        }

        "aria2.remove" | "aria2.forceRemove" => {
            let gid = match effective_params.get(0).and_then(|v| v.as_str()) {
                Some(g) => g,
                None => return JsonRpcResponse::err(id, -32602, "GID required"),
            };
            match manager.cancel_download(gid).await {
                Ok(_) => JsonRpcResponse::success(id, json!(gid)),
                Err(e) => JsonRpcResponse::err(id, -32000, &format!("Failed to remove: {}", e)),
            }
        }

        "aria2.getVersion" => {
            JsonRpcResponse::success(
                id,
                json!({
                    "version": "1.36.0",
                    "enabledFeatures": [
                        "Async DNS", "BitTorrent", "Firefox3 Cookie",
                        "GZip", "HTTPS", "Message Digest", "Metalink", "XML-RPC"
                    ]
                }),
            )
        }

        "aria2.getGlobalStat" => {
            let tasks_guard = manager.tasks.read().await;
            let mut active_count = 0usize;
            let mut waiting_count = 0usize;
            let mut stopped_count = 0usize;
            let mut total_speed = 0u64;

            for task in tasks_guard.values() {
                let status = task.status.lock().unwrap().clone();
                match status {
                    DownloadStatus::Downloading | DownloadStatus::Assembling => {
                        active_count += 1;
                        total_speed += *task.speed.lock().unwrap() as u64;
                    }
                    DownloadStatus::Queued | DownloadStatus::Paused => {
                        waiting_count += 1;
                    }
                    DownloadStatus::Completed | DownloadStatus::Failed(_) => {
                        stopped_count += 1;
                    }
                    DownloadStatus::Trash => {}
                }
            }

            JsonRpcResponse::success(
                id,
                json!({
                    "downloadSpeed": total_speed.to_string(),
                    "uploadSpeed": "0",
                    "numActive": active_count.to_string(),
                    "numWaiting": waiting_count.to_string(),
                    "numStopped": stopped_count.to_string(),
                    "numStoppedTotal": stopped_count.to_string()
                }),
            )
        }

        "system.multicall" => {
            // params: [ [ { "methodName": "...", "params": [...] } ] ]
            let calls = match effective_params.get(0).and_then(|v| v.as_array()) {
                Some(c) => c,
                None => return JsonRpcResponse::err(id, -32602, "Calls array required for multicall"),
            };

            let mut results = Vec::new();
            for call in calls {
                if let (Some(m_name), Some(m_params)) = (
                    call.get("methodName").and_then(|v| v.as_str()),
                    call.get("params").and_then(|v| v.as_array()),
                ) {
                    let sub_req = JsonRpcRequest {
                        jsonrpc: Some("2.0".to_string()),
                        id: None,
                        method: m_name.to_string(),
                        params: m_params.clone(),
                    };
                    let box_fut = Box::pin(dispatch_rpc(sub_req, manager));
                    let sub_resp = box_fut.await;
                    if let Some(res) = sub_resp.result {
                        results.push(json!([res]));
                    } else if let Some(err) = sub_resp.error {
                        results.push(json!({ "faultCode": err.code, "faultString": err.message }));
                    }
                }
            }
            JsonRpcResponse::success(id, json!(results))
        }

        _ => JsonRpcResponse::err(id, -32601, &format!("Method not found: {}", req.method)),
    }
}

/// Serializes task state into the official Aria2 JSON-RPC structure
fn format_task_status(task: &Arc<crate::engine::DownloadTask>) -> Value {
    let status_guard = task.status.lock().unwrap();
    let (status_str, err_code, err_msg) = match *status_guard {
        DownloadStatus::Downloading | DownloadStatus::Assembling => ("active", "0", ""),
        DownloadStatus::Queued => ("waiting", "0", ""),
        DownloadStatus::Paused => ("paused", "0", ""),
        DownloadStatus::Completed => ("complete", "0", ""),
        DownloadStatus::Failed(ref msg) => ("error", "1", msg.as_str()),
        DownloadStatus::Trash => ("removed", "0", ""),
    };

    let total_len = task.total_size.load(Ordering::Relaxed);
    let completed_len = task.network_downloaded.load(Ordering::Relaxed);
    let speed = *task.speed.lock().unwrap() as u64;
    let save_path = task.get_save_path();
    let dir = std::path::Path::new(&save_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    json!({
        "gid": task.id,
        "status": status_str,
        "totalLength": total_len.to_string(),
        "completedLength": completed_len.to_string(),
        "uploadLength": "0",
        "bitfield": "0",
        "downloadSpeed": speed.to_string(),
        "uploadSpeed": "0",
        "infoHash": "",
        "numSeeders": "0",
        "seeder": "false",
        "pieceLength": "1048576",
        "numPieces": "0",
        "connections": "8",
        "errorCode": err_code,
        "errorMessage": err_msg,
        "dir": dir,
        "files": [{
            "index": "1",
            "path": save_path,
            "length": total_len.to_string(),
            "completedLength": completed_len.to_string(),
            "selected": "true",
            "uris": [{
                "uri": task.original_url,
                "status": "used"
            }]
        }]
    })
}

/// Processes an incoming HTTP raw payload (handling CORS preflights, JSON-RPC 2.0 requests, and batching)
pub async fn process_http_request(raw_request: &str, manager: &Arc<DownloadManager>) -> String {
    let lines: Vec<&str> = raw_request.lines().collect();
    if lines.is_empty() {
        return "HTTP/1.1 400 Bad Request\r\n\r\n".to_string();
    }

    let first_line = lines[0].trim();
    if first_line.starts_with("OPTIONS") {
        return "HTTP/1.1 200 OK\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Access-Control-Allow-Methods: POST, GET, OPTIONS\r\n\
                Access-Control-Allow-Headers: Content-Type, Authorization, X-Requested-With, Origin, Accept\r\n\
                Access-Control-Max-Age: 86400\r\n\
                Content-Length: 0\r\n\r\n".to_string();
    }

    // Split headers and body
    let body = if let Some(idx) = raw_request.find("\r\n\r\n") {
        &raw_request[idx + 4..]
    } else if let Some(idx) = raw_request.find("\n\n") {
        &raw_request[idx + 2..]
    } else {
        ""
    }.trim();

    if body.is_empty() {
        return "HTTP/1.1 400 Bad Request\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Content-Type: text/plain\r\n\r\n\
                Empty RPC request body".to_string();
    }

    let response_body = if body.starts_with('[') {
        // Batch request
        if let Ok(batch) = serde_json::from_str::<Vec<JsonRpcRequest>>(body) {
            let mut responses = Vec::new();
            for req in batch {
                responses.push(dispatch_rpc(req, manager).await);
            }
            serde_json::to_string(&responses).unwrap_or_else(|_| "[]".to_string())
        } else {
            serde_json::to_string(&JsonRpcResponse::err(None, -32700, "Parse error")).unwrap()
        }
    } else if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(body) {
        let resp = dispatch_rpc(req, manager).await;
        serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string())
    } else {
        serde_json::to_string(&JsonRpcResponse::err(None, -32700, "Parse error")).unwrap()
    };

    format!(
        "HTTP/1.1 200 OK\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\r\n\
         {}",
        response_body.len(),
        response_body
    )
}

/// Starts the Aria2 JSON-RPC 2.0 Server on the specified port in the background
pub async fn start_aria2_server(port: u16, manager: Arc<DownloadManager>) -> Result<(), String> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => {
            println!("[Aria2 RPC] Compatibility Server listening on http://{}/jsonrpc", addr);
            l
        }
        Err(e) => {
            eprintln!("[Aria2 RPC] Could not bind to {}: {}. Trying port {}...", addr, e, port + 1);
            let alt_addr = format!("127.0.0.1:{}", port + 1);
            TcpListener::bind(&alt_addr).await.map_err(|e2| {
                format!("Failed to bind Aria2 RPC server on {} and {}: {}", addr, alt_addr, e2)
            })?
        }
    };

    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mgr = manager.clone();
                tokio::spawn(async move {
                    handle_connection(&mut stream, &mgr).await;
                });
            }
        }
    });

    Ok(())
}

async fn handle_connection(stream: &mut TcpStream, manager: &Arc<DownloadManager>) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];

    loop {
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                buffer.extend_from_slice(&chunk[..n]);
                if let Ok(text) = std::str::from_utf8(&buffer) {
                    if text.starts_with("OPTIONS") || text.contains("\r\n\r\n") || text.contains("\n\n") {
                        // Check Content-Length if present
                        if let Some(idx) = text.to_lowercase().find("content-length:") {
                            let rest = &text[idx + 15..];
                            if let Some(end_line) = rest.find("\r\n").or_else(|| rest.find('\n')) {
                                if let Ok(content_len) = rest[..end_line].trim().parse::<usize>() {
                                    let header_end = text.find("\r\n\r\n").map(|i| i + 4).or_else(|| text.find("\n\n").map(|i| i + 2)).unwrap_or(0);
                                    if buffer.len() < header_end + content_len {
                                        continue;
                                    }
                                }
                            }
                        }

                        let response = process_http_request(text, manager).await;
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.flush().await;
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_feature_1_aria2_get_version() {
        let manager = Arc::new(DownloadManager::new());
        let req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!("test_v1")),
            method: "aria2.getVersion".to_string(),
            params: vec![],
        };

        let resp = dispatch_rpc(req, &manager).await;
        assert_eq!(resp.id, Some(json!("test_v1")));
        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        assert_eq!(res["version"], "1.36.0");
        assert!(res["enabledFeatures"].as_array().unwrap().len() >= 5);
    }

    #[tokio::test]
    async fn test_feature_1_aria2_add_uri_and_tell_status() {
        let manager = Arc::new(DownloadManager::new());

        // Test aria2.addUri with options (dir, out, header)
        let add_req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!("add_1")),
            method: "aria2.addUri".to_string(),
            params: vec![
                json!(["https://example.com/archive.zip"]),
                json!({
                    "out": "custom_download.zip",
                    "header": [
                        "Cookie: session=xyz123",
                        "Referer: https://example.com/",
                        "User-Agent: Aria2Tester/1.0"
                    ]
                }),
            ],
        };

        let add_resp = dispatch_rpc(add_req, &manager).await;
        assert!(add_resp.error.is_none(), "addUri must succeed");
        let gid = add_resp.result.unwrap().as_str().unwrap().to_string();
        assert!(!gid.is_empty(), "GID must be non-empty");

        // Verify task exists with headers and filename
        let tasks = manager.tasks.read().await;
        let task = tasks.get(&gid).expect("Task must exist in manager");
        assert_eq!(task.get_filename(), "custom_download.zip");
        assert_eq!(task.cookie, "session=xyz123");
        assert_eq!(task.referrer, "https://example.com/");
        assert_eq!(task.user_agent, "Aria2Tester/1.0");
        drop(tasks);

        // Test aria2.tellStatus
        let status_req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!("stat_1")),
            method: "aria2.tellStatus".to_string(),
            params: vec![json!(gid)],
        };

        let status_resp = dispatch_rpc(status_req, &manager).await;
        assert!(status_resp.error.is_none(), "tellStatus must succeed");
        let st = status_resp.result.unwrap();
        assert_eq!(st["gid"], gid);
        assert!(st["files"].as_array().unwrap().len() == 1);
        assert_eq!(st["files"][0]["uris"][0]["uri"], "https://example.com/archive.zip");
    }

    #[tokio::test]
    async fn test_feature_1_aria2_pause_unpause_remove_lifecycle() {
        let manager = Arc::new(DownloadManager::new());

        let add_req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!("life_add")),
            method: "aria2.addUri".to_string(),
            params: vec![json!(["https://example.com/lifecycle.tar.gz"])],
        };

        let add_resp = dispatch_rpc(add_req, &manager).await;
        let gid = add_resp.result.unwrap().as_str().unwrap().to_string();

        // 1. Pause
        let pause_req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!("p1")),
            method: "aria2.pause".to_string(),
            params: vec![json!(gid)],
        };
        let p_res = dispatch_rpc(pause_req, &manager).await;
        assert!(p_res.error.is_none());

        let task = manager.tasks.read().await.get(&gid).cloned().unwrap();
        assert!(matches!(*task.status.lock().unwrap(), DownloadStatus::Paused));

        // 2. Unpause
        let unpause_req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!("u1")),
            method: "aria2.unpause".to_string(),
            params: vec![json!(gid)],
        };
        let u_res = dispatch_rpc(unpause_req, &manager).await;
        assert!(u_res.error.is_none());

        // 3. Remove
        let remove_req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!("r1")),
            method: "aria2.remove".to_string(),
            params: vec![json!(gid)],
        };
        let r_res = dispatch_rpc(remove_req, &manager).await;
        assert!(r_res.error.is_none());
    }

    #[tokio::test]
    async fn test_feature_1_aria2_http_cors_and_jsonrpc_batch() {
        let manager = Arc::new(DownloadManager::new());

        // 1. CORS Preflight OPTIONS request
        let options_http = "OPTIONS /jsonrpc HTTP/1.1\r\nHost: localhost:6800\r\nOrigin: chrome-extension://test\r\n\r\n";
        let opt_res = process_http_request(options_http, &manager).await;
        assert!(opt_res.starts_with("HTTP/1.1 200 OK"));
        assert!(opt_res.contains("Access-Control-Allow-Origin: *"));
        assert!(opt_res.contains("Access-Control-Allow-Methods: POST, GET, OPTIONS"));

        // 2. HTTP POST JSON-RPC Request
        let post_body = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": "http_1",
            "method": "aria2.getVersion",
            "params": []
        })).unwrap();

        let post_http = format!(
            "POST /jsonrpc HTTP/1.1\r\n\
             Host: localhost:6800\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n\
             {}",
            post_body.len(),
            post_body
        );

        let post_res = process_http_request(&post_http, &manager).await;
        assert!(post_res.starts_with("HTTP/1.1 200 OK"));
        assert!(post_res.contains("\"version\":\"1.36.0\""));
    }
}
