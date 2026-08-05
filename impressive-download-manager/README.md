# Impressive Download Manager (IDM) 🚀 `v0.5.6`

**Impressive Download Manager** is a state-of-the-art, multi-threaded open-source download manager engineered for maximum speed, instant UI responsiveness, and rock-solid reliability. Powered by **Rust**, **Tauri v2**, and **React**, it provides a sleek, modern alternative to classic desktop download managers like IDM and XDM.

---

## ✨ Key Features

### ⚡ Full Wire-Speed Multi-Threaded Engine
- **Parallel Chunk Acceleration**: Automatically splits download files into up to 32 parallel range connections.
- **Zero Thread Lock Contention**: Progress tracking uses hardware atomic counters (`AtomicU64`) and lock-free RAM buffers handed off to a dedicated disk-writer thread.
- **Optimized Socket Tuning**: Configured with `tcp_nodelay(true)` and keep-alive sockets for maximum network throughput without TCP buffer stalls or speed drops.

### ⏱️ Instant Popup Capture (< 50ms)
- **Zero-Delay UI**: Download capture windows (`popup-add` & `popup-progress`) open instantly in under 50ms pre-filled with browser metadata.
- **Asynchronous Inspection**: Heavy network operations (HEAD probes, 302 redirect resolution, and Content-Range inspection) run asynchronously in background `tokio` tasks without freezing the user interface.

### 🎛️ High-Precision Speed Limiter
- **Sliding-Window Rate Control**: Uses a Token Bucket rate limiter algorithm in Rust to strictly enforce bandwidth caps without speed spikes or drops.
- **Flexible UI Controls**: Enter any custom speed limit manually, select units from a dropdown (**`KB/s`**, **`MB/s`**, **`GB/s`**), or choose quick presets (`512 KB/s`, `1 MB/s`, `2 MB/s`, `5 MB/s`, `10 MB/s`).
- **Default Threshold**: Auto-defaults to `512 KB/s` when toggled ON and saves custom settings to local storage.

### 🔗 Direct Mirror CDN & Single-Stream Fallback
- **Multi-Hop 302 Redirect Tracking**: Follows complex redirect chains (SourceForge, Google Drive, file hosts) and resolves direct CDN mirror links before chunk allocation.
- **Dynamic Single-Stream Fallback**: Automatically falls back to a single continuous stream (1 chunk) if a server rejects multi-part range requests.

### 🔄 Safe Auto-Updater with Relaunch Protection
- **Cryptographically Signed Auto-Updates**: In-app secure software updates signed via key pairs and hosted on GitHub Releases.
- **Active Download Protection**: If downloads are currently active, updates wait until all downloads reach `Completed`, `Paused`, or `Failed` status before relaunching automatically.
- **Release Celebration Modal**: Displays an interactive feature showcase modal upon upgrading to a new version.

### 🌐 100% Browser Interception Extension
- **Chrome / Edge Manifest V3 Extension**: Unconditionally captures download links, passing cookies, referrers, file sizes, and URLs directly to the desktop app on local port 9600.
- **Background Daemon Wakeup**: Emits a protocol signal (`idm://wakeup`) if the desktop application is closed to start the manager on click.

---

## 🌐 Browser Extension Setup

To enable automatic download capture in Google Chrome, Microsoft Edge, Brave, or Vivaldi:

1. Open your browser and navigate to `chrome://extensions`.
2. Enable **Developer Mode** in the top-right corner.
3. Click **Load Unpacked**.
4. Select the `browser-extension` folder inside this repository.
5. All browser downloads will now be seamlessly intercepted and handed over to Impressive Download Manager!

---

## 🛠️ Development & Building

### Prerequisites
- **Node.js**: `v20` or later
- **Rust Toolchain**: `1.75` or later (`rustup`)
- **Linux C Libraries** (for Debian/Ubuntu): `libglib2.0-dev`, `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`

### Installation
```bash
# 1. Clone repository
git clone https://github.com/Shaheer-Gujjar1/IDM.git
cd IDM/impressive-download-manager

# 2. Install NPM dependencies
npm install

# 3. Run application in development mode
npm run tauri dev
```

### Production Build
```bash
# Build production frontend and native desktop package (.deb, .rpm, .exe)
npm run build
npx tauri build
```

---

## 📄 License

Distributed under the **MIT License**. Created with ❤️ by the Impressive Download Manager Team.
