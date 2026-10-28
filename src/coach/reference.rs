//! External reference laps: laps of other drivers captured from GT7 replays
//! ([`crate::session::replay`]), kept apart from the own sessions in `references/<track>/`.
//!
//! A replay runs slower than real time (~98 %), and GT7 sends the telemetry of the slower
//! playback. When a lap is saved, the official lap time (from the leaderboard) gives the factor
//! k = official / telemetry time: the lap is resampled to 60 rows per second of real time, and
//! everything that is a rate is scaled by 1/k (speeds, rpm, wheel speeds, angular velocities),
//! accelerations by 1/k². Positions, pedals, steering and temperatures stay as they are. The file
//! then reads like a lap of one's own (same columns), so every comparison works on it.

use crate::strategy::practice::track_key;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub use crate::session::replay::Pending;

/// Meta data of a saved reference (`references/<track key>/<id>.json`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Reference {
    pub id: String,
    pub driver: String,
    pub official_ms: i64,
    pub telemetry_ms: i64,
    /// official / telemetry
    pub k: f64,
    pub car_code: i32,
    pub car: String,
    pub track: String,
    pub length_m: f64,
    pub created: String,
    /// "replay"
    pub source: String,
}

/// Rates: scaled by 1/k
const RATES: [&str; 17] = [
    "speed_kmh",
    "rpm",
    "rpm_clutch_gearbox",
    "vel_x",
    "vel_y",
    "vel_z",
    "angvel_x",
    "angvel_y",
    "angvel_z",
    "v_fwd",
    "v_lat",
    "yaw_rate",
    "wheel_rps_fl",
    "wheel_rps_fr",
    "wheel_rps_rl",
    "wheel_rps_rr",
    "boost_bar",
];
/// Accelerations: scaled by 1/k²
const ACCELS: [&str; 3] = ["sway", "heave", "surge"];
/// Not interpolated (whole numbers, flags, angles that wrap)
const NEAREST: [&str; 10] =
    ["lap", "packet_id", "flags", "gear", "suggested_gear", "heading", "total_laps", "last_lap_ms", "best_lap_ms", "time_of_day_ms"];

pub fn root(data: &Path) -> PathBuf {
    data.join("references")
}

fn safe(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
}

pub fn pending(root: &Path) -> Vec<Pending> {
    let mut out: Vec<Pending> = fs::read_dir(root.join("pending"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a: &Pending, b| b.created.cmp(&a.created));
    out
}

pub fn list(root: &Path) -> Vec<Reference> {
    let mut out: Vec<Reference> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir() && e.file_name() != "pending")
        .flat_map(|d| fs::read_dir(d.path()).into_iter().flatten().flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a: &Reference, b| a.track.cmp(&b.track).then(a.official_ms.cmp(&b.official_ms)));
    out
}

/// References on a track (any car; the same car first).
pub fn of_track(root: &Path, track: &str, car: Option<i32>) -> Vec<Reference> {
    let key = track_key(track);
    let mut v: Vec<Reference> = list(root).into_iter().filter(|r| track_key(&r.track) == key).collect();
    v.sort_by_key(|r| (Some(r.car_code) != car, r.official_ms));
    v
}

/// The lap file of a saved reference.
pub fn file(root: &Path, id: &str) -> Option<PathBuf> {
    if !safe(id) {
        return None;
    }
    let r = list(root).into_iter().find(|r| r.id == id)?;
    let p = root.join(track_key(&r.track)).join(format!("{id}.csv"));
    p.is_file().then_some(p)
}

pub fn get(root: &Path, id: &str) -> Option<Reference> {
    list(root).into_iter().find(|r| r.id == id)
}

/// "1:50,099", "1:50.099", "110.099", "110099" → ms
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim().replace(',', ".");
    if s.is_empty() {
        return None;
    }
    // "1:50:099" (minutes, seconds, thousandths – a colon typed for the comma)
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 3 && parts[2].len() == 3 && !s.contains('.') {
        let (m, sec, ms) = (parts[0].parse::<i64>().ok()?, parts[1].parse::<i64>().ok()?, parts[2].parse::<i64>().ok()?);
        return (sec < 60).then_some((m * 60 + sec) * 1000 + ms);
    }
    if !s.contains([':', '.']) {
        return s.parse::<i64>().ok().filter(|v| *v > 1000);
    }
    let (m, rest) = match s.split_once(':') {
        Some((m, r)) => (m.parse::<f64>().ok()?, r.to_string()),
        None => (0.0, s.clone()),
    };
    let secs: f64 = rest.parse().ok()?;
    Some(((m * 60.0 + secs) * 1000.0).round() as i64)
}

