# AGENTS.md

DesktopOverlay — Windows-only Rust/Win32 GUI app that draws a wallpaper layer as a
child window inside Explorer's desktop host window (below icons, above nothing else).
No tests, no CI, no lint config exist in this repo.

## Commands

```powershell
cargo build --release          # target\release\DesktopOverlay.exe
cargo clippy                   # there is no test suite; clippy + build IS the verification step
```

- Crate name is `desktop-overlay` but the **binary is `DesktopOverlay`** — `cargo run`
  works, but artifact paths and autostart payloads use `DesktopOverlay.exe`.
- Requires MSVC (`vcvars64.bat` under `%ProgramFiles%\Microsoft Visual Studio\2022`).
  `powershell -ExecutionPolicy Bypass -File scripts\install-rust.ps1` installs rustup
  (idempotent) and warns if MSVC is missing — Rust will compile but linking will fail.
- Release profile is aggressive: `panic = "abort"`, `lto`, `codegen-units = 1`,
  `opt-level = "s"`, `strip = true`. Long link times are expected.
- App must be launched as the interactive desktop user: it draws into that user's
  Explorer windows and writes to `HKEY_CURRENT_USER`.

## Comments

- **Do not write comments in the code.** No `//`, no `/* */`, no `///` doc comments, no
  section banners, no "fix:" / "TODO:" notes explaining what you just changed, no comments
  restating the code. Nobody but an agent reads this repo, and an agent re-derives context from
  the code, `AGENTS.md`, and the log in `%APPDATA%\DesktopOverlay\overlay.log` anyway.
- The only acceptable comment is one that records a **non-obvious constraint** that the code
  itself cannot express and that an agent would otherwise "fix" away — e.g. why a `WM_PAINT`
  HDC is the only valid surface, why the 400 ms sleep after `restart_explorer()` exists. Keep
  such notes to one or two lines, put them at the site they apply to, and only when the
  invariant is absent from this file. Prefer recording it here in `AGENTS.md` instead.
- This applies to new comments only. Do not go delete existing comments in files you touch;
  that is unrelated churn.

## Language and version conventions

- **All code comments, doc comments, log messages, and UI strings are in Russian.**
  Keep writing new ones in Russian to match. READMEs are bilingual (`README.md` English,
  `README_RU.md` Russian) — update both when behavior changes.
- `app.manifest` carries its own `version="1.2.0.0"` that must be bumped in lockstep
  with `Cargo.toml`'s `version` (currently `1.2.0`).

## Non-obvious Win32 invariants (breaking these silently breaks the wallpaper)

Read `src/overlay.rs` and `src/native.rs` before touching the attach/paint path.

- **Attach to the `WorkerW` *after* the one holding `SHELLDRL_DefView`.**
  `native::find_wallpaper_worker_w()` sends `0x052C` to `Progman` to re-split the
  `WorkerW` chain, finds the host containing `SHELLDLL_DefView` (desktop icons), and
  takes the **next** sibling. Picking the icons host puts the `SysListView32` background
  on top of the image — the #1 failure mode of DIY wallpaper overlays.
- **Create the overlay as `WS_CHILD` in the very first `CreateWindowExW` call.** It is
  never shown top-level and re-parented; that path fails silently on some Explorer builds.
- **Paint only inside the `WM_PAINT` / `BeginPaint` HDC.** Drawing via a `GetDC` obtained
  after `EndPaint` does not composite at all. The whole surface is invalidated each repaint.
- **The repaint timer is load-bearing.** Explorer periodically repaints its own wallpaper
  over the foreign surface without sending `WM_PAINT`, erasing the overlay.
  `engine.rs`: repaint every 1000 ms, watchdog every 2000 ms.
- **Self-cleaning exit.** Before `DestroyWindow`, `OverlayWindow::show_system_wallpaper()`
  must repaint with the real wallpaper (from `TranscodedWallpaper` or the domain policy
  path). Otherwise burnt-in pixels survive until Explorer restarts; `RedrawWindow` does
  not fix it.
- Mouse click-through uses both `WS_EX_TRANSPARENT` **and** `WM_NCHITTEST → HTTRANSPARENT`.
  `OVERLAY_NO_EX_TRANSPARENT=1` drops the extended style as a workaround for DWM
  composition bugs; `OVERLAY_DEBUG_FILL=1` fills magenta to isolate windowing from painting.

## Rust-side patterns that are load-bearing

- **Global state lives in a `thread_local!` `RefCell<Option<App>>` in `src/app.rs`.**
  During `WM_NCCREATE` the slot is still empty, so any handler reached before
  `app::start()` must use `app::try_with_app(.., fallback)`. `app::with_app(..)` panics.
