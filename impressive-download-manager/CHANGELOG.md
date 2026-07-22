# CHANGELOG

## [0.3.3] – 2026-07-22

### Fixed
- **OS-Native Default Downloads Location**: Resolved issue where default download folder path was fixed to hardcoded user paths. Integrated `dirs::download_dir()` in Rust backend to dynamically detect standard system Downloads locations across Windows, Linux, and macOS.
- **GUI Directory Picker in Settings**: Added a "Browse" button alongside the Default Downloads Directory input in Settings, triggering native folder selection (`rfd::AsyncFileDialog`).
- **NSIS Setup Icon**: Configured `installerIcon`, `headerImage`, and `sidebarImage` under `bundle.windows.nsis` in `tauri.conf.json` so NSIS Windows setup executables render the application icon.
- **Popup Responsiveness on Low-Spec Systems**: Replaced 500ms progress polling loop with real-time zero-delay Tauri IPC event pushing (`download-progress`) and removed heavy GPU `backdrop-filter` blurs on popup windows.
- **Background Autostart (Windows & Linux)**: Added automatic Windows Registry startup registration (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) with `--background` mode, canonicalized Linux `.desktop` paths with `0o755` permissions, and connected the "Launch on Startup" toggle in Settings to a new `toggle_autostart` backend command.
- **Clean Settings Top Bar**: Hidden the "Search downloads..." input in the fixed topbar header when viewing the Settings panel.
- **Nala-Styled Windows Installer Script (`install_dependencies.bat`)**: Completely redesigned the Windows setup script into a step-by-step interactive CLI tool with ANSI color formatting. Added automatic detection for `winget` and `chocolatey` (with user preference selection), Visual Studio C++ Build Tools validation, official `rustup-init.exe` installer handling, Node.js LTS, NSIS, WiX Toolset, optional Docker Desktop installation, and an environment status checklist.

## [0.2.5] – 2026-07-19

### Fixed
- Fixed window button freeze when unhiding the background daemon window on Wayland by briefly toggling the `resizable` window property on reshow (forces GTK to rebuild input mapping regions without any visual flash).
- Resolved cold-start window button freeze on Linux (Wayland) by disabling window transparency (`"transparent": false` in `tauri.conf.json`) to prevent GTK input region mapping issues.
- Removed the temporary `nudge_window` maximize/unmaximize workaround from the frontend and backend.
- Resolved window button freeze by fixing ABBA deadlock in `engine.rs` (app_handle/tasks lock order).
- Fixed download tasks not syncing/displaying in the Tauri app UI.
- Updated version numbers across Cargo.toml, package.json, tauri.conf.json.

## [0.1.2] – 2026-07-18

### Added
- **Version bump** to `0.1.2` in `tauri.conf.json`, `package.json` and `Cargo.toml`.
- **File‑type specific liquid‑wave colors** for:
  - Videos (purple)
  - Audio (magenta)
  - Documents (blue)
  - Archives (amber)
  - **Executables** (red) – new category handling `.exe`, `.msi`, `.deb`, `.rpm`, `.dmg`, `.pkg`, `.appimage`, `.apk`.
- **Category CSS rules** (`.liquid-fill.cat‑*`) added to `src/V2.css`.
- **Download size display** in the “New Download Captured” popup (shows “Estimated File Size: …”).
- **Fast Range GET size fallback**: Automatically sends a partial HTTP GET range request (`Range: bytes=0-0`) to extract content length/range when server HEAD queries fail.
- **Single-Instance Enforcement**: Restricts TCP port 9600 binding to a single instance, communicating with and focusing the running instance on subsequent launches.

### Changed
- **Popup window title** renamed from **“New Download”** to **“New Download Captured”** (both the Tauri window title and the in‑popup header).
- **Removed duplicated header** inside the captured‑download popup – now only the native window title is shown.
- **Popup height** reduced to `370 px` to fit the new layout without scrolling.
- **Liquid‑fill animation handling**:
  - Paused \& completed states now only pause the animation (no background‑color overrides), preserving the file‑type color.
  - Completed state adds a subtle opacity reduction for a polished finish.
- **Added `popupSize` React state** to carry the estimated size from the backend to the UI.
- **Added `executables` entry** in `getFileCategory` (App.tsx).
- **Active downloads sorting**: Ongoing, paused, and queued downloads now sort to the top of the dashboard list.
- **Enabled window controls**: Removed `.resizable(false)` from all popups to guarantee that native Minimize, Maximize, and Close titlebar buttons stay active and functional on Linux/GNOME.

### Fixed
- **Light‑mode footer**: colors now use theme variables, making the footer visible in light mode.
- **Download progress**: completed items always display **100 %** even when size is unknown.
- **Header removal bug**: old “New Download Captured” title block inside the modal is gone.
- **Async locking deadlock**: Replaced async `tokio::sync::RwLock` fields (`status`, `speed`, `eta`) in the download engine with standard library `Mutex` guards, eliminating GUI freezes when downloading.

### Build & Packaging
- Updated build scripts to produce `.deb`, `.rpm`, and `.AppImage` bundles for version `0.1.2`.
- Verified successful packaging and installation on Debian/Ubuntu.

---

## Previous Versions (summary)

- **0.1.1** – Fixed size detection, added wave colors for videos/audio/documents/archives, improved footer visibility, adjusted popup sizing, corrected progress‑snap behavior.
- **0.1.0** – Initial release with core download manager functionality.

---
