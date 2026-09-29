# Roadmap

Where EasyQuickScreenshot is headed. Small, fast, does one thing well — every item is
weighed against that. Suggestions welcome via [issues](../../issues).

## Shipped
- Instant region capture (hotkey → crosshair → drag → PNG on disk + clipboard)
- Two modes: quick (fixed `temp.png`) and save (timestamped)
- Every capture opens the editor, and the letter that opened it commits it — `Ctrl+Alt+Q` … `Q`,
  `Ctrl+Alt+E` … `E`, with `Q` and `E` staying ordinary letters while a caption is being typed
- Drawing: rectangle, arrow, line, circle, pen and text right in
  the capture overlay (no second window), red by default; drag any shape or the whole region
  by its border to move it and by a corner to resize it; undo/redo covers every edit; the
  colour and width you picked are still set next capture; text is multi-line and can be
  re-opened by double-clicking it; `F` fills a rectangle or circle solid, for blacking
  something out; the key list is behind a `?` button instead of printed under the bar;
  `Enter` = quick, `Shift+Enter` = save
- `Ctrl+Shift+Alt+Q` — focus mode: point at a window, it lights up under a sliding grey
  pane, click it and the whole window goes to the editor. No dragging, exact window edges
  (DWM frame bounds). Off switch in settings, and off means the hotkey is never registered
- `Ctrl+Shift+Alt+E` — open the current save folder
- No dimming, no flash, no shutter; overlay can't leak into the shot
- Multi-monitor + mixed-DPI selection
- Optional settings & gallery app (separate process — never slows a capture)
- One-line PowerShell installer

## Next
- A short demo GIF in the README
- `windows` 0.58 → 0.62 in the core. Every Win32 call is written against the 0.58 signatures
  and each major rewrites them (`HWND` → `Option<HWND>` and friends), so it is a migration,
  not a version bump. The settings app is already on 0.62. Dependabot is told to stop
  offering the major until this is done by hand.
- **winget** package (Scoop is live; the winget PR needs re-filing)
- Annotate: clicking into text to place the caret, and a text size control of its own

## Screen recording — shipped in v0.11.0, what comes after
- **Shipped:** `Ctrl+Alt+E` region recording from the GPU (DXGI Desktop Duplication), H.264 +
  AAC through Media Foundation, system sound, a REC bar and region frame that never reach the
  video, and an editor with trim, crop, mute and volume. `Q` / `E` save it like a screenshot.
- **Next, in order of how often it would help:** microphone as a second switch (mixed in, or
  its own track); a region that spans two monitors; pause and resume; GIF export for short
  clips; the window picker (focus mode) as a way to choose what to record.

## Considered (not committed — feedback wanted)
- A full editor window: re-crop saved shots, resize handles, blur redaction
- Configurable output format (e.g. JPG/WebP) and filename pattern
- A "copy last capture's file path" hotkey
- Single-instance guard for the settings window

## Known limitations
- A recording covers one monitor: a region crossing two is trimmed to the one its centre is on.
  Rotated (portrait) monitors cannot be recorded yet
- Can't capture UAC prompts or the lock screen (Windows forbids it) or some
  exclusive-fullscreen games — windowed/borderless games are fine
- On AltGr layouts (Czech, German, …) `Ctrl+Alt` = AltGr, so `ctrl+alt`-based hotkeys
  can collide with typed characters — rebind in `config.toml` if it bites

## Non-goals
No cloud, no accounts, no telemetry, no bloat. It stays a tiny local tool.
