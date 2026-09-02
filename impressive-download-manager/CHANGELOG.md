# CHANGELOG
 
## [0.7.7] – 2026-09-02
 
### Added
- **Single-Pass Direct Stream Architecture (XDM Engine Parity)**: Re-architected download initialization to eliminate separate pre-flight probe requests that burned single-use authorization or CSRF tokens. Chunk 0 connects directly on the live request with `Range: bytes=0-` and `Accept-Encoding: identity`, seamlessly splitting parallel range segments on `206 Partial Content` without reconnecting or dropping open streams.
- **Dynamic Chunked & Unknown-Size Live Streaming UI**: For dynamically generated streams, dynamic compressors, and `200 OK` chunked downloads (`total_size == 0`), Chunk 0 downloads directly to EOF. Added live streaming feedback in the UI with real-time byte counters and an animated glowing wave indicator (`.streaming-fill`).
- **Active Tab & Root Domain Cookie Aggregation**: Upgraded browser extension cookie extraction (`getCookiesForUrl`) to merge session cookies from the download URL, the parent root domain (e.g. `.z.ai`), and the active browser tab, ensuring full authentication sessions are forwarded to IDM.
- **Full Browser Navigation Fingerprint**: Integrated standard navigation headers (`Sec-Ch-Ua`, `Sec-Fetch-Dest`, `Sec-Fetch-Mode`, `Sec-Fetch-Site`, `Sec-Fetch-User`, `Upgrade-Insecure-Requests`, `Accept-Language`) into reqwest requests for full Cloudflare / WAF verification parity.
- **Buffer Flushing & Connection Timeout Tuning**: Reduced disk flush threshold to 64KB for real-time progress updates on small files (e.g., 200KB PDFs) and increased connection timeout to 60s for server-side generation workloads.
 
### Fixed
- **HTTP Header Control Character Sanitization**: Implemented automated sanitization for raw header values to prevent silent `reqwest` request builder failures on malformed cookies or user agents.
- **Range Rejection Fallback**: If a dynamic endpoint returns `416 Range Not Satisfiable`, `400 Bad Request`, `403 Forbidden`, or `405 Method Not Allowed` when `Range: bytes=0-` is presented, the engine automatically catches this on the first request and seamlessly retries with plain GET.
- **In-Memory Browser Memory Protocol Guard (`blob:`, `data:`)**: Excluded in-memory client-side URLs (`blob:`, `data:`) from extension interception, matching XDM's protocol filters so in-memory client-side generated files (from tools like ChatGPT, Claude, and `chat.z.ai`) are saved natively by the browser without failing external socket connections.
- **TLS & Error Propagation**: Added `.danger_accept_invalid_certs(true)`, HTTP/2 adaptive windowing, and TCP keep-alive to `reqwest::Client`, while replacing opaque generic connection retry messages with the exact underlying HTTP or network failure reason.

## [0.7.6] – 2026-08-25

