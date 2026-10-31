use std::fs::File;
use std::io::Read;
use std::path::Path;
use serde::{Deserialize, Serialize};

/// eDonkey2000 chunk size is standard 9,728,000 bytes (9500 KiB)
pub const ED2K_CHUNK_SIZE: usize = 9_728_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ed2kLinkType {
    File,
    Server,
    ServerList,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed2kFileInfo {
    pub filename: String,
    pub file_size: u64,
    pub file_hash: String,
    pub aich_hash: Option<String>,
    pub sources: Vec<String>,
    pub raw_link: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed2kServerInfo {
    pub ip: String,
    pub port: u16,
    pub raw_link: String,
}

/// Checks if a string is an eDonkey2000 / eMule link
pub fn is_ed2k_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("ed2k://")
}

/// Parses an `ed2k://|file|...|/` link into `Ed2kFileInfo`
pub fn parse_ed2k_link(link: &str) -> Result<Ed2kFileInfo, String> {
    let trimmed = link.trim();
    if !trimmed.starts_with("ed2k://|file|") {
        return Err("Not a valid ed2k file link (must start with 'ed2k://|file|')".to_string());
    }

    // Strip prefix "ed2k://" and trailing "/" if present
    let inner = &trimmed["ed2k://".len()..];
    let parts: Vec<&str> = inner.split('|').collect();

    // Format: |file|<filename>|<filesize>|<filehash>|...|/
    // parts[0] is empty, parts[1] is "file", parts[2] is filename, parts[3] is filesize, parts[4] is filehash
    if parts.len() < 5 {
        return Err("Malformed ed2k link: missing required filename, size, or hash fields".to_string());
    }

    if parts[1] != "file" {
        return Err(format!("Expected 'file' in ed2k link, found '{}'", parts[1]));
    }

    let raw_filename = parts[2];
    let decoded_filename = urlencoding::decode(raw_filename)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| raw_filename.to_string());
    let filename = crate::proxy::sanitize_filename(&decoded_filename);

    let file_size: u64 = parts[3]
        .parse()
        .map_err(|e| format!("Invalid ed2k file size '{}': {}", parts[3], e))?;

    let file_hash = parts[4].to_lowercase();
    if file_hash.len() != 32 || !file_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Invalid ed2k MD4 file hash '{}' (expected 32 hex characters)", file_hash));
    }

    let mut aich_hash = None;
    let mut sources = Vec::new();

    // Check optional parameters in remaining parts
    for part in &parts[5..] {
        let p = part.trim();
        if p.is_empty() || p == "/" {
            continue;
        }

        if p.starts_with("h=") {
            aich_hash = Some(p[2..].to_string());
        } else if p.starts_with("s=") {
            sources.push(p[2..].to_string());
        } else if p.starts_with("sources=") {
            for s in p[8..].split(',') {
                if !s.is_empty() {
                    sources.push(s.to_string());
                }
            }
        } else if p.starts_with("http://") || p.starts_with("https://") {
            // Direct HTTP/HTTPS webseed source
            sources.push(p.to_string());
        }
    }

    Ok(Ed2kFileInfo {
        filename,
        file_size,
        file_hash,
        aich_hash,
        sources,
        raw_link: trimmed.to_string(),
    })
}

/// Parses a batch of ed2k links (`ed2k_pack`) from text, playlist, or collection file
pub fn parse_ed2k_pack(content: &str) -> Vec<Ed2kFileInfo> {
    let mut results = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("ed2k://|file|") {
            if let Ok(info) = parse_ed2k_link(trimmed) {
                results.push(info);
            }
        } else if let Some(idx) = trimmed.find("ed2k://|file|") {
            // Extract embedded link from line
            let sub = &trimmed[idx..];
            if let Some(end_idx) = sub.find("/|") {
                let candidate = &sub[..end_idx + 1];
                if let Ok(info) = parse_ed2k_link(candidate) {
                    results.push(info);
                }
            } else if let Some(end_idx) = sub.find("|/") {
                let candidate = &sub[..end_idx + 2];
                if let Ok(info) = parse_ed2k_link(candidate) {
                    results.push(info);
                }
            }
        }
    }

    results
}

