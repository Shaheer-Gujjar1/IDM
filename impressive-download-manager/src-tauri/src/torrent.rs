use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TorrentFileInfo {
    pub path: String,
    pub length: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TorrentMetadata {
    pub info_hash: String,
    pub name: String,
    pub piece_length: u64,
    pub pieces_count: usize,
    pub total_length: u64,
    pub files: Vec<TorrentFileInfo>,
    pub announce: Option<String>,
    pub announce_list: Vec<String>,
    pub web_seeds: Vec<String>,
}

impl TorrentMetadata {
    pub fn to_magnet(&self) -> MagnetLink {
        MagnetLink {
            info_hash: self.info_hash.clone(),
            name: self.name.clone(),
            trackers: self.announce_list.clone(),
            web_seeds: self.web_seeds.clone(),
            exact_length: if self.total_length > 0 {
                Some(self.total_length)
            } else {
                None
            },
        }
    }

    pub fn to_magnet_uri(&self) -> String {
        self.to_magnet().to_uri()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MagnetLink {
    pub info_hash: String,
    pub name: String,
    pub trackers: Vec<String>,
    pub web_seeds: Vec<String>,
    pub exact_length: Option<u64>,
}

impl MagnetLink {
    pub fn to_uri(&self) -> String {
        let mut uri = format!("magnet:?xt=urn:btih:{}", self.info_hash);
        if !self.name.is_empty() {
            uri.push_str(&format!("&dn={}", urlencoding::encode(&self.name)));
        }
        if let Some(len) = self.exact_length {
            uri.push_str(&format!("&xl={}", len));
        }
        for tr in &self.trackers {
            uri.push_str(&format!("&tr={}", urlencoding::encode(tr)));
        }
        for ws in &self.web_seeds {
            uri.push_str(&format!("&ws={}", urlencoding::encode(ws)));
        }
        uri
    }
}

/// Parses a BitTorrent Magnet URI (BEP 9 / BEP 53)
pub fn parse_magnet(uri: &str) -> Result<MagnetLink, String> {
    let clean = uri.trim();
    if !clean.starts_with("magnet:?") {
        return Err("Invalid magnet URI: missing 'magnet:?' prefix".to_string());
    }

    let query_str = &clean[8..];
    let mut info_hash = String::new();
    let mut name = String::new();
    let mut trackers = Vec::new();
    let mut web_seeds = Vec::new();
    let mut exact_length = None;

    for param in query_str.split('&') {
        if let Some((k, v)) = param.split_once('=') {
            let decoded_val = urlencoding::decode(v).unwrap_or(std::borrow::Cow::Borrowed(v)).to_string();
            match k {
                "xt" => {
                    // e.g. urn:btih:<hash> or urn:btmh:<hash>
                    let lower = decoded_val.to_lowercase();
                    if let Some(idx) = lower.find("urn:btih:") {
                        let raw_hash = &lower[idx + 9..];
                        if raw_hash.len() == 40 {
                            info_hash = raw_hash.to_string();
                        } else if raw_hash.len() == 32 {
                            // Base32 encoded SHA-1 hash -> convert to 40 hex chars
                            if let Ok(hex) = base32_to_hex(raw_hash) {
                                info_hash = hex;
                            } else {
                                info_hash = raw_hash.to_string();
                            }
                        } else {
                            info_hash = raw_hash.to_string();
                        }
                    }
                }
                "dn" => {
                    name = crate::proxy::sanitize_filename(&decoded_val);
                }
                "tr" => {
                    if !decoded_val.is_empty() && !trackers.contains(&decoded_val) {
                        trackers.push(decoded_val);
                    }
                }
                "ws" => {
                    if !decoded_val.is_empty() && !web_seeds.contains(&decoded_val) {
                        web_seeds.push(decoded_val);
                    }
                }
                "xl" => {
                    if let Ok(len) = decoded_val.parse::<u64>() {
                        exact_length = Some(len);
                    }
                }
                _ => {}
            }
        }
    }

    if info_hash.is_empty() {
        return Err("Invalid magnet URI: missing 'xt=urn:btih:' info-hash parameter".to_string());
    }

    if name.is_empty() {
        name = format!("torrent_{}", &info_hash[..8.min(info_hash.len())]);
    }

    Ok(MagnetLink {
        info_hash,
        name,
        trackers,
        web_seeds,
        exact_length,
    })
}

/// Decodes standard Base32 string into hexadecimal string
fn base32_to_hex(b32: &str) -> Result<String, ()> {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut bits = 0u32;
    let mut bit_count = 0;
    let mut bytes = Vec::new();

    for byte in b32.to_lowercase().bytes() {
        let val = ALPHABET.iter().position(|&c| c == byte).ok_or(())? as u32;
        bits = (bits << 5) | val;
        bit_count += 5;
        if bit_count >= 8 {
            bit_count -= 8;
            bytes.push((bits >> bit_count) as u8);
            bits &= (1 << bit_count) - 1;
        }
    }

    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        hex.push_str(&format!("{:02x}", b));
    }
    Ok(hex)
}

/// Standard Bencode Representation
#[derive(Debug, Clone, PartialEq)]
pub enum BencodeValue {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<BencodeValue>),
    Dict(BTreeMap<String, BencodeValue>),
}

impl BencodeValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            BencodeValue::Bytes(b) => std::str::from_utf8(b).ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            BencodeValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_dict(&self) -> Option<&BTreeMap<String, BencodeValue>> {
        match self {
            BencodeValue::Dict(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&Vec<BencodeValue>> {
        match self {
            BencodeValue::List(l) => Some(l),
            _ => None,
        }
    }

    /// Serializes value back to Bencode bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            BencodeValue::Int(i) => {
                out.extend_from_slice(format!("i{}e", i).as_bytes());
            }
            BencodeValue::Bytes(b) => {
                out.extend_from_slice(format!("{}:", b.len()).as_bytes());
                out.extend_from_slice(b);
            }
            BencodeValue::List(l) => {
                out.push(b'l');
                for item in l {
                    out.extend_from_slice(&item.encode());
                }
                out.push(b'e');
            }
            BencodeValue::Dict(d) => {
                out.push(b'd');
                for (k, v) in d {
                    let k_bytes = k.as_bytes();
                    out.extend_from_slice(format!("{}:", k_bytes.len()).as_bytes());
                    out.extend_from_slice(k_bytes);
                    out.extend_from_slice(&v.encode());
                }
                out.push(b'e');
            }
        }
        out
    }
}

