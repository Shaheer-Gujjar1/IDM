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

/// Computes raw 20-byte SHA-1 digest
pub fn sha1_raw(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let msg_len = data.len();
    let bit_len = (msg_len as u64) * 8;

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

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

/// Parses 40-character hex string into 20 raw bytes
pub fn hex_to_bytes20(hex: &str) -> Result<[u8; 20], String> {
    if hex.len() != 40 {
        return Err("Info-hash hex string must be exactly 40 characters".to_string());
    }
    let mut out = [0u8; 20];
    for i in 0..20 {
        let byte_str = &hex[i * 2..i * 2 + 2];
        out[i] = u8::from_str_radix(byte_str, 16)
            .map_err(|e| format!("Invalid hex character in hash: {}", e))?;
    }
    Ok(out)
}

/// Generates a standard BitTorrent Peer ID identifying IDM 0.9.0 (-ID0900-)
pub fn generate_peer_id() -> [u8; 20] {
    let mut peer_id = [0u8; 20];
    peer_id[0..8].copy_from_slice(b"-ID0900-");
    for i in 8..20 {
        peer_id[i] = (rand::random::<u8>() % 26) + b'a';
    }
    peer_id
}

/// Parses BEP 23 compact binary peer lists (6 bytes: 4 bytes IPv4 + 2 bytes port)
pub fn parse_compact_peers(bytes: &[u8]) -> Vec<std::net::SocketAddr> {
    let mut peers = Vec::new();
    for chunk in bytes.chunks_exact(6) {
        let ip = std::net::Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
        let port = u16::from_be_bytes([chunk[4], chunk[5]]);
        if port > 0 {
            peers.push(std::net::SocketAddr::V4(std::net::SocketAddrV4::new(ip, port)));
        }
    }
    peers
}

pub const PSTR: &[u8] = b"BitTorrent protocol";

/// Creates standard 68-byte BitTorrent peer wire handshake
pub fn create_peer_handshake(info_hash: &[u8; 20], peer_id: &[u8; 20]) -> [u8; 68] {
    let mut buf = [0u8; 68];
    buf[0] = 19;
    buf[1..20].copy_from_slice(PSTR);
    // 8 reserved bytes (indices 20..28) remain 0x00
    buf[28..48].copy_from_slice(info_hash);
    buf[48..68].copy_from_slice(peer_id);
    buf
}

/// Parses and validates incoming 68-byte peer handshake
pub fn parse_peer_handshake(buf: &[u8; 68]) -> Result<([u8; 20], [u8; 20]), String> {
    if buf[0] != 19 || &buf[1..20] != PSTR {
        return Err("Invalid BitTorrent handshake protocol string".to_string());
    }
    let mut info_hash = [0u8; 20];
    let mut peer_id = [0u8; 20];
    info_hash.copy_from_slice(&buf[28..48]);
    peer_id.copy_from_slice(&buf[48..68]);
    Ok((info_hash, peer_id))
}

/// BitTorrent Peer Wire Protocol (BEP 3) Message Representation
#[derive(Debug, Clone, PartialEq)]
pub enum PeerWireMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request { index: u32, begin: u32, length: u32 },
    Piece { index: u32, begin: u32, block: Vec<u8> },
}

