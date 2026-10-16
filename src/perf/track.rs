//! Power curve and shift points from ordinary driving (practice, races) instead of a launch run.
//!
//! Every stretch at full throttle, in a gear, without braking and nearly straight (low yaw rate)
//! is kept for the car. From all of them the same computation as for an acceleration run
//! ([`super::dyno::compute`]) gives the power over rpm and the best shift points – for every gear
//! that was used at full throttle, the high ones included. Hills are taken into account
//! (gravity along the slope), the slip filter of the dyno drops wheelspin.
//!
//! Kept per car in `perf/<car>/track.json` (the points, at most [`MAX_PTS`]) and recomputed every
//! [`RECOMPUTE_EVERY`] new points; the result goes to `perf/<car>/track_dyno.json`.

use super::dyno::{self, Dyno};
use super::{Pt, Sample};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// About 11 minutes of full throttle at 60 Hz
const MAX_PTS: usize = 40_000;
const RECOMPUTE_EVERY: usize = 1_500;
const FULL: u8 = 250;
const MIN_KMH: f32 = 30.0;
/// Straight enough: yaw rate below this (rad/s)
const MAX_YAW: f32 = 0.12;
const TICKS_PER_SEC: f32 = 60.0;

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    /// [t, km/h, distance m, rpm, gear, throttle, height m]; a row with throttle 0 separates stretches
    pts: Vec<[f32; 7]>,
    /// Recorded sessions already imported (`boxboxbox perf <session>`), so a second import adds nothing
    #[serde(default)]
    imported: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TrackDynoFile {
    pub dyno: Dyno,
    /// Full-throttle samples it was computed from
    pub points: usize,
    pub time: String,
}

#[derive(Default)]
pub struct TrackDyno {
    dir: Option<PathBuf>,
    car: i32,
    pts: Vec<Pt>,
    imported: Vec<String>,
    last_tick: Option<i32>,
    since: usize,
    /// Importing a recording: compute once at the end, not in the background as it goes
    import: bool,
}

impl TrackDyno {
    pub fn new(dir: Option<PathBuf>) -> Self {
        TrackDyno { dir, ..Default::default() }
    }

    pub fn for_import(dir: Option<PathBuf>) -> Self {
        TrackDyno { dir, import: true, ..Default::default() }
    }

    fn file(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(self.car.to_string()).join(name))
    }

    /// One packet. `yaw` = yaw rate (rad/s).
    pub fn update(&mut self, car: i32, s: &Sample, yaw: f32) {
        if car != self.car {
            self.update_car(car);
        }
        let good = s.flags & crate::telemetry::packet::flags::ON_TRACK != 0
            && s.flags & (crate::telemetry::packet::flags::PAUSED | crate::telemetry::packet::flags::LOADING) == 0
            && (1..=10).contains(&s.gear)
            && s.throttle >= FULL
            && s.brake == 0
            && s.speed_kmh >= MIN_KMH
            && yaw.abs() < MAX_YAW;
        let gap = self.last_tick.map(|t| s.tick - t);
        self.last_tick = Some(s.tick);
        if gap == Some(0) {
            return;
        }
        let open = self.pts.last().is_some_and(|p| p.thr >= FULL);
        if !good || gap.is_some_and(|g| g != 1) {
            if open {
                self.pts.push(Pt::default()); // separator
            }
            if !good {
                return;
            }
        }
        let t = s.tick as f32 / TICKS_PER_SEC;
        let dist = match self.pts.last() {
            Some(a) if a.thr >= FULL => a.s + (a.v + s.speed_kmh) / 2.0 / 3.6 * (t - a.t),
            _ => 0.0,
        };
        self.pts.push(Pt { t, v: s.speed_kmh, s: dist, rpm: s.rpm, gear: s.gear, thr: s.throttle, brk: s.brake, y: s.y });
        self.since += 1;
        if self.pts.len() > MAX_PTS {
            // Drop the oldest stretch(es)
            let cut = self.pts.len() - MAX_PTS;
            let at = self.pts.iter().skip(cut).position(|p| p.thr < FULL).map_or(cut, |i| cut + i + 1);
            self.pts.drain(..at);
        }
        if self.since >= RECOMPUTE_EVERY && !self.import {
            self.since = 0;
            self.recompute();
        }
    }

    fn recompute(&self) {
        let (Some(points), Some(result)) = (self.file("track.json"), self.file("track_dyno.json")) else { return };
        let pts = self.pts.clone();
        let imported = self.imported.clone();
        std::thread::spawn(move || {
            let n = pts.iter().filter(|p| p.thr >= FULL).count();
            if let Some(dyno) = dyno::compute(&pts, [0.0; 8]) {
                let file =
                    TrackDynoFile { dyno, points: n, time: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false) };
                write(&result, &serde_json::to_string(&file).unwrap_or_default());
            }
            write(&points, &serde_json::to_string(&to_saved(&pts, &imported)).unwrap_or_default());
        });
    }

    /// Recorded session imported before?
    pub fn imported(&mut self, car: i32, session: &str) -> bool {
        if car != self.car {
            self.update_car(car);
        }
        self.imported.iter().any(|s| s == session)
    }

    fn update_car(&mut self, car: i32) {
        self.save();
        self.car = car;
        let saved = self.file("track.json").and_then(|f| load(&f)).unwrap_or_default();
        (self.pts, self.imported) = saved;
        self.last_tick = None;
        self.since = 0;
    }

    /// End of an import: remember the session, compute and store now. Returns the result.
    pub fn finish_import(&mut self, session: &str) -> Option<TrackDynoFile> {
        self.imported.push(session.to_string());
        let n = self.pts.iter().filter(|p| p.thr >= FULL).count();
        let file = dyno::compute(&self.pts, [0.0; 8]).map(|dyno| TrackDynoFile {
            dyno,
            points: n,
            time: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        });
        if let (Some(f), Some(r)) = (self.file("track_dyno.json"), &file) {
            write(&f, &serde_json::to_string(r).unwrap_or_default());
        }
        self.save();
        self.since = 0;
        file
    }

    /// Keeps the points (car change, end of session).
    pub fn save(&self) {
        if self.pts.is_empty() {
            return;
        }
        if let Some(f) = self.file("track.json") {
            write(&f, &serde_json::to_string(&to_saved(&self.pts, &self.imported)).unwrap_or_default());
        }
    }
}

