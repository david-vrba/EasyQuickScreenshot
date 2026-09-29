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

## The modes

| Hotkey | Mode | What happens |
|---|---|---|
| `Ctrl+Alt+Q` | **Screenshot** | Drag a region and the editor opens. `Q` saves to one fixed file, `shots/temp.png`, which the next one **overwrites** — zero folder bloat. `E` keeps a timestamped PNG in `shots/saved/` instead. |
| `Ctrl+Shift+Alt+Q` | **Focus mode** | No dragging at all. Point at a window, it lights up, click it — the whole window goes to the editor. |
| `Ctrl+Alt+E` | **Record** | Drag a region and recording starts. Press it again to stop, then trim, crop and set the volume. `Q` saves `videos/temp.mp4`, `E` keeps a timestamped copy. |

**`Q` saves the quick file, `E` keeps a copy** — for screenshots and recordings alike. `Enter`
and `Shift+Enter` do the same two things. While you are typing a caption, `Q` and `E` are just
letters — nothing is saved.

Every capture also goes to the clipboard (configurable), so `Ctrl+V` works immediately.

**Q**uick and **E**asy — that's the name.

There's also `Ctrl+Shift+Alt+E`, which just **opens your saved-screenshots folder** in Explorer — no capture — and `Ctrl+Shift+Alt+V`, which opens the videos folder. Both always open wherever the folder currently points, so they stay correct after you change it in settings.

## Screen recording — `Ctrl+Alt+E`

1. Drag a region, exactly like a screenshot. Recording starts the moment you let go.
2. A small bar sits next to the region — a pulsing dot, the elapsed time, and a stop square —
   with a thin red frame around what is being recorded. **Neither appears in the video.**
3. Press `Ctrl+Alt+E` again, or click the bar. The editor opens on the recording:
   - **Trim** — drag the two gold handles on the timeline, or press `I` and `O` to set the start
     and end at the playhead. Playback stops at the end of the trim, so what plays is what saves.
   - **Crop** — drag the corners of the box over the picture; drag inside it to move it.
   - **Sound** — mute, or a volume slider from 0 to 200%.
   - `Q` saves `videos/temp.mp4`, `E` keeps a timestamped copy in `videos/saved/`, `Esc` throws it
     away. The saved video is also put on the clipboard as a file, so it pastes straight into a
     chat or a folder.

It records whatever the PC is playing (turn that off in settings), at 30 fps by default. Frames
come from the GPU through DXGI Desktop Duplication and are encoded to H.264 by the hardware
encoder when there is one — the screenshot path reads a 1080p region in 29 ms, which cannot keep
up with 30 fps, so recording does not use it. No edits means the file is simply moved into
place; any edit re-encodes it.

Limits for now: one monitor per recording (a region is trimmed to the monitor its centre is
on), no microphone, no pause.

## Focus mode — capture a window without drawing a box

Most captures are "that window, all of it". Dragging a rectangle around a window is work you
should not have to do, and the edges are never quite right.

Press `Ctrl+Shift+Alt+Q` and there is no rectangle to draw. The pointer becomes a hand,
and the window underneath it is covered by a soft grey pane with a white border, sitting
just inside its edges so a strip of the window itself still shows all the way round. Move
the mouse and the pane **slides** to the next window, the way the Windows snap preview does.
Click, and that window — exactly its own edges, not a guess — opens in the editor, where
`Enter` writes `temp.png` and `Shift+Enter` keeps a timestamped copy.

Over bare desktop the pane covers the whole monitor you are on, so there is no dead spot
where nothing happens. `Esc` or right-click cancels.

The window list is taken at the instant you press the hotkey, before the overlay exists —
the overlay spans every monitor, so once it is up it is the only window any point is over.
Edges come from the DWM frame bounds rather than `GetWindowRect`, which reports several
invisible pixels of resize border and would put the pane slightly wide of the window.

Turn it off in the settings app, or with `window_pick = false`. Off means the hotkey is
never registered, so the key stays free for something else.

## Annotating

Every capture ends here. The frozen region is held with a small toolbar under it — draw on
it, or don't, then press the letter you started with.

| | |
|---|---|
| `?` on the bar, or `F1` | the whole key list, as a panel. Click anywhere to dismiss it |
| `R` `A` `L` `C` `P` | rectangle · arrow · line · circle · pen |
| `F` | fill — the rectangle or circle you **just drew** turns solid, and so does the next one. Pick black, drag over a password, press `F`, gone. Off again each capture |
| `T` | text — click where it goes and type. `Enter` starts a new line; `Ctrl+Enter`, or a click anywhere else, places it |
| double-click text | reopen placed text and edit it — it goes back in the same spot, same order |
| while typing | `←` `→` `↑` `↓` `Home` `End` move the caret · `Del` deletes forward · `Ctrl+A` select all · `Ctrl+C` / `Ctrl+X` / `Ctrl+V` copy, cut, paste · `Esc` throws the text away |
| `1`–`8` | colour — recolours the shape you just drew and everything after it. **Red is the default**, and whatever you pick is still there next capture |
| mouse wheel | stroke width / text size — same: the shape you just drew, and the next ones. Also remembered |
| hold `Shift` | perfect square / circle, or snap a line to 45° |
| drag a border | move a shape by its outline, or move the whole region by its edge |
| drag a corner | resize it — the opposite corner stays put. Works on shapes and on the capture region |
| `Ctrl+Z` or right-click | undo — covers moves and resizes, not just drawing |
| `Ctrl+Y` or `Ctrl+Shift+Z` | redo |
| `Q`, `Enter`, or **QUICK** | write `shots/temp.png` — the fixed file |
| `E`, `Shift+Enter`, or **SAVE** | write a timestamped copy in `shots/saved/` |
| `Esc` | cancel the current shape, again to abort |

