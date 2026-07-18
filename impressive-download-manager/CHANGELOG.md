# CHANGELOG

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

### Changed
- **Popup window title** renamed from **“New Download”** to **“New Download Captured”** (both the Tauri window title and the in‑popup header).
- **Removed duplicated header** inside the captured‑download popup – now only the native window title is shown.
- **Popup height** reduced to `370 px` to fit the new layout without scrolling.
- **Liquid‑fill animation handling**:
  - Paused & completed states now only pause the animation (no background‑color overrides), preserving the file‑type color.
  - Completed state adds a subtle opacity reduction for a polished finish.
- **Added `popupSize` React state** to carry the estimated size from the backend to the UI.
- **Added `executables` entry** in `getFileCategory` (App.tsx).

### Fixed
- **Light‑mode footer**: colors now use theme variables, making the footer visible in light mode.
- **Download progress**: completed items always display **100 %** even when size is unknown.
- **Header removal bug**: old “New Download Captured” title block inside the modal is gone.

### Build & Packaging
- Updated build scripts to produce `.deb`, `.rpm`, and `.AppImage` bundles for version `0.1.2`.
- Verified successful packaging and installation on Debian/Ubuntu.

---

## Previous Versions (summary)

- **0.1.1** – Fixed size detection, added wave colors for videos/audio/documents/archives, improved footer visibility, adjusted popup sizing, corrected progress‑snap behavior.
- **0.1.0** – Initial release with core download manager functionality.

---
