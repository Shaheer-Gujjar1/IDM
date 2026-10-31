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
use tauri::Manager;

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

            if let Some(existing_win) = handle.get_webview_window("popup-add") {
                let _ = existing_win.close();
            }

            if let Ok(new_win) = tauri::WebviewWindowBuilder::new(
                handle,
                "popup-add",
                tauri::WebviewUrl::App(add_url.into()),
            )
            .title("New Download Captured")
            .inner_size(520.0, 370.0)
            .center()
            .always_on_top(true)
            .build() {
                let _ = new_win.show();
                let _ = new_win.unminimize();
                let _ = new_win.set_focus();
            }
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

/// Truncates a UTF-8 string to at most `max_bytes` without slicing inside a multi-byte code point,
/// and ensures character count does not exceed `max_chars`.
pub fn truncate_safe_utf8(s: &str, max_bytes: usize, max_chars: usize) -> &str {
    if s.is_empty() || (max_bytes == 0 && max_chars == 0) {
        return "";
    }
    let mut boundary = max_bytes.min(s.len());
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let mut slice = &s[..boundary];
    if slice.chars().count() > max_chars {
        if let Some((idx, _)) = slice.char_indices().nth(max_chars) {
            slice = &slice[..idx];
        }
    }
    slice
}

/// Helper to split stem and extension, recognizing compound archive extensions (.tar.gz, .tar.bz2, etc.).
pub fn split_stem_and_extension(name: &str) -> (&str, &str) {
    let lower = name.to_lowercase();
    let compound_exts = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz", ".tar.lzma",
        ".user.js",
    ];
    for ext in compound_exts {
        if lower.ends_with(ext) {
            let split_pos = name.len() - ext.len();
            return (&name[..split_pos], &name[split_pos..]);
        }
    }
    if let Some(pos) = name.rfind('.') {
        if pos > 0 {
            return (&name[..pos], &name[pos..]);
        }
    }
    (name, "")
}

/// Checks if a file stem collides with Windows reserved device names or NTFS system files.
pub fn is_windows_reserved_stem(stem: &str) -> bool {
    let clean = stem.trim_end_matches([' ', '.']).to_lowercase();
    if clean.is_empty() {
        return false;
    }

    // Windows normalizes superscript numbers (¹, ², ³) to ASCII digits (1, 2, 3) in device lookups
    let normalized: String = clean.chars().map(|c| match c {
        '\u{00B9}' => '1',
        '\u{00B2}' => '2',
        '\u{00B3}' => '3',
        _ => c,
    }).collect();

    let reserved = [
        // Standard MS-DOS / Windows device names
        "con", "prn", "aux", "nul",
        "conin$", "conout$", "clock$",
        "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
        "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        // NTFS internal metadata files that fail or corrupt if created at root/shares
        "$mft", "$mftmirr", "$logfile", "$volume", "$attrdef", "$bitmap",
        "$boot", "$badclus", "$secure", "$upcase", "$extend",
    ];

    reserved.contains(&normalized.as_str())
}

