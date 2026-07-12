# Impressive Download Manager (IDM) 🚀

Impressive Download Manager is a blazing-fast, multi-threaded desktop download accelerator built with **Rust (Tauri)** and **React (TypeScript)**. It features a state-of-the-art glassmorphic Command Center UI, real-time download calculations, link-refresh systems, and seamless Chrome integration.

---

## 🌟 Key Features

- **Blazing-Fast Multi-Threaded Engine:** Bypasses OS disk lock contention by running parallel downloads in RAM and writing sequential 512KB blocks using a single master writer thread.
- **Glassmorphic Command Center UI:** Features a detached floating navigation sidebar, spotlight search, and bento-grid progress trackers.
- **Real-Time Progress Tracking:** Circular fluid indicator and active download segment visualizers showing exactly which chunks are downloading.
- **Link Refresh System:** Automatically opens the source page in your browser if a download expires, re-captures the URL dynamically from Chrome, and resumes the task without losing progress.
- **Chrome / Edge Extension:** Seamlessly intercepts click downloads from your browser and sends them to the desktop engine via a secure localhost integration.

---

## 🛠️ Development Setup

Ensure you have [Node.js](https://nodejs.org/) (v18+) and [Rust](https://www.rust-lang.org/) installed.

### 1. Install Dependencies
```bash
npm install
```

### 2. Run in Development Mode
```bash
npm run tauri dev
```

---

## 📦 Packaging & Distribution

This application is ready for cross-platform builds. The packaging target formats are configured in `src-tauri/tauri.conf.json`.

### 1. Build for Linux (Debian / Ubuntu `.deb` & `.AppImage`)

On Debian-based systems (Ubuntu, Linux Mint, Debian), install the packaging dependencies:
```bash
sudo apt-get update
sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.1-dev build-essential curl wget file libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

Run the production build command:
```bash
npm run tauri build
```

- **Output Path:** `src-tauri/target/release/bundle/deb/impressive-download-manager_0.1.0_amd64.deb`
- **Output AppImage:** `src-tauri/target/release/bundle/appimage/impressive-download-manager_0.1.0_amd64.AppImage`

Install the generated `.deb` package:
```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/impressive-download-manager_0.1.0_amd64.deb
```

---

### 2. Build for Windows (`.exe` NSIS or `.msi` WiX Installers)

To cross-compile or compile directly on Windows for distribution:

#### Option A: NSIS Installer (Recommended `.exe`)
NSIS creates a lightweight, modern executable installer.
1. Download and install [NSIS](https://nsis.sourceforge.io/Download).
2. Open PowerShell or Command Prompt in the repository root and run:
   ```powershell
   npm run tauri build -- --target nsis
   ```
- **Output Path:** `src-tauri\target\release\bundle\nsis\impressive-download-manager_0.1.0_x64-setup.exe`

#### Option B: WiX Toolset Installer (MSI)
WiX generates a standard Windows Installer package.
1. Install [WiX Toolset v3](https://wixtoolset.org/releases/v3-11-2-rtm/) (requires .NET Framework 3.5.1).
2. Add WiX binaries to your Windows System PATH.
3. Run the Tauri build command:
   ```powershell
   npm run tauri build
   ```
- **Output Path:** `src-tauri\target\release\bundle\msi\impressive-download-manager_0.1.0_x64_en-US.msi`

---

## 🔌 Browser Extension Installation

To integrate Chrome/Edge downloads directly with IDM:

1. Open your browser and navigate to `chrome://extensions/`.
2. Enable **Developer mode** (toggle in the top-right corner).
3. Click **Load unpacked** in the top-left.
4. Select the `browser-extension` folder located inside this repository.
5. The extension will now automatically intercept file clicks and redirect download jobs to your desktop app on port `9600`.

---

## 🤝 Credits

- **A Product of:** [Lumen Lab]
- **Designed & Developed by:** [Shaheer Ahmed]
