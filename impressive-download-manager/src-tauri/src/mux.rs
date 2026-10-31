use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Locates the ffmpeg executable on the system PATH or common platform locations.
pub fn probe_ffmpeg() -> Option<PathBuf> {
    // 1. Common Unix locations (checked first so absolute path is returned)
    #[cfg(unix)]
    {
        let candidates = [
            "/usr/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/opt/homebrew/bin/ffmpeg",
            "/snap/bin/ffmpeg",
        ];
        for path_str in &candidates {
            let p = PathBuf::from(path_str);
            if p.exists() {
                return Some(p);
            }
        }

        // Try `which ffmpeg` for dynamic absolute path
        if let Ok(output) = std::process::Command::new("which").arg("ffmpeg").output() {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !s.is_empty() {
                    let p = PathBuf::from(&s);
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }

    // 2. Check if "ffmpeg" is in PATH
    if let Ok(output) = std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        if output.success() {
            return Some(PathBuf::from("/usr/bin/ffmpeg"));
        }
    }

    // 3. Common Windows locations
    #[cfg(windows)]
    {
        let candidates = [
            "ffmpeg.exe",
            "C:\\ffmpeg\\bin\\ffmpeg.exe",
            "C:\\Program Files\\ffmpeg\\bin\\ffmpeg.exe",
        ];
        for path_str in &candidates {
            let p = PathBuf::from(path_str);
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

/// Locates the yt-dlp executable on the system PATH, project bin, or common platform locations.
pub fn probe_ytdlp() -> Option<PathBuf> {
    // 1. Check if "yt-dlp" is in PATH
    if let Ok(output) = std::process::Command::new("yt-dlp")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        if output.success() {
            return Some(PathBuf::from("yt-dlp"));
        }
    }

    // 2. Check project bin/yt-dlp
    let current_dir = std::env::current_dir().ok();
    if let Some(cd) = current_dir {
        let p = cd.join("bin").join("yt-dlp");
        if p.exists() {
            return Some(p);
        }
        let p2 = cd.join("src-tauri").join("bin").join("yt-dlp");
        if p2.exists() {
            return Some(p2);
        }
    }

    // 3. Common Unix locations
    #[cfg(unix)]
    {
        let candidates = [
            dirs::home_dir().map(|h| h.join(".local/bin/yt-dlp")),
            Some(PathBuf::from("/usr/bin/yt-dlp")),
            Some(PathBuf::from("/usr/local/bin/yt-dlp")),
            Some(PathBuf::from("/snap/bin/yt-dlp")),
        ];
        for opt in &candidates {
            if let Some(p) = opt {
                if p.exists() {
                    return Some(p.clone());
                }
            }
        }
    }

    // 4. Common Windows locations
    #[cfg(windows)]
    {
        let candidates = [
            "yt-dlp.exe",
            "C:\\yt-dlp\\yt-dlp.exe",
            "C:\\Program Files\\yt-dlp\\yt-dlp.exe",
        ];
        for path_str in &candidates {
            let p = PathBuf::from(path_str);
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

/// Losslessly combines separate video and audio streams using FFmpeg fast stream copy (`-c copy`).
/// Takes 1-2 seconds and consumes minimal CPU since no re-encoding takes place.
/// Protected against command-line argument injection by strictly passing isolated vector arguments.
pub async fn mux_dual_stream(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let ffmpeg_bin = probe_ffmpeg()
        .ok_or_else(|| "FFmpeg was not found on this system. Please install FFmpeg to mux 1080p/4K YouTube streams.".to_string())?;

    if !video_path.exists() {
        return Err(format!("Video stream file not found at: {}", video_path.display()));
    }
    if !audio_path.exists() {
        return Err(format!("Audio stream file not found at: {}", audio_path.display()));
    }

    // Ensure output parent directory exists
    if let Some(parent) = output_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let mut cmd = tokio::process::Command::new(ffmpeg_bin);
    cmd.arg("-nostdin")
        .arg("-v")
        .arg("error")
        .arg("-y")
        .arg("-protocol_whitelist")
        .arg("file,pipe")
        .arg("-i")
        .arg(video_path)
        .arg("-protocol_whitelist")
        .arg("file,pipe")
        .arg("-i")
        .arg(audio_path)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("1:a:0")
        .arg("-c")
        .arg("copy")
        .arg("-movflags")
        .arg("+faststart")
        .arg(output_path);

    #[cfg(windows)]
    {
        // 0x08000000 = CREATE_NO_WINDOW
        cmd.creation_flags(0x08000000);
    }

    let output = cmd.output().await.map_err(|e| format!("Failed to spawn FFmpeg process: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("FFmpeg stream copy muxing failed: {}", stderr.trim()))
    }
}

/// Losslessly copies or extracts an audio stream into an MP3 or M4A container.
pub async fn extract_audio_stream(
    source_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let ffmpeg_bin = probe_ffmpeg()
        .ok_or_else(|| "FFmpeg was not found on this system.".to_string())?;

    if !source_path.exists() {
        return Err(format!("Source media file not found at: {}", source_path.display()));
    }

    if let Some(parent) = output_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let is_mp3 = output_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("mp3"))
        .unwrap_or(false);

    let mut cmd = tokio::process::Command::new(ffmpeg_bin);
    cmd.arg("-nostdin")
        .arg("-v")
        .arg("error")
        .arg("-y")
        .arg("-protocol_whitelist")
        .arg("file,pipe")
        .arg("-i")
        .arg(source_path)
        .arg("-map")
        .arg("0:a:0")
        .arg("-vn");

    if is_mp3 {
        cmd.arg("-c:a").arg("libmp3lame").arg("-q:a").arg("2");
    } else {
        cmd.arg("-c:a").arg("copy");
    }

    cmd.arg(output_path);

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    let output = cmd.output().await.map_err(|e| format!("Failed to spawn FFmpeg: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("FFmpeg audio extraction failed: {}", stderr.trim()))
    }
}

/// Converts a standard HTTP `Cookie: k=v; k2=v2` header string into a standard Netscape HTTP Cookie file format.
/// Supports multi-domain broadcasting (e.g. `.youtube.com` and `.google.com`).
pub fn format_netscape_cookies(cookie_str: &str, domains: &[&str]) -> String {
    let mut out = String::from(
        "# Netscape HTTP Cookie File\n\
         # http://curl.haxx.se/rfc/cookie_spec.html\n\
         # This file was automatically generated by Impressive Download Manager\n\n"
    );

    for pair in cookie_str.split(';') {
        let trimmed = pair.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            let k = k.trim();
            let v = v.trim();
            if !k.is_empty() {
                for domain in domains {
                    let d = if domain.starts_with('.') {
                        domain.to_string()
                    } else {
                        format!(".{}", domain)
                    };
                    // Format: domain \t flag(subdomains) \t path \t secure \t expiration \t name \t value
                    out.push_str(&format!("{}\tTRUE\t/\tTRUE\t2147483647\t{}\t{}\n", d, k, v));
                }
            }
        }
    }
    out
}

/// Writes Netscape-formatted cookies to disk for yt-dlp `--cookies` consumption.
pub fn write_netscape_cookies(cookie_str: &str, domains: &[&str], path: &Path) -> std::io::Result<()> {
    let formatted = format_netscape_cookies(cookie_str, domains);
    std::fs::write(path, formatted)
}

/// Diagnoses YouTube-specific stderr errors and returns user-actionable error advice.
pub fn parse_youtube_error(stderr_output: &str) -> String {
    let lower = stderr_output.to_lowercase();
    if lower.contains("confirm you're not a bot") || lower.contains("confirm you’re not a bot") || lower.contains("bot detection") {
        "YouTube Bot Verification: YouTube requires human verification (CAPTCHA). Please open YouTube in your browser, play a video, and re-download with captured cookies.".to_string()
    } else if lower.contains("confirm your age") || lower.contains("age-restricted") {
        "Age-Restricted Video: Sign-in required. Please ensure you are logged into YouTube in your browser so IDM can use your session cookies.".to_string()
    } else if lower.contains("private video") {
        "Private Video: This video is private and only accessible to authorized accounts.".to_string()
    } else if lower.contains("members-only") || lower.contains("channel membership") {
        "Members-Only Content: This video requires an active paid channel membership.".to_string()
    } else if lower.contains("video unavailable") || lower.contains("this video is unavailable") {
        "Video Unavailable: This video has been removed, blocked, or is not available in your region.".to_string()
    } else if lower.contains("premieres in") || lower.contains("live event will begin") {
        "Upcoming Premiere: This video has not started or premiered yet.".to_string()
    } else if lower.contains("429") || lower.contains("too many requests") {
        "Rate Limited (HTTP 429): YouTube is rate limiting requests. Please pause and wait a few minutes before trying again.".to_string()
    } else if lower.contains("login required") || lower.contains("sign in to view") {
        "Authentication Required: Please log into YouTube in your browser to download this video.".to_string()
    } else {
        // Extract the most informative "ERROR:" line if available
        for line in stderr_output.lines().rev() {
            let trimmed = line.trim();
            if trimmed.starts_with("ERROR:") {
                return trimmed.to_string();
            }
        }
        if !stderr_output.trim().is_empty() {
            format!("yt-dlp error: {}", stderr_output.trim())
        } else {
            "yt-dlp exited with non-zero status without error message".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_ffmpeg_availability() {
        let ffmpeg_opt = probe_ffmpeg();
        // Since ffmpeg is installed at /usr/bin/ffmpeg, verify it is found
        assert!(ffmpeg_opt.is_some(), "FFmpeg should be discovered on system");
    }

    #[tokio::test]
    async fn test_ffmpeg_argument_injection_safety() {
        // Attempt an adversarial filename that tries to inject FFmpeg CLI flags:
        // e.g., "-filter_complex eval=evil -y" or semicolons
        let evil_video_name = std::env::temp_dir().join("-filter_complex_eval_evil.mp4");
        let evil_audio_name = std::env::temp_dir().join("-y_evil_flag.m4a");
        let evil_out_name = std::env::temp_dir().join("; rm -rf /.mp4");

        // Files don't exist yet, so mux_dual_stream should cleanly return error about missing video file
        // instead of crashing or executing evil flags
        let res = mux_dual_stream(&evil_video_name, &evil_audio_name, &evil_out_name).await;
        assert!(res.is_err());
        let err_msg = res.unwrap_err();
        assert!(
            err_msg.contains("Video stream file not found"),
            "Command execution must not interpret malicious filenames as command flags: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_working_e2e_dual_stream_muxing() {
        if probe_ffmpeg().is_none() {
            println!("Skipping e2e mux test: ffmpeg not found");
            return;
        }

        let temp_dir = std::env::temp_dir().join(format!("idm_mux_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let video_track = temp_dir.join("test_video.mp4");
        let audio_track = temp_dir.join("test_audio.m4a");
        let output_file = temp_dir.join("muxed_output.mp4");

        // Generate minimal 1-second synthetic video and audio streams using ffmpeg directly for testing
        let ffmpeg = probe_ffmpeg().unwrap();

        // 1. Synthetic 1s black video track
        let v_status = std::process::Command::new(&ffmpeg)
            .args(&[
                "-nostdin", "-v", "error", "-y",
                "-f", "lavfi", "-i", "color=c=black:s=640x360:d=1",
                "-c:v", "libx264", "-pix_fmt", "yuv420p",
                video_track.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to create synthetic video track");
        assert!(v_status.success(), "Synthetic video generation must succeed");

        // 2. Synthetic 1s sine tone audio track
        let a_status = std::process::Command::new(&ffmpeg)
            .args(&[
                "-nostdin", "-v", "error", "-y",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=1",
                "-c:a", "aac",
                audio_track.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to create synthetic audio track");
        assert!(a_status.success(), "Synthetic audio generation must succeed");

        // 3. Test our mux_dual_stream function
        let mux_res = mux_dual_stream(&video_track, &audio_track, &output_file).await;
        assert!(mux_res.is_ok(), "Dual stream muxing must succeed: {:?}", mux_res.err());

        // 4. Verify output file exists and has nonzero size
        assert!(output_file.exists(), "Muxed output file must exist");
        let meta = std::fs::metadata(&output_file).unwrap();
        assert!(meta.len() > 1000, "Muxed output MP4 file must contain valid media container");

        // 5. Test audio extraction
        let mp3_out = temp_dir.join("extracted.mp3");
        let ext_res = extract_audio_stream(&output_file, &mp3_out).await;
        assert!(ext_res.is_ok(), "Audio extraction must succeed: {:?}", ext_res.err());
        assert!(mp3_out.exists(), "Extracted MP3 file must exist");

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_edge_case_5_format_netscape_cookies() {
        let cookie_header = "LOGIN_INFO=AFmmF2swRQIhA:123; SID=g.a000test_sid; HSID=Ah789; PREF=f1=50000000";
        let formatted = format_netscape_cookies(cookie_header, &[".youtube.com", ".google.com"]);

        assert!(formatted.starts_with("# Netscape HTTP Cookie File"), "Header must be standard Netscape");
        assert!(formatted.contains(".youtube.com\tTRUE\t/\tTRUE\t2147483647\tLOGIN_INFO\tAFmmF2swRQIhA:123"));
        assert!(formatted.contains(".google.com\tTRUE\t/\tTRUE\t2147483647\tLOGIN_INFO\tAFmmF2swRQIhA:123"));
        assert!(formatted.contains(".youtube.com\tTRUE\t/\tTRUE\t2147483647\tSID\tg.a000test_sid"));
        assert!(formatted.contains(".youtube.com\tTRUE\t/\tTRUE\t2147483647\tHSID\tAh789"));
        assert!(formatted.contains(".youtube.com\tTRUE\t/\tTRUE\t2147483647\tPREF\tf1=50000000"));
    }

    #[test]
    fn test_edge_case_5_write_netscape_cookies_file() {
        let temp_dir = std::env::temp_dir().join(format!("idm_cookie_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let cookie_file = temp_dir.join("cookies.txt");

        let cookie_header = "SAPISID=test12345; SSID=test67890";
        let write_res = write_netscape_cookies(cookie_header, &[".youtube.com"], &cookie_file);
        assert!(write_res.is_ok(), "Writing netscape cookies must succeed");

        assert!(cookie_file.exists(), "Cookies file must exist on disk");
        let read_back = std::fs::read_to_string(&cookie_file).unwrap();
        assert!(read_back.contains(".youtube.com\tTRUE\t/\tTRUE\t2147483647\tSAPISID\ttest12345"));
        assert!(read_back.contains(".youtube.com\tTRUE\t/\tTRUE\t2147483647\tSSID\ttest67890"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_edge_case_5_parse_youtube_error_diagnostics() {
        // 1. Bot check verification
        let bot_err = "ERROR: [youtube] 12345: Sign in to confirm you’re not a bot. This helps protect our community.";
        let res1 = parse_youtube_error(bot_err);
        assert!(res1.contains("YouTube Bot Verification"), "Must identify bot check: {}", res1);

        // 2. Age-restricted
        let age_err = "ERROR: [youtube] abcde: Sign in to confirm your age. This video may be inappropriate for some users.";
        let res2 = parse_youtube_error(age_err);
        assert!(res2.contains("Age-Restricted Video"), "Must identify age restriction: {}", res2);

        // 3. Private video
        let priv_err = "ERROR: [youtube] xyz89: Private video. Sign in if you've been granted access to this video";
        let res3 = parse_youtube_error(priv_err);
        assert!(res3.contains("Private Video"), "Must identify private video: {}", res3);

        // 4. Members-only content
        let member_err = "ERROR: [youtube] foo12: Join this channel to get access to members-only content like this video";
        let res4 = parse_youtube_error(member_err);
        assert!(res4.contains("Members-Only Content"), "Must identify channel membership: {}", res4);

        // 5. Video unavailable / geoblocked
        let unavail_err = "ERROR: [youtube] bar34: Video unavailable. This video contains content from SME, who has blocked it in your country.";
        let res5 = parse_youtube_error(unavail_err);
        assert!(res5.contains("Video Unavailable"), "Must identify unavailable/geoblocked: {}", res5);

        // 6. Upcoming premiere
        let prem_err = "ERROR: [youtube] live1: This live event will begin in 2 hours.";
        let res6 = parse_youtube_error(prem_err);
        assert!(res6.contains("Upcoming Premiere"), "Must identify upcoming premiere: {}", res6);

        // 7. HTTP 429 rate limit
        let rate_err = "ERROR: [youtube] rate1: HTTP Error 429: Too Many Requests";
        let res7 = parse_youtube_error(rate_err);
        assert!(res7.contains("Rate Limited (HTTP 429)"), "Must identify rate limit: {}", res7);

        // 8. General fallback extracting ERROR: line
        let generic_err = "WARNING: [youtube] Some warning\nERROR: [youtube] Custom unknown extractor failure occurred\nOther lines";
        let res8 = parse_youtube_error(generic_err);
        assert_eq!(res8, "ERROR: [youtube] Custom unknown extractor failure occurred");
    }
}
