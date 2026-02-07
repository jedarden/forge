# Atomic Binary Updates Research

**Goal**: Self-updating binary executable that atomically swaps itself without downtime

**Inspiration**: rustup, kubectl, helm, docker, cargo-binstall

---

## Overview

Instead of Python source code updates, the control panel is distributed as a **compiled binary** that can atomically replace itself during updates.

### Why Binary Distribution?

1. **Single-file deployment**: No Python interpreter, dependencies, or virtualenv needed
2. **Atomic updates**: Binary swap is a single filesystem operation
3. **Zero downtime**: Process can exec into new binary seamlessly
4. **Cross-platform**: Distribute platform-specific binaries (Linux, macOS, Windows)
5. **Faster startup**: No module imports, pre-compiled code

---

## Atomic Binary Swap Mechanism

### Unix/Linux/macOS Strategy

On Unix systems, `rename()` syscall is **atomic**. We leverage this for zero-downtime updates.

#### Update Process

```
1. Download new binary to temporary location
2. Verify checksum/signature
3. Make new binary executable (chmod +x)
4. Atomically rename new binary over old binary
5. Exec into new binary (seamless handoff)
```

#### Implementation (Rust Example)

```rust
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct BinaryUpdater {
    current_binary: PathBuf,
    update_url: String,
}

impl BinaryUpdater {
    async fn perform_update(&self) -> Result<(), UpdateError> {
        // Step 1: Download new binary to temp location
        let temp_binary = self.download_new_binary().await?;

        // Step 2: Verify checksum
        self.verify_checksum(&temp_binary).await?;

        // Step 3: Make executable
        let mut perms = fs::metadata(&temp_binary)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_binary, perms)?;

        // Step 4: Atomic rename (this is the critical step)
        // On Unix, rename() is atomic - either fully succeeds or fully fails
        fs::rename(&temp_binary, &self.current_binary)?;

        // Step 5: Exec into new binary
        self.exec_new_binary()?;

        Ok(())
    }

    async fn download_new_binary(&self) -> Result<PathBuf, UpdateError> {
        let temp_path = std::env::temp_dir().join("control-panel.new");

        // Download with progress
        let response = reqwest::get(&self.update_url).await?;
        let total_size = response.content_length().unwrap_or(0);

        let mut file = fs::File::create(&temp_path)?;
        let mut downloaded = 0u64;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            self.report_progress(downloaded, total_size);
        }

        Ok(temp_path)
    }

    async fn verify_checksum(&self, binary_path: &Path) -> Result<(), UpdateError> {
        // Download checksum file
        let checksum_url = format!("{}.sha256", self.update_url);
        let expected_checksum = reqwest::get(&checksum_url).await?.text().await?;

        // Calculate actual checksum
        let mut file = fs::File::open(binary_path)?;
        let mut hasher = Sha256::new();
        io::copy(&mut file, &mut hasher)?;
        let actual_checksum = format!("{:x}", hasher.finalize());

        if actual_checksum != expected_checksum.trim() {
            return Err(UpdateError::ChecksumMismatch);
        }

        Ok(())
    }

    fn exec_new_binary(&self) -> Result<!, UpdateError> {
        // Get current process arguments
        let args: Vec<String> = std::env::args().collect();

        // Exec into new binary (replaces current process)
        // This is atomic - control transfers to new binary immediately
        let err = exec::Command::new(&self.current_binary)
            .args(&args[1..])
            .exec();

        // exec() never returns on success, only on error
        Err(UpdateError::ExecFailed(err))
    }
}
```

### Windows Strategy

Windows doesn't allow replacing a running executable. Alternative approaches:

#### 1. Rename-and-Replace Pattern

