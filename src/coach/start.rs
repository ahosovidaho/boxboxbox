//! Race starts: how the car got away.
//!
//! A start is the first lap file of a race (GT7 sends the number of race laps) – a standing start
//! when the car stands at the beginning of the file, a rolling one otherwise. For a standing
//! start: 0–100 km/h, wheelspin (rear wheels faster than the car), engine speed when the clutch
//! bites, bogging down (engine speed collapsing) and the first upshifts against the limiter. For
//! a rolling start only the first lap against the best lap of the race.
//! The timing of the lights is not in the telemetry, so the reaction is not measured.

use super::compare::lap_files;
use super::lap::{self, Row, HZ};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Start {
    pub session: String,
    pub file: String,
    /// "standing" or "rolling"
    pub kind: &'static str,
    pub race_laps: u32,
    /// 0–100 km/h (s)
    pub to_100: Option<f64>,
    /// Time with the rear wheels spinning (s) in the first 5 s
    pub spin_s: f64,
    /// Engine speed when the car started moving, and the limiter
    pub launch_rpm: Option<f64>,
    pub max_rpm: f64,
    /// Engine speed fell below 45 % of the limiter while pulling away
    pub bog: bool,
    /// Upshifts in the first 10 s that sat on the limiter
    pub limiter_shifts: usize,
    pub advice: Vec<String>,
}

/// Rear wheel surface speed against the car speed (share above it).
fn rear_slip(r: &Row) -> Option<f64> {
    let v = r.speed_kmh / 3.6;
    if v < 2.0 || r.tire_radius[2] <= 0.0 {
        return None;
    }
    let w = (r.wheel_rps[2].abs() * r.tire_radius[2] + r.wheel_rps[3].abs() * r.tire_radius[3]) / 2.0;
    Some(w / v - 1.0)
}

/// Analyses one lap file if it is the start of a race.
pub fn analyse_file(session: &str, path: &Path) -> Option<Start> {
    let rows = lap::load(path).ok()?;
    let race_laps = rows.iter().find_map(|r| r.total_laps).filter(|t| *t > 0.0)? as u32;
    if rows.len() < (HZ * 10.0) as usize || rows[0].lap > 1.0 {
        return None;
    }
    let file = path.file_name()?.to_string_lossy().into_owned();
    let max_rpm = rows.iter().map(|r| r.rpm).fold(0.0, f64::max);
    if rows[0].speed_kmh > 3.0 {
        return Some(Start {
            session: session.into(),
            file,
            kind: "rolling",
            race_laps,
            to_100: None,
            spin_s: 0.0,
            launch_rpm: None,
            max_rpm,
            bog: false,
            limiter_shifts: 0,
            advice: Vec::new(),
        });
    }
    // Moment the car starts moving
    let go = rows.iter().position(|r| r.speed_kmh > 3.0)?;
    let launch_rpm = rows[go.saturating_sub((HZ * 0.3) as usize)..=go].iter().map(|r| r.rpm).fold(0.0, f64::max);
    let to_100 = rows[go..].iter().position(|r| r.speed_kmh >= 100.0).map(|i| lap::round_to(i as f64 / HZ, 2));
    let first5 = &rows[go..(go + (HZ * 5.0) as usize).min(rows.len())];
    let spin_s = lap::round_to(first5.iter().filter(|r| rear_slip(r).is_some_and(|s| s > 0.15)).count() as f64 / HZ, 2);
    let bog = first5.iter().take((HZ * 2.0) as usize).any(|r| r.rpm < max_rpm * 0.45 && r.throttle > 200.0 && r.speed_kmh < 60.0);
    let first10 = &rows[go..(go + (HZ * 10.0) as usize).min(rows.len())];
    let limiter_shifts = super::shifts::detect(first10, max_rpm).iter().filter(|e| e.to > e.from && e.limiter).count();
    let mut advice = Vec::new();
    if spin_s > 0.5 {
        advice.push(format!(
            "Kola se při rozjezdu protáčela {} s – při startu dávej jen ~70–80 % plynu a plný až po prvním řazení.",
            format!("{spin_s:.1}").replace('.', ",")
        ));
    }
    if bog {
        advice.push("Motor při rozjezdu padl do nízkých otáček – drž před startem víc otáček (kolem poloviny otáčkoměru).".into());
    }
    if limiter_shifts > 0 {
        advice.push(format!("Po startu {limiter_shifts}× řazení až na omezovači – řaď o kousek dřív."));
    }
    if advice.is_empty() {
        advice.push("Rozjezd čistý – bez prokluzu a bez propadu otáček.".into());
    }
    Some(Start {
        session: session.into(),
        file,
        kind: "standing",
        race_laps,
        to_100,
        spin_s,
        launch_rpm: Some(launch_rpm.round()),
        max_rpm: max_rpm.round(),
        bog,
        limiter_shifts,
        advice,
    })
}

/// Starts in a session (the first lap file of every race in it).
pub fn of_session(session: &Path) -> Vec<Start> {
    let name = session.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // A race start is in one of the first files after the lap count starts again; only look at
    // files that begin at lap 0 or 1
    let mut out: Vec<Start> = Vec::new();
    let mut prev_zero = false;
    for f in lap_files(session) {
        let n = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let zero = n.ends_with("_lap00.csv");
        // Lap 1 right after a lap-0 file is the same race (the start was in the lap-0 file)
        let first = zero || (n.ends_with("_lap01.csv") && !(prev_zero && out.last().is_some()));
        if first {
            if let Some(s) = analyse_file(&name, &f) {
                out.push(s);
            }
        }
        prev_zero = zero;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standing start with 1 s of wheelspin and the first upshift on the limiter.
    #[test]
    fn standing_start_with_wheelspin() {
        let dir = std::env::temp_dir().join(format!("bbb_start_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut csv = String::from("lap,lap_ms,total_laps,speed_kmh,rpm,gear,throttle,brake,wheel_rps_fl,wheel_rps_fr,wheel_rps_rl,wheel_rps_rr,tire_radius_fl,tire_radius_fr,tire_radius_rl,tire_radius_rr,fuel\n");
        let (mut v, mut gear) = (0.0f64, 1);
        for i in 0..900 {
            let t = i as f64 / 60.0;
            if t > 1.0 {
                v += 0.25;
            }
            let rpm = if t <= 1.0 {
                6000.0
            } else if gear == 1 && v > 60.0 {
                gear = 2;
                8000.0
            } else {
                5000.0 + v * 30.0
            };
            let rpm = if gear == 1 && v > 55.0 { 8000.0 } else { rpm };
            // Rear wheels 40 % faster than the car for the first second of moving
            let spin = if t > 1.0 && t < 2.0 { 1.4 } else { 1.0 };
            let w = v / 3.6 / 0.33;
            csv.push_str(&format!("1,{},5,{v:.2},{rpm},{gear},255,0,{w},{w},{},{},0.33,0.33,0.33,0.33,50\n", i * 16, w * spin, w * spin));
        }
        let f = dir.join("001_lap01.csv");
        std::fs::write(&f, csv).unwrap();
        let s = analyse_file("x", &f).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(s.kind, "standing");
        assert!(s.spin_s > 0.4 && s.spin_s < 1.1, "{s:?}");
        assert!(s.to_100.is_some_and(|t| t > 6.0 && t < 7.0), "{s:?}");
        assert!(s.advice[0].contains("protáčela"), "{s:?}");
    }
}
