use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use serde::{Deserialize, Serialize};

pub const NATIVE_HOST_NAME: &str = "com.impressive.idm";
pub const FIREFOX_EXTENSION_ID: &str = "idm-extension@impressive.com";
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024; // 1 MB WebExtension safety bound

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePayload {
    pub url: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub mime: Option<String>,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub cookie: Option<String>,
    #[serde(default, alias = "userAgent")]
    pub user_agent: Option<String>,
    #[serde(default, alias = "audioUrl")]
    pub audio_url: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default, alias = "queueOnly")]
    pub queue_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserRegistrationItem {
    pub browser: String,
    pub manifest_path: String,
    pub installed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationSummary {
    pub success: bool,
    pub registered_browsers: Vec<BrowserRegistrationItem>,
    pub detected_extension_ids: Vec<String>,
    pub binary_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub native_host_name: String,
    pub binary_path: String,
    pub browsers: Vec<BrowserRegistrationItem>,
}

/// Reads a 32-bit little-endian length-prefixed JSON message from the provided reader.
/// Complies strictly with the WebExtension Native Messaging protocol.
pub fn read_native_message<R: Read>(reader: &mut R) -> Result<serde_json::Value, std::io::Error> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let length = u32::from_le_bytes(len_bytes) as usize;

    if length == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Native message length cannot be 0",
        ));
    }

    if length > MAX_MESSAGE_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Native message exceeds limit of {} bytes: got {}", MAX_MESSAGE_SIZE, length),
        ));
    }

    let mut buf = vec![0u8; length];
    reader.read_exact(&mut buf)?;

    serde_json::from_slice::<serde_json::Value>(&buf).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse native JSON: {}", e),
        )
    })
}

/// Writes a 32-bit little-endian length-prefixed JSON message to the provided writer and flushes.
pub fn write_native_message<W: Write>(
    writer: &mut W,
    value: &serde_json::Value,
) -> Result<(), std::io::Error> {
    let payload = serde_json::to_vec(value).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to serialize native JSON: {}", e),
        )
    })?;

    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Response exceeds 1MB limit",
        ));
    }

    let len_bytes = (payload.len() as u32).to_le_bytes();
    writer.write_all(&len_bytes)?;
    writer.write_all(&payload)?;
    writer.flush()
}

