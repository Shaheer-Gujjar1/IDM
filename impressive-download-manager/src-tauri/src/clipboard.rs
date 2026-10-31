use std::collections::HashSet;
use std::sync::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardSnifferConfig {
    pub enabled: bool,
    pub strict_mode: bool,        // true: only file downloads, media & torrent/ed2k links; false: any valid URL
    pub auto_open_modal: bool,    // true: automatically open Add modal; false: display floating sniffer banner
    pub ignored_domains: Vec<String>,
}

impl Default for ClipboardSnifferConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strict_mode: true,
            auto_open_modal: true,
            ignored_domains: vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardDetectionResult {
    pub is_valid: bool,
    pub url: String,
    pub protocol: String,
    pub extension: Option<String>,
    pub category: String,
    pub suggested_filename: String,
    pub is_media_or_archive: bool,
    pub reason: String,
}

pub struct ClipboardSniffer {
    pub config: RwLock<ClipboardSnifferConfig>,
    pub last_sniffed_url: RwLock<Option<String>>,
    pub ignored_internal_urls: RwLock<HashSet<String>>,
}

impl ClipboardSniffer {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(ClipboardSnifferConfig::default()),
            last_sniffed_url: RwLock::new(None),
            ignored_internal_urls: RwLock::new(HashSet::new()),
        }
    }

    pub fn get_config(&self) -> ClipboardSnifferConfig {
        self.config.read().unwrap().clone()
    }

    pub fn set_config(&self, new_config: ClipboardSnifferConfig) {
        let mut cfg = self.config.write().unwrap();
        *cfg = new_config;
    }

    pub fn mark_internal_copy(&self, text: &str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            let mut set = self.ignored_internal_urls.write().unwrap();
            set.insert(trimmed.to_string());
            // Keep set size bounded to prevent unbounded memory growth
            if set.len() > 1000 {
                set.clear();
            }
        }
    }

    pub fn reset_last_sniffed(&self) {
        let mut last = self.last_sniffed_url.write().unwrap();
        *last = None;
    }

    pub fn inspect_and_sniff(&self, text: &str) -> Option<ClipboardDetectionResult> {
        let config = self.get_config();
        if !config.enabled {
            return None;
        }

        let raw = text.trim();
        if raw.is_empty() || raw.len() > 8192 {
            return None;
        }

        // Check if this was marked as an internal copy from inside IDM
        {
            let mut ignored = self.ignored_internal_urls.write().unwrap();
            if ignored.remove(raw) {
                return None;
            }
        }

        // Check if identical to last sniffed URL
        {
            let last = self.last_sniffed_url.read().unwrap();
            if let Some(ref prev) = *last {
                if prev == raw {
                    return None;
                }
            }
        }

        let detection = parse_and_classify_clipboard_url(raw, &config)?;

        if detection.is_valid {
            let mut last = self.last_sniffed_url.write().unwrap();
            *last = Some(raw.to_string());
            Some(detection)
        } else {
            None
        }
    }
}