```rust
// Windows atomic update strategy
async fn update_on_windows(&self) -> Result<(), UpdateError> {
    // Step 1: Download new binary
    let new_binary = self.download_new_binary().await?;

    // Step 2: Rename current binary to .old
    let old_binary = self.current_binary.with_extension("old");
    fs::rename(&self.current_binary, &old_binary)?;

    // Step 3: Move new binary to current location
    fs::rename(&new_binary, &self.current_binary)?;

    // Step 4: Spawn new process
    Command::new(&self.current_binary)
        .args(std::env::args().skip(1))
        .spawn()?;

    // Step 5: Exit current process
    std::process::exit(0);
}
```

#### 2. Launcher Pattern (Recommended for Windows)

```
control-panel-launcher.exe  (small stub, never updates)
    └─ launches control-panel-core.exe (actual binary, updates)
```

Launcher handles update logic:
```rust
// control-panel-launcher.exe
fn main() {
    loop {
        // Check if update pending
        if Path::new("control-panel-core.new").exists() {
            // Replace old binary with new
            fs::remove_file("control-panel-core.old").ok();
            fs::rename("control-panel-core.exe", "control-panel-core.old").ok();
            fs::rename("control-panel-core.new", "control-panel-core.exe")?;
        }

        // Launch core binary
        let exit_code = Command::new("control-panel-core.exe")
            .args(std::env::args().skip(1))
            .status()?
            .code();

        // Exit code 42 = restart for update
        if exit_code != Some(42) {
            break;
        }
        // Loop to restart with new binary
    }
}
```

---

## Update Manifest & Version Discovery

### Manifest Format (JSON)

Host a manifest file with latest version info:

```json
{
  "version": "1.3.0",
  "release_date": "2026-02-07T12:00:00Z",
  "binaries": {
    "linux-x64": {
      "url": "https://releases.example.com/control-panel-v1.3.0-linux-x64",
      "sha256": "a1b2c3d4e5f6...",
      "size": 12457600
    },
    "linux-arm64": {
      "url": "https://releases.example.com/control-panel-v1.3.0-linux-arm64",
      "sha256": "f6e5d4c3b2a1...",
      "size": 11834200
    },
    "darwin-x64": {
      "url": "https://releases.example.com/control-panel-v1.3.0-darwin-x64",
      "sha256": "1a2b3c4d5e6f...",
      "size": 13245800
    },
    "darwin-arm64": {
      "url": "https://releases.example.com/control-panel-v1.3.0-darwin-arm64",
      "sha256": "6f5e4d3c2b1a...",
      "size": 12756400
    },
    "windows-x64.exe": {
      "url": "https://releases.example.com/control-panel-v1.3.0-windows-x64.exe",
      "sha256": "d4c3b2a1f6e5...",
      "size": 14567200
    }
  },
  "changelog": {
    "features": [
      "Multi-worker coordination with bead-level locking",
      "Cost analytics dashboard with hourly breakdown"
    ],
    "fixes": [
      "Fixed memory leak in worker spawning (#42)",
      "Fixed race condition in task assignment (#38)"
    ]
  },
  "min_version": "1.0.0",
  "breaking_changes": false
}
```

### Version Check Implementation

```rust
#[derive(Deserialize)]
struct UpdateManifest {
    version: String,
    release_date: String,
    binaries: HashMap<String, BinaryInfo>,
    changelog: Changelog,
    min_version: String,
    breaking_changes: bool,
}

#[derive(Deserialize)]
struct BinaryInfo {
    url: String,
    sha256: String,
    size: u64,
}

async fn check_for_update() -> Result<Option<UpdateManifest>, UpdateError> {
    let manifest_url = "https://releases.example.com/manifest.json";
    let manifest: UpdateManifest = reqwest::get(manifest_url)
        .await?
        .json()
        .await?;

    let current_version = env!("CARGO_PKG_VERSION");

    if Version::parse(&manifest.version)? > Version::parse(current_version)? {
        Ok(Some(manifest))
    } else {
        Ok(None)
    }
}
```

---

## Build & Distribution Pipeline

### GitHub Actions CI/CD