impl PeerWireMessage {
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            PeerWireMessage::KeepAlive => vec![0, 0, 0, 0],
            PeerWireMessage::Choke => vec![0, 0, 0, 1, 0],
            PeerWireMessage::Unchoke => vec![0, 0, 0, 1, 1],
            PeerWireMessage::Interested => vec![0, 0, 0, 1, 2],
            PeerWireMessage::NotInterested => vec![0, 0, 0, 1, 3],
            PeerWireMessage::Have(index) => {
                let mut out = vec![0, 0, 0, 5, 4];
                out.extend_from_slice(&index.to_be_bytes());
                out
            }
            PeerWireMessage::Bitfield(bits) => {
                let len = (1 + bits.len()) as u32;
                let mut out = len.to_be_bytes().to_vec();
                out.push(5);
                out.extend_from_slice(bits);
                out
            }
            PeerWireMessage::Request { index, begin, length } => {
                let mut out = vec![0, 0, 0, 13, 6];
                out.extend_from_slice(&index.to_be_bytes());
                out.extend_from_slice(&begin.to_be_bytes());
                out.extend_from_slice(&length.to_be_bytes());
                out
            }
            PeerWireMessage::Piece { index, begin, block } => {
                let len = (9 + block.len()) as u32;
                let mut out = len.to_be_bytes().to_vec();
                out.push(7);
                out.extend_from_slice(&index.to_be_bytes());
                out.extend_from_slice(&begin.to_be_bytes());
                out.extend_from_slice(block);
                out
            }
        }
    }

    pub fn parse(buf: &[u8]) -> Result<Option<(PeerWireMessage, usize)>, String> {
        if buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if len == 0 {
            return Ok(Some((PeerWireMessage::KeepAlive, 4)));
        }
        if buf.len() < 4 + len {
            return Ok(None); // Need more bytes from TCP stream
        }

        let id = buf[4];
        let payload = &buf[5..4 + len];

        let msg = match id {
            0 => PeerWireMessage::Choke,
            1 => PeerWireMessage::Unchoke,
            2 => PeerWireMessage::Interested,
            3 => PeerWireMessage::NotInterested,
            4 => {
                if payload.len() < 4 {
                    return Err("Malformed Have message".to_string());
                }
                let idx = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                PeerWireMessage::Have(idx)
            }
            5 => PeerWireMessage::Bitfield(payload.to_vec()),
            6 => {
                if payload.len() < 12 {
                    return Err("Malformed Request message".to_string());
                }
                let index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                let length = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
                PeerWireMessage::Request { index, begin, length }
            }
            7 => {
                if payload.len() < 8 {
                    return Err("Malformed Piece message".to_string());
                }
                let index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                let block = payload[8..].to_vec();
                PeerWireMessage::Piece { index, begin, block }
            }
            _ => PeerWireMessage::KeepAlive,
        };

        Ok(Some((msg, 4 + len)))
    }
}

/// UDP Tracker Announcer (BEP 15)
pub async fn announce_udp_tracker(
    tracker_host: &str,
    tracker_port: u16,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    left: u64,
) -> Result<Vec<std::net::SocketAddr>, String> {
    use tokio::net::UdpSocket;
    use tokio::time::{timeout, Duration};

    let bind_addr = "0.0.0.0:0";
    let socket = UdpSocket::bind(bind_addr)
        .await
        .map_err(|e| format!("Failed to bind UDP socket: {}", e))?;

    let dest_addr = format!("{}:{}", tracker_host, tracker_port);
    socket.connect(&dest_addr)
        .await
        .map_err(|e| format!("Failed to connect UDP tracker: {}", e))?;

    // 1. Send Connect Request (BEP 15)
    // protocol_id: 0x41727101980 (64-bit int)
    // action: 0 (connect)
    // transaction_id: random u32
    let mut connect_req = [0u8; 16];
    connect_req[0..8].copy_from_slice(&0x41727101980i64.to_be_bytes());
    connect_req[8..12].copy_from_slice(&0u32.to_be_bytes());
    let transaction_id = rand::random::<u32>();
    connect_req[12..16].copy_from_slice(&transaction_id.to_be_bytes());

    socket.send(&connect_req).await.map_err(|e| e.to_string())?;

    let mut connect_resp = [0u8; 16];
    let n = timeout(Duration::from_secs(5), socket.recv(&mut connect_resp))
        .await
        .map_err(|_| "UDP tracker connect timed out".to_string())?
        .map_err(|e| e.to_string())?;

    if n < 16 {
        return Err("Invalid UDP tracker connect response length".to_string());
    }

    let action = u32::from_be_bytes([connect_resp[0], connect_resp[1], connect_resp[2], connect_resp[3]]);
    let resp_trans_id = u32::from_be_bytes([connect_resp[4], connect_resp[5], connect_resp[6], connect_resp[7]]);
    let connection_id = [
        connect_resp[8], connect_resp[9], connect_resp[10], connect_resp[11],
        connect_resp[12], connect_resp[13], connect_resp[14], connect_resp[15],
    ];

    if action != 0 || resp_trans_id != transaction_id {
        return Err("UDP tracker connect transaction mismatch".to_string());
    }

    // 2. Send Announce Request
    let mut announce_req = [0u8; 98];
    announce_req[0..8].copy_from_slice(&connection_id);
    announce_req[8..12].copy_from_slice(&1u32.to_be_bytes()); // action = announce (1)
    let ann_trans_id = rand::random::<u32>();
    announce_req[12..16].copy_from_slice(&ann_trans_id.to_be_bytes());
    announce_req[16..36].copy_from_slice(info_hash);
    announce_req[36..56].copy_from_slice(peer_id);
    announce_req[56..64].copy_from_slice(&0u64.to_be_bytes()); // downloaded
    announce_req[64..72].copy_from_slice(&left.to_be_bytes()); // left
    announce_req[72..80].copy_from_slice(&0u64.to_be_bytes()); // uploaded
    announce_req[80..84].copy_from_slice(&2u32.to_be_bytes()); // event: 2 (started)
    announce_req[84..88].copy_from_slice(&0u32.to_be_bytes()); // IP address (0 = default)
    announce_req[88..92].copy_from_slice(&rand::random::<u32>().to_be_bytes()); // key
    announce_req[92..96].copy_from_slice(&(-1i32).to_be_bytes()); // num_want (-1)
    announce_req[96..98].copy_from_slice(&6881u16.to_be_bytes()); // port

    socket.send(&announce_req).await.map_err(|e| e.to_string())?;

    let mut ann_resp = vec![0u8; 1024];
    let n = timeout(Duration::from_secs(5), socket.recv(&mut ann_resp))
        .await
        .map_err(|_| "UDP tracker announce timed out".to_string())?
        .map_err(|e| e.to_string())?;

    if n < 20 {
        return Err("Truncated UDP tracker announce response".to_string());
    }

    let ann_action = u32::from_be_bytes([ann_resp[0], ann_resp[1], ann_resp[2], ann_resp[3]]);
    if ann_action != 1 {
        return Err("UDP tracker returned error action".to_string());
    }

    // Peers are in compact binary format from byte 20 onwards
    Ok(parse_compact_peers(&ann_resp[20..n]))
}

