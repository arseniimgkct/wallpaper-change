<div align="center">

# DesktopOverlay

### A wallpaper layer that lives above the system wallpaper — and below everything else.

<p align="center">
  <img src="https://img.shields.io/badge/.NET-8.0-512BD4?style=flat-square&labelColor=0F1419" alt=".NET 8.0" />
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D4?style=flat-square&labelColor=0F1419" alt="Windows 10 | 11" />
  <img src="https://img.shields.io/badge/architecture-x64-D4D4D4?style=flat-square&labelColor=0F1419" alt="x64" />
  <img src="https://img.shields.io/badge/distribution-single--file%20EXE-2D8C3F?style=flat-square&labelColor=0F1419" alt="Single file EXE" />
  <img src="https://img.shields.io/badge/deps-none-6E7681?style=flat-square&labelColor=0F1419" alt="No dependencies" />
  <img src="https://img.shields.io/badge/license-MIT-C08B37?style=flat-square&labelColor=0F1419" alt="MIT" />
</p>

<p align="center">
  <b>English</b> &nbsp;&middot;&nbsp; <a href="README_RU.md">Русский</a>
</p>

</div>

---

## The short version

DesktopOverlay places a single image — or a single solid colour — directly on top of the wallpaper Windows itself draws, while staying strictly **under** the desktop icons and the taskbar. It changes nothing in `HKLM`, breaks no Group Policy, and leaves the machine exactly as it found it. One click removes it and hands the desktop back untouched.

Built for the machines where the wallpaper is locked down by domain policy, where *Settings → Personalization → Background* is greyed out, does nothing, or silently reverts. This is the way to get the desktop you want on a computer you do not own.

<p align="center"><sub>Native Win32 interop. No wallpaper engine, no redistributable, no installer, no services, no scheduled tasks, no admin rights.</sub></p>

---

## Why it exists

Corporate desktops usually ship with a locked wallpaper. The Group Policy setting wins over everything the user interface offers: the background picker is disabled, the preview lies, and any attempt to change the picture either does nothing or is reverted within seconds. There is no supported way to override this from the Settings app.

DesktopOverlay takes a different route. Instead of fighting the shell, it draws a child window **inside the desktop host window**, on the layer that sits between the shell's wallpaper and the icon layer. The shell keeps ownership of the real wallpaper; the overlay simply appears above it. Nothing is registered, nothing is impersonated, and no policy is violated — the picture is a guest at the shell's own table.

## What it does

| | |
|---|---|
| **Image overlay** | Any `JPG`, `JPEG`, `PNG`, `BMP`, `GIF`, `TIF` or `TIFF`. Scaled in *cover* mode: fills the screen completely, preserves aspect ratio, crops the overflow. |
| **Solid colour overlay** | Six built-in presets plus a full colour picker. Rendered to a full-resolution bitmap and treated exactly like an image. |
| **Drag and drop** | Drop a file onto the window. Drop a folder and the first supported image inside it is picked up automatically. |
| **Below everything** | Desktop icons, selection rectangles, right-click menus, taskbar, system tray — all of it stays on top and fully interactive. The overlay never takes focus and never swallows a click. |
| **Survives Explorer** | A watchdog re-attaches the surface within two seconds of an Explorer restart, and the layout re-flows on resolution, display and DPI changes. |
| **No flicker** | The shell repaints its own wallpaper periodically and erases the foreign surface without sending `WM_PAINT`. A one-second repaint cycle, backed by a pre-scaled bitmap and a single blit, keeps the image rock steady at effectively zero cost. |
| **Clean removal** | Before the window is destroyed it repaints itself with the genuine system wallpaper, so not a single pixel of the overlay is left behind. |
| **Light and dark** | The Windows theme can be flipped from the same window, with an automatic Explorer restart. |
| **Autostart** | Optional. Restores the last image at logon, without showing a window. |
| **Revert** | One action, and a written report of exactly what was restored. |

## Requirements

```
Operating system   Windows 10 or Windows 11, x64
Runtime            .NET 8 Desktop Runtime, or the bundled self-contained build
Privileges         None. Everything happens in the current user's own session.
```

## Building

```powershell
git clone <repository-url>
cd DesktopOverlay
dotnet publish -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true
```

The result is a single self-contained executable. Nothing to install, nothing to copy alongside it.

```powershell
# run
.\bin\Release\net8.0-windows\win-x64\publish\DesktopOverlay.exe
```

## Using it

1. Launch `DesktopOverlay.exe`. The window appears centred.
2. Drop an image onto the preview area, or press **Browse**, or pick one of the six colour presets, or open the full palette.
3. Press **Set wallpaper**. The overlay attaches to the desktop and a tray icon appears.
4. Close the window — it minimises to the tray, the overlay stays.
5. Tick **Start with Windows** if the image should survive a reboot.

Closing the window never terminates the application. The tray icon is the anchor, and the overlay is only as alive as the tray.

<p align="center"><sub>Tray: show window &nbsp;&middot;&nbsp; re-attach wallpaper &nbsp;&middot;&nbsp; revert wallpaper &nbsp;&middot;&nbsp; exit</sub></p>

## Command line

| Argument | Effect |
|---|---|
| *(none)* | Opens the utility window. |
| `--set "<path>"` | Opens the window with the image already selected and applies it immediately. |
| `--apply "<path>"` | Headless mode. No window: raises the overlay with the given image and lives in the tray. Used by autostart. |
| `--revert` | Reverts everything without opening a window. |
| `--revert --light` | Reverts everything and additionally restores the light Windows theme. |

A named mutex guarantees a single instance: a second launch exits quietly instead of fighting the first one over the desktop.

