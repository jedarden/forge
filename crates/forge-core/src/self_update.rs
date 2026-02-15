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
use std::path::{Path, PathBuf};
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

    // Download to a staging file
    let staging_file = get_staging_path(&current_exe)?;

    info!("Downloading to staging file: {:?}", staging_file);

    // Download the new binary
    download_file(download_url, &staging_file, expected_size, progress_tx).await?;

    // Verify the downloaded file is a valid executable
    verify_binary(&staging_file)?;

    info!("Update download complete! Binary staged at {:?}", staging_file);

    Ok(UpdateResult::Success {
        old_version: env!("CARGO_PKG_VERSION").to_string(),
        new_version: "latest".to_string(), // We don't have this info here anymore
    })
}

/// Restart the process using the newly downloaded binary.
///
/// This function performs the following steps:
/// 1. Gracefully shutdown current process (caller's responsibility)
/// 2. Exec the new binary from staging location
/// 3. New process will rename itself to the final location
///
/// This function DOES NOT RETURN on success - it replaces the current process.
/// On error, it returns a ForgeError.
pub fn restart_with_new_binary() -> crate::Result<()> {
    info!("Initiating restart with new binary");

    // Get current executable path
    let current_exe = env::current_exe().map_err(|e| ForgeError::UpdateInstall {
        message: format!("Failed to get current executable path: {}", e),
    })?;

    // Get staging path
    let staging_file = get_staging_path(&current_exe)?;

    // Verify staging file exists
    if !staging_file.exists() {
        return Err(ForgeError::UpdateInstall {
            message: format!("Staging file not found at {:?}", staging_file),
        });
    }

    info!("Staging file found at {:?}", staging_file);

    // Verify the staging file is executable
    verify_binary(&staging_file)?;

    // Get the final install path (e.g., ~/.cargo/bin/forge)
    let install_path = get_install_path(&current_exe)?;

    info!(
        "Will exec staging binary and install to {:?}",
        install_path
    );

    // Prepare arguments for exec
    // Pass special environment variable to signal the new process to rename itself
    let mut cmd = std::process::Command::new(&staging_file);
    cmd.env("FORGE_INSTALL_PATH", &install_path);
    cmd.env("FORGE_STAGING_PATH", &staging_file);
    cmd.env("FORGE_AUTO_RESTART", "1");

    // Preserve current args
    let args: Vec<String> = env::args().skip(1).collect();
    cmd.args(&args);

    info!(
        "Execing new binary: {:?} with args: {:?}",
        staging_file, args
    );

    // Replace current process with new binary using exec
    // On Unix, use exec syscall to replace the process
    exec_binary(&staging_file, &args)?;

    // This line should never be reached if exec succeeds
    unreachable!("exec() returned, which should never happen");
}

/// Check if this is a freshly exec'd process that needs to install itself.
///
/// Returns the install path if this process should rename itself, None otherwise.
pub fn check_and_perform_self_install() -> crate::Result<Option<PathBuf>> {
    // Check if we were exec'd with install instructions
    if env::var("FORGE_AUTO_RESTART").is_ok() {
        info!("Detected auto-restart environment - performing self-install");

        let install_path_str = env::var("FORGE_INSTALL_PATH").map_err(|_| {
            ForgeError::UpdateInstall {
                message: "FORGE_INSTALL_PATH not set".to_string(),
            }
        })?;
        let install_path = PathBuf::from(install_path_str);

        let staging_path_str = env::var("FORGE_STAGING_PATH").map_err(|_| {
            ForgeError::UpdateInstall {
                message: "FORGE_STAGING_PATH not set".to_string(),
            }
        })?;
        let staging_path = PathBuf::from(staging_path_str);

        info!(
            "Installing from {:?} to {:?}",
            staging_path, install_path
        );

        // Create backup of old binary
        let backup_path = install_path.with_extension("old");
        if install_path.exists() {
            info!("Backing up old binary to {:?}", backup_path);
            if backup_path.exists() {
                fs::remove_file(&backup_path).map_err(|e| ForgeError::UpdateInstall {
                    message: format!("Failed to remove old backup: {}", e),
                })?;
            }
            fs::rename(&install_path, &backup_path).map_err(|e| ForgeError::UpdateInstall {
                message: format!("Failed to backup old binary: {}", e),
            })?;
        }

        // Move staging binary to install location
        fs::rename(&staging_path, &install_path).map_err(|e| {
            // Try to restore backup on failure
            if backup_path.exists() {
                let _ = fs::rename(&backup_path, &install_path);
            }
            ForgeError::UpdateInstall {
                message: format!("Failed to install new binary: {}", e),
            }
        })?;

        // Set executable permissions
        fs::set_permissions(&install_path, fs::Permissions::from_mode(0o755)).map_err(|e| {
            ForgeError::UpdateInstall {
                message: format!("Failed to set executable permissions: {}", e),
            }
        })?;

        // Clean up environment variables
        // SAFETY: These environment variables are only used during the update process
        // and are safe to remove as we're done with them. No other threads should be
        // accessing them during this cleanup phase.
        unsafe {
            env::remove_var("FORGE_AUTO_RESTART");
            env::remove_var("FORGE_INSTALL_PATH");
            env::remove_var("FORGE_STAGING_PATH");
        }

        // Save the new version to ~/.forge/version
        let current_version = env!("CARGO_PKG_VERSION");
        if let Err(e) = save_current_version(current_version) {
            warn!("Failed to save version file: {}", e);
            // Don't fail the update just because version tracking failed
        }

        info!("Self-install complete! Running from {:?}", install_path);

        Ok(Some(install_path))
    } else {
        Ok(None)
    }
}