/// HTTP Tracker Announcer (Standard BEP 3)
pub async fn announce_http_tracker(
    tracker_url: &str,
    info_hash: &[u8; 20],
    peer_id: &[u8; 20],
    left: u64,
) -> Result<Vec<std::net::SocketAddr>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let mut hash_encoded = String::new();
    for b in info_hash {
        hash_encoded.push_str(&format!("%{:02X}", b));
    }

    let mut peer_encoded = String::new();
    for b in peer_id {
        peer_encoded.push_str(&format!("%{:02X}", b));
    }

    let sep = if tracker_url.contains('?') { "&" } else { "?" };
    let full_url = format!(
        "{}{}info_hash={}&peer_id={}&port=6881&uploaded=0&downloaded=0&left={}&compact=1&event=started",
        tracker_url, sep, hash_encoded, peer_encoded, left
    );

    let resp = client.get(&full_url).send().await.map_err(|e| e.to_string())?;
    let body = resp.bytes().await.map_err(|e| e.to_string())?;

    let bencode = parse_bencode(&body)?;
    let dict = bencode.as_dict().ok_or("Tracker response must be a bencoded dictionary")?;

    if let Some(failure) = dict.get("failure reason").and_then(|v| v.as_str()) {
        return Err(format!("Tracker returned failure: {}", failure));
    }

    if let Some(peers_val) = dict.get("peers") {
        match peers_val {
            BencodeValue::Bytes(b) => Ok(parse_compact_peers(b)),
            BencodeValue::List(l) => {
                let mut peers = Vec::new();
                for item in l {
                    if let Some(pdict) = item.as_dict() {
                        if let (Some(ip_str), Some(port)) = (
                            pdict.get("ip").and_then(|v| v.as_str()),
                            pdict.get("port").and_then(|v| v.as_i64()),
                        ) {
                            if let Ok(ip) = ip_str.parse::<std::net::IpAddr>() {
                                peers.push(std::net::SocketAddr::new(ip, port as u16));
                            }
                        }
                    }
                }
                Ok(peers)
            }
            _ => Ok(vec![]),
        }
    } else {
        Ok(vec![])
    }
}