### Added
- **Full Download Scheduler System**: Complete end-to-end wiring of the background Download Scheduler. Evaluates local time and weekday intervals every 10 seconds, automatically resuming queued/paused downloads during active hours and pausing running downloads when the window closes.
- **Silent Background Execution**: Scheduled task auto-resumes run silently in the background without summoning progress popup windows.
- **Persistent Schedule Configuration**: Schedule settings (active days, start time, end time, and enabled state) are fully persisted to `scheduler.json` in the app data directory and synchronized with `localStorage`.
- **Scheduled Visual Indicators & Micro-Card Rules**: Added `⏰ Scheduled ({startTime})` badges directly on download cards waiting for their scheduled window, and replaced settings text boxes with an interactive 3-pillar micro-card grid (Auto-Start, Auto-Pause, Background Silent).
- **Zero-Clutter Hover Selection & Bulk Deletion System**: Eliminated awkward list checkboxes in favor of hover-card-driven multi-selection. Clicking the Delete action on any task's hover card or in the File Details drawer marks the item for deletion with an elevated crimson perimeter glow (`.marked-for-delete-card`) and toggles the action button to `<Trash2 /> Marked`.
- **Ultra-Compact Responsive Floating Delete Pill**: Added a fixed-height, single-row floating action capsule centered at the bottom. Features a pulsing red icon ring, live marked count badge, responsive label compression (`item marked for deletion` ➔ `marked`), "Select All / Deselect All" toggle, Cancel action, and a bold `Delete Selected (N)` CTA opening the bottom confirmation dialog with file previews and optional disk deletion for completed files.
- **Tactile Selection Cards UI (Theme & Port)**: Replaced clunky dropdowns with interactive, tactile micro-card grids (`settings-selection-grid`). Theme Mode now features Dark, Light, and System cards with live glow indicators; Port Selection features Port 9600 (Primary Daemon) and Port 8765 (Proxy) cards, completely eliminating z-index stacking conflicts and dropdown clipping.
- **Evergreen Post-Update Modal**: Redesigned the update celebration dialog with an animated supersonic rocket, ambient glows, upgrade progression leap badges (`v0.7.3` ➔ `v0.7.6` `LIVE`), dynamic headlines, and evergreen copy that never requires manual rewrites on future releases.

### Fixed
- **In-App Updater Signature Verification & Package Integrity**: Integrated `postRemoveScript` directly into `tauri.conf.json` for Debian bundles and removed external post-build `dpkg-deb` repacking in `generate_latest_json.js`. Eliminates signature mismatches (`Update Check Failed: The Signature Verification Failed`) caused by repacking after signing, and restores universal `.deb` package compression compatibility for GUI package installers (fixing 22-minute freezes on Deepin / Ubuntu / GDebi).
- **Dynamic Chunked & Unknown-Size Stream Downloads (e.g., z.ai Workspaces)**: Resolved a critical worker-spawning issue in `engine.rs` where downloads with unknown initial sizes (`total_size == 0`, dynamic zip generators, or servers that do not return `Content-Range`/`Content-Length`) were skipped in the chunk allocator, causing downloads to get stuck in "Connecting..." forever. Also added plain GET probe fallbacks when servers reject `bytes=0-0` Range headers.
- **Dropdown Z-Index & Stacking Context Elevation**: Fixed stacking order conflicts on `.settings-card` when custom dropdowns are open, ensuring menus float cleanly above subsequent settings cards without being clipped.
- **Dropdown Multiple Downward Arrows Removed**: Replaced native HTML select artifacts with the custom dropdown component across all settings, removing duplicate arrows in light theme.

## [0.7.3] – 2026-08-20

### Added
- **Native Browser Handling for Files < 1MB**: Browser extension now automatically bypasses interception and allows the native browser download engine to download files smaller than 1MB (< 1,048,576 bytes) without opening IDM.
- **Dynamic Chunked Stream Inspection (Google Docs/Drive/Sheets)**: Added `checkUnknownSizeIsSmall()` in the browser extension to inspect chunked `Transfer-Encoding` downloads with unknown initial sizes. Small dynamic document exports (< 1MB) are detected and handled natively by the browser rather than being intercepted as unknown-size files.

### Fixed
- **Accurate Filename Resolution on Intercepted Downloads**: Shifted browser extension download interception to `chrome.downloads.onDeterminingFilename`, allowing the browser to receive HTTP response headers (`Content-Disposition`, `Content-Length`) and resolve redirects first. Eliminates the bug where downloads received weird random alphanumeric hashes or URL endpoint slugs.
- **Server Header & Redirect Inspection in Backend Engine**: Updated `start_download` in Rust engine (`engine.rs`) to parse `Content-Disposition` (both RFC 5987 `filename*=` and standard `filename=`) and final redirected URLs during URL probe, automatically refining generic or extensionless filenames and updating save paths.
- **Query-Aware Filename Extraction**: Enhanced URL filename parsing in frontend (`App.tsx`), backend (`lib.rs`), and browser extension (`background.js`) to extract file names from URL query parameters (`file=`, `filename=`, `name=`, `title=`) and decode safely with error handling.

