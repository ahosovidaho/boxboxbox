//! Diagnostic capture of GT7 replays (step 0 of external reference laps).
//!
//! GT7 sends telemetry while a replay plays (a leaderboard lap of an online time trial, say), but
//! without the "on track" flag, so the normal recorder ignores it. Armed from the dashboard, this
//! writes every packet that arrives while the car is not "on track" to
//! `replays/<date>_<car>.csv`: a wall-clock column first, then the recorder's columns (packet id,
//! flags, lap, last lap time, positions, pedals …). From such a file we can see what a replay
//! sends: whether the stream is continuous, what pause and seeking (L2/R2) look like, how the lap
//! counter and lap time behave.
//!
//! Arming lasts 30 minutes; a capture stops after 5 s "on track" (driving again), when disarmed
//! or after 15 minutes.

use super::recorder::{Recorder, HEADER};
use crate::telemetry::dynamics::Dynamics;
use crate::telemetry::packet::{flags, Packet};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ARM_FOR: Duration = Duration::from_secs(30 * 60);
const MAX_ROWS: u64 = 15 * 60 * 60;
const ON_TRACK_STOP: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct State {
    pub dir: PathBuf,
    pub armed_until: Option<Instant>,
    /// File being written, rows in it, when it started
    pub file: Option<PathBuf>,
    pub rows: u64,
    pub started: Option<Instant>,
    /// Last finished capture: file and rows
    pub last: Option<(PathBuf, u64)>,
}

pub type Shared = Arc<Mutex<State>>;

pub fn shared(dir: PathBuf) -> Shared {
    Arc::new(Mutex::new(State { dir, ..Default::default() }))
}

#[derive(Serialize)]
pub struct Status {
    pub armed: bool,
    pub armed_left_s: u64,
    pub recording: bool,
    pub file: Option<String>,
    pub rows: u64,
    pub seconds: u64,
    pub last_file: Option<String>,
    pub last_rows: Option<u64>,
}

pub fn status(s: &Shared) -> Status {
    let st = s.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let left = st.armed_until.map_or(0, |t| t.saturating_duration_since(now).as_secs());
    let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned());
    Status {
        armed: left > 0,
        armed_left_s: left,
        recording: st.file.is_some(),
        file: st.file.as_ref().and_then(name),
        rows: st.rows,
        seconds: st.started.map_or(0, |t| now.duration_since(t).as_secs()),
        last_file: st.last.as_ref().and_then(|(p, _)| name(p)),
        last_rows: st.last.as_ref().map(|(_, n)| *n),
    }
}

pub fn arm(s: &Shared, on: bool) {
    let mut st = s.lock().unwrap_or_else(|e| e.into_inner());
    st.armed_until = on.then(|| Instant::now() + ARM_FOR);
}

/// Writer side, owned by the tracker.
pub struct Capture {
    shared: Shared,
    w: Option<BufWriter<File>>,
    t0: Instant,
    on_track_since: Option<Instant>,
}

impl Capture {
    pub fn new(shared: Shared) -> Self {
        Capture { shared, w: None, t0: Instant::now(), on_track_since: None }
    }

    fn finish(&mut self, st: &mut State) {
        if let Some(mut w) = self.w.take() {
            let _ = w.flush();
        }
        if let Some(f) = st.file.take() {
            st.last = Some((f, st.rows));
        }
        st.started = None;
        self.on_track_since = None;
    }

    pub fn tick(&mut self, p: &Packet, d: &Dynamics, lap_ms: Option<i64>, now: Instant) {
        let shared = self.shared.clone();
        let mut st = shared.lock().unwrap_or_else(|e| e.into_inner());
        let armed = st.armed_until.is_some_and(|t| t > now);
        let on_track = p.flags & flags::ON_TRACK != 0;
        let loading = p.flags & flags::LOADING != 0;
        if self.w.is_none() {
            if !armed || on_track || loading || p.speed_ms < 0.2 {
                return;
            }
            let _ = fs::create_dir_all(&st.dir);
            let path = st.dir.join(format!("{}_car{}.csv", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"), p.car_code));
            let Ok(f) = File::create(&path) else { return };
            let mut w = BufWriter::new(f);
            let _ = writeln!(w, "wall_ms,{HEADER}");
            self.w = Some(w);
            self.t0 = now;
            st.file = Some(path);
            st.rows = 0;
            st.started = Some(now);
            println!("Replay capture: {}", st.file.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        }
        if on_track {
            self.on_track_since.get_or_insert(now);
        } else {
            self.on_track_since = None;
        }
        let stop = !armed || st.rows >= MAX_ROWS || self.on_track_since.is_some_and(|t| now.duration_since(t) >= ON_TRACK_STOP);
        if stop {
            self.finish(&mut st);
            return;
        }
        if let Some(w) = self.w.as_mut() {
            let ms = now.duration_since(self.t0).as_millis();
            let _ = writeln!(w, "{ms},{}", Recorder::row(p, d, lap_ms, None));
            st.rows += 1;
            if st.rows.is_multiple_of(60) {
                let _ = w.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_only_while_armed_and_off_track() {
        let dir = std::env::temp_dir().join(format!("bbb-replay-{}", std::process::id()));
        let s = shared(dir.clone());
        let mut c = Capture::new(s.clone());
        let d = Dynamics::default();
        let t = Instant::now();
        let replay = Packet { speed_ms: 30.0, flags: flags::IN_GEAR, car_code: 3590, ..Default::default() };
        c.tick(&replay, &d, None, t);
        assert!(!status(&s).recording, "not armed");
        arm(&s, true);
        for i in 0..120 {
            c.tick(&Packet { packet_id: i, ..replay.clone() }, &d, None, t + Duration::from_millis(i as u64 * 16));
        }
        assert_eq!(status(&s).rows, 120);
        // Driving again: stops after 5 s on track
        let driving = Packet { flags: flags::ON_TRACK | flags::IN_GEAR, ..replay.clone() };
        c.tick(&driving, &d, None, t + Duration::from_secs(3));
        c.tick(&driving, &d, None, t + Duration::from_secs(9));
        let st = status(&s);
        assert!(!st.recording && st.last_rows == Some(121), "{:?}", st.last_rows);
        let text = fs::read_to_string(dir.join(st.last_file.unwrap())).unwrap();
        assert_eq!(text.lines().count(), 122);
        assert!(text.starts_with("wall_ms,lap,"));
        let _ = fs::remove_dir_all(dir);
    }
}
