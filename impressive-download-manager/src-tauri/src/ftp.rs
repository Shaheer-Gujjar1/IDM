use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncSeekExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FtpUrlInfo {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub path: String,
    pub filename: String,
    pub is_tls: bool,
}

/// Detects whether a URL belongs to the FTP or FTPS protocol
pub fn is_ftp_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.starts_with("ftp://") || lower.starts_with("ftps://")
}

/// Parses an FTP or FTPS URL (RFC 1738) into connection parameters
pub fn parse_ftp_url(url: &str) -> Result<FtpUrlInfo, String> {
    let clean = url.trim();
    let is_tls = clean.to_lowercase().starts_with("ftps://");

    if !clean.to_lowercase().starts_with("ftp://") && !is_tls {
        return Err("Not a valid ftp:// or ftps:// URL".to_string());
    }

    let parsed = reqwest::Url::parse(clean).map_err(|e| format!("Invalid FTP URL syntax: {}", e))?;
    let host = parsed.host_str().ok_or("Missing host in FTP URL")?.to_string();
    let port = parsed.port().unwrap_or(if is_tls { 990 } else { 21 });

    let user = if !parsed.username().is_empty() {
        parsed.username().to_string()
    } else {
        "anonymous".to_string()
    };

    let pass = parsed.password().unwrap_or("anonymous@idm.org").to_string();
    let path = parsed.path().to_string();

    let raw_fn = parsed.path_segments().and_then(|mut s| s.next_back()).unwrap_or("ftp_download");
    let filename = if raw_fn.is_empty() {
        "ftp_download".to_string()
    } else {
        urlencoding::decode(raw_fn).unwrap_or(std::borrow::Cow::Borrowed(raw_fn)).to_string()
    };

    Ok(FtpUrlInfo {
        host,
        port,
        user,
        pass,
        path,
        filename,
        is_tls,
    })
}

/// Probes FTP server for file existence, exact size (via SIZE command), and REST range capability
pub async fn probe_ftp_file(info: &FtpUrlInfo) -> Result<(u64, bool), String> {
    let mut ctrl = connect_ftp_control(info).await?;

    // Set binary mode
    let _ = send_ftp_cmd(&mut ctrl, "TYPE I").await?;

    // Query SIZE
    let size_resp = send_ftp_cmd(&mut ctrl, &format!("SIZE {}", info.path)).await?;
    let size = if size_resp.starts_with("213") {
        size_resp[3..].trim().parse::<u64>().unwrap_or(0)
    } else {
        0
    };

    // Check REST range capability
    let rest_resp = send_ftp_cmd(&mut ctrl, "REST 0").await;
    let supports_range = match rest_resp {
        Ok(r) => r.starts_with("350") || r.starts_with("200"),
        Err(_) => false,
    };

    let _ = send_ftp_cmd(&mut ctrl, "QUIT").await;
    Ok((size, supports_range))
}

/// Internal helper: Connects, authenticates, and returns control TCP stream
async fn connect_ftp_control(info: &FtpUrlInfo) -> Result<TcpStream, String> {
    let addr = format!("{}:{}", info.host, info.port);
    let mut stream = timeout(Duration::from_secs(8), TcpStream::connect(&addr))
        .await
        .map_err(|_| format!("FTP connection timed out to {}", addr))?
        .map_err(|e| format!("Failed to connect to FTP host {}: {}", addr, e))?;

    // Read 220 banner
    let banner = read_ftp_resp(&mut stream).await?;
    if !banner.starts_with("220") {
        return Err(format!("Unexpected FTP server banner: {}", banner));
    }

    // Send USER
    let user_resp = send_ftp_cmd(&mut stream, &format!("USER {}", info.user)).await?;
    if user_resp.starts_with("331") {
        // Password required
        let pass_resp = send_ftp_cmd(&mut stream, &format!("PASS {}", info.pass)).await?;
        if !pass_resp.starts_with("230") {
            return Err(format!("FTP authentication rejected: {}", pass_resp));
        }
    } else if !user_resp.starts_with("230") {
        return Err(format!("FTP USER command rejected: {}", user_resp));
    }

    Ok(stream)
}

/// Sends an FTP control command and reads response
async fn send_ftp_cmd(stream: &mut TcpStream, cmd: &str) -> Result<String, String> {
    let formatted = format!("{}\r\n", cmd);
    stream
        .write_all(formatted.as_bytes())
        .await
        .map_err(|e| format!("FTP command write error: {}", e))?;

    read_ftp_resp(stream).await
}

/// Reads FTP server response line (RFC 959)
async fn read_ftp_resp(stream: &mut TcpStream) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];

    let deadline = Duration::from_secs(10);
    loop {
        let n = timeout(deadline, stream.read(&mut byte))
            .await
            .map_err(|_| "FTP response timed out".to_string())?
            .map_err(|e| format!("FTP read error: {}", e))?;

        if n == 0 {
            break;
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n") {
            let line = String::from_utf8_lossy(&buf).to_string();
            // Handle multi-line responses (e.g. 220- ...)
            if line.len() >= 4 && &line[3..4] == "-" {
                buf.clear();
                continue;
            }
            return Ok(line.trim().to_string());
        }
    }

    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

