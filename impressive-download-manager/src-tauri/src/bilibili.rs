use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliPageItem {
    pub page: usize,
    pub cid: u64,
    pub part: String,
    pub duration_seconds: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliVideoInfo {
    pub bvid: String,
    pub aid: u64,
    pub title: String,
    pub author: String,
    pub thumbnail: String,
    pub description: String,
    pub duration_seconds: u64,
    pub page_count: usize,
    pub pages: Vec<BilibiliPageItem>,
    pub current_page: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BilibiliStreamStreams {
    pub video_url: String,
    pub audio_url: Option<String>,
    pub quality: u32,
    pub quality_desc: String,
    pub format: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BilibiliId {
    pub bvid: Option<String>,
    pub aid: Option<u64>,
    pub page: usize,
}

pub fn is_bilibili_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains("bilibili.com") || lower.contains("b23.tv")
}

pub fn extract_bilibili_id(url: &str) -> Option<BilibiliId> {
    let lower = url.to_lowercase();
    if !is_bilibili_url(&lower) {
        return None;
    }

    let mut page = 1;
    if let Ok(parsed) = reqwest::Url::parse(url) {
        for (k, v) in parsed.query_pairs() {
            if k == "p" {
                if let Ok(p_num) = v.parse::<usize>() {
                    page = p_num.max(1);
                }
            }
        }
    }

    // Check for BV ID (e.g. BV1xx411c7mD or /BV1...)
    let bv_pattern = regex::Regex::new(r"(?i)(BV[0-9a-zA-Z]{10})").ok()?;
    if let Some(caps) = bv_pattern.captures(url) {
        if let Some(m) = caps.get(1) {
            return Some(BilibiliId {
                bvid: Some(m.as_str().to_string()),
                aid: None,
                page,
            });
        }
    }

    // Check for AV ID (e.g. av170001 or /av170001)
    let av_pattern = regex::Regex::new(r"(?i)av([0-9]+)").ok()?;
    if let Some(caps) = av_pattern.captures(url) {
        if let Some(m) = caps.get(1) {
            if let Ok(aid_num) = m.as_str().parse::<u64>() {
                return Some(BilibiliId {
                    bvid: None,
                    aid: Some(aid_num),
                    page,
                });
            }
        }
    }

    None
}

pub fn get_bilibili_default_headers(cookie: Option<&str>) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert(
        "User-Agent".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
    );
    headers.insert(
        "Referer".to_string(),
        "https://www.bilibili.com/".to_string(),
    );
    headers.insert(
        "Accept".to_string(),
        "*/*".to_string(),
    );
    headers.insert(
        "Accept-Language".to_string(),
        "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7".to_string(),
    );

    if let Some(c) = cookie {
        if !c.trim().is_empty() {
            headers.insert("Cookie".to_string(), c.trim().to_string());
        }
    }

    headers
}

pub fn quality_code_to_desc(code: u32) -> &'static str {
    match code {
        127 => "8K Ultra HD",
        126 => "Dolby Vision",
        125 => "HDR High Dynamic",
        120 => "4K Ultra HD",
        116 => "1080p 60fps",
        112 => "1080p High Bitrate",
        80 => "1080p HD",
        64 => "720p HD",
        32 => "480p Clear",
        16 => "360p Fluency",
        _ => "Auto Quality",
    }
}

pub async fn resolve_bilibili_video(
    client: &reqwest::Client,
    url: &str,
    cookie: Option<&str>,
) -> Result<BilibiliVideoInfo, String> {
    let id_info = extract_bilibili_id(url).ok_or_else(|| {
        format!("Not a valid Bilibili video link (expected BV or av format): {}", url)
    })?;

    let api_url = if let Some(ref bv) = id_info.bvid {
        format!("https://api.bilibili.com/x/web-interface/view?bvid={}", bv)
    } else if let Some(aid) = id_info.aid {
        format!("https://api.bilibili.com/x/web-interface/view?aid={}", aid)
    } else {
        return Err("Unable to determine Bilibili video ID".to_string());
    };

    let mut req = client.get(&api_url);
    for (k, v) in get_bilibili_default_headers(cookie) {
        req = req.header(k, v);
    }

    let resp = req.send().await.map_err(|e| format!("Failed to fetch Bilibili video metadata: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Bilibili API returned HTTP status {}", resp.status()));
    }

    let body_json: serde_json::Value = resp.json().await.map_err(|e| format!("Invalid JSON from Bilibili API: {}", e))?;
    let code = body_json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);

    if code != 0 {
        let msg = body_json.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown Bilibili error");
        // Fallback to yt-dlp probe if web API returns restricted/geo-blocked error
        return fallback_yt_dlp_bilibili_resolve(url).await.map_err(|yt_err| {
            format!("Bilibili API error: {} (yt-dlp fallback also failed: {})", msg, yt_err)
        });
    }

    let data = body_json.get("data").ok_or_else(|| "Missing data field in Bilibili response".to_string())?;
    parse_bilibili_data_json(data, id_info.page)
}