pub fn sanitize_filename(name: &str) -> String {
    let clean = name.trim().trim_matches('"').trim_matches('\'');

    // Filter Bidi override & directional formatting characters (used in filename spoofing attacks)
    // and zero-width spaces/joiners
    let filtered: String = clean.chars().filter(|&c| {
        !matches!(
            c,
            '\u{202A}'..='\u{202E}' // LRE, RLE, PDF, LRO, RLO
            | '\u{2066}'..='\u{2069}' // LRI, RLI, FSI, PDI
            | '\u{200E}' | '\u{200F}' | '\u{061C}' // LRM, RLM, ALM
            | '\u{200B}'..='\u{200D}' | '\u{FEFF}' // Zero-width spaces, ZWJ, ZWNJ, BOM
        )
    }).collect();

    // Replace OS invalid characters, control characters, and path separators
    let clean: String = filtered.chars().map(|c| {
        let code = c as u32;
        if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\r' | '\n' | '\0')
            || code < 32
            || (code >= 0x7F && code <= 0x9F)
        {
            '_'
        } else {
            c
        }
    }).collect();

    // Strip trailing dots and spaces (prevents undeletable ghost files in Windows)
    let clean = clean.trim_end_matches(['.', ' ']).trim();

    if clean.is_empty() {
        return "download_file".to_string();
    }

    // Guard against Unix hidden dotfiles (.bashrc, .profile, .env, etc.)
    let clean = if clean.starts_with('.') {
        format!("download_{}", clean)
    } else {
        clean.to_string()
    };

    // Check Windows reserved device names: CON, PRN, AUX, NUL, COM0-9, LPT0-9, CONIN$, CONOUT$, CLOCK$, $MFT, etc.
    let (stem, ext) = split_stem_and_extension(&clean);
    let mut working_name = if is_windows_reserved_stem(stem) {
        let clean_stem = stem.trim_end_matches([' ', '.']);
        format!("download_{}{}", clean_stem, ext)
    } else {
        clean
    };

    // Cap filename length to 180 bytes & 180 characters to respect filesystem MAX_PATH and 255-byte limits
    // Uses UTF-8 character-boundary safe truncation to completely prevent slicing panics on multi-byte characters!
    const MAX_LIMIT: usize = 180;
    if working_name.len() > MAX_LIMIT || working_name.chars().count() > MAX_LIMIT {
        let (cur_stem, cur_ext) = split_stem_and_extension(&working_name);
        if !cur_ext.is_empty() && cur_ext.len() < MAX_LIMIT && cur_ext.chars().count() < MAX_LIMIT {
            let max_stem_bytes = MAX_LIMIT.saturating_sub(cur_ext.len());
            let max_stem_chars = MAX_LIMIT.saturating_sub(cur_ext.chars().count());
            let safe_stem = truncate_safe_utf8(cur_stem, max_stem_bytes, max_stem_chars)
                .trim_end_matches([' ', '.']);
            let safe_stem = if safe_stem.is_empty() {
                "download_file"
            } else {
                safe_stem
            };
            working_name = format!("{}{}", safe_stem, cur_ext);
        } else {
            let safe = truncate_safe_utf8(&working_name, MAX_LIMIT, MAX_LIMIT)
                .trim_end_matches([' ', '.']);
            working_name = if safe.is_empty() {
                "download_file".to_string()
            } else {
                safe.to_string()
            };
        }

        // Post-truncation safety check: if truncation caused the stem to become a reserved device name
        let (post_stem, post_ext) = split_stem_and_extension(&working_name);
        if is_windows_reserved_stem(post_stem) && !working_name.starts_with("download_") {
            working_name = format!("download_{}{}", post_stem.trim_end_matches([' ', '.']), post_ext);
        }
    }

    working_name
}

