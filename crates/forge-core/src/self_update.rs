//! Self-update functionality for FORGE binary.
//!
//! This module provides atomic self-update from GitHub releases:
//! 1. Checks GitHub releases API for latest version
//! 2. Compares with current version (semver)
//! 3. Downloads the appropriate binary for the platform
//! 4. Atomically swaps the running binary
//!
//! # Usage
//!
//! ```no_run
//! use forge_core::self_update::{check_for_update, perform_update, UpdateStatus};
//!
//! // Check for updates
//! let status = check_for_update("0.1.9").await?;
//! match status {
//!     UpdateStatus::UpToDate => println!("Already running latest version"),
//!     UpdateStatus::Available { current, latest, download_url, asset_size } => {
//!         println!("Update available: {} -> {}", current, latest);
//!         perform_update(&download_url, asset_size, |progress| {
//!             println!("Downloaded {}%", progress.percent);
//!         }).await?;
//!     }
//! }
//! ```

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Duration;

#[allow(unused_imports)] // Used implicitly by reqwest Response::chunk()
use futures_util::StreamExt;
use serde::Deserialize;
use tracing::{debug, error, info, warn};

use crate::ForgeError;

/// GitHub API URL for latest release
const GITHUB_RELEASES_API: &str = "https://api.github.com/repos/jedarden/forge/releases/latest";

/// User agent for GitHub API requests
const USER_AGENT: &str = concat!("forge/", env!("CARGO_PKG_VERSION"));

/// HTTP client timeout for API requests
const API_TIMEOUT_SECS: u64 = 10;

/// HTTP client timeout for downloads
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;

/// Status of an update check.
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    /// Already running the latest version
    UpToDate,
    /// Update available with download details
    Available {
        /// Current version
        current: String,
        /// Latest available version
        latest: String,
        /// URL to download the binary
        download_url: String,
        /// Size of the asset in bytes
        asset_size: u64,
    },
}

/// Progress information for download operations.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// Bytes downloaded so far
    pub downloaded: u64,
    /// Total bytes to download
    pub total: u64,
    /// Percentage complete (0-100)
    pub percent: u8,
}

/// Result of a self-update operation.
#[derive(Debug, Clone)]
pub enum UpdateResult {
    /// Update completed successfully
    Success {
        /// Previous version
        old_version: String,
        /// New version
        new_version: String,
    },
    /// Update failed with error message
    Failed(String),
    /// Already up to date
    AlreadyUpToDate,
}

/// GitHub release asset information.
#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    #[serde(rename = "browser_download_url")]
    download_url: String,
    size: u64,
    state: String,
}

/// GitHub release information from API.
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    #[serde(rename = "tag_name")]
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

/// Check for available updates from GitHub releases.
///
/// # Arguments
///
/// * `current_version` - Current version string (e.g., "0.1.9")
///
/// # Returns
///
/// - `UpdateStatus::UpToDate` if already on latest version
/// - `UpdateStatus::Available` with download details if update available
/// - `ForgeError` on network or parsing errors
pub async fn check_for_update(current_version: &str) -> crate::Result<UpdateStatus> {
    info!("Checking for updates (current: {})", current_version);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(API_TIMEOUT_SECS))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| ForgeError::UpdateCheck {
            message: format!("Failed to create HTTP client: {}", e),
        })?;

    let response = client
        .get(GITHUB_RELEASES_API)
        .send()
        .await
        .map_err(|e| ForgeError::UpdateCheck {
            message: format!("Failed to fetch releases: {}", e),
        })?;

    if !response.status().is_success() {
        return Err(ForgeError::UpdateCheck {
            message: format!("GitHub API returned status {}", response.status()),
        });
    }

    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|e| ForgeError::UpdateCheck {
            message: format!("Failed to parse release response: {}", e),
        })?;

    // Parse version from tag (e.g., "v0.1.9" -> "0.1.9")
    let latest_version = release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name);

    info!("Latest version: {}", latest_version);

    // Compare versions using semver
    if !is_newer_version(current_version, latest_version)? {
        info!("Already running latest version");
        return Ok(UpdateStatus::UpToDate);
    }

    // Find the appropriate asset for this platform
    let asset_name = get_asset_name_for_platform();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name && a.state == "uploaded")
        .ok_or_else(|| ForgeError::UpdateAssetNotFound {
            platform: asset_name.to_string(),
        })?;

    info!(
        "Update available: {} -> {} ({} bytes)",
        current_version, latest_version, asset.size
    );

    Ok(UpdateStatus::Available {
        current: current_version.to_string(),
        latest: latest_version.to_string(),
        download_url: asset.download_url.clone(),
        asset_size: asset.size,
    })
}

