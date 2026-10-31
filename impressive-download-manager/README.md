# Impressive Download Manager (IDM) 🚀 `v0.9.0`

**Impressive Download Manager** is an open-source, multi-threaded desktop download engine engineered in **Rust**, **Tauri v2**, and **React 19**. Built for raw wire-speed throughput, instant UI responsiveness, and universal protocol versatility.

---

## 🏗️ Architectural Overview

```
                      +------------------------------------------+
                      |         React 19 + TypeScript UI         |
                      |  (Liquid Glassmorphic Aesthetic Canvas)  |
                      +--------------------+---------------------+
                                           | Tauri v2 IPC
                      +--------------------+---------------------+
                      |           Tauri / Rust Core              |
                      +----+-------------------------------+-----+
                           |                               |
       +-------------------+---------------+   +-----------+-------------------+
       |    Network Stream Workers         |   | Dedicated Disk I/O Worker     |
       |  - Dynamic Range Workers (1..32)  |   |  - Zero-Merge Sparse Offset   |
       |  - Dynamic Work-Stealing Loop     |   |    Direct File Writes         |
       |  - Token Bucket Bandwidth Limiter |   |  - Non-blocking Ring Buffers  |
       |  - TLS & Client Hints Bypass      |   |  - Hardware Atomic Telemetry  |
       +-------------------+---------------+   +-------------------------------+
                           |
       +-------------------+---------------------------------------------------+
       |                   Multi-Protocol & Service Engines                    |
       |  - Native BitTorrent / Magnet Swarming (BEP 3, BEP 15, BEP 23)        |
       |  - Aria2 JSON-RPC 2.0 Server (:6800) for Browser Extensions          |
       |  - MPEG-DASH (.mpd) & HLS (.m3u8) Adaptive Stream Assembly            |
       |  - YouTube Batch Playlists & Multi-Language Subtitles (.srt/.vtt)     |
       |  - Bilibili BV/av Episode Parser & Anti-Hotlinking Stream Pipeline   |
       |  - eDonkey2000 / eMule (ed2k:// & ed2k_pack) with MD4 Chunk Hasher    |
       |  - Pure-Rust FTP & FTPS Engine (RFC 959, 2228, 3659)                 |
       |  - GitHub Releases Fast CDN Mirror Accelerator                        |
       |  - Hugging Face AI Models & Datasets Downloader (with LFS Verifier)   |
       |  - OS Power Manager (Auto-Shutdown, Sleep, Hibernate, App Quit)       |
       |  - Background Clipboard URL Auto-Sniffer & Deduplicator               |
       |  - Site Logins & HTTP Auth Credentials Vault (RFC 7617 Basic Auth)    |
       +-----------------------------------------------------------------------+
```

---

## ⚡ Core Engine Features

### 1. Zero-Merge Sparse Direct Offset Writing
- Unlike legacy download managers that save chunks into separate temporary files (`chunk_0.part`, `chunk_1.part`) and merge them after downloading finishes, IDM uses OS sparse file writing (`write_at_offset` / `pwrite`).
- Each range worker writes directly into its predetermined byte position within the final file (`.part`).
- Eliminates the disk thrashing and waiting time of the file assembly phase. Upon reaching 100%, the file is instantly ready.

### 2. Dynamic Work-Stealing Supervisor
- Monitors the progress of all active chunk workers in real-time.
- If one connection slows down or stalls near the end (the classic 99% bottleneck), idle workers automatically steal and split remaining unwritten blocks down to 256KB chunks.

### 3. Native BitTorrent & Magnet Swarming Engine
- Written in pure Rust without external C torrent libraries.
- Implements **BEP 3** (Peer Wire Protocol with handshake, bitfield, unchoke, piece requests), **BEP 15** (UDP Tracker Protocol), and **BEP 23** (Compact Peer Lists).
- Accepts both `magnet:?xt=urn:btih:...` and `.torrent` files with RFC 3174 SHA-1 piece integrity verification.

### 4. Aria2 JSON-RPC 2.0 Server (`127.0.0.1:6800`)
- Built-in JSON-RPC 2.0 server listening on port 6800.
- Serves as a 100% drop-in backend for popular Aria2 web frontends and browser extensions (AriaNg, BaiduExporter, Camtd, Aria2 Explorer).
- Implements: `aria2.addUri`, `aria2.tellStatus`, `aria2.tellActive`, `aria2.tellWaiting`, `aria2.tellStopped`, `aria2.pause`, `aria2.unpause`, `aria2.remove`, `aria2.getVersion`, `aria2.getGlobalStat`, and `system.multicall`.

