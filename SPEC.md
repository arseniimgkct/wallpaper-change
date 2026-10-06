# DesktopOverlay — Functional & Technical Specification for Rust Port

## 1. Operating Principle & Core Mission

DesktopOverlay places a picture or a solid color on the Windows desktop **directly behind desktop icons and above the system wallpaper**, without permanently altering or damaging the real Windows wallpaper in domain policies or user settings.

### Key Invariants:
1. **Zero System Wallpaper Tampering on Apply**: Applying an image or solid color NEVER modifies the registry wallpaper or policy values. The system wallpaper remains completely intact.
2. **Corporate & Domain Policy Immunity**: Because the overlay is a child window of the shell hierarchy, it displays seamlessly even on domain-joined machines where GPO locks wallpaper personalization.
3. **Instant, Zero-Trace Reversibility**: Exiting the process or clicking "Revert" cleanly removes the overlay and leaves the original desktop wallpaper untouched.
4. **HKCU Scope Exclusively**: No elevation (`asInvoker`), no Windows service, no driver, no writes to `HKLM`.
5. **No Live Testing on Host Machine**: Do not run live tests that kill `explorer.exe` or modify desktop state during development; verification is done via `cargo check` and build artifacts.

---

## 2. Low-Level Wallpaper Overlay Architecture (Win32 & WorkerW)

### 2.1 Shell Window Hierarchy
```
Progman (Program Manager)
 └─ WorkerW (created by 0x052C)
 └─ WorkerW
     └─ SHELLDLL_DefView
         └─ SysListView32 (Desktop Icons)
 └─ WorkerW [TARGET HOST]
     └─ DesktopOverlaySurface (Our WS_CHILD overlay window)
```

### 2.2 Host Window Discovery (`FindWallpaperWorkerW`)
1. Call `FindWindowW(w!("Progman"), None)`. If not found, fallback to `EnumWindows` scanning for class `"Progman"`.
2. Send message `0x052C` (`MSG_SPLIT_WORKERW`) to `Progman` with a 1000 ms timeout via `SendMessageTimeoutW`. This forces Explorer/DWM to split the `WorkerW` hierarchy.
3. Enumerate all top-level windows using `EnumWindows` and collect all windows with class `"WorkerW"` in top-down Z-order.
4. Locate the `WorkerW` that owns a child window with class `"SHELLDLL_DefView"`.
5. The **Target Host** is the immediate *next* `WorkerW` in the enumerated list (i.e. `workers[icons_index + 1]`).
6. Fallback 1: If no subsequent `WorkerW` exists, fall back to `Progman` itself.
7. Fallback 2: If `SHELLDLL_DefView` is directly inside `Progman`, take the first `WorkerW`.
8. Fallback 3: If no valid host can be found, return `None` (attach fails safely).

### 2.3 Window Class & Styles
- **Class Name**: `DesktopOverlaySurface`
- **Class Styles**: `CS_HREDRAW | CS_VREDRAW`, `hbrBackground = NULL`, `hCursor = LoadCursor(IDC_ARROW)`.
- **Window Style**: `WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS`.
- **Extended Window Style**: `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT` (unless `OVERLAY_NO_EX_TRANSPARENT=1` is set).
- **Z-Order**: `HWND_BOTTOM` (`SetWindowPos` with `SWP_NOACTIVATE | SWP_SHOWWINDOW`).
- **Input Routing**:
  - `WM_NCHITTEST` returns `HTTRANSPARENT` (`-1`) so all mouse clicks pass through to icons or desktop context menu.
  - `WM_ERASEBKGND` returns `1` (prevents flicker).

### 2.4 Layout & Multi-Monitor Policy
- **Primary Screen Only**: Screen bounds retrieved via `GetSystemMetrics(SM_CXSCREEN)` and `GetSystemMetrics(SM_CYSCREEN)`.
- **Coordinate Mapping**: Screen `(0, 0)` is converted to host client coordinates via `MapWindowPoints(HWND(null), host_hwnd, &mut pt)`.
- Window is positioned at `(pt.x, pt.y, screen_w, screen_h)`.
- `WM_DISPLAYCHANGE` triggers re-layout and forced repaint.