pub fn parse_and_classify_clipboard_url(raw: &str, config: &ClipboardSnifferConfig) -> Option<ClipboardDetectionResult> {
    let lower = raw.to_lowercase();

    // 1. Magnet links
    if lower.starts_with("magnet:?") {
        let mut filename = "torrent_download".to_string();
        for param in raw["magnet:?".len()..].split('&') {
            if let Some(stripped) = param.strip_prefix("dn=") {
                if let Ok(decoded) = urlencoding::decode(stripped) {
                    filename = decoded.to_string();
                }
            }
        }
        return Some(ClipboardDetectionResult {
            is_valid: true,
            url: raw.to_string(),
            protocol: "magnet".to_string(),
            extension: Some("torrent".to_string()),
            category: "Torrent / Magnet".to_string(),
            suggested_filename: filename,
            is_media_or_archive: true,
            reason: "BitTorrent Magnet Link".to_string(),
        });
    }

    // 2. eDonkey2000 / eMule links
    if lower.starts_with("ed2k://") {
        let parts: Vec<&str> = raw.split('|').collect();
        let filename = if parts.len() >= 3 && !parts[2].is_empty() {
            parts[2].to_string()
        } else {
            "ed2k_download".to_string()
        };
        return Some(ClipboardDetectionResult {
            is_valid: true,
            url: raw.to_string(),
            protocol: "ed2k".to_string(),
            extension: Some("ed2k".to_string()),
            category: "eD2k / eMule".to_string(),
            suggested_filename: filename,
            is_media_or_archive: true,
            reason: "eDonkey2000 / eMule Protocol Link".to_string(),
        });
    }

    // 3. FTP / FTPS
    if lower.starts_with("ftp://") || lower.starts_with("ftps://") {
        let protocol = if lower.starts_with("ftps://") { "ftps" } else { "ftp" };
        let filename = extract_filename_from_url_path(raw);
        let ext = extract_extension(&filename);
        let cat = detect_category_by_extension(ext.as_deref());
        return Some(ClipboardDetectionResult {
            is_valid: true,
            url: raw.to_string(),
            protocol: protocol.to_string(),
            extension: ext,
            category: cat,
            suggested_filename: filename,
            is_media_or_archive: true,
            reason: "FTP/FTPS File Link".to_string(),
        });
    }

    // 4. HTTP / HTTPS
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let protocol = if lower.starts_with("https://") { "https" } else { "http" };

        // Check ignored domains
        for domain in &config.ignored_domains {
            if !domain.is_empty() && (lower.contains(&format!("://{}", domain.to_lowercase())) || lower.contains(&format!("://www.{}", domain.to_lowercase()))) {
                return None;
            }
        }

        // Known video & streaming platforms
        if is_streaming_platform(&lower) {
            let filename = if lower.contains("youtube.com/playlist") || lower.contains("list=") {
                "YouTube_Playlist".to_string()
            } else if lower.contains("youtube") || lower.contains("youtu.be") {
                "YouTube_Video".to_string()
            } else if lower.contains("bilibili") {
                "Bilibili_Video".to_string()
            } else {
                "Stream_Video".to_string()
            };

            return Some(ClipboardDetectionResult {
                is_valid: true,
                url: raw.to_string(),
                protocol: protocol.to_string(),
                extension: Some("mp4".to_string()),
                category: "Video".to_string(),
                suggested_filename: filename,
                is_media_or_archive: true,
                reason: "Streaming Media Platform Link".to_string(),
            });
        }

        // Check file extension in URL
        let filename = extract_filename_from_url_path(raw);
        let ext = extract_extension(&filename);
        let is_download_ext = ext.as_deref().map(is_downloadable_extension).unwrap_or(false);

        // Check GitHub release or Hugging Face links
        let is_special_hub = lower.contains("github.com") && (lower.contains("/releases/download/") || lower.contains("/archive/"))
            || lower.contains("raw.githubusercontent.com")
            || lower.contains("huggingface.co");

        let is_media_or_archive = is_download_ext || is_special_hub;
        let category = detect_category_by_extension(ext.as_deref());

        if is_media_or_archive {
            return Some(ClipboardDetectionResult {
                is_valid: true,
                url: raw.to_string(),
                protocol: protocol.to_string(),
                extension: ext,
                category,
                suggested_filename: filename,
                is_media_or_archive: true,
                reason: "Direct Download Link / Media File".to_string(),
            });
        }

        // If strict mode is enabled, non-download/non-media pages are ignored
        if config.strict_mode {
            return None;
        }

        // If strict mode is off, any valid HTTP/HTTPS URL is accepted
        return Some(ClipboardDetectionResult {
            is_valid: true,
            url: raw.to_string(),
            protocol: protocol.to_string(),
            extension: ext,
            category: "Other".to_string(),
            suggested_filename: filename,
            is_media_or_archive: false,
            reason: "Web URL (Catch-All Mode)".to_string(),
        });
    }

    None
}

fn is_streaming_platform(lower: &str) -> bool {
    lower.contains("youtube.com/watch")
        || lower.contains("youtu.be/")
        || lower.contains("youtube.com/playlist")
        || lower.contains("youtube.com/shorts/")
        || lower.contains("bilibili.com/video/")
        || lower.contains("vimeo.com/")
        || lower.contains("dailymotion.com/video/")
        || lower.contains(".m3u8")
        || lower.contains(".mpd")
}

fn extract_filename_from_url_path(url_str: &str) -> String {
    let clean = url_str.split('?').next().unwrap_or(url_str);
    let clean = clean.split('#').next().unwrap_or(clean);
    let parts: Vec<&str> = clean.trim_end_matches('/').split('/').collect();
    if let Some(&last) = parts.last() {
        if !last.is_empty() {
            if let Ok(decoded) = urlencoding::decode(last) {
                return decoded.to_string();
            }
            return last.to_string();
        }
    }
    "downloaded_file".to_string()
}

fn extract_extension(filename: &str) -> Option<String> {
    if let Some(pos) = filename.rfind('.') {
        if pos < filename.len() - 1 && pos > 0 {
            let ext = &filename[pos + 1..];
            if ext.len() <= 12 && !ext.contains('/') && !ext.contains(' ') {
                return Some(ext.to_lowercase());
            }
        }
    }
    None
}

