# CHANGELOG

## [0.4.4] – 2026-07-25

### Fixed
- **100% Mandatory IDM Background Capture**: Configured browser extension to unconditionally cancel native browser downloads immediately and pass 100% of download payloads directly to Impressive Download Manager background engine on port 9600.
- **Background Autostart Sync**: Enhanced startup initialization in frontend (`App.tsx`) and backend (`lib.rs`) to persist autostart preference in `localStorage` and register system startup entries across Windows Registry (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) and Linux (`~/.config/autostart`).
- **Updater Plugin ACL Permission Fix**: Granted `"updater:default"` permission in `src-tauri/capabilities/default.json` so the frontend webview is authorized to invoke updater IPC commands (`plugin:updater|check` and `plugin:updater|download_and_install`).
- **Fast Retry Tuning**: Reduced link capture deduplication window to 2500ms so rapid manual link re-clicks work instantly.

## [0.4.1] – 2026-07-24

### Added
- **In-App Secure Software Updater**: Integrated cryptographic key-pair signed auto-updates (`tauri-plugin-updater` & `@tauri-apps/plugin-updater`). Configured signed updater endpoints in `src-tauri/tauri.conf.json` (`latest.json`), registered updater plugin in Rust backend (`src-tauri/src/lib.rs`), and added a manual "Check for Updates" UI button at the bottom of Settings.
- **Silent Background & Manual Update Checks**: App automatically checks for signed updates on startup in the background and stages updates seamlessly.

### Fixed
- **Strict 100% IDM Download Interception**: Configured browser extension to unconditionally cancel standard browser downloads immediately and pass 100% of download payloads to Impressive Download Manager background engine on port 9600.
- **Fast Retry Tuning**: Reduced link capture deduplication window from 6000ms to 2500ms so rapid manual re-clicks work instantly.
- **Standalone Popup Window Navigation Fix**: Fixed relative URL path resolution in Rust backend (`src-tauri/src/lib.rs`). Replaced absolute leading slashes (`/index.html?...`) with relative paths (`index.html?...`) in `tauri::WebviewUrl::App(...)` constructor calls across popup builders (`open_progress_window`, `open_complete_window`, `refresh_download_link`, and `popup-add`), resolving "Could not connect to localhost: Connection refused" blank screen errors.
- **User-Focused Documentation (`README.md`)**: Restructured `README.md` to focus entirely on end-user features, installation instructions pointing directly to pre-compiled binaries in the GitHub Releases section, and browser extension setup.

## [0.3.3] – 2026-07-22

### Fixed
- **OS-Native Default Downloads Location**: Resolved issue where default download folder path was fixed to hardcoded user paths. Integrated `dirs::download_dir()` in Rust backend to dynamically detect standard system Downloads locations across Windows, Linux, and macOS.
- **GUI Directory Picker in Settings**: Added a "Browse" button alongside the Default Downloads Directory input in Settings, triggering native folder selection (`rfd::AsyncFileDialog`).
- **NSIS Setup Icon**: Configured `installerIcon`, `headerImage`, and `sidebarImage` under `bundle.windows.nsis` in `tauri.conf.json` so NSIS Windows setup executables render the application icon.
- **Popup Responsiveness & Aesthetics**: Configured progress popup polling loop to a fast 200ms interval for immediate responsiveness without UI state drops, and preserved full glassmorphic popup styling.
- **Background Autostart (Windows & Linux)**: Added automatic Windows Registry startup registration (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) with `--background` mode, canonicalized Linux `.desktop` paths with `0o755` permissions, and connected the "Launch on Startup" toggle in Settings to a new `toggle_autostart` backend command.
- **Clean Settings Top Bar**: Hidden the "Search downloads..." input in the fixed topbar header when viewing the Settings panel.

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