/// Perform the self-update: download and atomic swap.
///
/// # Arguments
///
/// * `download_url` - URL to download the binary from
/// * `expected_size` - Expected size of the download in bytes
/// * `progress_tx` - Optional channel to send progress updates
///
/// # Returns
///
/// - `UpdateResult::Success` on successful update
/// - `UpdateResult::Failed` with error message on failure
pub async fn perform_update(
    download_url: &str,
    expected_size: u64,
    progress_tx: Option<Sender<DownloadProgress>>,
) -> crate::Result<UpdateResult> {
    info!("Starting update download from {}", download_url);

    // Get current executable path
    let current_exe = env::current_exe().map_err(|e| ForgeError::UpdateInstall {
        message: format!("Failed to get current executable path: {}", e),
    })?;

    // Download to a temporary file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("forge-update-{}", std::process::id()));

    info!("Downloading to temporary file: {:?}", temp_file);

    // Download the new binary
    download_file(download_url, &temp_file, expected_size, progress_tx).await?;

    // Verify the downloaded file is a valid executable
    verify_binary(&temp_file)?;

    // Perform atomic swap
    let backup_path = current_exe.with_extension("old");
    atomic_swap(&current_exe, &temp_file, &backup_path)?;

    info!("Update complete! Old binary backed up to {:?}", backup_path);

    // Cleanup temp file (it should have been renamed, but just in case)
    let _ = fs::remove_file(&temp_file);

    Ok(UpdateResult::Success {
        old_version: env!("CARGO_PKG_VERSION").to_string(),
        new_version: "latest".to_string(), // We don't have this info here anymore
    })
}

/// Download a file with progress reporting.
async fn download_file(
    url: &str,
    dest: &Path,
    expected_size: u64,
    progress_tx: Option<Sender<DownloadProgress>>,
) -> crate::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| ForgeError::UpdateDownload {
            message: format!("Failed to create HTTP client: {}", e),
        })?;

    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| ForgeError::UpdateDownload {
            message: format!("Failed to start download: {}", e),
        })?;

    if !response.status().is_success() {
        return Err(ForgeError::UpdateDownload {
            message: format!("Download failed with status {}", response.status()),
        });
    }

    // Create the destination file
    let mut file = fs::File::create(dest).map_err(|e| ForgeError::UpdateDownload {
        message: format!("Failed to create temp file: {}", e),
    })?;

    // Download in chunks with progress reporting
    let mut downloaded: u64 = 0;

    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|e| ForgeError::UpdateDownload {
                message: format!("Download interrupted: {}", e),
            })?;

        match chunk {
            Some(chunk) => {
                file.write_all(&chunk).map_err(|e| ForgeError::UpdateDownload {
                    message: format!("Failed to write to temp file: {}", e),
                })?;

                downloaded += chunk.len() as u64;

                // Report progress
                if let Some(ref tx) = progress_tx {
                    let percent = if expected_size > 0 {
                        ((downloaded as f64 / expected_size as f64) * 100.0) as u8
                    } else {
                        0
                    };

                    let progress = DownloadProgress {
                        downloaded,
                        total: expected_size,
                        percent: percent.min(100),
                    };

                    // Send progress (ignore errors if receiver is gone)
                    let _ = tx.send(progress);
                }
            }
            None => break, // End of stream
        }
    }

    // Verify download size
    if expected_size > 0 && downloaded != expected_size {
        return Err(ForgeError::UpdateDownload {
            message: format!(
                "Download size mismatch: expected {}, got {}",
                expected_size, downloaded
            ),
        });
    }

    info!("Download complete: {} bytes", downloaded);
    Ok(())
}

/// Verify the downloaded binary is a valid executable.
fn verify_binary(path: &Path) -> crate::Result<()> {
    // Check file exists and has content
    let metadata = fs::metadata(path).map_err(|e| ForgeError::UpdateVerification {
        message: format!("Failed to read downloaded file: {}", e),
    })?;

    if metadata.len() == 0 {
        return Err(ForgeError::UpdateVerification {
            message: "Downloaded file is empty".to_string(),
        });
    }

    // Check ELF magic bytes (Linux executable)
    let mut file = fs::File::open(path).map_err(|e| ForgeError::UpdateVerification {
        message: format!("Failed to open downloaded file: {}", e),
    })?;

    let mut magic = [0u8; 4];
    Read::read_exact(&mut file, &mut magic).map_err(|e| ForgeError::UpdateVerification {
        message: format!("Failed to read file header: {}", e),
    })?;

    // ELF magic: 0x7f 'E' 'L' 'F'
    if magic != [0x7f, 0x45, 0x4c, 0x46] {
        return Err(ForgeError::UpdateVerification {
            message: "Downloaded file is not a valid ELF binary".to_string(),
        });
    }

    info!("Binary verification passed (valid ELF)");
    Ok(())
}