pub fn parse_bilibili_data_json(data: &serde_json::Value, current_page: usize) -> Result<BilibiliVideoInfo, String> {
    let bvid = data.get("bvid").and_then(|b| b.as_str()).unwrap_or("").to_string();
    let aid = data.get("aid").and_then(|a| a.as_u64()).unwrap_or(0);
    let title = data.get("title").and_then(|t| t.as_str()).unwrap_or("Bilibili Video").to_string();
    let author = data.get("owner").and_then(|o| o.get("name")).and_then(|n| n.as_str()).unwrap_or("Bilibili Uploader").to_string();
    let thumbnail = data.get("pic").and_then(|p| p.as_str()).unwrap_or("").to_string();
    let description = data.get("desc").and_then(|d| d.as_str()).unwrap_or("").to_string();
    let duration_seconds = data.get("duration").and_then(|d| d.as_u64()).unwrap_or(0);

    let mut pages = Vec::new();
    if let Some(pages_arr) = data.get("pages").and_then(|p| p.as_array()) {
        for p in pages_arr {
            let p_num = p.get("page").and_then(|n| n.as_u64()).unwrap_or(1) as usize;
            let cid = p.get("cid").and_then(|c| c.as_u64()).unwrap_or(0);
            let part_title = p.get("part").and_then(|pt| pt.as_str()).unwrap_or("").to_string();
            let p_dur = p.get("duration").and_then(|d| d.as_u64()).unwrap_or(0);

            let part_url = if p_num == 1 {
                format!("https://www.bilibili.com/video/{}", bvid)
            } else {
                format!("https://www.bilibili.com/video/{}?p={}", bvid, p_num)
            };

            pages.push(BilibiliPageItem {
                page: p_num,
                cid,
                part: if part_title.is_empty() { format!("Part {}", p_num) } else { part_title },
                duration_seconds: p_dur,
                url: part_url,
            });
        }
    }

    if pages.is_empty() {
        pages.push(BilibiliPageItem {
            page: 1,
            cid: data.get("cid").and_then(|c| c.as_u64()).unwrap_or(0),
            part: title.clone(),
            duration_seconds,
            url: format!("https://www.bilibili.com/video/{}", bvid),
        });
    }

    let page_count = pages.len();
    Ok(BilibiliVideoInfo {
        bvid,
        aid,
        title,
        author,
        thumbnail,
        description,
        duration_seconds,
        page_count,
        pages,
        current_page: current_page.min(page_count).max(1),
    })
}

