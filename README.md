# Impressive Download Manager (IDM) 🚀

<p align="center">
  <img src="./impressive-download-manager/src-tauri/icons/128x128.png" alt="IDM Logo" width="100" height="100" />
</p>

<p align="center">
  <b>A blazing-fast, modern, multi-threaded open-source download manager engineered with Rust and Tauri v2.</b><br />
  Featuring zero-merge sparse offset writing, native BitTorrent P2P swarming, Aria2 JSON-RPC server, adaptive streaming (DASH/HLS/YouTube/Bilibili), universal protocol accelerators, and a fluid liquid glassmorphic UI.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Version-0.9.0-6366f1?style=for-the-badge&logo=rocket" alt="Version 0.9.0" />
  <img src="https://img.shields.io/badge/Tauri-v2-24c8db?style=for-the-badge&logo=tauri" alt="Tauri v2" />
  <img src="https://img.shields.io/badge/Rust-2021_Edition-dea584?style=for-the-badge&logo=rust" alt="Rust 2021" />
  <img src="https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react" alt="React 19" />
  <img src="https://img.shields.io/badge/License-MIT-green?style=for-the-badge" alt="License MIT" />
</p>

---

## 🌟 Why Impressive Download Manager?

Most legacy download managers suffer from file assembly bottlenecks (downloading separate chunk files and then taking minutes to merge them), high CPU/RAM overhead, intrusive ads, or broken integrations with modern web streams. 

**Impressive Download Manager** is built from the ground up in **Rust** and **Tauri v2** to maximize throughput at the network interface layer while maintaining a near-zero resource footprint and a state-of-the-art liquid aesthetic.

---

## ⚡ Supercharged Feature Matrix

### 🚀 1. Wire-Speed Multi-Threaded Engine & Zero-Merge I/O
- **Zero-Merge Sparse Direct Offset Writing (`pwrite`)**: Segments write directly into their pre-allocated target byte offsets in parallel. Eliminates the slow post-download chunk-merging phase and avoids doubling disk space usage. Downloads are ready instantly upon reaching 100%.
- **Dynamic Work-Stealing Supervisor**: Automatically monitors worker speeds and splits lagging tail segments down to 256KB across idle workers, completely eliminating the notorious 99% download freeze.
- **Hardware-Level Atomic Telemetry**: Thread-safe progress and real-time speed calculation using hardware `AtomicU64` registers and non-blocking ring buffers.
- **Nanosecond Token Bucket Bandwidth Limiter**: Precise sliding-window rate limiter preventing speed spikes or TCP bufferbloat. Configurable in `KB/s`, `MB/s`, and `GB/s`.
- **Socket Level Tuning**: Enabled `TCP_NODELAY` and HTTP keep-alive pooling to eliminate TCP window stalls.

### 🧲 2. Native BitTorrent & Magnet P2P Swarming
- **Pure-Rust P2P Engine**: Native implementation of BEP 3 (Peer Wire Protocol), BEP 15 (UDP Tracker Protocol), and BEP 23 (Compact Peer Lists).
- **Magnet & Torrent Support**: Accepts `magnet:?xt=urn:btih:...` links as well as raw `.torrent` files.
- **Piece Verification**: Hardware-accelerated RFC 3174 SHA-1 hash validation per piece with direct sparse disk persistence.

### 🌐 3. Aria2 JSON-RPC 2.0 Server (`127.0.0.1:6800`)
- **Universal Browser Extension Compatibility**: Built-in JSON-RPC 2.0 daemon running on port `6800`.
- **Drop-in Aria2 Replacement**: Works seamlessly out of the box with popular browser extensions and CLI tools like **AriaNg**, **BaiduExporter**, **Camtd**, and **Aria2 Explorer**.
- **Supported Methods**: `aria2.addUri`, `aria2.tellStatus`, `aria2.tellActive`, `aria2.tellWaiting`, `aria2.tellStopped`, `aria2.pause`, `aria2.unpause`, `aria2.remove`, `aria2.getVersion`, `aria2.getGlobalStat`, and `system.multicall`.

### 🎬 4. Adaptive Media & Streaming Suite
- **MPEG-DASH (`.mpd`)**: Full XML manifest parser supporting `SegmentTemplate`, `SegmentTimeline`, and `SegmentList`. Auto-pairs highest bandwidth audio/video tracks and muxes losslessly via FFmpeg stream-copy.
- **HLS (`.m3u8`)**: Dynamic playlist crawler with support for master playlists, keyframe alignment, and live/VOD stream captures.
- **YouTube Batch Playlists & Subtitles**: Fetches complete YouTube playlist tracklists for one-click batch queueing. Extracts and downloads SubRip (`.srt`) and WebVTT (`.vtt`) subtitles in any language.
- **Bilibili Dedicated Engine**: Resolves `BV...` and `av...` episodes, scrapes multi-part series (`p=1, p=2...`), and automatically injects anti-hotlinking referrers (`Referer: https://www.bilibili.com/`) across all streams.

