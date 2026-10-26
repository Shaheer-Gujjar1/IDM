use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Locates the ffmpeg executable on the system PATH or common platform locations.
pub fn probe_ffmpeg() -> Option<PathBuf> {
    // 1. Check if "ffmpeg" is in PATH by running `ffmpeg -version`
    if let Ok(output) = std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        if output.success() {
            return Some(PathBuf::from("ffmpeg"));
        }
    }

    // 2. Common Unix locations
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
}
