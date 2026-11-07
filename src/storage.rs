//! Storage of the recordings: how much what takes, packing, summaries, clean-up and the bin.
//!
//! Recorded laps are raw CSV (60 rows per second, ~3.5 MB per lap). Three layers keep the size
//! down:
//! 1. **Packing**: a closed session is compressed lap by lap (`x.csv` → `x.csv.gz`, ~4× smaller,
//!    nothing lost; everything reads packed files).
//! 2. **Summary**: `analysis/summary.json` keeps what the coach computed (every lap, every
//!    corner, incidents, shifts, starts, setups). The notebook and the briefing need nothing more.
//! 3. **Clean-up** of sessions older than `keep_days`: only the best lap stays (plus a few normal
//!    laps in the session that holds the record of the car on the track – the fuel model needs
//!    them) and the summary. Sessions without a single complete lap can be removed.
//!
//! Nothing is deleted at once: removed files go to `trash/<date>/<session>/` and are deleted
//! for good after `trash_days` (or when the bin is emptied). Pinned sessions are never cleaned.
//! Automatic clean-up is off until the driver switches it on; packing is on.

use crate::coach::compare::{lap_files, Comparer, Comparison};
use crate::coach::lap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Normal laps kept besides the best one in the session with the record (fuel model)
const KEEP_NORMAL: usize = 3;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Pack closed sessions
    pub compress: bool,
    /// Clean old sessions by itself (otherwise only on demand)
    pub auto_clean: bool,
    /// Sessions older than this are cleaned (days)
    pub keep_days: u32,
    /// Removed files stay in the bin this long (days)
    pub trash_days: u32,
    /// Sessions never cleaned
    pub pinned: BTreeSet<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config { compress: true, auto_clean: false, keep_days: 31, trash_days: 7, pinned: BTreeSet::new() }
    }
}

/// What the coach knew about a session when it was summarised (and cleaned).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub version: u32,
    pub created: String,
    pub comparison: Comparison,
    pub kart: bool,
    pub starts: Vec<Value>,
    /// Complete laps (file, official ms) of the full session
    pub complete: Vec<(String, i64)>,
    /// Cleaned: only these laps are still there
    pub pruned: bool,
    pub kept: Vec<(String, i64)>,
}

pub struct Paths {
    pub root: PathBuf,
    pub recordings: PathBuf,
}

impl Paths {
    pub fn new(root: &Path) -> Self {
        Paths { root: root.to_path_buf(), recordings: root.join("recordings") }
    }
    fn config(&self) -> PathBuf {
        self.root.join("storage.json")
    }
    pub fn trash(&self) -> PathBuf {
        self.root.join("trash")
    }
}

pub fn load_config(p: &Paths) -> Config {
    fs::read_to_string(p.config()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_config(p: &Paths, c: &Config) -> io::Result<()> {
    write_atomic(&p.config(), serde_json::to_string_pretty(c).unwrap_or_default().as_bytes())
}

fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data)?;
    fs::rename(tmp, path)
}

fn summary_path(session: &Path) -> PathBuf {
    session.join("analysis").join("summary.json")
}

pub fn read_summary(session: &Path) -> Option<Summary> {
    serde_json::from_str(&fs::read_to_string(summary_path(session)).ok()?).ok()
}

/// Laps still there in a cleaned session (file, official ms).
pub fn kept_laps(session: &Path) -> Option<Vec<(String, i64)>> {
    read_summary(session).filter(|s| s.pruned).map(|s| s.kept)
}

/// The comparison of a session: from its summary when it was cleaned, else computed.
pub fn comparison(session: &Path, comparer: &mut Comparer) -> io::Result<Comparison> {
    match read_summary(session).filter(|s| s.pruned) {
        Some(s) => Ok(s.comparison),
        None => comparer.compare(session),
    }
}

fn now_local() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// Writes (or refreshes) the summary of a session that is not cleaned yet.
pub fn summarise(session: &Path, comparer: &mut Comparer) -> io::Result<Summary> {
    if let Some(s) = read_summary(session).filter(|s| s.pruned) {
        return Ok(s);
    }
    let comparison = comparer.compare(session)?;
    let complete: Vec<(String, i64)> = comparer.complete_laps(session).iter().map(|(p, ms)| (lap::lap_name(p), *ms)).collect();
    let kart = comparison.reference.as_ref().and_then(|r| lap::load(&session.join(&r.file)).ok()).is_some_and(|rows| lap::is_kart(&rows));
    let starts = crate::coach::start::of_session(session).into_iter().map(|s| json!(s)).collect();
    let s = Summary { version: 1, created: now_local(), comparison, kart, starts, complete, pruned: false, kept: Vec::new() };
    write_atomic(&summary_path(session), serde_json::to_string(&s).unwrap_or_default().as_bytes())?;
    Ok(s)
}

