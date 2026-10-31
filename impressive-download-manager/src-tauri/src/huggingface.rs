use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HuggingFaceModelInfo {
    pub owner: String,
    pub repo: String,
    pub is_dataset: bool,
    pub revision: String,
    pub filepath: String,
    pub filename: String,
    pub download_url: String,
    pub is_model_weights: bool,
}

/// Detects whether a URL is a Hugging Face resource
pub fn is_huggingface_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.contains("huggingface.co/") || lower.contains("hf.co/")
}

/// Checks whether an AI model file is high-capacity weights (GGUF, Safetensors, PyTorch, ONNX, Bin)
pub fn is_weights_file(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    lower.ends_with(".gguf")
        || lower.ends_with(".safetensors")
        || lower.ends_with(".bin")
        || lower.ends_with(".onnx")
        || lower.ends_with(".pt")
        || lower.ends_with(".pth")
        || lower.ends_with(".h5")
        || lower.ends_with(".tflite")
        || lower.ends_with(".model")
        || lower.ends_with(".parquet")
        || lower.ends_with(".arrow")
}

/// Normalizes Hugging Face web URLs (e.g. /blob/) into direct binary stream URLs (/resolve/?download=true)
pub fn normalize_huggingface_url(url: &str) -> Option<String> {
    let clean = url.trim();
    if !is_huggingface_url(clean) {
        return None;
    }

    let parsed = reqwest::Url::parse(clean).ok()?;
    let mut path = parsed.path().to_string();

    // Replace '/blob/' preview with direct stream '/resolve/'
    if path.contains("/blob/") {
        path = path.replace("/blob/", "/resolve/");
    }

    // Ensure it targets /resolve/
    if !path.contains("/resolve/") && !path.contains("/raw/") {
        return None;
    }

    let mut new_url = format!("https://huggingface.co{}", path);
    if !clean.contains("download=true") {
        let sep = if new_url.contains('?') { "&" } else { "?" };
        new_url.push_str(&format!("{}download=true", sep));
    }

    Some(new_url)
}

/// Parses detailed metadata from Hugging Face model or dataset URLs
pub fn parse_huggingface_url(url: &str) -> Option<HuggingFaceModelInfo> {
    let clean = url.trim();
    if !is_huggingface_url(clean) {
        return None;
    }

    let parsed = reqwest::Url::parse(clean).ok()?;
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();

    let mut is_dataset = false;
    let mut seg_offset = 0;

    if segments.first() == Some(&"datasets") {
        is_dataset = true;
        seg_offset = 1;
    }

    if segments.len() < seg_offset + 4 {
        return None;
    }

    let owner = segments[seg_offset].to_string();
    let repo = segments[seg_offset + 1].to_string();
    // segments[seg_offset + 2] is "resolve" or "blob" or "raw"
    let revision = segments[seg_offset + 3].to_string();
    let filepath = segments[seg_offset + 4..].join("/");
    let raw_filename = segments.last().unwrap_or(&"model.bin");
    let filename = urlencoding::decode(raw_filename)
        .unwrap_or(std::borrow::Cow::Borrowed(raw_filename))
        .to_string();

    let download_url = normalize_huggingface_url(clean).unwrap_or_else(|| clean.to_string());
    let is_model_weights = is_weights_file(&filename);

    Some(HuggingFaceModelInfo {
        owner,
        repo,
        is_dataset,
        revision,
        filepath,
        filename,
        download_url,
        is_model_weights,
    })
}

/// Injects Hugging Face User Access Token (HF_TOKEN) for private or gated models (Llama 3, Mistral, Gemma)
pub fn apply_huggingface_auth(
    builder: reqwest::RequestBuilder,
    custom_token: Option<&str>,
) -> reqwest::RequestBuilder {
    // 1. Check custom token first
    if let Some(tok) = custom_token {
        let clean = tok.trim();
        if !clean.is_empty() {
            return builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {}", clean));
        }
    }

    // 2. Check environment variable HF_TOKEN or HUGGING_FACE_HUB_TOKEN
    if let Ok(env_tok) = std::env::var("HF_TOKEN").or_else(|_| std::env::var("HUGGING_FACE_HUB_TOKEN")) {
        let clean = env_tok.trim();
        if !clean.is_empty() {
            return builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {}", clean));
        }
    }

    builder
}

/// Extracts expected SHA-256 checksum from Hugging Face Git LFS HTTP response headers
pub fn extract_huggingface_lfs_sha256(headers: &reqwest::header::HeaderMap) -> Option<String> {
    // 1. Check X-Linked-ETag
    if let Some(val) = headers.get("x-linked-etag").and_then(|v| v.to_str().ok()) {
        let clean = val.trim_matches('"').trim();
        if clean.len() == 64 {
            return Some(clean.to_lowercase());
        }
    }

    // 2. Check standard ETag (often contains the 64-char hex LFS sha256)
    if let Some(val) = headers.get(reqwest::header::ETAG).and_then(|v| v.to_str().ok()) {
        let clean = val.trim_matches('"').trim();
        if clean.len() == 64 {
            return Some(clean.to_lowercase());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_huggingface_blob_to_resolve() {
        let web_url = "https://huggingface.co/TheBloke/Llama-2-7B-Chat-GGUF/blob/main/llama-2-7b-chat.Q4_K_M.gguf";
        let resolved = normalize_huggingface_url(web_url).expect("Should normalize HF url");

        assert!(resolved.contains("/resolve/main/llama-2-7b-chat.Q4_K_M.gguf"));
        assert!(resolved.contains("download=true"));
        assert!(!resolved.contains("/blob/"));
    }

    #[test]
    fn test_parse_huggingface_model_metadata() {
        let url = "https://huggingface.co/meta-llama/Meta-Llama-3-8B-Instruct/resolve/main/model-00001-of-00004.safetensors";
        let info = parse_huggingface_url(url).expect("Should parse HF model URL");

        assert_eq!(info.owner, "meta-llama");
        assert_eq!(info.repo, "Meta-Llama-3-8B-Instruct");
        assert_eq!(info.revision, "main");
        assert_eq!(info.filename, "model-00001-of-00004.safetensors");
        assert!(!info.is_dataset);
        assert!(info.is_model_weights);
    }

    #[test]
    fn test_parse_huggingface_dataset_metadata() {
        let url = "https://huggingface.co/datasets/glue/resolve/main/sst2/train-00000-of-00001.parquet";
        let info = parse_huggingface_url(url).expect("Should parse HF dataset URL");

        assert_eq!(info.owner, "glue");
        assert_eq!(info.filename, "train-00000-of-00001.parquet");
        assert!(info.is_dataset);
        assert!(info.is_model_weights);
    }

    #[test]
    fn test_extract_huggingface_lfs_sha256() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-linked-etag",
            reqwest::header::HeaderValue::from_static("\"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\""),
        );

        let hash = extract_huggingface_lfs_sha256(&headers);
        assert_eq!(hash, Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string()));
    }
}
