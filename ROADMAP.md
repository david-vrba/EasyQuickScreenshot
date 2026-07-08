# Roadmap

Where EasyQuickScreenshot is headed. Small, fast, does one thing well — every item is
weighed against that. Suggestions welcome via [issues](../../issues).

## Shipped
- Instant region capture (hotkey → crosshair → drag → PNG on disk + clipboard)
- Two modes: quick (fixed `temp.png`) and save (timestamped)
- `Ctrl+Shift+Alt+E` — open the current save folder
- No dimming, no flash, no shutter; overlay can't leak into the shot
- Multi-monitor + mixed-DPI selection
- Optional settings & gallery app (separate process — never slows a capture)
- One-line PowerShell installer

## Next
- Tagged **v0.2.0** release with a downloadable build
- **winget** and **Scoop** packages (`winget install …`, `scoop install …`)
- A short demo GIF in the README

## Considered (not committed — feedback wanted)
- Light in-app annotation (arrow / box / blur) before saving
- Configurable output format (e.g. JPG/WebP) and filename pattern
- A "copy last capture's file path" hotkey
- Single-instance guard for the settings window

## Known limitations
- Can't capture UAC prompts or the lock screen (Windows forbids it) or some
  exclusive-fullscreen games — windowed/borderless games are fine
- On AltGr layouts (Czech, German, …) `Ctrl+Alt` = AltGr, so `ctrl+alt`-based hotkeys
  can collide with typed characters — rebind in `config.toml` if it bites

## Non-goals
No cloud, no accounts, no telemetry, no bloat. It stays a tiny local tool.