```yaml
# .github/workflows/release.yml
name: Build and Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            name: linux-x64
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            name: linux-arm64
          - os: macos-latest
            target: x86_64-apple-darwin
            name: darwin-x64
          - os: macos-latest
            target: aarch64-apple-darwin
            name: darwin-arm64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            name: windows-x64.exe

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v3

      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          target: ${{ matrix.target }}

      - name: Build binary
        run: cargo build --release --target ${{ matrix.target }}

      - name: Strip binary (Unix)
        if: runner.os != 'Windows'
        run: strip target/${{ matrix.target }}/release/control-panel

      - name: Calculate checksum
        run: |
          cd target/${{ matrix.target }}/release
          sha256sum control-panel* > control-panel-${{ matrix.name }}.sha256

      - name: Upload to release
        uses: actions/upload-release-asset@v1
        with:
          upload_url: ${{ github.event.release.upload_url }}
          asset_path: ./target/${{ matrix.target }}/release/control-panel
          asset_name: control-panel-${{ github.ref_name }}-${{ matrix.name }}

  update-manifest:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - name: Generate manifest
        run: |
          # Generate manifest.json with URLs and checksums
          python scripts/generate_manifest.py ${{ github.ref_name }}

      - name: Upload manifest
        run: |
          aws s3 cp manifest.json s3://releases.example.com/manifest.json
```

---

## Self-Update Implementation

### Update Command (Rust)

```rust
// control-panel/src/update.rs

pub async fn perform_self_update(ui: &mut UI) -> Result<(), UpdateError> {
    ui.show_message("Checking for updates...");

    // Check for updates
    let manifest = match check_for_update().await? {
        Some(m) => m,
        None => {
            ui.show_message("✓ Already up to date");
            return Ok(());
        }
    };

    ui.show_update_prompt(&manifest);

    // Wait for user confirmation
    if !ui.confirm_update() {
        return Ok(());
    }

    // Detect platform
    let platform = detect_platform();
    let binary_info = manifest.binaries.get(&platform)
        .ok_or(UpdateError::PlatformNotSupported)?;

    // Download with progress
    ui.start_progress_bar(binary_info.size);
    let temp_binary = download_with_progress(&binary_info.url, |downloaded, total| {
        ui.update_progress(downloaded, total);
    }).await?;

    // Verify checksum
    ui.show_message("Verifying checksum...");
    verify_checksum(&temp_binary, &binary_info.sha256)?;

    // Perform atomic swap
    ui.show_message("Installing update...");
    let current_binary = std::env::current_exe()?;

    #[cfg(unix)]
    {
        atomic_replace_unix(&temp_binary, &current_binary)?;
        exec_new_binary(&current_binary)?;
    }

    #[cfg(windows)]
    {
        atomic_replace_windows(&temp_binary, &current_binary)?;
        restart_process()?;
    }

    Ok(())
}

fn detect_platform() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("macos", "aarch64") => "darwin-arm64",
        ("windows", "x86_64") => "windows-x64.exe",
        _ => panic!("Unsupported platform: {}-{}", os, arch),
    }.to_string()
}

#[cfg(unix)]
fn atomic_replace_unix(new_binary: &Path, current_binary: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;

    // Make new binary executable
    let mut perms = fs::metadata(new_binary)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(new_binary, perms)?;

    // Atomic rename
    fs::rename(new_binary, current_binary)?;

    Ok(())
}

#[cfg(unix)]
fn exec_new_binary(binary: &Path) -> ! {
    let args: Vec<String> = std::env::args().collect();

    // exec() replaces current process with new binary
    let err = exec::Command::new(binary)
        .args(&args[1..])
        .exec();

    // Should never reach here
    panic!("exec failed: {}", err);
}

#[cfg(windows)]
fn atomic_replace_windows(new_binary: &Path, current_binary: &Path) -> Result<(), UpdateError> {
    // Rename current to .old
    let old_binary = current_binary.with_extension("old");
    fs::rename(current_binary, &old_binary).ok(); // Ignore if doesn't exist

    // Move new binary to current location
    fs::rename(new_binary, current_binary)?;

    Ok(())
}

#[cfg(windows)]
fn restart_process() -> ! {
    let current_exe = std::env::current_exe().unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Spawn new process
    Command::new(&current_exe)
        .args(&args)
        .spawn()
        .expect("Failed to restart");

    // Exit current process
    std::process::exit(0);
}
```

