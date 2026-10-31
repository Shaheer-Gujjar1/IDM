use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SiteCredential {
    pub id: String,
    pub site_pattern: String, // e.g. "vault.corp.net", "*.private-host.org", "http://192.168.1.50"
    pub username: String,
    pub password: String,
    pub created_at: u64,
}

static GLOBAL_SITE_LOGINS: OnceLock<Arc<SiteLoginsManager>> = OnceLock::new();

pub fn get_global_site_logins() -> Arc<SiteLoginsManager> {
    GLOBAL_SITE_LOGINS.get_or_init(|| {
        let mgr = Arc::new(SiteLoginsManager::new());
        if let Some(cfg_dir) = dirs::config_dir() {
            let p = cfg_dir.join("impressive-download-manager").join("site_logins.json");
            mgr.set_storage_path(p);
        }
        mgr
    }).clone()
}

pub struct SiteLoginsManager {
    pub credentials: RwLock<HashMap<String, SiteCredential>>,
    pub storage_path: RwLock<Option<PathBuf>>,
}

impl SiteLoginsManager {
    pub fn new() -> Self {
        Self {
            credentials: RwLock::new(HashMap::new()),
            storage_path: RwLock::new(None),
        }
    }

    pub fn set_storage_path(&self, path: PathBuf) {
        *self.storage_path.write().unwrap() = Some(path);
        let _ = self.load_from_disk();
    }