### 2.5 High-Performance Cover Scaling & Double-Buffered GDI Painting
- `WM_PAINT` must strictly use `BeginPaint` / `EndPaint` pairing (mismatched `GetDC` causes severe DWM surface damage and blinking).
- **Cover Crop Algorithm**:
  $$\text{scale} = \max\left(\frac{\text{client\_w}}{\text{image\_w}}, \frac{\text{client\_h}}{\text{image\_h}}\right)$$
  $$\text{target\_w} = \text{round}(\text{image\_w} \times \text{scale}), \quad \text{target\_h} = \text{round}(\text{image\_h} \times \text{scale})$$
  $$\text{offset\_x} = \max\left(0, \frac{\text{target\_w} - \text{client\_w}}{2}\right), \quad \text{offset\_y} = \max\left(0, \frac{\text{target\_h} - \text{client\_h}}{2}\right)$$
- **Cache**: Resampled 32-bit BGRA bitmap is cached for current `(width, height)`. Repaints at the same resolution perform an instantaneous `SetDIBitsToDevice` blit without resampling (0% CPU usage).
- **Debug Variables**:
  - `OVERLAY_DEBUG_FILL=1`: Paints solid Magenta (`#FF00FF`) to verify DWM child window composition.
  - `OVERLAY_NO_EX_TRANSPARENT=1`: Removes click-through behavior for hit-testing diagnostics.

### 2.6 Watchdog & Explorer Recovery
- A background watchdog thread checks overlay status every 2000 ms.
- If `IsWindow(overlay_hwnd)` is false or parent handle changed (e.g. `explorer.exe` restarted or crashed), the engine logs `пересоздание оверлея: окно потеряно (перезапуск Explorer?)` and automatically re-attaches the overlay.
- A 1000 ms periodic `InvalidateRect` + `UpdateWindow` ensures the overlay remains visible if the shell repaints the desktop background.

