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

# Screenshot: drag a region and the editor opens. Press Q to save over the temp file, or
# E to keep a timestamped copy in <shots_dir>/saved/.
quick_hotkey = "ctrl+alt+q"

# Focus mode: no dragging. Point at a window, it lights up, click it and the whole window
# goes to the editor. Set to false and the hotkey below is not registered at all.
window_pick = true
window_hotkey = "ctrl+shift+alt+q"

# Screen recording: drag a region and recording starts. Press this again, or the stop
# button, and the video editor opens — trim, crop, volume, then Q or E as for a screenshot.
record_hotkey = "ctrl+alt+e"
# Frames per second, 10 to 60.
record_fps = 30
# "system" records whatever the PC is playing; "none" records no sound.
record_audio = "system"

# Open the saved-screenshots folder in Explorer (opens whatever shots_dir points to now — no capture)
folder_hotkey = "ctrl+shift+alt+e"
# Open the videos folder in Explorer
videos_folder_hotkey = "ctrl+shift+alt+v"

# Where screenshots go. Relative paths resolve against this config file's folder.
shots_dir = "shots"
# Where recordings go: temp.mp4 here, keepers in saved/. Same rule for relative paths.
videos_dir = "videos"

# Filename of the quick-shot temp file (lives directly in shots_dir)
temp_file = "temp.png"

# Also copy every capture to the clipboard so you can paste it immediately
copy_to_clipboard = true

# What marks your position while selecting — exactly one of:
#   "lines"  = full-screen crosshair lines (the mouse cursor is hidden)
#   "cursor" = a plain crosshair mouse cursor, no lines
crosshair_style = "lines"
"#;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RawConfig {
    pub quick_hotkey: String,
    pub window_hotkey: String,
    pub window_pick: bool,
    pub record_hotkey: String,
    pub record_fps: u32,
    pub record_audio: String,
    pub folder_hotkey: String,
    pub videos_folder_hotkey: String,
    pub shots_dir: String,
    pub videos_dir: String,
    pub temp_file: String,
    pub copy_to_clipboard: bool,
    pub crosshair_style: String,
    pub timing_log: bool,
    /// Read and ignored: since 0.10.0 every capture opens the editor, so there is no
    /// separate drawing hotkey, and since 0.11.0 `Ctrl+Alt+E` records instead of saving.
    /// Both fields stay because `deny_unknown_fields` would otherwise reject every config
    /// file that still carries them.
    pub annotate_hotkey: String,
    pub save_hotkey: String,
}