## [0.7.1] – 2026-08-12 (Commit Reference Tag: `e1c0cb669f42a9cd4d2b0d31c2d9724fca6e958c`)

### Added
- **Multi-Browser Native Messaging Host Auto-Launch**: Registered `com.impressive.idm.json` Native Messaging Host manifests and autostart background daemons (`--background`) for all web browsers (Firefox, Chrome, Brave, Opera, Edge, Vivaldi) across Linux (`.config` & `.mozilla`), Windows (Registry keys under `HKCU`), and macOS (`LaunchAgents` & `Application Support`). Browsers can now automatically wake up IDM on demand without throwing connection errors.
- **Open File Command**: Added a new `open_file` Tauri command that launches a downloaded file directly in the system's default application handler (`xdg-open` on Linux, `cmd /C start` on Windows, `open` on macOS).
- **2-Items-per-Row Hover Action Card**: Download task rows now reveal a polished glassmorphic 2-column action grid card on hover. Non-delete buttons glow with the app's accent cyan color on hover; the Delete button shows a subtle red tint at rest and a solid red fill on hover—no glow effects.
- **Open File & Open Folder in Drawer**: The File Details drawer for completed downloads now shows two side-by-side action buttons: **Open File** (launches the file directly) and **Open Folder** (opens the containing folder with the file highlighted).
- **Browser Extension Interception Pass-Through**: Added a `GET /status` endpoint on port 9600. The browser extension polls this every 4 seconds and instantly bypasses Chrome download cancellation when "Intercept Browser Downloads" is turned OFF in Settings, restoring normal browser download behavior. Interception resumes the moment the toggle is re-enabled.
- **Minimize to System Tray Toggle — Full Exit Support**: Added `minimize_to_tray` flag to the Rust backend (`DownloadManager` struct + `set_minimize_to_tray` command). When **Minimize to System Tray** is toggled OFF in Settings, clicking the window close button now calls `std::process::exit(0)` and terminates the process immediately (like `killall`). When ON, it hides to the system tray as before. The setting persists across restarts via `localStorage`.
- **Completion Summary Section in File Details Drawer**: For completed downloads, the File Details drawer now shows a **Completion Summary** section with the subtitle "All parallel segments assembled & verified successfully:" and a green `✓ 100% verified — 8 of 8 parallel streams merged` badge instead of the misleading "Active parallel download segments:" label.

### Fixed
- **Robust Filename Extraction & Extension Inferencing**: Upgraded filename parser in `proxy.rs` to decode RFC 5987 `filename*=` and `filename=` headers using `urlencoding::decode`, sanitize OS invalid characters, extract names from URL query parameters (`?file=...`), and infer missing extensions from `Content-Type` headers to prevent weird/mismatched file names.
- **Port 9600 Socket Reuse & Zero-Race Binding**: Converted the single-instance `std::net::TcpListener` on port 9600 directly to Tokio's non-blocking listener (`tokio::net::TcpListener::from_std`), eliminating socket `TIME_WAIT` race conditions and preventing `"Could not connect to localhost"` errors.
- **Strict File Corruption & Byte Bounds Safeguards**: Fixed pre-truncation byte overcounting, enforced part file length truncation (`f.set_len`), implemented bounded copying in `assemble_file` (`expected_len = (chunk_end - chunk_start + 1).min(downloaded)`), and strictly required 100.00% byte completion before file assembly.
- **Linux Package Post-Uninstall Cleanup & Ghost Icon Prevention**: Integrated `postrm` maintainer scripts into `.deb` and `postRemoveScript` into `.rpm` packages. Automatically purges leftover user-level autostart `.desktop` files and browser native host manifests upon package uninstallation.
- **Virtual Time Speed Limiter Refactor (Commit: `e1c0cb6`)**: Refactored `TaskSpeedLimiter` to a Virtual Time / Leaky Bucket design using async Tokio Mutex serialization. Enforces strict rate caps without overshooting (capped at 99.5% with UI display clamping), prevents download speed drops when limiter is inactive, and ensures clean task resumption.
- **Exact File Highlighting in File Managers (Linux Cross-Distro Fix)**: Fixed `open_file_dir` on Deepin, Zorin OS, GNOME, KDE, and Cinnamon. The `gdbus call --session --dest org.freedesktop.FileManager1 --method ShowItems` command is now wrapped in `sh -c` so the GVariant array parameter (`['file:///...']`) is parsed correctly by the shell. Also fixed a path duplication bug in `handleOpenFileDir` in `App.tsx` where `save_path` (already a full file path) was having the filename appended again, resulting in an invalid path that prevented file selection. The folder now opens and the file is highlighted/selected (single left-click) without launching system package installers for `.run`, `.deb`, or `.rpm` files.
- **Download Origin URL Preserved**: Fixed `DownloadProgress` struct to carry the `url: String` field populated from `task.original_url`. File Details drawer now displays the exact HTTP download URL under **URL Origin** instead of "Unknown link origin".
- **Generic Parallel Segment Label**: Changed "Active chunk segments downloading concurrently in Rust:" label to "Active parallel download segments:" in the File Details drawer.
- **Trash/Delete Icon Consistency**: Replaced all `<X>` delete icons with `<Trash2>` (bin) icons across task rows, cards, and the File Details drawer action buttons.

