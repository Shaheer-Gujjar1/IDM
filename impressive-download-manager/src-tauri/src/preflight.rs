use std::path::Path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskSpaceInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub formatted_available: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightProbeResult {
    pub url: String,
    pub final_url: String,
    pub filename: String,
    pub total_size: u64,
    pub formatted_size: String,
    pub supports_range: bool,
    pub category: String,
    pub mime_type: Option<String>,
    pub available_disk_space: Option<u64>,
    pub formatted_disk_space: Option<String>,
    pub has_enough_space: bool,
    pub file_already_exists: bool,
    pub existing_file_size: Option<u64>,
}

/// Automatically classifies a download into authentic IDM categories based on MIME type and file extension.
pub fn detect_category(filename: &str, mime_type: Option<&str>) -> &'static str {
    // 1. Check MIME type first if available
    if let Some(mime) = mime_type {
        let m = mime.to_lowercase();
        if m.starts_with("video/") {
            return "Video";
        }
        if m.starts_with("audio/") {
            return "Audio";
        }
        if m.contains("zip")
            || m.contains("tar")
            || m.contains("compressed")
            || m.contains("archive")
            || m.contains("7z")
            || m.contains("rar")
        {
            return "Compressed";
        }
        if m.starts_with("text/")
            || m.contains("pdf")
            || m.contains("msword")
            || m.contains("officedocument")
            || m.contains("document")
            || m.contains("sheet")
            || m.contains("presentation")
        {
            return "Documents";
        }
        if m.contains("application/x-msdownload")
            || m.contains("application/x-executable")
            || m.contains("application/vnd.android.package-archive")
        {
            return "Programs";
        }
    }

    // 2. Check for magnet link or extension from filename
    let lower = filename.to_lowercase();
    if lower.starts_with("magnet:?") {
        return "Compressed";
    }

    let ext = match lower.rfind('.') {
        Some(idx) => &lower[idx + 1..],
        None => return "Other",
    };

    match ext {
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v" | "flv" | "3gp" | "ts" | "m3u8" | "wmv" => "Video",
        "mp3" | "m4a" | "aac" | "flac" | "ogg" | "wav" | "wma" | "opus" | "m4b" => "Audio",
        "zip" | "7z" | "rar" | "tar" | "gz" | "bz2" | "xz" | "iso" | "dmg" | "tgz" | "torrent" => "Compressed",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "csv" | "epub" | "rtf" | "md" | "odt" | "ods" => "Documents",
        "exe" | "msi" | "msix" | "deb" | "rpm" | "apk" | "appimage" | "bin" | "sh" | "bat" | "cmd" | "gguf" | "safetensors" | "onnx" | "pt" | "pth" | "tflite" | "parquet" | "arrow" => "Programs",
        _ => "Other",
    }
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "Unknown".to_string();
    }
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

/// Safely inspects available disk space on the target partition without shell interpolation.
pub fn get_disk_space(target_path: &str) -> Result<DiskSpaceInfo, String> {
    let p = Path::new(target_path);
    // Find an existing ancestor directory if target does not yet exist
    let mut check_path = p;
    while !check_path.exists() {
        if let Some(parent) = check_path.parent() {
            check_path = parent;
        } else {
            break;
        }
    }

    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::mem::MaybeUninit;
        use std::os::unix::ffi::OsStrExt;

        let path_bytes = if check_path.as_os_str().is_empty() {
            b".".as_slice()
        } else {
            check_path.as_os_str().as_bytes()
        };

        if let Ok(c_path) = CString::new(path_bytes) {
            let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();
            let res = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
            if res == 0 {
                let stat = unsafe { stat.assume_init() };
                let block_size = if stat.f_frsize > 0 {
                    stat.f_frsize as u64
                } else {
                    stat.f_bsize as u64
                };
                let total_bytes = stat.f_blocks as u64 * block_size;
                let available_bytes = stat.f_bavail as u64 * block_size;
                return Ok(DiskSpaceInfo {
                    total_bytes,
                    available_bytes,
                    formatted_available: format_bytes(available_bytes),
                });
            }
        }

        // Secondary fallback to df only if statvfs fails
        let output = std::process::Command::new("df")
            .arg("-k")
            .arg(check_path)
            .output()
            .map_err(|e| format!("Failed to inspect disk space: {}", e))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = stdout.lines().collect();
            if lines.len() >= 2 {
                let cols: Vec<&str> = lines[1].split_whitespace().collect();
                if cols.len() >= 4 {
                    let total_kb: u64 = cols[1].parse().unwrap_or(0);
                    let avail_kb: u64 = cols[3].parse().unwrap_or(0);
                    let total_bytes = total_kb * 1024;
                    let available_bytes = avail_kb * 1024;
                    return Ok(DiskSpaceInfo {
                        total_bytes,
                        available_bytes,
                        formatted_available: format_bytes(available_bytes),
                    });
                }
            }
        }
        Err("Could not inspect disk space".to_string())
    }

    #[cfg(not(unix))]
    {
        // Fallback for non-unix OS
        Ok(DiskSpaceInfo {
            total_bytes: 100 * 1024 * 1024 * 1024,
            available_bytes: 50 * 1024 * 1024 * 1024,
            formatted_available: "50.00 GB".to_string(),
        })
    }
}