/// Parses an `ed2k://|server|ip|port|/` link into `Ed2kServerInfo`
pub fn parse_ed2k_server_link(link: &str) -> Result<Ed2kServerInfo, String> {
    let trimmed = link.trim();
    if !trimmed.starts_with("ed2k://|server|") {
        return Err("Not a valid ed2k server link".to_string());
    }

    let inner = &trimmed["ed2k://".len()..];
    let parts: Vec<&str> = inner.split('|').collect();
    if parts.len() < 4 {
        return Err("Malformed ed2k server link".to_string());
    }

    let ip = parts[2].to_string();
    let port: u16 = parts[3].parse().map_err(|e| format!("Invalid ed2k server port: {}", e))?;

    Ok(Ed2kServerInfo {
        ip,
        port,
        raw_link: trimmed.to_string(),
    })
}

// =========================================================================
// Pure-Rust RFC 1320 MD4 Implementation & eDonkey2000 Hasher
// =========================================================================

#[inline]
fn f(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (!x & z)
}

#[inline]
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & y) | (x & z) | (y & z)
}

#[inline]
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

/// Pure-Rust RFC 1320 MD4 calculation on a byte slice, returning 16-byte raw digest
pub fn md4(data: &[u8]) -> [u8; 16] {
    let mut a: u32 = 0x67452301;
    let mut b: u32 = 0xefcdab89;
    let mut c: u32 = 0x98badcfe;
    let mut d: u32 = 0x10325476;

    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut x = [0u32; 16];
        for i in 0..16 {
            x[i] = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }

        let aa = a;
        let bb = b;
        let cc = c;
        let dd = d;

        // Round 1
        let r1 = |a: &mut u32, b: u32, c: u32, d: u32, k: usize, s: u32| {
            *a = a.wrapping_add(f(b, c, d)).wrapping_add(x[k]).rotate_left(s);
        };
        r1(&mut a, b, c, d, 0, 3);
        r1(&mut d, a, b, c, 1, 7);
        r1(&mut c, d, a, b, 2, 11);
        r1(&mut b, c, d, a, 3, 19);
        r1(&mut a, b, c, d, 4, 3);
        r1(&mut d, a, b, c, 5, 7);
        r1(&mut c, d, a, b, 6, 11);
        r1(&mut b, c, d, a, 7, 19);
        r1(&mut a, b, c, d, 8, 3);
        r1(&mut d, a, b, c, 9, 7);
        r1(&mut c, d, a, b, 10, 11);
        r1(&mut b, c, d, a, 11, 19);
        r1(&mut a, b, c, d, 12, 3);
        r1(&mut d, a, b, c, 13, 7);
        r1(&mut c, d, a, b, 14, 11);
        r1(&mut b, c, d, a, 15, 19);

        // Round 2
        let r2 = |a: &mut u32, b: u32, c: u32, d: u32, k: usize, s: u32| {
            *a = a.wrapping_add(g(b, c, d)).wrapping_add(x[k]).wrapping_add(0x5a827999).rotate_left(s);
        };
        r2(&mut a, b, c, d, 0, 3);
        r2(&mut d, a, b, c, 4, 5);
        r2(&mut c, d, a, b, 8, 9);
        r2(&mut b, c, d, a, 12, 13);
        r2(&mut a, b, c, d, 1, 3);
        r2(&mut d, a, b, c, 5, 5);
        r2(&mut c, d, a, b, 9, 9);
        r2(&mut b, c, d, a, 13, 13);
        r2(&mut a, b, c, d, 2, 3);
        r2(&mut d, a, b, c, 6, 5);
        r2(&mut c, d, a, b, 10, 9);
        r2(&mut b, c, d, a, 14, 13);
        r2(&mut a, b, c, d, 3, 3);
        r2(&mut d, a, b, c, 7, 5);
        r2(&mut c, d, a, b, 11, 9);
        r2(&mut b, c, d, a, 15, 13);

        // Round 3
        let r3 = |a: &mut u32, b: u32, c: u32, d: u32, k: usize, s: u32| {
            *a = a.wrapping_add(h(b, c, d)).wrapping_add(x[k]).wrapping_add(0x6ed9eba1).rotate_left(s);
        };
        r3(&mut a, b, c, d, 0, 3);
        r3(&mut d, a, b, c, 8, 9);
        r3(&mut c, d, a, b, 4, 11);
        r3(&mut b, c, d, a, 12, 15);
        r3(&mut a, b, c, d, 2, 3);
        r3(&mut d, a, b, c, 10, 9);
        r3(&mut c, d, a, b, 6, 11);
        r3(&mut b, c, d, a, 14, 15);
        r3(&mut a, b, c, d, 1, 3);
        r3(&mut d, a, b, c, 9, 9);
        r3(&mut c, d, a, b, 5, 11);
        r3(&mut b, c, d, a, 13, 15);
        r3(&mut a, b, c, d, 3, 3);
        r3(&mut d, a, b, c, 11, 9);
        r3(&mut c, d, a, b, 7, 11);
        r3(&mut b, c, d, a, 15, 15);

        a = a.wrapping_add(aa);
        b = b.wrapping_add(bb);
        c = c.wrapping_add(cc);
        d = d.wrapping_add(dd);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a.to_le_bytes());
    out[4..8].copy_from_slice(&b.to_le_bytes());
    out[8..12].copy_from_slice(&c.to_le_bytes());
    out[12..16].copy_from_slice(&d.to_le_bytes());
    out
}

