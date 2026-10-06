# AGENTS.md

Windows WinForms utility (`net8.0-windows`, x64, C#). Puts an image or a solid colour on the
desktop **above** the system wallpaper, without changing the wallpaper itself. One `.csproj`,
no tests, no CI, no linters. `SPEC.md` is the source of truth (functional contract, invariants,
known limitations). When README and code disagree, trust `SPEC.md` and the source.

## Branches

- `main` — released C# version; also holds `SPEC.md` and this file.
- Local branch `rust` currently points at the C#-only state (the `SPEC.md` commit). No Rust code
  in it despite the name.
- The real Rust port lives on `origin/rust-port`: `Cargo.toml`, `src/*.rs`, `build.rs`,
  `app.manifest`, `scripts/install-rust.ps1`. The C# sources are kept there as the reference.
- Before editing "the wrong" implementation, check `git status -sb` and
  `git rev-parse --short HEAD`.

## Build and run

```powershell
dotnet build                          # enough: single .csproj at the root
dotnet publish DesktopOverlay.csproj -c Release
.\bin\Release\net8.0-windows\win-x64\publish\DesktopOverlay.exe
```

- Windows only: the code is Win32 P/Invoke and WinForms end to end.
- RID `win-x64`, `SelfContained`, `PublishSingleFile` are already set in the `.csproj`, so
  publish yields a single EXE (~64 MB) with no extra flags.
- No solution file exists and `.gitignore` contains `*.sln` — creating one is pointless.
- `bin/`, `obj/`, `*.exe`, `*.dll` are gitignored; do not commit them.

## Verification

- `dotnet build` is the only automated gate (currently 0 warnings; `Nullable` is on, so any new
  CS8xxx is a regression).
- No `.Designer.cs` / `.resx`: the whole UI is built in code inside `MainForm.cs` (which also
  holds the `DropZone` class). The Visual Studio designer does not apply — edit the constructor
  by hand.
- The app is a console-less `WinExe`: stdout is empty and all diagnostics go to
  `%APPDATA%\DesktopOverlay\overlay.log`. Do not look for terminal output.

## Side effects — the main caution

The utility modifies the user's system: it writes to `HKCU` (theme, `TaskbarSmallIcons`,
autostart) and **restarts Explorer** (`ShellUtil.RestartExplorer` kills every `explorer.exe` and
starts a new one, waiting for `Shell_TrayWnd`).

- Do not run the app or `--revert` "just to check that it builds". Revert touches the same
  registry branches as normal operation, and restarts the shell.
- Applying **does not** change the system wallpaper. `Reverter.cs` only *deletes* existing
  `Wallpaper`/`WallpaperStyle`/`TileWallpaper` values under `HKCU\Control Panel\Desktop`.
- `Reverter.RevertAll` is the single rollback path, shared by the button, the tray item and
  `--revert`. Any new reversible action must go through it and into the user-facing report.
- A domain path `\\server\share` in `RevertAll` is recognised as policy and deliberately not
  deleted. Do not "fix" that condition.
- After writing the theme or the taskbar size, an Explorer restart is mandatory: the shell reads
  those values only at startup. Every write is read back and logged as requested/confirmed.
- `Thread.Sleep` + `Application.DoEvents()` in the removal, theme and taskbar paths are
  deliberate shell synchronisation, not laziness.

## Launch modes

- Mutex `Local\DesktopOverlay.SingleInstance`: a second instance exits silently. If `dotnet run`
  "does nothing", the app is most likely already running.
- Entry points live in `Program.cs` (which also holds the `ImageExtensions` and `DataDir`
  constants, the log, `LoadStableCopy` and the `TrayContext` class):
  - no arguments — utility window;
  - `--set "<path>"` — window opens with the image preselected and applies it immediately;
  - `--apply "<path>"` — autostart: overlay and tray only, no window;
  - `--revert [--light]` — revert without a window. There is no `--help`; unknown arguments are
    ignored.

## Structure

- `OverlayEngine.cs` — overlay lifecycle: watchdog (2 s), repaint timer (1 s), tray icon, host
  lookup, re-attach after an Explorer restart.
- `OverlayWindow.cs` — the surface itself: window class, `WS_CHILD` attach to the correct
  `WorkerW` (not the one holding `SHELLDLL_DefView`), `HWND_BOTTOM` Z-order, `WM_PAINT` via
  `BeginPaint`/`EndPaint`, cached cover scaling, repaint with the real system wallpaper on
  removal.
- `Native.cs` — all P/Invoke, including the `WorkerW` discovery. Add new Win32 calls here.
- `ThemeUtil` lives inside `Reverter.cs`, not in a file of its own.
- The order in `OverlayEngine.Remove()` is mandatory: repaint with the system wallpaper →
  `Sleep(150)` + `DoEvents` → `DestroyWindow` → destroy the tray → `RedrawDesktop`. Skipping the
  first two steps leaves the last painted frame frozen on screen.

## Deliberate asymmetries, easy to break

- A clipboard paste applies the image **immediately**; Browse and drag-and-drop wait for the
  "Set wallpaper" button. This is intentional (`SPEC.md` §2.2).
- The overlay covers only the primary monitor; multi-monitor spanning is out of scope.
- The log records only the first 8 paint events — otherwise the 1 s timer produces thousands of
  lines per hour.

## Diagnostics via environment variables

- `OVERLAY_DEBUG_FILL=1` — fills the surface with magenta; a DWM problem then shows up as a
  magenta rectangle.
- `OVERLAY_NO_EX_TRANSPARENT=1` — drops `WS_EX_TRANSPARENT` and `WM_NCHITTEST → HTTRANSPARENT`
  so the overlay starts receiving clicks. Needed when diagnosing desktop overlap.
- The log records the window handle, the host handle and the resulting rectangle: nearly every
  "the image does not appear" case is solved by reading `overlay.log`.

## Code comments

- Code comments are read by **an AI agent only**. Humans read `SPEC.md`.
- So a comment need not be human-readable, need not explain the obvious, and need not apologise
  for the code. It may be terse, clipped, without punctuation.
- Comment only what an agent would otherwise get wrong: an invariant that is easy to break, a
  "do not fix this" warning, the reason for odd code, step ordering, the meaning of a constant.
- Do not comment the obvious. Restating a signature or "returns the result" is noise; delete it
  while refactoring.
- Style of the port (`origin/rust-port`): a short `///` on every item, starting at the module,
  no blank lines between members. Use `src/app.rs` as the density reference.
- A comment that contradicts the code is worse than no comment.

## Documentation and language

- `README.md` (EN) and `README_RU.md` (RU) mirror each other. Fix one, fix the other: both carry
  a feature table, a CLI table, a diagnostics section and a per-file responsibility table.
- New module file → add a row to the file table in both READMEs.
- User-facing UI text and log messages are Russian (`SPEC.md` §10.9): a human reads them. Write
  new user-facing strings in Russian.
- The language of code comments is unrestricted, see "Code comments" above. Russian is what they
  currently happen to be; pick whichever wording is more precise.