async fn fallback_yt_dlp_bilibili_resolve(url: &str) -> Result<BilibiliVideoInfo, String> {
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.args([
        "--dump-single-json",
        "--flat-playlist",
        "--referer", "https://www.bilibili.com/",
        "--add-header", "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        url,
    ]);

    let output = cmd.output().await.map_err(|e| format!("Failed to execute yt-dlp: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let json_val: serde_json::Value = serde_json::from_str(&stdout_str).map_err(|e| format!("Invalid yt-dlp JSON: {}", e))?;

    let title = json_val.get("title").and_then(|t| t.as_str()).unwrap_or("Bilibili Video").to_string();
    let author = json_val.get("uploader").and_then(|u| u.as_str()).unwrap_or("Bilibili Uploader").to_string();
    let thumbnail = json_val.get("thumbnail").and_then(|t| t.as_str()).unwrap_or("").to_string();
    let duration = json_val.get("duration").and_then(|d| d.as_u64()).unwrap_or(0);
    let desc = json_val.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
    let bvid = json_val.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();

    let mut pages = Vec::new();
    if let Some(entries) = json_val.get("entries").and_then(|e| e.as_array()) {
        for (idx, entry) in entries.iter().enumerate() {
            let part_title = entry.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let entry_url = entry.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string();
            let entry_dur = entry.get("duration").and_then(|d| d.as_u64()).unwrap_or(0);
            pages.push(BilibiliPageItem {
                page: idx + 1,
                cid: (idx + 1) as u64,
                part: if part_title.is_empty() { format!("Part {}", idx + 1) } else { part_title },
                duration_seconds: entry_dur,
                url: if entry_url.is_empty() { format!("https://www.bilibili.com/video/{}?p={}", bvid, idx + 1) } else { entry_url },
            });
        }
    }

    if pages.is_empty() {
        pages.push(BilibiliPageItem {
            page: 1,
            cid: 1,
            part: title.clone(),
            duration_seconds: duration,
            url: url.to_string(),
        });
    }

    let page_count = pages.len();
    Ok(BilibiliVideoInfo {
        bvid,
        aid: 0,
        title,
        author,
        thumbnail,
        description: desc,
        duration_seconds: duration,
        page_count,
        pages,
        current_page: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_bilibili_url() {
        assert!(is_bilibili_url("https://www.bilibili.com/video/BV1xx411c7mD"));
        assert!(is_bilibili_url("http://bilibili.com/video/av170001?p=3"));
        assert!(is_bilibili_url("https://b23.tv/BV1xx411c7mD"));
        assert!(!is_bilibili_url("https://youtube.com/watch?v=123"));
        assert!(!is_bilibili_url("https://example.com/video.mp4"));
    }

    #[test]
    fn test_extract_bilibili_id_bv() {
        let url = "https://www.bilibili.com/video/BV1GJ411x7h7/?spm_id_from=333.337";
        let id_info = extract_bilibili_id(url).expect("Should extract BV ID");
        assert_eq!(id_info.bvid.as_deref(), Some("BV1GJ411x7h7"));
        assert_eq!(id_info.page, 1);
    }

    #[test]
    fn test_extract_bilibili_id_multi_page() {
        let url = "https://www.bilibili.com/video/BV1GJ411x7h7?p=4";
        let id_info = extract_bilibili_id(url).expect("Should extract BV ID with page");
        assert_eq!(id_info.bvid.as_deref(), Some("BV1GJ411x7h7"));
        assert_eq!(id_info.page, 4);
    }

    #[test]
    fn test_extract_bilibili_id_av() {
        let url = "https://www.bilibili.com/video/av170001";
        let id_info = extract_bilibili_id(url).expect("Should extract AV ID");
        assert_eq!(id_info.aid, Some(170001));
    }

    #[test]
    fn test_bilibili_headers() {
        let headers = get_bilibili_default_headers(Some("SESSDATA=test_cookie_123"));
        assert_eq!(headers.get("Referer").unwrap(), "https://www.bilibili.com/");
        assert!(headers.get("User-Agent").unwrap().contains("Mozilla"));
        assert_eq!(headers.get("Cookie").unwrap(), "SESSDATA=test_cookie_123");
    }

    #[test]
    fn test_quality_code_to_desc() {
        assert_eq!(quality_code_to_desc(80), "1080p HD");
        assert_eq!(quality_code_to_desc(64), "720p HD");
        assert_eq!(quality_code_to_desc(120), "4K Ultra HD");
        assert_eq!(quality_code_to_desc(16), "360p Fluency");
    }

    #[test]
    fn test_parse_bilibili_data_json() {
        let sample = serde_json::json!({
            "bvid": "BV1xx411c7mD",
            "aid": 170001,
            "title": "Sample Bilibili Anime Episode",
            "desc": "An exciting anime episode with multiple parts",
            "duration": 3600,
            "pic": "https://i0.hdslb.com/bfs/archive/sample.jpg",
            "owner": {
                "name": "Bilibili Animation Studio"
            },
            "pages": [
                {
                    "page": 1,
                    "cid": 1001,
                    "part": "Episode 1: The Beginning",
                    "duration": 1800
                },
                {
                    "page": 2,
                    "cid": 1002,
                    "part": "Episode 2: The Journey",
                    "duration": 1800
                }
            ]
        });

        let info = parse_bilibili_data_json(&sample, 2).expect("Should parse successfully");
        assert_eq!(info.bvid, "BV1xx411c7mD");
        assert_eq!(info.aid, 170001);
        assert_eq!(info.title, "Sample Bilibili Anime Episode");
        assert_eq!(info.author, "Bilibili Animation Studio");
        assert_eq!(info.page_count, 2);
        assert_eq!(info.current_page, 2);
        assert_eq!(info.pages[0].part, "Episode 1: The Beginning");
        assert_eq!(info.pages[1].part, "Episode 2: The Journey");
    }
}
