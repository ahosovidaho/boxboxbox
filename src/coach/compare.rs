//! Lap comparison: every complete lap of a session is mapped onto the fastest lap and compared
//! corner by corner (time lost, braking point, apex speed, full-throttle point, balance).

use super::lap::{self, Row, G, HZ};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Per-corner figures of one lap.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CornerStats {
    pub brake_at: Option<f64>,
    pub brake_max: f64,
    pub v_entry: f64,
    pub v_min: f64,
    pub v_exit: f64,
    pub full_at: Option<f64>,
    pub gear_min: Option<i64>,
    pub under: f64,
    pub over: f64,
    pub rear_slide: f64,
    pub glat: f64,
    pub tcs: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LapCorner {
    pub n: usize,
    /// Time lost against the reference in this corner section (s)
    pub dt: f64,
    #[serde(flatten)]
    pub stats: Option<CornerStats>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LapResult {
    pub file: String,
    pub lap: i64,
    pub ms: i64,
    pub delta_ms: i64,
    pub corners: Vec<LapCorner>,
    /// Spins and slides in this lap
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incidents: Vec<super::incident::Incident>,
}

impl LapResult {
    pub fn corner(&self, n: usize) -> &LapCorner {
        self.corners.iter().find(|c| c.n == n).expect("corner exists in every lap")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CornerInfo {
    pub n: usize,
    pub dir: String,
    pub x: f64,
    pub z: f64,
    pub s: i64,
    #[serde(rename = "ref")]
    pub reference: Option<CornerStats>,
    /// Where the reference lap starts braking for this corner (x, z)
    #[serde(default)]
    pub brake_pos: Option<[f64; 2]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub file: String,
    pub ms: i64,
    pub lap: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Comparison {
    pub reference: Option<Reference>,
    pub steer_ratio: f64,
    pub corners: Vec<CornerInfo>,
    pub laps: Vec<LapResult>,
    /// Sum of the best corner sections over all laps
    pub theoretical_ms: Option<f64>,
}

impl Comparison {
    pub fn reference_lap(&self) -> Option<&LapResult> {
        let file = &self.reference.as_ref()?.file;
        self.laps.iter().find(|l| &l.file == file)
    }
}

/// Lightweight facts about a lap file, read without keeping the data in memory.
#[derive(Clone, Debug)]
struct Meta {
    n: usize,
    lap: Option<i64>,
    first_ms: Option<f64>,
    last_ms: Option<f64>,
    /// Official time of the previous lap (sent in the packets of this lap)
    prev_official: Option<f64>,
    /// Distance (m) between the first and the last position of the lap
    gap_m: Option<f64>,
}

/// A lap that does not end where it started (m) is not a full lap: on tracks with separate
/// start and finish lines (Nürburgring GP) the first race lap starts on the grid, ~350 m short.
pub const CLOSED_MAX_M: f64 = 80.0;

/// Corner definition stored per session in `corners.json` so numbering stays stable.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CornerDef {
    n: usize,
    dir: String,
    from: [f64; 2],
    apex: [f64; 2],
    to: [f64; 2],
}

#[derive(Serialize, Deserialize)]
struct CornerFile {
    source: i64,
    corners: Vec<CornerDef>,
}

/// A corner mapped onto a reference lap (indices into the reference rows).
#[derive(Clone, Debug)]
struct Corner {
    n: usize,
    dir: String,
    from: usize,
    to: usize,
    apex: usize,
    x: f64,
    z: f64,
    s: f64,
}

/// Cache key of a lap file: path, modification time, size.
type FileKey = (PathBuf, SystemTime, u64);
/// Cache key of a comparison: lap file, reference file, corner windows.
type ResultKey = (PathBuf, PathBuf, Vec<(usize, usize, usize)>);

/// Keeps caches between runs so only new laps are loaded and projected.
#[derive(Default)]
pub struct Comparer {
    meta: HashMap<FileKey, Meta>,
    results: HashMap<ResultKey, LapResult>,
}

pub fn lap_files(session: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(session)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "csv"))
        .collect();
    files.sort();
    files
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl Comparer {
    fn meta(&mut self, path: &Path) -> Meta {
        let md = fs::metadata(path).ok();
        let key = (
            path.to_path_buf(),
            md.as_ref().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH),
            md.map(|m| m.len()).unwrap_or(0),
        );
        if let Some(m) = self.meta.get(&key) {
            return m.clone();
        }
        let text = fs::read_to_string(path).unwrap_or_default();
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
        let col = |n: &str| header.iter().position(|c| *c == n);
        let (c_lap, c_ms, c_last) = (col("lap"), col("lap_ms"), col("last_lap_ms"));
        let (c_x, c_z) = (col("pos_x"), col("pos_z"));
        let num = |line: &str, c: Option<usize>| -> Option<f64> {
            let cell = line.split(',').nth(c?)?;
            if cell.is_empty() {
                None
            } else {
                cell.parse().ok()
            }
        };
        let rows: Vec<&str> = lines.filter(|l| !l.is_empty()).collect();
        let first = rows.first().copied();
        let last = rows.last().copied();
        let early = rows.get(rows.len().min(6).saturating_sub(1)).copied().filter(|_| !rows.is_empty());
        let m = Meta {
            n: rows.len(),
            lap: first.and_then(|l| num(l, c_lap)).map(|v| v as i64),
            first_ms: first.and_then(|l| num(l, c_ms)),
            last_ms: last.and_then(|l| num(l, c_ms)),
            prev_official: early.and_then(|l| num(l, c_last)),
            gap_m: match (first, last) {
                (Some(a), Some(b)) => (|| Some((num(a, c_x)? - num(b, c_x)?).hypot(num(a, c_z)? - num(b, c_z)?)))(),
                _ => None,
            },
        };
        self.meta.retain(|k, _| k.0 != path);
        self.meta.insert(key, m.clone());
        m
    }

    /// All complete laps `(file, official ms)`: started at the line, ended where they started and
    /// the recorded duration matches the official time (sent as `last_lap_ms` in the following lap).
    pub fn complete_laps(&mut self, session: &Path) -> Vec<(PathBuf, i64)> {
        let files = lap_files(session);
        let mut out = Vec::new();
        for pair in files.windows(2) {
            let (a, b) = (self.meta(&pair[0]), self.meta(&pair[1]));
            if a.n < 600 || b.n == 0 || b.lap != Some(a.lap.unwrap_or(0) + 1) {
                continue;
            }
            let recorded = a.last_ms.unwrap_or(0.0);
            if let Some(official) = b.prev_official.filter(|o| *o > 0.0) {
                let closed = a.gap_m.is_none_or(|g| g < CLOSED_MAX_M);
                if (recorded + 1000.0 / HZ - official).abs() < 400.0 && a.first_ms.unwrap_or(0.0) < 100.0 && closed {
                    out.push((pair[0].clone(), official as i64));
                }
            }
        }
        out
    }

    /// Compares all complete laps of a session against the fastest one.
    pub fn compare(&mut self, session: &Path) -> std::io::Result<Comparison> {
        let laps = self.complete_laps(session);
        let Some((ref_path, ref_ms)) = laps.iter().min_by_key(|(_, ms)| *ms).cloned() else {
            return Ok(Comparison::default());
        };
        let mut reference = lap::load(&ref_path)?;
        let (ratio, _) = lap::steer_ratio(&reference);
        prepare(&mut reference, ratio);
        project(&mut reference, None);

        let corners_path = session.join("corners.json");
        if !corners_path.exists() {
            // Corners are defined once per session from the first complete lap
            if laps[0].0 == ref_path {
                write_corner_defs(&corners_path, &reference)?;
            } else {
                let mut first = lap::load(&laps[0].0)?;
                prepare(&mut first, ratio);
                write_corner_defs(&corners_path, &first)?;
            }
        }
        let corners = map_corner_defs(&corners_path, &reference)?;
        let ckey: Vec<(usize, usize, usize)> = corners.iter().map(|c| (c.n, c.from, c.to)).collect();

        let mut result = Comparison {
            reference: Some(Reference { file: file_name(&ref_path), ms: ref_ms, lap: reference[0].lap as i64 }),
            steer_ratio: lap::round_to(ratio, 1),
            ..Default::default()
        };
        for c in &corners {
            let stats = corner_stats(&reference, s_of(&reference, c.from), s_of(&reference, c.to), c.s);
            let brake_pos = stats.as_ref().and_then(|st| st.brake_at).and_then(|b| {
                reference.iter().find(|r| r.s.is_some_and(|s| s >= b)).map(|r| [lap::round_to(r.pos_x, 1), lap::round_to(r.pos_z, 1)])
            });
            result.corners.push(CornerInfo {
                n: c.n,
                dir: c.dir.clone(),
                x: lap::round_to(c.x, 1),
                z: lap::round_to(c.z, 1),
                s: lap::round_i(c.s),
                reference: stats,
                brake_pos,
            });
        }
        for (path, ms) in &laps {
            let key = (path.clone(), ref_path.clone(), ckey.clone());
            if !self.results.contains_key(&key) {
                let lap_result = if *path == ref_path {
                    evaluate_lap(path, *ms, ref_ms, &reference, &reference, &corners)
                } else {
                    let mut rows = lap::load(path)?;
                    prepare(&mut rows, ratio);
                    project(&mut rows, Some(&reference));
                    evaluate_lap(path, *ms, ref_ms, &rows, &reference, &corners)
                };
                self.results.insert(key.clone(), lap_result);
            }
            result.laps.push(self.results[&key].clone());
        }
        // Drop cached results that belong to an older reference
        self.results.retain(|k, _| k.1 == ref_path);
        result.theoretical_ms = Some(
            ref_ms as f64
                + 1000.0
                    * result
                        .corners
                        .iter()
                        .map(|c| result.laps.iter().map(|l| l.corner(c.n).dt).fold(f64::INFINITY, f64::min))
                        .sum::<f64>(),
        );
        Ok(result)
    }
}

fn prepare(rows: &mut [Row], ratio: f64) {
    lap::add_dynamics(rows, ratio);
    lap::with_distance(rows);
}

fn s_of(rows: &[Row], i: usize) -> f64 {
    rows[i].s.unwrap_or(0.0)
}

fn evaluate_lap(path: &Path, ms: i64, ref_ms: i64, rows: &[Row], reference: &[Row], corners: &[Corner]) -> LapResult {
    let lap_corners = corners
        .iter()
        .map(|c| {
            let (s0, s1) = (s_of(reference, c.from), s_of(reference, c.to));
            let dt = (time_at(rows, s1) - time_at(rows, s0)) - (time_at(reference, s1) - time_at(reference, s0));
            LapCorner { n: c.n, dt: lap::round_to(dt, 3), stats: corner_stats(rows, s0, s1, c.s) }
        })
        .collect();
    let sections: Vec<(usize, f64, f64)> =
        corners.iter().map(|c| (c.n, s_of(reference, c.from), s_of(reference, c.to))).collect::<Vec<_>>();
    let incidents = super::incident::detect(rows, reference, &sections);
    LapResult { file: file_name(path), lap: rows[0].lap as i64, ms, delta_ms: ms - ref_ms, corners: lap_corners, incidents }
}

fn dist2(r: &Row, x: f64, z: f64) -> f64 {
    (r.pos_x - x).powi(2) + (r.pos_z - z).powi(2)
}

/// Index of the minimum (first one on ties, like Python's `min`).
fn argmin(range: impl Iterator<Item = usize>, key: impl Fn(usize) -> f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for i in range {
        let k = key(i);
        if best.is_none_or(|(_, b)| k < b) {
            best = Some((i, k));
        }
    }
    best.map(|(i, _)| i)
}

/// Assigns every sample the distance along the reference lap (`None` = the lap is its own reference).
fn project(rows: &mut [Row], reference: Option<&[Row]>) {
    let ref_pts: Vec<(f64, f64, f64)> = match reference {
        Some(r) => r.iter().map(|p| (p.pos_x, p.pos_z, p.s.unwrap_or(0.0))).collect(),
        None => rows.iter().map(|p| (p.pos_x, p.pos_z, p.s.unwrap_or(0.0))).collect(),
    };
    let n = ref_pts.len();
    let mut idx = 0usize;
    for r in rows.iter_mut() {
        let (x, z) = (r.pos_x, r.pos_z);
        let d2 = |i: usize| (ref_pts[i].0 - x).powi(2) + (ref_pts[i].1 - z).powi(2);
        let (lo, hi) = (idx.saturating_sub(30), (n - 1).min(idx + 240));
        let mut best = argmin(lo..=hi, d2).unwrap_or(0);
        if d2(best).sqrt() > 25.0 {
            // Lost the reference: search globally
            best = argmin(0..n, d2).unwrap_or(0);
        }
        idx = best;
        r.s_ref = ref_pts[best].2;
    }
}

/// Lap time (s) at which `rows` reaches reference distance `s` (linear interpolation).
fn time_at(rows: &[Row], s: f64) -> f64 {
    let (mut lo, mut hi) = (0usize, rows.len() - 1);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if rows[mid].s_ref < s {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let i = lo.max(1);
    let (a, b) = (&rows[i - 1], &rows[i]);
    let ds = b.s_ref - a.s_ref;
    let f = if ds > 0.0 { (s - a.s_ref) / ds } else { 0.0 };
    ((i - 1) as f64 + f.clamp(0.0, 1.0)) / HZ
}

/// Corners of a lap including the braking zone before them; the lap is split into gapless sections.
fn corners_of(rows: &[Row]) -> Vec<Corner> {
    let raw = lap::segments(rows, |r| r.vf > 8.0 && r.sway.abs() > 7.0, 15, 20);
    let mut found: Vec<(usize, usize, usize, usize)> = Vec::new(); // start, turn, end, apex
    for (s, e) in raw {
        let mut b = s;
        while b > 0 && s_of(rows, s) - s_of(rows, b) < 250.0 && (rows[b - 1].brake > 10.0 || rows[b - 1].throttle < 200.0) {
            b -= 1;
        }
        let apex = argmin(s..=e, |i| rows[i].speed_kmh).unwrap_or(s);
        found.push((b, s, e, apex));
    }
    let mut bounds = vec![0];
    for w in found.windows(2) {
        bounds.push((w[0].2 + w[1].0) / 2);
    }
    bounds.push(rows.len() - 1);
    found
        .iter()
        .enumerate()
        .map(|(k, &(_, turn, end, apex))| {
            let a = &rows[apex];
            let yaw = lap::mean((turn..=end).map(|i| rows[i].yaw));
            Corner {
                n: k + 1,
                dir: if yaw > 0.0 { "links" } else { "rechts" }.into(),
                from: bounds[k],
                to: bounds[k + 1],
                apex,
                x: a.pos_x,
                z: a.pos_z,
                s: a.s.unwrap_or(0.0),
            }
        })
        .collect()
}

fn write_corner_defs(path: &Path, rows: &[Row]) -> std::io::Result<()> {
    let pos = |i: usize| [lap::round_to(rows[i].pos_x, 2), lap::round_to(rows[i].pos_z, 2)];
    let defs = corners_of(rows)
        .iter()
        .map(|c| CornerDef { n: c.n, dir: c.dir.clone(), from: pos(c.from), apex: pos(c.apex), to: pos(c.to) })
        .collect();
    let file = CornerFile { source: rows[0].lap as i64, corners: defs };
    fs::write(path, serde_json::to_string_pretty(&file).unwrap_or_default())
}

/// Index of the closest reference point at or after `start`. Only a third of the lap ahead is
/// searched so start/finish (beginning ≈ end) and nearby track parts are not confused.
fn nearest_from(rows: &[Row], x: f64, z: f64, start: usize) -> usize {
    let start = start.min(rows.len() - 1);
    let end = rows.len().min(start + (rows.len() / 3).max(60));
    argmin(start..end, |i| dist2(&rows[i], x, z)).unwrap_or(start)
}

/// Maps the session's corner definitions onto `reference`.
fn map_corner_defs(path: &Path, reference: &[Row]) -> std::io::Result<Vec<Corner>> {
    let file: CornerFile = serde_json::from_str(&fs::read_to_string(path)?)?;
    let mut out: Vec<Corner> = Vec::new();
    let mut prev = 0usize;
    for (k, d) in file.corners.iter().enumerate() {
        let i_from = if k == 0 { 0 } else { nearest_from(reference, d.from[0], d.from[1], prev.saturating_sub(50)) };
        let i_apex = nearest_from(reference, d.apex[0], d.apex[1], i_from);
        let i_to = nearest_from(reference, d.to[0], d.to[1], i_apex);
        prev = i_to;
        let a = &reference[i_apex];
        out.push(Corner {
            n: d.n,
            dir: d.dir.clone(),
            from: i_from,
            to: i_to,
            apex: i_apex,
            x: a.pos_x,
            z: a.pos_z,
            s: a.s.unwrap_or(0.0),
        });
    }
    if let Some(first) = out.first_mut() {
        first.from = 0; // gapless up to start/finish
    }
    if let Some(last) = out.last_mut() {
        last.to = reference.len() - 1;
    }
    Ok(out)
}

/// Figures of one lap in the section `[s_from, s_to]` (reference distances).
fn corner_stats(rows: &[Row], s_from: f64, s_to: f64, s_apex: f64) -> Option<CornerStats> {
    let seg: Vec<&Row> = rows.iter().filter(|r| s_from <= r.s_ref && r.s_ref <= s_to).collect();
    if seg.is_empty() {
        return None;
    }
    let brake_at = seg.iter().find(|r| r.brake > 25.0 && r.s_ref <= s_apex + 30.0).map(|r| r.s_ref);
    let mut near_apex: Vec<&Row> = seg.iter().copied().filter(|r| (r.s_ref - s_apex).abs() < 60.0).collect();
    if near_apex.is_empty() {
        near_apex = seg.clone();
    }
    let v_min = near_apex.iter().map(|r| r.speed_kmh).fold(f64::INFINITY, f64::min);
    let full_at = seg.iter().filter(|r| r.s_ref > s_apex - 20.0).find(|r| r.throttle > 245.0).map(|r| r.s_ref);
    let bals: Vec<f64> = seg.iter().filter(|r| r.sway.abs() > 6.0).filter_map(|r| r.bal).collect();
    let pct = |count: usize, total: usize| if total == 0 { 0.0 } else { count as f64 / total as f64 * 100.0 };
    Some(CornerStats {
        brake_at,
        brake_max: seg.iter().map(|r| r.brake).fold(f64::MIN, f64::max) / 2.55,
        v_entry: seg[0].speed_kmh,
        v_min,
        v_exit: seg[seg.len() - 1].speed_kmh,
        full_at,
        gear_min: near_apex.iter().filter(|r| r.gear > 0.0 && r.gear < 15.0).map(|r| r.gear as i64).min(),
        under: pct(bals.iter().filter(|b| **b > 2.0).count(), bals.len()),
        over: pct(bals.iter().filter(|b| **b < -1.5).count(), bals.len()),
        rear_slide: seg.iter().filter_map(|r| r.ar).map(f64::abs).fold(0.0, f64::max),
        glat: seg.iter().map(|r| r.sway.abs()).fold(f64::MIN, f64::max) / G,
        tcs: pct(seg.iter().filter(|r| r.has_flag(11)).count(), seg.len()),
    })
}

/// Rear slip angle (°) from which a lap counts as "the rear stepped out" in a corner
const RISK_SLIDE: f64 = 8.0;

/// Normal laps for the consistency: up to this × the best (spins, excursions and stops left out)
const NORMAL_LAP: f64 = 1.07;

/// Grid step of the traces (m along the reference lap)
const TRACE_STEP: f64 = 4.0;

/// Index and fraction where `rows` reach reference distance `s` (like [`time_at`]).
fn at(rows: &[Row], s: f64) -> (usize, f64) {
    let (mut lo, mut hi) = (0usize, rows.len() - 1);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if rows[mid].s_ref < s {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let i = lo.max(1);
    let (a, b) = (&rows[i - 1], &rows[i]);
    let ds = b.s_ref - a.s_ref;
    (i - 1, if ds > 0.0 { ((s - a.s_ref) / ds).clamp(0.0, 1.0) } else { 0.0 })
}

/// Channels of one lap on the distance grid of the reference lap.
fn trace(rows: &[Row], grid: &[f64], reference: &[Row]) -> serde_json::Value {
    let lerp = |(i, f): (usize, f64), v: &dyn Fn(&Row) -> f64| {
        let j = (i + 1).min(rows.len() - 1);
        v(&rows[i]) + (v(&rows[j]) - v(&rows[i])) * f
    };
    let r1 = |v: f64| (v * 10.0).round() / 10.0;
    let mut speed = Vec::with_capacity(grid.len());
    let (mut thr, mut brk, mut steer, mut gear, mut delta) = (vec![], vec![], vec![], vec![], vec![]);
    for &s in grid {
        let p = at(rows, s);
        speed.push(r1(lerp(p, &|r| r.speed_kmh)));
        thr.push(lerp(p, &|r| r.throttle_pct()).round());
        brk.push(lerp(p, &|r| r.brake_pct()).round());
        steer.push(r1(lerp(p, &|r| r.steering.to_degrees())));
        let g = rows[p.0].gear;
        gear.push(if (1.0..15.0).contains(&g) { g } else { 0.0 });
        delta.push(lap::round_to(time_at(rows, s) - time_at(reference, s), 3));
    }
    serde_json::json!({ "speed": speed, "throttle": thr, "brake": brk, "steer": steer, "gear": gear, "delta": delta })
}

impl Comparer {
    /// Speed, pedals, steering, gear and time delta of one lap against the fastest lap of the
    /// session, on a common distance grid – for the corner graphs on the dashboard.
    /// `lap_file` = None: the latest complete lap.
    pub fn traces(&mut self, session: &Path, lap_file: Option<&str>) -> std::io::Result<serde_json::Value> {
        let laps = self.complete_laps(session);
        let Some((ref_path, ref_ms)) = laps.iter().min_by_key(|(_, ms)| *ms).cloned() else {
            return Ok(serde_json::json!({ "laps": [] }));
        };
        let mut reference = lap::load(&ref_path)?;
        let (ratio, _) = lap::steer_ratio(&reference);
        prepare(&mut reference, ratio);
        project(&mut reference, None);
        let target = match lap_file {
            Some(f) => laps.iter().find(|(p, _)| file_name(p) == f).cloned(),
            None => laps.last().cloned(),
        }
        .unwrap_or((ref_path.clone(), ref_ms));
        let mut rows = if target.0 == ref_path { reference.clone() } else { lap::load(&target.0)? };
        if target.0 != ref_path {
            prepare(&mut rows, ratio);
            project(&mut rows, Some(&reference));
        }
        let length = reference.last().and_then(|r| r.s).unwrap_or(0.0);
        let grid: Vec<f64> = (0..).map(|k| k as f64 * TRACE_STEP).take_while(|s| *s <= length).collect();
        let corners_path = session.join("corners.json");
        let corners = if corners_path.exists() { map_corner_defs(&corners_path, &reference)? } else { Vec::new() };
        let corners: Vec<serde_json::Value> = corners
            .iter()
            .map(|c| {
                let (s0, s1) = (s_of(&reference, c.from), s_of(&reference, c.to));
                let dt = (time_at(&rows, s1) - time_at(&rows, s0)) - (time_at(&reference, s1) - time_at(&reference, s0));
                serde_json::json!({
                    "n": c.n, "from": lap::round_i(s0), "to": lap::round_i(s1), "apex": lap::round_i(c.s),
                    "dt": lap::round_to(dt, 3),
                    "ref": corner_stats(&reference, s0, s1, c.s), "lap": corner_stats(&rows, s0, s1, c.s),
                })
            })
            .collect();
        // Map of the reference lap (every 3rd grid point is plenty)
        let map: Vec<[f64; 2]> = grid
            .iter()
            .step_by(3)
            .map(|&s| {
                let (i, _) = at(&reference, s);
                [lap::round_to(reference[i].pos_x, 1), lap::round_to(reference[i].pos_z, 1)]
            })
            .collect();
        let lap_of = |p: &Path| meta_lap(p);
        // Risk per corner over all laps of the session, and every spin / slide
        let res = self.compare(session)?;
        let mut incidents = Vec::new();
        for l in &res.laps {
            for i in &l.incidents {
                incidents.push(serde_json::json!({ "file": l.file, "lap": l.lap, "ms": l.ms, "incident": i }));
            }
        }
        let risk: Vec<serde_json::Value> = res
            .corners
            .iter()
            .map(|c| {
                // Beyond what the best lap itself does there (karts slide on every lap)
                let base = res.reference_lap().and_then(|r| r.corner(c.n).stats.as_ref()).map_or(0.0, |s| s.rear_slide);
                let limit = RISK_SLIDE.max(base + 6.0);
                let slides = res.laps.iter().filter(|l| l.corner(c.n).stats.as_ref().is_some_and(|s| s.rear_slide >= limit)).count();
                let spins = res.laps.iter().flat_map(|l| &l.incidents).filter(|i| i.corner == Some(c.n) && i.kind == "spin").count();
                let moments = res.laps.iter().flat_map(|l| &l.incidents).filter(|i| i.corner == Some(c.n) && i.kind == "slide").count();
                let n = res.laps.len().max(1);
                let level = if spins > 0 || slides * 10 >= n * 4 {
                    2
                } else if moments > 0 || slides * 10 >= n * 2 {
                    1
                } else {
                    0
                };
                serde_json::json!({ "n": c.n, "laps": res.laps.len(), "slides": slides, "spins": spins, "moments": moments, "level": level })
            })
            .collect();
        // Consistency: spread of every corner over the normal laps (incidents left out)
        let best = res.laps.iter().map(|l| l.ms).min().unwrap_or(0) as f64;
        let normal: Vec<&LapResult> = res.laps.iter().filter(|l| (l.ms as f64) <= best * NORMAL_LAP).collect();
        let consistency: Vec<serde_json::Value> = res
            .corners
            .iter()
            .map(|c| {
                let dts: Vec<f64> = normal.iter().map(|l| l.corner(c.n).dt).collect();
                let by_lap: Vec<(i64, f64)> = normal.iter().map(|l| (l.lap, l.corner(c.n).dt)).collect();
                serde_json::json!({
                    "n": c.n,
                    "laps": dts.len(),
                    "mean": lap::round_to(lap::mean(dts.iter().copied()), 3),
                    "sd": lap::round_to(lap::pstdev(&dts), 3),
                    "best": lap::round_to(dts.iter().copied().fold(f64::INFINITY, f64::min), 3),
                    "worst": lap::round_to(dts.iter().copied().fold(f64::NEG_INFINITY, f64::max), 3),
                    "by_lap": by_lap,
                })
            })
            .collect();
        let times: Vec<f64> = normal.iter().map(|l| l.ms as f64).collect();
        // Setup of every lap (written by the recorder) → mean loss per corner and setup
        let meta: serde_json::Value =
            fs::read_to_string(session.join("session.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let setup_of = |file: &str| meta["lap_setups"][file].as_str().unwrap_or_default().to_string();
        let mut ids: Vec<String> = normal.iter().map(|l| setup_of(&l.file)).collect();
        ids.sort();
        ids.dedup();
        let by_setup: Vec<serde_json::Value> = if ids.len() < 2 {
            Vec::new()
        } else {
            ids.iter()
                .map(|id| {
                    let laps: Vec<&&LapResult> = normal.iter().filter(|l| setup_of(&l.file) == *id).collect();
                    let corners: serde_json::Map<String, serde_json::Value> = res
                        .corners
                        .iter()
                        .map(|c| (c.n.to_string(), lap::round_to(lap::mean(laps.iter().map(|l| l.corner(c.n).dt)), 3).into()))
                        .collect();
                    let ms: Vec<f64> = laps.iter().map(|l| l.ms as f64).collect();
                    serde_json::json!({ "id": id, "laps": laps.len(), "mean_ms": lap::round_i(lap::mean(ms)), "corners": corners })
                })
                .collect()
        };
        Ok(serde_json::json!({
            "consistency": consistency,
            "by_setup": by_setup,
            "normal": { "laps": times.len(), "mean_ms": lap::round_i(lap::mean(times.iter().copied())),
                        "sd_ms": lap::round_i(lap::pstdev(&times)), "theoretical_ms": res.theoretical_ms.map(lap::round_i) },
            "risk": risk,
            "incidents": incidents,
            "session": file_name(session),
            "laps": laps.iter().map(|(p, ms)| serde_json::json!({ "file": file_name(p), "lap": lap_of(p), "ms": ms })).collect::<Vec<_>>(),
            "reference": { "file": file_name(&ref_path), "lap": lap_of(&ref_path), "ms": ref_ms },
            "lap": { "file": file_name(&target.0), "lap": lap_of(&target.0), "ms": target.1 },
            "step": TRACE_STEP,
            "s": grid.iter().map(|s| lap::round_i(*s)).collect::<Vec<_>>(),
            "map": map,
            "corners": corners,
            "ref_trace": trace(&reference, &grid, &reference),
            "lap_trace": trace(&rows, &grid, &reference),
        }))
    }
}

/// Lap number from a file name like `011_lap08.csv`
fn meta_lap(p: &Path) -> i64 {
    let name = file_name(p);
    name.split("_lap").nth(1).and_then(|r| r.trim_end_matches(".csv").parse().ok()).unwrap_or(0)
}