/// Computes MD4 and returns 32-character lowercase hex string
pub fn md4_hex(data: &[u8]) -> String {
    let digest = md4(data);
    let mut hex = String::with_capacity(32);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", b);
    }
    hex
}

/// Calculates the eDonkey2000 / eMule hash for an in-memory byte buffer
pub fn calculate_ed2k_hash(data: &[u8]) -> String {
    if data.len() < ED2K_CHUNK_SIZE {
        return md4_hex(data);
    }

    // Files >= 9,728,000 bytes: MD4 of each 9500 KiB chunk, then MD4 of the concatenated chunk hashes
    let mut chunk_hashes = Vec::new();
    for chunk in data.chunks(ED2K_CHUNK_SIZE) {
        let chunk_md4 = md4(chunk);
        chunk_hashes.extend_from_slice(&chunk_md4);
    }

    md4_hex(&chunk_hashes)
}

/// Streams and computes the eDonkey2000 / eMule hash of a file on disk
pub fn calculate_ed2k_file_hash(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("Failed to open file for ED2K hashing: {}", e))?;
    let metadata = file.metadata().map_err(|e| format!("Cannot read file metadata: {}", e))?;
    let file_len = metadata.len();

    if file_len < ED2K_CHUNK_SIZE as u64 {
        let mut buffer = Vec::with_capacity(file_len as usize);
        file.read_to_end(&mut buffer).map_err(|e| format!("Error reading file: {}", e))?;
        return Ok(md4_hex(&buffer));
    }

    let mut chunk_buffer = vec![0u8; ED2K_CHUNK_SIZE];
    let mut concatenated_hashes = Vec::new();

    loop {
        let mut bytes_read_for_chunk = 0;
        while bytes_read_for_chunk < ED2K_CHUNK_SIZE {
            let n = file
                .read(&mut chunk_buffer[bytes_read_for_chunk..])
                .map_err(|e| format!("Error reading ED2K chunk: {}", e))?;
            if n == 0 {
                break;
            }
            bytes_read_for_chunk += n;
        }

        if bytes_read_for_chunk == 0 {
            break;
        }

        let chunk_digest = md4(&chunk_buffer[..bytes_read_for_chunk]);
        concatenated_hashes.extend_from_slice(&chunk_digest);

        if bytes_read_for_chunk < ED2K_CHUNK_SIZE {
            break; // EOF reached
        }
    }

    Ok(md4_hex(&concatenated_hashes))
}

