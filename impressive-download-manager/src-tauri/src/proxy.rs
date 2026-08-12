use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use hyper::service::service_fn;
use hyper::body::Incoming;
use hyper::{Request, Response, Method, StatusCode};
use hyper_util::rt::{TokioIo, TokioExecutor};
use hyper_util::server::conn::auto;
use http_body_util::{Full, combinators::BoxBody, BodyExt, Empty};
use bytes::Bytes;

pub struct ProxyServer {
    pub running: Arc<AtomicBool>,
    pub port: u16,
    pub app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    abort_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ProxyServer {
    pub fn new(port: u16) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            port,
            app_handle: Arc::new(Mutex::new(None)),
            abort_handle: Mutex::new(None),
        }
    }

    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        let app_handle = self.app_handle.clone();
        tauri::async_runtime::spawn(async move {
            *app_handle.lock().await = Some(handle);
        });
    }

    pub async fn start(&self) -> Result<(), String> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind proxy to {}: {}", addr, e))?;

        self.running.store(true, Ordering::Relaxed);
        let running_flag = self.running.clone();
        let app_handle_clone = self.app_handle.clone();

        println!("[Local Proxy Server] Started forward proxy listening on http://{}", addr);

        let handle = tokio::spawn(async move {
            let auto_server = auto::Builder::new(TokioExecutor::new());

            while running_flag.load(Ordering::Relaxed) {
                let (stream, peer_addr) = match listener.accept().await {
                    Ok(val) => val,
                    Err(_) => break,
                };

                let io = TokioIo::new(stream);
                let app_handle = app_handle_clone.clone();
                let service = service_fn(move |req| handle_proxy_request(req, peer_addr, app_handle.clone()));

                let auto_server = auto_server.clone();
                tokio::spawn(async move {
                    if let Err(err) = auto_server.serve_connection(io, service).await {
                        let _ = err;
                    }
                });
            }
            println!("[Local Proxy Server] Proxy server loop stopped.");
        });

        *self.abort_handle.lock().await = Some(handle);
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::Relaxed);
        let mut guard = self.abort_handle.lock().await;
        if let Some(handle) = guard.take() {
            handle.abort();
        }
        println!("[Local Proxy Server] Proxy server stopped.");
        Ok(())
    }
}

async fn handle_proxy_request(
    req: Request<Incoming>,
    peer_addr: SocketAddr,
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let method = req.method().clone();
    let uri = req.uri().clone();

    println!("[Proxy Log] [{}] {} {}", peer_addr, method, uri);

    // Phase 4: HTTPS CONNECT Tunneling & Download Pattern Inspection
    if method == Method::CONNECT {
        return handle_connect_tunnel(req).await;
    }

    // Handle Standard HTTP Pass-Through & Interception Handoff
    handle_http_passthrough(req, app_handle).await
}

async fn handle_connect_tunnel(
    req: Request<Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let host_port = match req.uri().authority() {
        Some(auth) => auth.to_string(),
        None => {
            let mut resp = Response::new(empty_body());
            *resp.status_mut() = StatusCode::BAD_REQUEST;
            return Ok(resp);
        }
    };

    let host_port_lower = host_port.to_lowercase();
    let is_known_download_host = [
        "downloads.sourceforge.net", "dl.sourceforge.net",
        "mediafire.com", "github.com", "objects.githubusercontent.com",
        "sourceforge.net"
    ].iter().any(|domain| host_port_lower.contains(domain));

    if is_known_download_host {
        println!("[Proxy HTTPS Target Inspection] 🔒 HTTPS CONNECT tunnel requested for high-probability download target host: {}", host_port);
    } else {
        println!("[Proxy HTTPS Tunnel] Establishing transparent TCP CONNECT tunnel to {}", host_port);
    }

    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let mut upgraded_io = TokioIo::new(upgraded);
                match TcpStream::connect(&host_port).await {
                    Ok(mut target_stream) => {
                        let _ = tokio::io::copy_bidirectional(&mut upgraded_io, &mut target_stream).await;
                    }
                    Err(e) => {
                        eprintln!("[Proxy Connect Error] Failed to connect to {}: {}", host_port, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("[Proxy Upgrade Error] Upgrade failed for {}: {}", host_port, e);
            }
        }
    });

    Ok(Response::new(empty_body()))
}

