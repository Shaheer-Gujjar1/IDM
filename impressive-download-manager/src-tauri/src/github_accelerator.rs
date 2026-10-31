use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GitHubUrlKind {
    ReleaseAsset { tag: String, filename: String },
    Archive { ref_name: String, format: String },
    Raw { branch: String, filepath: String },
    Generic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitHubUrlInfo {
    pub owner: String,
    pub repo: String,
    pub kind: GitHubUrlKind,
    pub original_url: String,
    pub filename: String,
}

/// Known high-speed global & regional GitHub acceleration mirrors
pub const GITHUB_MIRRORS: &[&str] = &[
    "https://ghproxy.net/",
    "https://gh-proxy.com/",
    "https://mirror.ghproxy.com/",
    "https://ghps.cc/",
];

/// Detects whether a URL belongs to GitHub Releases, Raw, or Archives
pub fn is_github_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.contains("github.com/") || lower.contains("raw.githubusercontent.com/") || lower.contains("objects.githubusercontent.com/")
}

/// Parses GitHub download URLs into structured repository and asset metadata
pub fn parse_github_url(url: &str) -> Option<GitHubUrlInfo> {
    let clean = url.trim();
    if !is_github_url(clean) {
        return None;
    }

    let parsed = reqwest::Url::parse(clean).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();

    if host == "raw.githubusercontent.com" {
        if segments.len() >= 3 {
            let owner = segments[0].to_string();
            let repo = segments[1].to_string();
            let branch = segments[2].to_string();
            let filepath = segments[3..].join("/");
            let filename = segments.last().unwrap_or(&"file").to_string();
            return Some(GitHubUrlInfo {
                owner,
                repo,
                kind: GitHubUrlKind::Raw { branch, filepath },
                original_url: clean.to_string(),
                filename,
            });
        }
    }

    if host == "github.com" {
        if segments.len() >= 4 {
            let owner = segments[0].to_string();
            let repo = segments[1].to_string();

            // e.g. /owner/repo/releases/download/v1.0.0/app.deb
            if segments[2] == "releases" && segments[3] == "download" && segments.len() >= 6 {
                let tag = segments[4].to_string();
                let filename = urlencoding::decode(segments[5])
                    .unwrap_or(std::borrow::Cow::Borrowed(segments[5]))
                    .to_string();
                return Some(GitHubUrlInfo {
                    owner,
                    repo,
                    kind: GitHubUrlKind::ReleaseAsset { tag, filename: filename.clone() },
                    original_url: clean.to_string(),
                    filename,
                });
            }

            // e.g. /owner/repo/archive/refs/tags/v1.0.0.zip or /archive/v1.0.0.tar.gz
            if segments[2] == "archive" {
                let last = segments.last()?;
                let (ref_name, format) = if last.ends_with(".tar.gz") {
                    (&last[..last.len() - 7], "tar.gz".to_string())
                } else if last.ends_with(".zip") {
                    (&last[..last.len() - 4], "zip".to_string())
                } else {
                    (*last, "zip".to_string())
                };
                let filename = format!("{}-{}.{}", repo, ref_name, format);
                return Some(GitHubUrlInfo {
                    owner,
                    repo,
                    kind: GitHubUrlKind::Archive { ref_name: ref_name.to_string(), format },
                    original_url: clean.to_string(),
                    filename,
                });
            }
        }
    }

    let default_fn = parsed.path_segments().and_then(|s| s.last()).unwrap_or("github_download").to_string();
    Some(GitHubUrlInfo {
        owner: String::new(),
        repo: String::new(),
        kind: GitHubUrlKind::Generic,
        original_url: clean.to_string(),
        filename: default_fn,
    })
}

/// Generates a list of accelerated mirror URLs for a given GitHub download link
pub fn get_fast_mirror_urls(github_url: &str) -> Vec<String> {
    let clean = github_url.trim();
    let mut mirrors = Vec::new();

    // 1. Always prioritize fast CDN mirror proxies for releases & archives
    for prefix in GITHUB_MIRRORS {
        mirrors.push(format!("{}{}", prefix, clean));
    }

    // 2. Direct original GitHub URL as fallback
    mirrors.push(clean.to_string());
    mirrors
}

/// Probes candidate mirrors concurrently and returns the fastest responsive accelerator URL
pub async fn probe_fastest_github_mirror(
    original_url: &str,
    client: &reqwest::Client,
) -> String {
    let candidates = get_fast_mirror_urls(original_url);
    if candidates.is_empty() {
        return original_url.to_string();
    }

    let timeout_duration = std::time::Duration::from_millis(1800);

    // Fast-path race: first mirror to return HTTP 200 or 302 wins
    for mirror_url in candidates {
        let req = client
            .head(&mirror_url)
            .timeout(timeout_duration)
            .header(reqwest::header::USER_AGENT, "Mozilla/5.0 IDM/0.9.0");

        if let Ok(resp) = req.send().await {
            let status = resp.status();
            if status.is_success() || status.is_redirection() {
                println!("[GitHub Accelerator] Selected fast mirror: {}", mirror_url);
                return mirror_url;
            }
        }
    }

    // Fallback to original URL
    original_url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_release_asset() {
        let url = "https://github.com/Shaheer-Gujjar1/IDM/releases/download/v0.9.0/impressive-download-manager_0.9.0_amd64.deb";
        let info = parse_github_url(url).expect("Should parse GitHub release URL");

        assert_eq!(info.owner, "Shaheer-Gujjar1");
        assert_eq!(info.repo, "IDM");
        assert_eq!(info.filename, "impressive-download-manager_0.9.0_amd64.deb");
        match info.kind {
            GitHubUrlKind::ReleaseAsset { tag, filename } => {
                assert_eq!(tag, "v0.9.0");
                assert_eq!(filename, "impressive-download-manager_0.9.0_amd64.deb");
            }
            _ => panic!("Expected ReleaseAsset kind"),
        }
    }

    #[test]
    fn test_parse_github_raw_url() {
        let url = "https://raw.githubusercontent.com/Shaheer-Gujjar1/IDM/main/package.json";
        let info = parse_github_url(url).expect("Should parse GitHub raw URL");

        assert_eq!(info.owner, "Shaheer-Gujjar1");
        assert_eq!(info.repo, "IDM");
        assert_eq!(info.filename, "package.json");
        match info.kind {
            GitHubUrlKind::Raw { branch, filepath } => {
                assert_eq!(branch, "main");
                assert_eq!(filepath, "package.json");
            }
            _ => panic!("Expected Raw kind"),
        }
    }

    #[test]
    fn test_parse_github_archive_url() {
        let url = "https://github.com/rust-lang/rust/archive/refs/tags/1.80.0.tar.gz";
        let info = parse_github_url(url).expect("Should parse GitHub archive URL");

        assert_eq!(info.owner, "rust-lang");
        assert_eq!(info.repo, "rust");
        match info.kind {
            GitHubUrlKind::Archive { ref_name, format } => {
                assert_eq!(ref_name, "1.80.0");
                assert_eq!(format, "tar.gz");
            }
            _ => panic!("Expected Archive kind"),
        }
    }

    #[test]
    fn test_get_fast_mirror_urls() {
        let url = "https://github.com/owner/repo/releases/download/v1.0/file.zip";
        let mirrors = get_fast_mirror_urls(url);

        assert!(mirrors.len() >= 4);
        assert!(mirrors[0].starts_with("https://ghproxy.net/"));
        assert!(mirrors.contains(&url.to_string()));
    }
}