/// Get the staging path for the downloaded binary.
fn get_staging_path(_current_exe: &Path) -> crate::Result<PathBuf> {
    let temp_dir = std::env::temp_dir();
    let staging_file = temp_dir.join(format!("forge-update-{}", std::process::id()));
    Ok(staging_file)
}

/// Get the final install path for the binary.
///
/// Determines the canonical install location (e.g., ~/.cargo/bin/forge).
fn get_install_path(current_exe: &Path) -> crate::Result<PathBuf> {
    // Try to determine if we're running from ~/.cargo/bin
    if let Some(home) = env::var_os("HOME") {
        let cargo_bin = PathBuf::from(home).join(".cargo").join("bin").join("forge");
        if cargo_bin.exists() || current_exe == cargo_bin {
            return Ok(cargo_bin);
        }
    }

    // Fall back to current executable location
    Ok(current_exe.to_path_buf())
}

/// Execute a binary, replacing the current process (Unix-only).
///
/// This uses the `exec` family of syscalls to replace the current process
/// with the new binary. This function does not return on success.
#[cfg(unix)]
fn exec_binary(path: &Path, args: &[String]) -> crate::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    // Convert path to CString
    let path_cstring = CString::new(path.as_os_str().as_bytes()).map_err(|e| {
        ForgeError::UpdateInstall {
            message: format!("Failed to convert path to CString: {}", e),
        }
    })?;

    // Convert args to CString array
    let mut c_args: Vec<CString> = vec![path_cstring.clone()];
    for arg in args {
        let c_arg = CString::new(arg.as_bytes()).map_err(|e| ForgeError::UpdateInstall {
            message: format!("Failed to convert arg to CString: {}", e),
        })?;
        c_args.push(c_arg);
    }

    // Convert to raw pointers for execv
    let mut c_arg_ptrs: Vec<*const i8> = c_args.iter().map(|s| s.as_ptr()).collect();
    c_arg_ptrs.push(std::ptr::null()); // NULL-terminated array

    // Call execv - this replaces the current process
    unsafe {
        libc::execv(path_cstring.as_ptr(), c_arg_ptrs.as_ptr());
    }

    // If we get here, exec failed
    let errno = std::io::Error::last_os_error();
    Err(ForgeError::UpdateInstall {
        message: format!("execv() failed: {}", errno),
    })
}

/// Non-Unix platforms are not supported for self-update restart.
#[cfg(not(unix))]
fn exec_binary(_path: &Path, _args: &[String]) -> crate::Result<()> {
    Err(ForgeError::UpdateInstall {
        message: "Self-update restart not supported on this platform".to_string(),
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

/// Get the path to the version tracking file (~/.forge/version).
fn get_version_file_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".forge")
        .join("version")
}

/// Get the path to the startup marker file (~/.forge/.startup-in-progress).
/// This file is created at startup and deleted on successful initialization.
fn get_startup_marker_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".forge")
        .join(".startup-in-progress")
}

/// Save the current version to ~/.forge/version.
///
/// This is called after a successful update to track the new version.
pub fn save_current_version(version: &str) -> crate::Result<()> {
    let version_file = get_version_file_path();

    // Ensure parent directory exists
    if let Some(parent) = version_file.parent() {
        fs::create_dir_all(parent).map_err(|e| ForgeError::UpdateInstall {
            message: format!("Failed to create .forge directory: {}", e),
        })?;
    }

    fs::write(&version_file, version).map_err(|e| ForgeError::UpdateInstall {
        message: format!("Failed to write version file: {}", e),
    })?;

    info!("Saved version {} to {:?}", version, version_file);
    Ok(())
}

/// Read the last known version from ~/.forge/version.
pub fn read_last_version() -> Option<String> {
    let version_file = get_version_file_path();
    fs::read_to_string(&version_file).ok().map(|s| s.trim().to_string())
}

/// Mark startup as in progress by creating a marker file.
/// This file will be deleted on successful startup.
pub fn mark_startup_in_progress() -> crate::Result<()> {
    let marker_file = get_startup_marker_path();

    // Ensure parent directory exists
    if let Some(parent) = marker_file.parent() {
        fs::create_dir_all(parent).map_err(|e| ForgeError::UpdateInstall {
            message: format!("Failed to create .forge directory: {}", e),
        })?;
    }

    fs::write(&marker_file, "").map_err(|e| ForgeError::UpdateInstall {
        message: format!("Failed to create startup marker: {}", e),
    })?;

    debug!("Created startup marker: {:?}", marker_file);
    Ok(())
}