async fn handle_http_passthrough(
    req: Request<Incoming>,
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let uri_str = req.uri().to_string();

    let cookie = req.headers().get("cookie").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let referrer = req.headers().get("referer").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();

    let host = match req.uri().host() {
        Some(h) => h.to_string(),
        None => {
            let mut resp = Response::new(empty_body());
            *resp.status_mut() = StatusCode::BAD_REQUEST;
            return Ok(resp);
        }
    };

    let port = req.uri().port_u16().unwrap_or(80);
    let addr = format!("{}:{}", host, port);

    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Proxy Forward Error] Connection failed to {}: {}", addr, e);
            let mut resp = Response::new(empty_body());
            *resp.status_mut() = StatusCode::BAD_GATEWAY;
            return Ok(resp);
        }
    };

    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;

    tokio::spawn(async move {
        if let Err(err) = conn.await {
            eprintln!("[Proxy Client Connection Error] {}", err);
        }
    });

    let res = sender.send_request(req).await?;

    // Phase 3: Interception & Handoff
    if is_download_response(&uri_str, res.headers()) {
        let cl = res.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let filename = get_best_filename(&uri_str, res.headers());

        println!("[Proxy Handoff] 🚀 INTERCEPTED DOWNLOAD & HANDING OFF TO IDM ENGINE!");
        println!("  -> URL: {}", uri_str);
        println!("  -> Filename: {}", filename);
        println!("  -> Size: {}", cl);

        // Send to Tauri Engine & open popup window
        let app_handle_guard = app_handle.lock().await;
        if let Some(ref handle) = *app_handle_guard {
            let add_url = format!(
                "index.html#popup=add&url={}&filename={}&cookie={}&referrer={}&size={}",
                urlencoding::encode(&uri_str),
                urlencoding::encode(&filename),
                urlencoding::encode(&cookie),
                urlencoding::encode(&referrer),
                cl
            );

            let _ = tauri::WebviewWindowBuilder::new(
                handle,
                "popup-add",
                tauri::WebviewUrl::App(add_url.into()),
            )
            .title("New Download Captured")
            .inner_size(520.0, 370.0)
            .center()
            .build();
        }

        // Return clean fake response to browser so it cancels native download
        return Ok(download_captured_response());
    }

    Ok(res.map(|b| b.boxed()))
}

fn download_captured_response() -> Response<BoxBody<Bytes, hyper::Error>> {
    let html = r#"<!DOCTYPE html>
<html>
<head><title>Download Intercepted</title></head>
<body style="font-family: system-ui, sans-serif; text-align: center; padding-top: 60px; background-color: #0f172a; color: #f8fafc;">
  <h2 style="color: #38bdf8;">🚀 Download Handed Off to Impressive Download Manager</h2>
  <p style="color: #94a3b8;">Your download has been intercepted at network level and passed to IDM.</p>
</body>
</html>"#;

    let mut resp = Response::new(
        Full::new(Bytes::from(html))
            .map_err(|never| match never {})
            .boxed()
    );
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    resp
}

fn sanitize_filename(name: &str) -> String {
    let mut clean = name.trim().trim_matches('"').trim_matches('\'').to_string();
    for invalid in ['/', '\\', ':', '*', '?', '"', '<', '>', '|'] {
        clean = clean.replace(invalid, "_");
    }
    clean.trim().to_string()
}

fn parse_content_disposition(value: &str) -> Option<String> {
    if value.is_empty() { return None; }
    
    // Pass 1: filename*= (RFC 5987)
    for part in value.split(';') {
        let part = part.trim();
        if part.to_lowercase().starts_with("filename*=") {
            if let Some(val) = part.split('=').nth(1) {
                let clean = val.trim_matches('"').trim();
                let encoded_str = if let Some(idx) = clean.rfind("''") {
                    &clean[idx + 2..]
                } else {
                    clean
                };
                if let Ok(decoded) = urlencoding::decode(encoded_str) {
                    let sanitized = sanitize_filename(&decoded);
                    if !sanitized.is_empty() {
                        return Some(sanitized);
                    }
                }
            }
        }
    }

    // Pass 2: filename=
    for part in value.split(';') {
        let part = part.trim();
        if part.to_lowercase().starts_with("filename=") {
            if let Some(val) = part.split('=').nth(1) {
                let clean = val.trim_matches('"').trim();
                if let Ok(decoded) = urlencoding::decode(clean) {
                    let sanitized = sanitize_filename(&decoded);
                    if !sanitized.is_empty() {
                        return Some(sanitized);
                    }
                }
            }
        }
    }

    None
}

fn is_generic_filename(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "captured_download"
        || lower == "download"
        || lower == "download.php"
        || lower == "file.php"
        || lower == "index.php"
        || lower == "index.cgi"
        || lower == "get_file.php"
        || lower == "attachment.php"
}

fn extension_from_content_type(ct: &str) -> Option<&'static str> {
    let lower = ct.to_lowercase();
    if lower.contains("application/pdf") { Some(".pdf") }
    else if lower.contains("application/zip") || lower.contains("application/x-zip-compressed") { Some(".zip") }
    else if lower.contains("application/x-rar-compressed") { Some(".rar") }
    else if lower.contains("application/x-7z-compressed") { Some(".7z") }
    else if lower.contains("application/x-tar") { Some(".tar") }
    else if lower.contains("application/gzip") { Some(".tar.gz") }
    else if lower.contains("video/mp4") { Some(".mp4") }
    else if lower.contains("video/x-matroska") { Some(".mkv") }
    else if lower.contains("video/webm") { Some(".webm") }
    else if lower.contains("audio/mpeg") { Some(".mp3") }
    else if lower.contains("audio/wav") { Some(".wav") }
    else if lower.contains("image/jpeg") { Some(".jpg") }
    else if lower.contains("image/png") { Some(".png") }
    else if lower.contains("image/gif") { Some(".gif") }
    else if lower.contains("image/webp") { Some(".webp") }
    else { None }
}

