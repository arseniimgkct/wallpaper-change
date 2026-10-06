# DesktopOverlay — Functional Specification

Status of this document: describes the behaviour that is **actually implemented today** in the
C# reference application (`DesktopOverlay.csproj`, .NET 8 / WinForms, x64). It is written as the
functional contract for the Rust port on the `rust` branch.

Every statement below was verified against the source. Where the current implementation deviates
from what a naive reading of the README would suggest, the deviation is called out explicitly in
[§12 Known limitations](#12-known-limitations-and-quirks). Those items are part of current
behaviour and must be reproduced or deliberately fixed, not accidentally changed.

---

## 1. Purpose and operating principle

DesktopOverlay places a picture or a solid colour on the Windows desktop **on top of the system
wallpaper**, and leaves the system wallpaper itself untouched.

This distinction is the whole product. The alternative approach — writing a new `Wallpaper` value
into the registry — is deliberately rejected for three reasons:

1. **Corporate policy wins.** On domain-joined machines the wallpaper is usually enforced by GPO.
   An overlay draws *above* policy, so it works without administrator rights and without fighting
   the policy agent.
2. **It is reversible in one action.** Nothing is destroyed; the overlay is simply detached.
3. **Settings stay untouched.** The Windows *Personalization* pane keeps showing the real system
   wallpaper. A user who opens Settings must not see the app's picture and conclude that Windows
   is broken.

**Consequence:** the overlay only exists while the process runs. Closing the app removes the
picture. This is expected behaviour, not a bug.

### 1.1 Target environment

| Property | Value |
|---|---|
| OS | Windows 10 or Windows 11, x64 only |
| Shell | Explorer with the classic desktop window tree (`Progman` / `WorkerW`) |
| Runtime | .NET 8 Desktop Runtime, or self-contained single-file EXE |
| Privileges | Standard user. No elevation, no service, no HKLM writes |
| Registry scope | `HKEY_CURRENT_USER` exclusively |
| Distribution | One EXE file, no installer, no configuration file, no external dependencies |

---

## 2. User-visible features

### 2.1 Selecting an image

Four input paths, all converging on the same internal "chosen file" state:

| # | Path | Behaviour |
|---|---|---|
| 1 | **Browse** (`Обзор…`) | Standard Win32 open dialog, filter `.jpg .jpeg .png .bmp .gif .tif .tiff` plus "All files". `CheckFileExists` is on. |
| 2 | **Drag and drop** | Accepted on both the preview zone and the whole window. `DragEnter` pre-validates and shows the copy cursor only when a usable file is present. |
| 3 | **Clipboard** (`Ctrl+V` or the `📋 Буфер` button) | Accepts an image, a file, a folder, or a copied path. Applies immediately — see §2.2. |
| 4 | **Command line** `--set` | Preselects a path and applies it right away, without user interaction. |

Accepted extensions are defined once in `Program.ImageExtensions` and validated by
`Program.IsSupportedImage` (case-insensitive). Extension checking is a gate only; the file must
still decode successfully.

**Dropped folder resolution.** If a dropped or copied item is a directory, the first supported
image in it is taken, ordered case-insensitively by filename. Only the top level is enumerated —
no recursion.

**Preview.** A successful selection renders the image into the drop zone with
`PictureBoxSizeMode.Zoom` (letterboxed, never cropped, never stretched). The hint text hides and
the **Apply** button becomes enabled. Only the *Apply* action commits; selecting alone changes
nothing on the desktop.

**Failure.** If the file cannot be decoded, the preview clears, the hint returns, and the reason
is shown in the status line. The previous selection is discarded.

### 2.2 Clipboard ingestion

`ClipboardImage.TryTake` accepts, in priority order:

1. **File-drop list** (`CF_HDROP`) — first supported image, or the first supported image inside a
   dropped folder.
2. **Text** — the first line, trimmed of quotes, is treated as a path if it has a supported
   extension and the file exists. Supports copying a file path in Explorer or from a browser.
3. **Image** (`Clipboard.GetImage`) — saved as `%APPDATA%\DesktopOverlay\wallpaper.png`.
4. **Raw bitmap** (`DataFormats.Bitmap`) — same save path.

Retry policy: `ExternalException` from another application holding the clipboard is retried up to
**12 attempts with an 80 ms delay** (≈1 s total). After that the user is told to copy again.

**Clipboard applies immediately.** Unlike Browse and Drag-and-drop, a successful paste does not
wait for **Apply** — it selects *and* applies in one action. This asymmetry is intentional and
must be preserved.

### 2.3 Solid colour

Six fixed presets plus a full palette:

| Preset | RGB |
|---|---|
| Чёрный | `0,0,0` |
| Тёмный графит | `18,18,18` |
| Тёмно-синий | `28,33,40` |
| Сланцевый | `37,42,52` |
| Серый | `50,50,50` |
| Белый | `245,245,245` |

- **Palette** (`Палитра…`) opens `ColorDialog` with `FullOpen` and `AnyColor` enabled, seeded at
  `#121212`.
- A colour is materialised as a real PNG: `max(1920, primary screen width)` ×
  `max(1080, primary screen height)`, `32bppPArgb`, filled with the colour and saved as
  `wallpaper.png`. The floor of 1920×1080 exists so that solid colours survive a later resolution
  decrease without being upscaled from a smaller bitmap.
- Selecting a colour only *selects*. Like a file, it needs **Apply**.

### 2.4 Apply

**Установить обои** performs, in order:

1. `Program.LoadStableCopy` — copies the source into `%APPDATA%\DesktopOverlay\wallpaper.<ext>`
   and loads a decoupled `Bitmap` from the copy.
2. `OverlayEngine.Apply(image, tray: true)` — attaches the overlay window and installs the tray icon.
3. On success, the status line reports success and the autostart checkbox is re-read from the registry.

If attaching fails, the message points at `overlay.log` and the image is disposed rather than
leaked.

### 2.5 Tray icon

Present whenever an overlay is applied. Menu, in order:

| Item | Action |
|---|---|
| **Показать окно утилиты** (bold) | Show and activate the utility window from the tray |
| **Переприкрепить обои** | Force a full detach/reattach — the manual escape hatch for a lost overlay |
| **Откатить обои** | Same revert flow as the button; prompts before touching anything |
| *(separator)* | |
| **Выход** | Remove the overlay and exit the process |

Tray exit routes through `OverlayEngine.Remove()`, so the system wallpaper is restored before the
process dies. In the pure-tray (`--apply`) mode there is no window to return to; the app stays
resident with only the tray icon.

### 2.6 Window close semantics

Closing the main window with the X button **hides** it; it does not exit. `OnFormClosing` cancels
the close and calls `Hide()` unless `_reallyExit` is set. The process keeps running with the
overlay and tray icon alive. Real exit is via tray → **Выход**.

### 2.7 Windows theme toggle

- Button label always states the *target* state, never the current one: `🌙 Сменить тему Windows
  на тёмную` or `☀ Сменить тему Windows на светлую`.
- Writes both `AppsUseLightTheme` and `SystemUsesLightTheme` in
  `HKCU\...\Themes\Personalize` as `DWORD`.
- Reads back to confirm, then restarts Explorer **only if the theme actually changed or the write
  failed**. No change → no restart, no visible desktop flicker.
- Explorer restart is mandatory after a write: the shell reads these values only at startup.

### 2.8 Taskbar size toggle

Toggles `TaskbarSmallIcons` (`DWORD`, 40 px ↔ 30 px) in
`HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced`.

- **Windows 11 (build ≥ 22000): button permanently disabled** with the explanation that Microsoft
  removed the setting and there is no supported way to change it. The check is by OS build number,
  not by capability probing.
- Label reflects the current state: shrink or restore.
- Registry write → read-back confirm → restart Explorer → sleep 400 ms → redraw desktop → update label.
- The value is read tolerantly: `int`, `long`, `byte`, and numeric `string` are all accepted,
  because Explorer sometimes leaves the value as a string.

### 2.9 Autostart

Checkbox **Запускать вместе с Windows**, backed by the value `DesktopOverlay` in
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

- The written command is `"<current exe>" --apply "<saved wallpaper path>"`.
- If no saved wallpaper exists yet, enabling autostart is refused with a message telling the user
  to apply wallpapers first. Autostart cannot point at a non-existent image.
- The checkbox reflects registry state, not just click state, after apply and revert.

### 2.10 Revert

Button **Откатить всё** asks two questions before acting:

1. "Remove the overlay and return the system wallpaper?" — No aborts everything.
2. "Also restore the light Windows theme?" — Yes/Decide per invocation.

Then `Reverter.RevertAll` runs. **Theme is restored only on explicit request**; the default revert
leaves the theme as it is.

---

## 3. Overlay mechanics

### 3.1 Why a child of `WorkerW`

```
Progman
 └─ WorkerW      ← "host" windows live here
 └─ WorkerW
     └─ SHELLDLL_DefView
         └─ SysListView32   ← desktop icons
 └─ WorkerW      ← our overlay draws here
```

The overlay is a **child window (`WS_CHILD`)** of the `WorkerW` that sits *below* the one owning
`SHELLDLL_DefView`. Painting there puts the picture underneath the icons and under the taskbar,
while leaving it a genuine participant in the desktop window tree.

### 3.2 Host discovery — `Native.FindWallpaperWorkerW`

1. Find `Progman` by class name, with a fallback `EnumWindows` scan for a top-level `Progman`.
2. Send `MSG_SPLIT_WORKERW` (0x052C) with a 1000 ms timeout, which forces the shell to recreate
   the `WorkerW` hierarchy.
3. Collect all top-level windows of class `WorkerW` in Z-order.
4. Find the index of the one containing `SHELLDLL_DefView`.
5. Return the *next* `WorkerW`. If none follows, fall back to `Progman` itself.
6. If no `WorkerW` contains `SHELLDLL_DefView`, return zero — attach fails cleanly.

The fallback to `Progman` and the zero return are both real paths on real systems; neither is
dead code.

### 3.3 Window style and behaviour

| Property | Value | Reason |
|---|---|---|
| Extended style | `WS_EX_NOACTIVATE \| WS_EX_TOOLWINDOW` | Never steals focus; absent from Alt+Tab |
| Extended style | `+ WS_EX_TRANSPARENT` | Click-through |
| `WM_NCHITTEST` | returns `-1` (`HTTRANSPARENT`) | Click-through, second line of defence |
| Style | `WS_CHILD \| WS_VISIBLE` | Participates in the desktop tree |
| Z-order | `HWND_BOTTOM` | Icons and taskbar stay above the picture |
| Class name | `DesktopOverlaySurface` | Registered once per process; `hbrBackground = 0` |

**Only the primary monitor is covered.** Layout uses `Screen.PrimaryScreen.Bounds`, converted to
the host's client coordinates via `MapWindowPoints`. Multi-monitor wallpaper spanning is explicitly
out of scope.

### 3.4 Painting

- `WM_PAINT` → `BeginPaint` / `EndPaint` with a **real paint context**. Using a `GetDC` +
  `EndPaint` mismatch makes DWM treat the surface as damaged, which produces constant flicker.
  This is the single most important detail in the whole renderer.
- `WM_ERASEBKGND` returns 1: the background is never erased, only painted over.
- **Cover scaling.** `scale = max(w/srcW, h/srcH)`, centred, cropped to the client rect, with
  `HighQualityBicubic` interpolation. The image always fills the screen; overflow is cut.
- **Scaling cache.** The scaled bitmap is cached and keyed on target size *and* source identity.
  A repaint at unchanged size is a single blit — no resampling. This is what makes the 1 s repaint
  timer nearly free.
- **Blit mode.** `CompositingMode.SourceCopy` for an exact pixel copy.

### 3.5 Repaint triggers

Three, deliberately redundant:

| Trigger | Mechanism |
|---|---|
| On demand | `InvalidateRect` + `UpdateWindow` after attach, layout, or repaint timer |
| Timer | 1000 ms periodic `ForceRepaint` — defeats shell redraws that skip our window |
| Display change | `SystemEvents.DisplaySettingsChanged` → relayout + repaint |

### 3.6 Survival and re-attach — the watchdog

A 2000 ms watchdog calls `EnsureOverlay`, which:

- returns early if the overlay is still attached and correctly sized;
- relayouts if the window rect no longer matches the primary screen;
- otherwise **recreates the window from scratch**, logging the reason.

Recreation is the mechanism that survives an Explorer restart: the shell destroys our child window
and rebuilds the whole hierarchy, and the watchdog notices the handle is gone or re-parented and
rebuilds the overlay. A user does not have to intervene. Manual recovery is still available via
tray → **Переприкрепить обои**, which forces the same path with `force: true`.

An unhealthy window is reported to the status line, so the UI can say the overlay failed to embed
rather than silently showing nothing.

### 3.7 Clean removal

`Remove()` must leave **zero** pixels behind. The sequence is not optional:

1. If the window is still alive, `ShowSystemWallpaper()` first — repaint the surface with the real
   system wallpaper, so the shell repaints genuine content into that area.
2. `Thread.Sleep(150)` + `Application.DoEvents()` to let the repaint land before the window dies.
3. `DestroyWindow`.
4. Destroy the tray icon.
5. `Native.RedrawDesktop()` — invalidate `Progman` and every `WorkerW` with
   `RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_FRAME`.

Destroying the window without step 1–2 leaves the last painted frame frozen on screen.

The genuine system wallpaper is resolved from `%APPDATA%\Microsoft\Windows\Themes\TranscodedWallpaper`,
falling back to the `Wallpaper` policy value in `HKCU\...\Policies\System` when that file exists.
If neither loads, the window is closed anyway and the failure is logged.

---

## 4. Image stability copy

Every applied image is copied to `%APPDATA%\DesktopOverlay\wallpaper.<ext>`.

The copy exists for one reason: **the overlay must keep working after the original file is moved,
renamed, or cleaned out of Downloads.** Autostart also references the copy, not the original.

The copy is refreshed only when it is stale — differing file length, or the copy's write time is
older than the source's. Comparing only timestamps would re-copy constantly; comparing only length
would miss same-size edits.

The image is decoded through a `FileStream` and re-wrapped in `new Bitmap(...)` to drop the file
handle, which otherwise blocks the user from deleting or replacing the file.

---

## 5. Command-line interface

No `--help`. Dispatch happens in `Program.Main` before any UI is built.

| Invocation | Mode |
|---|---|
| `DesktopOverlay.exe` | Utility window, tray icon only after Apply |
| `DesktopOverlay.exe --set "<path>"` | Window opens with the path preselected and applies immediately. Requires an existing, supported file. |
| `DesktopOverlay.exe --apply "<path>"` | Headless tray mode. No window ever. Loads a stable copy, applies the overlay, tray icon only. |
| `DesktopOverlay.exe --revert` | Full revert, then exit. No window, no prompts. |
| `DesktopOverlay.exe --revert --light` | Full revert **plus** light-theme restoration, then exit. |

### 5.1 Single instance

A named mutex `Local\DesktopOverlay.SingleInstance` is created with `initiallyOwned: true`. A
second instance checks `isFirst`, logs "already running", and exits silently.

This is a design requirement, not merely resource saving: two instances would create two overlays
and two tray icons over the same desktop.

---

## 6. Revert contract

`Reverter.RevertAll(engine, restoreLightTheme)` is the **single** rollback path, shared by the
button, the tray item, and `--revert`. Steps, each contributing to a human-readable report:

| Step | Effect | Report fragment |
|---|---|---|
| 1 | Remove the overlay | `оверлей снят` |
| 2 | Disable autostart, if enabled | `автозапуск выключен` |
| 3 | Clear the wallpaper override, if present | `системные обои возвращены` |
| 4 | Restore normal taskbar size + restart Explorer | `обычный размер панели задач восстановлен` |
| 5 | Restore light theme, **only if requested** | `светлая тема восстановлена` |
| 6 | Delete the saved image copy | `копия картинки удалена` |
| 7 | Redraw the desktop | — |

The report is joined with commas, logged, and shown to the user. Only steps that actually changed
something appear, so the report doubles as a record of what was touched.

**Domain wallpapers are never deleted.** If `HKCU\Control Panel\Desktop\Wallpaper` starts with
`\\`, it is domain policy and `ClearWallpaperOverride` returns early. This guard must survive any
refactor — without it, revert would destroy a corporate setting the user cannot restore.

### 6.1 What revert does *not* do

- Does not delete the user's original image file.
- Does not change the theme unless asked.
- Does not uninstall Explorer state beyond the taskbar value it owns.
- Does not remove the `Wallpaper` policy value set by a GPO.

---

## 7. Persistent state

Location: `%APPDATA%\DesktopOverlay\` (falls back to `%USERPROFILE%` if `APPDATA` is unset).

| Artifact | Purpose |
|---|---|
| `overlay.log` | Append-only diagnostics, timestamp `yyyy-MM-dd HH:mm:ss` |
| `wallpaper.<ext>` | Stability copy of the applied image |
| `wallpaper.png` | Solid-colour render, or a clipboard image |

Log-volume discipline: **the first 8 paint events are logged, then paint logging stops.** The
1 s repaint timer would otherwise produce ~3600 identical lines per hour. This is a deliberate
mechanism, not an oversight.

### 7.1 Diagnostics via environment variables

| Variable | Effect |
|---|---|
| `OVERLAY_DEBUG_FILL=1` | Fills the surface with magenta instead of the image. Reveals DWM composition problems as a magenta rectangle. |
| `OVERLAY_NO_EX_TRANSPARENT=1` | Drops `WS_EX_TRANSPARENT` and the `WM_NCHITTEST → HTTRANSPARENT` reply, so the overlay receives clicks. Use when debugging overlap or input routing. |

### 7.2 Log messages worth knowing

| Message | Meaning |
|---|---|
| `прикреплено: hwnd=… host=…` | Attach succeeded; both handles recorded |
| `размещено: client=(x,y) WxH, экран=(x,y) WxH` | Layout result. Client ≠ screen indicates a host mapping problem. |
| `[paint #N] …` | Paint events 1–8 only |
| `пересоздание оверлея: окно потеряно (перезапуск Explorer?)` | Watchdog recovered the overlay |
| `CreateWindowEx вернул 0, Win32 error = …` | Attach failed with a Win32 error code |
| `Панель задач: запрошена …, подтверждена …` | `подтверждена нет` means GPO blocked the registry write |
| `Тема Windows: запрошена …, подтверждена …` | Same read-back convention |
| `буфер обмена недоступен: …` | Clipboard retries exhausted |

---

## 8. Explorer restart

`ShellUtil.RestartExplorer` is shared by the theme toggle, the taskbar toggle, and revert step 4.

1. `Kill()` every `explorer.exe`, waiting up to 1500 ms each.
2. `Thread.Sleep(800)`.
3. Wait for the taskbar: poll for top-level class `Shell_TrayWnd` up to 48 times × 250 ms (12 s).
4. If it appeared on its own, done — Explorer restarted itself.
5. Otherwise start `%WINDIR%\explorer.exe` manually.

Step 3 is what makes the restart robust: normally Explorer auto-restarts, and killing it without
that check can leave a user with no desktop.

---

## 9. Module responsibilities

| File | Responsibility |
|---|---|
| `Program.cs` | Entry point, single-instance mutex, CLI dispatch, `TrayContext`, constants, logging, stable-copy loader, app icon |
| `MainForm.cs` | The entire UI: layout, presets, palette, drag and drop, clipboard paste, apply, revert, theme toggle, taskbar toggle, autostart, status line. Also `DropZone`. |
| `OverlayEngine.cs` | Overlay lifecycle: apply/remove, watchdog, repaint timer, tray icon, host search, re-attach on Explorer restart, health check |
| `OverlayWindow.cs` | The surface: class registration, `WS_CHILD` attach, Z-order, `WM_PAINT`, cover scaling, cache, clean-removal repaint |
| `Native.cs` | All P/Invoke: window enumeration, `WorkerW` search, window creation, painting, desktop redraw, DWM dark mode |
| `ClipboardImage.cs` | Clipboard ingestion across all formats, with retry |
| `Reverter.cs` | Rollback of everything reversible, plus `ThemeUtil` |
| `TaskbarUtil.cs` | Taskbar button size; Windows 11 gate |
| `ShellUtil.cs` | Explorer restart with taskbar wait |
| `AutoStart.cs` | `HKCU\...\Run` entry for autostart |

---

## 10. Behavioural invariants

These must hold in the port. They are the properties that make the app trustworthy.

1. **The system wallpaper value is never modified while applying.** Only on revert.
2. **Autostart only ever points at a file that exists.**
3. **Remove leaves no residue**, via paint-genuine-wallpaper → wait → destroy → redraw.
4. **Every registry write is read back and logged** as requested/confirmed.
5. **Exactly one instance**, enforced by a named mutex.
6. **The overlay never takes focus** and never receives clicks in normal operation.
7. **Revert is complete and unconditional** except for the theme, which is opt-in.
8. **Domain wallpapers (`\\…`) are never deleted.**
9. **Every user-visible string, log line, and code comment is in Russian.**
10. **Only `HKEY_CURRENT_USER` is written.** No elevation, ever.
11. **The overlay disappears when the process exits** — by design.
12. **Explorer restart only happens after a confirmed registry write** that the shell needs to re-read.

---

## 11. Diagnostics playbook

| Symptom | Cause and resolution |
|---|---|
| Picture does not appear | Almost always attached to the wrong `WorkerW`. Read `прикреплено: hwnd=… host=…` and `размещено:` from the log and compare. |
| Picture flickers | Repaint timer not effective — check for periodic `[paint #N]` entries. Confirm the `BeginPaint`/`EndPaint` pairing was not replaced by `GetDC`. |
| Magenta rectangle | `OVERLAY_DEBUG_FILL=1` is set. Unset it; the surface is fine. |
| Overlay gone after logout or Explorer crash | Nothing to do — the watchdog restores it. If not, use tray → **Переприкрепить обои**. |
| Settings shows the old wallpaper | Correct. The app never changes system wallpapers. |
| Taskbar unchanged | Check `Панель задач: подтверждена …`. `нет` means a domain policy blocked the write. `да` but no change means Explorer did not restart. On Windows 11 the button is disabled by design. |
| Second launch does nothing | By design: single-instance mutex. Check the log. |

---

## 12. Known limitations and quirks

Current behaviour. Reproduce faithfully or fix deliberately — do not change by accident.

1. **Primary monitor only.** No multi-monitor spanning; secondary displays are untouched.
2. **Clipboard applies immediately**, while Browse and drag-and-drop require **Apply**.
3. **`LoadStableCopy` disposes its temporary `Image` after cloning.** Fine in .NET; a port must
   re-check ownership of the returned bitmap to avoid freeing memory still in use.
4. **No `--help` and no argument validation.** Unrecognised arguments are ignored and the window
   opens normally.
5. **Solid-colour minimum canvas is 1920×1080**, independent of the actual screen.
6. **The tray icon only exists while an overlay is applied.** Launching to revert creates no icon.
7. **`Thread.Sleep` and `Application.DoEvents()` appear in the removal, theme, and taskbar paths.**
   This is deliberate synchronisation with the shell, not laziness.
8. **Explorer restart kills every `explorer.exe`.** Any unsaved Explorer state is lost.
9. **No animation, no slideshow, no multi-image rotation.** One image at a time.
10. **No image editing, cropping, or filters.**
11. **No localisation.** UI text is Russian only.
12. **No tests, no CI, no linter configuration.** `dotnet build` (currently 0 warnings, `Nullable`
    enabled) is the only automated gate.

---

## 13. Porting notes for the Rust implementation

Constraints implied by the behaviour above:

- **Dependencies.** `windows` (0.62) for Win32/GDI/registry/COM, `image` for decoding and PNG
  encoding. No UI framework: the existing UI is hand-built WinForms layout, so the port should
  draw controls manually or use a minimal immediate-mode approach to match the pixel layout.
- **Threading model.** Single-threaded message loop with global state, matching Win32 window
  procedures. Keep the mutex-equivalent single-instance check.
- **COM.** `OleInitialize` is required before file dialogs, the colour dialog, the tray icon, and
  drag-and-drop. Missing it breaks dialogs in ways that look unrelated.
- **Clipboard.** Enumerate formats rather than assuming `CF_DIB`: handle `CF_HDROP`, `CF_DIB`,
  `CF_DIBV5`, and `CF_BITMAP`. Retain the 12 × 80 ms retry.
- **Paint context.** Use `BeginPaint`/`EndPaint` and blit. The flicker bug in §3.4 is the classic
  failure mode of getting this wrong — it is worth an explicit test.
- **Registry.** Read back after every write, and tolerate values stored as `int`, `long`, `byte`,
  or numeric `string`. Explorer is inconsistent, and a strict `DWORD` read will report a write as
  failed when it succeeded.
- **Executable metadata.** Icon and manifest (DPI awareness, `asInvoker`) must be embedded at
  build time; the current project sets both via MSBuild properties.
- **Ship one file.** Self-contained, single-file, x64, no installer — the defining distribution
  property of the product.
