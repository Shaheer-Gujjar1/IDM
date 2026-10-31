use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, PartialEq)]
pub struct DashRepresentation {
    pub id: String,
    pub bandwidth: u64,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub codecs: Option<String>,
    pub mime_type: Option<String>,
    pub init_url_template: Option<String>,
    pub media_url_template: Option<String>,
    pub start_number: u64,
    pub timescale: u64,
    pub duration: u64,
    pub segment_urls: Vec<String>,
    pub timeline_segments: Vec<(u64, u64)>, // (time, duration)
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashAdaptationSet {
    pub content_type: String, // "video", "audio", "text"
    pub mime_type: Option<String>,
    pub representations: Vec<DashRepresentation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashManifest {
    pub is_dynamic: bool,
    pub duration_seconds: f64,
    pub base_url: Option<String>,
    pub video_representations: Vec<DashRepresentation>,
    pub audio_representations: Vec<DashRepresentation>,
}

#[derive(Debug, Clone)]
pub struct DashDownloadPlan {
    pub source_url: String,
    pub video_init: Option<String>,
    pub video_segments: Vec<String>,
    pub audio_init: Option<String>,
    pub audio_segments: Vec<String>,
    pub duration_seconds: f64,
    pub is_dynamic: bool,
}

/// Checks if a given URL is a DASH (.mpd) stream
pub fn is_dash_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if parsed.path().to_lowercase().ends_with(".mpd") {
            return true;
        }
    }
    lower.contains(".mpd")
}

/// Parses ISO 8601 duration string like "PT1H2M3.45S" or "PT120S" into total seconds
pub fn parse_iso8601_duration(duration_str: &str) -> f64 {
    let s = duration_str.trim();
    if !s.starts_with('P') {
        return 0.0;
    }

    let mut total_secs = 0.0f64;
    let time_part = if let Some(idx) = s.find('T') {
        &s[idx + 1..]
    } else {
        ""
    };

    let mut current_num = String::new();
    for ch in time_part.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            current_num.push(ch);
        } else {
            let val: f64 = current_num.parse().unwrap_or(0.0);
            current_num.clear();
            match ch {
                'H' => total_secs += val * 3600.0,
                'M' => total_secs += val * 60.0,
                'S' => total_secs += val,
                _ => {}
            }
        }
    }

    total_secs
}