fn get_best_filename(uri_str: &str, headers: &hyper::HeaderMap) -> String {
    let cd = headers.get("content-disposition").and_then(|v| v.to_str().ok()).unwrap_or("");
    let ct = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");

    if let Some(name) = parse_content_disposition(cd) {
        if !name.is_empty() && !is_generic_filename(&name) {
            return name;
        }
    }

    let mut candidate = String::new();
    if let Ok(parsed) = reqwest::Url::parse(uri_str) {
        // Query param check
        for (k, v) in parsed.query_pairs() {
            let k_lower = k.to_lowercase();
            if (k_lower.contains("file") || k_lower.contains("name") || k_lower.contains("title")) && v.contains('.') {
                if let Ok(decoded) = urlencoding::decode(&v) {
                    let sanitized = sanitize_filename(&decoded);
                    if !sanitized.is_empty() && !is_generic_filename(&sanitized) {
                        candidate = sanitized;
                        break;
                    }
                }
            }
        }

        if candidate.is_empty() {
            if let Some(last) = parsed.path_segments().and_then(|s| s.last()) {
                if !last.is_empty() {
                    if let Ok(decoded) = urlencoding::decode(last) {
                        candidate = sanitize_filename(&decoded);
                    }
                }
            }
        }
    }

    if candidate.is_empty() || is_generic_filename(&candidate) {
        candidate = "download".to_string();
    }

    if !candidate.contains('.') || candidate.ends_with(".php") || candidate.ends_with(".cgi") || candidate.ends_with(".aspx") || candidate.ends_with(".ashx") {
        if let Some(ext) = extension_from_content_type(ct) {
            if candidate.contains('.') {
                if let Some(dot_idx) = candidate.rfind('.') {
                    candidate.truncate(dot_idx);
                }
            }
            candidate.push_str(ext);
        }
    }

    candidate
}

pub fn is_download_response(url_str: &str, headers: &hyper::HeaderMap) -> bool {
    // 0. STRICT REJECTION: HTML / Plain Text responses are NEVER file downloads
    if let Some(ct) = headers.get(hyper::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        let ct_lower = ct.to_lowercase();
        if ct_lower.contains("text/html") || ct_lower.contains("text/plain") {
            return false;
        }
    }

    // 1. Check Content-Disposition: attachment or filename=
    if let Some(cd) = headers.get(hyper::header::CONTENT_DISPOSITION).and_then(|v| v.to_str().ok()) {
        let cd_lower = cd.to_lowercase();
        if cd_lower.contains("attachment") || cd_lower.contains("filename=") {
            return true;
        }
    }

    // 2. Check Content-Type against known binary types
    if let Some(ct) = headers.get(hyper::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        let ct_lower = ct.to_lowercase();
        if ct_lower.contains("application/octet-stream")
            || ct_lower.contains("application/zip")
            || ct_lower.contains("application/x-msdownload")
            || ct_lower.contains("application/x-tar")
            || ct_lower.contains("application/gzip")
            || ct_lower.contains("application/x-debian-package")
            || ct_lower.contains("application/x-redhat-package-manager")
            || ct_lower.contains("application/x-apple-diskimage")
            || ct_lower.contains("application/x-iso9660-image")
            || ct_lower.contains("application/vnd.microsoft.portable-executable")
        {
            return true;
        }
    }

    // 3. Check File Extension in URL (.exe, .zip, .run, .deb, .msi, .iso, .dmg, .tar.gz, etc.)
    let path = match reqwest::Url::parse(url_str) {
        Ok(parsed) => parsed.path().to_lowercase(),
        Err(_) => url_str.to_lowercase(),
    };

    let ext_match = [
        ".exe", ".zip", ".run", ".deb", ".msi", ".iso", ".dmg", ".tar.gz",
        ".tgz", ".rar", ".7z", ".gz", ".bin", ".appimage"
    ].iter().any(|ext| path.ends_with(ext));

    if ext_match {
        return true;
    }

    // 4. Check Content-Length threshold > 5MB (5,242,880 bytes)
    if let Some(cl) = headers.get(hyper::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
    {
        if cl > 5 * 1024 * 1024 {
            if let Some(ct) = headers.get(hyper::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
                let ct_lower = ct.to_lowercase();
                if !ct_lower.contains("text/html")
                    && !ct_lower.contains("text/css")
                    && !ct_lower.contains("javascript")
                    && !ct_lower.contains("application/json")
                {
                    return true;
                }
            } else {
                return true;
            }
        }
    }

    false
}

fn empty_body() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}
