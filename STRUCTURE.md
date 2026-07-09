# STRUCTURE.md

Architecture reference for contributors (human or AI). Read this before touching code — it is the entire mental model.

## Two processes, one config file

The project is **two separate executables** that share `config.toml` and never block each other:

- **`eqs.exe`** — the capture engine (this document's main subject). Tiny, resident, no runtime. Owns the hotkeys, overlay, and file writes. **Nothing may be added here that slows a capture.**
- **`eqs-settings.exe`** — an optional Tauri companion (`settings-app/`) for editing settings, browsing the gallery, and an about/dashboard view. Launched on demand from the tray; a normal person never needs it, so the fast core never carries its weight.

They coordinate through the filesystem plus one Win32 message: the tray menu spawns the settings app with `--config <path>`; after the settings app writes `config.toml` it posts `WM_APP+2` (`WM_EQS_RELOAD`) to the core's hidden `EQS_MAIN` window, which hot-reloads live. See `settings-app/` below.

## What this program is (the core, `eqs.exe`)

A single resident Win32 process. One hidden window owns a tray icon and three global hotkeys: two run a synchronous capture flow (quick / save), the third just opens the current save folder in Explorer (no capture). A capture press runs one synchronous flow and returns to the message loop. There are no threads, no async, no state between captures.

```
hotkey pressed
  └─ capture.rs   grab entire virtual desktop → BGRA buffer   (screen is read ONCE)
  └─ overlay.rs   fullscreen topmost window paints that frozen buffer at FULL brightness
                  (no dimming — a deliberate speed/clarity decision); crosshair guides and
                  the drag border are drawn with R2_NOT pixel inversion so they read on any
                  background; user drags a rectangle; returns rect or None (cancelled)
  └─ capture.rs   crop() slices the SAME buffer — overlay pixels can never leak in
  └─ save.rs      encode PNG → write <path>.png.tmp → rename (atomic-ish)
  └─ clipboard.rs copy crop as CF_DIB (best-effort, silent on failure)
```

The `crosshair_style` setting picks exactly one position marker: `"lines"` (full-screen
guides, mouse cursor hidden via `WM_SETCURSOR`) or `"cursor"` (class cross cursor, no lines).

## Files

| File | Owns |
|---|---|
| `src/main.rs` | Entry point, single-instance mutex, DPI awareness, hidden window + message loop, hotkey registration (quick / save / open-folder), tray-menu commands (launch settings, open shots/config, reload), the capture flow glue, headless `--shoot` and `--render-test` hooks |
| `src/config.rs` | `config.toml` discovery/parsing, defaults, hotkey-string → `(modifiers, vk)` parsing |
| `src/capture.rs` | `Screenshot` (BGRA buffer + geometry), `capture_virtual_screen()` via GDI BitBlt, `crop()` |
| `src/overlay.rs` | Selection UI: window class, nested message loop, GDI double-buffered painting, mouse/keyboard handling |
| `src/save.rs` | PNG encode (`png` crate, fast compression), atomic writes, timestamped filenames |
| `src/clipboard.rs` | CF_DIB clipboard writer with retry (clipboard can be locked by other apps) |
| `src/tray.rs` | Tray icon add/remove (brand icon embedded via `include_bytes!`), right-click menu |
| `build.rs` | Embeds `assets/icon.ico` into the exe (skips gracefully if no resource compiler) |
| `assets/` | Brand icon: `ducky.ico` (master art) + `gen_icon.py`, which derives the square multi-size `icon.ico` / `icon-256.png` / `icon-64.png` the exe and tray embed |
| `scripts/autostart.ps1` | HKCU Run-key register/unregister |
| `scripts/e2e-test.ps1` | Injected-input end-to-end test against a running instance |

### `settings-app/` — the Tauri companion (`eqs-settings.exe`)

Separate crate, separate `target/`, own build. Vanilla HTML/CSS/JS frontend (no npm build step — `withGlobalTauri` exposes `invoke`), rendered by the OS WebView2. All logic is Rust commands:

| File | Owns |
|---|---|
| `src/main.rs` | Tauri builder + command handlers (`load_config`, `save_config`, `gallery_stats`, `gallery_list`, `pick_shots_folder`, `open_path`/`reveal_path`/`open_url`) |
| `src/config_io.rs` | Reads/writes the shared `config.toml` via `toml_edit` (comments survive); mirrors the core's config discovery + hotkey grammar so it never saves something the core can't register |
| `src/gallery.rs` | Lists `saved/`, builds base64 PNG thumbnails (`image` crate) — self-contained, no asset-protocol scope needed |
| `src/tray_signal.rs` | `FindWindow("EQS_MAIN")` + `PostMessage(WM_APP+2)` to hot-reload the running core after a save |
| `ui/` | `index.html` / `styles.css` / `app.js` — Settings, Gallery, About tabs; the hotkey fields capture a live key-combo press |
| `tauri.conf.json`, `capabilities/default.json` | Window + CSP config; permissions (`core:default`, `dialog`, `opener`) |
| `icons/` | Tauri icon set derived from the ducky |

### Project meta (not code)

| Path | Purpose |
|---|---|
| `scripts/install.ps1` | One-line installer (`irm … \| iex`) — pulls the latest release exe(s) into the user folder |
| `.github/workflows/ci.yml` | CI — builds core + settings app + advisory clippy on every push/PR |
| `.github/workflows/release.yml` | CD — a `v*` tag builds both exes and publishes the release with checksums |
| `.github/dependabot.yml` | Weekly cargo + GitHub-Actions dependency updates |
| `SECURITY.md` / `ROADMAP.md` | Security policy (local-only, private reporting) + public roadmap |
| `bucket/eqs.json` | Scoop manifest — the repo doubles as a Scoop bucket |
| `packaging/winget/` | winget manifests (portable) submitted to microsoft/winget-pkgs |

## Invariants — do not break these

1. **One capture per flow.** The screen is read once into `Screenshot.pixels` (top-down BGRA, 4 bytes/px). Overlay preview AND final crop both come from this buffer. Never re-grab the screen after the overlay closes — the overlay would be in the shot and the content may have changed.
2. **Two coordinate spaces.** Buffer coords (0,0 = top-left of buffer) vs virtual-screen coords (can be negative — monitors left/above primary). `Screenshot.origin_x/y` maps between them. The overlay window is positioned at the virtual-screen origin so its client coords == buffer coords. `--shoot` takes virtual-screen coords and converts.
3. **Writes are atomic.** Always `*.png.tmp` + rename (`save::write_png_atomic`). External watchers/agents read `temp.png`; they must never see a half-written file. (Rust's `rename` on Windows replaces the destination.)
4. **Screenshots never enter git.** `.gitignore` blocks `/shots` and `*.png` globally. Any new output path must be added there too.
5. **No feedback on success.** No sound, no flash, no toast. Failure → message box. This is a product decision, not an omission.
6. **Re-entrancy guard.** `IN_CAPTURE` (main.rs) drops hotkey presses while an overlay is open — the nested message loop would otherwise re-enter the flow.
7. **Per-Monitor-V2 DPI awareness** is set at startup, so every coordinate in the process is a physical pixel. Never add DPI scaling math.

## Testing without a human

- `eqs.exe --shoot X Y W H out.png` — headless capture of a virtual-screen rect (no overlay). Exit codes: 0 ok, 2 bad args, 3 capture failed, 4 empty crop, 5 write failed.
- `eqs.exe --render-test SX SY W H lines|cursor out.png` — composes one real overlay frame (guides + selection border) over a live capture with no window, so the drawing code is verifiable pixel-for-pixel from a screenshot diff. Same exit-code scheme.
- `eqs.exe --config path.toml` — run against a throwaway config (isolated shots dir, clipboard off).
- Full e2e: `pwsh scripts/e2e-test.ps1` (or `-Key E` for save mode) against a running instance started with a throwaway `--config` — it injects the hotkey + a drag and the output file should appear. It moves the real mouse briefly.

## Build

```
cargo build              # debug
cargo build --release    # LTO, stripped, ~600 KB
```

Builds on stable Rust with the GNU **or** MSVC toolchain; no build scripts, no vendored assets. The only dependencies are `windows` (Win32 bindings), `png`, `serde` + `toml`.

## Roadmap

The live roadmap (shipped / next / considered) is [`ROADMAP.md`](ROADMAP.md). Architecture-facing notes:

- The settings/gallery companion (**Tauri**) is **shipped** — see `settings-app/` above.
- Linux/macOS ports would only touch `capture` / `overlay` / `clipboard` / `tray`; `main` / `config` / `save` are already portable.
- Freeze-free live mode (skip the frozen snapshot) and optional JPEG/WebP output remain open ideas.