/// Parses raw bytes into BencodeValue
pub fn parse_bencode(bytes: &[u8]) -> Result<BencodeValue, String> {
    let mut cursor = 0;
    parse_bencode_recursive(bytes, &mut cursor)
}

fn parse_bencode_recursive(bytes: &[u8], cursor: &mut usize) -> Result<BencodeValue, String> {
    if *cursor >= bytes.len() {
        return Err("Unexpected end of bencode data".to_string());
    }

    match bytes[*cursor] {
        b'i' => {
            *cursor += 1;
            let start = *cursor;
            while *cursor < bytes.len() && bytes[*cursor] != b'e' {
                *cursor += 1;
            }
            if *cursor >= bytes.len() {
                return Err("Unterminated bencode integer".to_string());
            }
            let s = std::str::from_utf8(&bytes[start..*cursor])
                .map_err(|e| format!("Invalid integer utf-8: {}", e))?;
            let val = s.parse::<i64>().map_err(|e| format!("Invalid integer format: {}", e))?;
            *cursor += 1; // skip 'e'
            Ok(BencodeValue::Int(val))
        }
        b'l' => {
            *cursor += 1;
            let mut list = Vec::new();
            while *cursor < bytes.len() && bytes[*cursor] != b'e' {
                list.push(parse_bencode_recursive(bytes, cursor)?);
            }
            if *cursor >= bytes.len() {
                return Err("Unterminated bencode list".to_string());
            }
            *cursor += 1; // skip 'e'
            Ok(BencodeValue::List(list))
        }
        b'd' => {
            *cursor += 1;
            let mut dict = BTreeMap::new();
            while *cursor < bytes.len() && bytes[*cursor] != b'e' {
                let key_val = parse_bencode_recursive(bytes, cursor)?;
                let key = match key_val {
                    BencodeValue::Bytes(b) => String::from_utf8_lossy(&b).to_string(),
                    _ => return Err("Bencode dict key must be a byte string".to_string()),
                };
                let value = parse_bencode_recursive(bytes, cursor)?;
                dict.insert(key, value);
            }
            if *cursor >= bytes.len() {
                return Err("Unterminated bencode dictionary".to_string());
            }
            *cursor += 1; // skip 'e'
            Ok(BencodeValue::Dict(dict))
        }
        b'0'..=b'9' => {
            let start = *cursor;
            while *cursor < bytes.len() && bytes[*cursor] >= b'0' && bytes[*cursor] <= b'9' {
                *cursor += 1;
            }
            if *cursor >= bytes.len() || bytes[*cursor] != b':' {
                return Err("Invalid byte string length delimiter".to_string());
            }
            let len_str = std::str::from_utf8(&bytes[start..*cursor])
                .map_err(|e| format!("Invalid string len: {}", e))?;
            let len = len_str.parse::<usize>().map_err(|e| format!("Invalid len parse: {}", e))?;
            *cursor += 1; // skip ':'

            if *cursor + len > bytes.len() {
                return Err("Byte string length exceeds buffer".to_string());
            }
            let data = bytes[*cursor..*cursor + len].to_vec();
            *cursor += len;
            Ok(BencodeValue::Bytes(data))
        }
        c => Err(format!("Invalid bencode token: '{}' (byte {})", c as char, c)),
    }
}

