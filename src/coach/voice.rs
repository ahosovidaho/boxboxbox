//! Which corners the voice coach calls on each track (Coach → Trénink → Hlasový kouč).
//!
//! Stored per track in `coach/voice.json` of the data folder, so a choice made on one device
//! holds on the one at the wheel. "auto": the corners that cost the most (the browser picks
//! them from the coach's figures); "pick": only the corners chosen.

use crate::strategy::practice::track_key;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prefs {
    /// "auto" or "pick"
    pub mode: String,
    #[serde(default)]
    pub corners: Vec<usize>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { mode: "auto".into(), corners: Vec::new() }
    }
}

pub fn path(data_root: &Path) -> PathBuf {
    data_root.join("coach").join("voice.json")
}

fn load_all(file: &Path) -> BTreeMap<String, Prefs> {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn get(file: &Path, track: &str) -> Prefs {
    load_all(file).remove(&track_key(track)).unwrap_or_default()
}

pub fn set(file: &Path, track: &str, mut p: Prefs) -> std::io::Result<Prefs> {
    if p.mode != "pick" {
        p.mode = "auto".into();
    }
    p.corners.sort_unstable();
    p.corners.dedup();
    let mut all = load_all(file);
    all.insert(track_key(track), p.clone());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, serde_json::to_string_pretty(&all).unwrap_or_default())?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_per_track() {
        let dir = std::env::temp_dir().join(format!("bbb-voice-{}", std::process::id()));
        let f = path(&dir);
        assert_eq!(get(&f, "Yas Marina Circuit"), Prefs::default());
        set(&f, "Yas Marina Circuit", Prefs { mode: "pick".into(), corners: vec![9, 8, 9] }).unwrap();
        assert_eq!(get(&f, "yas marina circuit"), Prefs { mode: "pick".into(), corners: vec![8, 9] });
        assert_eq!(get(&f, "Nürburgring").mode, "auto");
        set(&f, "Nürburgring", Prefs { mode: "nonsense".into(), corners: vec![] }).unwrap();
        assert_eq!(get(&f, "Nürburgring").mode, "auto");
        let _ = std::fs::remove_dir_all(dir);
    }
}