/// Issues an optimized pre-flight HTTP probe (HEAD or Range 0-0 GET) to detect real file metadata,
/// Content-Disposition filename, Content-Length, and category before starting full download.
pub async fn probe_download_url(
    url: &str,
    cookie: Option<&str>,
    referrer: Option<&str>,
    user_agent: Option<&str>,
    save_folder: Option<&str>,
) -> Result<PreflightProbeResult, String> {
    // FTP / FTPS Protocol Probe Handling
    if url.starts_with("ftp://") || url.starts_with("ftps://") {
        let ftp_info = crate::ftp::parse_ftp_url(url)?;
        let filename_candidate = crate::proxy::sanitize_filename(&ftp_info.filename);
        let category = detect_category(&filename_candidate, None).to_string();

        let (total_size, supports_range) = crate::ftp::probe_ftp_file(&ftp_info).await.unwrap_or((0, false));

        let target_folder = save_folder.unwrap_or(".");
        let target_file_path = std::path::Path::new(target_folder).join(&filename_candidate);
        let file_already_exists = target_file_path.exists();
        let existing_file_size = if file_already_exists {
            std::fs::metadata(&target_file_path).map(|m| m.len()).ok()
        } else {
            None
        };

        let disk_info = get_disk_space(target_folder).ok();
        let (available_disk_space, formatted_disk_space, has_enough_space) = match disk_info {
            Some(info) => {
                let enough = if total_size > 0 { info.available_bytes >= total_size } else { true };
                (Some(info.available_bytes), Some(info.formatted_available), enough)
            }
            None => (None, None, true),
        };

        return Ok(PreflightProbeResult {
            url: url.to_string(),
            final_url: url.to_string(),
            filename: filename_candidate,
            total_size,
            formatted_size: format_bytes(total_size),
            supports_range,
            category,
            mime_type: None,
            available_disk_space,
            formatted_disk_space,
            has_enough_space,
            file_already_exists,
            existing_file_size,
        });
    }

    // eDonkey2000 / eMule Protocol Probe Handling
    if crate::ed2k::is_ed2k_url(url) {
        let ed2k_info = crate::ed2k::parse_ed2k_link(url)?;
        let filename_candidate = crate::proxy::sanitize_filename(&ed2k_info.filename);
        let category = detect_category(&filename_candidate, None).to_string();
        let total_size = ed2k_info.file_size;

        let target_folder = save_folder.unwrap_or(".");
        let target_file_path = std::path::Path::new(target_folder).join(&filename_candidate);
        let file_already_exists = target_file_path.exists();
        let existing_file_size = if file_already_exists {
            std::fs::metadata(&target_file_path).map(|m| m.len()).ok()
        } else {
            None
        };

        let disk_info = get_disk_space(target_folder).ok();
        let (available_disk_space, formatted_disk_space, has_enough_space) = match disk_info {
            Some(info) => {
                let enough = if total_size > 0 { info.available_bytes >= total_size } else { true };
                (Some(info.available_bytes), Some(info.formatted_available), enough)
            }
            None => (None, None, true),
        };

        return Ok(PreflightProbeResult {
            url: url.to_string(),
            final_url: url.to_string(),
            filename: filename_candidate,
            total_size,
            formatted_size: format_bytes(total_size),
            supports_range: true,
            category,
            mime_type: None,
            available_disk_space,
            formatted_disk_space,
            has_enough_space,
            file_already_exists,
            existing_file_size,
        });
    }

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Unsupported protocol. Only http://, https://, ftp://, ftps://, and ed2k:// URLs are supported.".to_string());
    }

    // Hugging Face AI URL normalization (/blob/ -> /resolve/)
    let probe_url_target = if crate::huggingface::is_huggingface_url(url) {
        crate::huggingface::normalize_huggingface_url(url).unwrap_or_else(|| url.to_string())
    } else {
        url.to_string()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(10))
        .tcp_nodelay(true)
        .build()
        .map_err(|e| format!("Client build error: {}", e))?;

    let profile = crate::engine::BrowserProfile::detect(user_agent.unwrap_or(""));
    let target_ref = probe_url_target.clone();
    let build_request = move |mut req: reqwest::RequestBuilder| {
        if crate::huggingface::is_huggingface_url(&target_ref) {
            req = crate::huggingface::apply_huggingface_auth(req, None);
        }
        profile.apply_headers(req, &target_ref, referrer.unwrap_or(""), cookie.unwrap_or(""), false)
    };

    let mut final_url = probe_url_target.clone();
    let mut filename_candidate = String::new();
    let mut total_size = 0u64;
    let mut supports_range = false;
    let mut mime_type = None;

    // 1. Try HEAD request first
    if let Ok(res) = build_request(client.head(&probe_url_target)).send().await {
        final_url = res.url().to_string();
        if let Some(cd) = res.headers().get(reqwest::header::CONTENT_DISPOSITION) {
            if let Ok(cd_str) = cd.to_str() {
                if let Some(fn_parsed) = crate::proxy::parse_content_disposition(cd_str) {
                    filename_candidate = fn_parsed;
                }
            }
        }
        if let Some(cl) = res.headers().get(reqwest::header::CONTENT_LENGTH) {
            if let Ok(cl_str) = cl.to_str() {
                total_size = cl_str.parse().unwrap_or(0);
            }
        }
        if let Some(ar) = res.headers().get(reqwest::header::ACCEPT_RANGES) {
            if let Ok(ar_str) = ar.to_str() {
                if ar_str.to_lowercase().contains("bytes") {
                    supports_range = true;
                }
            }
        }
        if let Some(ct) = res.headers().get(reqwest::header::CONTENT_TYPE) {
            if let Ok(ct_str) = ct.to_str() {
                mime_type = Some(ct_str.split(';').next().unwrap_or(ct_str).trim().to_string());
            }
        }
    }

    // 2. If HEAD returned 0 size, try GET with Range 0-0
    if total_size == 0 {
        if let Ok(res) = build_request(client.get(&probe_url_target).header(reqwest::header::RANGE, "bytes=0-0")).send().await {
            final_url = res.url().to_string();
            if res.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                supports_range = true;
            }
            if let Some(cd) = res.headers().get(reqwest::header::CONTENT_DISPOSITION) {
                if let Ok(cd_str) = cd.to_str() {
                    if let Some(fn_parsed) = crate::proxy::parse_content_disposition(cd_str) {
                        filename_candidate = fn_parsed;
                    }
                }
            }
            if let Some(cr) = res.headers().get(reqwest::header::CONTENT_RANGE) {
                if let Ok(cr_str) = cr.to_str() {
                    if let Some(slash_idx) = cr_str.rfind('/') {
                        let total_str = cr_str[slash_idx + 1..].trim();
                        if let Ok(s) = total_str.parse::<u64>() {
                            total_size = s;
                        }
                    }
                }
            }
            if total_size == 0 {
                if let Some(cl) = res.headers().get(reqwest::header::CONTENT_LENGTH) {
                    if let Ok(cl_str) = cl.to_str() {
                        total_size = cl_str.parse().unwrap_or(0);
                    }
                }
            }
            if mime_type.is_none() {
                if let Some(ct) = res.headers().get(reqwest::header::CONTENT_TYPE) {
                    if let Ok(ct_str) = ct.to_str() {
                        mime_type = Some(ct_str.split(';').next().unwrap_or(ct_str).trim().to_string());
                    }
                }
            }
        }
    }

    // 3. Fallback filename from URL if Content-Disposition was not present
    if filename_candidate.is_empty() || crate::proxy::is_generic_filename(&filename_candidate) {
        if let Some(hf) = crate::huggingface::parse_huggingface_url(url) {
            filename_candidate = hf.filename;
        } else if let Some(gh) = crate::github_accelerator::parse_github_url(url) {
            filename_candidate = gh.filename;
        } else if let Ok(parsed) = reqwest::Url::parse(&final_url) {
            for (k, v) in parsed.query_pairs() {
                let k_lower = k.to_lowercase();
                if (k_lower.contains("file") || k_lower.contains("name") || k_lower.contains("title")) && v.contains('.') {
                    if let Ok(decoded) = urlencoding::decode(&v) {
                        let sanitized = crate::proxy::sanitize_filename(&decoded);
                        if !crate::proxy::is_generic_filename(&sanitized) && sanitized.contains('.') {
                            filename_candidate = sanitized;
                            break;
                        }
                    }
                }
            }
            if filename_candidate.is_empty() {
                if let Some(last_seg) = parsed.path_segments().and_then(|s| s.last()) {
                    if !last_seg.is_empty() && last_seg.contains('.') {
                        if let Ok(decoded) = urlencoding::decode(last_seg) {
                            let sanitized = crate::proxy::sanitize_filename(&decoded);
                            if !crate::proxy::is_generic_filename(&sanitized) {
                                filename_candidate = sanitized;
                            }
                        }
                    }
                }
            }
        }
    }

    if filename_candidate.is_empty() {
        filename_candidate = "downloaded_file".to_string();
    }
    filename_candidate = crate::proxy::sanitize_filename(&filename_candidate);

    let category = detect_category(&filename_candidate, mime_type.as_deref()).to_string();

    // 4. Disk space and existing duplicate file probe
    let target_folder = save_folder.unwrap_or(".");
    let target_file_path = std::path::Path::new(target_folder).join(&filename_candidate);
    let file_already_exists = target_file_path.exists();
    let existing_file_size = if file_already_exists {
        std::fs::metadata(&target_file_path).map(|m| m.len()).ok()
    } else {
        None
    };

    let disk_info = get_disk_space(target_folder).ok();
    let (available_disk_space, formatted_disk_space, has_enough_space) = match disk_info {
        Some(info) => {
            let enough = if total_size > 0 {
                info.available_bytes >= total_size
            } else {
                true
            };
            (Some(info.available_bytes), Some(info.formatted_available), enough)
        }
        None => (None, None, true),
    };

    Ok(PreflightProbeResult {
        url: url.to_string(),
        final_url,
        filename: filename_candidate,
        total_size,
        formatted_size: format_bytes(total_size),
        supports_range,
        category,
        mime_type,
        available_disk_space,
        formatted_disk_space,
        has_enough_space,
        file_already_exists,
        existing_file_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase7_detect_category_extensions() {
        assert_eq!(detect_category("movie.mkv", None), "Video");
        assert_eq!(detect_category("stream.mp4", None), "Video");
        assert_eq!(detect_category("track.flac", None), "Audio");
        assert_eq!(detect_category("song.mp3", None), "Audio");
        assert_eq!(detect_category("archive.7z", None), "Compressed");
        assert_eq!(detect_category("backup.tar.gz", None), "Compressed");
        assert_eq!(detect_category("document.pdf", None), "Documents");
        assert_eq!(detect_category("sheet.xlsx", None), "Documents");
        assert_eq!(detect_category("setup.exe", None), "Programs");
        assert_eq!(detect_category("package.deb", None), "Programs");
        assert_eq!(detect_category("unknown.dat", None), "Other");
    }

    #[test]
    fn test_phase7_detect_category_mime_precedence() {
        assert_eq!(detect_category("download", Some("video/mp4")), "Video");
        assert_eq!(detect_category("audio_stream", Some("audio/mpeg")), "Audio");
        assert_eq!(detect_category("file.bin", Some("application/zip")), "Compressed");
        assert_eq!(detect_category("blob", Some("application/pdf")), "Documents");
        assert_eq!(detect_category("installer", Some("application/x-msdownload")), "Programs");
    }

    #[test]
    fn test_phase7_format_bytes() {
        assert_eq!(format_bytes(0), "Unknown");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024 * 512), "512.0 KB");
        assert_eq!(format_bytes(1024 * 1024 * 25), "25.00 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 4), "4.00 GB");
    }

    #[test]
    fn test_phase7_get_disk_space_current_dir() {
        let res = get_disk_space(".");
        assert!(res.is_ok(), "Disk space probe should succeed for current directory");
        let info = res.unwrap();
        assert!(info.total_bytes > 0);
        assert!(info.available_bytes > 0);
        assert!(!info.formatted_available.is_empty());
    }

    #[tokio::test]
    async fn test_phase7_probe_url_security_rejections() {
        let res = probe_download_url("file:///etc/passwd", None, None, None, None).await;
        assert!(res.is_err(), "Local file:// protocol must be rejected");

        let res = probe_download_url("javascript:alert(1)", None, None, None, None).await;
        assert!(res.is_err(), "javascript: protocol must be rejected");
    }
}
