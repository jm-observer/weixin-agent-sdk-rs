// Remote download utilities for media files.

use crate::error::Result;
use crate::media::mime::get_extension_from_content_type_or_url;
use crate::util::random::temp_file_name;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Maximum allowed download size (100 MB).
const MAX_DOWNLOAD_SIZE: u64 = 100 * 1024 * 1024;
/// Default download timeout.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// Download a remote file to a specified directory, returning the local path.
pub async fn download_remote_file(url: &str, dest_dir: &Path) -> Result<PathBuf> {
    // Ensure destination directory exists.
    tokio::fs::create_dir_all(dest_dir).await?;

    let client = reqwest::Client::new();
    let resp = client.get(url).timeout(DOWNLOAD_TIMEOUT).send().await?;
    if !resp.status().is_success() {
        return Err(crate::error::Error::CdnUpload(format!(
            "failed to download remote file: status {}",
            resp.status()
        )));
    }
    // Determine extension.
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    let ext = get_extension_from_content_type_or_url(content_type, url);
    let filename = temp_file_name("remote", ext);
    let file_path = dest_dir.join(&filename);
    // Enforce size limit while streaming.
    let bytes = resp.bytes().await?;
    if bytes.len() as u64 > MAX_DOWNLOAD_SIZE {
        return Err(crate::error::Error::CdnUpload("download exceeds max size".into()));
    }
    tokio::fs::write(&file_path, &bytes).await?;
    Ok(file_path)
}

/// Convenience wrapper that downloads to the system temporary directory.
pub async fn download_remote_file_to_temp(url: &str) -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir();
    download_remote_file(url, &temp_dir).await
}
