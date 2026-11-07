//! Library of the recorded sessions: car, track (recognised by the shape of the best lap against
//! the layouts stored in practice), complete laps and the best lap. Lets the coach look beyond a
//! single session: the best lap ever on a track with a car, all sessions of a track (notebook).
//!
//! The facts of a session are cached in `<session>/analysis/index.json` and recomputed when the
//! lap files change. The track is not cached when it was not recognised (the layout may be stored
//! later).

use super::compare::{lap_files, Comparer};
use super::lap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SessionInfo {
    /// Folder name in the recordings
    pub session: String,
    pub car_code: i32,
    pub car: String,
    /// "2026-10-19T13:12:46+02:00"
    pub started: String,
    pub track: Option<String>,
    /// Complete laps
    pub laps: usize,
    pub best_ms: Option<i64>,
    pub best_file: Option<String>,
    /// Lap files and their total size when this was computed (cache key)
    pub key: String,
    /// Stored layouts when the track was looked for (an unknown track is looked for again when
    /// a layout has been added)
    pub layouts: usize,
}

fn cache_key(session: &Path) -> String {
    let files = lap_files(session);
    let size: u64 = files.iter().filter_map(|f| fs::metadata(f).ok()).map(|m| m.len()).sum();
    format!("{}:{size}", files.len())
}

/// Positions of a lap file (x, z), for the layout check.
pub fn lap_points(file: &Path) -> Vec<[f32; 2]> {
    lap::load(file).map(|rows| rows.iter().map(|r| [r.pos_x as f32, r.pos_z as f32]).collect()).unwrap_or_default()
}

/// Facts of one session. `track_of`: layout name for the positions of a lap.
pub fn info(session: &Path, comparer: &mut Comparer, track_of: &dyn Fn(&[[f32; 2]]) -> Option<String>, layouts: usize) -> SessionInfo {
    let key = cache_key(session);
    let cache = session.join("analysis").join("index.json");
    let cached: Option<SessionInfo> = fs::read_to_string(&cache).ok().and_then(|t| serde_json::from_str(&t).ok());
    if let Some(c) = cached.as_ref().filter(|c| c.key == key && (c.track.is_some() || c.layouts == layouts)) {
        return c.clone();
    }
    let meta: serde_json::Value =
        fs::read_to_string(session.join("session.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut out = match cached.filter(|c| c.key == key) {
        Some(c) => c,
        None => {
            // A cleaned session counts its laps from the summary
            let laps = match crate::storage::read_summary(session).filter(|s| s.pruned) {
                Some(s) => s.complete.iter().map(|(f, ms)| (session.join(f), *ms)).collect(),
                None => comparer.complete_laps(session),
            };
            let best = laps.iter().min_by_key(|(_, ms)| *ms);
            SessionInfo {
                session: session.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                car_code: meta["car_code"].as_i64().unwrap_or(0) as i32,
                car: meta["car"].as_str().unwrap_or_default().to_string(),
                started: meta["started"].as_str().unwrap_or_default().to_string(),
                track: None,
                laps: laps.len(),
                best_ms: best.map(|b| b.1),
                best_file: best.map(|b| lap::lap_name(&b.0)),
                key,
                layouts,
            }
        }
    };
    if let Some(f) = &out.best_file {
        out.track = track_of(&lap_points(&session.join(f)));
    }
    out.layouts = layouts;
    if fs::create_dir_all(session.join("analysis")).is_ok() {
        let _ = fs::write(&cache, serde_json::to_string_pretty(&out).unwrap_or_default());
    }
    out
}

/// All sessions with at least one complete lap, newest first.
pub fn index(
    recordings: &Path,
    comparer: &mut Comparer,
    track_of: &dyn Fn(&[[f32; 2]]) -> Option<String>,
    layouts: usize,
) -> Vec<SessionInfo> {
    let mut dirs: Vec<_> = fs::read_dir(recordings).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs.reverse();
    dirs.iter().map(|d| info(d, comparer, track_of, layouts)).filter(|i| i.laps > 0).collect()
}

/// Sessions of a track (and car, when given), newest first.
pub fn of_track<'a>(all: &'a [SessionInfo], track: &str, car: Option<i32>) -> Vec<&'a SessionInfo> {
    let key = crate::strategy::practice::track_key(track);
    all.iter()
        .filter(|i| i.track.as_deref().is_some_and(|t| crate::strategy::practice::track_key(t) == key))
        .filter(|i| car.is_none_or(|c| i.car_code == c))
        .collect()
}

/// The fastest session (best lap) among `sessions`.
pub fn best<'a>(sessions: &[&'a SessionInfo]) -> Option<&'a SessionInfo> {
    sessions.iter().filter(|i| i.best_ms.is_some()).min_by_key(|i| i.best_ms).copied()
}