/// Validates and sanitizes native download payloads against malicious inputs,
/// CRLF injection, credential leakage, and path traversal attacks.
pub fn sanitize_native_payload(value: &serde_json::Value) -> Result<NativePayload, String> {
    if !value.is_object() {
        return Err("Native message must be a JSON object".to_string());
    }

    let raw_url = value
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required 'url' field".to_string())?
        .trim();

    if raw_url.len() > 16384 {
        return Err("URL exceeds maximum length of 16384 characters".to_string());
    }

    let parsed_url = reqwest::Url::parse(raw_url).map_err(|e| format!("Invalid download URL: {}", e))?;
    let scheme = parsed_url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("Unsupported protocol '{}'. Only HTTP/HTTPS is allowed.", scheme));
    }

    if !parsed_url.username().is_empty() || parsed_url.password().is_some() {
        return Err("URLs containing embedded username/passwords are prohibited".to_string());
    }

    let clean_header = |val: Option<&serde_json::Value>| -> Option<String> {
        val.and_then(|v| v.as_str()).and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() || trimmed.contains('\r') || trimmed.contains('\n') || trimmed.contains('\0') {
                None
            } else if trimmed.len() > 16384 {
                Some(trimmed[..16384].to_string())
            } else {
                Some(trimmed.to_string())
            }
        })
    };

    let filename = value
        .get("filename")
        .and_then(|v| v.as_str())
        .map(|s| crate::proxy::sanitize_filename(s));

    let audio_url = value
        .get("audio_url")
        .or_else(|| value.get("audioUrl"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .and_then(|s| {
            if let Ok(u) = reqwest::Url::parse(s) {
                if (u.scheme() == "http" || u.scheme() == "https") && u.username().is_empty() {
                    Some(s.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        });

    let size = value.get("size").and_then(|v| v.as_u64());
    let mime = clean_header(value.get("mime"));
    let referrer = clean_header(value.get("referrer"));
    let cookie = clean_header(value.get("cookie"));
    let user_agent = clean_header(value.get("userAgent").or_else(|| value.get("user_agent")));
    let category = clean_header(value.get("category"));
    let queue_only = value.get("queueOnly").or_else(|| value.get("queue_only")).and_then(|v| v.as_bool());

    Ok(NativePayload {
        url: parsed_url.to_string(),
        filename,
        size,
        mime,
        referrer,
        cookie,
        user_agent,
        audio_url,
        category,
        queue_only,
    })
}

/// Forwards a validated payload to the Impressive Download Manager desktop capture server on 127.0.0.1:9600.
/// If port 9600 is offline, this spawns the desktop binary in background and waits up to 8s.
pub fn forward_to_desktop(payload: &NativePayload) -> Result<serde_json::Value, String> {
    let check_port_open = || -> bool {
        std::net::TcpStream::connect_timeout(
            &"127.0.0.1:9600".parse().unwrap(),
            Duration::from_millis(300),
        )
        .is_ok()
    };

    if !check_port_open() {
        // App is closed; launch desktop binary
        if let Ok(exe_path) = std::env::current_exe() {
            eprintln!("[NativeHost] Launching desktop app at {:?}", exe_path);
            let mut cmd = std::process::Command::new(&exe_path);
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());

            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }

            let _ = cmd.spawn();
        }

        // Poll for up to 8 seconds (40 attempts * 200ms)
        let mut ready = false;
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(200));
            if check_port_open() {
                ready = true;
                break;
            }
        }

        if !ready {
            return Err("Could not connect to Impressive Download Manager daemon on port 9600.".to_string());
        }
    }

    // Deliver payload via HTTP POST to 127.0.0.1:9600/download
    let json_body = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let http_request = format!(
        "POST /download HTTP/1.1\r\n\
         Host: 127.0.0.1:9600\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n\
         {}",
        json_body.len(),
        json_body
    );

    let mut stream = std::net::TcpStream::connect_timeout(
        &"127.0.0.1:9600".parse().unwrap(),
        Duration::from_secs(3),
    )
    .map_err(|e| format!("Failed to connect to capture server: {}", e))?;

    stream
        .write_all(http_request.as_bytes())
        .map_err(|e| format!("Failed to send HTTP request: {}", e))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| format!("Failed to read capture server response: {}", e))?;

    let resp_str = String::from_utf8_lossy(&response);
    if resp_str.starts_with("HTTP/1.1 200") || resp_str.contains(" 200 OK") {
        Ok(serde_json::json!({
            "ok": true,
            "status": "accepted",
            "message": "Download received by Impressive Download Manager"
        }))
    } else if resp_str.contains("403 Forbidden") {
        Err("Download interception is disabled in Impressive Download Manager settings.".to_string())
    } else {
        Err(format!("Desktop rejected download: {}", resp_str.lines().next().unwrap_or("Unknown error")))
    }
}

/// The main entry point when the executable is launched in native messaging mode by Chrome/Firefox.
/// Binary protocol on stdin/stdout. Zero non-protocol characters on stdout.
pub fn run_native_host() {
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    loop {
        let msg = match read_native_message(&mut stdin) {
            Ok(v) => v,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    // Browser closed native messaging stream cleanly
                    break;
                }
                eprintln!("[NativeHost] Read error: {}", e);
                let err_resp = serde_json::json!({
                    "ok": false,
                    "error": format!("Native framing error: {}", e)
                });
                let _ = write_native_message(&mut stdout, &err_resp);
                break;
            }
        };

        let action = msg.get("action").and_then(|v| v.as_str()).unwrap_or("download");

        let response = match action {
            "ping" => serde_json::json!({
                "ok": true,
                "status": "pong",
                "name": NATIVE_HOST_NAME,
                "version": "0.8.0"
            }),
            "download" => {
                match sanitize_native_payload(&msg) {
                    Ok(payload) => match forward_to_desktop(&payload) {
                        Ok(res) => res,
                        Err(e) => serde_json::json!({ "ok": false, "error": e }),
                    },
                    Err(e) => serde_json::json!({ "ok": false, "error": e }),
                }
            }
            "sync_browsers" => {
                let custom_id = msg.get("extensionId").and_then(|v| v.as_str()).map(|s| s.to_string());
                match register_all_browser_manifests(custom_id) {
                    Ok(summary) => serde_json::json!({ "ok": true, "summary": summary }),
                    Err(e) => serde_json::json!({ "ok": false, "error": e }),
                }
            }
            other => serde_json::json!({
                "ok": false,
                "error": format!("Unsupported native action: {}", other)
            }),
        };

        if let Err(e) = write_native_message(&mut stdout, &response) {
            eprintln!("[NativeHost] Write error: {}", e);
            break;
        }
    }
}