    pub fn get_all(&self) -> Vec<SiteCredential> {
        let creds = self.credentials.read().unwrap();
        let mut list: Vec<SiteCredential> = creds.values().cloned().collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    pub fn add_or_update(&self, mut cred: SiteCredential) -> Result<SiteCredential, String> {
        cred.site_pattern = cred.site_pattern.trim().to_lowercase();
        cred.username = cred.username.trim().to_string();

        if cred.site_pattern.is_empty() {
            return Err("Site pattern/domain cannot be empty".to_string());
        }

        if cred.id.is_empty() {
            cred.id = uuid::Uuid::new_v4().to_string();
        }
        if cred.created_at == 0 {
            cred.created_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }

        {
            let mut map = self.credentials.write().unwrap();
            map.insert(cred.id.clone(), cred.clone());
        }

        let _ = self.save_to_disk();
        Ok(cred)
    }

    pub fn delete(&self, id: &str) -> bool {
        let removed = {
            let mut map = self.credentials.write().unwrap();
            map.remove(id).is_some()
        };
        if removed {
            let _ = self.save_to_disk();
        }
        removed
    }

    pub fn find_credentials_for_url(&self, url_str: &str) -> Option<SiteCredential> {
        let parsed = reqwest::Url::parse(url_str).ok()?;
        let host = parsed.host_str()?.to_lowercase();
        let port_opt = parsed.port();
        let scheme = parsed.scheme().to_lowercase();

        let creds = self.credentials.read().unwrap();

        for cred in creds.values() {
            let pat = cred.site_pattern.to_lowercase();

            // 1. Exact host match
            if pat == host {
                return Some(cred.clone());
            }

            // 2. Host with port match (e.g. "files.corp:8080")
            if let Some(port) = port_opt {
                if pat == format!("{}:{}", host, port) {
                    return Some(cred.clone());
                }
            }

            // 3. Full scheme and host (e.g. "https://vault.corp")
            if pat == format!("{}://{}", scheme, host) {
                return Some(cred.clone());
            }

            // 4. Wildcard subdomain match (e.g. "*.corp.net" or ".corp.net")
            if let Some(suffix) = pat.strip_prefix("*.") {
                if host == suffix || host.ends_with(&format!(".{}", suffix)) {
                    return Some(cred.clone());
                }
            } else if let Some(suffix) = pat.strip_prefix('.') {
                if host == suffix || host.ends_with(&format!(".{}", suffix)) {
                    return Some(cred.clone());
                }
            }

            // 5. Bare domain matching subdomains (e.g. pattern "corp.net" matches "dl.corp.net")
            if !pat.contains('/') && !pat.contains(':') && host.ends_with(&format!(".{}", pat)) {
                return Some(cred.clone());
            }

            // 6. URL prefix match (e.g. "https://vault.corp.net/private/")
            if pat.starts_with("http://") || pat.starts_with("https://") || pat.starts_with("ftp://") {
                if url_str.to_lowercase().starts_with(&pat) {
                    return Some(cred.clone());
                }
            }
        }

        None
    }

    pub fn save_to_disk(&self) -> Result<(), String> {
        let path_guard = self.storage_path.read().unwrap();
        if let Some(ref path) = *path_guard {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let list = self.get_all();
            let json = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
            std::fs::write(path, json).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn load_from_disk(&self) -> Result<(), String> {
        let path_guard = self.storage_path.read().unwrap();
        if let Some(ref path) = *path_guard {
            if path.exists() {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                if let Ok(list) = serde_json::from_slice::<Vec<SiteCredential>>(&bytes) {
                    let mut map = self.credentials.write().unwrap();
                    map.clear();
                    for c in list {
                        map.insert(c.id.clone(), c);
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn generate_basic_auth_header(cred: &SiteCredential) -> String {
    let raw = format!("{}:{}", cred.username, cred.password);
    let encoded = base64_encode(raw.as_bytes());
    format!("Basic {}", encoded)
}

/// Pure-Rust RFC 4648 Base64 Encoder
pub fn base64_encode(input: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        out.push(CHARSET[(b0 >> 2) as usize] as char);
        out.push(CHARSET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARSET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARSET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_standard_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"Aladdin:open sesame"), "QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
    }

    #[test]
    fn test_site_logins_crud_and_lookup() {
        let manager = SiteLoginsManager::new();

        let cred1 = SiteCredential {
            id: "".to_string(),
            site_pattern: "vault.corp.net".to_string(),
            username: "alice".to_string(),
            password: "supersecretpassword".to_string(),
            created_at: 0,
        };

        let saved = manager.add_or_update(cred1).expect("Should save credential");
        assert!(!saved.id.is_empty());
        assert_eq!(saved.site_pattern, "vault.corp.net");

        // Exact match lookup
        let found = manager.find_credentials_for_url("https://vault.corp.net/files/doc.pdf");
        assert!(found.is_some());
        let val = found.unwrap();
        assert_eq!(val.username, "alice");

        // Basic auth header check
        let auth_hdr = generate_basic_auth_header(&val);
        assert!(auth_hdr.starts_with("Basic "));

        // Non-matching domain
        assert!(manager.find_credentials_for_url("https://other-site.com/file.zip").is_none());

        // Delete test
        assert!(manager.delete(&saved.id));
        assert!(manager.find_credentials_for_url("https://vault.corp.net/files/doc.pdf").is_none());
    }

    #[test]
    fn test_site_logins_wildcard_and_subdomain_matching() {
        let manager = SiteLoginsManager::new();

        let cred = SiteCredential {
            id: "wildcard_1".to_string(),
            site_pattern: "*.debrid-vault.com".to_string(),
            username: "bob_pro".to_string(),
            password: "p@ssword123".to_string(),
            created_at: 100,
        };
        manager.add_or_update(cred).unwrap();

        // Subdomain 1
        let match1 = manager.find_credentials_for_url("https://stream.debrid-vault.com/video.mp4");
        assert!(match1.is_some());
        assert_eq!(match1.unwrap().username, "bob_pro");

        // Subdomain 2 with port
        let match2 = manager.find_credentials_for_url("https://cdn2.debrid-vault.com:443/dl.iso");
        assert!(match2.is_some());

        // Bare domain
        let match3 = manager.find_credentials_for_url("https://debrid-vault.com/login");
        assert!(match3.is_some());
    }
}
