// The video editor's back end: which recording is being edited, and saving it. The media
// work itself — trim, crop, volume, encode — happens in `eqs.exe --export-video`, so this
// crate stays free of media code and the two can never disagree about how a video is made.

use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

/// Set when the app was started as `eqs-settings --edit-video <file> --eqs <eqs.exe>`.
pub struct Session {
    pub video: PathBuf,
    pub eqs: PathBuf,
}

#[derive(Serialize)]
pub struct SessionDto {
    pub path: String,
}

fn arg_after(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

pub fn from_args() -> Option<Session> {
    let video = PathBuf::from(arg_after("--edit-video")?);
    let eqs = arg_after("--eqs")
        .map(PathBuf::from)
        .unwrap_or_else(|| sibling("eqs.exe"));
    Some(Session { video, eqs })
}

fn sibling(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// The crop as eqs expects it: "x,y,w,h" in video pixels, or "-" for none.
pub fn crop_arg(crop: Option<[i32; 4]>) -> String {
    match crop {
        Some([x, y, w, h]) if w > 0 && h > 0 => format!("{x},{y},{w},{h}"),
        _ => "-".into(),
    }
}

/// Save the recording with the editor's edits. `to` is "quick" (the temp file) or "keep"
/// (a timestamped copy). Returns where it landed.
pub fn save(
    session: &Session,
    config: &std::path::Path,
    to: &str,
    start: f64,
    end: f64,
    crop: Option<[i32; 4]>,
    volume: f64,
) -> Result<String, String> {
    if to != "quick" && to != "keep" {
        return Err(format!("unknown destination \"{to}\""));
    }
    if !(start.is_finite() && end.is_finite() && volume.is_finite()) || end <= start {
        return Err("the trim is empty".into());
    }
    let output = Command::new(&session.eqs)
        .arg("--export-video")
        .arg(&session.video)
        .arg(to)
        .arg(format!("{start:.3}"))
        .arg(format!("{end:.3}"))
        .arg(crop_arg(crop))
        .arg(format!("{:.3}", volume.clamp(0.0, 2.0)))
        .arg("--config")
        .arg(config)
        .output()
        .map_err(|e| format!("could not run {}: {e}", session.eqs.display()))?;
    let said = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        Ok(said)
    } else if said.is_empty() {
        Err(format!("saving failed ({})", output.status))
    } else {
        Err(said)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crop_goes_over_as_four_numbers_or_a_dash() {
        assert_eq!(crop_arg(Some([10, 20, 640, 360])), "10,20,640,360");
        assert_eq!(crop_arg(None), "-");
        assert_eq!(crop_arg(Some([0, 0, 0, 360])), "-", "an empty crop is no crop");
    }
}