- **Never call into `app` from code that already runs inside `app::with_app(..)`.**
  `with_app` holds a `borrow_mut()`; a nested `with_app` on the same thread panics
  with `RefCell already borrowed` and `panic = "abort"` kills the process. Modules
  below `app` therefore return results outward instead of calling back: `engine.rs`
  owns its image and exposes `set_image` / `take_error`, and `on_tray_message`
  returns a `TrayCommand` for the caller to execute after the borrow ends.
- **Per-window state is a `Box`ed struct whose raw pointer is stashed in
  `GWLP_USERDATA`** (`overlay.rs`, `ui/main_form.rs`). It must be zeroed in `WM_NCDESTROY`,
  otherwise the next `CreateWindowExW` sees a dangling pointer.
- **Never hold a `&mut MainState` across anything that pumps messages.**
  `dialogs::*`, `engine::pump_messages`, `browser_picker::show` and `shell::restart_explorer`
  dispatch messages synchronously, so `WM_PAINT` re-enters and takes a second `&mut` on
  the same state. Action handlers take `hwnd` and use the short-lived
  `main_form::with_state` borrow instead of `&mut self`.
- **The `RefCell` borrow needs a runtime guard, not just a comment.** `engine.remove()`
  pumps messages *while* the borrow is held, so a `WM_TIMER` or tray message arriving in
  that window used to double-borrow and abort the process. `app.rs` tracks the borrow in
  a `thread_local!` flag: `with_app` asserts, and the window handlers use
  `try_with_app` so the event is dropped instead of killing the process and burning the
  wallpaper into the desktop.
- **The window is sized by its client area, not by `CreateWindowExW`.** The layout is
  authored in logical coordinates up to (500, 512); passing that as the *window* size
  clips the status label. Use `window_size_for_client` (`AdjustWindowRectEx`) and scale
  every control rect through `paint::scale_rect` in both painting and `hit`, or the
  buttons drift from their artwork at high DPI.
- The preview is expensive to build: resample the source into BGRA **once**, in
  `controls::Preview`, and cache it on `MainState`. Rebuilding it inside
  `ControlState::draw_drop_zone` made every hover frame pay for a full resample.
- Tooltip anchors are client coordinates; the tooltip window is a top-level popup, so
  convert through `ClientToScreen` or it renders in the screen corner.
- `main.rs` carries `#![windows_subsystem = "windows"]`. Cargo rejects the same key in
  `Cargo.toml` as an unused manifest key.
- Single instance is a named mutex `Local\DesktopOverlay.SingleInstance`. A second launch
  **exits silently**, so you cannot keep two copies alive for side-by-side testing.
- Re-registering a window class returns 0 and is intentionally ignored — not a bug.
- The `windows` crate feature list in `Cargo.toml` is explicit. Using a new Win32 API
  requires adding the corresponding `Win32_*` feature first, or the build fails.
- `panic = "abort"` in release means a panic kills the process with no unwinding cleanup —
  the overlay teardown path is the only thing that restores the desktop.

## Rollback contract

`src/reverter.rs` must keep removing exactly this footprint and nothing else (no registry
snapshots — they are worthless once values are changed by others):

```
overlay surface       engine.remove()
autostart             HKCU\Software\Microsoft\Windows\CurrentVersion\Run
wallpaper override    HKCU\Control Panel\Desktop  (Wallpaper, WallpaperStyle, TileWallpaper)
saved image           %APPDATA%\DesktopOverlay\wallpaper.*
taskbar size          HKCU\...\Explorer\Advanced\TaskbarSmallIcons  (Windows 10 only)
Edge pin / default    taskbarpin verb + http/https/.htm/.html associations
```

- A `\\server\share` wallpaper value is **domain policy, not an override** — never delete it.
- `HKCU\UserChoice` cannot be written directly since Windows 1803; go through shell verbs /
  registered `ProgId`s (`browser.rs`, `taskbar_pins.rs`). Do not add a direct write.
- Theme and `TaskbarSmallIcons` are read by Explorer **only at startup**: any change requires
  `shell::restart_explorer()`, and `reverter.rs` sleeps 400 ms after each restart. Removing
  the sleep causes the reported state to race the actual shell state.
- `taskbar.rs` refuses the small-taskbar toggle on Windows 11 (build number check); the UI
  button is expected to be disabled there.

## Debugging

- Log: `%APPDATA%\DesktopOverlay\overlay.log` (append-only). It is the primary diagnostic —
  it records attach handles (`прикреплено: hwnd=… host=…`), placement rects, and the first 8
  paint events (`MAX_PAINT_LOGS`), then stops.
- `wallpaper.*` in the same folder is a deliberate stable copy so autostart survives the
  original file being moved or deleted. Do not "clean up" writes there.
- README "Troubleshooting" maps specific log lines to specific failures; keep it accurate.