### 📦 5. Universal Protocol Accelerators
- **eDonkey2000 / eMule (`ed2k://`, `ed2k_pack`)**: Pure-Rust RFC 1320 MD4 chunk hasher (9,728,000-byte blocks), AICH root hash verification, and batch pack extraction.
- **FTP & FTPS Protocol Suite**: Native async FTP engine implementing RFC 959, RFC 2228 (Explicit TLS), and RFC 3659 (`REST <offset>` for multi-connection segmented downloads).
- **GitHub Releases Accelerator**: Races fast regional CDN mirrors (`ghproxy.net`, `gh-proxy.com`, `ghps.cc`) with adaptive timeout probes to dramatically speed up release asset downloads.
- **Hugging Face AI Hub Accelerator**: Auto-detects model checkpoints (`.safetensors`, `.gguf`, `.onnx`, `.bin`) and datasets (`.parquet`), injects Hugging Face API tokens with S3 redirect protection, and verifies Git LFS SHA-256 hashes automatically.

### 🛡️ 6. System Automation, Power & Security
- **Power Management (Auto-Shutdown / Sleep / Hibernate / Quit)**: Automatically powers down, suspends, hibernates the PC, or closes IDM when all active downloads complete. Features a 30-second cancellation countdown banner with native OS abort commands (`shutdown /a` on Windows, `shutdown -c` on Linux).
- **Background Clipboard URL Auto-Sniffer**: Continuously monitors the system clipboard for download links, magnets, torrents, YouTube/Bilibili videos, and archives with duplicate suppression and customizable toast actions.
- **Site Logins & HTTP Auth Vault**: Centralized credential manager supporting RFC 7617 HTTP Basic Auth and private FTP logins with wildcard subdomain matching (`*.corp.com`).
- **Auto Hash & Checksum Verification**: One-click verification suite supporting MD5, SHA-1, and SHA-256 with instant visual integrity badges.
- **Interactive Duplicate Name Conflict Dialog**: Real-time pre-flight collision detection with choices: *Auto-Rename / Keep Both*, *Overwrite File*, or *Resume Download*.
- **Anti-Bot TLS Fingerprinting & WAF 403 Bypass**: Spoofs modern browser client hints (`sec-ch-ua`), handles Sec-Fetch headers, and automatically retries with adjusted referrers on HTTP 403 Forbidden.

### 🎨 7. Modern Liquid Glassmorphic Aesthetic
- **Fluid Wave Progress Visualizer**: Multi-layered sine/cosine harmonic wave canvas with rising micro-bubbles representing dynamic stream throughput.
- **Category Router**: Auto-sorts files into `Video`, `Audio`, `Compressed`, `Documents`, `Programs`, and `Other` with custom directory persistence.
- **Theme Engine**: Synchronized Dark, Light, and System themes across main application, pre-flight dialogs, and progress popups.

---

## 📥 Installation

Ready-to-use binaries for **Windows** and **Linux** are available in the **[Releases Section](https://github.com/Shaheer-Gujjar1/IDM/releases)**!

| Platform | Format | Description |
| :--- | :--- | :--- |
| **Windows** | `.exe` / `.msi` | Standard NSIS installer or enterprise MSI package |
| **Linux (Debian/Ubuntu)** | `.deb` | Native Debian package (`sudo dpkg -i ...`) |
| **Linux (Fedora/RHEL)** | `.rpm` | Red Hat package (`sudo rpm -i ...`) |
| **Linux (Universal)** | `.AppImage` | Standalone portable executable (`chmod +x ...`) |

---

## 🔌 Browser Extension Integration

IDM includes a companion **Manifest V3** browser extension for seamless one-click link and video sniffing.

1. Open your browser and navigate to the extensions page:
   - **Chrome / Brave / Edge**: `chrome://extensions`
   - **Firefox**: `about:debugging#/runtime/this-firefox`
2. Enable **Developer Mode** (toggle in the top-right corner).
3. Click **Load Unpacked**.
4. Select the `browser-extension` folder located inside the IDM repository or release archive.
5. All browser downloads and supported media streams will now be intercepted by IDM!

> [!TIP]
> You can also connect any extension supporting **Aria2** (like *AriaNg* or *Aria2 Explorer*) directly to `http://127.0.0.1:6800/jsonrpc`!

---

## 🛠️ Building from Source

### Prerequisites
- **Node.js**: `v20.x` or later
- **Rust Toolchain**: `1.75` or later (`rustup`)
- **System Libraries (Linux)**:
  ```bash
  sudo apt-get update && sudo apt-get install -y \
    libwebkit2gtk-4.1-dev \
    build-essential \
    curl \
    wget \
    file \
    libssl-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev
  ```

### Development
```bash
# 1. Clone the repository
git clone https://github.com/Shaheer-Gujjar1/IDM.git
cd IDM/impressive-download-manager

# 2. Install frontend dependencies
npm install

# 3. Launch in development mode
npm run tauri dev
```

### Running Backend Tests
IDM maintains an extensive unit and integration test suite (129+ automated tests):
```bash
cd impressive-download-manager/src-tauri
cargo test
```

### Production Build
```bash
cd impressive-download-manager
npm run build
npx tauri build
```
Built binaries will be generated inside `src-tauri/target/release/bundle/`.

---

## 📄 License

This project is licensed under the **MIT License** — see the [LICENSE](LICENSE) file for details.
