use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YouTubePlaylistItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub duration_seconds: Option<u64>,
    pub thumbnail: Option<String>,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YouTubePlaylistInfo {
    pub id: String,
    pub title: String,
    pub author: String,
    pub video_count: usize,
    pub items: Vec<YouTubePlaylistItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleTrackInfo {
    pub language_code: String,
    pub language_name: String,
    pub format: String,
    pub is_auto_generated: bool,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCue {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Checks if a given URL is a YouTube playlist URL
pub fn is_youtube_playlist_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if !lower.contains("youtube.com") && !lower.contains("youtu.be") {
        return false;
    }
    lower.contains("list=")
}

/// Extracts the YouTube playlist ID (`list=...`) from a URL
pub fn extract_playlist_id(url: &str) -> Option<String> {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        for (k, v) in parsed.query_pairs() {
            if k == "list" && !v.is_empty() {
                return Some(v.into_owned());
            }
        }
    }
    None
}

/// Resolves YouTube playlist metadata and video items using yt-dlp flat-playlist discovery
pub async fn resolve_youtube_playlist(
    playlist_url: &str,
    limit: Option<usize>,
) -> Result<YouTubePlaylistInfo, String> {
    let playlist_id = extract_playlist_id(playlist_url)
        .unwrap_or_else(|| "playlist".to_string());

    let ytdlp_bin = crate::mux::probe_ytdlp()
        .ok_or_else(|| "yt-dlp is required to resolve YouTube playlists".to_string())?;

    let mut cmd = tokio::process::Command::new(ytdlp_bin);
    cmd.arg("--flat-playlist")
       .arg("--dump-single-json")
       .arg("--no-warnings");

    if let Some(l) = limit {
        cmd.arg("--playlist-end").arg(l.to_string());
    }

    cmd.arg(playlist_url);

    let output = cmd.output().await.map_err(|e| format!("Failed to run yt-dlp: {}", e))?;
    if !output.status.success() {
        let err_text = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp playlist resolution failed: {}", err_text));
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let parsed_json: serde_json::Value = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("Failed to parse playlist JSON: {}", e))?;

    let title = parsed_json.get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("YouTube Playlist")
        .to_string();

    let author = parsed_json.get("uploader")
        .or_else(|| parsed_json.get("channel"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown Author")
        .to_string();

    let mut items = Vec::new();
    if let Some(entries) = parsed_json.get("entries").and_then(|v| v.as_array()) {
        for (idx, entry) in entries.iter().enumerate() {
            let vid = entry.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if vid.is_empty() {
                continue;
            }

            let vtitle = entry.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled Video").to_string();
            let vurl = format!("https://www.youtube.com/watch?v={}", vid);
            let duration = entry.get("duration").and_then(|v| v.as_u64());

            let thumbnail = entry.get("thumbnails")
                .and_then(|th| th.as_array())
                .and_then(|arr| arr.last())
                .and_then(|th| th.get("url"))
                .and_then(|u| u.as_str())
                .map(|s| s.to_string())
                .or_else(|| Some(format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", vid)));

            items.push(YouTubePlaylistItem {
                id: vid,
                title: vtitle,
                url: vurl,
                duration_seconds: duration,
                thumbnail,
                index: idx + 1,
            });
        }
    }

    let video_count = items.len();

    Ok(YouTubePlaylistInfo {
        id: playlist_id,
        title,
        author,
        video_count,
        items,
    })
}

// =========================================================================
// Subtitle (.srt / .vtt) Processing & Parsing Engine
// =========================================================================

/// Formats milliseconds to SubRip (.srt) timestamp: `HH:MM:SS,mmm`
pub fn format_timestamp_srt(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let rem = ms % 3_600_000;
    let minutes = rem / 60_000;
    let rem2 = rem % 60_000;
    let seconds = rem2 / 1_000;
    let millis = rem2 % 1_000;
    format!("{:02}:{:02}:{:02},{:03}", hours, minutes, seconds, millis)
}

/// Formats milliseconds to WebVTT (.vtt) timestamp: `HH:MM:SS.mmm`
pub fn format_timestamp_vtt(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let rem = ms % 3_600_000;
    let minutes = rem / 60_000;
    let rem2 = rem % 60_000;
    let seconds = rem2 / 1_000;
    let millis = rem2 % 1_000;
    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

/// Parses YouTube timedtext JSON3 format into cue objects
pub fn parse_timedtext_json3(json_content: &str) -> Result<Vec<SubtitleCue>, String> {
    let parsed: serde_json::Value = serde_json::from_str(json_content)
        .map_err(|e| format!("Invalid timedtext JSON: {}", e))?;

    let mut cues = Vec::new();

    if let Some(events) = parsed.get("events").and_then(|v| v.as_array()) {
        let mut cue_idx = 1;
        for ev in events {
            let start_ms = ev.get("tStartMs").and_then(|v| v.as_u64()).unwrap_or(0);
            let duration_ms = ev.get("dDurationMs").and_then(|v| v.as_u64()).unwrap_or(2000);
            let end_ms = start_ms + duration_ms;

            let mut text = String::new();
            if let Some(segs) = ev.get("segs").and_then(|v| v.as_array()) {
                for s in segs {
                    if let Some(part) = s.get("utf8").and_then(|v| v.as_str()) {
                        text.push_str(part);
                    }
                }
            }

            let clean_text = text.trim();
            if !clean_text.is_empty() && clean_text != "\n" {
                cues.push(SubtitleCue {
                    index: cue_idx,
                    start_ms,
                    end_ms,
                    text: clean_text.to_string(),
                });
                cue_idx += 1;
            }
        }
    }

    Ok(cues)
}

/// Converts YouTube timedtext JSON3 content into SubRip (.srt) subtitle text
pub fn convert_timedtext_json3_to_srt(json_content: &str) -> Result<String, String> {
    let cues = parse_timedtext_json3(json_content)?;
    let mut out = String::new();

    for cue in cues {
        out.push_str(&format!("{}\n", cue.index));
        out.push_str(&format!(
            "{} --> {}\n",
            format_timestamp_srt(cue.start_ms),
            format_timestamp_srt(cue.end_ms)
        ));
        out.push_str(&cue.text);
        out.push_str("\n\n");
    }

    Ok(out)
}

/// Converts YouTube timedtext JSON3 content into WebVTT (.vtt) subtitle text
pub fn convert_timedtext_json3_to_vtt(json_content: &str) -> Result<String, String> {
    let cues = parse_timedtext_json3(json_content)?;
    let mut out = String::from("WEBVTT\n\n");

    for cue in cues {
        out.push_str(&format!("{}\n", cue.index));
        out.push_str(&format!(
            "{} --> {}\n",
            format_timestamp_vtt(cue.start_ms),
            format_timestamp_vtt(cue.end_ms)
        ));
        out.push_str(&cue.text);
        out.push_str("\n\n");
    }

    Ok(out)
}

/// Standalone subtitle downloader: downloads `.srt` or `.vtt` subtitles for a YouTube video
pub async fn download_youtube_subtitles_standalone(
    video_url: &str,
    lang: Option<&str>,
    format: &str,
    output_dir: &Path,
    custom_filename: Option<&str>,
) -> Result<PathBuf, String> {
    let ytdlp_bin = crate::mux::probe_ytdlp()
        .ok_or_else(|| "yt-dlp is required to download YouTube subtitles".to_string())?;

    let lang_code = lang.unwrap_or("en");
    let sub_format = if format.to_lowercase() == "vtt" { "vtt" } else { "srt" };

    let mut cmd = tokio::process::Command::new(ytdlp_bin);
    cmd.arg("--skip-download")
       .arg("--write-sub")
       .arg("--write-auto-sub")
       .arg("--sub-lang").arg(lang_code)
       .arg("--sub-format").arg(sub_format)
       .arg("--convert-subs").arg(sub_format);

    let output_template = if let Some(cf) = custom_filename {
        output_dir.join(format!("{}.%(ext)s", cf)).to_string_lossy().to_string()
    } else {
        output_dir.join("%(title)s.%(ext)s").to_string_lossy().to_string()
    };

    cmd.arg("-o").arg(&output_template);
    cmd.arg(video_url);

    let output = cmd.output().await.map_err(|e| format!("Failed to execute yt-dlp for subtitles: {}", e))?;
    if !output.status.success() {
        let err_text = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp subtitle download failed: {}", err_text));
    }

    // Locate the written subtitle file in output_dir
    let expected_ext = format!(".{}", sub_format);
    let mut matching_file = None;

    if let Ok(mut entries) = tokio::fs::read_dir(output_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let p = entry.path();
            if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                if ext.eq_ignore_ascii_case(sub_format) {
                    matching_file = Some(p);
                    break;
                }
            }
        }
    }

    matching_file.ok_or_else(|| format!("Subtitle file with extension {} was not found after download", expected_ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_youtube_playlist_url() {
        assert!(is_youtube_playlist_url("https://www.youtube.com/playlist?list=PLrAXtmErZgOdP_8GzKtK"));
        assert!(is_youtube_playlist_url("https://youtube.com/watch?v=dQw4w9WgXcQ&list=PLrAXtmErZgOdP_8GzKtK"));
        assert!(is_youtube_playlist_url("https://youtu.be/dQw4w9WgXcQ?list=PLrAXtmErZgOdP_8GzKtK"));

        assert!(!is_youtube_playlist_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(!is_youtube_playlist_url("https://example.com/playlist?list=12345"));
    }

    #[test]
    fn test_extract_playlist_id() {
        let id1 = extract_playlist_id("https://www.youtube.com/playlist?list=PLrAXtmErZgOdP_8GzKtK");
        assert_eq!(id1.as_deref(), Some("PLrAXtmErZgOdP_8GzKtK"));

        let id2 = extract_playlist_id("https://youtube.com/watch?v=123&list=MY_COOL_PLAYLIST_999&index=4");
        assert_eq!(id2.as_deref(), Some("MY_COOL_PLAYLIST_999"));

        let id3 = extract_playlist_id("https://youtube.com/watch?v=123");
        assert!(id3.is_none());
    }

    #[test]
    fn test_format_timestamps() {
        // 1 hour, 2 minutes, 3 seconds, 456 millis = 3600000 + 120000 + 3000 + 456 = 3723456 ms
        let ms = 3_723_456;
        assert_eq!(format_timestamp_srt(ms), "01:02:03,456");
        assert_eq!(format_timestamp_vtt(ms), "01:02:03.456");

        assert_eq!(format_timestamp_srt(0), "00:00:00,000");
        assert_eq!(format_timestamp_vtt(0), "00:00:00.000");
    }

    #[test]
    fn test_convert_timedtext_json3_to_srt_and_vtt() {
        let json3 = r#"{
            "events": [
                {
                    "tStartMs": 1500,
                    "dDurationMs": 3000,
                    "segs": [
                        { "utf8": "Hello World! " },
                        { "utf8": "Welcome to IDM." }
                    ]
                },
                {
                    "tStartMs": 5000,
                    "dDurationMs": 2500,
                    "segs": [
                        { "utf8": "This is subtitle track 2." }
                    ]
                }
            ]
        }"#;

        let srt = convert_timedtext_json3_to_srt(json3).expect("Should convert to SRT");
        assert!(srt.contains("1\n00:00:01,500 --> 00:00:04,500\nHello World! Welcome to IDM."));
        assert!(srt.contains("2\n00:00:05,000 --> 00:00:07,500\nThis is subtitle track 2."));

        let vtt = convert_timedtext_json3_to_vtt(json3).expect("Should convert to VTT");
        assert!(vtt.starts_with("WEBVTT\n\n"));
        assert!(vtt.contains("00:00:01.500 --> 00:00:04.500"));
    }
}