/// Packs every unpacked lap file of a session. Each file is checked after packing before the
/// original goes. Returns the bytes saved.
pub fn pack(session: &Path) -> io::Result<u64> {
    let mut saved = 0u64;
    for f in lap_files(session) {
        if f.extension().is_some_and(|e| e == "gz") {
            continue;
        }
        let text = fs::read(&f)?;
        let gz = lap::gz_path(&f);
        let tmp = gz.with_extension("gz.tmp");
        {
            let mut enc = flate2::write::GzEncoder::new(io::BufWriter::new(fs::File::create(&tmp)?), flate2::Compression::new(6));
            enc.write_all(&text)?;
            enc.finish()?.flush()?;
        }
        // Read back: same text, or keep the original
        let back = {
            use std::io::Read;
            let mut v = Vec::with_capacity(text.len());
            flate2::read::GzDecoder::new(fs::File::open(&tmp)?).read_to_end(&mut v)?;
            v
        };
        if back != text {
            let _ = fs::remove_file(&tmp);
            return Err(io::Error::other(format!("packing check failed: {}", f.display())));
        }
        // Keep the time of the recording: the age of a session is taken from its lap files
        if let Ok(t) = fs::metadata(&f).and_then(|m| m.modified()) {
            let _ = fs::File::options().write(true).open(&tmp).and_then(|h| h.set_modified(t));
        }
        fs::rename(&tmp, &gz)?;
        saved += (text.len() as u64).saturating_sub(fs::metadata(&gz).map(|m| m.len()).unwrap_or(0));
        fs::remove_file(&f)?;
    }
    Ok(saved)
}