### 5. Adaptive Media Streaming Engine
- **MPEG-DASH (`.mpd`)**: Parses XML manifests, resolves `SegmentTemplate`, `SegmentTimeline`, and `SegmentList`, downloads audio and video adaptation streams in parallel, and muxes them into `.mp4` using FFmpeg stream-copy.
- **HLS (`.m3u8`)**: Dynamic playlist crawler supporting live, event, and VOD playlists with keyframe sync.
- **YouTube Batch Playlists & Subtitles**: Scrapes full playlist tracklists, retrieves SubRip (`.srt`) and WebVTT (`.vtt`) subtitles, and performs dual-stream 1080p+ video/audio muxing.
- **Bilibili Multi-Part Parser**: Scrapes multi-part episodes (`p=1, p=2...`), generates batch download tasks, and injects `Referer: https://www.bilibili.com/` across all chunk requests to bypass CDN 403 Forbidden errors.

### 6. Universal Protocol Accelerators
- **eDonkey2000 (`ed2k://`, `ed2k_pack`)**: Pure-Rust RFC 1320 MD4 chunk hasher on 9,728,000-byte blocks with AICH hash checking and collection pack support.
- **FTP & FTPS**: RFC 959, RFC 2228 (Explicit TLS), and RFC 3659 (`REST <offset>`) for parallel segmented FTP transfers.
- **GitHub CDN Accelerator**: Concurrently races global and regional mirrors (`ghproxy.net`, `gh-proxy.com`, `ghps.cc`) with adaptive latency probes to route releases through the fastest mirror.
- **Hugging Face AI Hub**: Downloads models (`.safetensors`, `.gguf`, `.bin`) and datasets (`.parquet`), handles HF API tokens with S3 redirect safety, and verifies Git LFS SHA-256 hashes.

### 7. Automation, Power & Security
- **Power Management**: Configurable Auto-Shutdown, Sleep (Suspend), Hibernate, or App Quit upon queue completion. Features a prominent 30-second cancellation countdown banner with native OS abort execution (`shutdown /a` on Windows, `shutdown -c` on Linux).
- **Background Clipboard Sniffer**: Scans system clipboard for URLs, torrents, magnets, streaming media, and archive formats with intelligent deduplication and customizable toast actions.
- **Site Logins Vault**: Stores and injects RFC 7617 HTTP Basic Authentication headers and private FTP credentials with wildcard domain matching (`*.internal.net`).
- **Integrity Suite**: Real-time hash calculation (MD5, SHA-1, SHA-256) with auto-matching verification badges.

---

## 📂 Project Structure

```
impressive-download-manager/
├── browser-extension/          # Chrome / Edge Manifest V3 companion extension
│   ├── manifest.json
│   ├── background.js           # Native messaging & HTTP interceptor
│   └── youtube-page.js         # In-page media detection overlay
├── src/                        # React 19 + TypeScript Frontend
│   ├── components/             # Liquid canvas, modal dialogs, drawer panels
│   ├── App.tsx                 # Main application dashboard
│   ├── App.css                 # Custom glassmorphic styling & wave keyframes
│   └── main.tsx
└── src-tauri/                  # Rust 2021 Backend
    ├── Cargo.toml
    └── src/
        ├── aria2_rpc.rs        # JSON-RPC 2.0 server (port 6800)
        ├── bilibili.rs         # Bilibili episode scraper & anti-403 pipeline
        ├── clipboard.rs        # Background clipboard URL sniffer
        ├── dash.rs             # MPEG-DASH XML manifest parser & downloader
        ├── ed2k.rs             # eDonkey2000 / eMule protocol & MD4 chunk hasher
        ├── engine.rs           # Core multi-threaded download engine & work-stealer
        ├── ftp.rs              # RFC 959 / 2228 / 3659 FTP & FTPS client
        ├── github_mirror.rs    # GitHub Releases fast CDN mirror accelerator
        ├── hls.rs              # HLS .m3u8 playlist crawler & stream muxer
        ├── huggingface.rs      # Hugging Face AI weights & datasets downloader
        ├── power.rs            # OS Auto-Shutdown / Sleep / Hibernate / Quit
        ├── preflight.rs        # URL probe, duplicate conflict resolution & hash suite
        ├── site_logins.rs      # HTTP Basic auth & private FTP credential vault
        ├── torrent.rs          # Native BitTorrent P2P swarming & magnet engine
        ├── youtube.rs          # YouTube batch playlist & subtitle engine
        └── lib.rs              # Tauri IPC commands registration & daemon lifecycles
```

---

## 🛠️ Development & Building

### Prerequisites
- **Node.js**: `v20.0.0` or later
- **Rust Toolchain**: `1.75` or later (`rustup update stable`)
- **FFmpeg** (optional, recommended for DASH / HLS video/audio muxing)

### Installation
```bash
# 1. Install frontend dependencies
npm install

# 2. Run in development mode (launches Vite dev server and Tauri window)
npm run tauri dev
```

### Running Test Suites
IDM features 129+ automated tests covering protocols, parsing, hash suites, and work-stealing logic:
```bash
cd src-tauri
cargo test
```

### Production Build
```bash
# Build frontend bundle
npm run build

# Package desktop installer for current OS (.exe/.msi on Windows, .deb/.rpm on Linux)
npx tauri build
```

---

## 📄 License

Distributed under the **MIT License**. Created with ❤️ by the Impressive Download Manager Team.
