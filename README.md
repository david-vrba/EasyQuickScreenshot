<p align="center">
  <img src="assets/icon-256.png" width="110" alt="EasyQuickScreenshot">
</p>

# EasyQuickScreenshot

**Region screenshots at the speed of a keystroke. No flash, no shutter, no folder full of junk.**

A tiny (~600 KB) native Windows tray app written in Rust. Press a hotkey, your cursor becomes a crosshair, drag a rectangle — the screenshot is on disk and on your clipboard before you let go of the mouse. That's the entire experience.

---

## Why this exists

Windows' built-in `Win+Shift+S` is slow to activate, pops a toolbar you didn't ask for, and quietly dumps every capture into `Pictures\Screenshots` forever. Third-party tools bundle editors, uploaders, and accounts.

EasyQuickScreenshot does one thing: **get the pixels you're pointing at into a file and onto your clipboard, instantly.** It sits resident in the tray, so there is zero startup cost when you press the hotkey.

## The two modes

| Hotkey | Mode | What happens |
|---|---|---|
| `Ctrl+Alt+Q` | **Quick shot** | Saves to a single fixed file, `shots/temp.png`. The next quick shot **overwrites it**. One file, forever — zero folder bloat. |
| `Ctrl+Alt+E` | **Easy save** | Saves a timestamped PNG to `shots/saved/` for captures you want to keep. |

Both modes also copy the capture to the clipboard (configurable), so `Ctrl+V` works immediately.

**Q**uick and **E**asy — that's the name.

### Why a fixed temp file is a superpower

`shots/temp.png` is always the latest thing you captured, at a path that never changes. That makes it scriptable:

- Feed it to an AI agent: *"look at temp.png"* — no hunting for filenames.
- Watch it from a script and react to every new capture.
- Attach "whatever I just screenshotted" in one step, forever.

## Install

**Download** the latest `eqs.exe` from [Releases](../../releases) and put it in any writable folder — done. No installer, no runtime, no admin rights.

**Or build from source** (Rust required):

```
cargo build --release                     # core → target/release/eqs.exe
cd settings-app && cargo build --release   # optional UI → settings-app/target/release/eqs-settings.exe
```

Run `eqs.exe` — a tray icon appears and the hotkeys are live. On first run it writes a default `config.toml` next to the exe. (The settings app is optional; see below.)

**Start with Windows:**

```
pwsh scripts/autostart.ps1          # register autostart (current user)
pwsh scripts/autostart.ps1 -Remove  # unregister
```

## Using it

1. Press the hotkey. The screen freezes — no dimming, no effects — and full-screen crosshair lines mark your position. *(Prefer a plain crosshair cursor instead of the lines? Set `crosshair_style = "cursor"`.)* The capture is taken **before** the overlay appears, so the overlay can never end up in your screenshot.
2. Drag a rectangle. The border and guides invert the pixels beneath them, so they're visible on any background, with live pixel dimensions below the selection.
3. Release. The PNG is written and copied. No confirmation, no flash, no sound — check the tray tooltip if you forget your keys.

**Cancel** with `Esc` or right-click. Selections under 3×3 px are treated as accidental and discarded. Multi-monitor selections (across mixed-DPI displays) work — the overlay spans the entire virtual desktop.

## Settings & gallery (optional)

Prefer a UI over editing a text file? There's an optional companion app, **`eqs-settings.exe`** — a tiny window (built with Tauri, using the WebView2 that's already on Windows) with three tabs:

- **Settings** — edit hotkeys (just press the combo), folders, and options; saving applies to the running app instantly.
- **Gallery** — browse your saved screenshots as thumbnails, open or reveal any one.
- **About** — quick links and stats.

Put `eqs-settings.exe` next to `eqs.exe` and open it from the tray → *Settings & gallery…*. The core capture app is a completely separate process, so this never adds a millisecond to a capture — power users can ignore it entirely, and people who don't touch config files get a friendly front door.

## Config

`config.toml` lives next to `eqs.exe` (auto-created on first run). Edit it in the settings app, or by hand — tray → *Open config* to edit, tray → *Reload config* to apply without restarting.

```toml
quick_hotkey = "ctrl+alt+q"   # modifiers: ctrl, alt, shift, win
save_hotkey  = "ctrl+alt+e"   # keys: a-z, 0-9, f1-f24, printscreen, space
shots_dir    = "shots"        # relative paths resolve against this file's folder
temp_file    = "temp.png"
copy_to_clipboard = true
crosshair_style = "lines"     # "lines" = full-screen guides, cursor hidden
                              # "cursor" = plain crosshair cursor, no lines
```

## Good to know

- **Your screenshots stay yours.** Everything is local. This repo's `.gitignore` blocks `shots/` and all image files, so captures can never be committed by accident.
- **AltGr layouts** (Czech, Polish, German, …): Windows treats `AltGr` as `Ctrl+Alt`, so a hotkey like `ctrl+alt+e` also swallows `AltGr+E` (e.g. `€` on some layouts) while the app runs. If that bites you, rebind in `config.toml`.
- **Hotkey already taken?** You get one warning at startup naming the conflicting binding — rebind and hit *Reload config*.
- **What it won't capture:** UAC prompts and the lock screen (Windows forbids it), and some exclusive-fullscreen games. Windowed/borderless games are fine.
- If a capture fails you get a message box; if you see nothing, it worked. Silence is the feature.

## Contributing

Small codebase, deliberately boring architecture — [`STRUCTURE.md`](STRUCTURE.md) explains every file and invariant in five minutes. PRs welcome.

## License

[MIT](LICENSE)