---

## State Preservation Across Updates

### Save State Before Exec

```rust
fn save_state_before_update(app_state: &AppState) -> Result<(), UpdateError> {
    let state_file = dirs::data_dir()
        .ok_or(UpdateError::NoDataDir)?
        .join("control-panel")
        .join("state_before_update.json");

    let serialized = serde_json::to_string_pretty(app_state)?;
    fs::write(&state_file, serialized)?;

    Ok(())
}
```

### Restore State After Exec

```rust
fn restore_state_after_update() -> Option<AppState> {
    let state_file = dirs::data_dir()?
        .join("control-panel")
        .join("state_before_update.json");

    if !state_file.exists() {
        return None;
    }

    let contents = fs::read_to_string(&state_file).ok()?;
    let state: AppState = serde_json::from_str(&contents).ok()?;

    // Delete state file after successful restore
    fs::remove_file(&state_file).ok();

    Some(state)
}
```

### AppState Structure

```rust
#[derive(Serialize, Deserialize)]
struct AppState {
    // Worker pool state
    workers: Vec<WorkerInfo>,

    // Conversation history
    conversation_history: Vec<ConversationExchange>,

    // UI state
    ui: UIState,

    // Filters and settings
    filters: FilterSettings,
    preferences: UserPreferences,

    // Timestamp
    saved_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize)]
struct UIState {
    active_panel: String,
    scroll_positions: HashMap<String, usize>,
    expanded_sections: HashSet<String>,
}
```

---

## Rollback on Update Failure

### Backup Current Binary

```rust
fn backup_current_binary() -> Result<PathBuf, UpdateError> {
    let current_binary = std::env::current_exe()?;
    let backup_path = current_binary.with_extension("backup");

    fs::copy(&current_binary, &backup_path)?;

    Ok(backup_path)
}
```

### Restore from Backup

```rust
fn rollback_to_backup(backup_path: &Path) -> Result<(), UpdateError> {
    let current_binary = std::env::current_exe()?;

    #[cfg(unix)]
    {
        fs::rename(backup_path, &current_binary)?;
        exec_new_binary(&current_binary)?;
    }

    #[cfg(windows)]
    {
        fs::copy(backup_path, &current_binary)?;
        restart_process();
    }

    Ok(())
}
```

---

## Update UI Flow

```
Press [U]

┌─ UPDATE AVAILABLE ──────────────────────────────────────────────────┐
│ Current version: v1.2.3                                             │
│ New version:     v1.3.0 (released 2 days ago)                       │
│                                                                      │
│ What's New:                                                          │
│ ✨ Multi-worker coordination with bead-level locking                │
│ ✨ Cost analytics dashboard with hourly breakdown                   │
│ 🐛 Fixed memory leak in worker spawning (#42)                       │
│                                                                      │
│ Download size: 12.4 MB                                               │
│                                                                      │
│ Proceed? [Y/n] _                                                     │
└──────────────────────────────────────────────────────────────────────┘

[Press Y]

┌─ DOWNLOADING UPDATE ────────────────────────────────────────────────┐
│ Downloading control-panel-v1.3.0-linux-x64...                       │
│                                                                      │
│ ████████████████████████░░░░░░░░░░░░  68%                           │
│ 8.4 MB / 12.4 MB  •  2.1 MB/s  •  2s remaining                      │
│                                                                      │
│                                                    [Esc] Cancel      │
└──────────────────────────────────────────────────────────────────────┘

Download complete

┌─ INSTALLING UPDATE ─────────────────────────────────────────────────┐
│ ✓ Download complete (12.4 MB)                                       │
│ ✓ Checksum verified (a1b2c3d4...)                                   │
│ ✓ Backup created                                                    │
│ ✓ Binary replaced                                                   │
│                                                                      │
│ Restarting in new version...                                        │
└──────────────────────────────────────────────────────────────────────┘

[Binary execs into new version]

╔═══════════════════════════════════════════════════════════════════╗
║ CONTROL PANEL v1.3.0                                             ║
║ ✓ Updated from v1.2.3                                            ║
║                                                                   ║
║ [Press Enter to continue]                                         ║
╚═══════════════════════════════════════════════════════════════════╝
```