/// Perform atomic swap of the binary.
///
/// Strategy:
/// 1. Rename current exe to .old (backup)
/// 2. Rename new binary to current exe path
/// 3. Set executable permissions
/// 4. On failure, attempt to restore from backup
fn atomic_swap(current_exe: &Path, new_binary: &Path, backup_path: &Path) -> crate::Result<()> {
    info!(
        "Performing atomic swap: {:?} -> {:?} (backup: {:?})",
        new_binary, current_exe, backup_path
    );

    // Step 1: Rename current executable to backup
    // This may fail if the backup already exists from a previous update
    if backup_path.exists() {
        debug!("Removing old backup file");
        fs::remove_file(backup_path).map_err(|e| ForgeError::UpdateInstall {
            message: format!("Failed to remove old backup: {}", e),
        })?;
    }

    // Try rename first (atomic), fall back to copy+delete
    let rename_result = fs::rename(current_exe, backup_path);
    if let Err(e) = rename_result {
        // If we can't rename the current exe, we might not have permissions
        // or the file is busy. Try a copy instead.
        warn!("Rename failed, trying copy: {}", e);
        fs::copy(current_exe, backup_path).map_err(|copy_err| ForgeError::UpdateInstall {
            message: format!(
                "Failed to backup current binary (rename: {}, copy: {})",
                e, copy_err
            ),
        })?;
        fs::remove_file(current_exe).map_err(|e2| ForgeError::UpdateInstall {
            message: format!("Failed to remove old binary after copy: {}", e2),
        })?;
    }

    // Step 2: Move new binary to current exe path
    // Try rename first, fall back to copy
    if let Err(e) = fs::rename(new_binary, current_exe) {
        warn!("Rename of new binary failed, trying copy: {}", e);
        fs::copy(new_binary, current_exe).map_err(|e2| {
            // Attempt rollback
            error!("Failed to install new binary: {}", e2);
            let _ = fs::rename(backup_path, current_exe);
            ForgeError::UpdateInstall {
                message: format!("Failed to install new binary: {}", e2),
            }
        })?;
        fs::remove_file(new_binary).ok();
    }

    // Step 3: Set executable permissions
    fs::set_permissions(current_exe, fs::Permissions::from_mode(0o755)).map_err(|e| {
        error!("Failed to set permissions: {}", e);
        // Attempt rollback
        let _ = fs::remove_file(current_exe);
        let _ = fs::rename(backup_path, current_exe);
        ForgeError::UpdateInstall {
            message: format!("Failed to set executable permissions: {}", e),
        }
    })?;

    // Step 4: Clean up backup (optional, keep it for safety)
    // We keep the backup for potential rollback

    info!("Atomic swap completed successfully");
    Ok(())
}

/// Get the asset name for the current platform.
fn get_asset_name_for_platform() -> &'static str {
    // Currently only supporting Linux
    // Future: detect macOS, Windows, ARM architectures
    #[cfg(target_arch = "x86_64")]
    {
        "forge"
    }
    #[cfg(target_arch = "aarch64")]
    {
        "forge-linux-aarch64"
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        "forge"
    }
}

/// Compare two version strings to determine if the second is newer.
///
/// Uses semantic versioning comparison (major.minor.patch).
fn is_newer_version(current: &str, latest: &str) -> crate::Result<bool> {
    let current_parts: Vec<u32> = current
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    let latest_parts: Vec<u32> = latest
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    if current_parts.is_empty() || latest_parts.is_empty() {
        return Err(ForgeError::UpdateCheck {
            message: format!("Invalid version format: {} or {}", current, latest),
        });
    }

    // Pad to 3 components (major.minor.patch)
    let current_parts = pad_version(&current_parts);
    let latest_parts = pad_version(&latest_parts);

    // Compare major.minor.patch
    Ok(latest_parts > current_parts)
}

/// Pad version components to 3 elements.
fn pad_version(parts: &[u32]) -> [u32; 3] {
    let mut result = [0u32; 3];
    for (i, &part) in parts.iter().enumerate().take(3) {
        result[i] = part;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        // Basic comparisons
        assert!(is_newer_version("0.1.0", "0.1.1").unwrap());
        assert!(is_newer_version("0.1.9", "0.2.0").unwrap());
        assert!(is_newer_version("0.9.9", "1.0.0").unwrap());

        // Equal versions
        assert!(!is_newer_version("0.1.9", "0.1.9").unwrap());

        // Older versions
        assert!(!is_newer_version("0.2.0", "0.1.9").unwrap());
        assert!(!is_newer_version("1.0.0", "0.9.9").unwrap());

        // Incomplete versions (padded with zeros)
        assert!(is_newer_version("0.1", "0.1.1").unwrap());
        assert!(!is_newer_version("0.1.0", "0.1").unwrap()); // 0.1 == 0.1.0
    }

    #[test]
    fn test_pad_version() {
        assert_eq!(pad_version(&[1, 2, 3]), [1, 2, 3]);
        assert_eq!(pad_version(&[1, 2]), [1, 2, 0]);
        assert_eq!(pad_version(&[1]), [1, 0, 0]);
        assert_eq!(pad_version(&[]), [0, 0, 0]);
    }

    #[test]
    fn test_asset_name() {
        // Should return "forge" for x86_64
        let name = get_asset_name_for_platform();
        assert!(!name.is_empty());
    }
}