The toolbar sits centred under your selection, flips above it near the bottom, and drops
just inside the top edge when neither fits — which is what a full-screen capture leaves.
It is placed against the monitor you are capturing, not the whole desktop, so it stays
reachable on a multi-monitor setup where the screens are different heights. It is drawn in
the preview only, so wherever it sits it is never in the file.

Nothing else is printed on screen. The keys live behind the `?` button on the bar, because
a list you read once and then stare at forever is just clutter. The one exception is while
typing, where `Ctrl+Enter` to finish is the only key nobody guesses.

Every tool is also a button on the toolbar, so you never have to remember a key. Shapes are
drawn stroke-only — nothing is ever filled in over your screenshot.

Hover a drawn shape and small white grips appear on it. Drag a grip to resize, drag anywhere
else on the outline to move the whole thing. The capture region itself works the same way: its
four corners resize the shot and its edges slide it, so a selection that came out slightly
wrong is fixed in place instead of started over. Grips are preview only — they never reach
the file.

Text is multi-line and stays editable. Double-click any text you already placed to open it
again, with the toolbar switching to that text's own colour and size. `Esc` while editing
leaves the original exactly as it was.

It opens **inside the capture overlay**: no second window, no app launch, nothing to wait for.
A plain `Ctrl+Alt+Q` is untouched and never shows the toolbar.

### Why a fixed temp file is a superpower

`shots/temp.png` is always the latest thing you captured, at a path that never changes. That makes it scriptable:

- Feed it to an AI agent: *"look at temp.png"* — no hunting for filenames.
- Watch it from a script and react to every new capture.
- Attach "whatever I just screenshotted" in one step, forever.

## Install

**Requirements:** Windows 10 or 11 · 64-bit. No runtime, no admin, no account. *(The optional settings app also uses WebView2 — already present on Windows 11 and most Windows 10.)*

**One line** — PowerShell fetches the latest build and starts it:

```powershell
irm https://raw.githubusercontent.com/david-vrba/EasyQuickScreenshot/main/scripts/install.ps1 | iex
```

Installs to your user folder (no admin), launches it, and prints the hotkeys. Add `-Autostart` to also start it on login.

**Or grab it yourself:** download `eqs.exe` from [Releases](../../releases), drop it in any writable folder, run it. No installer.

**Package managers:**
- **Scoop** — `scoop bucket add eqs https://github.com/david-vrba/EasyQuickScreenshot` then `scoop install eqs/eqs`
- **winget** — `winget install DavidVrba.EasyQuickScreenshot` *(pending review in the Microsoft catalog)*

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
3. Release. The editor opens on the frozen region. Press `Q` (or `E`) and the PNG is written and copied — no confirmation, no flash, no sound. Draw on it first if you want to; most of the time you won't.

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
quick_hotkey  = "ctrl+alt+q"        # modifiers: ctrl, alt, shift, win
window_pick   = true                # focus mode: pick a whole window, no dragging
window_hotkey = "ctrl+shift+alt+q"  # keys: a-z, 0-9, f1-f24, printscreen, space
record_hotkey = "ctrl+alt+e"        # start / stop a screen recording
record_fps    = 30                  # 10 to 60
record_audio  = "system"            # "system" = what the PC plays, "none" = silent
folder_hotkey = "ctrl+shift+alt+e"  # opens the save folder in Explorer (no capture)
videos_folder_hotkey = "ctrl+shift+alt+v"
shots_dir     = "shots"             # relative paths resolve against this file's folder
videos_dir    = "videos"
temp_file    = "temp.png"
copy_to_clipboard = true
crosshair_style = "lines"     # "lines" = full-screen guides, cursor hidden
                              # "cursor" = plain crosshair cursor, no lines
```

## Good to know

- **Your screenshots and recordings stay yours.** Everything is local. This repo's `.gitignore` blocks `shots/`, `videos/`, and every image and video format, so a capture can never be committed by accident.
- **AltGr layouts** (Czech, Polish, German, …): Windows treats `AltGr` as `Ctrl+Alt`, so a hotkey like `ctrl+alt+e` also swallows `AltGr+E` (e.g. `€` on some layouts) while the app runs. If that bites you, rebind in `config.toml`.
- **Hotkey already taken?** You get one warning at startup naming the conflicting binding — rebind and hit *Reload config*.
- **What it won't capture:** UAC prompts and the lock screen (Windows forbids it), and some exclusive-fullscreen games. Windowed/borderless games are fine.
- If a capture fails you get a message box; if you see nothing, it worked. Silence is the feature.

**Changes land on the shape you just drew** — as long as you have not switched tools since.
Draw a rectangle, press `F`, and that rectangle fills. Switch to the arrow first and pick blue,
and only the next arrow is blue; the rectangle stays as it was. Rolling the wheel through
five sizes is one `Ctrl+Z` back to where you started, not five.

**Slow to start?** Set `timing_log = true` in `config.toml` and every capture adds a line to
`eqs-timing.log` next to the exe: how long the press waited, the screen grab, the window, and
the first frame. It is written after the overlay closes, so measuring costs the capture nothing.
`eqs.exe --time-startup` runs the same measurement without a hotkey.

If the tray icon ever disappears on its own, look for **`eqs-panic.log`** next to `eqs.exe`.
A crash writes the reason and the exact line there before the process dies.

## Contributing

Small codebase, deliberately boring architecture — [`STRUCTURE.md`](STRUCTURE.md) explains every file and invariant in five minutes. PRs welcome.

## License

[MIT](LICENSE)