---

## Alternative: Self-Extracting Installer

For more complex installations (with assets, config templates, etc.):

```rust
// Embed tarball in binary
const PAYLOAD: &[u8] = include_bytes!("../dist/control-panel.tar.gz");

fn self_extract_and_install() -> Result<(), UpdateError> {
    // Extract embedded tarball
    let temp_dir = tempdir()?;
    let mut archive = Archive::new(GzDecoder::new(PAYLOAD));
    archive.unpack(&temp_dir)?;

    // Install files
    let install_dir = dirs::data_dir()
        .ok_or(UpdateError::NoDataDir)?
        .join("control-panel");

    fs::create_dir_all(&install_dir)?;

    // Copy binary
    fs::copy(
        temp_dir.path().join("control-panel"),
        install_dir.join("control-panel")
    )?;

    // Copy assets
    copy_dir_all(
        temp_dir.path().join("assets"),
        install_dir.join("assets")
    )?;

    Ok(())
}
```

---

## Comparison: Binary Update Strategies

| Strategy | Pros | Cons | Best For |
|----------|------|------|----------|
| **Atomic rename (Unix)** | Zero downtime, instant swap | Unix-only | Production servers |
| **Launcher stub (Windows)** | Clean separation, easy rollback | Extra binary | Windows systems |
| **Self-extracting installer** | Can bundle assets | Larger binary | Complex deployments |
| **Remote binary fetch** | Smallest initial binary | Requires internet | Auto-updates |

---

## Recommended Stack

**Language**: Rust
- Fast compilation
- Single binary output (no runtime dependencies)
- Excellent async support
- Cross-compilation built-in

**TUI Framework**: Ratatui (Rust)
- Mature terminal UI framework
- Similar to Python's Textual
- Low latency, high performance

**HTTP Client**: reqwest
- Async downloads with progress
- Robust error handling

**Serialization**: serde + serde_json
- Fast (de)serialization
- Schema evolution support

**Build**: cargo
- Built-in cross-compilation
- Easy CI/CD integration

---

## Implementation Checklist

- [ ] Create Rust binary project (`cargo new control-panel`)
- [ ] Implement TUI with Ratatui
- [ ] Add self-update module with atomic replace
- [ ] Set up GitHub Actions for multi-platform builds
- [ ] Create release manifest endpoint
- [ ] Add checksum verification
- [ ] Implement state save/restore
- [ ] Add rollback capability
- [ ] Test on Linux, macOS, Windows
- [ ] Write installation docs (`curl | sh` installer)
- [ ] Set up automatic update checking
- [ ] Add update notification banner

---

## Resources

- **Rust self-update crate**: https://github.com/jaemk/self_update
- **Ratatui TUI framework**: https://github.com/ratatui-org/ratatui
- **Example: rustup self-update**: https://github.com/rust-lang/rustup
- **Example: kubectl self-update**: https://github.com/kubernetes/kubectl
- **Atomic file operations**: https://rcrowley.org/2010/01/06/things-unix-can-do-atomically.html

This approach provides **ccdash-like updates** with a single keypress, atomic binary swaps, and zero downtime!
