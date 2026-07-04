# STRUCTURE.md

Architecture reference for contributors (human or AI). Read this before touching code — it is the entire mental model.

## What this program is

A single resident Win32 process. One hidden window owns a tray icon and two global hotkeys. A hotkey press runs one synchronous capture flow and returns to the message loop. There are no threads, no async, no state between captures.

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
| `src/main.rs` | Entry point, single-instance mutex, DPI awareness, hidden window + message loop, hotkey registration, tray-menu commands, the capture flow glue, headless `--shoot` test hook |
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
- `eqs.exe --config path.toml` — run against a throwaway config (isolated shots dir, clipboard off).
- Full e2e: `pwsh scripts/e2e-test.ps1` (or `-Key E` for save mode) against a running instance started with a throwaway `--config` — it injects the hotkey + a drag and the output file should appear. It moves the real mouse briefly.

## Build

```
cargo build              # debug
cargo build --release    # LTO, stripped, ~600 KB
```

Builds on stable Rust with the GNU **or** MSVC toolchain; no build scripts, no vendored assets. The only dependencies are `windows` (Win32 bindings), `png`, `serde` + `toml`.

## Roadmap candidates

- Settings/dashboard/gallery companion app — **Tauri** (decided, not yet built); separate process launched from the tray so the capture engine's speed is untouched
- Optional JPEG output for very large captures
- `--last` CLI flag printing the temp-file path (for scripts)
- Freeze-free live mode (skip the frozen snapshot, capture after selection)
- Linux/macOS ports (the module boundaries were chosen so only `capture`/`overlay`/`clipboard`/`tray` are OS-specific)