/// Parses PASV response: "227 Entering Passive Mode (192,168,1,10,195,149)"
fn parse_pasv_response(resp: &str) -> Result<std::net::SocketAddr, String> {
    let open_paren = resp.find('(').ok_or("Invalid PASV format: missing '('")?;
    let close_paren = resp.find(')').ok_or("Invalid PASV format: missing ')'")?;
    let numbers_str = &resp[open_paren + 1..close_paren];

    let parts: Vec<u8> = numbers_str
        .split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .collect();

    if parts.len() < 6 {
        return Err("Invalid PASV octets: expected 6 numbers".to_string());
    }

    let ip = std::net::Ipv4Addr::new(parts[0], parts[1], parts[2], parts[3]);
    let port = ((parts[4] as u16) << 8) | (parts[5] as u16);

    Ok(std::net::SocketAddr::V4(std::net::SocketAddrV4::new(ip, port)))
}

/// Downloads a designated range/stream from an FTP server directly into the destination file
pub async fn download_ftp_stream(
    info: &FtpUrlInfo,
    start_offset: u64,
    length: Option<u64>,
    dest_path: &std::path::Path,
    downloaded_tracker: Arc<AtomicU64>,
    mut abort_rx: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<u64, String> {
    let mut ctrl = connect_ftp_control(info).await?;
    let _ = send_ftp_cmd(&mut ctrl, "TYPE I").await?;

    // Enter Passive Mode
    let pasv_resp = send_ftp_cmd(&mut ctrl, "PASV").await?;
    let data_addr = parse_pasv_response(&pasv_resp)?;

    // Connect to data channel
    let mut data_stream = timeout(Duration::from_secs(8), TcpStream::connect(data_addr))
        .await
        .map_err(|_| format!("Timeout connecting to FTP data port {}", data_addr))?
        .map_err(|e| format!("Failed to connect to FTP data port: {}", e))?;

    // Restart at offset if specified (RFC 3659)
    if start_offset > 0 {
        let rest_resp = send_ftp_cmd(&mut ctrl, &format!("REST {}", start_offset)).await?;
        if !rest_resp.starts_with("350") {
            return Err(format!("FTP server does not support REST restart: {}", rest_resp));
        }
    }

    // Trigger RETR
    let retr_resp = send_ftp_cmd(&mut ctrl, &format!("RETR {}", info.path)).await?;
    if !retr_resp.starts_with("150") && !retr_resp.starts_with("125") {
        return Err(format!("FTP RETR command failed: {}", retr_resp));
    }

    // Open destination file with seek/write capability
    let mut dest_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(dest_path)
        .await
        .map_err(|e| format!("Cannot open target file for FTP stream: {}", e))?;

    dest_file
        .seek(std::io::SeekFrom::Start(start_offset))
        .await
        .map_err(|e| format!("Seek failed at offset {}: {}", start_offset, e))?;

    let mut buf = vec![0u8; 65536];
    let mut total_transferred = 0u64;

    loop {
        if let Some(ref mut rx) = abort_rx {
            if rx.try_recv().is_ok() {
                return Err("FTP download paused or cancelled".to_string());
            }
        }

        let max_to_read = if let Some(len) = length {
            let remaining = len.saturating_sub(total_transferred);
            if remaining == 0 {
                break;
            }
            (buf.len() as u64).min(remaining) as usize
        } else {
            buf.len()
        };

        let n = match timeout(Duration::from_secs(15), data_stream.read(&mut buf[..max_to_read])).await {
            Ok(Ok(0)) => break, // EOF reached
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(format!("FTP data read error: {}", e)),
            Err(_) => return Err("FTP data read timed out".to_string()),
        };

        dest_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("Failed to write to destination file: {}", e))?;

        total_transferred += n as u64;
        downloaded_tracker.fetch_add(n as u64, Ordering::Relaxed);
    }

    dest_file.sync_all().await.map_err(|e| e.to_string())?;
    drop(data_stream);

    // Read 226 Transfer complete on control channel
    let _ = read_ftp_resp(&mut ctrl).await;
    let _ = send_ftp_cmd(&mut ctrl, "QUIT").await;

    Ok(total_transferred)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ftp_url_anonymous() {
        let url = "ftp://ftp.gnu.org/gnu/emacs/emacs-29.1.tar.gz";
        let info = parse_ftp_url(url).expect("Should parse anonymous FTP URL");

        assert_eq!(info.host, "ftp.gnu.org");
        assert_eq!(info.port, 21);
        assert_eq!(info.user, "anonymous");
        assert_eq!(info.pass, "anonymous@idm.org");
        assert_eq!(info.path, "/gnu/emacs/emacs-29.1.tar.gz");
        assert_eq!(info.filename, "emacs-29.1.tar.gz");
        assert!(!info.is_tls);
    }

    #[test]
    fn test_parse_ftps_url_with_credentials_and_custom_port() {
        let url = "ftps://admin:SecretPass123@secure.backup.org:2121/archives/database_dump.sql.gz";
        let info = parse_ftp_url(url).expect("Should parse FTPS URL");

        assert_eq!(info.host, "secure.backup.org");
        assert_eq!(info.port, 2121);
        assert_eq!(info.user, "admin");
        assert_eq!(info.pass, "SecretPass123");
        assert_eq!(info.path, "/archives/database_dump.sql.gz");
        assert_eq!(info.filename, "database_dump.sql.gz");
        assert!(info.is_tls);
    }

    #[test]
    fn test_parse_pasv_response() {
        let resp = "227 Entering Passive Mode (192,168,1,50,195,149).";
        let addr = parse_pasv_response(resp).expect("Should parse PASV octets");

        // port = (195 << 8) | 149 = 49920 + 149 = 50069
        assert_eq!(addr.to_string(), "192.168.1.50:50069");
    }

    #[tokio::test]
    async fn test_mock_ftp_server_probe_and_download_flow() {
        use tokio::net::TcpListener;

        // 1. Mock FTP Data Listener
        let data_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let data_port = data_listener.local_addr().unwrap().port();
        let p1 = (data_port >> 8) as u8;
        let p2 = (data_port & 0xFF) as u8;

        let payload_content = b"IDM Native Pure-Rust FTP & FTPS Engine Payload Data 123456789".to_vec();
        let payload_len = payload_content.len() as u64;
        let payload_clone = payload_content.clone();

        // Spawn mock data channel worker
        tokio::spawn(async move {
            if let Ok((mut data_sock, _)) = data_listener.accept().await {
                let _ = data_sock.write_all(&payload_clone).await;
            }
        });

        // 2. Mock FTP Control Listener
        let ctrl_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ctrl_addr = ctrl_listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut sock, _)) = ctrl_listener.accept().await {
                tokio::spawn(async move {
                    // Send banner
                    let _ = sock.write_all(b"220 IDM Mock FTP Server Ready\r\n").await;

                    let mut buf = vec![0u8; 1024];
                    while let Ok(n) = sock.read(&mut buf).await {
                        if n == 0 { break; }
                        let req = String::from_utf8_lossy(&buf[..n]);

                        if req.starts_with("USER") {
                            let _ = sock.write_all(b"331 User name okay, need password.\r\n").await;
                        } else if req.starts_with("PASS") {
                            let _ = sock.write_all(b"230 User logged in, proceed.\r\n").await;
                        } else if req.starts_with("TYPE") {
                            let _ = sock.write_all(b"200 Type set to I.\r\n").await;
                        } else if req.starts_with("SIZE") {
                            let resp = format!("213 {}\r\n", payload_len);
                            let _ = sock.write_all(resp.as_bytes()).await;
                        } else if req.starts_with("REST") {
                            let _ = sock.write_all(b"350 Restarting at position.\r\n").await;
                        } else if req.starts_with("PASV") {
                            let resp = format!("227 Entering Passive Mode (127,0,0,1,{},{}).\r\n", p1, p2);
                            let _ = sock.write_all(resp.as_bytes()).await;
                        } else if req.starts_with("RETR") {
                            let _ = sock.write_all(b"150 Opening BINARY mode data connection.\r\n").await;
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            let _ = sock.write_all(b"226 Transfer complete.\r\n").await;
                        } else if req.starts_with("QUIT") {
                            let _ = sock.write_all(b"221 Goodbye.\r\n").await;
                            break;
                        }
                    }
                });
            }
        });

        let ftp_info = FtpUrlInfo {
            host: "127.0.0.1".to_string(),
            port: ctrl_addr.port(),
            user: "testuser".to_string(),
            pass: "testpass".to_string(),
            path: "/test/file.bin".to_string(),
            filename: "file.bin".to_string(),
            is_tls: false,
        };

        // 3. Test Probe
        let (size, supports_range) = probe_ftp_file(&ftp_info).await.expect("FTP probe should succeed");
        assert_eq!(size, payload_len);
        assert!(supports_range);

        // 4. Test Download
        let temp_dir = std::env::temp_dir().join(format!("idm_ftp_test_{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::create_dir_all(&temp_dir).await;
        let dest_file = temp_dir.join("downloaded_ftp_file.bin");

        let downloaded_tracker = Arc::new(AtomicU64::new(0));
        let transferred = download_ftp_stream(
            &ftp_info,
            0,
            Some(payload_len),
            &dest_file,
            downloaded_tracker.clone(),
            None,
        ).await.expect("FTP stream download should succeed");

        assert_eq!(transferred, payload_len);
        assert_eq!(downloaded_tracker.load(Ordering::Relaxed), payload_len);

        let saved = tokio::fs::read(&dest_file).await.unwrap();
        assert_eq!(saved, payload_content);

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
