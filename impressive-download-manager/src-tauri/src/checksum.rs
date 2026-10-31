use std::fs::File;
use std::io::Read;
use std::path::Path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecksumResult {
    pub algorithm: String,
    pub calculated: String,
    pub expected: Option<String>,
    pub matched: bool,
    pub file_size_bytes: u64,
}

/// Pure-Rust RFC 1321 MD5 Hasher
pub fn md5_hex(data: &[u8]) -> String {
    let mut a: u32 = 0x67452301;
    let mut b: u32 = 0xefcdab89;
    let mut c: u32 = 0x98badcfe;
    let mut d: u32 = 0x10325476;

    let s = [
        7, 12, 17, 22,  7, 12, 17, 22,  7, 12, 17, 22,  7, 12, 17, 22,
        5,  9, 14, 20,  5,  9, 14, 20,  5,  9, 14, 20,  5,  9, 14, 20,
        4, 11, 16, 23,  4, 11, 16, 23,  4, 11, 16, 23,  4, 11, 16, 23,
        6, 10, 15, 21,  6, 10, 15, 21,  6, 10, 15, 21,  6, 10, 15, 21,
    ];

    let k: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in padded.chunks(64) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }

        let mut aa = a;
        let mut bb = b;
        let mut cc = c;
        let mut dd = d;

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((bb & cc) | ((!bb) & dd), i),
                16..=31 => ((dd & bb) | ((!dd) & cc), (5 * i + 1) % 16),
                32..=47 => (bb ^ cc ^ dd, (3 * i + 5) % 16),
                _ => (cc ^ (bb | (!dd)), (7 * i) % 16),
            };

            let temp = dd;
            dd = cc;
            cc = bb;
            bb = bb.wrapping_add(
                (aa.wrapping_add(f).wrapping_add(k[i]).wrapping_add(m[g])).rotate_left(s[i]),
            );
            aa = temp;
        }

        a = a.wrapping_add(aa);
        b = b.wrapping_add(bb);
        c = c.wrapping_add(cc);
        d = d.wrapping_add(dd);
    }

    let bytes = [
        a.to_le_bytes(),
        b.to_le_bytes(),
        c.to_le_bytes(),
        d.to_le_bytes(),
    ];
    let mut hex = String::with_capacity(32);
    for word in bytes {
        for byte in word {
            hex.push_str(&format!("{:02x}", byte));
        }
    }
    hex
}

/// Pure-Rust FIPS 180-4 Standard SHA-256 Hasher
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = [
        0x6a09e667u32, 0xbb67ae85u32, 0x3c6ef372u32, 0xa54ff53au32,
        0x510e527fu32, 0x9b05688cu32, 0x1f83d9abu32, 0x5be0cd19u32,
    ];

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut ch = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch_val = (e & f) ^ ((!e) & g);
            let temp1 = ch.wrapping_add(s1).wrapping_add(ch_val).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            ch = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(ch);
    }

    format!(
        "{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}",
        h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]
    )
}

/// Reads a file from disk and computes its hash using the specified algorithm
pub fn compute_file_checksum(file_path: &Path, algorithm: &str) -> Result<String, String> {
    if !file_path.exists() {
        return Err(format!("File does not exist: {}", file_path.display()));
    }

    let mut file = File::open(file_path).map_err(|e| format!("Failed to open file: {}", e))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).map_err(|e| format!("Failed to read file: {}", e))?;

    let algo_clean = algorithm.trim().to_lowercase();
    match algo_clean.as_str() {
        "md5" => Ok(md5_hex(&buffer)),
        "sha1" | "sha-1" => Ok(crate::torrent::sha1_hex(&buffer)),
        "sha256" | "sha-256" | "" => Ok(sha256_hex(&buffer)),
        other => Err(format!("Unsupported checksum algorithm '{}'. Supported: md5, sha1, sha256", other)),
    }
}

/// Auto-detects algorithm from expected hash length or uses specified algorithm
pub fn verify_file_checksum(
    file_path: &Path,
    expected_hash: &str,
    forced_algorithm: Option<&str>,
) -> Result<ChecksumResult, String> {
    let clean_expected = expected_hash.trim().to_lowercase();
    if clean_expected.is_empty() {
        return Err("Expected hash cannot be empty".to_string());
    }

    let algo = match forced_algorithm {
        Some(a) if !a.trim().is_empty() => a.trim().to_lowercase(),
        _ => match clean_expected.len() {
            32 => "md5".to_string(),
            40 => "sha1".to_string(),
            64 => "sha256".to_string(),
            _ => "sha256".to_string(),
        },
    };

    let calculated = compute_file_checksum(file_path, &algo)?;
    let matched = calculated.eq_ignore_ascii_case(&clean_expected);
    let file_size_bytes = std::fs::metadata(file_path).map(|m| m.len()).unwrap_or(0);

    Ok(ChecksumResult {
        algorithm: algo,
        calculated,
        expected: Some(clean_expected),
        matched,
        file_size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_standard_test_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(b"message digest"), "f96b697d7cb7938d525a2f31aaf161d0");
    }

    #[test]
    fn test_sha256_standard_test_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn test_file_checksum_verification_and_auto_detect() {
        let temp_dir = std::env::temp_dir().join(format!("idm_cksum_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("test_payload.bin");
        std::fs::write(&file_path, b"Impressive Download Manager Checksum Verification Payload").unwrap();

        // 1. Calculate actual hashes
        let md5 = compute_file_checksum(&file_path, "md5").unwrap();
        let sha1 = compute_file_checksum(&file_path, "sha1").unwrap();
        let sha256 = compute_file_checksum(&file_path, "sha256").unwrap();

        // 2. Verify auto-detection of algorithm by length
        let res_md5 = verify_file_checksum(&file_path, &md5, None).unwrap();
        assert!(res_md5.matched);
        assert_eq!(res_md5.algorithm, "md5");

        let res_sha1 = verify_file_checksum(&file_path, &sha1, None).unwrap();
        assert!(res_sha1.matched);
        assert_eq!(res_sha1.algorithm, "sha1");

        let res_sha256 = verify_file_checksum(&file_path, &sha256, None).unwrap();
        assert!(res_sha256.matched);
        assert_eq!(res_sha256.algorithm, "sha256");

        // 3. Negative mismatch check
        let bad_sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        let res_bad = verify_file_checksum(&file_path, bad_sha256, None).unwrap();
        assert!(!res_bad.matched);
        assert_eq!(res_bad.calculated, sha256);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