### 2.7 Clean Removal Sequence (Zero Residue)
1. Repaint the overlay window with the genuine system wallpaper (`TranscodedWallpaper` from `%APPDATA%\Microsoft\Windows\Themes\` or registry policy).
2. Sleep 150 ms to allow GDI / DWM frame presentation to land on screen.
3. Call `DestroyWindow(overlay_hwnd)`.
4. Call `RedrawWindow` on `Progman` and all `WorkerW` instances with `RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_FRAME`.

---

## 3. Features & User Actions

### 3.1 Image Selection
- **Formats**: `.jpg`, `.jpeg`, `.png`, `.bmp`, `.gif`, `.webp`, `.tif`, `.tiff`.
- **File Dialog**: Standard file picker with image filters + All Files.
- **Drag & Drop**: Supports dropping an image file or a directory (takes first supported image inside the directory, sorted case-insensitively).
- **Clipboard (`Ctrl+V` / Button)**:
  - Priority: `CF_HDROP` (copied Explorer files/folders) $\to$ File path text $\to$ Direct bitmap data.
  - Retry policy: 12 attempts with 80 ms delays to handle clipboard lock contention.
  - **Immediate Apply**: Clipboard paste applies the wallpaper immediately without requiring a second click.
- **Stable Copy**: Applied images are saved to `%APPDATA%\DesktopOverlay\wallpaper.<ext>` so wallpapers survive deletion from Downloads.

### 3.2 Solid Colors
- 6 Default Presets:
  1. Чёрный (`#000000` / `0,0,0`)
  2. Тёмный графит (`#121212` / `18,18,18`)
  3. Тёмно-синий (`#1C2128` / `28,33,40`)
  4. Сланцевый (`#252A34` / `37,42,52`)
  5. Серый (`#323232` / `50,50,50`)
  6. Белый (`#F5F5F5` / `245,245,245`)
- Custom Palette: Native `ChooseColorW` dialog (`CC_RGBINIT | CC_FULLOPEN | CC_ANYCOLOR`).
- Renders to `%APPDATA%\DesktopOverlay\wallpaper.png` at $\max(1920, \text{screen\_w}) \times \max(1080, \text{screen\_h})$.

### 3.3 Windows Theme Toggle
- Toggles `AppsUseLightTheme` and `SystemUsesLightTheme` in `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`.
- Button label reflects target state: `🌙 Сменить тему Windows на тёмную` or `☀ Сменить тему Windows на светлую`.
- Restarts Explorer only if the registry value actually changed.

### 3.4 Taskbar Size Toggle
- Toggles `TaskbarSmallIcons` in `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced`.
- **Windows 11 Guard**: On build $\ge 22000$, button is permanently disabled with notice *"В Windows 11 настройка удалена Microsoft"*.
- On Windows 10: Toggles between standard (40px) and small (30px), restarts Explorer.

### 3.5 Microsoft Edge & Default Browser Management (`BrowserUtil` & `TaskbarPins`)
- **Unpin Edge**: Uses shell folder verbs `taskbarunpin` / `открепить от панели задач` with fallback deletion of shortcut in `%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar`.
- **Browser Alternatives**: Detects installed Chrome and Firefox.
- **Make Default**: Firefox is launched with `-setDefaultBrowser`; Chrome/Edge open `chrome://settings/defaultBrowser`.
- **Button State**: Displays current default browser (reads `UrlAssociations\http\UserChoice\ProgId`).

### 3.6 Autostart
- Registry: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` $\to$ `DesktopOverlay` = `"<exe_path>" --apply "<saved_wallpaper_path>"`.
- Refuses activation if no saved wallpaper exists yet.

### 3.7 Revert Contract (`RevertAll`)
Reverts all system modifications with a comma-separated audit report:
1. Remove overlay window (`оверлей снят`).
2. Disable autostart (`автозапуск выключен`).
3. Clear HKCU wallpaper override if not domain GPO (`\\...`) (`системные обои возвращены`).
4. Restore normal taskbar size if small (`обычный размер панели задач восстановлен`).
5. Restore light theme if explicitly requested (`светлая тема восстановлена`).
6. Restore Microsoft Edge to taskbar and default browser if changed (`Edge возвращён на панель задач и как браузер по умолчанию`).
7. Delete saved image copy (`копия картинки удалена`).
8. Redraw desktop.

---

## 4. CLI, Single Instance & IPC

- **Named Mutex**: `Local\DesktopOverlay.SingleInstance`.
- **Second Instance Behavior**:
  - `DesktopOverlay.exe --revert` $\to$ Sends IPC `REVERT` $\to$ exits.
  - `DesktopOverlay.exe --set "<path>"` $\to$ Sends IPC `SET <path>` $\to$ exits.
  - `DesktopOverlay.exe` $\to$ Sends IPC `SHOW` (brings existing window to front) $\to$ exits.
- **First Instance Modes**:
  - `DesktopOverlay.exe`: Opens GUI utility window.
  - `DesktopOverlay.exe --apply "<path>"`: Headless tray-only mode (no main window created).
  - `DesktopOverlay.exe --set "<path>"`: Opens GUI, preselects image, applies immediately.
  - `DesktopOverlay.exe --revert [--light]`: Headless rollback and exit.

---

## 5. System Tray & Window Lifecycle

- **Window Close (X)**: Hides window to system tray.
- **Tray Menu**:
  1. **Показать окно утилиты** (Bold / Primary action)
  2. **Переприкрепить обои** (Force re-attach)
  3. **Откатить обои** (Confirmation modal $\to$ Revert)
  4. *(separator)*
  5. **Выход** (Clean removal $\to$ Process termination)

---

## 6. Diagnostics & Logging

- Path: `%APPDATA%\DesktopOverlay\overlay.log`.
- Format: `yyyy-MM-dd HH:mm:ss <message>`.
- **Log Volume Discipline**: Logs the first 8 paint events (`[paint #N]`), then stops paint logging to prevent log flooding.

---

## 7. Rust Tech Stack

| Component | Library / Approach |
|---|---|
| Language | Rust 2021 Edition (`rustc >= 1.80`) |
| GUI Framework | `eframe` / `egui` (0.31) with clean dark theme |
| Win32 Interop | `windows` (0.58) GDI, User32, Registry, Shell |
| Image Processing | `image` (0.25) PNG, JPEG, BMP, GIF, WebP, TIFF |
| Clipboard & Dialogs | `arboard` (3.4), `rfd` (0.15), native `ChooseColorW` |
| System Tray | `tray-icon` (0.19) |
| Manifest & Metadata | `winres` (0.1) with `asInvoker`, Win10/11 compatibility list, multi-resolution `app.ico` |
| Distribution | Single standalone `.exe` (~11 MB unstripped, ~3 MB optimized) |