fn to_saved(pts: &[Pt], imported: &[String]) -> Saved {
    Saved { pts: pts.iter().map(|p| [p.t, p.v, p.s, p.rpm, p.gear as f32, p.thr as f32, p.y]).collect(), imported: imported.to_vec() }
}

fn load(path: &Path) -> Option<(Vec<Pt>, Vec<String>)> {
    let s: Saved = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let pts = s
        .pts
        .into_iter()
        .map(|a| Pt { t: a[0], v: a[1], s: a[2], rpm: a[3], gear: a[4] as u8, thr: a[5] as u8, brk: 0, y: a[6] })
        .collect();
    Some((pts, s.imported))
}

fn write(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

/// The stored result for a car, if any.
pub fn read(dir: &Path, car: i32) -> Option<TrackDynoFile> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(car.to_string()).join("track_dyno.json")).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::packet::flags;

    /// Straight-line runs through gears 2–5 with a power curve peaking at 7 000 rpm, shifting
    /// at the limiter (8 000); some stretches cornering (high yaw) must be ignored.
    #[test]
    fn shift_points_from_driving() {
        let mut td = TrackDyno::new(None);
        let ratios = [0.0, 0.0, 70.0, 60.0, 52.0, 46.0];
        let power = |rpm: f32| 260.0 * (1.0 - ((rpm - 7000.0) / 2000.0).powi(2)).max(0.05); // W/kg
        let mut tick = 0;
        for run in 0..6 {
            let (mut v, mut gear) = (60.0f32, 2usize);
            while v < 230.0 {
                let rpm = v * ratios[gear];
                if rpm >= 8000.0 && gear < 5 {
                    gear += 1;
                    continue;
                }
                let s = Sample {
                    tick,
                    speed_kmh: v,
                    rpm,
                    gear: gear as u8,
                    throttle: 255,
                    brake: 0,
                    y: 0.0,
                    flags: flags::ON_TRACK,
                    gear_ratios: [0.0; 8],
                };
                let yaw = if run == 5 { 0.5 } else { 0.0 };
                td.update(7, &s, yaw);
                let acc = power(rpm.min(8000.0)) / (v / 3.6); // m/s²
                v += acc * 3.6 / 60.0;
                tick += 1;
            }
            tick += 30; // braking zone
        }
        let d = dyno::compute(&td.pts, [0.0; 8]).expect("curve");
        assert!(d.shifts.len() >= 3, "{:?}", d.shifts);
        for s in &d.shifts {
            assert!(!s.late, "{s:?}");
            assert!((7000.0..8000.0).contains(&s.rpm), "{s:?}");
        }
    }
}
