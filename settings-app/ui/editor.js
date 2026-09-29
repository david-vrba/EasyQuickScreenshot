// The video editor: trim with two handles, crop with a box over the picture, mute and
// volume, then Q saves over videos/temp.mp4 and E keeps a timestamped copy. The saving
// itself runs in eqs.exe; this file only decides what to ask for.

(async function () {
  const invoke = window.__TAURI__.core.invoke;
  const session = await invoke("video_session");
  if (!session) return; // opened as the plain settings window
  document.body.classList.add("editing");

  const $ = (id) => document.getElementById(id);
  const video = $("clip");
  const stage = $("stage");
  const cropBox = $("crop");
  const track = $("track");
  const kept = $("kept");
  const playhead = $("playhead");
  const inHandle = $("in-handle");
  const outHandle = $("out-handle");
  const volume = $("volume");
  const mute = $("mute");

  const MIN_CROP = 32; // px of video; smaller than this is a mistake, not a crop
  const edit = { start: 0, end: 0, crop: [0, 0, 0, 0], volume: 100, muted: false };
  let size = [0, 0];
  let duration = 0;
  let busy = false;

  video.src = window.__TAURI__.core.convertFileSrc(session.path);
  video.addEventListener("loadedmetadata", () => {
    duration = video.duration;
    size = [video.videoWidth, video.videoHeight];
    edit.end = duration;
    edit.crop = [0, 0, size[0], size[1]];
    render();
  });
  video.addEventListener("error", () => status("This recording could not be opened.", "err"));
  window.addEventListener("resize", render);

  /* ---------- time ---------- */
  const clock = (t) => {
    const m = Math.floor(t / 60);
    const s = (t - m * 60).toFixed(1).padStart(4, "0");
    return `${m}:${s}`;
  };
  const timeAt = (clientX) => {
    const r = track.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * duration;
  };

  function togglePlay() {
    if (!video.paused) return video.pause();
    if (video.currentTime < edit.start || video.currentTime >= edit.end - 0.05) {
      video.currentTime = edit.start;
    }
    video.play();
    requestAnimationFrame(follow);
  }

  // Playback stops at the end of the trim, so what plays is exactly what will be saved.
  function follow() {
    if (video.currentTime >= edit.end) {
      video.pause();
      video.currentTime = edit.end;
    }
    renderTime();
    if (!video.paused) requestAnimationFrame(follow);
  }

  function setStart(t) {
    edit.start = Math.min(Math.max(0, t), edit.end - 0.1);
    render();
  }
  function setEnd(t) {
    edit.end = Math.max(Math.min(duration, t), edit.start + 0.1);
    render();
  }

  function dragHandle(handle, apply) {
    handle.addEventListener("pointerdown", (e) => {
      e.stopPropagation();
      handle.setPointerCapture(e.pointerId);
      const move = (ev) => {
        apply(timeAt(ev.clientX));
        video.currentTime = timeAt(ev.clientX);
      };
      handle.addEventListener("pointermove", move);
      handle.addEventListener("pointerup", () => handle.removeEventListener("pointermove", move), { once: true });
    });
  }
  dragHandle(inHandle, setStart);
  dragHandle(outHandle, setEnd);
  track.addEventListener("pointerdown", (e) => {
    video.currentTime = timeAt(e.clientX);
    renderTime();
  });
  video.addEventListener("seeked", renderTime);
  video.addEventListener("click", togglePlay);
  $("play").addEventListener("click", togglePlay);
  video.addEventListener("play", () => ($("play").textContent = "Pause"));
  video.addEventListener("pause", () => ($("play").textContent = "Play"));

  /* ---------- crop ---------- */
  // Where the picture actually sits inside the <video> box (object-fit: contain), so the
  // crop box can be converted between screen pixels and video pixels.
  function picture() {
    const v = video.getBoundingClientRect();
    const s = stage.getBoundingClientRect();
    const scale = Math.min(v.width / size[0], v.height / size[1]);
    const w = size[0] * scale;
    const h = size[1] * scale;
    return { left: v.left - s.left + (v.width - w) / 2, top: v.top - s.top + (v.height - h) / 2, w, h, scale };
  }

  const place = (el, left, top, width, height) =>
    Object.assign(el.style, {
      left: `${left}px`,
      top: `${top}px`,
      width: `${Math.max(0, width)}px`,
      height: `${Math.max(0, height)}px`,
    });

  function clampCrop([x, y, w, h]) {
    w = Math.max(MIN_CROP, Math.min(w, size[0]));
    h = Math.max(MIN_CROP, Math.min(h, size[1]));
    x = Math.max(0, Math.min(x, size[0] - w));
    y = Math.max(0, Math.min(y, size[1] - h));
    return [x, y, w, h];
  }

  // Dragging the box moves it; dragging a corner moves that corner while the opposite
  // one stays where it is.
  cropBox.addEventListener("pointerdown", (e) => {
    cropBox.setPointerCapture(e.pointerId);
    const corner = e.target.dataset.corner;
    const from = { x: e.clientX, y: e.clientY, crop: [...edit.crop] };
    const move = (ev) => {
      const { scale } = picture();
      const dx = (ev.clientX - from.x) / scale;
      const dy = (ev.clientY - from.y) / scale;
      let [x, y, w, h] = from.crop;
      if (corner === undefined) {
        x += dx;
        y += dy;
      } else {
        const right = corner === "1" || corner === "2";
        const bottom = corner === "2" || corner === "3";
        if (right) w += dx;
        else {
          x += dx;
          w -= dx;
        }
        if (bottom) h += dy;
        else {
          y += dy;
          h -= dy;
        }
        if (w < MIN_CROP) {
          if (!right) x -= MIN_CROP - w;
          w = MIN_CROP;
        }
        if (h < MIN_CROP) {
          if (!bottom) y -= MIN_CROP - h;
          h = MIN_CROP;
        }
      }
      edit.crop = clampCrop([x, y, w, h]);
      render();
    };
    cropBox.addEventListener("pointermove", move);
    cropBox.addEventListener("pointerup", () => cropBox.removeEventListener("pointermove", move), { once: true });
  });
  $("crop-reset").addEventListener("click", () => {
    edit.crop = [0, 0, size[0], size[1]];
    render();
  });

  /* ---------- sound ---------- */
  function applySound() {
    // The preview cannot play louder than the file; above 100% is applied when saving.
    video.volume = Math.min(1, edit.volume / 100);
    video.muted = edit.muted;
    $("volume-label").textContent = edit.muted ? "muted" : `${edit.volume}%`;
  }
  volume.addEventListener("input", () => {
    edit.volume = Number(volume.value);
    applySound();
  });
  mute.addEventListener("change", () => {
    edit.muted = mute.checked;
    applySound();
  });

  /* ---------- drawing ---------- */
  function renderTime() {
    if (!duration) return;
    playhead.style.left = `${(video.currentTime / duration) * 100}%`;
    $("time").textContent = `${clock(video.currentTime)} / ${clock(duration)}`;
  }

  function render() {
    if (!duration) return;
    const a = (edit.start / duration) * 100;
    const b = (edit.end / duration) * 100;
    inHandle.style.left = `${a}%`;
    outHandle.style.left = `${b}%`;
    kept.style.left = `${a}%`;
    kept.style.width = `${b - a}%`;
    $("trim-label").textContent = `Keeping ${clock(edit.start)} – ${clock(edit.end)}  (${(edit.end - edit.start).toFixed(1)} s)`;

    const pic = picture();
    const [x, y, w, h] = edit.crop;
    const cl = pic.left + x * pic.scale;
    const ct = pic.top + y * pic.scale;
    const cw = w * pic.scale;
    const ch = h * pic.scale;
    place(cropBox, cl, ct, cw, ch);
    // The picture outside the crop, in four strips: above, below, then the two sides.
    place($("shade-top"), pic.left, pic.top, pic.w, ct - pic.top);
    place($("shade-bottom"), pic.left, ct + ch, pic.w, pic.top + pic.h - ct - ch);
    place($("shade-left"), pic.left, ct, cl - pic.left, ch);
    place($("shade-right"), cl + cw, ct, pic.left + pic.w - cl - cw, ch);
    const whole = x === 0 && y === 0 && w === size[0] && h === size[1];
    $("crop-label").textContent = whole ? "Full frame" : `${Math.round(w)} × ${Math.round(h)}`;
    $("crop-reset").hidden = whole;
    renderTime();
  }

  /* ---------- saving ---------- */
  function status(text, kind) {
    const el = $("editor-status");
    el.textContent = text;
    el.className = "status" + (kind ? " " + kind : "");
  }

  async function save(to) {
    if (busy || !duration) return;
    busy = true;
    video.pause();
    status(to === "quick" ? "Saving to temp.mp4…" : "Keeping a copy…");
    const [x, y, w, h] = edit.crop.map(Math.round);
    const whole = x === 0 && y === 0 && w === size[0] && h === size[1];
    try {
      const path = await invoke("save_video", {
        to,
        start: edit.start,
        end: edit.end,
        crop: whole ? null : [x, y, w, h],
        volume: edit.muted ? 0 : edit.volume / 100,
      });
      status(`Saved — ${path}`, "ok");
      setTimeout(() => invoke("close_editor"), 600);
    } catch (e) {
      status(String(e), "err");
      busy = false;
    }
  }

  $("save-quick").addEventListener("click", () => save("quick"));
  $("save-keep").addEventListener("click", () => save("keep"));
  $("discard").addEventListener("click", () => invoke("close_editor"));

  document.addEventListener("keydown", (e) => {
    if (busy || e.ctrlKey || e.altKey || e.metaKey) return;
    const key = e.key.toLowerCase();
    // Arrow keys on the volume slider belong to the slider.
    if (e.target.tagName === "INPUT" && key.startsWith("arrow")) return;
    const step = e.shiftKey ? 1 : 1 / 30;
    if (key === " ") togglePlay();
    else if (key === "q" || (key === "enter" && !e.shiftKey)) save("quick");
    else if (key === "e" || (key === "enter" && e.shiftKey)) save("keep");
    else if (key === "escape") invoke("close_editor");
    else if (key === "i") setStart(video.currentTime);
    else if (key === "o") setEnd(video.currentTime);
    else if (key === "arrowleft") video.currentTime = Math.max(0, video.currentTime - step);
    else if (key === "arrowright") video.currentTime = Math.min(duration, video.currentTime + step);
    else return;
    e.preventDefault();
  });

  applySound();
})();
