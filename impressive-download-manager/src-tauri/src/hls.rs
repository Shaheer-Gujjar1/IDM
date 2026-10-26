use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, PartialEq)]
pub struct HlsVariant {
    pub url: String,
    pub bandwidth: u64,
    pub resolution: Option<String>,
    pub audio_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsMediaPlan {
    pub urls: Vec<String>,
    pub duration: f64,
    pub extension: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HlsParseResult {
    Master(Vec<HlsVariant>),
    Media(HlsMediaPlan),
}

#[derive(Debug, Clone)]
pub struct HlsPlan {
    pub urls: Vec<String>,
    pub duration: f64,
    pub extension: String,
    pub source: String,
    pub audio_plan: Option<Box<HlsPlan>>,
}

/// Helper to parse key-value attributes from HLS tags like #EXT-X-STREAM-INF:BANDWIDTH=128000,RESOLUTION=1920x1080
pub fn parse_attributes(line: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let re = regex::Regex::new(r#"([A-Z0-9-]+)=(?:"([^"]*)"|([^,]*))"#).unwrap();
    for cap in re.captures_iter(line) {
        if let Some(key) = cap.get(1) {
            let val = cap.get(2).or_else(|| cap.get(3)).map(|m| m.as_str()).unwrap_or("");
            attrs.insert(key.as_str().to_string(), val.to_string());
        }
    }
    attrs
}

/// Strips sensitive headers (Cookie, Authorization) on cross-origin redirects or segment CDNs
pub fn is_same_origin(source: &str, target: &str) -> bool {
    if let (Ok(u1), Ok(u2)) = (reqwest::Url::parse(source), reqwest::Url::parse(target)) {
        u1.origin() == u2.origin()
    } else {
        false
    }
}

/// Parses an HLS manifest text, resolving relative URIs against base_url
pub fn parse_hls(text: &str, base_url: &str) -> Result<HlsParseResult, String> {
    let base = reqwest::Url::parse(base_url).map_err(|e| format!("Invalid base URL: {}", e))?;

    if !text.trim_start().starts_with("#EXTM3U") {
        return Err("The server did not return a valid HLS playlist (missing #EXTM3U).".to_string());
    }

    let mut lines = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty());
    let mut variants = Vec::new();
    let mut segments = Vec::new();
    let mut audio_map: HashMap<String, String> = HashMap::new();

    let mut current_variant_attrs: Option<HashMap<String, String>> = None;
    let mut total_duration = 0.0f64;
    let mut init_map_url: Option<String> = None;
    let mut is_encrypted = false;
    let mut has_endlist = false;

    while let Some(line) = lines.next() {
        if line.starts_with("#EXT-X-MEDIA:") {
            let attrs = parse_attributes(line);
            let media_type = attrs.get("TYPE").map(|s| s.as_str()).unwrap_or("");
            let group_id = attrs.get("GROUP-ID").cloned();
            let uri = attrs.get("URI").cloned();
            let is_default = attrs.get("DEFAULT").map(|s| s.as_str()) == Some("YES");

            if media_type == "AUDIO" {
                if let (Some(gid), Some(rel_uri)) = (group_id, uri) {
                    if is_default || !audio_map.contains_key(&gid) {
                        if let Ok(abs_url) = base.join(&rel_uri) {
                            audio_map.insert(gid, abs_url.to_string());
                        }
                    }
                }
            }
        } else if line.starts_with("#EXT-X-STREAM-INF:") {
            current_variant_attrs = Some(parse_attributes(line));
        } else if line.starts_with("#EXT-X-KEY:") {
            let attrs = parse_attributes(line);
            if attrs.get("METHOD").map(|s| s.as_str()) != Some("NONE") {
                is_encrypted = true;
            }
        } else if line.starts_with("#EXT-X-MAP:") {
            let attrs = parse_attributes(line);
            if let Some(uri) = attrs.get("URI") {
                if let Ok(abs_map) = base.join(uri) {
                    init_map_url = Some(abs_map.to_string());
                }
            }
        } else if line.starts_with("#EXT-X-ENDLIST") {
            has_endlist = true;
        } else if line.starts_with("#EXTINF:") {
            let dur_str = line.trim_start_matches("#EXTINF:").split(',').next().unwrap_or("0");
            if let Ok(dur) = dur_str.parse::<f64>() {
                total_duration += dur;
            }
        } else if !line.starts_with('#') {
            // URL line
            let abs_url = base.join(line).map_err(|e| format!("Invalid segment URL {}: {}", line, e))?.to_string();

            if let Some(attrs) = current_variant_attrs.take() {
                let bw = attrs.get("BANDWIDTH").and_then(|b| b.parse::<u64>().ok()).unwrap_or(0);
                let res = attrs.get("RESOLUTION").cloned();
                let audio_grp = attrs.get("AUDIO").cloned();
                let audio_url = audio_grp.and_then(|g| audio_map.get(&g).cloned());

                variants.push(HlsVariant {
                    url: abs_url,
                    bandwidth: bw,
                    resolution: res,
                    audio_url,
                });
            } else {
                segments.push(abs_url);
            }
        }
    }

    if is_encrypted {
        return Err("Encrypted HLS stream is not supported yet.".to_string());
    }

    if !variants.is_empty() {
        // Sort highest bandwidth first
        variants.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
        return Ok(HlsParseResult::Master(variants));
    }

    if !has_endlist {
        return Err("Live HLS streams are not supported. Only On-Demand videos (#EXT-X-ENDLIST) can be downloaded.".to_string());
    }

    if segments.is_empty() {
        return Err("HLS playlist contains 0 media segments.".to_string());
    }

    if segments.len() > 20000 {
        return Err("HLS playlist exceeds safety limit of 20,000 segments.".to_string());
    }

    let mut urls = Vec::with_capacity(segments.len() + 1);
    let extension = if let Some(map_url) = init_map_url {
        urls.push(map_url);
        ".mp4".to_string()
    } else {
        ".ts".to_string()
    };
    urls.extend(segments);

    Ok(HlsParseResult::Media(HlsMediaPlan {
        urls,
        duration: total_duration,
        extension,
    }))
}

/// Recursively resolves master playlists down to media playlists and audio tracks
pub async fn resolve_hls(
    client: &reqwest::Client,
    url: &str,
    cookie: &str,
    user_agent: &str,
    depth: usize,
    source_url: &str,
) -> Result<HlsPlan, String> {
    if depth > 3 {
        return Err("Too many nested HLS playlists (>3).".to_string());
    }

    let mut req = client.get(url).timeout(Duration::from_secs(15));
    if is_same_origin(source_url, url) && !cookie.is_empty() {
        req = req.header(reqwest::header::COOKIE, cookie);
    }
    if !user_agent.is_empty() {
        req = req.header(reqwest::header::USER_AGENT, user_agent);
    }

    let res = req.send().await.map_err(|e| format!("Failed to fetch HLS manifest: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("HLS manifest request failed with HTTP {}", res.status()));
    }

    let final_url = res.url().to_string();
    let text = res.text().await.map_err(|e| format!("Failed to read HLS manifest text: {}", e))?;

    let parsed = parse_hls(&text, &final_url)?;

    match parsed {
        HlsParseResult::Master(variants) => {
            if variants.is_empty() {
                return Err("Master playlist contains no usable variants.".to_string());
            }
            // Pick highest quality variant
            let top_variant = &variants[0];
            let mut video_plan = Box::pin(resolve_hls(client, &top_variant.url, cookie, user_agent, depth + 1, source_url)).await?;

            if let Some(ref audio_url) = top_variant.audio_url {
                if let Ok(audio_plan) = Box::pin(resolve_hls(client, audio_url, cookie, user_agent, depth + 1, source_url)).await {
                    video_plan.audio_plan = Some(Box::new(audio_plan));
                }
            }
            Ok(video_plan)
        }
        HlsParseResult::Media(media) => {
            Ok(HlsPlan {
                urls: media.urls,
                duration: media.duration,
                extension: media.extension,
                source: source_url.to_string(),
                audio_plan: None,
            })
        }
    }
}

/// Runs concurrent HLS segment downloader and sequential assembler
pub async fn run_hls_stream_download(
    client: reqwest::Client,
    plan: HlsPlan,
    temp_dir: std::path::PathBuf,
    target_save_path: String,
    cookie: String,
    user_agent: String,
    downloaded_tracker: Arc<AtomicU64>,
    total_size_tracker: Arc<AtomicU64>,
    _abort_rx: Option<tokio::sync::broadcast::Sender<()>>,
) -> Result<String, String> {
    tokio::fs::create_dir_all(&temp_dir).await.map_err(|e| e.to_string())?;

    let total_segments = plan.urls.len();
    let concurrency = 6usize.min(total_segments);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let aborted_flag = Arc::new(AtomicBool::new(false));

    let mut join_handles = Vec::with_capacity(total_segments);

    for (index, segment_url) in plan.urls.into_iter().enumerate() {
        let sem = semaphore.clone();
        let client_clone = client.clone();
        let cookie_clone = cookie.clone();
        let ua_clone = user_agent.clone();
        let source_clone = plan.source.clone();
        let temp_dir_clone = temp_dir.clone();
        let downloaded_ref = downloaded_tracker.clone();
        let is_aborted = aborted_flag.clone();

        let handle = tokio::spawn(async move {
            let _permit = match sem.acquire().await {
                Ok(p) => p,
                Err(_) => return Err("Worker semaphore cancelled".to_string()),
            };

            if is_aborted.load(Ordering::Relaxed) {
                return Err("Download aborted".to_string());
            }

            let part_path = temp_dir_clone.join(format!("{}.part", index));
            if part_path.exists() {
                if let Ok(meta) = tokio::fs::metadata(&part_path).await {
                    downloaded_ref.fetch_add(meta.len(), Ordering::Relaxed);
                    return Ok(());
                }
            }

            // Retry up to 4 attempts with exponential backoff
            let mut last_err = String::new();
            for attempt in 0..4 {
                if is_aborted.load(Ordering::Relaxed) {
                    return Err("Aborted".to_string());
                }

                let mut req = client_clone.get(&segment_url).timeout(Duration::from_secs(30));
                if is_same_origin(&source_clone, &segment_url) && !cookie_clone.is_empty() {
                    req = req.header(reqwest::header::COOKIE, &cookie_clone);
                }
                if !ua_clone.is_empty() {
                    req = req.header(reqwest::header::USER_AGENT, &ua_clone);
                }

                match req.send().await {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            let bytes = match resp.bytes().await {
                                Ok(b) => b,
                                Err(e) => {
                                    last_err = format!("Chunk stream error: {}", e);
                                    let backoff = 200 * (1 << attempt);
                                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                                    continue;
                                }
                            };

                            let bytes_len = bytes.len() as u64;
                            if let Ok(mut f) = tokio::fs::File::create(&part_path).await {
                                if f.write_all(&bytes).await.is_ok() && f.flush().await.is_ok() {
                                    downloaded_ref.fetch_add(bytes_len, Ordering::Relaxed);
                                    return Ok(());
                                }
                            }
                        } else {
                            last_err = format!("HTTP {}", resp.status());
                        }
                    }
                    Err(e) => {
                        last_err = format!("Network error: {}", e);
                    }
                }

                let backoff = 200 * (1 << attempt);
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }

            is_aborted.store(true, Ordering::Relaxed);
            Err(format!("Segment {} failed after 4 retries: {}", index, last_err))
        });

        join_handles.push(handle);
    }

    // Wait for all segment workers to finish
    for handle in join_handles {
        match handle.await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                aborted_flag.store(true, Ordering::Relaxed);
                return Err(format!("HLS segment download failure: {}", e));
            }
            Err(e) => {
                aborted_flag.store(true, Ordering::Relaxed);
                return Err(format!("Worker task panicked: {}", e));
            }
        }
    }

    // Sequential Assembly: stitch 0..total_segments in chronological order into output file
    let total_bytes_downloaded = downloaded_tracker.load(Ordering::Relaxed);
    total_size_tracker.store(total_bytes_downloaded, Ordering::Relaxed);

    let (final_path, mut out_file) = crate::engine::DownloadManager::allocate_collision_free_path(&target_save_path);

    let assembly_res = tokio::task::spawn_blocking(move || -> Result<(), String> {
        use std::io::{Read, Write};
        let mut buf = vec![0u8; 1024 * 1024 * 2]; // 2MB stream assembly buffer

        for i in 0..total_segments {
            let part_path = temp_dir.join(format!("{}.part", i));
            let mut part_file = std::fs::File::open(&part_path).map_err(|e| format!("Missing segment {}: {}", i, e))?;
            loop {
                let n = part_file.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 { break; }
                out_file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            }
        }

        let _ = out_file.sync_all();
        drop(out_file);

        // Clean up temporary segment part files
        for i in 0..total_segments {
            let _ = std::fs::remove_file(temp_dir.join(format!("{}.part", i)));
        }
        let _ = std::fs::remove_dir(&temp_dir);
        Ok(())
    }).await;

    match assembly_res {
        Ok(Ok(_)) => Ok(final_path),
        Ok(Err(e)) => Err(format!("Assembly failed: {}", e)),
        Err(_) => Err("Assembly task panicked".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hls_master_playlist() {
        let master_manifest = r#"#EXTM3U
#EXT-X-VERSION:4
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio-high",NAME="English",DEFAULT=YES,URI="audio/en.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,AUDIO="audio-high"
1080p/prog_index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720,AUDIO="audio-high"
720p/prog_index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=854x480,AUDIO="audio-high"
480p/prog_index.m3u8"#;

        let base = "https://streaming.test/manifest.m3u8";
        let res = parse_hls(master_manifest, base).expect("Master playlist parse failed");

        if let HlsParseResult::Master(variants) = res {
            assert_eq!(variants.len(), 3);
            assert_eq!(variants[0].bandwidth, 5000000);
            assert_eq!(variants[0].resolution, Some("1920x1080".to_string()));
            assert_eq!(variants[0].url, "https://streaming.test/1080p/prog_index.m3u8");
            assert_eq!(variants[0].audio_url, Some("https://streaming.test/audio/en.m3u8".to_string()));
        } else {
            panic!("Expected HlsParseResult::Master");
        }
    }

    #[test]
    fn test_parse_hls_media_playlist_ts_segments() {
        let media_manifest = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXTINF:9.009,
segment_000.ts
#EXTINF:9.009,
segment_001.ts
#EXTINF:8.000,
segment_002.ts
#EXT-X-ENDLIST"#;

        let base = "https://streaming.test/1080p/index.m3u8";
        let res = parse_hls(media_manifest, base).expect("Media playlist parse failed");

        if let HlsParseResult::Media(media) = res {
            assert_eq!(media.urls.len(), 3);
            assert_eq!(media.extension, ".ts");
            assert_eq!(media.urls[0], "https://streaming.test/1080p/segment_000.ts");
            assert_eq!(media.urls[1], "https://streaming.test/1080p/segment_001.ts");
            assert_eq!(media.urls[2], "https://streaming.test/1080p/segment_002.ts");
            assert!((media.duration - 26.018).abs() < 0.001);
        } else {
            panic!("Expected HlsParseResult::Media");
        }
    }

    #[test]
    fn test_parse_hls_media_playlist_fmp4_init_map() {
        let fmp4_manifest = r#"#EXTM3U
#EXT-X-VERSION:7
#EXT-X-TARGETDURATION:6
#EXT-X-MAP:URI="init.mp4"
#EXTINF:6.000,
chunk_0.m4s
#EXTINF:6.000,
chunk_1.m4s
#EXT-X-ENDLIST"#;

        let base = "https://cdn.test/video/manifest.m3u8";
        let res = parse_hls(fmp4_manifest, base).expect("fMP4 parse failed");

        if let HlsParseResult::Media(media) = res {
            assert_eq!(media.extension, ".mp4");
            // init.mp4 MUST be first segment
            assert_eq!(media.urls.len(), 3);
            assert_eq!(media.urls[0], "https://cdn.test/video/init.mp4");
            assert_eq!(media.urls[1], "https://cdn.test/video/chunk_0.m4s");
            assert_eq!(media.urls[2], "https://cdn.test/video/chunk_1.m4s");
        } else {
            panic!("Expected HlsParseResult::Media");
        }
    }

    #[test]
    fn test_parse_hls_security_rejections() {
        let base = "https://streaming.test/playlist.m3u8";

        // 1. Missing #EXTM3U
        assert!(parse_hls("Not an HLS playlist", base).is_err());

        // 2. Encrypted HLS stream (DRM/AES-128)
        let encrypted = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:10,\nseg1.ts\n#EXT-X-ENDLIST";
        assert!(parse_hls(encrypted, base).is_err());

        // 3. Live stream (missing #EXT-X-ENDLIST)
        let live = "#EXTM3U\n#EXTINF:10,\nseg1.ts\n#EXTINF:10,\nseg2.ts";
        assert!(parse_hls(live, base).is_err());
    }

    #[test]
    fn test_scoped_headers_cross_origin_isolation() {
        assert!(is_same_origin("https://example.com/playlist.m3u8", "https://example.com/segment1.ts"));
        assert!(!is_same_origin("https://example.com/playlist.m3u8", "https://cdn.s3.amazonaws.com/segment1.ts"));
        assert!(!is_same_origin("https://example.com/playlist.m3u8", "https://sub.example.com/segment1.ts"));
        assert!(!is_same_origin("https://example.com/playlist.m3u8", "http://example.com/segment1.ts"));
    }

    #[tokio::test]
    async fn test_working_e2e_hls_stream_download_and_assembly() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind listener");
        let local_addr = listener.local_addr().expect("Failed to get local address");

        // 3 distinct segment payloads
        let seg0 = b"SEGMENT_0_RAW_BYTES_MPEGTS_AAAA";
        let seg1 = b"SEGMENT_1_RAW_BYTES_MPEGTS_BBBB";
        let seg2 = b"SEGMENT_2_RAW_BYTES_MPEGTS_CCCC";

        let server_handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut req_buf = vec![0u8; 4096];
                    if let Ok(n) = stream.read(&mut req_buf).await {
                        let req_text = String::from_utf8_lossy(&req_buf[..n]);

                        if req_text.contains("GET /seg0.ts") {
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", seg0.len());
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(seg0).await;
                        } else if req_text.contains("GET /seg1.ts") {
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", seg1.len());
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(seg1).await;
                        } else if req_text.contains("GET /seg2.ts") {
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", seg2.len());
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(seg2).await;
                        }
                    }
                });
            }
        });

        let plan = HlsPlan {
            urls: vec![
                format!("http://{}/seg0.ts", local_addr),
                format!("http://{}/seg1.ts", local_addr),
                format!("http://{}/seg2.ts", local_addr),
            ],
            duration: 30.0,
            extension: ".ts".to_string(),
            source: format!("http://{}/master.m3u8", local_addr),
            audio_plan: None,
        };

        let temp_dir = std::env::temp_dir().join(format!("idm_hls_test_parts_{}", uuid::Uuid::new_v4()));
        let out_dir = std::env::temp_dir().join(format!("idm_hls_test_out_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&out_dir);
        let target_save_file = out_dir.join("stream_output.ts").to_string_lossy().to_string();

        let client = reqwest::Client::new();
        let downloaded = Arc::new(AtomicU64::new(0));
        let total = Arc::new(AtomicU64::new(0));

        let res = run_hls_stream_download(
            client,
            plan,
            temp_dir.clone(),
            target_save_file.clone(),
            String::new(),
            String::new(),
            downloaded.clone(),
            total.clone(),
            None,
        ).await;

        assert!(res.is_ok(), "HLS stream download must succeed: {:?}", res.err());
        let final_path = res.unwrap();

        // Verify output file exists and matches concatenated segments
        assert!(std::path::Path::new(&final_path).exists(), "Output assembled video must exist");
        let content = std::fs::read(&final_path).expect("Failed to read assembled stream");

        let mut expected = Vec::new();
        expected.extend_from_slice(seg0);
        expected.extend_from_slice(seg1);
        expected.extend_from_slice(seg2);

        assert_eq!(content, expected, "Assembled HLS video file must match concatenated segments byte-for-byte");

        // Verify temp directory was cleaned up
        assert!(!temp_dir.join("0.part").exists(), "Temporary segment files must be cleaned up after assembly");

        server_handle.abort();
        let _ = std::fs::remove_dir_all(&out_dir);
        let _ = std::fs::remove_file(&final_path);
    }
}