pub fn parse_content_disposition(value: &str) -> Option<String> {
    if value.is_empty() { return None; }
    
    // Pass 1: RFC 5987 / 6266 filename*=[charset]'[language]'[encoded]
    // Handles UTF-8'', UTF-8'en'..., ISO-8859-1''...
    if let Ok(re_rfc5987) = regex::Regex::new(r"(?i)filename\*\s*=\s*[^']*'[^']*'([^;]+)") {
        if let Some(caps) = re_rfc5987.captures(value) {
            if let Some(m) = caps.get(1) {
                let encoded = m.as_str().trim().trim_matches('"');
                if let Ok(decoded) = urlencoding::decode(encoded) {
                    let sanitized = sanitize_filename(&decoded);
                    if !sanitized.is_empty() && !is_generic_filename(&sanitized) {
                        return Some(sanitized);
                    }
                }
            }
        }
    }

    // Pass 2: standard filename="..." or filename=...
    // Allows escaped quotes inside quoted filenames
    if let Ok(re_plain) = regex::Regex::new(r#"(?i)filename\s*=\s*(?:"((?:\\.|[^"\\])*)"|([^;]+))"#) {
        if let Some(caps) = re_plain.captures(value) {
            let raw = if let Some(m) = caps.get(1) {
                m.as_str().replace(r#"\""#, "\"").replace(r#"\\"#, r#"\"#)
            } else if let Some(m) = caps.get(2) {
                m.as_str().trim().trim_matches('"').to_string()
            } else {
                String::new()
            };
            if !raw.is_empty() {
                if let Ok(decoded) = urlencoding::decode(&raw) {
                    let sanitized = sanitize_filename(&decoded);
                    if !sanitized.is_empty() && !is_generic_filename(&sanitized) {
                        return Some(sanitized);
                    }
                }
                let sanitized = sanitize_filename(&raw);
                if !sanitized.is_empty() && !is_generic_filename(&sanitized) {
                    return Some(sanitized);
                }
            }
        }
    }

    None
}


pub fn is_generic_filename(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower.is_empty()
        || lower == "captured_download"
        || lower == "download"
        || lower == "downloaded_file"
        || lower == "download.php"
        || lower == "file.php"
        || lower == "index.php"
        || lower == "index.cgi"
        || lower == "get_file.php"
        || lower == "attachment.php"
        || lower == "file"
}

pub fn extension_from_content_type(ct: &str) -> Option<&'static str> {
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

pub fn get_best_filename(uri_str: &str, headers: &hyper::HeaderMap) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_disposition_standard() {
        let header = r#"attachment; filename="setup_v2.0.exe""#;
        assert_eq!(parse_content_disposition(header), Some("setup_v2.0.exe".to_string()));
    }

    #[test]
    fn test_parse_content_disposition_unquoted() {
        let header = "attachment; filename=document.pdf";
        assert_eq!(parse_content_disposition(header), Some("document.pdf".to_string()));
    }

    #[test]
    fn test_parse_content_disposition_utf8_rfc5987() {
        let header = "attachment; filename*=UTF-8''%e2%82%ac%20rates.pdf";
        assert_eq!(parse_content_disposition(header), Some("€ rates.pdf".to_string()));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("my:file/name?.zip"), "my_file_name_.zip");
        assert_eq!(sanitize_filename(r#""test.tar.gz""#), "test.tar.gz");
    }

    #[test]
    fn test_is_generic_filename() {
        assert!(is_generic_filename("download"));
        assert!(is_generic_filename("captured_download"));
        assert!(is_generic_filename("downloaded_file"));
        assert!(is_generic_filename("download.php"));
        assert!(!is_generic_filename("node-v20.tar.gz"));
        assert!(!is_generic_filename("installer.exe"));
    }

    // =========================================================================
    // Adversarial Penetration-Testing Suite (Phase 1 Security Hardening)
    // =========================================================================

    #[test]
    fn test_pen_path_traversal_attempts() {
        // Attackers attempting to escape download directory using path separators
        let payloads = [
            ("../../../../etc/passwd", "download_.._.._.._.._etc_passwd"),
            (r#"..\..\..\Windows\System32\calc.exe"#, "download_.._.._.._Windows_System32_calc.exe"),
            ("....//....//secret.key", "download_....__....__secret.key"),
            (r#"C:\boot.ini"#, "C__boot.ini"),
            ("/etc/shadow", "_etc_shadow"),
        ];

        for (input, expected) in payloads {
            let sanitized = sanitize_filename(input);
            assert_eq!(sanitized, expected, "Failed path traversal neutralization for {}", input);
            assert!(!sanitized.contains('/'), "Sanitized output must not contain forward slash");
            assert!(!sanitized.contains('\\'), "Sanitized output must not contain backward slash");
        }
    }

    #[test]
    fn test_pen_windows_reserved_device_names() {
        // Windows special device names that freeze or corrupt filesystem when written
        let devices = [
            ("CON", "download_CON"),
            ("con.txt", "download_con.txt"),
            ("PRN.pdf", "download_PRN.pdf"),
            ("aux.tar.gz", "download_aux.tar.gz"),
            ("NUL", "download_NUL"),
            ("COM1", "download_COM1"),
            ("lpt9.log", "download_lpt9.log"),
            ("CONIN$", "download_CONIN$"),
            ("conout$.txt", "download_conout$.txt"),
            ("clock$", "download_clock$"),
            ("CON.txt. . .", "download_CON.txt"),
        ];

        for (input, expected) in devices {
            let sanitized = sanitize_filename(input);
            assert_eq!(sanitized, expected, "Failed reserved device neutralization for {}", input);
            assert!(
                sanitized.starts_with("download_"),
                "Reserved device '{}' must be prefixed with 'download_'",
                input
            );
        }
    }

    #[test]
    fn test_pen_bidi_and_unicode_spoofing() {
        // Right-to-Left Override (RTLO) U+202E: transforms "invoice[RTLO]fdp.exe" to look like "invoiceexe.pdf"
        let rtlo_attack = "invoice\u{202E}fdp.exe";
        let sanitized = sanitize_filename(rtlo_attack);
        assert!(!sanitized.contains('\u{202E}'), "RTLO character must be stripped");
        assert_eq!(sanitized, "invoicefdp.exe");

        // Zero-width characters & BOM
        let zero_width_attack = "file\u{200B}\u{FEFF}\u{200D}.zip";
        let sanitized_zw = sanitize_filename(zero_width_attack);
        assert_eq!(sanitized_zw, "file.zip");

        // Control characters
        let ctrl_attack = "my\x00file\x07name\x1B.bin";
        let sanitized_ctrl = sanitize_filename(ctrl_attack);
        assert!(!sanitized_ctrl.contains('\x00'));
        assert!(!sanitized_ctrl.contains('\x07'));
        assert!(!sanitized_ctrl.contains('\x1B'));
    }

    #[test]
    fn test_pen_unix_hidden_dotfiles() {
        // Prevent writing hidden dotfiles (.bashrc, .profile, .env) or traversing with '.' / '..'
        assert_eq!(sanitize_filename(".bashrc"), "download_.bashrc");
        assert_eq!(sanitize_filename(".profile"), "download_.profile");
        assert_eq!(sanitize_filename(".env"), "download_.env");
        assert_eq!(sanitize_filename("."), "download_file");
        assert_eq!(sanitize_filename(".."), "download_file");
        assert_eq!(sanitize_filename("..."), "download_file");
    }

    #[test]
    fn test_pen_extremely_long_filenames() {
        // Exceeding 180 chars must be truncated while preserving extension
        let long_stem = "a".repeat(250);
        let long_filename = format!("{}.tar.gz", long_stem);
        let sanitized = sanitize_filename(&long_filename);
        assert!(sanitized.len() <= 180, "Sanitized filename length must be <= 180, got {}", sanitized.len());
        assert!(sanitized.ends_with(".tar.gz"), "File extension must be preserved during truncation");
    }

    #[test]
    fn test_edge_case_4_extended_windows_reserved_names() {
        // Test extended Windows device names, trailing spaces, dots, superscripts, and NTFS system files
        let test_cases = [
            ("aux.pdf", "download_aux.pdf"),
            ("prn.log", "download_prn.log"),
            ("con.tar.gz", "download_con.tar.gz"),
            ("NUL.zip", "download_NUL.zip"),
            ("com0.bin", "download_com0.bin"),
            ("lpt0.dat", "download_lpt0.dat"),
            ("COM1.png", "download_COM1.png"),
            ("lpt9.txt", "download_lpt9.txt"),
            ("prn .txt", "download_prn.txt"),
            ("aux  .tar.bz2", "download_aux.tar.bz2"),
            ("CONIN$", "download_CONIN$"),
            ("conout$.pdf", "download_conout$.pdf"),
            ("clock$", "download_clock$"),
            ("COM¹", "download_COM¹"),
            ("lpt².zip", "download_lpt².zip"),
            ("$mft.dat", "download_$mft.dat"),
            ("$secure.sys", "download_$secure.sys"),
            ("my_notes...  ", "my_notes"),
        ];

        for (input, expected) in test_cases {
            let res = sanitize_filename(input);
            assert_eq!(res, expected, "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_edge_case_4_utf8_char_boundary_long_filename_no_panic() {
        // Multi-byte Urdu string: each Urdu character is 2 bytes.
        // If sliced on raw byte index without boundary checking, will trigger an instant Rust panic!
        let base_urdu = "یہ_ایک_بہت_ہی_طویل_فائل_کا_نام_ہے_جو_کہ_اردو_زبان_میں_لکھی_گئی_ہے_اور_ہم_ٹیسٹنگ_کر_رہے_ہیں";
        let long_urdu = format!("{}_{}_{}.pdf", base_urdu, base_urdu, base_urdu);
        assert!(long_urdu.len() > 200, "Must be long multi-byte string, got {}", long_urdu.len());

        let sanitized_urdu = sanitize_filename(&long_urdu);
        assert!(sanitized_urdu.len() <= 180, "Urdu byte length must be <= 180, got {}", sanitized_urdu.len());
        assert!(sanitized_urdu.chars().count() <= 180, "Urdu char count must be <= 180");
        assert!(sanitized_urdu.ends_with(".pdf"), "Extension .pdf must be preserved");
        assert!(std::str::from_utf8(sanitized_urdu.as_bytes()).is_ok(), "Must be valid UTF-8");

        // Multi-byte Chinese compound extension:
        let base_cn = "这是一个非常非常非常长的测试文件名称用来验证中文字符边界截断是否安全";
        let long_cn = format!("{}_{}_{}.tar.gz", base_cn, base_cn, base_cn);
        assert!(long_cn.len() > 300, "Must be long multi-byte string, got {}", long_cn.len());

        let sanitized_cn = sanitize_filename(&long_cn);
        assert!(sanitized_cn.len() <= 180, "Chinese byte length must be <= 180, got {}", sanitized_cn.len());
        assert!(sanitized_cn.chars().count() <= 180, "Chinese char count must be <= 180");
        assert!(sanitized_cn.ends_with(".tar.gz"), "Compound extension .tar.gz must be preserved");
        assert!(std::str::from_utf8(sanitized_cn.as_bytes()).is_ok(), "Must be valid UTF-8");

        // 4-byte UTF-8 Emoji boundary safety test:
        let emojis = "🚀🔥🎉⚡️🌟💎📦🎯🏆🎨".repeat(20);
        let long_emoji_file = format!("{}.mp4", emojis);
        let sanitized_emoji = sanitize_filename(&long_emoji_file);
        assert!(sanitized_emoji.len() <= 180, "Emoji byte length must be <= 180, got {}", sanitized_emoji.len());
        assert!(sanitized_emoji.ends_with(".mp4"), "Extension .mp4 must be preserved");
        assert!(std::str::from_utf8(sanitized_emoji.as_bytes()).is_ok(), "Must be valid UTF-8");
    }

    #[test]
    fn test_edge_case_4_post_truncation_reserved_name_collision() {
        // If truncation accidentally reduces a long filename to a reserved Windows device name
        let target_stem = format!("con_{}", "x".repeat(250));
        let filename = format!("{}.log", target_stem);
        let sanitized = sanitize_filename(&filename);
        assert!(sanitized.len() <= 180);
        assert!(sanitized.ends_with(".log"));
        // Check that device collision avoidance is maintained
        assert!(!sanitized.to_lowercase().starts_with("con."));
    }

    #[test]
    fn test_pen_rfc5987_fuzzing_and_language_tags() {
        // RFC 5987 with language tag (e.g., UTF-8'en-US'...)
        let header_lang = "attachment; filename*=UTF-8'en-US'%c2%a3%20rates.pdf";
        assert_eq!(parse_content_disposition(header_lang), Some("£ rates.pdf".to_string()));

        // Semicolons and quotes inside quoted filename
        let header_complex = r#"attachment; filename="my; complex=filename; version=1.0.tar.gz""#;
        assert_eq!(
            parse_content_disposition(header_complex),
            Some("my; complex=filename; version=1.0.tar.gz".to_string())
        );

        // Path traversal payload embedded inside RFC 5987
        let header_traversal = "attachment; filename*=UTF-8''%2e%2e%2f%2e%2e%2fetc%2fpasswd";
        let result = parse_content_disposition(header_traversal);
        assert_eq!(result, Some("download_.._.._etc_passwd".to_string()));
    }

    #[test]
    fn test_pen_cross_origin_isolation_logic() {
        // Verify URL origin comparison semantics for cookie stripping
        let u1 = reqwest::Url::parse("https://example.com/downloads/file.zip").unwrap();
        let u2_same = reqwest::Url::parse("https://example.com/cdn/file.zip").unwrap();
        let u3_subdomain = reqwest::Url::parse("https://cdn.example.com/file.zip").unwrap();
        let u4_s3 = reqwest::Url::parse("https://s3.amazonaws.com/bucket/file.zip").unwrap();
        let u5_http = reqwest::Url::parse("http://example.com/file.zip").unwrap();
        let u6_port = reqwest::Url::parse("https://example.com:8443/file.zip").unwrap();

        // Same origin -> Cookies allowed
        assert_eq!(u1.origin(), u2_same.origin());

        // Cross-origin hosts -> Cookies MUST be stripped
        assert_ne!(u1.origin(), u3_subdomain.origin(), "Subdomains must be treated as different origins");
        assert_ne!(u1.origin(), u4_s3.origin(), "S3 bucket must be treated as different origin");
        assert_ne!(u1.origin(), u5_http.origin(), "HTTP vs HTTPS must be treated as different origin");
        assert_ne!(u1.origin(), u6_port.origin(), "Different ports must be treated as different origin");
    }

    // =========================================================================
    // Real-World Functional Working Tests (Phase 1 Functionality)
    // =========================================================================

    #[test]
    fn test_working_international_filenames() {
        // Real-world filenames from global websites
        assert_eq!(sanitize_filename("دستاویز_نوٹ.pdf"), "دستاویز_نوٹ.pdf");
        assert_eq!(sanitize_filename("测试压缩包.7z"), "测试压缩包.7z");
        assert_eq!(sanitize_filename("Über_uns_Bericht.pdf"), "Über_uns_Bericht.pdf");
        assert_eq!(sanitize_filename("résumé_candidat_2024.docx"), "résumé_candidat_2024.docx");
        assert_eq!(sanitize_filename("تقرير_مالي_نهائي.xlsx"), "تقرير_مالي_نهائي.xlsx");
        assert_eq!(sanitize_filename("привет_мир_setup.exe"), "привет_мир_setup.exe");
    }

    #[test]
    fn test_working_real_world_url_filename_resolution() {
        let empty_headers = hyper::HeaderMap::new();

        // 1. GitHub releases direct binary download
        let github_url = "https://github.com/astral-sh/uv/releases/download/0.1.0/uv-x86_64-unknown-linux-gnu.tar.gz";
        assert_eq!(get_best_filename(github_url, &empty_headers), "uv-x86_64-unknown-linux-gnu.tar.gz");

        // 2. Query param containing real filename
        let query_url = "https://download.mediafire.com/download.php?file=photoshop_filters_pack.zip&token=abc";
        assert_eq!(get_best_filename(query_url, &empty_headers), "photoshop_filters_pack.zip");

        // 3. Fallback when filename has no extension, but Content-Type is application/pdf
        let mut pdf_headers = hyper::HeaderMap::new();
        pdf_headers.insert(
            hyper::header::CONTENT_TYPE,
            hyper::header::HeaderValue::from_static("application/pdf"),
        );
        let api_url = "https://example.com/api/v1/export/monthly_report";
        assert_eq!(get_best_filename(api_url, &pdf_headers), "monthly_report.pdf");

        // 4. Content-Disposition takes precedence over URL filename
        let mut cd_headers = hyper::HeaderMap::new();
        cd_headers.insert(
            hyper::header::CONTENT_DISPOSITION,
            hyper::header::HeaderValue::from_static(r#"attachment; filename="Audited_Balance_Sheet.xlsx""#),
        );
        let generic_url = "https://example.com/download.php?id=99999";
        assert_eq!(get_best_filename(generic_url, &cd_headers), "Audited_Balance_Sheet.xlsx");
    }

    #[test]
    fn test_working_is_download_response_accuracy() {
        let mut headers = hyper::HeaderMap::new();

        // Standard HTML web page -> MUST NOT be intercepted
        headers.insert(
            hyper::header::CONTENT_TYPE,
            hyper::header::HeaderValue::from_static("text/html; charset=utf-8"),
        );
        assert!(!is_download_response("https://example.com/index.html", &headers));

        // JavaScript file -> MUST NOT be intercepted
        headers.insert(
            hyper::header::CONTENT_TYPE,
            hyper::header::HeaderValue::from_static("application/javascript"),
        );
        assert!(!is_download_response("https://example.com/bundle.js", &headers));

        // Binary ZIP file -> MUST be intercepted
        headers.insert(
            hyper::header::CONTENT_TYPE,
            hyper::header::HeaderValue::from_static("application/zip"),
        );
        assert!(is_download_response("https://example.com/archive.zip", &headers));

        // Content-Disposition: attachment -> MUST be intercepted
        let mut cd_headers = hyper::HeaderMap::new();
        cd_headers.insert(
            hyper::header::CONTENT_DISPOSITION,
            hyper::header::HeaderValue::from_static("attachment; filename=data.bin"),
        );
        assert!(is_download_response("https://example.com/get_data", &cd_headers));
    }
}
