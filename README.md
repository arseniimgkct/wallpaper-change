# DesktopOverlay (Rust)

DesktopOverlay places a picture or a solid color on the Windows desktop **directly behind desktop icons and above the system wallpaper**, without permanently altering or damaging the real Windows wallpaper in domain policies or user settings.

## Features & Architecture

- **WorkerW Layer Injection**: Discovers `Progman` and splits the WorkerW window hierarchy using Win32 message `0x052C`, hosting a `WS_CHILD` overlay surface behind desktop icons.
- **Zero-Tampering**: Does not overwrite system wallpaper files or registry settings upon apply.
- **Instant Reversibility (`--revert`)**: Restores original wallpaper presentation and destroys overlay cleanly.
- **High-Performance Resampling & Caching**: Resamples source images with cover scaling and caches 32-bit BGRA bitmaps for instant GDI blitting (`0% CPU` overhead on subsequent paints).
- **GUI & Headless Modes**:
  - `DesktopOverlay.exe`: Interactive dark-themed utility (eframe / egui 0.31).
  - `DesktopOverlay.exe --apply <path>`: Headless tray mode for autostart.
  - `DesktopOverlay.exe --set <path>`: Preselects and applies image in GUI.
  - `DesktopOverlay.exe --revert [--light]`: Headless rollback.
- **System Tray**: Hides to tray on close, provides quick access to show window, reattach, revert, and exit.
- **Single Instance & IPC**: Named Mutex `Local\DesktopOverlay.SingleInstance` + Named Pipe `\\.\pipe\DesktopOverlay.IPC`.

## Building

```powershell
cargo build --release
```

The output standalone binary is located at `target/release/DesktopOverlay.exe`.
