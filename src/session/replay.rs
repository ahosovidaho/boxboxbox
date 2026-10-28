//! Reference laps from GT7 replays (a leaderboard lap of an online time trial, say).
//!
//! GT7 sends telemetry while a replay plays, without the "on track" flag, so the normal recorder
//! ignores it. Armed from the Reference page, this keeps the packets of one lap from line to line
//! and saves it as a pending reference in `references/pending/` – but only when the lap was played
//! straight through: no pause, no seeking (L2/R2), no gap in the packets. The driver then gives the
//! driver's name and the official lap time, and the lap is put into the library
//! ([`crate::coach::reference`]), corrected for the replay speed.
//!
//! Measured 28 Oct 2026 (Barcelona, Elantra N TC, RTW_Ran 1:50.099): a replay runs at ~98 % speed;
//! 60 packets a second, positions, speeds, rpm and wheel speeds all consistent with the slower
//! playback (1:52.3 of telemetry for the 1:50.099 lap).

use super::recorder::{Recorder, HEADER};
use crate::telemetry::dynamics::Dynamics;
use crate::telemetry::packet::{flags, Packet};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ARM_FOR: Duration = Duration::from_secs(30 * 60);
/// A lap needs at least this many packets (10 s)
const MIN_ROWS: usize = 600;
/// Position step per packet beyond this is a jump in the replay (seeking), m
const JUMP_M: f32 = 6.0;

#[derive(Default)]
pub struct State {
    /// `references/` in the data folder
    pub dir: PathBuf,
    pub armed_until: Option<Instant>,
    /// "off", "waiting" (armed, no replay), "line" (replay running, waiting for the line),
    /// "recording", "saved", "invalid" (lap spoilt, waiting for the next line)
    pub phase: &'static str,
    /// Why the lap was spoilt
    pub note: String,
    pub rows: usize,
    /// Pending reference saved last (id)
    pub saved: Option<String>,
}

pub type Shared = Arc<Mutex<State>>;

pub fn shared(dir: PathBuf) -> Shared {
    Arc::new(Mutex::new(State { dir, phase: "off", ..Default::default() }))
}

#[derive(Serialize)]
pub struct Status {
    pub armed: bool,
    pub armed_left_s: u64,
    pub phase: &'static str,
    pub note: String,
    pub rows: usize,
    pub seconds: f64,
    pub saved: Option<String>,
}

pub fn status(s: &Shared) -> Status {
    let st = s.lock().unwrap_or_else(|e| e.into_inner());
    let left = st.armed_until.map_or(0, |t| t.saturating_duration_since(Instant::now()).as_secs());
    let phase = if left == 0 && st.phase != "saved" { "off" } else { st.phase };
    Status {
        armed: left > 0,
        armed_left_s: left,
        phase,
        note: st.note.clone(),
        rows: st.rows,
        seconds: st.rows as f64 / 60.0,
        saved: st.saved.clone(),
    }
}

pub fn arm(s: &Shared, on: bool) {
    let mut st = s.lock().unwrap_or_else(|e| e.into_inner());
    st.armed_until = on.then(|| Instant::now() + ARM_FOR);
    st.phase = if on { "waiting" } else { "off" };
    st.note.clear();
    st.rows = 0;
    if on {
        st.saved = None;
    }
}

/// Meta data of a pending reference (`references/pending/<id>.json`).
#[derive(Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Pending {
    pub id: String,
    pub car_code: i32,
    pub car: String,
    pub rows: usize,
    /// Lap time by the packets (60 a second) and as GT7 reported it in the replay, ms
    pub telemetry_ms: i64,
    pub game_ms: i64,
    pub length_m: f64,
    pub created: String,
}

/// Writer side, owned by the tracker.
pub struct Capture {
    shared: Shared,
    buf: Vec<String>,
    /// Lap number seen last (None = no replay packet yet), the current lap started at the line,
    /// and nothing spoilt it so far
    lap: Option<i16>,
    from_line: bool,
    valid: bool,
    prev_pkt: Option<i32>,
    prev_pos: Option<(f32, f32)>,
    frozen: u32,
    length: f64,
}

impl Capture {
    pub fn new(shared: Shared) -> Self {
        Capture {
            shared,
            buf: Vec::new(),
            lap: None,
            from_line: false,
            valid: false,
            prev_pkt: None,
            prev_pos: None,
            frozen: 0,
            length: 0.0,
        }
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.lap = None;
        self.from_line = false;
        self.valid = false;
        self.prev_pkt = None;
        self.prev_pos = None;
        self.frozen = 0;
        self.length = 0.0;
    }

    fn spoil(&mut self, st: &mut State, why: &str) {
        if self.valid && self.from_line {
            st.phase = "invalid";
            st.note = why.to_string();
        }
        self.valid = false;
    }