/// Size of everything below a path (bytes).
pub fn size_of(path: &Path) -> u64 {
    match fs::metadata(path) {
        Ok(m) if m.is_dir() => fs::read_dir(path).into_iter().flatten().flatten().map(|e| size_of(&e.path())).sum(),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

fn age_days(session: &Path) -> f64 {
    let newest = lap_files(session).iter().filter_map(|f| fs::metadata(f).ok()?.modified().ok()).max();
    let t = newest.or_else(|| fs::metadata(session).ok()?.modified().ok()).unwrap_or(SystemTime::now());
    SystemTime::now().duration_since(t).unwrap_or_default().as_secs_f64() / 86_400.0
}

/// A session is still being written (or was a moment ago): never touched.
fn busy(session: &Path) -> bool {
    age_days(session) * 86_400.0 < 30.0 * 60.0
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && !n.contains(['/', '\\']) && n != ".." && n != "."
}

/// Moves `files` (inside `session`) to the bin: `trash/<date>/<session>/`.
fn to_trash(p: &Paths, session: &Path, files: &[PathBuf]) -> io::Result<u64> {
    let name = session.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = p.trash().join(chrono::Local::now().format("%Y-%m-%d").to_string()).join(&name);
    fs::create_dir_all(&dir)?;
    let mut moved = 0;
    for f in files {
        moved += fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        let target = dir.join(f.file_name().unwrap_or_default());
        fs::rename(f, &target)?;
    }
    Ok(moved)
}

/// What a clean-up of one session keeps: the best lap, and in the record session of the car on
/// the track also the latest normal laps (fuel model).
fn keep_set(sum: &Summary, record: bool) -> Vec<(String, i64)> {
    let Some(r) = sum.comparison.reference.as_ref() else { return Vec::new() };
    let mut keep = vec![(r.file.clone(), r.ms)];
    if record {
        let limit = r.ms as f64 * 1.07;
        for l in sum.comparison.laps.iter().rev().filter(|l| l.file != r.file && l.ms as f64 <= limit).take(KEEP_NORMAL) {
            keep.push((l.file.clone(), l.ms));
        }
    }
    keep
}

/// Cleans a session: summary first, then every lap file except the kept ones to the bin.
/// `record`: the session holds the best lap of the car on its track. Returns the bytes moved.
pub fn prune(p: &Paths, session: &Path, comparer: &mut Comparer, record: bool) -> io::Result<u64> {
    let mut sum = summarise(session, comparer)?;
    if sum.pruned {
        return Ok(0);
    }
    let keep = keep_set(&sum, record);
    if keep.is_empty() {
        return Err(io::Error::other("no complete lap to keep"));
    }
    let names: BTreeSet<&str> = keep.iter().map(|k| k.0.as_str()).collect();
    let drop: Vec<PathBuf> = lap_files(session).into_iter().filter(|f| !names.contains(lap::lap_name(f).as_str())).collect();
    // Summary marked first: if moving stops halfway, the session still reads right
    sum.pruned = true;
    sum.kept = keep;
    write_atomic(&summary_path(session), serde_json::to_string(&sum).unwrap_or_default().as_bytes())?;
    to_trash(p, session, &drop)
}

/// Moves a whole session to the bin.
pub fn remove_session(p: &Paths, name: &str) -> io::Result<u64> {
    if !valid_name(name) {
        return Err(io::Error::other("bad name"));
    }
    let src = p.recordings.join(name);
    let size = size_of(&src);
    let dir = p.trash().join(chrono::Local::now().format("%Y-%m-%d").to_string());
    fs::create_dir_all(&dir)?;
    let mut target = dir.join(name);
    if target.exists() {
        // Laps of the session were cleaned into the bin the same day: keep both
        target = dir.join(format!("{name}__celá"));
    }
    fs::rename(&src, &target)?;
    Ok(size)
}

/// Bin entries: (date, session folder, bytes, files).
pub fn trash_entries(p: &Paths) -> Vec<(String, String, u64, usize)> {
    let mut out = Vec::new();
    for day in fs::read_dir(p.trash()).into_iter().flatten().flatten() {
        let date = day.file_name().to_string_lossy().into_owned();
        for s in fs::read_dir(day.path()).into_iter().flatten().flatten() {
            let n = s.file_name().to_string_lossy().into_owned();
            let files = fs::read_dir(s.path()).into_iter().flatten().count();
            out.push((date.clone(), n, size_of(&s.path()), files));
        }
    }
    out.sort();
    out.reverse();
    out
}

/// Puts a bin entry back: a whole session as it was, cleaned laps into their session (which is
/// then no longer marked as cleaned).
pub fn restore(p: &Paths, date: &str, name: &str) -> io::Result<()> {
    if !valid_name(date) || !valid_name(name) {
        return Err(io::Error::other("bad name"));
    }
    let src = p.trash().join(date).join(name);
    let session_name = name.strip_suffix("__celá").unwrap_or(name);
    let target = p.recordings.join(session_name);
    if !target.exists() {
        fs::rename(&src, &target)?;
    } else {
        for f in fs::read_dir(&src)?.flatten() {
            let to = target.join(f.file_name());
            if !to.exists() {
                fs::rename(f.path(), to)?;
            }
        }
        let _ = fs::remove_dir_all(&src);
        if let Some(mut s) = read_summary(&target) {
            s.pruned = false;
            s.kept.clear();
            write_atomic(&summary_path(&target), serde_json::to_string(&s).unwrap_or_default().as_bytes())?;
        }
    }
    let day = p.trash().join(date);
    if fs::read_dir(&day).map(|mut d| d.next().is_none()).unwrap_or(false) {
        let _ = fs::remove_dir(day);
    }
    Ok(())
}

/// Deletes bin entries for good: all, or those older than `older_days`.
pub fn purge(p: &Paths, older_days: Option<u32>) -> io::Result<u64> {
    let mut freed = 0;
    let today = chrono::Local::now().date_naive();
    for day in fs::read_dir(p.trash()).into_iter().flatten().flatten() {
        let date = day.file_name().to_string_lossy().into_owned();
        let old = match (older_days, chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")) {
            (None, _) => true,
            (Some(d), Ok(dt)) => (today - dt).num_days() >= d as i64,
            (Some(_), Err(_)) => false,
        };
        if old {
            freed += size_of(&day.path());
            fs::remove_dir_all(day.path())?;
        }
    }
    Ok(freed)
}

/// Free and total space of the disk holding `path` (bytes).
pub fn disk(path: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: valid C string and a zeroed struct for the result
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        let f = st.f_frsize as u64;
        Some((st.f_bavail as u64 * f, st.f_blocks as u64 * f))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// One session as the storage page shows it.
#[derive(Clone, Debug, Serialize)]
pub struct SessionRow {
    pub session: String,
    pub car: String,
    pub track: Option<String>,
    pub started: String,
    pub files: usize,
    pub laps: usize,
    pub best_ms: Option<i64>,
    pub bytes: u64,
    /// Bytes the lap files would take unpacked (estimate for packed ones)
    pub age_days: f64,
    /// "live", "raw", "packed", "pruned"
    pub state: &'static str,
    pub pinned: bool,
    /// Holds the best lap of the car on the track
    pub record: bool,
    /// No complete lap at all
    pub empty: bool,
}

/// What a clean-up would do.
#[derive(Clone, Debug, Serialize)]
pub struct Step {
    pub session: String,
    /// "pack", "prune" or "remove" (no complete lap)
    pub action: &'static str,
    pub frees: u64,
    pub why: String,
}

/// Packed size of raw lap files (measured: ~26 % of the CSV)
const PACK_RATIO: f64 = 0.27;

pub fn session_rows(p: &Paths, cfg: &Config, all: &[crate::coach::library::SessionInfo]) -> Vec<SessionRow> {
    let mut dirs: Vec<PathBuf> =
        fs::read_dir(&p.recordings).into_iter().flatten().flatten().map(|e| e.path()).filter(|d| d.is_dir()).collect();
    dirs.sort();
    dirs.reverse();
    // Record session per car and track
    let mut records: BTreeSet<String> = BTreeSet::new();
    {
        let mut best: std::collections::BTreeMap<(i32, String), (i64, String)> = Default::default();
        for i in all {
            if let (Some(t), Some(ms)) = (&i.track, i.best_ms) {
                let k = (i.car_code, crate::strategy::practice::track_key(t));
                if best.get(&k).is_none_or(|b| ms < b.0) {
                    best.insert(k, (ms, i.session.clone()));
                }
            }
        }
        records.extend(best.into_values().map(|v| v.1));
    }
    let newest = dirs.first().cloned();
    dirs.iter()
        .map(|d| {
            let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let files = lap_files(d);
            let info = all.iter().find(|i| i.session == name);
            let meta: Value =
                fs::read_to_string(d.join("session.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            let sum = read_summary(d);
            let pruned = sum.as_ref().is_some_and(|s| s.pruned);
            let packed = !files.is_empty() && files.iter().all(|f| f.extension().is_some_and(|e| e == "gz"));
            let live = Some(d) == newest.as_ref() && busy(d);
            let state = if live {
                "live"
            } else if pruned {
                "pruned"
            } else if packed {
                "packed"
            } else {
                "raw"
            };
            let laps = if pruned { sum.as_ref().map_or(0, |s| s.complete.len()) } else { info.map_or(0, |i| i.laps) };
            SessionRow {
                session: name.clone(),
                car: meta["car"].as_str().unwrap_or_default().to_string(),
                track: info.and_then(|i| i.track.clone()),
                started: meta["started"].as_str().unwrap_or_default().to_string(),
                files: files.len(),
                laps,
                best_ms: info.and_then(|i| i.best_ms),
                bytes: size_of(d),
                age_days: (age_days(d) * 10.0).round() / 10.0,
                state,
                pinned: cfg.pinned.contains(&name),
                record: records.contains(&name),
                empty: laps == 0,
            }
        })
        .collect()
}

/// The recommended clean-up: pack closed sessions; clean those older than `keep_days`; remove
/// sessions without a complete lap that are older than a day. Pinned and live ones are left.
pub fn plan(rows: &[SessionRow], cfg: &Config) -> Vec<Step> {
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r.state != "live" && !r.pinned) {
        if r.empty && r.age_days >= 1.0 {
            out.push(Step { session: r.session.clone(), action: "remove", frees: r.bytes, why: "žádné celé kolo".into() });
        } else if r.age_days >= cfg.keep_days as f64 && r.state != "pruned" && !r.empty {
            // Kept: the best lap (+ 3 in the record session), packed
            let per_lap = r.bytes as f64 / r.files.max(1) as f64;
            let keep = if r.record { 1 + KEEP_NORMAL } else { 1 } as f64;
            let ratio = if r.state == "raw" { PACK_RATIO } else { 1.0 };
            let frees = (r.bytes as f64 - keep * per_lap * ratio).max(0.0) as u64;
            out.push(Step { session: r.session.clone(), action: "prune", frees, why: format!("starší než {} dní", cfg.keep_days) });
        } else if r.state == "raw" && cfg.compress {
            out.push(Step {
                session: r.session.clone(),
                action: "pack",
                frees: (r.bytes as f64 * (1.0 - PACK_RATIO)) as u64,
                why: "bez ztráty dat".into(),
            });
        }
    }
    out
}

/// One change of the stored files at a time (dashboard and the coach thread).
static OPS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Carries out one step. `record`: the session holds a record (keeps more laps when cleaned).
pub fn run_step(p: &Paths, s: &Step, record: bool, comparer: &mut Comparer) -> io::Result<u64> {
    let _one = OPS.lock().unwrap_or_else(|e| e.into_inner());
    if !valid_name(&s.session) {
        return Err(io::Error::other("bad name"));
    }
    let dir = p.recordings.join(&s.session);
    if busy(&dir) {
        return Err(io::Error::other("session is being recorded"));
    }
    match s.action {
        "pack" => {
            let _ = summarise(&dir, comparer);
            pack(&dir)
        }
        "prune" => {
            let moved = prune(p, &dir, comparer, record)?;
            pack(&dir)?;
            Ok(moved)
        }
        "remove" => remove_session(p, &s.session),
        _ => Err(io::Error::other("unknown action")),
    }
}

/// Background housekeeping (coach thread, when nothing is being driven): summarise and pack
/// closed sessions one at a time; with automatic clean-up also clean old sessions and empty the
/// bin of old entries. Returns what it did, for the log.
pub fn housekeeping(
    root: &Path,
    comparer: &mut Comparer,
    all: &dyn Fn(&mut Comparer) -> Vec<crate::coach::library::SessionInfo>,
) -> Option<String> {
    let p = Paths::new(root);
    let cfg = load_config(&p);
    let mut dirs: Vec<PathBuf> = fs::read_dir(&p.recordings).ok()?.flatten().map(|e| e.path()).filter(|d| d.is_dir()).collect();
    dirs.sort();
    // Something is being driven: later
    if dirs.last().is_some_and(|d| busy(d)) {
        return None;
    }
    if cfg.auto_clean {
        let _ = purge(&p, Some(cfg.trash_days));
        let index = all(comparer);
        let rows = session_rows(&p, &cfg, &index);
        if let Some(step) = plan(&rows, &cfg).into_iter().find(|s| s.action == "prune") {
            let record = rows.iter().any(|r| r.session == step.session && r.record);
            return run_step(&p, &step, record, comparer).ok().map(|b| format!("uklizeno {}: {} MB do koše", step.session, b / 1_000_000));
        }
    }
    if cfg.compress {
        let _one = OPS.lock().unwrap_or_else(|e| e.into_inner());
        for d in &dirs {
            let raw = lap_files(d).iter().any(|f| f.extension().is_some_and(|e| e == "csv"));
            if raw && !busy(d) {
                let _ = summarise(d, comparer);
                let saved = pack(d).ok()?;
                let name = d.file_name()?.to_string_lossy().into_owned();
                return Some(format!("zabaleno {name}: −{} MB", saved / 1_000_000));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, state: &'static str, age: f64, laps: usize, pinned: bool) -> SessionRow {
        SessionRow {
            session: name.into(),
            car: "x".into(),
            track: None,
            started: String::new(),
            files: 10,
            laps,
            best_ms: None,
            bytes: 100_000_000,
            age_days: age,
            state,
            pinned,
            record: false,
            empty: laps == 0,
        }
    }

    #[test]
    fn plan_packs_cleans_old_and_leaves_pinned_and_live() {
        let cfg = Config::default();
        let rows = vec![
            row("live", "live", 0.0, 5, false),
            row("new", "raw", 3.0, 5, false),
            row("old", "packed", 40.0, 5, false),
            row("old_pinned", "raw", 40.0, 5, true),
            row("aborted", "raw", 2.0, 0, false),
            row("done", "pruned", 60.0, 5, false),
        ];
        let p = plan(&rows, &cfg);
        let got: Vec<(&str, &str)> = p.iter().map(|s| (s.session.as_str(), s.action)).collect();
        assert_eq!(got, [("new", "pack"), ("old", "prune"), ("aborted", "remove")]);
        // A packed old session keeps one lap of ten
        assert_eq!(p[1].frees, 90_000_000);
    }

    #[test]
    fn packing_keeps_the_text() {
        let dir = std::env::temp_dir().join(format!("bbb_pack_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = "lap,speed_kmh\n".to_string() + &"1,123.4\n".repeat(5000);
        std::fs::write(dir.join("001_lap01.csv"), &text).unwrap();
        let saved = pack(&dir).unwrap();
        assert!(saved > 30_000, "{saved}");
        assert!(!dir.join("001_lap01.csv").exists());
        assert_eq!(lap::read_text(&dir.join("001_lap01.csv")).unwrap(), text);
        assert_eq!(lap_files(&dir).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