/// Scans standard Chromium browser profiles for installed Impressive Download Manager extension IDs.
pub fn detect_installed_extension_ids() -> Vec<String> {
    let mut ids = Vec::new();

    #[cfg(target_os = "linux")]
    let base_paths = {
        let home = std::env::var("HOME").unwrap_or_default();
        let home_path = Path::new(&home);
        vec![
            home_path.join(".config").join("google-chrome"),
            home_path.join(".config").join("chromium"),
            home_path.join(".config").join("BraveSoftware").join("Brave-Browser"),
            home_path.join(".config").join("microsoft-edge"),
            home_path.join(".config").join("opera"),
            home_path.join(".config").join("vivaldi"),
        ]
    };

    #[cfg(target_os = "windows")]
    let base_paths = {
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let p = Path::new(&local_app_data);
        vec![
            p.join("Google").join("Chrome").join("User Data"),
            p.join("Microsoft").join("Edge").join("User Data"),
            p.join("BraveSoftware").join("Brave-Browser").join("User Data"),
            p.join("Opera Software").join("Opera Stable"),
            p.join("Vivaldi").join("User Data"),
        ]
    };

    #[cfg(target_os = "macos")]
    let base_paths = {
        let home = std::env::var("HOME").unwrap_or_default();
        let p = Path::new(&home).join("Library").join("Application Support");
        vec![
            p.join("Google").join("Chrome"),
            p.join("Microsoft Edge"),
            p.join("BraveSoftware").join("Brave-Browser"),
            p.join("Vivaldi"),
            p.join("com.operasoftware.Opera"),
        ]
    };

    for base in base_paths {
        if !base.exists() {
            continue;
        }
        // Profiles typically named 'Default', 'Profile 1', 'Profile 2', etc.
        let mut candidates = vec![base.join("Default")];
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("Profile ") {
                    candidates.push(entry.path());
                }
            }
        }

        for prof in candidates {
            let pref_file = prof.join("Preferences");
            if let Ok(content) = std::fs::read_to_string(&pref_file) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(settings) = json.get("extensions").and_then(|e| e.get("settings")).and_then(|s| s.as_object()) {
                        for (ext_id, val) in settings {
                            if ext_id.len() == 32 && ext_id.chars().all(|c| c.is_ascii_lowercase()) {
                                let mut is_match = false;
                                if let Some(path_str) = val.get("path").and_then(|p| p.as_str()) {
                                    if path_str.contains("browser-extension") || path_str.contains("impressive") {
                                        is_match = true;
                                    }
                                }
                                if let Some(manifest) = val.get("manifest").and_then(|m| m.as_object()) {
                                    if let Some(name) = manifest.get("name").and_then(|n| n.as_str()) {
                                        if name.contains("Impressive Download Manager") {
                                            is_match = true;
                                        }
                                    }
                                }
                                if is_match && !ids.contains(ext_id) {
                                    ids.push(ext_id.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    ids
}

/// Constructs a valid Native Messaging Host Manifest JSON compliant with RFC/Chromium/Firefox requirements.
pub fn generate_manifest(exe_path: &str, is_firefox: bool, allowed_ids: &[String]) -> serde_json::Value {
    if is_firefox {
        let mut extensions = vec![FIREFOX_EXTENSION_ID.to_string()];
        for id in allowed_ids {
            if id.contains('@') && !extensions.contains(id) {
                extensions.push(id.clone());
            }
        }
        serde_json::json!({
            "name": NATIVE_HOST_NAME,
            "description": "Impressive Download Manager Native Host",
            "path": exe_path,
            "type": "stdio",
            "allowed_extensions": extensions
        })
    } else {
        let mut origins = Vec::new();
        for id in allowed_ids {
            let clean = id.trim();
            if clean.len() == 32 && clean.chars().all(|c| c.is_ascii_lowercase()) {
                let origin = format!("chrome-extension://{}/", clean);
                if !origins.contains(&origin) {
                    origins.push(origin);
                }
            }
        }
        serde_json::json!({
            "name": NATIVE_HOST_NAME,
            "description": "Impressive Download Manager Native Host",
            "path": exe_path,
            "type": "stdio",
            "allowed_origins": origins
        })
    }
}

/// Returns standard browser manifest directories on Linux, Windows, or macOS.
pub fn get_browser_manifest_targets() -> Vec<(&'static str, PathBuf, bool)> {
    let mut targets = Vec::new();

    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let h = Path::new(&home);
            targets.push(("Chrome", h.join(".config").join("google-chrome").join("NativeMessagingHosts"), false));
            targets.push(("Chromium", h.join(".config").join("chromium").join("NativeMessagingHosts"), false));
            targets.push(("Brave", h.join(".config").join("BraveSoftware").join("Brave-Browser").join("NativeMessagingHosts"), false));
            targets.push(("Edge", h.join(".config").join("microsoft-edge").join("NativeMessagingHosts"), false));
            targets.push(("Opera", h.join(".config").join("opera").join("NativeMessagingHosts"), false));
            targets.push(("Vivaldi", h.join(".config").join("vivaldi").join("NativeMessagingHosts"), false));
            targets.push(("Firefox", h.join(".mozilla").join("native-messaging-hosts"), true));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let app_sup = Path::new(&home).join("Library").join("Application Support");
            targets.push(("Chrome", app_sup.join("Google").join("Chrome").join("NativeMessagingHosts"), false));
            targets.push(("Brave", app_sup.join("BraveSoftware").join("Brave-Browser").join("NativeMessagingHosts"), false));
            targets.push(("Edge", app_sup.join("Microsoft Edge").join("NativeMessagingHosts"), false));
            targets.push(("Opera", app_sup.join("com.operasoftware.Opera").join("NativeMessagingHosts"), false));
            targets.push(("Vivaldi", app_sup.join("Vivaldi").join("NativeMessagingHosts"), false));
            targets.push(("Firefox", app_sup.join("Mozilla").join("NativeMessagingHosts"), true));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            let host_dir = Path::new(&app_data).join("ImpressiveDownloadManager").join("NativeMessaging");
            targets.push(("WindowsHost", host_dir, false));
        }
    }

    targets
}

/// Automatically registers manifests across all installed and supported browsers on the machine.
pub fn register_all_browser_manifests(custom_id: Option<String>) -> Result<RegistrationSummary, String> {
    let current_exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("Failed to locate executable: {}", e))?;
    let exe_str = current_exe.to_string_lossy().to_string();

    let mut extension_ids = detect_installed_extension_ids();
    if let Some(ref cid) = custom_id {
        let clean = cid.trim().to_lowercase();
        if clean.len() == 32 && clean.chars().all(|c| c.is_ascii_lowercase()) && !extension_ids.contains(&clean) {
            extension_ids.insert(0, clean);
        }
    }

    let mut items = Vec::new();
    let targets = get_browser_manifest_targets();

    for (browser_name, dir_path, is_firefox) in targets {
        let manifest_val = generate_manifest(&exe_str, is_firefox, &extension_ids);
        let manifest_content = serde_json::to_string_pretty(&manifest_val).unwrap_or_default();
        let manifest_file = dir_path.join(format!("{}.json", NATIVE_HOST_NAME));

        match std::fs::create_dir_all(&dir_path) {
            Ok(_) => {
                match std::fs::write(&manifest_file, &manifest_content) {
                    Ok(_) => {
                        #[cfg(target_os = "windows")]
                        {
                            // Register registry keys on Windows
                            let json_path_str = manifest_file.to_string_lossy().to_string();
                            for reg_key in &[
                                "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.impressive.idm",
                                "HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.impressive.idm",
                                "HKCU\\Software\\BraveSoftware\\Brave-Browser\\NativeMessagingHosts\\com.impressive.idm",
                                "HKCU\\Software\\Mozilla\\NativeMessagingHosts\\com.impressive.idm",
                            ] {
                                let _ = std::process::Command::new("reg")
                                    .args(&["add", reg_key, "/ve", "/t", "REG_SZ", "/d", &json_path_str, "/f"])
                                    .spawn();
                            }
                        }

                        items.push(BrowserRegistrationItem {
                            browser: browser_name.to_string(),
                            manifest_path: manifest_file.to_string_lossy().to_string(),
                            installed: true,
                            error: None,
                        });
                    }
                    Err(e) => {
                        items.push(BrowserRegistrationItem {
                            browser: browser_name.to_string(),
                            manifest_path: manifest_file.to_string_lossy().to_string(),
                            installed: false,
                            error: Some(format!("Failed to write manifest: {}", e)),
                        });
                    }
                }
            }
            Err(e) => {
                items.push(BrowserRegistrationItem {
                    browser: browser_name.to_string(),
                    manifest_path: manifest_file.to_string_lossy().to_string(),
                    installed: false,
                    error: Some(format!("Failed to create directory: {}", e)),
                });
            }
        }
    }

    Ok(RegistrationSummary {
        success: items.iter().any(|i| i.installed),
        registered_browsers: items,
        detected_extension_ids: extension_ids,
        binary_path: exe_str,
    })
}

/// Returns the current registration status of the native host across all browsers.
pub fn get_integration_status() -> IntegrationStatus {
    let exe_str = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let targets = get_browser_manifest_targets();
    let mut items = Vec::new();

    for (browser_name, dir_path, _) in targets {
        let manifest_file = dir_path.join(format!("{}.json", NATIVE_HOST_NAME));
        let installed = manifest_file.exists();
        items.push(BrowserRegistrationItem {
            browser: browser_name.to_string(),
            manifest_path: manifest_file.to_string_lossy().to_string(),
            installed,
            error: None,
        });
    }

    IntegrationStatus {
        native_host_name: NATIVE_HOST_NAME.to_string(),
        binary_path: exe_str,
        browsers: items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase8_native_framing_roundtrip() {
        let original = serde_json::json!({
            "action": "download",
            "url": "https://example.com/archive.zip",
            "filename": "archive.zip",
            "size": 1048576
        });

        let mut buffer = Vec::new();
        write_native_message(&mut buffer, &original).expect("Write must succeed");

        assert!(buffer.len() > 4, "Buffer must contain length prefix and payload");
        let len_prefix = u32::from_le_bytes(buffer[0..4].try_into().unwrap()) as usize;
        assert_eq!(len_prefix, buffer.len() - 4, "Length prefix must match payload length");

        let mut cursor = std::io::Cursor::new(buffer);
        let decoded = read_native_message(&mut cursor).expect("Read must succeed");

        assert_eq!(decoded["action"], "download");
        assert_eq!(decoded["url"], "https://example.com/archive.zip");
        assert_eq!(decoded["filename"], "archive.zip");
        assert_eq!(decoded["size"], 1048576);
    }

    #[test]
    fn test_phase8_native_framing_oversize_rejection() {
        // Construct header with 2MB length prefix (> 1MB limit)
        let invalid_length: u32 = 2 * 1024 * 1024;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&invalid_length.to_le_bytes());
        buffer.extend_from_slice(b"{}");

        let mut cursor = std::io::Cursor::new(buffer);
        let res = read_native_message(&mut cursor);
        assert!(res.is_err(), "Must reject payload exceeding 1MB");
        assert!(res.unwrap_err().to_string().contains("exceeds limit"));
    }

    #[test]
    fn test_phase8_native_payload_sanitization_security() {
        // 1. Rejection of embedded credentials
        let bad_cred = serde_json::json!({
            "url": "https://admin:secret123@malicious.com/file.bin"
        });
        assert!(sanitize_native_payload(&bad_cred).is_err(), "Must reject embedded credentials");

        // 2. Rejection of non-HTTP schemes
        let bad_scheme = serde_json::json!({
            "url": "file:///etc/passwd"
        });
        assert!(sanitize_native_payload(&bad_scheme).is_err(), "Must reject file:// scheme");

        // 3. CRLF injection in headers stripped
        let crlf_payload = serde_json::json!({
            "url": "https://example.com/test.iso",
            "filename": "../../../etc/shadow",
            "cookie": "session=123\r\nX-Injected: Bad",
            "referrer": "https://clean.com/refer"
        });
        let sanitized = sanitize_native_payload(&crlf_payload).expect("Must sanitize");
        assert_eq!(sanitized.cookie, None, "Must discard headers with CRLF injection");
        assert!(!sanitized.filename.unwrap().contains("../"), "Path traversal in filename must be sanitized");
    }

    #[test]
    fn test_phase8_manifest_generation_origins_strict() {
        let ids = vec![
            "abcdefghijklmnopabcdefghijklmnop".to_string(), // valid 32 chars
            "malicious-id-with-special!chars".to_string(),  // invalid
            "SHORT".to_string(),                             // invalid
        ];
        let manifest = generate_manifest("/usr/bin/impressive-download-manager", false, &ids);

        assert_eq!(manifest["name"], "com.impressive.idm");
        assert_eq!(manifest["type"], "stdio");
        assert_eq!(manifest["path"], "/usr/bin/impressive-download-manager");

        let origins = manifest["allowed_origins"].as_array().unwrap();
        assert_eq!(origins.len(), 1, "Only valid 32-letter IDs should produce allowed_origins");
        assert_eq!(origins[0], "chrome-extension://abcdefghijklmnopabcdefghijklmnop/");
    }

    #[test]
    fn test_phase8_manifest_firefox_allowed_extensions() {
        let manifest = generate_manifest("/usr/bin/impressive-download-manager", true, &[]);
        assert_eq!(manifest["name"], "com.impressive.idm");
        let ext = manifest["allowed_extensions"].as_array().unwrap();
        assert_eq!(ext[0], FIREFOX_EXTENSION_ID);
    }
}
