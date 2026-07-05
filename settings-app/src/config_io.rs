// Reads and writes the shared config.toml. Writing goes through toml_edit so the file's
// comments and formatting survive a round-trip. Config discovery mirrors the core app:
// --config <path>, else config.toml beside the exe, else the current directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{value, DocumentMut};

pub const DEFAULT_CONFIG: &str = r#"# EasyQuickScreenshot config
# Hotkey format: modifiers + key, e.g. "ctrl+alt+q", "shift+f9", "ctrl+shift+printscreen"
# Modifiers: ctrl, alt, shift, win — Keys: a-z, 0-9, f1-f24, printscreen, space

# Quick shot: overwrites the same temp file every time (zero folder bloat)
quick_hotkey = "ctrl+alt+q"

# Easy save: writes a timestamped file into <shots_dir>/saved/
save_hotkey = "ctrl+alt+e"

# Where screenshots go. Relative paths resolve against this config file's folder.
shots_dir = "shots"

# Filename of the quick-shot temp file (lives directly in shots_dir)
temp_file = "temp.png"

# Also copy every capture to the clipboard so you can paste it immediately
copy_to_clipboard = true

# What marks your position while selecting — exactly one of:
#   "lines"  = full-screen crosshair lines (the mouse cursor is hidden)
#   "cursor" = a plain crosshair mouse cursor, no lines
crosshair_style = "lines"
"#;

#[derive(Serialize, Deserialize, Clone)]
pub struct ConfigDto {
    pub quick_hotkey: String,
    pub save_hotkey: String,
    pub shots_dir: String,
    pub temp_file: String,
    pub copy_to_clipboard: bool,
    pub crosshair_style: String,
    // Resolved, read-only context for the UI (not written back):
    pub config_path: String,
    pub shots_dir_abs: String,
    pub saved_dir_abs: String,
}

fn config_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1).cloned())
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn resolve_config_path() -> PathBuf {
    if let Some(p) = config_arg() {
        return PathBuf::from(p);
    }
    let beside_exe = exe_dir().join("config.toml");
    if beside_exe.exists() {
        return beside_exe;
    }
    let in_cwd = PathBuf::from("config.toml");
    if in_cwd.exists() {
        return in_cwd;
    }
    beside_exe
}

fn base_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(exe_dir)
}

fn resolve_shots_dir(config_path: &Path, shots_dir: &str) -> PathBuf {
    if Path::new(shots_dir).is_absolute() {
        PathBuf::from(shots_dir)
    } else {
        base_dir(config_path).join(shots_dir)
    }
}

fn read_document(config_path: &Path) -> DocumentMut {
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|t| t.parse::<DocumentMut>().ok())
        .unwrap_or_else(|| DEFAULT_CONFIG.parse::<DocumentMut>().unwrap())
}

fn str_field(doc: &DocumentMut, key: &str, fallback: &str) -> String {
    doc.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(fallback)
        .to_string()
}

pub fn load() -> ConfigDto {
    let config_path = resolve_config_path();
    let doc = read_document(&config_path);

    let shots_dir = str_field(&doc, "shots_dir", "shots");
    let temp_file = str_field(&doc, "temp_file", "temp.png");
    let shots_abs = resolve_shots_dir(&config_path, &shots_dir);

    ConfigDto {
        quick_hotkey: str_field(&doc, "quick_hotkey", "ctrl+alt+q"),
        save_hotkey: str_field(&doc, "save_hotkey", "ctrl+alt+e"),
        shots_dir,
        temp_file,
        copy_to_clipboard: doc
            .get("copy_to_clipboard")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        crosshair_style: str_field(&doc, "crosshair_style", "lines"),
        saved_dir_abs: shots_abs.join("saved").to_string_lossy().into_owned(),
        shots_dir_abs: shots_abs.to_string_lossy().into_owned(),
        config_path: config_path.to_string_lossy().into_owned(),
    }
}

pub fn save(dto: &ConfigDto) -> Result<(), String> {
    validate(dto)?;

    let config_path = resolve_config_path();
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }

    let mut doc = read_document(&config_path);
    doc["quick_hotkey"] = value(dto.quick_hotkey.trim());
    doc["save_hotkey"] = value(dto.save_hotkey.trim());
    doc["shots_dir"] = value(dto.shots_dir.trim());
    doc["temp_file"] = value(dto.temp_file.trim());
    doc["copy_to_clipboard"] = value(dto.copy_to_clipboard);
    doc["crosshair_style"] = value(dto.crosshair_style.trim());

    std::fs::write(&config_path, doc.to_string()).map_err(|e| e.to_string())
}

// Mirror the core app's hotkey grammar so the app never fails to register what we saved.
fn validate(dto: &ConfigDto) -> Result<(), String> {
    validate_hotkey(&dto.quick_hotkey).map_err(|e| format!("Quick-shot hotkey: {e}"))?;
    validate_hotkey(&dto.save_hotkey).map_err(|e| format!("Save hotkey: {e}"))?;
    if dto.quick_hotkey.trim().eq_ignore_ascii_case(dto.save_hotkey.trim()) {
        return Err("The two hotkeys must be different.".into());
    }
    if dto.temp_file.trim().is_empty() {
        return Err("Temp file name can't be empty.".into());
    }
    if !matches!(dto.crosshair_style.trim(), "lines" | "cursor") {
        return Err("Crosshair style must be \"lines\" or \"cursor\".".into());
    }
    Ok(())
}

fn validate_hotkey(spec: &str) -> Result<(), String> {
    let mut has_key = false;
    for token in spec.split('+').map(|t| t.trim().to_ascii_lowercase()) {
        if token.is_empty() {
            return Err("empty component".into());
        }
        match token.as_str() {
            "ctrl" | "control" | "alt" | "shift" | "win" | "super" => {}
            key if is_valid_key(key) => {
                if has_key {
                    return Err("only one non-modifier key allowed".into());
                }
                has_key = true;
            }
            other => return Err(format!("unknown key \"{other}\"")),
        }
    }
    if has_key {
        Ok(())
    } else {
        Err("needs a non-modifier key (e.g. Q)".into())
    }
}

fn is_valid_key(key: &str) -> bool {
    let one_alnum = key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric();
    let fkey = key.starts_with('f')
        && key[1..].parse::<u32>().map(|n| (1..=24).contains(&n)).unwrap_or(false);
    let named = matches!(key, "printscreen" | "prtscn" | "space" | "insert" | "home" | "end" | "pause");
    one_alnum || fkey || named
}