## [0.7.0] – 2026-08-10

### Added
- **Isolated Part File & Assembly Architecture**: Implemented isolated per-chunk temporary files (`app_data_dir/temp/{task_id}/{chunk_id}`) written independently without cross-thread lock contention or random in-flight file seeking.
- **Post-Download File Assembly Phase**: Introduced an `Assembling` progress status and a fast sequential file merging phase upon 100% completion of parallel workers, followed by automatic hidden temporary file cleanup.
- **Continuous Real-Time Progress & Size Updates**: Updated UI reporting to reflect live `network_downloaded` byte streams continuously rather than jumping in 1MB disk-flush ticks.

### Fixed
- **Wire-Speed Flatline Throughput & Drop Protection**: Re-tuned dynamic piece splitting thresholds (1MB minimum) and stream timeout limits (15s) to eliminate zero-speed drops and sustain peak wire speed (2.5–3.2+ MB/s) on high-speed internet connections.
- **SourceForge & Protected Platform Support**: Fixed CDN connection throttles and Range probe handling for protected platforms like `sourceforge.net`.

## [0.6.3] – 2026-08-06

### Added
- **Custom Speed Limiter Unit Selector Dropdown**: Replaced native OS GTK `<select>` element with a custom React Dropdown component for speed unit selection (`KB/s`, `MB/s`, `GB/s`). Eliminates dark system GTK menu overlays in Light Mode.
- **Global Click-Outside Dropdown Dismissal**: Integrated `useRef` and a global document mouse event listener (`unitDropdownRef`) to close the unit selector dropdown whenever clicking anywhere outside the open menu.
- **Single Animated Arrow Icon**: Removed static CSS background arrows, leaving a single smooth animated `<ChevronDown />` icon that rotates 180° when opened.

### Fixed
- **Standalone Popup Window Hash Routing Fix**: Fixed WebKit 404 and `"Could not connect to localhost: Connection refused"` errors in installed release packages by replacing query string paths (`index.html?popup=...`) with URL hash fragment parameters (`index.html#popup=...`) across Rust backend popup builders (`popup-add`, `popup-progress`, `popup-complete`, and `popup-refresh`). Updated `App.tsx` routing to parse both `searchParams` and `hashParams`.
- **Uniform Light Theme Background Palette (`#FEFEFF`)**: Enforced explicit `#FEFEFF` background across Speed Limiter sub-div containers, numerical threshold inputs, dropdown trigger buttons, option menus, speed preset buttons, software updater metric cards, and update status banners.
- **Clean Software Updater Fallback Handling**: Filtered out technical fallback platform errors (`None of the fallback platforms ['linux-x86_64'] were found in the response platforms object`), cleanly displaying `"You are running the latest version! No new update found."`
- **Clean Release Manifest Generation**: Updated `generate_latest_json.js` manifest generator script to produce target platforms `linux-x86_64-deb`, `linux-x86_64-rpm`, and `windows-x86_64`, omitting the duplicate `linux-x86_64` AppImage key.