/// Parses an official `.torrent` file payload into structured TorrentMetadata
pub fn parse_torrent_file(torrent_bytes: &[u8]) -> Result<TorrentMetadata, String> {
    let root = parse_bencode(torrent_bytes)?;
    let dict = root.as_dict().ok_or_else(|| "Torrent root must be a dictionary".to_string())?;

    let announce = dict.get("announce").and_then(|v| v.as_str()).map(|s| s.to_string());
    let mut announce_list = Vec::new();

    if let Some(ann_list) = dict.get("announce-list").and_then(|v| v.as_list()) {
        for tier in ann_list {
            if let Some(tier_list) = tier.as_list() {
                for tr in tier_list {
                    if let Some(s) = tr.as_str() {
                        if !announce_list.contains(&s.to_string()) {
                            announce_list.push(s.to_string());
                        }
                    }
                }
            }
        }
    }
    if let Some(ref a) = announce {
        if !announce_list.contains(a) {
            announce_list.insert(0, a.clone());
        }
    }

    let mut web_seeds = Vec::new();
    if let Some(ws) = dict.get("url-list") {
        if let Some(s) = ws.as_str() {
            web_seeds.push(s.to_string());
        } else if let Some(l) = ws.as_list() {
            for item in l {
                if let Some(s) = item.as_str() {
                    web_seeds.push(s.to_string());
                }
            }
        }
    }

    let info_val = dict.get("info").ok_or_else(|| "Missing 'info' dictionary in torrent file".to_string())?;
    let info_dict = info_val.as_dict().ok_or_else(|| "'info' must be a dictionary".to_string())?;

    // Compute official SHA-1 Info-Hash of the bencoded 'info' dict
    let info_encoded = info_val.encode();
    let info_hash = sha1_hex(&info_encoded);

    let raw_name = info_dict.get("name").and_then(|v| v.as_str()).unwrap_or("download");
    let name = crate::proxy::sanitize_filename(raw_name);

    let piece_length = info_dict.get("piece length").and_then(|v| v.as_i64()).unwrap_or(0) as u64;
    let pieces_count = info_dict.get("pieces").map(|v| match v {
        BencodeValue::Bytes(b) => b.len() / 20,
        _ => 0,
    }).unwrap_or(0);

    let mut files = Vec::new();
    let mut total_length = 0u64;

    if let Some(length) = info_dict.get("length").and_then(|v| v.as_i64()) {
        // Single file mode
        let len = length as u64;
        total_length = len;
        files.push(TorrentFileInfo {
            path: name.clone(),
            length: len,
        });
    } else if let Some(files_list) = info_dict.get("files").and_then(|v| v.as_list()) {
        // Multi-file mode
        for f in files_list {
            if let Some(f_dict) = f.as_dict() {
                let f_len = f_dict.get("length").and_then(|v| v.as_i64()).unwrap_or(0) as u64;
                total_length += f_len;

                let mut path_parts = Vec::new();
                if let Some(path_list) = f_dict.get("path").and_then(|v| v.as_list()) {
                    for p in path_list {
                        if let Some(p_str) = p.as_str() {
                            path_parts.push(crate::proxy::sanitize_filename(p_str));
                        }
                    }
                }
                let relative_path = path_parts.join("/");
                files.push(TorrentFileInfo {
                    path: relative_path,
                    length: f_len,
                });
            }
        }
    }

    Ok(TorrentMetadata {
        info_hash,
        name,
        piece_length,
        pieces_count,
        total_length,
        files,
        announce,
        announce_list,
        web_seeds,
    })
}