pub fn is_downloadable_extension(ext: &str) -> bool {
    matches!(
        ext,
        // Compressed & Archives
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "zst" | "iso" | "dmg" | "pkg" | "deb" | "rpm" | "apk" | "crx" | "xpi"
        // Executables & Installers
        | "exe" | "msi" | "bin" | "appimage"
        // Video
        | "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "ts" | "m3u8" | "mpd"
        // Audio
        | "mp3" | "flac" | "wav" | "aac" | "m4a" | "ogg" | "opus" | "wma"
        // Documents
        | "pdf" | "epub" | "mobi" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "csv"
        // AI / Weights / Datasets
        | "gguf" | "safetensors" | "onnx" | "pt" | "pth" | "parquet" | "arrow"
        // P2P / Links
        | "torrent" | "ed2k_pack"
    )
}

fn detect_category_by_extension(ext: Option<&str>) -> String {
    match ext {
        Some("mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "ts" | "m3u8" | "mpd") => "Video".to_string(),
        Some("mp3" | "flac" | "wav" | "aac" | "m4a" | "ogg" | "opus" | "wma") => "Audio".to_string(),
        Some("zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "zst" | "iso" | "dmg" | "pkg" | "deb" | "rpm" | "apk" | "ed2k_pack") => "Compressed".to_string(),
        Some("pdf" | "epub" | "mobi" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "csv") => "Documents".to_string(),
        Some("exe" | "msi" | "appimage") => "Programs".to_string(),
        _ => "Other".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_sniffer_magnet_link() {
        let sniffer = ClipboardSniffer::new();
        let magnet = "magnet:?xt=urn:btih:d6b41221b025350b9ec903d3bc17f8a7e0c469e3&dn=Ubuntu-24.04-Server.iso&tr=http://tracker.example.com";
        let res = sniffer.inspect_and_sniff(magnet);
        assert!(res.is_some());
        let val = res.unwrap();
        assert!(val.is_valid);
        assert_eq!(val.protocol, "magnet");
        assert_eq!(val.suggested_filename, "Ubuntu-24.04-Server.iso");
        assert_eq!(val.category, "Torrent / Magnet");

        // Subsequent identical check should return None (deduplication)
        assert!(sniffer.inspect_and_sniff(magnet).is_none());
    }

    #[test]
    fn test_clipboard_sniffer_ed2k_link() {
        let sniffer = ClipboardSniffer::new();
        let ed2k = "ed2k://|file|sample_dataset.tar.gz|10485760|0123456789ABCDEF0123456789ABCDEF|/";
        let res = sniffer.inspect_and_sniff(ed2k);
        assert!(res.is_some());
        let val = res.unwrap();
        assert_eq!(val.protocol, "ed2k");
        assert_eq!(val.suggested_filename, "sample_dataset.tar.gz");
    }

    #[test]
    fn test_clipboard_sniffer_youtube_and_streaming() {
        let sniffer = ClipboardSniffer::new();
        let yt = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let res = sniffer.inspect_and_sniff(yt).unwrap();
        assert_eq!(res.category, "Video");
        assert!(res.is_media_or_archive);

        let m3u8 = "https://cdn.live.stream.net/playlist.m3u8";
        let res_m3u8 = sniffer.inspect_and_sniff(m3u8).unwrap();
        assert_eq!(res_m3u8.category, "Video");
    }

    #[test]
    fn test_clipboard_sniffer_strict_vs_non_strict() {
        let sniffer = ClipboardSniffer::new();
        let regular_webpage = "https://en.wikipedia.org/wiki/Rust_(programming_language)";

        // Strict mode (default): should ignore standard webpage
        assert!(sniffer.inspect_and_sniff(regular_webpage).is_none());

        // Switch to non-strict (Catch-all) mode
        sniffer.set_config(ClipboardSnifferConfig {
            enabled: true,
            strict_mode: false,
            auto_open_modal: true,
            ignored_domains: vec![],
        });
        sniffer.reset_last_sniffed();

        let res = sniffer.inspect_and_sniff(regular_webpage);
        assert!(res.is_some());
        assert_eq!(res.unwrap().protocol, "https");
    }

    #[test]
    fn test_clipboard_sniffer_internal_copy_ignored() {
        let sniffer = ClipboardSniffer::new();
        let url = "https://download.archive.org/linux-distro.iso";
        sniffer.mark_internal_copy(url);

        // Should ignore because it was marked as internal IDM copy
        assert!(sniffer.inspect_and_sniff(url).is_none());

        // A different URL should work
        let new_url = "https://download.archive.org/another-distro.iso";
        let res = sniffer.inspect_and_sniff(new_url);
        assert!(res.is_some());
    }
}