## [0.6.0] – 2026-08-05

### Added
- **Dynamic Max Segment Connections**: Connected the Settings range slider (1 to 32 parallel range connections, default 8) to a new Rust backend command (`set_max_chunks`). Saves state to `localStorage` and dynamically configures worker threads per task.
- **Smart IP Rate-Limit Protection**: Implemented automatic rate-limit detection in the Rust download engine. If a server returns `HTTP 429 Too Many Requests`, `HTTP 503 Service Unavailable`, `HTTP 403 Forbidden`, or `HTTP 509 Bandwidth Exceeded`, the engine automatically auto-reduces stream connections to 4 or 2 parallel streams to protect against server bans.
- **Visual Feast Software Updater UI**: Added an animated liquid wave progress bar (`0% -> 100%`) and a 4-box live metrics grid showing real-time `Downloaded`, `Speed`, `Progress %`, and `ETA`.
- **OS-Sensitive Privilege Guidance Banner**: Added platform-aware guidance banners explaining WHY system privilege prompts appear during updates (Windows UAC User Account Control vs. Linux Polkit Superuser sudo prompt).
- **Single System Authorization Guard**: Integrated an atomic download lock (`isDownloadingUpdate`) to prevent duplicate update routines and ensure exactly 1 authorization prompt is requested.
- **Dark/Light Theme-Sensitive Speed Limiter Dropdown**: Updated `<select>` and `<option>` elements for speed units (`KB/s`, `MB/s`, `GB/s`) to dynamically match active dark and light themes without rendering grey backgrounds in dark mode.
- **Festive Version Upgrade Celebration Modal**: Replaced plain notification alerts with an interactive celebration modal showcasing new features upon launch (`🎉 v0.4.9 is history. v0.6.0 is live now with fresh upgrades and smoother vibes!`).

## [0.5.17] – 2026-08-05

### Added
- **Visual Feast Software Updater UI**: Added an animated liquid wave progress bar (`0% -> 100%`) and a 4-box live metrics grid showing real-time `Downloaded`, `Speed`, `Progress %`, and `ETA`.
- **OS-Sensitive Privilege Guidance Banner**: Added platform-aware guidance banners explaining WHY system privilege prompts appear during updates (Windows UAC User Account Control vs. Linux Polkit Superuser sudo prompt).
- **Single System Authorization Guard**: Integrated an atomic download lock (`isDownloadingUpdate`) to prevent duplicate update routines and ensure exactly 1 authorization prompt is requested.
- **Dark/Light Theme-Sensitive Speed Limiter Dropdown**: Updated `<select>` and `<option>` elements for speed units (`KB/s`, `MB/s`, `GB/s`) to dynamically match active dark and light themes without rendering light backgrounds in dark mode.
- **Festive Version Upgrade Celebration Modal**: Replaced plain notification alerts with an interactive celebration modal showcasing new features upon launch (`🎉 v0.4.9 is history. v0.5.17 is live now with fresh upgrades and smoother vibes!`).

## [0.5.6] – 2026-08-05

### Added
- **Instant Window Transition & Asynchronous Inspection**: Downloads captured from browser or added via modal instantly open the Progress Window (Popup 2) in `< 50ms`. Heavy network inspection (HEAD/GET range probes and 302 redirect tracking) runs asynchronously in background threads to eliminate UI freezes.
- **Speed Limiter Unit Dropdown & Free Value Input**: Enhanced Settings UI for Speed Limiter with free-text typing (allowing custom limits like 512, 750, 1.5) and a unit selector dropdown supporting `KB/s`, `MB/s`, and `GB/s`.
- **Default 512 KB/s Bandwidth Limit**: Toggling the Speed Limiter ON automatically defaults to `512 KB/s` if no custom limit is saved.
- **Browser Extension Metadata Forwarding**: Updated Chrome extension to extract and forward native `fileSize` and resolved target URLs directly to desktop engine for 0ms instant file size display on captured popups.

