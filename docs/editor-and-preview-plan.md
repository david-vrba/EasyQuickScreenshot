# EasyQuickScreenshot — Annotation & Capture-FX plan

Status: **Epic 1 BUILT 2026-08-31.** Epics 2–3 are still design only.
Owner: David. Drafted 2026-07-09 · Rev 2 2026-07-27 (quick-annotate flow) · **Rev 3 2026-08-31 (Epic 1 shipped)**.

> **What actually got built** — `src/annotate.rs` + the annotate phase in `src/overlay.rs`.
> Matches this plan except where reality was simpler or better:
> - **No dirty-rect / layer cache.** The overlay already full-blits every frame and always has;
>   shapes are redrawn each frame and cost nothing measurable. The optimisation was solving a
>   problem that did not exist.
> - **Mouse wheel, not `[` / `]`, for stroke width.** Those two keys sit on different physical
>   keys per layout; the wheel is layout-proof.
> - **No rounded rectangle corners** (open question 7 was never answered — sharp only for now).
> - **No `Ctrl+E` escalation yet** — there is no Epic 2 editor to escalate to.
> - Right-click also undoes, which the plan did not call for but the phase-one reflex expects.

Three future epics, in order:
1. **Quick Annotate** — in-overlay red-first annotation bar (rectangle, arrow, line, circle, pen).
   The 90% case. No new window, no app launch.
2. **Full editor** — crop, re-editable shapes with handles, export formats. The 10% case.
3. **"Fancy mode"** capture animation + docked draggable preview. Optional, off by default.

Design north star (unchanged): **fast, tiny, intuitive, bulletproof.**
"Very very easy" is a hard requirement, not a nice-to-have. The plain capture path
(`Ctrl+Alt+Q` / `Ctrl+Alt+E`) **never** gets slower and **never** changes behaviour.

> Legend: **[David]** = his stated requirement · **[Rec]** = my recommendation to accept/reject
> during refinement · **[?]** = open question that needs his decision before build.

---

## 0. The tier model (the central design decision)

The earlier draft had one answer: "capture, then launch an editor window." That is the **wrong
default** for what David actually described — *"very easy, very quick, very convenient, very
optimized for performance."* Launching a WebView2 window costs ~300–800 ms of cold start and
throws a second app at you for what is usually a 3-second job: draw one red rectangle, done.

So the plan splits into two tiers with a clean escalation path between them:

| | **Tier 1 — Quick Annotate** | **Tier 2 — Full editor** |
|---|---|---|
| Where | **Inside the capture overlay** (native, GDI+) | Separate Tauri/canvas window |
| Opens in | ~0 ms (overlay is already up) | ~300–800 ms (WebView2) |
| Tools | rectangle · arrow · line · circle · **pen** | + crop · handles · re-edit · text/blur later |
| Editing | draw-and-go + undo (no handles) | full re-editable vector objects |
| Output | Enter → writes the file + clipboard | Save / Save-as-copy / Export-as / Copy |
| Use | 90% of annotations | when you need to fix, crop, or export a format |
| Reached by | `Ctrl+Alt+Shift+Q` (one new global) | from the Tier-1 bar (`Ctrl+E`) or the gallery |

**DECIDED 2026-07-27 [David]: accepted — the overlay bar is the main event, the editor is the
fallback behind `Ctrl+E` with no global hotkey of its own.**

**Ship Tier 1 first and ship it alone if we have to.** It is the feature David described;
Tier 2 is the safety net behind it. Tier 1 also has a strategic bonus: the **crop tool disappears
from the critical path**, because in EQS the selection *is* the crop — you already dragged the
exact region. Crop only matters for re-cropping later, which is a Tier-2 job.

---

## Epic 1 — Quick Annotate (Tier 1)

### 1.1 How it opens — the trigger

**DECIDED 2026-07-27: exactly ONE new global hotkey.** `Ctrl+Alt+Shift+Q` = "capture, and let me
draw on it." Where the file lands is chosen **at commit time**, not up front.