/// Standard BitTorrent block size (16 KiB)
pub const BLOCK_SIZE: u32 = 16384;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmPeerInfo {
    pub addr: String,
    pub connected: bool,
    pub peer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmProgress {
    pub total_pieces: usize,
    pub completed_pieces: usize,
    pub connected_peers: usize,
    pub downloaded_bytes: u64,
}

/// Native Pure-Rust BitTorrent Swarm P2P Downloader (BEP 3, BEP 15, BEP 23)
pub async fn run_p2p_swarm_download(
    info_hash: [u8; 20],
    peer_id: [u8; 20],
    piece_length: u64,
    total_length: u64,
    piece_hashes: Vec<[u8; 20]>,
    trackers: Vec<String>,
    direct_peers: Vec<std::net::SocketAddr>,
    dest_path: std::path::PathBuf,
    downloaded_tracker: std::sync::Arc<std::sync::atomic::AtomicU64>,
    mut abort_rx: Option<tokio::sync::broadcast::Receiver<()>>,
) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncSeekExt};
    use tokio::net::TcpStream;
    use tokio::time::{timeout, Duration};
    use std::sync::atomic::Ordering;

    // 1. Gather all candidate swarm peers from trackers & direct peer hints
    let mut candidate_peers = direct_peers.clone();

    for tr in &trackers {
        let tr_clean = tr.trim();
        if tr_clean.starts_with("udp://") {
            if let Ok(url) = reqwest::Url::parse(tr_clean) {
                if let (Some(host), Some(port)) = (url.host_str(), url.port()) {
                    if let Ok(found) = announce_udp_tracker(host, port, &info_hash, &peer_id, total_length).await {
                        for p in found {
                            if !candidate_peers.contains(&p) {
                                candidate_peers.push(p);
                            }
                        }
                    }
                }
            }
        } else if tr_clean.starts_with("http://") || tr_clean.starts_with("https://") {
            if let Ok(found) = announce_http_tracker(tr_clean, &info_hash, &peer_id, total_length).await {
                for p in found {
                    if !candidate_peers.contains(&p) {
                        candidate_peers.push(p);
                    }
                }
            }
        }
    }

    if candidate_peers.is_empty() {
        return Err("No active BitTorrent peers found in swarm across configured trackers".to_string());
    }

    // 2. Prepare destination file
    if let Some(parent) = dest_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let mut dest_file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&dest_path)
        .await
        .map_err(|e| format!("Cannot open target file for swarm writing: {}", e))?;

    if total_length > 0 {
        let _ = dest_file.set_len(total_length).await;
    }

    let total_pieces = if piece_length > 0 && total_length > 0 {
        ((total_length + piece_length - 1) / piece_length) as usize
    } else if !piece_hashes.is_empty() {
        piece_hashes.len()
    } else {
        1
    };

    let handshake_bytes = create_peer_handshake(&info_hash, &peer_id);
    let mut completed_pieces = 0usize;

    // 3. Connect to peers and download pieces
    for peer_addr in candidate_peers {
        if completed_pieces >= total_pieces {
            break;
        }

        if let Some(ref mut rx) = abort_rx {
            if rx.try_recv().is_ok() {
                return Err("Download cancelled by user".to_string());
            }
        }

        let stream_res = timeout(Duration::from_secs(4), TcpStream::connect(peer_addr)).await;
        let mut stream = match stream_res {
            Ok(Ok(s)) => s,
            _ => continue, // Try next peer
        };

        // Send Handshake
        if stream.write_all(&handshake_bytes).await.is_err() {
            continue;
        }

        // Read Handshake
        let mut resp_handshake = [0u8; 68];
        if timeout(Duration::from_secs(5), stream.read_exact(&mut resp_handshake)).await.is_err() {
            continue;
        }

        let (resp_hash, _) = match parse_peer_handshake(&resp_handshake) {
            Ok(h) => h,
            Err(_) => continue,
        };

        if resp_hash != info_hash {
            continue; // Info hash mismatch
        }

        // Express interest and unchoke peer
        let _ = stream.write_all(&PeerWireMessage::Interested.serialize()).await;
        let _ = stream.write_all(&PeerWireMessage::Unchoke.serialize()).await;

        let mut read_buf = Vec::with_capacity(65536);
        let mut peer_choked = false;
        let mut incoming_tmp = [0u8; 16384];

        // Download loop for this peer
        'peer_loop: while completed_pieces < total_pieces {
            if let Some(ref mut rx) = abort_rx {
                if rx.try_recv().is_ok() {
                    return Err("Download cancelled by user".to_string());
                }
            }

            // If not choked, request next piece block
            if !peer_choked {
                let piece_idx = completed_pieces as u32;
                let cur_piece_len = if piece_idx as usize == total_pieces - 1 && total_length > 0 {
                    let rem = total_length % piece_length;
                    if rem == 0 { piece_length } else { rem }
                } else {
                    piece_length
                };

                let mut begin = 0u32;
                while (begin as u64) < cur_piece_len {
                    let block_len = ((cur_piece_len - begin as u64) as u32).min(BLOCK_SIZE);
                    let req_msg = PeerWireMessage::Request {
                        index: piece_idx,
                        begin,
                        length: block_len,
                    };
                    if stream.write_all(&req_msg.serialize()).await.is_err() {
                        break 'peer_loop;
                    }
                    begin += block_len;
                }
            }

            // Read message from stream
            let n = match timeout(Duration::from_secs(10), stream.read(&mut incoming_tmp)).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break 'peer_loop,
                Ok(Ok(n)) => n,
            };
            read_buf.extend_from_slice(&incoming_tmp[..n]);

            // Parse all complete messages in buffer
            loop {
                match PeerWireMessage::parse(&read_buf) {
                    Ok(Some((msg, bytes_consumed))) => {
                        read_buf.drain(0..bytes_consumed);
                        match msg {
                            PeerWireMessage::Choke => {
                                peer_choked = true;
                            }
                            PeerWireMessage::Unchoke => {
                                peer_choked = false;
                            }
                            PeerWireMessage::Piece { index, begin, block } => {
                                let block_len = block.len() as u64;
                                let offset = (index as u64) * piece_length + (begin as u64);
                                if dest_file.seek(std::io::SeekFrom::Start(offset)).await.is_ok() {
                                    let _ = dest_file.write_all(&block).await;
                                }
                                downloaded_tracker.fetch_add(block_len, Ordering::Relaxed);

                                let piece_total_len = if index as usize == total_pieces - 1 && total_length > 0 {
                                    let rem = total_length % piece_length;
                                    if rem == 0 { piece_length } else { rem }
                                } else {
                                    piece_length
                                };

                                if (begin as u64) + block_len >= piece_total_len {
                                    completed_pieces += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(None) => break, // Need more bytes from TCP stream
                    Err(_) => break 'peer_loop,
                }
            }
        }
    }

    let _ = dest_file.sync_all().await;
    Ok(dest_path.to_string_lossy().to_string())
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

    #[test]
    fn test_feature_3_peer_handshake_roundtrip() {
        let info_hash = hex_to_bytes20("4a2d8e3b7c1f6a9e0d2c5b8a1f4e7d0c3b6a9e2f").unwrap();
        let peer_id = generate_peer_id();

        let handshake = create_peer_handshake(&info_hash, &peer_id);
        assert_eq!(handshake.len(), 68);
        assert_eq!(handshake[0], 19);
        assert_eq!(&handshake[1..20], PSTR);

        let (parsed_hash, parsed_peer_id) = parse_peer_handshake(&handshake).unwrap();
        assert_eq!(parsed_hash, info_hash);
        assert_eq!(parsed_peer_id, peer_id);
    }

    #[test]
    fn test_feature_3_peer_wire_messages_serialization_and_parsing() {
        // KeepAlive
        let keep_alive = PeerWireMessage::KeepAlive;
        let bytes = keep_alive.serialize();
        assert_eq!(bytes, vec![0, 0, 0, 0]);
        let parsed = PeerWireMessage::parse(&bytes).unwrap().unwrap();
        assert_eq!(parsed.0, PeerWireMessage::KeepAlive);
        assert_eq!(parsed.1, 4);

        // Choke / Unchoke
        let unchoke_bytes = PeerWireMessage::Unchoke.serialize();
        assert_eq!(unchoke_bytes, vec![0, 0, 0, 1, 1]);
        let parsed_unchoke = PeerWireMessage::parse(&unchoke_bytes).unwrap().unwrap();
        assert_eq!(parsed_unchoke.0, PeerWireMessage::Unchoke);

        // Interested
        let interested_bytes = PeerWireMessage::Interested.serialize();
        assert_eq!(interested_bytes, vec![0, 0, 0, 1, 2]);
        let parsed_interested = PeerWireMessage::parse(&interested_bytes).unwrap().unwrap();
        assert_eq!(parsed_interested.0, PeerWireMessage::Interested);

        // Have
        let have_msg = PeerWireMessage::Have(42);
        let have_bytes = have_msg.serialize();
        let parsed_have = PeerWireMessage::parse(&have_bytes).unwrap().unwrap();
        assert_eq!(parsed_have.0, PeerWireMessage::Have(42));

        // Request
        let req_msg = PeerWireMessage::Request { index: 3, begin: 16384, length: 16384 };
        let req_bytes = req_msg.serialize();
        let parsed_req = PeerWireMessage::parse(&req_bytes).unwrap().unwrap();
        assert_eq!(parsed_req.0, PeerWireMessage::Request { index: 3, begin: 16384, length: 16384 });

        // Piece
        let block_data = b"IDM P2P SWARM BLOCK".to_vec();
        let piece_msg = PeerWireMessage::Piece { index: 0, begin: 0, block: block_data.clone() };
        let piece_bytes = piece_msg.serialize();
        let parsed_piece = PeerWireMessage::parse(&piece_bytes).unwrap().unwrap();
        assert_eq!(parsed_piece.0, PeerWireMessage::Piece { index: 0, begin: 0, block: block_data });
    }

    #[test]
    fn test_feature_3_compact_peer_parsing() {
        // 2 peers: 127.0.0.1:6881 (0x7F, 0x00, 0x00, 0x01, 0x1A, 0xE1)
        //          192.168.1.10:51413 (0xC0, 0xA8, 0x01, 0x0A, 0xC8, 0xD5)
        let raw = vec![
            127, 0, 0, 1, 0x1A, 0xE1,
            192, 168, 1, 10, 0xC8, 0xD5,
        ];
        let peers = parse_compact_peers(&raw);
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].to_string(), "127.0.0.1:6881");
        assert_eq!(peers[1].to_string(), "192.168.1.10:51413");
    }

    #[tokio::test]
    async fn test_feature_3_mock_p2p_swarm_transfer_and_hash_verification() {
        use tokio::net::TcpListener;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let info_hash = hex_to_bytes20("abcdefabcdefabcdefabcdefabcdefabcdefabcd").unwrap();
        let client_peer_id = generate_peer_id();
        let mock_peer_id = generate_peer_id();

        // 1. Start a mock BitTorrent peer listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_peer_addr = listener.local_addr().unwrap();

        let payload_piece = b"Pure Rust BitTorrent P2P Swarm Engine Piece Payload 1234567890".to_vec();
        let payload_piece_clone = payload_piece.clone();
        let info_hash_copy = info_hash;

        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                // Read client handshake
                let mut client_handshake = [0u8; 68];
                let _ = socket.read_exact(&mut client_handshake).await;

                // Send server handshake
                let server_handshake = create_peer_handshake(&info_hash_copy, &mock_peer_id);
                let _ = socket.write_all(&server_handshake).await;

                // Send unchoke
                let _ = socket.write_all(&PeerWireMessage::Unchoke.serialize()).await;

                // Read request message
                let mut req_buf = vec![0u8; 17];
                let _ = socket.read_exact(&mut req_buf).await;

                // Send piece message
                let piece_msg = PeerWireMessage::Piece {
                    index: 0,
                    begin: 0,
                    block: payload_piece_clone,
                };
                let _ = socket.write_all(&piece_msg.serialize()).await;
            }
        });

        // 2. Run swarm download targeting our mock peer
        let temp_dir = std::env::temp_dir().join(format!("idm_swarm_test_{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::create_dir_all(&temp_dir).await;
        let dest_file = temp_dir.join("swarm_download.bin");

        let piece_len = payload_piece.len() as u64;
        let downloaded = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

        let result = run_p2p_swarm_download(
            info_hash,
            client_peer_id,
            piece_len,
            piece_len,
            vec![sha1_raw(&payload_piece)],
            vec![],
            vec![mock_peer_addr],
            dest_file.clone(),
            downloaded.clone(),
            None,
        ).await;

        assert!(result.is_ok(), "Swarm download should complete successfully: {:?}", result);
        assert_eq!(downloaded.load(std::sync::atomic::Ordering::Relaxed), piece_len);

        // Verify content and SHA-1 hash on disk
        let saved_bytes = tokio::fs::read(&dest_file).await.unwrap();
        assert_eq!(saved_bytes, payload_piece);
        assert_eq!(sha1_raw(&saved_bytes), sha1_raw(&payload_piece));

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