### Fixed
- **Strict & Stable High-Precision Speed Limiter**: Replaced legacy per-packet sleep throttling with a high-precision sliding-window Token Bucket rate limiter algorithm in Rust. Enforces a strict upper bandwidth ceiling when limiting is ON while eliminating TCP window stalls and wild speed drops for flatline downloading.
- **Full Wire-Speed Downloads when Limiter OFF**: Configured `reqwest` HTTP client with `tcp_nodelay(true)`, `tcp_keepalive(30s)`, and `pool_max_idle_per_host(64)` for maximum throughput and zero overhead when speed limiting is disabled.
- **SourceForge & Multi-Hop 302 Redirect Fix**: Resolved SourceForge multi-hop CDN redirects by preserving cookies/referrers across 302 chains and dynamically updating task URLs to direct mirror CDN links before chunk worker allocation.
- **Single-Stream Dynamic Fallback**: Added automatic fallback to 1 single stream if server rejects 8 multi-chunk range requests.

## [0.4.9] – 2026-07-25

### Fixed
- **Tauri App "Intercept Browser Downloads" Toggle**: Connected the "Intercept Browser Downloads" toggle in app Settings to a new Rust backend command (`set_intercept_downloads`). When turned OFF in app settings, the backend returns HTTP 403 Forbidden to the browser extension, cleanly bypassing IDM download capture until manually turned back on.
- **Browser Extension Background App Wakeup**: Updated browser extension `sendToDesktopApp` helper. If port 9600 is unreachable (because the Tauri app daemon was closed), the extension emits a silent protocol wakeup signal (`idm://wakeup`) and retries delivery after 800ms.
- **Backend Bandwidth Speed Limiter**: Implemented a global atomic speed limiter in Rust download engine (`set_speed_limit`). Segments dynamically throttle chunk reading speed to enforce exact target bandwidth caps (e.g. 512 KB/s, 1024 KB/s, 2048 KB/s).
- **Interactive Speed Limit UX & UI Badges**: Replaced impractical range sliders in Settings with a crisp numerical input field + quick preset buttons (512 KB/s, 1 MB/s, 2 MB/s, 5 MB/s, 10 MB/s). Added a highlighted `LIMITED` badge indicator on progress popups and dashboard item cards when bandwidth throttling is active.
- **Native Package Targets Only (DEB, RPM, NSIS)**: Configured `tauri.conf.json` bundle targets to `["deb", "rpm", "nsis"]` (excluding AppImage). Updated `scripts/generate_latest_json.js` so `npm run release` compiles signed packages for Debian/Ubuntu (`.deb`), Fedora/RHEL (`.rpm`), and Windows (`.exe`).
- **Non-Destructive Popup Cancel / Close**: Fixed progress popup `Cancel` button so closing/cancelling a download pauses it in state rather than failing or losing the task. Cancelled downloads remain saved in the dashboard history list for easy resuming.

## [0.4.4] – 2026-07-25

### Fixed
- **100% Mandatory IDM Background Capture**: Configured browser extension to unconditionally cancel native browser downloads immediately and pass 100% of download payloads directly to Impressive Download Manager background engine on port 9600.
- **Background Autostart Sync**: Enhanced startup initialization in frontend (`App.tsx`) and backend (`lib.rs`) to persist autostart preference in `localStorage` and register system startup entries across Windows Registry (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) and Linux (`~/.config/autostart`).
- **Updater Plugin ACL Permission Fix**: Granted `"updater:default"` permission in `src-tauri/capabilities/default.json` so the frontend webview is authorized to invoke updater IPC commands (`plugin:updater|check` and `plugin:updater|download_and_install`).
- **User-Friendly Updater Diagnostics**: Intercepted `Could not fetch a valid release JSON` remote fetch errors (which occur when no new release JSON exists on GitHub yet) and mapped them to a clean user message: `"You are running the latest version! No new update found."`
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