| Hotkey | Behaviour |
|---|---|
| `Ctrl+Alt+Q` | quick shot → `temp.png` + clipboard. **Unchanged. Instant. No bar.** |
| `Ctrl+Alt+E` | save shot → timestamped PNG + clipboard. **Unchanged. Instant. No bar.** |
| `Ctrl+Alt+Shift+E` | open the current save folder. **Unchanged** (existing folder hotkey). |
| **`Ctrl+Alt+Shift+Q`** | **NEW — the only addition.** select region → **annotate bar** → commit |

Then, inside the bar:

| Commit | Writes |
|---|---|
| `Enter` | `shots/temp.png` (overwritten — the agent-feedable fixed path) + clipboard |
| `Shift+Enter` | `shots/saved/<timestamp>.png` (a keeper) + clipboard |

**Why one hotkey and not two.** Three reasons, and they all point the same way:

1. ⚠️ **The obvious second combo is already taken.** `Ctrl+Alt+Shift+E` ships as the
   open-save-folder hotkey (`src/config.rs:22`), so a symmetric Q/E pair is impossible without
   moving a hotkey David already uses.
2. **`Ctrl+Alt` = `AltGr` on the Czech layout.** Every new `Ctrl+Alt+…` binding swallows a
   character system-wide (that's why `AltGr+E` / € is already gone). Adding one is a cost; adding
   two is twice the cost for no extra capability.
3. **The decision belongs at the end, not the start.** With a plain capture you must decide up
   front because there's no UI. With annotate you're already stopping to draw — and you only
   really know whether a shot is a throwaway or a keeper *after* you've marked it up. Moving the
   choice to commit time is strictly better information.

So: **4 global hotkeys total** (quick, save, folder, annotate) instead of 5 — one fewer than the
previous draft, with the same capability.

- The bar carries **two commit buttons**: `✓` (temp) and `✓+` (save a keeper), so it's fully
  mouse-driveable and the keyboard shortcut is discoverable from the tooltip.
- **[Rec] Optional second global, off by default.** `annotate_save_hotkey` exists in
  `config.toml` but ships **empty = disabled**. Anyone who wants a dedicated "annotate → keeper"
  key can set one; **[Rec] `Ctrl+Alt+Shift+S`** (*S for Save*) is the suggested value if David
  later decides he wants it after all. Nothing breaks either way.
- **[Rec] No global hotkey for the full editor.** The editor is the rare case; reach it from the
  annotate bar (`Ctrl+E` / the "⋯" button) or the gallery's "Edit" action. This keeps the global
  namespace clean.
- **[?] Optional accelerator — "Shift on release."** During a *plain* `Ctrl+Alt+Q`, if you're
  still holding `Shift` when you release the mouse button, drop into annotate mode instead of
  saving immediately. Means you never have to decide before the shot. Cheap to add, slightly
  magic. Rec: build it, ship it OFF, promote it if it feels good.
- **[?] AltGr caveat.** `Ctrl+Alt` = `AltGr` on the Czech layout. `AltGr+Shift+Q` / `AltGr+Shift+A`
  need a 10-second check that they don't type a character. Rebindable regardless.

### 1.2 The flow, end to end

```
Ctrl+Alt+Shift+Q
      │
      ▼
[SELECT]  crosshair + guides (exactly as today, no dimming)
      │  drag ─ release
      ▼
[ANNOTATE]  screen frozen · selection outlined · toolbar floats just below the selection
      │     • rectangle tool is pre-armed, colour = red, width = last used
      │     • draw · draw · draw   (Ctrl+Z undoes, right-click undoes)
      │     • switch tool: click the bar, or press R / A / L / C / P
      │     • switch colour: click a swatch, or press 1–8   (1 = red = default)
      │     • width: [ and ]        • Shift = constrain (true circle / 45° line)
      │
      ├── Enter ───────────▶  flatten → temp.png + clipboard → overlay closes      ✅ done
      ├── Shift+Enter ─────▶  flatten → shots/saved/<timestamp>.png + clipboard    ✅ keeper
      ├── Ctrl+E ──────────▶  hand off to the full editor (Tier 2) with shapes intact
      └── Esc ─────────────▶  1st Esc = cancel the in-progress shape
                              2nd Esc = abort the whole capture, nothing written
```

**The important property:** from hotkey to saved annotated screenshot is **one uninterrupted
gesture with zero window launches**. No app appears. No taskbar flicker. No focus steal.

### 1.3 The quick menu (toolbar)

**[David]** A quick menu for red lines / red arrows / red rectangles / red pen, plus other
colours, red as the default.

- **[Rec] It is drawn *into the overlay*, not a separate HWND.** No extra window to create,
  position, z-order, or DPI-scale; hit-testing is a list of rects. Fastest possible.
- **Placement:** floats directly **below the selection**; if there's no room, above it; if neither,
  inside the top-left of the selection. Always clamped to the monitor holding the selection.
- **Look:** rounded dark pill, subtle shadow, GDI+ anti-aliased. **[Rec] icons drawn as GDI+
  vector paths — zero image assets, crisp at every DPI, adds ~0 KB.**
- **Layout (left → right):**

```
┌────────────────────────────────────────────────────────────────────────────┐
│  ▭  ↗  ╱  ○  ✎  │  ● ● ● ● ● ● ● ●  │  ▁ ▃ ▅  │  ↶ ↷  │  ⋯  │  ✓   ✓+    │
│  R  A  L  C  P  │  1 2 3 4 5 6 7 8  │  [   ]  │       │     │ Enter Shift │
└────────────────────────────────────────────────────────────────────────────┘
   tools            colour swatches     width     undo/   full   commit:
                    (1 = red, default)            redo    editor temp / keeper
```

- Every button shows its key in the tooltip, so the bar teaches the keyboard flow.
- **[Rec]** Last-used tool, colour and width persist for the session, and optionally into
  `config.toml` (`last_color`, `last_width`, `last_tool`) so day two starts where day one ended.

### 1.4 Tools

**Hard rules [David], unchanged:** every shape is **border only, never filled**, and
**geometrically correct** (a circle is a true circle, a line is a true straight line).

| Tool | Key | Behaviour |
|---|---|---|
| **Rectangle** | `R` | drag corner→corner. `Shift` = perfect square. **[?]** rounded↔sharp corners toggle — Tier 1 or Tier 2 only? (Rec: one global toggle in settings, no per-shape UI in the quick bar.) |
| **Arrow** | `A` | drag tail→head. `Shift` = snap 0/45/90°. Modern tapered shaft + clean head (see 1.5). |
| **Line** | `L` | drag end→end. `Shift` = snap 0/45/90°. |
| **Circle / ellipse** | `C` | drag a bounding box. `Shift` = perfect circle. **[?]** draw from centre instead of corner when holding `Alt`? (Rec: yes, it's the standard.) |
| **Pen (freehand)** | `P` | **[David — NEW this round]** free drawing. Captured as a point list, smoothed (Catmull-Rom → Bézier), round caps + joins so it doesn't look like MS Paint. |

**Default tool = rectangle** (the archetypal "look at this" annotation), then last-used wins.
**Default colour = red** [David], always — even after a session used blue, `1`/red stays the
first swatch.

**Not in Tier 1 — DECIDED 2026-07-27 [David]: draw-and-go + undo, no handles.** No selection, no
moving or resizing a shape after it's drawn, no text, no blur. `Ctrl+Z` and redraw beats any
handle UI for a 3-second job, and it's what keeps this tier tiny and instant. Need to fiddle?
`Ctrl+E` escalates to Tier 2.

### 1.5 Arrows — "not the ugly one"

**[David]** Recognisable as an arrow, but a better, modern design.
**[Rec]** Tapered shaft + a clean, slightly concave triangular head; head size scales with stroke
width; stroke colour only. Reference set to mimic: **Excalidraw**, **tldraw**, **Lucide/Tabler**
arrow icons. Straight only in Tier 1; curved arrows are a Tier-2 nicety.
**[?]** One style, or two presets (solid head vs line/chevron head)? Rec: one, done well.

### 1.6 Colour palette

Red first, seven others. **[Rec]** high-contrast, screenshot-legible values (Apple system colours
— already tested against light *and* dark UI):

| Key | Colour | Hex |
|---|---|---|
| `1` | **Red (default)** | `#FF3B30` |
| `2` | Orange | `#FF9500` |
| `3` | Yellow | `#FFCC00` |
| `4` | Green | `#34C759` |
| `5` | Blue | `#007AFF` |
| `6` | Purple | `#AF52DE` |
| `7` | Black | `#000000` |
| `8` | White | `#FFFFFF` |

Editable in `config.toml` (`[annotate] palette = [...]`) for anyone who wants their own eight.
Stroke width presets **2 / 4 / 6 px** at 100% DPI, DPI-scaled, default **4**, `[` / `]` to change.

### 1.7 Keyboard map (complete)

| Key | Action |
|---|---|
| `R` `A` `L` `C` `P` | rectangle · arrow · line · circle · pen |
| `1`–`8` | colour |
| `[` `]` | stroke width down / up |
| `Shift` (held) | constrain — square / circle / 0-45-90° |
| `Alt` (held) | draw from centre (rect/circle) **[?]** |
| `Ctrl+Z` / `Ctrl+Y` | undo / redo |
| right-click | undo last shape (mouse-only undo) **[?]** |
| `Enter` | commit → `temp.png` + clipboard → close |
| `Shift+Enter` | commit → timestamped keeper in `shots/saved/` + clipboard → close |
| `Ctrl+E` | escalate to the full editor, shapes intact |
| `Esc` | cancel in-progress shape → (again) abort capture |

Everything is also reachable by mouse from the bar. Keyboard is the accelerator, never the
requirement.

### 1.8 Performance plan (the "very optimized" requirement, concretely)

This is the part that decides whether it *feels* like EQS. Budget and the techniques to hit it:

| Concern | Plan |
|---|---|
| **Plain capture stays instant** | Annotate is a **separate state** entered only from the Shift hotkeys. Zero new branches on the `Ctrl+Alt+Q` hot path — same code, same timings. Non-negotiable invariant. |
| **No process launch** | Tier 1 never starts a process, never touches WebView2, never allocates a window. The overlay is already up and already holds the frozen screen bitmap. |
| **Redraw cost** | **Dirty-rect only.** Repaint the union of the previous and current shape bounds, not the screen. Typical: a few thousand pixels — sub-millisecond. |
| **Many shapes** | **Layer caching:** each finished shape is composited once into an *annotation layer* bitmap. A frame = blit frozen screen → blit annotation layer → draw the single in-progress shape. **O(1) regardless of shape count.** Undo re-rasterises the vector list (rare, cheap). |
| **Multi-monitor** | The overlay already spans all monitors (verified 6400×1440). Work is scoped to the selection rect, so a triple-monitor setup costs the same as one. |
| **Allocation** | Reuse the memory DC and buffers; zero heap allocation per `WM_MOUSEMOVE`. Target a steady 60 fps while dragging. |
| **Quality** | **GDI+** for the annotation layer (anti-aliasing, round caps/joins, Bézier) — lazily initialised **only** when annotate mode is first entered (~1–3 ms, off the fast path). Plain GDI stays the engine for capture. |
| **Binary size** | GDI+ is a system DLL; the module should add roughly **40–60 KB**. `eqs.exe` stays well under 1 MB. |
| **Encode** | Commit uses the existing flatten → PNG → atomic tmp+rename path. Nothing new. |

### 1.9 Where the file lands

Chosen at commit time (see 1.1), so one hotkey covers both destinations:

- `Enter` → `shots/temp.png` (overwritten, the agent-feedable fixed path) + clipboard.
- `Shift+Enter` → `shots/saved/<timestamp>.png` + clipboard.
- **[Rec]** The file is written **once, on commit, already flattened** — no un-annotated version
  is written first. That avoids the surprise of an agent reading `temp.png` mid-annotation.
  **[?] Confirm** — the alternative (write immediately, overwrite on commit) is more crash-proof
  but can hand out a half-finished image.

### 1.10 Build phasing (when we DO build)

1. **State machine + bar** — new hotkeys → annotate state, frozen buffer, toolbar drawn + hit-tested, commit/abort. *No tools yet.*
2. **Rectangle + line + colour + width + undo** — proves the render loop, dirty rects, layer cache.
3. **Arrow (the good one) + circle.**
4. **Pen** (smoothing).
5. **Polish** — Shift/Alt constraints, persistence of last style, DPI + multi-monitor placement, `Ctrl+E` escalation stub.

---

## Epic 2 — Full editor (Tier 2)

Unchanged in intent from Rev 1, but **demoted**: it is the fallback for the rare case, not the
main event. Build after Tier 1 is in daily use, so its scope is decided by what Tier 1 actually
turns out to be missing.

- **Opens from:** the Tier-1 bar (`Ctrl+E` / "⋯"), or the gallery's "Edit" on any saved shot.
  **No global hotkey.**
- **Architecture [Rec]:** a second window/mode of the existing Tauri app
  (`eqs-settings.exe --edit <path>`), HTML `<canvas>`/SVG, vector overlay on the raster image,
  flatten on export. Avoids a third binary. Needs the **single-instance guard** the settings app
  still lacks.
- **What it adds over Tier 1:**
  - **Crop** — drag a crop rect with handles, live dimensions, Enter confirms, Esc cancels.
    (Only needed for *re*-cropping; the capture selection is the primary crop.)
  - **Re-editable shapes** — click to select, 8 resize handles on rectangles, endpoint handles on
    lines/arrows, drag-body to move, arrow-keys to nudge, Delete to remove.
  - **Export bar (top-left) [David]:** **Save** (overwrite the source) · **Save as copy** (new
    file, original untouched) · **Export as…** (PNG/JPG/WebP/BMP via the `image` crate the app
    already depends on) · **[Rec] Copy to clipboard**.
  - **[Rec]** Undo/redo (trivial with the vector model) — also present in Tier 1.
- **Parked for later, explicitly:** text annotation, blur/pixelate redaction, numbered step
  badges, curved arrows. Text and blur are the two most likely "next" — flagged, not scoped.
- **Product rule that never bends:** no filled shapes, ever. Border only.

---

## Epic 3 — "Fancy mode" capture animation + docked draggable preview

**After Epics 1–2.** **[David]** explicitly: *very hard to do well*, **opt-in**, **default OFF**
(beta). Documented as intent; not a build commitment.

### 3.1 The sequence (as described)

1. **Flash** — the captured region flashes a **bright grey-white light** so it's obvious you just
   shot *that* area. **[David] said ~1s**; **[Rec]** a flash reads best at ~120–250 ms — propose a
   quick flash, tunable. **[?] confirm duration.**
2. **Morph / travel** — right after (~0.3 s), the captured image **shrinks and travels** from
   where it was to the **bottom-left corner** of the screen.
3. **Dock** — it settles bottom-left as a **bordered preview thumbnail**, clean design,
   **always-on-top above every window**.

### 3.2 Interaction

- **[David]** Hover → **grab cursor**; **drag it anywhere** to drag the image out. Dropping it in
  a terminal / app drops the *file* there — often more convenient than pasting.
- **[David]** Successful drop → it disappears. Failed drag → it stays.
- **[David]** It stays docked forever until dragged. Toggle on/off, **default OFF**.

### 3.3 Why this is the hard part (flagging honestly)

- **[Rec]** "Above **everything**" is only *mostly* achievable — a topmost layered window sits
  above normal windows, but not reliably above another app's topmost window or an
  exclusive-fullscreen game. Scope it to "above all normal windows" and accept the OS limit.
- **[Rec]** **Drag-out-as-a-file** is the genuinely hard bit: it needs a real OLE drag source
  offering **`CF_HDROP`** so terminals/Explorer/editors accept it. That means a saved PNG on disk
  plus a Win32 `IDataObject` / `DoDragDrop` implementation. Doable, fiddly, native — not a webview
  thing.
- **[Rec]** Flash + morph = a click-through, per-pixel-alpha **layered window** animating a
  shrink/translate (GDI+ or Direct2D). Moderate difficulty.
- **[Rec]** Lives in the **native layer**, and the animation must start **after** the file is
  written so the "never slow a capture" invariant holds.

### 3.4 Open questions

- **[?]** Multiple shots in a row — **stack** or **replace**? (Rec: only the latest, or a max-3
  stack with oldest auto-expiring.)
- **[?]** Any dismiss without dragging (click-away, right-click → dismiss, tiny ✕)? He said "stays
  forever until dragged" — confirm there is genuinely no dismiss.
- **[?]** Does the dock also appear after an *annotated* capture, or only plain ones? (Rec:
  independent of annotation — it's a capture-time behaviour, fires on commit.)
- **[?]** Flash duration (see 3.1); corner confirmed as bottom-left.

### 3.5 Phasing (later)

1. Static: after capture, show the bordered preview docked bottom-left, topmost. No animation.
2. Add flash + morph/travel.
3. Add OLE drag-out (`CF_HDROP`); disappear-on-drop / stay-on-fail.
4. Polish: stacking policy, settings toggle, multi-monitor placement, DPI.

---

## Cross-cutting notes

- **Hotkey inventory after Epic 1:** quick · save · open-folder · **annotate** (**4**, all
  rebindable) — plus an optional, empty-by-default `annotate_save_hotkey`. The full editor
  deliberately gets none. The settings app's uniqueness-validation must grow from 3-way to
  N-way and skip empty values, and the conflict warning updated.
- **Config additions:** `annotate_hotkey`, `annotate_save_hotkey` (optional, empty), `[annotate]` block
  (`palette`, `default_color`, `default_width`, `default_tool`, `rounded_corners`,
  `remember_last_style`). All comment-preserving via `toml_edit`, as today.
- **Settings surface:** new "Annotate" tab — hotkeys, palette, default tool/width, rounded
  corners; later a "Fancy mode" toggle + flash duration.
- **No dimming.** Dimming was removed in v0.2.0 on purpose (speed + preference). Annotate mode
  outlines the selection; it must **not** reintroduce a dim layer.
- **Formats:** PNG today; JPG/WebP/BMP arrive with Tier 2's Export-as, via the `image` crate.
- **Testing:** extend the existing headless hooks — a `--annotate-render-test` that composites a
  scripted shape list and diffs pixels, so the render path is verifiable in CI without a desktop.

---

## Decisions

### ✅ Settled 2026-07-27
1. **Tier split — ACCEPTED.** In-overlay quick bar is the main event; the full editor is a
   `Ctrl+E` escalation with no global hotkey.
2. **Trigger — SETTLED as one hotkey.** `Ctrl+Alt+Shift+Q` is the only new global; destination is
   picked at commit (`Enter` = temp, `Shift+Enter` = keeper). David was undecided between several
   combos, so the design was changed to not need a second one. An optional
   `annotate_save_hotkey` ships empty; `Ctrl+Alt+Shift+S` is the suggested value if he wants it.
   **He can veto this in one word and we go back to a two-hotkey pair.**
3. **Draw-and-go — ACCEPTED.** Undo yes, selection handles no.

### ❓ Still open
4. **Write-on-commit** — the annotated file is written **once**, on commit. No un-annotated
   version hits `temp.png` first. Accept? (Rec: yes.)
5. **"Shift on release"** accelerator (turn a plain `Ctrl+Alt+Q` into an annotate session by
   holding Shift when you let go) — build it? ship it off by default?
6. Arrow style: one good design, or two presets (solid head vs chevron)?
7. Rounded↔sharp rectangle corners: a global setting, or a per-shape toggle in the bar?
8. Epic 3: flash duration (~150 ms vs your 1 s), stack vs replace, any dismiss-without-drag.
9. Anything to pull forward into v1 (text? blur redaction?) or hold the line at the list above?