/// Resamples a replay lap to real time and scales its rates (see the module docs).
pub fn correct(text: &str, k: f64) -> String {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let rows: Vec<Vec<&str>> = lines.filter(|l| !l.is_empty()).map(|l| l.split(',').collect()).collect();
    if rows.is_empty() || k <= 0.0 {
        return text.to_string();
    }
    let kind = |name: &str| {
        if RATES.contains(&name) {
            1
        } else if ACCELS.contains(&name) {
            2
        } else if NEAREST.contains(&name) {
            3
        } else {
            0
        }
    };
    let kinds: Vec<u8> = header.iter().map(|h| kind(h)).collect();
    let col = |n: &str| header.iter().position(|h| *h == n);
    let (c_lap, c_ms, c_pkt) = (col("lap"), col("lap_ms"), col("packet_id"));
    // Real time of source row j: j / 60 · k → target row m at m / 60 comes from j = m / k
    let n_out = ((rows.len() - 1) as f64 * k).floor() as usize + 1;
    let mut out = String::with_capacity(text.len());
    out.push_str(&header.join(","));
    out.push('\n');
    for m in 0..n_out {
        let j = m as f64 / k;
        let (j0, f) = (j.floor() as usize, j - j.floor());
        let j1 = (j0 + 1).min(rows.len() - 1);
        let (a, b) = (&rows[j0.min(rows.len() - 1)], &rows[j1]);
        let near = if f < 0.5 { a } else { b };
        let cells: Vec<String> = (0..header.len())
            .map(|i| {
                if Some(i) == c_lap {
                    return "1".into();
                }
                if Some(i) == c_ms {
                    return format!("{}", (m as f64 * 1000.0 / 60.0).round() as i64);
                }
                if Some(i) == c_pkt {
                    return m.to_string();
                }
                let (x, y) = (a.get(i).copied().unwrap_or(""), b.get(i).copied().unwrap_or(""));
                if kinds[i] == 3 || x.is_empty() || y.is_empty() {
                    return near.get(i).copied().unwrap_or("").to_string();
                }
                let (Ok(x), Ok(y)) = (x.parse::<f64>(), y.parse::<f64>()) else { return near.get(i).copied().unwrap_or("").to_string() };
                let mut v = x + (y - x) * f;
                match kinds[i] {
                    1 => v /= k,
                    2 => v /= k * k,
                    _ => {}
                }
                format!("{v:.4}").trim_end_matches('0').trim_end_matches('.').to_string()
            })
            .collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// Positions of a captured lap (x, z).
pub fn pending_points(root: &Path, id: &str) -> Vec<[f32; 2]> {
    if !safe(id) {
        return vec![];
    }
    fs::read_to_string(root.join("pending").join(format!("{id}.csv"))).map(|t| points(&t)).unwrap_or_default()
}

/// Positions of a lap file (x, z).
fn points(text: &str) -> Vec<[f32; 2]> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let (Some(x), Some(z)) = (header.iter().position(|h| *h == "pos_x"), header.iter().position(|h| *h == "pos_z")) else { return vec![] };
    lines
        .filter_map(|l| {
            let c: Vec<&str> = l.split(',').collect();
            Some([c.get(x)?.parse().ok()?, c.get(z)?.parse().ok()?])
        })
        .collect()
}

/// Puts a pending lap into the library. `track` is used when the layout is not recognised.
pub fn save(
    root: &Path,
    id: &str,
    driver: &str,
    official_ms: i64,
    track: Option<&str>,
    track_of: &dyn Fn(&[[f32; 2]]) -> Option<String>,
) -> Result<Reference, String> {
    if !safe(id) {
        return Err("bad id".into());
    }
    let dir = root.join("pending");
    let p: Pending =
        fs::read_to_string(dir.join(format!("{id}.json"))).ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or("Záznam nenalezen.")?;
    let text = fs::read_to_string(dir.join(format!("{id}.csv"))).map_err(|e| e.to_string())?;
    let k = official_ms as f64 / p.telemetry_ms.max(1) as f64;
    if !(0.9..=1.1).contains(&k) {
        let t = p.telemetry_ms as f64 / 1000.0;
        return Err(format!("Oficiální čas nesedí na záznam ({t:.1} s z přehrávání) – zkontroluj ho."));
    }
    let track = track_of(&points(&text))
        .or_else(|| track.map(str::to_string).filter(|t| !t.trim().is_empty()))
        .ok_or("Trať nepoznám podle tvaru – vyber ji v seznamu (nebo si ji ulož v Tréninku na stránce Strategie).")?;
    let driver: String = driver.trim().chars().filter(|c| !c.is_control()).take(40).collect();
    let tag: String = driver.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let rid = format!("{}_{}", id, if tag.is_empty() { "ref".into() } else { tag });
    let out_dir = root.join(track_key(&track));
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    fs::write(out_dir.join(format!("{rid}.csv")), correct(&text, k)).map_err(|e| e.to_string())?;
    let r = Reference {
        id: rid.clone(),
        driver: if driver.is_empty() { "?".into() } else { driver },
        official_ms,
        telemetry_ms: p.telemetry_ms,
        k: (k * 10000.0).round() / 10000.0,
        car_code: p.car_code,
        car: p.car,
        track,
        length_m: p.length_m,
        created: p.created,
        source: "replay".into(),
    };
    fs::write(out_dir.join(format!("{rid}.json")), serde_json::to_string_pretty(&r).unwrap_or_default()).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(dir.join(format!("{id}.csv")));
    let _ = fs::remove_file(dir.join(format!("{id}.json")));
    Ok(r)
}

/// Removes a pending lap or a saved reference (its two files).
pub fn remove(root: &Path, id: &str) -> Result<(), String> {
    if !safe(id) {
        return Err("bad id".into());
    }
    let pend = root.join("pending");
    if pend.join(format!("{id}.json")).is_file() {
        let _ = fs::remove_file(pend.join(format!("{id}.csv")));
        return fs::remove_file(pend.join(format!("{id}.json"))).map_err(|e| e.to_string());
    }
    let r = get(root, id).ok_or("Reference nenalezena.")?;
    let d = root.join(track_key(&r.track));
    let _ = fs::remove_file(d.join(format!("{id}.csv")));
    fs::remove_file(d.join(format!("{id}.json"))).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_parse() {
        assert_eq!(parse_time("1:50,099"), Some(110_099));
        assert_eq!(parse_time("1:50.099"), Some(110_099));
        assert_eq!(parse_time("110.099"), Some(110_099));
        assert_eq!(parse_time("110099"), Some(110_099));
        assert_eq!(parse_time("1:50:099"), Some(110_099));
        assert_eq!(parse_time(""), None);
    }

    #[test]
    fn a_slow_replay_is_resampled_and_scaled() {
        // 61 rows at 60/s = 1 s of telemetry, 98 km/h constant, real lap 0.98 s
        let mut t = String::from("lap,lap_ms,packet_id,speed_kmh,sway,pos_x,gear\n");
        for i in 0..61 {
            t.push_str(&format!("9,{},{},98,1.96,{},3\n", i * 16, 1000 + i, i as f64 * 0.4537));
        }
        let out = correct(&t, 0.98);
        let rows: Vec<Vec<&str>> = out.lines().skip(1).map(|l| l.split(',').collect()).collect();
        assert_eq!(rows.len(), 59, "0.98 s of real time at 60 rows a second");
        assert_eq!(rows[0][0], "1");
        let v: f64 = rows[30][3].parse().unwrap();
        assert!((v - 100.0).abs() < 1e-6, "{v}");
        let g: f64 = rows[30][4].parse().unwrap();
        assert!((g - 1.96 / 0.98 / 0.98).abs() < 1e-3, "{g}");
        // Position at real time 0.5 s = telemetry time 0.5/0.98 s
        let x: f64 = rows[30][5].parse().unwrap();
        assert!((x - 30.0 / 0.98 * 0.4537).abs() < 1e-3, "{x}");
        assert_eq!(rows[30][6], "3");
    }
}