/// Pure Rust Standard SHA-1 Hasher (RFC 3174)
/// Computes 160-bit SHA-1 hash and formats as a 40-character lowercase hexadecimal string
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let msg_len = data.len();
    let bit_len = (msg_len as u64) * 8;

    // Pad message to multiple of 64 bytes (512 bits)
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };

            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    format!("{:08x}{:08x}{:08x}{:08x}{:08x}", h0, h1, h2, h3, h4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_3_sha1_hasher_standard_vectors() {
        // RFC 3174 test vectors
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn test_feature_3_magnet_link_parsing_and_serialization() {
        let uri = "magnet:?xt=urn:btih:4a2d8e3b7c1f6a9e0d2c5b8a1f4e7d0c3b6a9e2f&dn=Ubuntu-24.04-Desktop.iso&xl=5000000000&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&ws=https%3A%2F%2Freleases.ubuntu.com%2F24.04%2Fubuntu-24.04-desktop-amd64.iso";
        let parsed = parse_magnet(uri).expect("Failed to parse valid magnet link");

        assert_eq!(parsed.info_hash, "4a2d8e3b7c1f6a9e0d2c5b8a1f4e7d0c3b6a9e2f");
        assert_eq!(parsed.name, "Ubuntu-24.04-Desktop.iso");
        assert_eq!(parsed.exact_length, Some(5000000000));
        assert_eq!(parsed.trackers, vec!["udp://tracker.opentrackr.org:1337/announce"]);
        assert_eq!(parsed.web_seeds, vec!["https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso"]);

        // Verify roundtrip serialization
        let generated_uri = parsed.to_uri();
        assert!(generated_uri.contains("xt=urn:btih:4a2d8e3b7c1f6a9e0d2c5b8a1f4e7d0c3b6a9e2f"));
        assert!(generated_uri.contains("dn=Ubuntu-24.04-Desktop.iso"));
        assert!(generated_uri.contains("xl=5000000000"));
        assert!(generated_uri.contains("ws="));
    }

    #[test]
    fn test_feature_3_bencode_serialization_and_deserialization() {
        // String
        let b_str = parse_bencode(b"4:spam").unwrap();
        assert_eq!(b_str.as_str(), Some("spam"));
        assert_eq!(b_str.encode(), b"4:spam");

        // Int
        let b_int = parse_bencode(b"i42e").unwrap();
        assert_eq!(b_int.as_i64(), Some(42));
        assert_eq!(b_int.encode(), b"i42e");

        // List
        let b_list = parse_bencode(b"l4:spami42ee").unwrap();
        assert_eq!(b_list.as_list().unwrap().len(), 2);
        assert_eq!(b_list.encode(), b"l4:spami42ee");

        // Dict
        let b_dict = parse_bencode(b"d3:bar4:spam3:fooi42ee").unwrap();
        let dict_map = b_dict.as_dict().unwrap();
        assert_eq!(dict_map.get("bar").unwrap().as_str(), Some("spam"));
        assert_eq!(dict_map.get("foo").unwrap().as_i64(), Some(42));
        assert_eq!(b_dict.encode(), b"d3:bar4:spam3:fooi42ee");
    }

    #[test]
    fn test_feature_3_torrent_file_metadata_and_info_hash() {
        // Build a synthetic valid .torrent bencode dictionary
        let mut info_dict = BTreeMap::new();
        info_dict.insert("name".to_string(), BencodeValue::Bytes(b"archlinux-2024.iso".to_vec()));
        info_dict.insert("piece length".to_string(), BencodeValue::Int(524288));
        info_dict.insert("length".to_string(), BencodeValue::Int(1048576000));
        info_dict.insert("pieces".to_string(), BencodeValue::Bytes(vec![0xAA; 40])); // 2 pieces

        let info_val = BencodeValue::Dict(info_dict);
        let expected_info_hash = sha1_hex(&info_val.encode());

        let mut torrent_dict = BTreeMap::new();
        torrent_dict.insert("announce".to_string(), BencodeValue::Bytes(b"http://tracker.archlinux.org/announce".to_vec()));
        torrent_dict.insert("url-list".to_string(), BencodeValue::Bytes(b"https://geo.mirror.pkgbuild.com/iso/archlinux.iso".to_vec()));
        torrent_dict.insert("info".to_string(), info_val);

        let torrent_root = BencodeValue::Dict(torrent_dict);
        let torrent_bytes = torrent_root.encode();

        let parsed = parse_torrent_file(&torrent_bytes).expect("Failed to parse synthetic torrent file");
        assert_eq!(parsed.name, "archlinux-2024.iso");
        assert_eq!(parsed.info_hash, expected_info_hash);
        assert_eq!(parsed.piece_length, 524288);
        assert_eq!(parsed.pieces_count, 2);
        assert_eq!(parsed.total_length, 1048576000);
        assert_eq!(parsed.announce, Some("http://tracker.archlinux.org/announce".to_string()));
        assert_eq!(parsed.web_seeds, vec!["https://geo.mirror.pkgbuild.com/iso/archlinux.iso"]);
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].path, "archlinux-2024.iso");
        assert_eq!(parsed.files[0].length, 1048576000);

        // Verify conversion from TorrentMetadata to MagnetLink URI
        let magnet_uri = parsed.to_magnet_uri();
        assert!(magnet_uri.starts_with("magnet:?xt=urn:btih:"));
        assert!(magnet_uri.contains(&expected_info_hash));
        assert!(magnet_uri.contains("dn=archlinux-2024.iso"));
        assert!(magnet_uri.contains("xl=1048576000"));
        assert!(magnet_uri.contains("ws=https%3A%2F%2Fgeo.mirror.pkgbuild.com%2Fiso%2Farchlinux.iso"));
    }

    #[test]
    fn test_feature_3_multi_file_torrent_parsing() {
        let mut f1 = BTreeMap::new();
        f1.insert("length".to_string(), BencodeValue::Int(15000000));
        let path1 = vec![
            BencodeValue::Bytes(b"Album".to_vec()),
            BencodeValue::Bytes(b"01_Track.flac".to_vec()),
        ];
        f1.insert("path".to_string(), BencodeValue::List(path1));

        let mut f2 = BTreeMap::new();
        f2.insert("length".to_string(), BencodeValue::Int(500000));
        let path2 = vec![
            BencodeValue::Bytes(b"Album".to_vec()),
            BencodeValue::Bytes(b"cover.jpg".to_vec()),
        ];
        f2.insert("path".to_string(), BencodeValue::List(path2));

        let files_list = vec![BencodeValue::Dict(f1), BencodeValue::Dict(f2)];

        let mut info_dict = BTreeMap::new();
        info_dict.insert("name".to_string(), BencodeValue::Bytes(b"Album".to_vec()));
        info_dict.insert("piece length".to_string(), BencodeValue::Int(262144));
        info_dict.insert("pieces".to_string(), BencodeValue::Bytes(vec![0x11; 20]));
        info_dict.insert("files".to_string(), BencodeValue::List(files_list));

        let mut torrent_dict = BTreeMap::new();
        torrent_dict.insert("info".to_string(), BencodeValue::Dict(info_dict));

        let torrent_bytes = BencodeValue::Dict(torrent_dict).encode();
        let parsed = parse_torrent_file(&torrent_bytes).expect("Failed to parse multi-file torrent");

        assert_eq!(parsed.name, "Album");
        assert_eq!(parsed.total_length, 15500000);
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(parsed.files[0].path, "Album/01_Track.flac");
        assert_eq!(parsed.files[0].length, 15000000);
        assert_eq!(parsed.files[1].path, "Album/cover.jpg");
        assert_eq!(parsed.files[1].length, 500000);
    }

    #[test]
    fn test_feature_3_magnet_base32_hash_decoding() {
        // Base32 32-char string: "jbswy3dpehpk3pxp" repeated/padded
        // 20 zero bytes in base32 is 32 'a's:
        let b32 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let hex = base32_to_hex(b32).expect("Base32 decode failed");
        assert_eq!(hex, "0000000000000000000000000000000000000000");

        let magnet_uri = format!("magnet:?xt=urn:btih:{}&dn=zero.bin", b32);
        let parsed = parse_magnet(&magnet_uri).expect("Failed to parse base32 magnet");
        assert_eq!(parsed.info_hash, "0000000000000000000000000000000000000000");
        assert_eq!(parsed.name, "zero.bin");
    }

    #[test]
    fn test_feature_3_bencode_error_handling_and_corrupt_data() {
        // Unterminated integer
        assert!(parse_bencode(b"i42").is_err());
        // Invalid integer characters
        assert!(parse_bencode(b"i42xfe").is_err());
        // Unterminated list
        assert!(parse_bencode(b"l4:spami42").is_err());
        // Unterminated dictionary
        assert!(parse_bencode(b"d3:bar").is_err());
        // Buffer overflow byte string length
        assert!(parse_bencode(b"999:short").is_err());
        // Invalid leading token
        assert!(parse_bencode(b"xyz").is_err());
        // Empty buffer
        assert!(parse_bencode(b"").is_err());
    }
}
