# Roadmap

Where EasyQuickScreenshot is headed. Small, fast, does one thing well — every item is
weighed against that. Suggestions welcome via [issues](../../issues).

## Shipped
- Instant region capture (hotkey → crosshair → drag → PNG on disk + clipboard)
- Two modes: quick (fixed `temp.png`) and save (timestamped)
- `Ctrl+Shift+Alt+Q` — annotate: rectangle, arrow, line, circle, pen and text drawn right in
  the capture overlay (no second window), red by default; drag any shape or the whole region
  by its border to move it and by a corner to resize it; undo/redo covers every edit; the
  colour and width you picked are still set next capture; text is multi-line and can be
  re-opened by double-clicking it; `Enter` = quick, `Shift+Enter` = save
- `Ctrl+Shift+Alt+E` — open the current save folder
- No dimming, no flash, no shutter; overlay can't leak into the shot
- Multi-monitor + mixed-DPI selection
- Optional settings & gallery app (separate process — never slows a capture)
- One-line PowerShell installer

## Next
- A short demo GIF in the README
- **winget** package (Scoop is live; the winget PR needs re-filing)
- Annotate: clicking into text to place the caret, and a text size control of its own

## Considered (not committed — feedback wanted)
- A full editor window: re-crop saved shots, resize handles, blur redaction
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
