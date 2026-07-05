const invoke = window.__TAURI__.core.invoke;
const REPO_URL = "https://github.com/david-vrba/EasyQuickScreenshot";

const state = { cfg: null };

/* ---------- tabs ---------- */
document.querySelectorAll(".tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    const name = tab.dataset.tab;
    document.querySelectorAll(".tab").forEach((t) => t.classList.toggle("is-active", t === tab));
    document.querySelectorAll(".panel").forEach((p) =>
      p.classList.toggle("is-active", p.dataset.panel === name)
    );
    if (name === "gallery") loadGallery();
  });
});

/* ---------- settings ---------- */
async function loadConfig() {
  state.cfg = await invoke("load_config");
  const c = state.cfg;
  setHotkey("quick_hotkey", c.quick_hotkey);
  setHotkey("save_hotkey", c.save_hotkey);
  setHotkey("folder_hotkey", c.folder_hotkey);
  document.getElementById("shots_dir").value = c.shots_dir;
  document.getElementById("shots_abs").textContent = "→ " + c.shots_dir_abs;
  document.getElementById("temp_file").value = c.temp_file;
  document.getElementById("copy_to_clipboard").checked = c.copy_to_clipboard;
  setSegmented(c.crosshair_style);
  document.getElementById("config_path").textContent = c.config_path;
}

function setHotkey(field, val) {
  document.querySelector(`.hotkey[data-hotkey="${field}"]`).textContent = val;
}
function setSegmented(val) {
  document.querySelectorAll("#crosshair_style button").forEach((b) =>
    b.classList.toggle("is-active", b.dataset.val === val)
  );
}

document.querySelectorAll("#crosshair_style button").forEach((b) => {
  b.addEventListener("click", () => setSegmented(b.dataset.val));
});

document.getElementById("browse").addEventListener("click", async () => {
  const picked = await invoke("pick_shots_folder");
  if (picked) {
    document.getElementById("shots_dir").value = picked;
    document.getElementById("shots_abs").textContent = "→ " + picked;
  }
});

/* ---------- hotkey capture ---------- */
const MOD_ORDER = ["ctrl", "alt", "shift", "win"];
let recordingBtn = null;
let prevHotkey = "";

function keyToken(e) {
  const code = e.code;
  let m;
  if ((m = code.match(/^Key([A-Z])$/))) return m[1].toLowerCase();
  if ((m = code.match(/^Digit(\d)$/))) return m[1];
  if ((m = code.match(/^F(\d{1,2})$/))) return "f" + m[1];
  if (code === "Space") return "space";
  if (code === "PrintScreen") return "printscreen";
  if (code === "Insert") return "insert";
  if (code === "Home") return "home";
  if (code === "End") return "end";
  return null; // pure modifier or unsupported — keep waiting
}

document.querySelectorAll(".hotkey").forEach((btn) => {
  btn.addEventListener("click", () => startRecording(btn));
});

function startRecording(btn) {
  if (recordingBtn) stopRecording(recordingBtn, true);
  recordingBtn = btn;
  prevHotkey = btn.textContent;
  btn.classList.add("recording");
  btn.textContent = "…";
}
function stopRecording(btn, restore) {
  btn.classList.remove("recording");
  if (restore) btn.textContent = prevHotkey;
  if (recordingBtn === btn) recordingBtn = null;
}

window.addEventListener("keydown", (e) => {
  if (!recordingBtn) return;
  e.preventDefault();
  if (e.key === "Escape") {
    stopRecording(recordingBtn, true);
    return;
  }
  const key = keyToken(e);
  if (!key) return; // waiting for a real key
  const mods = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.altKey) mods.push("alt");
  if (e.shiftKey) mods.push("shift");
  if (e.metaKey) mods.push("win");
  mods.sort((a, b) => MOD_ORDER.indexOf(a) - MOD_ORDER.indexOf(b));
  recordingBtn.textContent = [...mods, key].join("+");
  stopRecording(recordingBtn, false);
});

/* ---------- save ---------- */
document.getElementById("save").addEventListener("click", async () => {
  const status = document.getElementById("status");
  const c = state.cfg;
  c.quick_hotkey = document.querySelector('.hotkey[data-hotkey="quick_hotkey"]').textContent.trim();
  c.save_hotkey = document.querySelector('.hotkey[data-hotkey="save_hotkey"]').textContent.trim();
  c.folder_hotkey = document.querySelector('.hotkey[data-hotkey="folder_hotkey"]').textContent.trim();
  c.shots_dir = document.getElementById("shots_dir").value;
  c.temp_file = document.getElementById("temp_file").value;
  c.copy_to_clipboard = document.getElementById("copy_to_clipboard").checked;
  c.crosshair_style = document.querySelector("#crosshair_style .is-active").dataset.val;

  status.className = "status";
  status.textContent = "";
  try {
    await invoke("save_config", { cfg: c });
    status.className = "status ok";
    status.textContent = "Saved — applied to the running app.";
    await loadConfig();
  } catch (err) {
    status.className = "status err";
    status.textContent = String(err);
  }
});

/* ---------- gallery ---------- */
async function loadGallery() {
  const stats = await invoke("gallery_stats");
  document.getElementById("gallery_stats").innerHTML = `
    <div class="stat"><b>${stats.count}</b><span>Saved</span></div>
    <div class="stat"><b>${stats.total_mb}</b><span>MB on disk</span></div>`;

  const shots = await invoke("gallery_list");
  const grid = document.getElementById("gallery_grid");
  const empty = document.getElementById("gallery_empty");
  grid.innerHTML = "";
  empty.hidden = shots.length > 0;

  for (const s of shots) {
    const card = document.createElement("div");
    card.className = "shot";
    const dims = s.width && s.height ? `${s.width}×${s.height}` : "";
    card.innerHTML = `
      <img class="thumb" src="${s.thumb}" alt="" />
      <div class="meta">
        <div class="name" title="${s.name}">${s.name}</div>
        <div class="sub">${dims} · ${fmtSize(s.size_kb)}</div>
      </div>
      <div class="acts">
        <button data-act="open">Open</button>
        <button data-act="reveal">Reveal</button>
      </div>`;
    card.querySelector(".thumb").addEventListener("click", () => invoke("open_path", { path: s.path }));
    card.querySelector('[data-act="open"]').addEventListener("click", () => invoke("open_path", { path: s.path }));
    card.querySelector('[data-act="reveal"]').addEventListener("click", () => invoke("reveal_path", { path: s.path }));
    grid.appendChild(card);
  }
}

function fmtSize(kb) {
  return kb >= 1024 ? (kb / 1024).toFixed(1) + " MB" : kb + " KB";
}

document.getElementById("refresh_gallery").addEventListener("click", loadGallery);
document.getElementById("open_saved").addEventListener("click", () =>
  invoke("open_path", { path: state.cfg.saved_dir_abs })
);

/* ---------- about ---------- */
document.getElementById("open_config").addEventListener("click", () =>
  invoke("open_path", { path: state.cfg.config_path })
);
document.getElementById("open_repo").addEventListener("click", () =>
  invoke("open_url", { url: REPO_URL })
);

loadConfig();