## Environment variables

| Variable | Value | Meaning |
|---|---|---|
| `OVERLAY_DEBUG_FILL` | `1` | Fills the surface with magenta. Isolates windowing problems from painting problems. |
| `OVERLAY_NO_EX_TRANSPARENT` | `1` | Drops `WS_EX_TRANSPARENT` and relies on `WM_NCHITTEST → HTTRANSPARENT` for click pass-through. Workaround for builds where DWM drops a transparent overlay from composition. |

## How it works

The desktop is not one window. It is a small, undocumented hierarchy of them, and this project targets that hierarchy directly.

```
Progman
 ├── WorkerW          ← the shell's own wallpaper lives here
 ├── WorkerW
 │    └── SHELLDLL_DefView
 │         └── SysListView32    ← desktop icons
 └── WorkerW          ← the overlay attaches HERE: below the icons, above nothing else
```

The window is created **as a child** of the lower `WorkerW` from the very first call, with `WS_CHILD`, and is then pushed to the bottom of the Z-order. It is never shown top-level and re-parented, because that path fails silently on some Explorer builds.

Four details do most of the work:

- **The correct `WorkerW`.** The icon layer is not the target. The overlay is attached to the sibling host *below* the one containing `SHELLDLL_DefView`. The icon layer is transparent, so the image shows through it and the icons need no redrawing at all. Attaching to the wrong host — the one that owns the icons — puts the `SysListView32` background on top of the image, which is the single most common failure in DIY wallpaper overlays.
- **`BeginPaint` only.** DWM composes exclusively what was drawn inside the `WM_PAINT` context. Drawing through a `GetDC` obtained after `EndPaint` does not appear on screen at all. The whole surface is invalidated before each repaint, so a partial update region can never leave unpainted bands.
- **An active repaint loop.** The shell periodically repaints its own wallpaper over the desktop and erases the foreign surface without ever sending `WM_PAINT`. The image flickers and vanishes. A one-second timer redraws it, exactly the way live wallpapers push continuous frames.
- **A self-cleaning exit.** After `DestroyWindow`, the shell does not repaint the region the window occupied, and burnt-in pixels survive until Explorer restarts. `RedrawWindow` over `Progman` and every `WorkerW` does not reliably help. The surface therefore repaints itself with the real system wallpaper — read from `TranscodedWallpaper`, or from the domain policy path — while it is still alive, and only then closes.

## Project layout

| File | Responsibility |
|---|---|
| `Program.cs` | Entry point, single-instance mutex, command-line dispatch, headless tray context, log, stable image copy into `%APPDATA%`. |
| `MainForm.cs` | The user interface: drag and drop, colour presets, palette, preview, apply, revert, autostart toggle, theme switch. |
| `OverlayEngine.cs` | Lifecycle of the overlay: watchdog, repaint timer, tray icon, host search, re-attach on Explorer restart, teardown. |
| `OverlayWindow.cs` | The surface itself: window class, `WS_CHILD` attach, Z-order, `WM_PAINT` painting, *cover* scaling, clean shutdown repaint. |
| `Native.cs` | P/Invoke surface: `EnumWindows`, `FindWindowEx`, `CreateWindowEx`, `SetWindowPos`, `RedrawWindow`, plus the `WorkerW` discovery logic and the DWM dark-mode attribute. |
| `AutoStart.cs` | Autostart through `HKCU\...\CurrentVersion\Run`, with the `--apply` payload. |
| `Reverter.cs` | Deterministic rollback: overlay, autostart, wallpaper override, saved image, theme. Also `ThemeUtil`. |

## Safety and rollback

The utility knows precisely what it modifies, and reverts precisely that. It never snapshots the registry — a snapshot is worthless once the values have already been changed by someone else. Instead it removes exactly its own footprint:

```
overlay surface            removed via OverlayEngine.Remove()
autostart entry            deleted from HKCU\...\CurrentVersion\Run
wallpaper override         Wallpaper, WallpaperStyle, TileWallpaper in HKCU\Control Panel\Desktop
saved image                wallpaper.* in %APPDATA%\DesktopOverlay
light theme                restored on request only
```

A domain `\\server\share` wallpaper path is recognised as policy, not as an override, and is never deleted. The desktop is redrawn, and the action is written to the log together with a human-readable report.

## Logs and stored data

```
%APPDATA%\DesktopOverlay\
 ├── overlay.log      append-only diagnostics
 └── wallpaper.*      working copy of the image, for autostart
```

The working copy exists for one reason: so the overlay keeps working after the original file is moved, renamed, or cleared out of *Downloads*. The log is capped in practice by its own usefulness — the first eight paint events are recorded, then the noise stops.

## Troubleshooting

**The image does not appear.** The overlay is almost certainly attached to the wrong `WorkerW`. Confirm the host in the log: it records the window handle, the host handle and the resulting rectangle.

**The image flickers.** The repaint timer is not running. Confirm `overlay.log` shows periodic paint entries.

**A magenta rectangle appears.** `OVERLAY_DEBUG_FILL=1` is set in the environment. Remove it — the surface is fine, the fill is diagnostic.

**The overlay is missing after sign-out or Explorer crash.** Nothing to do: the watchdog restores it. If it does not, re-attach manually from the tray menu.

**The Windows Settings background pane appears unchanged.** That is expected. The overlay does not touch the system wallpaper, which is the entire point of the design.

## License

MIT.

---

<div align="center">

<p align="center"><sub>DesktopOverlay &nbsp;&middot;&nbsp; .NET 8 &nbsp;&middot;&nbsp; Windows 10 / 11 x64 &nbsp;&middot;&nbsp; MIT</sub></p>

</div>