impl Default for RawConfig {
    fn default() -> Self {
        RawConfig {
            quick_hotkey: "ctrl+alt+q".into(),
            window_hotkey: "ctrl+shift+alt+q".into(),
            window_pick: true,
            record_hotkey: "ctrl+alt+e".into(),
            record_fps: 30,
            record_audio: "system".into(),
            folder_hotkey: "ctrl+shift+alt+e".into(),
            videos_folder_hotkey: "ctrl+shift+alt+v".into(),
            shots_dir: "shots".into(),
            videos_dir: "videos".into(),
            temp_file: "temp.png".into(),
            copy_to_clipboard: true,
            crosshair_style: "lines".into(),
            timing_log: false,
            annotate_hotkey: String::new(),
            save_hotkey: String::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum CrosshairStyle {
    /// Full-screen guide lines through the pointer position; mouse cursor hidden.
    Lines,
    /// Plain crosshair mouse cursor only.
    Cursor,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Hotkey {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub vk: u32,
}

pub struct Config {
    pub quick_hotkey: Hotkey,
    pub window_hotkey: Hotkey,
    pub record_hotkey: Hotkey,
    pub folder_hotkey: Hotkey,
    pub videos_folder_hotkey: Hotkey,
    pub quick_hotkey_label: String,
    pub window_hotkey_label: String,
    pub record_hotkey_label: String,
    pub folder_hotkey_label: String,
    pub videos_folder_hotkey_label: String,
    /// Focus mode is offered at all. Off means the hotkey is never registered, so it stays
    /// free for whatever else wants it.
    pub window_pick: bool,
    pub record_fps: u32,
    pub record_system_sound: bool,
    pub shots_dir: PathBuf,
    pub temp_path: PathBuf,
    pub saved_dir: PathBuf,
    pub videos_dir: PathBuf,
    pub copy_to_clipboard: bool,
    pub crosshair_style: CrosshairStyle,
    /// Append how long each capture took to reach its first frame to `eqs-timing.log`.
    pub timing_log: bool,
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
    let shots_dir = beside(&base, &raw.shots_dir);
    let hotkey = |name: &str, spec: &str| {
        parse_hotkey(spec).ok_or(format!("invalid {}: \"{}\"", name, spec))
    };

    Ok(Config {
        quick_hotkey: hotkey("quick_hotkey", &raw.quick_hotkey)?,
        window_hotkey: hotkey("window_hotkey", &raw.window_hotkey)?,
        record_hotkey: hotkey("record_hotkey", &raw.record_hotkey)?,
        folder_hotkey: hotkey("folder_hotkey", &raw.folder_hotkey)?,
        videos_folder_hotkey: hotkey("videos_folder_hotkey", &raw.videos_folder_hotkey)?,
        window_pick: raw.window_pick,
        record_fps: raw.record_fps.clamp(10, 60),
        record_system_sound: match raw.record_audio.as_str() {
            "system" => true,
            "none" => false,
            other => return Err(format!("invalid record_audio: \"{}\" (use \"system\" or \"none\")", other)),
        },
        quick_hotkey_label: raw.quick_hotkey,
        window_hotkey_label: raw.window_hotkey,
        record_hotkey_label: raw.record_hotkey,
        folder_hotkey_label: raw.folder_hotkey,
        videos_folder_hotkey_label: raw.videos_folder_hotkey,
        temp_path: shots_dir.join(&raw.temp_file),
        saved_dir: shots_dir.join("saved"),
        videos_dir: beside(&base, &raw.videos_dir),
        shots_dir,
        copy_to_clipboard: raw.copy_to_clipboard,
        crosshair_style: match raw.crosshair_style.as_str() {
            "lines" => CrosshairStyle::Lines,
            "cursor" => CrosshairStyle::Cursor,
            other => return Err(format!("invalid crosshair_style: \"{}\" (use \"lines\" or \"cursor\")", other)),
        },
        timing_log: raw.timing_log,
        config_path,
    })
}

/// A configured folder: absolute as written, otherwise next to the config file.
fn beside(base: &Path, dir: &str) -> PathBuf {
    if Path::new(dir).is_absolute() {
        PathBuf::from(dir)
    } else {
        base.join(dir)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `RawConfig` denies unknown fields, so a key that lives in the shipped default but not
    /// in the struct makes every first run fail to read the file the app just wrote itself.
    #[test]
    fn the_config_the_app_writes_on_first_run_parses() {
        let raw: RawConfig = toml::from_str(DEFAULT_CONFIG).expect("default config");
        assert_eq!(raw.quick_hotkey, "ctrl+alt+q");
        assert_eq!(raw.window_hotkey, "ctrl+shift+alt+q");
        assert_eq!(raw.record_hotkey, "ctrl+alt+e");
        assert_eq!(raw.record_audio, "system");
        assert!(raw.window_pick, "focus mode ships on");
        assert_eq!(raw.crosshair_style, "lines");
        assert!(raw.copy_to_clipboard);
    }

    /// The example is what people copy by hand, so it has to stay in step with the default.
    #[test]
    fn the_example_config_offers_every_hotkey() {
        let example: RawConfig =
            toml::from_str(include_str!("../config.example.toml")).expect("config.example.toml");
        let default: RawConfig = toml::from_str(DEFAULT_CONFIG).expect("default config");
        assert_eq!(example.quick_hotkey, default.quick_hotkey);
        assert_eq!(example.window_hotkey, default.window_hotkey);
        assert_eq!(example.record_hotkey, default.record_hotkey);
        assert_eq!(example.folder_hotkey, default.folder_hotkey);
        assert_eq!(example.videos_folder_hotkey, default.videos_folder_hotkey);
        assert_eq!(example.videos_dir, default.videos_dir);
    }

    #[test]
    fn a_config_from_before_the_recorder_still_loads() {
        // David's own file: written before record_hotkey existed, still naming save_hotkey
        // and annotate_hotkey. `deny_unknown_fields` must not turn that into a startup error.
        let old = "quick_hotkey = \"ctrl+alt+q\"\nsave_hotkey = \"ctrl+alt+e\"\nannotate_hotkey = \"ctrl+shift+alt+q\"\n";
        let raw: RawConfig = toml::from_str(old).expect("an old config parses");
        assert_eq!(raw.record_hotkey, "ctrl+alt+e", "and picks up the recorder's default");
    }
}
