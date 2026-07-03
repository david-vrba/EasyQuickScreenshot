// Config loading + hotkey string parsing.
// Search order: --config <path> arg, then config.toml next to the exe, then cwd, then built-in defaults.

use serde::Deserialize;
use std::path::{Path, PathBuf};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

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
"#;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RawConfig {
    pub quick_hotkey: String,
    pub save_hotkey: String,
    pub shots_dir: String,
    pub temp_file: String,
    pub copy_to_clipboard: bool,
}

impl Default for RawConfig {
    fn default() -> Self {
        RawConfig {
            quick_hotkey: "ctrl+alt+q".into(),
            save_hotkey: "ctrl+alt+e".into(),
            shots_dir: "shots".into(),
            temp_file: "temp.png".into(),
            copy_to_clipboard: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Hotkey {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub vk: u32,
}

pub struct Config {
    pub quick_hotkey: Hotkey,
    pub save_hotkey: Hotkey,
    pub quick_hotkey_label: String,
    pub save_hotkey_label: String,
    pub shots_dir: PathBuf,
    pub temp_path: PathBuf,
    pub saved_dir: PathBuf,
    pub copy_to_clipboard: bool,
    pub config_path: PathBuf,
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn find_config_path(cli_override: Option<&str>) -> PathBuf {
    if let Some(p) = cli_override {
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
    // First run: create a default config beside the exe so the tray "Open config" always works.
    let _ = std::fs::write(&beside_exe, DEFAULT_CONFIG);
    beside_exe
}

pub fn load(cli_override: Option<&str>) -> Result<Config, String> {
    let config_path = find_config_path(cli_override);
    let raw = match std::fs::read_to_string(&config_path) {
        Ok(text) => toml::from_str::<RawConfig>(&text)
            .map_err(|e| format!("{}:\n{}", config_path.display(), e))?,
        Err(_) => RawConfig::default(),
    };

    let base = config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(exe_dir);

    let shots_dir = if Path::new(&raw.shots_dir).is_absolute() {
        PathBuf::from(&raw.shots_dir)
    } else {
        base.join(&raw.shots_dir)
    };

    Ok(Config {
        quick_hotkey: parse_hotkey(&raw.quick_hotkey)
            .ok_or(format!("invalid quick_hotkey: \"{}\"", raw.quick_hotkey))?,
        save_hotkey: parse_hotkey(&raw.save_hotkey)
            .ok_or(format!("invalid save_hotkey: \"{}\"", raw.save_hotkey))?,
        quick_hotkey_label: raw.quick_hotkey,
        save_hotkey_label: raw.save_hotkey,
        temp_path: shots_dir.join(&raw.temp_file),
        saved_dir: shots_dir.join("saved"),
        shots_dir,
        copy_to_clipboard: raw.copy_to_clipboard,
        config_path,
    })
}

fn parse_hotkey(spec: &str) -> Option<Hotkey> {
    let mut modifiers = MOD_NOREPEAT;
    let mut vk: Option<u32> = None;

    for token in spec.split('+').map(|t| t.trim().to_ascii_lowercase()) {
        match token.as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" | "super" => modifiers |= MOD_WIN,
            key => {
                if vk.is_some() {
                    return None; // two non-modifier keys
                }
                vk = Some(parse_key(key)?);
            }
        }
    }
    Some(Hotkey {
        modifiers,
        vk: vk?,
    })
}

fn parse_key(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    match key {
        _ if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() => {
            Some(bytes[0].to_ascii_uppercase() as u32)
        }
        _ if key.starts_with('f') && key.len() >= 2 => {
            let n: u32 = key[1..].parse().ok()?;
            (1..=24).contains(&n).then(|| 0x6F + n) // VK_F1 = 0x70
        }
        "printscreen" | "prtscn" => Some(0x2C), // VK_SNAPSHOT
        "space" => Some(0x20),
        "insert" => Some(0x2D),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "pause" => Some(0x13),
        _ => None,
    }
}