/// Mark startup as successful by deleting the marker file.
pub fn mark_startup_successful() -> crate::Result<()> {
    let marker_file = get_startup_marker_path();

    if marker_file.exists() {
        fs::remove_file(&marker_file).map_err(|e| ForgeError::UpdateInstall {
            message: format!("Failed to remove startup marker: {}", e),
        })?;
        debug!("Removed startup marker: {:?}", marker_file);
    }

    Ok(())
}

/// Check if the previous startup crashed (marker file exists).
pub fn did_previous_startup_crash() -> bool {
    get_startup_marker_path().exists()
}

/// Rollback result indicating what happened.
#[derive(Debug, Clone, PartialEq)]
pub enum RollbackResult {
    /// Successfully rolled back from backup
    RolledBack {
        /// Version that was rolled back from
        failed_version: String,
        /// Version rolled back to (if known)
        restored_version: Option<String>,
    },
    /// No rollback needed (no crash detected)
    NotNeeded,
    /// Rollback failed (backup not found or other error)
    Failed(String),
}

/// Attempt to rollback to the .old backup if startup crashed.
///
/// This should be called at the very beginning of main() before any other initialization.
///
/// Returns:
/// - `RollbackResult::RolledBack` if rollback was performed
/// - `RollbackResult::NotNeeded` if no crash was detected
/// - `RollbackResult::Failed` if rollback was needed but failed
pub fn check_and_rollback() -> RollbackResult {
    // Check if previous startup crashed
    if !did_previous_startup_crash() {
        return RollbackResult::NotNeeded;
    }

    warn!("Detected crash on previous startup, attempting rollback...");

    // Get current executable path
    let current_exe = match env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            error!("Failed to get current executable path: {}", e);
            return RollbackResult::Failed(format!("Failed to get executable path: {}", e));
        }
    };

    let backup_path = current_exe.with_extension("old");

    // Check if backup exists
    if !backup_path.exists() {
        error!("Backup file not found: {:?}", backup_path);
        // Clean up the marker file since we can't rollback
        let _ = fs::remove_file(get_startup_marker_path());
        return RollbackResult::Failed("Backup file not found".to_string());
    }

    // Read the failed version (current version before rollback)
    let failed_version = env!("CARGO_PKG_VERSION").to_string();

    // Perform the rollback: backup.old -> current exe
    info!("Rolling back from {:?} to {:?}", current_exe, backup_path);

    // Remove current (broken) exe
    if let Err(e) = fs::remove_file(&current_exe) {
        error!("Failed to remove broken binary: {}", e);
        return RollbackResult::Failed(format!("Failed to remove broken binary: {}", e));
    }

    // Restore from backup
    if let Err(e) = fs::copy(&backup_path, &current_exe) {
        error!("Failed to restore from backup: {}", e);
        return RollbackResult::Failed(format!("Failed to restore from backup: {}", e));
    }

    // Set executable permissions
    if let Err(e) = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755)) {
        error!("Failed to set permissions after rollback: {}", e);
        return RollbackResult::Failed(format!("Failed to set permissions: {}", e));
    }

    // Clean up marker file
    let _ = fs::remove_file(get_startup_marker_path());

    // Get the restored version from the version file (if it exists)
    let restored_version = read_last_version();

    info!("Rollback successful! Restored from backup.");

    RollbackResult::RolledBack {
        failed_version,
        restored_version,
    }
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

    #[test]
    fn test_version_tracking() {
        // Test saving and reading version
        let test_version = "0.1.9";
        save_current_version(test_version).unwrap();
        let read_version = read_last_version();
        assert_eq!(read_version, Some(test_version.to_string()));
    }

    #[test]
    fn test_startup_marker() {
        // Clean up any existing marker
        let marker_path = get_startup_marker_path();
        let _ = fs::remove_file(&marker_path);

        // Should not be in progress initially
        assert!(!did_previous_startup_crash());

        // Mark as in progress
        mark_startup_in_progress().unwrap();
        assert!(did_previous_startup_crash());

        // Mark as successful
        mark_startup_successful().unwrap();
        assert!(!did_previous_startup_crash());
    }

    #[test]
    fn test_rollback_not_needed() {
        // Clean up any existing marker
        let marker_path = get_startup_marker_path();
        let _ = fs::remove_file(&marker_path);

        // Should return NotNeeded when no crash detected
        let result = check_and_rollback();
        assert_eq!(result, RollbackResult::NotNeeded);
    }

    #[test]
    fn test_rollback_no_backup() {
        // Create marker to simulate crash
        mark_startup_in_progress().unwrap();

        // Should fail when backup doesn't exist
        let result = check_and_rollback();
        match result {
            RollbackResult::Failed(msg) => {
                assert!(msg.contains("Backup file not found"));
            }
            _ => panic!("Expected RollbackResult::Failed"),
        }

        // Clean up
        let _ = fs::remove_file(get_startup_marker_path());
    }
}
