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

/// The laps of a session against the saved reference `id` (see [`Comparer::against`]).
pub fn compare(
    comparer: &mut super::compare::Comparer,
    root: &Path,
    session: &Path,
    id: &str,
) -> std::io::Result<super::compare::Comparison> {
    let not_found = || std::io::Error::new(std::io::ErrorKind::NotFound, "reference not found");
    let r = get(root, id).ok_or_else(not_found)?;
    let f = file(root, id).ok_or_else(not_found)?;
    let ext = super::compare::External { id: r.id.clone(), driver: r.driver.clone(), car_code: r.car_code, car: r.car.clone() };
    comparer.against(session, &f, r.official_ms, ext)
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

/// A lap of one's own recordings as a reference (a training race, a good lap of a friend's
/// session recorded on this dashboard): copied as it is (real time, k = 1).
#[allow(clippy::too_many_arguments)]
pub fn from_lap(
    root: &Path,
    lap: &Path,
    driver: &str,
    official_ms: i64,
    car_code: i32,
    car: &str,
    track: &str,
    created: &str,
) -> Result<Reference, String> {
    let text = super::lap::read_text(lap).map_err(|e| e.to_string())?;
    let pts = points(&text);
    if pts.len() < 100 {
        return Err("Kolo je moc krátké.".into());
    }
    let length_m = pts.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt() as f64).sum::<f64>();
    let driver: String = driver.trim().chars().filter(|c| !c.is_control()).take(40).collect();
    let driver = if driver.is_empty() { "Já".to_string() } else { driver };
    let tag: String = driver.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let stamp: String = created.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
    let rid = format!("own{stamp}_{official_ms}_{tag}");
    let out_dir = root.join(track_key(track));
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    fs::write(out_dir.join(format!("{rid}.csv")), &text).map_err(|e| e.to_string())?;
    let r = Reference {
        id: rid.clone(),
        driver,
        official_ms,
        telemetry_ms: official_ms,
        k: 1.0,
        car_code,
        car: car.to_string(),
        track: track.to_string(),
        length_m: length_m.round(),
        created: created.to_string(),
        source: "recording".into(),
    };
    fs::write(out_dir.join(format!("{rid}.json")), serde_json::to_string_pretty(&r).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(r)
}

/// First line of a shared lap (`.boxlap`): this, then the [`Reference`] as JSON; the lap CSV
/// follows. The whole file is gzip.
const PACK_HEAD: &str = "#boxboxbox-lap 1 ";
/// A shared lap unpacked is never bigger than this (a lap at 60 Hz is ~5 MB)
const PACK_MAX: u64 = 40 << 20;
/// Columns a lap needs to be compared at all
const PACK_COLUMNS: [&str; 4] = ["pos_x", "pos_z", "speed_kmh", "lap_ms"];

/// 129840 → "2:09,840"
fn lap_text(ms: i64) -> String {
    format!("{}:{:02},{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// A saved reference as one file to send to a friend: (file name, gzip bytes).
pub fn export(root: &Path, id: &str) -> Result<(String, Vec<u8>), String> {
    use std::io::Write;
    let r = get(root, id).ok_or("Reference nenalezena.")?;
    let lap = file(root, id).ok_or("Soubor reference chybí.")?;
    let text = super::lap::read_text(&lap).map_err(|e| e.to_string())?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let meta = serde_json::to_string(&r).map_err(|e| e.to_string())?;
    gz.write_all(format!("{PACK_HEAD}{meta}\n").as_bytes()).map_err(|e| e.to_string())?;
    gz.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    let bytes = gz.finish().map_err(|e| e.to_string())?;
    let ms = r.official_ms;
    let time = format!("{}m{:02}s{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000);
    let tag = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>();
    Ok((format!("{}_{}_{}.boxlap", track_key(&r.track), time, tag(&r.driver)), bytes))
}

/// A lap a friend shared (`.boxlap` from [`export`]) into the references. The track is the
/// stored layout its line runs on (`track_of`), else the name in the file. Nothing of the file
/// is trusted: size, columns, names and id are checked or made anew.
pub fn import(root: &Path, bytes: &[u8], track_of: &dyn Fn(&[[f32; 2]]) -> Option<String>, created: &str) -> Result<Reference, String> {
    use std::io::Read;
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes)
        .take(PACK_MAX)
        .read_to_string(&mut text)
        .map_err(|_| "Tohle není soubor kola z BOXBOXBOX (.boxlap).".to_string())?;
    let (head, csv) = text.split_once('\n').ok_or("Soubor je prázdný.")?;
    let meta = head.strip_prefix(PACK_HEAD).ok_or("Tohle není soubor kola z BOXBOXBOX (.boxlap), nebo je z novější verze.")?;
    let theirs: Reference = serde_json::from_str(meta).map_err(|_| "Popis kola v souboru je poškozený.".to_string())?;
    let columns: Vec<&str> = csv.lines().next().unwrap_or_default().split(',').collect();
    if let Some(c) = PACK_COLUMNS.iter().find(|c| !columns.contains(c)) {
        return Err(format!("V souboru chybí sloupec {c}."));
    }
    let pts = points(csv);
    if pts.len() < 100 || theirs.official_ms <= 0 {
        return Err("Kolo v souboru je moc krátké.".into());
    }
    let clean = |s: &str, n: usize| s.trim().chars().filter(|c| !c.is_control()).take(n).collect::<String>();
    let track = track_of(&pts).unwrap_or_else(|| clean(&theirs.track, 80));
    if track.is_empty() {
        return Err("Nevím, na jaké trati kolo je.".into());
    }
    let driver = clean(&theirs.driver, 40);
    let driver = if driver.is_empty() { "Kamarád".to_string() } else { driver };
    // The same lap once (imported twice, or one's own lap coming back)
    if list(root).iter().any(|r| {
        track_key(&r.track) == track_key(&track)
            && r.official_ms == theirs.official_ms
            && r.driver == driver
            && r.car_code == theirs.car_code
    }) {
        return Err(format!("Kolo {driver} {} na {track} už v referencích máš.", lap_text(theirs.official_ms)));
    }
    let tag: String = driver.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let stamp: String = created.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
    let rid = format!("shr{stamp}_{}_{tag}", theirs.official_ms);
    let length_m = pts.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt() as f64).sum::<f64>();
    let r = Reference {
        id: rid.clone(),
        driver,
        official_ms: theirs.official_ms,
        telemetry_ms: theirs.telemetry_ms,
        k: if theirs.k > 0.5 && theirs.k < 1.5 { theirs.k } else { 1.0 },
        car_code: theirs.car_code,
        car: clean(&theirs.car, 60),
        track: track.clone(),
        length_m: length_m.round(),
        created: created.to_string(),
        source: "shared".into(),
    };
    let dir = root.join(track_key(&track));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("{rid}.csv")), csv).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("{rid}.json")), serde_json::to_string_pretty(&r).unwrap_or_default()).map_err(|e| e.to_string())?;
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
    fn a_shared_lap_goes_out_and_comes_back_as_a_reference() {
        let dir = std::env::temp_dir().join(format!("share-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // An own lap: a circle of 300 rows
        let mut t = String::from("lap,lap_ms,speed_kmh,pos_x,pos_z\n");
        for i in 0..300 {
            let a = i as f64 / 300.0 * std::f64::consts::TAU;
            t.push_str(&format!("3,{},180,{:.2},{:.2}\n", i * 16, 300.0 * a.cos(), 300.0 * a.sin()));
        }
        let lap = dir.join("lap.csv");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&lap, &t).unwrap();
        let mine =
            from_lap(&dir.join("mine"), &lap, "Viktor", 129_840, 3531, "AMG GT3 '20", "Spa-Francorchamps", "2026-11-23 21:43").unwrap();
        let (name, bytes) = export(&dir.join("mine"), &mine.id).unwrap();
        assert_eq!(name, "spafrancorchamps_2m09s840_Viktor.boxlap");
        // The friend's dashboard: the track is the stored layout the line runs on
        let theirs = dir.join("theirs");
        let r = import(&theirs, &bytes, &|_: &[[f32; 2]]| Some("Spa-Francorchamps".into()), "2026-11-24 14:00").unwrap();
        assert_eq!((r.driver.as_str(), r.official_ms, r.car_code, r.source.as_str()), ("Viktor", 129_840, 3531, "shared"));
        assert!(r.id.starts_with("shr20261124140") && file(&theirs, &r.id).is_some());
        assert_eq!(fs::read_to_string(file(&theirs, &r.id).unwrap()).unwrap(), t);
        // The same file again: not a second copy
        let again = import(&theirs, &bytes, &|_: &[[f32; 2]]| Some("Spa-Francorchamps".into()), "2026-11-24 14:05");
        assert!(again.unwrap_err().contains("už v referencích máš"));
        assert_eq!(list(&theirs).len(), 1);
        // Not a lap file, or one without positions: refused
        assert!(import(&theirs, b"hello", &|_: &[[f32; 2]]| None, "x").is_err());
        let mut bad = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(
            &mut bad,
            format!("{PACK_HEAD}{}\nlap,speed_kmh\n1,2\n", serde_json::to_string(&mine).unwrap()).as_bytes(),
        )
        .unwrap();
        assert!(import(&theirs, &bad.finish().unwrap(), &|_: &[[f32; 2]]| None, "x").unwrap_err().contains("pos_x"));
        let _ = fs::remove_dir_all(&dir);
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