    pub fn tick(&mut self, p: &Packet, d: &Dynamics, now: Instant) {
        let shared = self.shared.clone();
        let mut st = shared.lock().unwrap_or_else(|e| e.into_inner());
        let armed = st.armed_until.is_some_and(|t| t > now);
        let replay = p.flags & flags::ON_TRACK == 0 && p.flags & flags::LOADING == 0;
        if !armed || st.phase == "saved" {
            if !self.buf.is_empty() || self.lap.is_some() {
                self.reset();
            }
            return;
        }
        if !replay {
            if self.lap.is_some() {
                self.reset();
                st.phase = "waiting";
            }
            return;
        }
        // Continuity: packets one after the other, no pause, no jump in position
        let pos = (p.position[0], p.position[2]);
        if let Some(prev) = self.prev_pkt {
            if p.packet_id == prev {
                return; // the same packet again (paused)
            }
            if p.packet_id != prev + 1 {
                self.spoil(&mut st, "mezera v datech");
            }
        }
        if p.flags & flags::PAUSED != 0 {
            self.spoil(&mut st, "pauza");
        }
        let step = self.prev_pos.map_or(0.0, |(x, z)| ((pos.0 - x).powi(2) + (pos.1 - z).powi(2)).sqrt());
        if step > JUMP_M {
            self.spoil(&mut st, "přetočení (skok v přehrávání)");
        }
        self.frozen = if step < 0.01 && p.speed_ms > 5.0 { self.frozen + 1 } else { 0 };
        if self.frozen == 10 {
            self.spoil(&mut st, "pauza");
        }
        self.prev_pkt = Some(p.packet_id);
        self.prev_pos = Some(pos);
        // The line: the lap number changes
        if self.lap != Some(p.current_lap) {
            if self.from_line && self.valid && self.buf.len() >= MIN_ROWS {
                match save_pending(&st.dir, p, &self.buf, self.length) {
                    Ok(id) => {
                        st.saved = Some(id);
                        st.phase = "saved";
                        st.armed_until = None;
                        self.reset();
                        return;
                    }
                    Err(e) => {
                        st.phase = "invalid";
                        st.note = e.to_string();
                    }
                }
            }
            let first = self.lap.is_none();
            self.lap = Some(p.current_lap);
            self.buf.clear();
            self.length = 0.0;
            self.from_line = !first && p.current_lap > 0;
            self.valid = self.from_line;
            if self.from_line {
                st.phase = "recording";
                st.note.clear();
            } else if st.phase == "waiting" {
                st.phase = "line";
            }
        } else {
            self.length += step as f64;
        }
        if self.from_line && self.valid {
            self.buf.push(Recorder::row(p, d, Some((self.buf.len() as f64 * 1000.0 / 60.0) as i64), None));
        }
        st.rows = if self.valid { self.buf.len() } else { 0 };
    }
}

fn save_pending(dir: &std::path::Path, p: &Packet, rows: &[String], length: f64) -> std::io::Result<String> {
    let pending = dir.join("pending");
    fs::create_dir_all(&pending)?;
    let now = chrono::Local::now();
    let id = format!("{}_car{}", now.format("%Y-%m-%d_%H-%M-%S"), p.car_code);
    let mut text = String::with_capacity(rows.len() * 800);
    text.push_str(HEADER);
    text.push('\n');
    for r in rows {
        text.push_str(r);
        text.push('\n');
    }
    fs::write(pending.join(format!("{id}.csv")), text)?;
    let meta = Pending {
        id: id.clone(),
        car_code: p.car_code,
        car: crate::telemetry::cars::name(p.car_code).unwrap_or_default().to_string(),
        rows: rows.len(),
        telemetry_ms: (rows.len() as f64 * 1000.0 / 60.0).round() as i64,
        game_ms: p.last_lap_ms as i64,
        length_m: (length * 10.0).round() / 10.0,
        created: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
    };
    fs::write(pending.join(format!("{id}.json")), serde_json::to_string_pretty(&meta).unwrap_or_default())?;
    println!("Reference lap captured: {id} ({} packets)", rows.len());
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("bbb-ref-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    /// A replay lap: `n` packets along a circle of ~1 km, lap number `lap`.
    fn drive(c: &mut Capture, start: i32, n: i32, lap: i16, t: Instant) -> i32 {
        let d = Dynamics::default();
        for i in 0..n {
            let a = (i as f32 / n as f32) * std::f32::consts::TAU;
            let p = Packet {
                packet_id: start + i,
                current_lap: lap,
                speed_ms: 30.0,
                flags: flags::IN_GEAR,
                car_code: 3590,
                last_lap_ms: 112_000,
                position: [160.0 * a.cos(), 0.0, 160.0 * a.sin()],
                ..Default::default()
            };
            c.tick(&p, &d, t);
        }
        start + n
    }

    #[test]
    fn one_lap_from_line_to_line_is_kept() {
        let dir = tmp();
        let s = shared(dir.clone());
        let mut c = Capture::new(s.clone());
        let t = Instant::now();
        arm(&s, true);
        let id = drive(&mut c, 0, 700, 8, t); // joined mid-lap: not kept
        assert_eq!(status(&s).phase, "line");
        let id = drive(&mut c, id, 700, 9, t);
        assert_eq!(status(&s).phase, "recording");
        drive(&mut c, id, 1, 10, t); // the line again
        let st = status(&s);
        assert_eq!(st.phase, "saved", "{}", st.note);
        let p: Pending =
            serde_json::from_str(&fs::read_to_string(dir.join("pending").join(format!("{}.json", st.saved.unwrap()))).unwrap()).unwrap();
        assert_eq!(p.rows, 700);
        assert!((p.length_m - 1005.3).abs() < 2.0, "{}", p.length_m);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_jump_spoils_the_lap() {
        let dir = tmp();
        let s = shared(dir.clone());
        let mut c = Capture::new(s.clone());
        let t = Instant::now();
        arm(&s, true);
        let id = drive(&mut c, 0, 100, 8, t);
        let id = drive(&mut c, id, 300, 9, t);
        // Seek: a jump of 200 packets in the circle
        let id = drive(&mut c, id + 200, 300, 9, t);
        assert_eq!(status(&s).phase, "invalid");
        drive(&mut c, id, 700, 10, t);
        assert_eq!(status(&s).phase, "recording", "the next lap starts clean");
        assert!(!dir.join("pending").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