/// Verifies whether a downloaded file matches the expected ED2K hash
pub fn verify_ed2k_file(path: &Path, expected_hash: &str) -> Result<bool, String> {
    let clean_expected = expected_hash.trim().to_lowercase();
    let calculated = calculate_ed2k_file_hash(path)?;
    Ok(calculated == clean_expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md4_rfc1320_standard_vectors() {
        // RFC 1320 standard test vectors
        assert_eq!(md4_hex(b""), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(md4_hex(b"a"), "bde52cb31de33e46245e05fbdbd6fb24");
        assert_eq!(md4_hex(b"abc"), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(md4_hex(b"message digest"), "d9130a8164549fe818874806e1c7014b");
        assert_eq!(
            md4_hex(b"abcdefghijklmnopqrstuvwxyz"),
            "d79e1c308aa5bbcdeea8ed63df412da9"
        );
        assert_eq!(
            md4_hex(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"),
            "043f8582f241db351ce627e153e7f0e4"
        );
    }

    #[test]
    fn test_parse_ed2k_link_basic() {
        let link = "ed2k://|file|Ubuntu_22.04_LTS.iso|3825205248|31d6cfe0d16ae931b73c59d7e0c089c0|/";
        let info = parse_ed2k_link(link).expect("Should parse basic ED2K file link");

        assert_eq!(info.filename, "Ubuntu_22.04_LTS.iso");
        assert_eq!(info.file_size, 3825205248);
        assert_eq!(info.file_hash, "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert!(info.aich_hash.is_none());
        assert!(info.sources.is_empty());
    }

    #[test]
    fn test_parse_ed2k_link_with_aich_and_sources() {
        let link = "ed2k://|file|Test%20Video%20Clip.mp4|104857600|d9130a8164549fe818874806e1c7014b|h=ABCDEF1234567890|s=http://mirror.example.com/test.mp4|s=192.168.1.50:4662|/";
        let info = parse_ed2k_link(link).expect("Should parse ED2K link with tags");

        assert_eq!(info.filename, "Test Video Clip.mp4");
        assert_eq!(info.file_size, 104857600);
        assert_eq!(info.file_hash, "d9130a8164549fe818874806e1c7014b");
        assert_eq!(info.aich_hash.as_deref(), Some("ABCDEF1234567890"));
        assert_eq!(info.sources.len(), 2);
        assert_eq!(info.sources[0], "http://mirror.example.com/test.mp4");
        assert_eq!(info.sources[1], "192.168.1.50:4662");
    }

    #[test]
    fn test_parse_ed2k_pack_batch() {
        let pack = r#"
# eMule Collection Pack
ed2k://|file|Document1.pdf|102400|bde52cb31de33e46245e05fbdbd6fb24|/
Some random text between links
ed2k://|file|Document2.pdf|204800|a448017aaf21d8525fc10ae87aa6729d|/
ed2k://|server|212.63.206.35|4242|/
"#;
        let files = parse_ed2k_pack(pack);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].filename, "Document1.pdf");
        assert_eq!(files[1].filename, "Document2.pdf");
    }

    #[test]
    fn test_ed2k_hash_small_and_chunked() {
        // Small data < 9,728,000 bytes uses single MD4
        let small_data = b"Hello eDonkey2000 pure Rust IDM!";
        let hash = calculate_ed2k_hash(small_data);
        assert_eq!(hash, md4_hex(small_data));

        // Test file verification
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("idm_ed2k_test_{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&test_file, small_data).unwrap();

        let matched = verify_ed2k_file(&test_file, &hash).unwrap();
        assert!(matched);

        let _ = std::fs::remove_file(&test_file);
    }
}