/// Helper to parse XML tag attributes into a key-value map
pub fn parse_xml_attributes(tag_content: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let re = regex::Regex::new(r#"([a-zA-Z0-9_:-]+)=(?:"([^"]*)"|'([^']*)')(?:\s|/|>|$)"#).unwrap();
    for cap in re.captures_iter(tag_content) {
        if let Some(key) = cap.get(1) {
            let val = cap.get(2).or_else(|| cap.get(3)).map(|m| m.as_str()).unwrap_or("");
            attrs.insert(key.as_str().to_string(), val.to_string());
        }
    }
    attrs
}

/// Replaces DASH segment template variables ($RepresentationID$, $Number$, $Number%0Nd$, $Time$, $Bandwidth$)
pub fn replace_template_variables(
    template: &str,
    rep_id: &str,
    number: Option<u64>,
    time: Option<u64>,
    bandwidth: Option<u64>,
) -> String {
    let mut result = template.replace("$RepresentationID$", rep_id);

    if let Some(bw) = bandwidth {
        result = result.replace("$Bandwidth$", &bw.to_string());
    }

    if let Some(t) = time {
        result = result.replace("$Time$", &t.to_string());
    }

    if let Some(n) = number {
        // Handle formatted $Number%0Nd$ like $Number%05d$
        let re_fmt = regex::Regex::new(r#"\$Number%0(\d+)d\$"#).unwrap();
        if let Some(cap) = re_fmt.captures(&result) {
            if let Some(width_str) = cap.get(1) {
                if let Ok(width) = width_str.as_str().parse::<usize>() {
                    let formatted = format!("{:0width$}", n, width = width);
                    let full_match = cap.get(0).unwrap().as_str();
                    result = result.replace(full_match, &formatted);
                }
            }
        }
        result = result.replace("$Number$", &n.to_string());
    }

    // Replace literal $$ with $
    result.replace("$$", "$")
}

/// Parses an MPEG-DASH XML manifest into a structured `DashManifest`
pub fn parse_dash_manifest(xml: &str, base_url: &str) -> Result<DashManifest, String> {
    let base = reqwest::Url::parse(base_url).map_err(|e| format!("Invalid base URL: {}", e))?;

    // Check root MPD tag
    let mpd_re = regex::Regex::new(r#"<MPD\b([^>]*)>"#).unwrap();
    let mpd_caps = mpd_re.captures(xml).ok_or_else(|| "Not a valid MPEG-DASH manifest (missing <MPD> root)".to_string())?;
    let mpd_attrs = parse_xml_attributes(mpd_caps.get(1).map(|m| m.as_str()).unwrap_or(""));

    let is_dynamic = mpd_attrs.get("type").map(|s| s.as_str()) == Some("dynamic");
    let duration_str = mpd_attrs.get("mediaPresentationDuration").map(|s| s.as_str()).unwrap_or("");
    let duration_seconds = parse_iso8601_duration(duration_str);

    // Global BaseURL if present
    let mut global_base_url = None;
    let base_url_re = regex::Regex::new(r#"<BaseURL\b[^>]*>([^<]+)</BaseURL>"#).unwrap();
    if let Some(caps) = base_url_re.captures(xml) {
        if let Some(bu) = caps.get(1) {
            let trimmed = bu.as_str().trim();
            if let Ok(abs) = base.join(trimmed) {
                global_base_url = Some(abs.to_string());
            }
        }
    }

    let effective_base = global_base_url.as_deref().unwrap_or(base_url);

    let mut video_representations = Vec::new();
    let mut audio_representations = Vec::new();

    // Iterate through AdaptationSets
    let adaptation_re = regex::Regex::new(r#"(?s)<AdaptationSet\b([^>]*)>(.*?)</AdaptationSet>"#).unwrap();
    for adapt_cap in adaptation_re.captures_iter(xml) {
        let adapt_attrs = parse_xml_attributes(adapt_cap.get(1).unwrap().as_str());
        let adapt_body = adapt_cap.get(2).unwrap().as_str();

        let mut content_type = adapt_attrs.get("contentType").cloned().unwrap_or_default();
        let mime_type = adapt_attrs.get("mimeType").cloned();

        if content_type.is_empty() {
            if let Some(ref mt) = mime_type {
                if mt.starts_with("video") {
                    content_type = "video".to_string();
                } else if mt.starts_with("audio") {
                    content_type = "audio".to_string();
                }
            }
        }

        // Shared SegmentTemplate at AdaptationSet level if present
        let mut shared_segment_template = None;
        let seg_temp_re = regex::Regex::new(r#"<SegmentTemplate\b([^>]*?)(?:/>|>(.*?)</SegmentTemplate>)"#).unwrap();
        if let Some(st_cap) = seg_temp_re.captures(adapt_body) {
            let st_attrs = parse_xml_attributes(st_cap.get(1).unwrap().as_str());
            let st_inner = st_cap.get(2).map(|m| m.as_str()).unwrap_or("");
            shared_segment_template = Some((st_attrs, st_inner.to_string()));
        }

        // Parse Representations within this AdaptationSet
        let rep_re = regex::Regex::new(r#"(?s)<Representation\b([^>]*?)(?:/>|>(.*?)</Representation>)"#).unwrap();
        for rep_cap in rep_re.captures_iter(adapt_body) {
            let rep_attrs = parse_xml_attributes(rep_cap.get(1).unwrap().as_str());
            let rep_body = rep_cap.get(2).map(|m| m.as_str()).unwrap_or("");

            let id = rep_attrs.get("id").cloned().unwrap_or_else(|| "1".to_string());
            let bandwidth = rep_attrs.get("bandwidth").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let width = rep_attrs.get("width").and_then(|v| v.parse::<u64>().ok());
            let height = rep_attrs.get("height").and_then(|v| v.parse::<u64>().ok());
            let codecs = rep_attrs.get("codecs").cloned();
            let rep_mime = rep_attrs.get("mimeType").cloned().or_else(|| mime_type.clone());

            if content_type.is_empty() {
                if width.is_some() || height.is_some() {
                    content_type = "video".to_string();
                } else if let Some(ref m) = rep_mime {
                    if m.starts_with("video") {
                        content_type = "video".to_string();
                    } else if m.starts_with("audio") {
                        content_type = "audio".to_string();
                    }
                }
            }

            // Local or shared SegmentTemplate
            let (active_st_attrs, active_st_body) = if let Some(st_cap) = seg_temp_re.captures(rep_body) {
                let st_attrs = parse_xml_attributes(st_cap.get(1).unwrap().as_str());
                let st_inner = st_cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                (Some(st_attrs), st_inner)
            } else if let Some((ref shared_attrs, ref shared_body)) = shared_segment_template {
                (Some(shared_attrs.clone()), shared_body.clone())
            } else {
                (None, String::new())
            };

            let mut init_url_template = None;
            let mut media_url_template = None;
            let mut start_number = 1u64;
            let mut timescale = 1u64;
            let mut segment_duration = 0u64;
            let mut timeline_segments = Vec::new();

            if let Some(st) = active_st_attrs {
                init_url_template = st.get("initialization").cloned();
                media_url_template = st.get("media").cloned();
                if let Some(sn) = st.get("startNumber").and_then(|v| v.parse::<u64>().ok()) {
                    start_number = sn;
                }
                if let Some(ts) = st.get("timescale").and_then(|v| v.parse::<u64>().ok()) {
                    timescale = ts;
                }
                if let Some(d) = st.get("duration").and_then(|v| v.parse::<u64>().ok()) {
                    segment_duration = d;
                }

                // Check SegmentTimeline `<S t="..." d="..." r="..."/>`
                if !active_st_body.is_empty() {
                    let s_re = regex::Regex::new(r#"<S\b([^>]*?)/>"#).unwrap();
                    let mut current_time = 0u64;
                    for s_cap in s_re.captures_iter(&active_st_body) {
                        let s_attrs = parse_xml_attributes(s_cap.get(1).unwrap().as_str());
                        if let Some(t) = s_attrs.get("t").and_then(|v| v.parse::<u64>().ok()) {
                            current_time = t;
                        }
                        let d = s_attrs.get("d").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                        let r = s_attrs.get("r").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);

                        timeline_segments.push((current_time, d));
                        current_time += d;

                        if r > 0 {
                            for _ in 0..r {
                                timeline_segments.push((current_time, d));
                                current_time += d;
                            }
                        }
                    }
                }
            }

            // Direct SegmentList parsing `<SegmentList>`
            let mut segment_urls = Vec::new();
            let seg_list_re = regex::Regex::new(r#"(?s)<SegmentList\b[^>]*>(.*?)</SegmentList>"#).unwrap();
            if let Some(sl_cap) = seg_list_re.captures(rep_body) {
                let sl_inner = sl_cap.get(1).unwrap().as_str();
                let init_re = regex::Regex::new(r#"<Initialization\b([^>]*?)/>"#).unwrap();
                if let Some(init_cap) = init_re.captures(sl_inner) {
                    let init_attrs = parse_xml_attributes(init_cap.get(1).unwrap().as_str());
                    if let Some(src) = init_attrs.get("sourceURL") {
                        init_url_template = Some(src.clone());
                    }
                }

                let seg_url_re = regex::Regex::new(r#"<SegmentURL\b([^>]*?)/>"#).unwrap();
                for surl_cap in seg_url_re.captures_iter(sl_inner) {
                    let surl_attrs = parse_xml_attributes(surl_cap.get(1).unwrap().as_str());
                    if let Some(m) = surl_attrs.get("media") {
                        segment_urls.push(m.clone());
                    }
                }
            }

            let rep = DashRepresentation {
                id,
                bandwidth,
                width,
                height,
                codecs,
                mime_type: rep_mime,
                init_url_template,
                media_url_template,
                start_number,
                timescale,
                duration: segment_duration,
                segment_urls,
                timeline_segments,
                base_url: Some(effective_base.to_string()),
            };

            if content_type == "video" {
                video_representations.push(rep);
            } else if content_type == "audio" {
                audio_representations.push(rep);
            } else if width.is_some() || height.is_some() {
                video_representations.push(rep);
            } else {
                audio_representations.push(rep);
            }
        }
    }

    // Sort representations by bandwidth descending (highest quality first)
    video_representations.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
    audio_representations.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));

    Ok(DashManifest {
        is_dynamic,
        duration_seconds,
        base_url: Some(effective_base.to_string()),
        video_representations,
        audio_representations,
    })
}

/// Resolves an optimal `DashDownloadPlan` (picking best video and best audio)
pub fn resolve_dash_plan(manifest: &DashManifest, original_url: &str) -> Result<DashDownloadPlan, String> {
    let base = reqwest::Url::parse(manifest.base_url.as_deref().unwrap_or(original_url))
        .map_err(|e| format!("Invalid base URL: {}", e))?;

    // Helper to generate full URLs for a representation
    let build_urls_for_rep = |rep: &DashRepresentation| -> (Option<String>, Vec<String>) {
        let rep_base = rep.base_url.as_deref()
            .and_then(|b| reqwest::Url::parse(b).ok())
            .unwrap_or_else(|| base.clone());

        let init_url = rep.init_url_template.as_ref().map(|tmpl| {
            let replaced = replace_template_variables(tmpl, &rep.id, None, None, Some(rep.bandwidth));
            rep_base.join(&replaced).map(|u| u.to_string()).unwrap_or(replaced)
        });

        let mut segment_urls = Vec::new();

        if !rep.segment_urls.is_empty() {
            for rel in &rep.segment_urls {
                let resolved = rep_base.join(rel).map(|u| u.to_string()).unwrap_or_else(|_| rel.clone());
                segment_urls.push(resolved);
            }
        } else if let Some(ref media_tmpl) = rep.media_url_template {
            if !rep.timeline_segments.is_empty() {
                let mut current_num = rep.start_number;
                for (time, _) in &rep.timeline_segments {
                    let replaced = replace_template_variables(
                        media_tmpl,
                        &rep.id,
                        Some(current_num),
                        Some(*time),
                        Some(rep.bandwidth),
                    );
                    let resolved = rep_base.join(&replaced).map(|u| u.to_string()).unwrap_or(replaced);
                    segment_urls.push(resolved);
                    current_num += 1;
                }
            } else if rep.duration > 0 && rep.timescale > 0 && manifest.duration_seconds > 0.0 {
                let seg_duration_secs = rep.duration as f64 / rep.timescale as f64;
                let total_segments = (manifest.duration_seconds / seg_duration_secs).ceil() as u64;
                for i in 0..total_segments {
                    let num = rep.start_number + i;
                    let time = i * rep.duration;
                    let replaced = replace_template_variables(
                        media_tmpl,
                        &rep.id,
                        Some(num),
                        Some(time),
                        Some(rep.bandwidth),
                    );
                    let resolved = rep_base.join(&replaced).map(|u| u.to_string()).unwrap_or(replaced);
                    segment_urls.push(resolved);
                }
            }
        }

        (init_url, segment_urls)
    };

    let (video_init, video_segments) = if let Some(best_video) = manifest.video_representations.first() {
        build_urls_for_rep(best_video)
    } else {
        (None, Vec::new())
    };

    let (audio_init, audio_segments) = if let Some(best_audio) = manifest.audio_representations.first() {
        build_urls_for_rep(best_audio)
    } else {
        (None, Vec::new())
    };

    if video_segments.is_empty() && audio_segments.is_empty() {
        return Err("No downloadable media segments could be resolved from DASH manifest".to_string());
    }

    Ok(DashDownloadPlan {
        source_url: original_url.to_string(),
        video_init,
        video_segments,
        audio_init,
        audio_segments,
        duration_seconds: manifest.duration_seconds,
        is_dynamic: manifest.is_dynamic,
    })
}

/// Downloads a sequence of segment URLs into a single concatenated media file
async fn download_segments_to_file(
    client: reqwest::Client,
    init_url: Option<String>,
    segment_urls: Vec<String>,
    dest_path: &Path,
    downloaded_tracker: Arc<AtomicU64>,
    aborted_flag: Arc<AtomicBool>,
) -> Result<u64, String> {
    let mut file = tokio::fs::File::create(dest_path)
        .await
        .map_err(|e| format!("Cannot create file {:?}: {}", dest_path, e))?;

    let mut total_written = 0u64;

    // 1. Download Initialization Segment first if present
    if let Some(init) = init_url {
        if aborted_flag.load(Ordering::Relaxed) {
            return Err("DASH transfer cancelled".to_string());
        }
        let resp = client.get(&init)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("Init segment download error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Init segment HTTP error: {}", resp.status()));
        }

        let bytes = resp.bytes().await.map_err(|e| format!("Init segment read error: {}", e))?;
        file.write_all(&bytes).await.map_err(|e| e.to_string())?;
        total_written += bytes.len() as u64;
        downloaded_tracker.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    }

    // 2. Download media segments in sequence with concurrency pool
    let total_segments = segment_urls.len();
    for (idx, seg_url) in segment_urls.into_iter().enumerate() {
        if aborted_flag.load(Ordering::Relaxed) {
            return Err("DASH transfer cancelled".to_string());
        }

        let mut success = false;
        let mut last_err = String::new();

        for attempt in 0..4 {
            let req = client.get(&seg_url).timeout(Duration::from_secs(30));
            match req.send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let bytes = match resp.bytes().await {
                            Ok(b) => b,
                            Err(e) => {
                                last_err = format!("Read error: {}", e);
                                tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
                                continue;
                            }
                        };

                        file.write_all(&bytes).await.map_err(|e| e.to_string())?;
                        total_written += bytes.len() as u64;
                        downloaded_tracker.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        success = true;
                        break;
                    } else {
                        last_err = format!("HTTP {}", resp.status());
                    }
                }
                Err(e) => {
                    last_err = format!("Network error: {}", e);
                }
            }

            tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
        }

        if !success {
            aborted_flag.store(true, Ordering::Relaxed);
            return Err(format!("DASH segment {}/{} failed: {}", idx + 1, total_segments, last_err));
        }
    }

    file.flush().await.map_err(|e| e.to_string())?;
    Ok(total_written)
}

/// Executes full MPEG-DASH download, downloads video and audio tracks, and muxes to MP4 using FFmpeg
pub async fn run_dash_stream_download(
    client: reqwest::Client,
    plan: DashDownloadPlan,
    temp_dir: PathBuf,
    final_output_path: String,
    downloaded_tracker: Arc<AtomicU64>,
    total_size_tracker: Arc<AtomicU64>,
    mut abort_rx: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<String, String> {
    let _ = tokio::fs::create_dir_all(&temp_dir).await;
    let aborted_flag = Arc::new(AtomicBool::new(false));

    // Listen for abort in background
    let abort_flag_clone = aborted_flag.clone();
    if let Some(mut rx) = abort_rx.take() {
        tokio::spawn(async move {
            if rx.recv().await.is_ok() {
                abort_flag_clone.store(true, Ordering::Relaxed);
            }
        });
    }

    let has_video = !plan.video_segments.is_empty();
    let has_audio = !plan.audio_segments.is_empty();

    let temp_video_file = temp_dir.join("dash_video.mp4");
    let temp_audio_file = temp_dir.join("dash_audio.m4a");

    let client_video = client.clone();
    let dl_tracker_video = downloaded_tracker.clone();
    let abort_video = aborted_flag.clone();

    // 1. Download Video Track
    if has_video {
        download_segments_to_file(
            client_video,
            plan.video_init,
            plan.video_segments,
            &temp_video_file,
            dl_tracker_video,
            abort_video,
        ).await?;
    }

    // 2. Download Audio Track
    if has_audio {
        let client_audio = client.clone();
        let dl_tracker_audio = downloaded_tracker.clone();
        let abort_audio = aborted_flag.clone();

        download_segments_to_file(
            client_audio,
            plan.audio_init,
            plan.audio_segments,
            &temp_audio_file,
            dl_tracker_audio,
            abort_audio,
        ).await?;
    }

    let final_dest = PathBuf::from(&final_output_path);
    if let Some(parent) = final_dest.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    // 3. Mux or Copy
    if has_video && has_audio {
        if let Some(ffmpeg_bin) = crate::mux::probe_ffmpeg() {
            println!("[DASH Engine] Muxing video and audio tracks via FFmpeg: {}", final_output_path);
            let mut cmd = tokio::process::Command::new(ffmpeg_bin);
            cmd.arg("-y")
               .arg("-i").arg(&temp_video_file)
               .arg("-i").arg(&temp_audio_file)
               .arg("-c").arg("copy")
               .arg("-movflags").arg("+faststart")
               .arg(&final_output_path);

            let output = cmd.output().await.map_err(|e| format!("FFmpeg execution failed: {}", e))?;
            if !output.status.success() {
                // Fallback: copy video file
                let _ = tokio::fs::copy(&temp_video_file, &final_dest).await;
            }
        } else {
            // No FFmpeg: copy video file
            let _ = tokio::fs::copy(&temp_video_file, &final_dest).await;
        }
    } else if has_video {
        let _ = tokio::fs::copy(&temp_video_file, &final_dest).await;
    } else if has_audio {
        let _ = tokio::fs::copy(&temp_audio_file, &final_dest).await;
    }

    // Update total size
    if let Ok(meta) = tokio::fs::metadata(&final_dest).await {
        total_size_tracker.store(meta.len(), Ordering::Relaxed);
    }

    // Cleanup temp files
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    Ok(final_output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_dash_url() {
        assert!(is_dash_url("https://example.com/live/manifest.mpd"));
        assert!(is_dash_url("https://dash.akamaized.net/akamai/test/sample.mpd?token=xyz"));
        assert!(!is_dash_url("https://example.com/playlist.m3u8"));
        assert!(!is_dash_url("https://example.com/video.mp4"));
    }

    #[test]
    fn test_parse_iso8601_duration() {
        assert_eq!(parse_iso8601_duration("PT1H2M3.5S"), 3723.5);
        assert_eq!(parse_iso8601_duration("PT30M"), 1800.0);
        assert_eq!(parse_iso8601_duration("PT45S"), 45.0);
        assert_eq!(parse_iso8601_duration("PT1H"), 3600.0);
        assert_eq!(parse_iso8601_duration("invalid"), 0.0);
    }

    #[test]
    fn test_replace_template_variables() {
        let tmpl = "chunk_$RepresentationID$_$Number%05d$.m4s";
        let replaced = replace_template_variables(tmpl, "video_1080p", Some(42), None, None);
        assert_eq!(replaced, "chunk_video_1080p_00042.m4s");

        let tmpl_time = "seg_$RepresentationID$_$Time$.m4s";
        let replaced_time = replace_template_variables(tmpl_time, "audio_en", None, Some(96000), None);
        assert_eq!(replaced_time, "seg_audio_en_96000.m4s");
    }

    #[test]
    fn test_parse_dash_manifest_and_resolve_plan() {
        let sample_mpd = r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT30S">
    <BaseURL>https://cdn.example.com/dash/</BaseURL>
    <Period duration="PT30S">
        <AdaptationSet mimeType="video/mp4" contentType="video">
            <SegmentTemplate timescale="1000" duration="10000" initialization="init-$RepresentationID$.mp4" media="chunk-$RepresentationID$-$Number%03d$.m4s" startNumber="1"/>
            <Representation id="v720p" bandwidth="1500000" width="1280" height="720"/>
            <Representation id="v1080p" bandwidth="3000000" width="1920" height="1080"/>
        </AdaptationSet>
        <AdaptationSet mimeType="audio/mp4" contentType="audio">
            <SegmentTemplate timescale="1000" duration="10000" initialization="audio-init.mp4" media="audio-$Number$.m4s" startNumber="1"/>
            <Representation id="a128k" bandwidth="128000"/>
        </AdaptationSet>
    </Period>
</MPD>"#;

        let manifest = parse_dash_manifest(sample_mpd, "https://cdn.example.com/dash/manifest.mpd").unwrap();
        assert!(!manifest.is_dynamic);
        assert_eq!(manifest.duration_seconds, 30.0);
        assert_eq!(manifest.video_representations.len(), 2);
        // Best quality video sorted first
        assert_eq!(manifest.video_representations[0].id, "v1080p");
        assert_eq!(manifest.video_representations[0].bandwidth, 3000000);
        assert_eq!(manifest.audio_representations.len(), 1);

        let plan = resolve_dash_plan(&manifest, "https://cdn.example.com/dash/manifest.mpd").unwrap();
        assert_eq!(plan.video_init.as_deref(), Some("https://cdn.example.com/dash/init-v1080p.mp4"));
        assert_eq!(plan.audio_init.as_deref(), Some("https://cdn.example.com/dash/audio-init.mp4"));
        // 30 seconds / 10s per segment = 3 segments
        assert_eq!(plan.video_segments.len(), 3);
        assert_eq!(plan.video_segments[0], "https://cdn.example.com/dash/chunk-v1080p-001.m4s");
        assert_eq!(plan.video_segments[1], "https://cdn.example.com/dash/chunk-v1080p-002.m4s");
        assert_eq!(plan.video_segments[2], "https://cdn.example.com/dash/chunk-v1080p-003.m4s");
        assert_eq!(plan.audio_segments.len(), 3);
    }
}
