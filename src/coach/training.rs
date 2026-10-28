//! Training tasks: one corner, one change, checked on the next laps.
//!
//! The driver (or the coach's suggestion) picks a corner and one figure to work on – apex speed,
//! exit speed, braking point, brake release, full throttle, coasting. The task stores the figure of
//! the reference lap and the baseline: the median gap of the normal laps so far. The target is a
//! quarter less gap. Only laps driven after the task was set count (a lap with a spin or slide in
//! the corner does not count); after 3 laps the series is judged, after 5 at the latest. A gain
//! bought with a slower exit (more than 2 km/h) is not a success.
//!
//! Stored in `coach/training.json`: the active task and the finished ones (newest first).
//! Idea from ApexTrace 0.6 ("focused practice"), built on the coach's own corner figures.

use super::advice::{corner_tips, typical};
use super::compare::{Comparison, CornerStats, LapResult};
use crate::tr;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const METRICS: [&str; 6] = ["apex", "exit", "brake", "release", "throttle", "coast"];
/// Laps after which a series is judged, and the most it runs
pub const SERIES_MIN: usize = 3;
pub const SERIES_MAX: usize = 5;
/// A slower exit than this (km/h, median against the baseline) spoils a gain
const EXIT_DROP: f64 = 2.0;
/// Normal laps: within 107 % of the best
const NORMAL: f64 = 1.07;
/// Finished tasks kept
const DONE_MAX: usize = 30;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct TaskLap {
    pub session: String,
    pub file: String,
    pub lap: i64,
    pub run: usize,
    /// Gap to the reference figure; `None` = spin or slide in the corner, or no figure
    pub value: Option<f64>,
    /// Exit speed, km/h
    pub exit: Option<f64>,
    /// Time lost in the corner against the best lap of that session, s
    pub dt: f64,
    pub spin: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Task {
    pub id: String,
    /// When it was set (RFC 3339) and as Unix seconds (laps written after it count)
    pub created: String,
    pub created_ts: i64,
    pub track: String,
    pub car: i32,
    pub corner: usize,
    pub metric: String,
    /// The instruction (short) and what to try (longer)
    pub short: String,
    pub tip: String,
    /// What the recording showed when the task was set
    pub observed: String,
    pub ref_session: String,
    pub ref_lap: i64,
    pub ref_ms: i64,
    /// Figure of the reference lap (km/h or m along the lap)
    pub ref_value: f64,
    /// Median gap of the laps before (and how many), median exit speed and time lost
    pub baseline: f64,
    pub baseline_n: usize,
    pub baseline_exit: f64,
    pub baseline_dt: f64,
    /// Gap allowed for success (as a loss, ≥ 0)
    pub target_loss: f64,
    pub laps: Vec<TaskLap>,
    /// "active", "done", "done_warn", "missed", "cancelled"
    pub status: String,
    pub result: Option<String>,
    pub finished: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Saved {
    pub active: Option<Task>,
    pub done: Vec<Task>,
}

pub fn path(coach_dir: &Path) -> PathBuf {
    coach_dir.join("training.json")
}

pub fn load(file: &Path) -> Saved {
    fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(file: &Path, s: &Saved) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(s).unwrap_or_default())?;
    fs::rename(tmp, file)
}

/// Finishes the active task (status given) and moves it to the finished ones.
pub fn retire(s: &mut Saved, status: &str) {
    if let Some(mut t) = s.active.take() {
        if t.status == "active" {
            t.status = status.into();
            t.finished = Some(now_iso());
        }
        s.done.insert(0, t);
        s.done.truncate(DONE_MAX);
    }
}

fn now_iso() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

pub fn label(metric: &str) -> String {
    match metric {
        "apex" => tr!("Scheitelgeschwindigkeit", "Apex speed", "Rychlost na vrcholu"),
        "exit" => tr!("Ausgangsgeschwindigkeit", "Exit speed", "Rychlost na výjezdu"),
        "brake" => tr!("Bremspunkt", "Braking point", "Bod brzdění"),
        "release" => tr!("Bremse lösen", "Brake release", "Povolení brzdy"),
        "throttle" => tr!("Vollgas", "Full throttle", "Plný plyn"),
        _ => tr!("Rollen ohne Pedal", "Coasting", "Jízda bez pedálu"),
    }
}

pub fn unit(metric: &str) -> &'static str {
    if matches!(metric, "apex" | "exit") {
        "km/h"
    } else {
        "m"
    }
}

/// The figure of a corner (absolute: km/h, or m along the best lap).
fn value(metric: &str, st: &CornerStats) -> Option<f64> {
    match metric {
        "apex" => Some(st.v_min),
        "exit" => Some(st.v_exit),
        "brake" => st.brake_at,
        "release" => st.brake_off,
        "throttle" => st.full_at,
        "coast" => Some(st.coast_m),
        _ => None,
    }
}

/// How bad a gap to the reference is (≥ 0): slower apex/exit, any braking point difference,
/// later full throttle, more coasting.
pub fn loss(metric: &str, gap: f64) -> f64 {
    match metric {
        "apex" | "exit" => (-gap).max(0.0),
        "brake" | "release" => gap.abs(),
        _ => gap.max(0.0),
    }
}

/// Smallest gap worth a task.
fn min_loss(metric: &str) -> f64 {
    match metric {
        "apex" | "exit" => 1.5,
        "brake" | "release" => 5.0,
        "throttle" => 8.0,
        _ => 10.0,
    }
}

/// The coach's corner tips by what they are about.
fn tip_metric(key: &str) -> Option<&'static str> {
    Some(match key {
        "overbrake" | "early_brake" => "brake",
        "slow_apex" | "flat_lift" | "late_apex" | "understeer" | "gear_low" => "apex",
        "early_apex" | "exit_slide" | "early_throttle" | "slow_exit" | "gear_high" => "exit",
        "late_throttle" => "throttle",
        "coast" => "coast",
        _ => return None,
    })
}

fn num(v: f64) -> String {
    format!("{:.1}", v.abs()).replace('.', ",").replace(",0", "")
}

/// A signed figure for the dashboard: "−4,8 km/h", "+6 m" (metres whole).
pub fn signed(v: f64, unit: &str) -> String {
    let v = if unit == "m" { v.round() } else { v };
    let s = if v > 0.05 {
        "+"
    } else if v < -0.05 {
        "−"
    } else {
        "±"
    };
    format!("{s}{} {unit}", num(v))
}

/// Instruction for a metric when the coach has no tip of its own for it.
fn generic(metric: &str, gap: f64) -> (String, String) {
    let x = if unit(metric) == "m" { format!("{}", gap.abs().round()) } else { num(gap) };
    match metric {
        "apex" => (
            tr!("Scheitel {x} km/h schneller", "Apex {x} km/h faster", "Vrcholem o {x} km/h rychleji"),
            tr!(
                "Gleicher Bremspunkt, aber die Bremse früher und weicher lösen – lass das Auto in den Scheitel rollen.",
                "Same braking point, but release the brake earlier and more smoothly – let the car roll into the apex.",
                "Stejný brzdný bod, ale brzdu povol dřív a plynuleji – nech auto do vrcholu dojet."
            ),
        ),
        "exit" => (
            tr!("Ausgang {x} km/h schneller", "Exit {x} km/h faster", "Výjezd o {x} km/h rychleji"),
            tr!(
                "Gerader rausfahren: Gas ab dem Scheitel gleichmäßig, Vollgas mit geradem Lenkrad.",
                "Straighter exit: throttle smoothly from the apex, full with the wheel straight.",
                "Rovnější výjezd: plyn plynule od vrcholu, plný až s narovnaným volantem."
            ),
        ),
        "brake" if gap > 0.0 => (
            tr!("{x} m früher bremsen", "Brake {x} m earlier", "Brzdi o {x} m dřív"),
            tr!(
                "Du bremst später als die Referenz und verlierst es im Scheitel – früher und weicher bremsen.",
                "You brake later than the reference and lose it at the apex – brake earlier and more smoothly.",
                "Brzdíš později než reference a ztratíš to ve vrcholu – brzdi dřív a plynuleji."
            ),
        ),
        "brake" => (
            tr!("{x} m später bremsen", "Brake {x} m later", "Brzdi o {x} m později"),
            tr!(
                "Du bremst früher als nötig – nimm dir einen Bremspunkt ein Stück weiter vorne.",
                "You brake earlier than needed – pick a braking marker a bit further on.",
                "Brzdíš dřív, než je potřeba – vyber si brzdný bod o kousek dál."
            ),
        ),
        "release" if gap > 0.0 => (
            tr!("Bremse {x} m früher lösen", "Release the brake {x} m earlier", "Pusť brzdu o {x} m dřív"),
            tr!(
                "Die Bremse früher und gleichmäßig in die Kurve lösen.",
                "Release the brake earlier and evenly into the corner.",
                "Brzdu povoluj dřív a plynule do zatáčky."
            ),
        ),
        "release" => (
            tr!("Bremse {x} m länger halten", "Hold the brake {x} m longer", "Drž brzdu o {x} m déle"),
            tr!(
                "Länger in die Kurve bremsen (Trail Braking), damit die Front greift.",
                "Brake further into the corner (trail braking) so the front bites.",
                "Brzdi do zatáčky déle (trail braking), ať se předek zakousne."
            ),
        ),
        "throttle" => (
            tr!("Vollgas {x} m früher", "Full throttle {x} m earlier", "Plný plyn o {x} m dřív"),
            tr!(
                "Nach dem Scheitel entschlossener Gas geben, Vollgas sobald das Lenkrad gerade wird.",
                "After the apex add throttle more decisively, full as soon as the wheel straightens.",
                "Po vrcholu přidávej rozhodněji, plný plyn hned, jak se volant rovná."
            ),
        ),
        _ => (
            tr!("Von der Bremse direkt aufs Gas", "From the brake straight to the throttle", "Z brzdy rovnou na plyn"),
            tr!(
                "Du rollst {x} m mehr ohne Pedal – von der Bremse gleitend aufs Gas.",
                "You coast {x} m more without a pedal – blend from the brake onto the throttle.",
                "Jedeš o {x} m víc bez pedálu – z brzdy přejdi plynule rovnou na plyn."
            ),
        ),
    }
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

fn incident_in(l: &LapResult, n: usize) -> bool {
    l.incidents.iter().any(|i| i.corner == Some(n))
}

/// Normal laps of a session for corner `n` (not the reference lap, within 107 %, no spin or
/// slide in the corner).
fn normal_laps(res: &Comparison, n: usize) -> Vec<&LapResult> {
    let Some(r) = res.reference_lap() else { return vec![] };
    let limit = r.ms as f64 * NORMAL;
    res.laps.iter().filter(|l| l.file != r.file && (l.ms as f64) <= limit && !incident_in(l, n)).collect()
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CandMetric {
    pub metric: String,
    pub label: String,
    pub unit: &'static str,
    /// Median gap to the reference
    pub gap: f64,
    /// Laps with a gap worth working on
    pub k: usize,
    pub short: String,
    pub text: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Candidate {
    pub n: usize,
    /// Normal laps looked at, laps that lost time here, median time lost (s)
    pub laps: usize,
    pub lost: usize,
    pub dt: f64,
    pub metrics: Vec<CandMetric>,
}

/// Corners worth a task in a session: where time goes in most laps, with the figures that differ
/// from the reference (most often first).
pub fn candidates(res: &Comparison) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for c in &res.corners {
        let Some(rs) = c.reference.as_ref() else { continue };
        let laps = normal_laps(res, c.n);
        if laps.len() < SERIES_MIN {
            continue;
        }
        let dts: Vec<f64> = laps.iter().map(|l| l.corner(c.n).dt).collect();
        let lost = dts.iter().filter(|d| **d > 0.05).count();
        let dt = median(dts).unwrap_or(0.0);
        let stats: Vec<&CornerStats> = laps.iter().filter_map(|l| l.corner(c.n).stats.as_ref()).collect();
        let tips = typical(&stats).map(|t| corner_tips(&t, rs)).unwrap_or_default();
        let mut metrics: Vec<CandMetric> = METRICS
            .iter()
            .filter_map(|m| {
                let r = value(m, rs)?;
                let gaps: Vec<f64> = stats.iter().filter_map(|st| value(m, st).map(|v| v - r)).collect();
                if gaps.len() < SERIES_MIN {
                    return None;
                }
                // Points along the lap further apart than this are another line, not a gap
                if matches!(*m, "brake" | "release" | "throttle") && gaps.iter().any(|g| g.abs() > 80.0) {
                    return None;
                }
                let gap = median(gaps.clone())?;
                if loss(m, gap) < min_loss(m) {
                    return None;
                }
                let k = gaps.iter().filter(|g| loss(m, **g) >= min_loss(m)).count();
                let (short, text) = match tips.iter().find(|t| tip_metric(t.key) == Some(*m)) {
                    Some(t) => (t.short.clone(), t.text.clone()),
                    None => generic(m, gap),
                };
                Some(CandMetric { metric: m.to_string(), label: label(m), unit: unit(m), gap: (gap * 10.0).round() / 10.0, k, short, text })
            })
            .collect();
        metrics.sort_by(|a, b| {
            b.k.cmp(&a.k).then((loss(&b.metric, b.gap) / min_loss(&b.metric)).total_cmp(&(loss(&a.metric, a.gap) / min_loss(&a.metric))))
        });
        if metrics.is_empty() || dt <= 0.02 {
            continue;
        }
        out.push(Candidate { n: c.n, laps: laps.len(), lost, dt: (dt * 1000.0).round() / 1000.0, metrics });
    }
    out.sort_by(|a, b| (b.lost as f64 / b.laps as f64 * b.dt).total_cmp(&(a.lost as f64 / a.laps as f64 * a.dt)));
    out
}

/// A new task for corner `n` and `metric` from the laps of a session.
pub fn start(res: &Comparison, session: &str, track: &str, car: i32, n: usize, metric: &str) -> Result<Task, String> {
    if !METRICS.contains(&metric) {
        return Err("unknown metric".into());
    }
    let c = res.corners.iter().find(|c| c.n == n).ok_or_else(|| tr!("Unbekannte Kurve.", "Unknown corner.", "Neznámá zatáčka."))?;
    let rs = c.reference.as_ref().ok_or_else(|| tr!("Keine Referenz.", "No reference.", "Chybí referenční kolo."))?;
    let reference = res.reference_lap().ok_or_else(|| tr!("Keine Referenz.", "No reference.", "Chybí referenční kolo."))?;
    let ref_value = value(metric, rs).ok_or_else(|| {
        tr!("Die Referenz hat diesen Wert hier nicht.", "The reference has no such figure here.", "Referenční kolo tu tenhle údaj nemá.")
    })?;
    let laps = normal_laps(res, n);
    let rows: Vec<(f64, f64, f64)> = laps
        .iter()
        .filter_map(|l| {
            let lc = l.corner(n);
            let st = lc.stats.as_ref()?;
            Some((value(metric, st)? - ref_value, st.v_exit, lc.dt))
        })
        .collect();
    if rows.len() < SERIES_MIN {
        return Err(tr!(
            "Für diese Kurve fehlen noch mindestens 3 normale Runden.",
            "This corner needs at least 3 normal laps first.",
            "Na tuhle zatáčku zatím nemám aspoň 3 normální kola."
        ));
    }
    let baseline = median(rows.iter().map(|r| r.0).collect()).unwrap_or(0.0);
    let bl = loss(metric, baseline);
    if bl < min_loss(metric) * 0.5 {
        return Err(tr!(
            "Hier fährst du schon wie die Referenz – nimm einen anderen Wert oder eine andere Kurve.",
            "You already match the reference here – pick another figure or corner.",
            "Tady už jedeš jako referenční kolo – vyber jiný údaj nebo jinou zatáčku."
        ));
    }
    let cand = candidates(res).into_iter().find(|x| x.n == n).and_then(|x| x.metrics.into_iter().find(|m| m.metric == metric));
    let (short, tip) = cand.as_ref().map_or_else(|| generic(metric, baseline), |m| (m.short.clone(), m.text.clone()));
    let k = rows.iter().filter(|r| loss(metric, r.0) >= min_loss(metric)).count();
    let (lab, gap, total, rl) = (label(metric), signed(baseline, unit(metric)), rows.len(), reference.lap);
    let observed = tr!(
        "{lab}: {gap} gegenüber der Referenz (R{rl}) – in {k} von {total} Runden.",
        "{lab}: {gap} against the reference (L{rl}) – in {k} of {total} laps.",
        "{lab}: {gap} proti referenci (K{rl}) – v {k} z {total} kol."
    );
    let now = chrono::Local::now();
    Ok(Task {
        id: format!("{}-T{n}-{metric}", now.format("%Y%m%d%H%M%S")),
        created: now_iso(),
        created_ts: now.timestamp(),
        track: track.to_string(),
        car,
        corner: n,
        metric: metric.to_string(),
        short,
        tip,
        observed,
        ref_session: session.to_string(),
        ref_lap: reference.lap,
        ref_ms: reference.ms,
        ref_value,
        baseline,
        baseline_n: rows.len(),
        baseline_exit: median(rows.iter().map(|r| r.1).collect()).unwrap_or(0.0),
        baseline_dt: median(rows.iter().map(|r| r.2).collect()).unwrap_or(0.0),
        target_loss: bl * 0.75,
        laps: Vec::new(),
        status: "active".into(),
        result: None,
        finished: None,
    })
}

fn file_ts(session_dir: &Path, file: &str) -> i64 {
    let p = session_dir.join(file);
    let md = fs::metadata(&p).or_else(|_| fs::metadata(session_dir.join(format!("{file}.gz"))));
    md.and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64)
}

impl Task {
    fn valid(&self) -> Vec<&TaskLap> {
        self.laps.iter().filter(|l| l.value.is_some()).collect()
    }

    pub fn now(&self) -> Option<f64> {
        median(self.valid().iter().filter_map(|l| l.value).collect())
    }

    fn exit_change(&self) -> Option<f64> {
        median(self.valid().iter().filter_map(|l| l.exit).collect()).map(|e| e - self.baseline_exit)
    }

    pub fn ok(&self, v: f64) -> bool {
        loss(&self.metric, v) <= self.target_loss + 1e-9
    }

    /// The target as a bound on the gap: "≥ −3,6 km/h", "±4 m", "≤ +6 m".
    pub fn target_text(&self) -> String {
        let u = unit(&self.metric);
        let t = if u == "m" { format!("{}", self.target_loss.round().max(1.0)) } else { num(self.target_loss) };
        match self.metric.as_str() {
            "apex" | "exit" => format!("≥ −{t} {u}"),
            "brake" | "release" => format!("± {t} {u}"),
            _ => format!("≤ +{t} {u}"),
        }
    }

    /// Adds the laps of `res` driven after the task was set and judges the series. Returns a radio
    /// message when the series ended.
    pub fn update(&mut self, res: &Comparison, session: &str, session_dir: &Path) -> Option<Value> {
        if self.status != "active" {
            return None;
        }
        // One lap at a time, so a catch-up over several laps judges the series where it ended
        let limit = res.reference_lap().map_or(f64::MAX, |r| r.ms as f64 * NORMAL);
        let mut msg = None;
        for l in &res.laps {
            if self.status != "active" {
                break;
            }
            if self.laps.iter().any(|x| x.session == session && x.file == l.file) || file_ts(session_dir, &l.file) < self.created_ts {
                continue;
            }
            let lc = l.corner(self.corner);
            let spin = incident_in(l, self.corner);
            let st = lc.stats.as_ref();
            // A lap off the pace (spin or excursion elsewhere) or a figure far off says nothing
            let sane = |v: f64| if unit(&self.metric) == "km/h" { v.abs() <= 40.0 } else { v.abs() <= 80.0 };
            let value = if spin || l.ms as f64 > limit {
                None
            } else {
                st.and_then(|st| value(&self.metric, st)).map(|v| v - self.ref_value).filter(|v| sane(*v))
            };
            self.laps.push(TaskLap {
                session: session.to_string(),
                file: l.file.clone(),
                lap: l.lap,
                run: l.run,
                value,
                exit: st.map(|s| s.v_exit),
                dt: lc.dt,
                spin,
            });
            if value.is_some() {
                msg = self.judge();
            }
        }
        msg
    }

    /// The series after a new lap: finished (success, success with a slower exit, missed) or not.
    fn judge(&mut self) -> Option<Value> {
        let valid = self.valid().len();
        if valid < SERIES_MIN {
            return None;
        }
        let now = self.now()?;
        let exit = self.exit_change().unwrap_or(0.0);
        let ok = self.ok(now);
        let (n, u) = (self.corner, unit(&self.metric));
        let (gain, before) = (signed(now, u), signed(self.baseline, u));
        let status = if ok && exit >= -EXIT_DROP {
            "done"
        } else if valid >= SERIES_MAX {
            if ok {
                "done_warn"
            } else {
                "missed"
            }
        } else {
            return None;
        };
        let ex = signed(exit, "km/h");
        let text = match status {
            "done" => tr!(
                "K{n} geschafft: {gain} (vorher {before}), Ausgang {ex}.",
                "T{n} done: {gain} (before {before}), exit {ex}.",
                "T{n} splněno: {gain} (předtím {before}), výjezd {ex}."
            ),
            "done_warn" => tr!(
                "K{n}: Ziel erreicht ({gain}), aber der Ausgang ist {ex} langsamer.",
                "T{n}: target reached ({gain}), but the exit is {ex} slower.",
                "T{n}: cíl máš ({gain}), ale výjezd je o {ex} pomalejší – drž výjezd."
            ),
            _ => {
                let target = self.target_text();
                tr!(
                    "K{n}: noch nicht – {gain} (Ziel {target}). Noch eine Serie?",
                    "T{n}: not yet – {gain} (target {target}). Another series?",
                    "T{n}: zatím ne – {gain} (cíl {target}). Dáš ještě jednu sérii?"
                )
            }
        };
        self.status = status.into();
        self.result = Some(text.clone());
        self.finished = Some(now_iso());
        Some(json!({ "level": if status == "done" { "info" } else { "warn" }, "corner": n, "text": text }))
    }

    /// For the dashboard: the figures, the series and how it is going.
    pub fn view(&self) -> Value {
        let u = unit(&self.metric);
        let valid = self.valid();
        let now = self.now();
        let exit = self.exit_change();
        let (n, need) = (valid.len(), SERIES_MIN);
        let verdict = match (self.status.as_str(), now) {
            ("done", _) => json!({ "tone": "good", "title": self.result }),
            ("done_warn", _) => json!({ "tone": "warn", "title": self.result }),
            ("missed", _) => json!({ "tone": "bad", "title": self.result }),
            ("cancelled", _) => json!({ "tone": "info", "title": tr!("Abgebrochen", "Cancelled", "Zrušeno") }),
            (_, None) => json!({ "tone": "info", "title": tr!(
                "Fahre 3–5 Runden – ich werte jede aus.",
                "Drive 3–5 laps – I check each one.",
                "Odjeď 3–5 kol – vyhodnotím každé."
            ) }),
            (_, Some(v)) => {
                let (g, d) = (signed(v - self.baseline, u), signed(exit.unwrap_or(0.0), "km/h"));
                let better = loss(&self.metric, v) < loss(&self.metric, self.baseline);
                let left = SERIES_MAX.saturating_sub(n);
                if n < need {
                    let more = need - n;
                    json!({ "tone": if better { "good" } else { "info" },
                        "title": tr!("Bisher {g} gegenüber vorher", "So far {g} against before", "Zatím {g} proti předtím"),
                        "text": tr!("Ausgang {d}. Noch {more} Runde(n), dann werte ich die Serie.", "Exit {d}. {more} more lap(s), then I judge the series.", "Výjezd {d}. Ještě {more} kola a vyhodnotím sérii.") })
                } else if self.ok(v) {
                    json!({ "tone": "warn",
                        "title": tr!("Ziel erreicht, aber der Ausgang leidet", "Target reached, but the exit suffers", "Cíl máš, ale výjezd je horší"),
                        "text": tr!("Ausgang {d} – halte den Ausgang. Noch {left} Runde(n).", "Exit {d} – keep the exit. {left} more lap(s).", "Výjezd {d} – drž výjezd. Ještě {left} kola.") })
                } else {
                    json!({ "tone": if better { "info" } else { "bad" },
                        "title": tr!("Bisher {g} gegenüber vorher – noch nicht am Ziel", "So far {g} against before – not at the target yet", "Zatím {g} proti předtím – cíl ještě ne"),
                        "text": tr!("Noch {left} Runde(n) in dieser Serie.", "{left} more lap(s) in this series.", "V sérii zbývají ještě {left} kola.") })
                }
            }
        };
        json!({
            "id": self.id, "status": self.status, "n": self.corner, "metric": self.metric, "label": label(&self.metric), "unit": u,
            "short": self.short, "tip": self.tip, "observed": self.observed, "track": self.track, "car": self.car,
            "created": self.created, "finished": self.finished, "result": self.result,
            "ref": { "session": self.ref_session, "lap": self.ref_lap, "ms": self.ref_ms, "value": self.ref_value },
            "before": self.baseline, "before_n": self.baseline_n, "before_text": signed(self.baseline, u),
            "target_loss": self.target_loss, "target_text": self.target_text(),
            "now": now, "now_text": now.map(|v| signed(v, u)), "now_ok": now.map(|v| self.ok(v)),
            "exit_change": exit, "valid": n, "need": need, "max": SERIES_MAX,
            "laps": self.laps.iter().map(|l| json!({ "lap": l.lap, "run": l.run, "value": l.value, "ok": l.value.map(|v| self.ok(v)), "spin": l.spin, "dt": l.dt })).collect::<Vec<_>>(),
            "verdict": verdict,
        })
    }
}

/// Coach worker: after a session's laps were analysed, the active task gets its new laps when
/// the session is on its track with its car. Returns the radio message when the series ended.
pub fn on_session(file: &Path, res: &Comparison, session_dir: &Path, car: i32) -> Option<Value> {
    let mut s = load(file);
    let t = s.active.as_mut()?;
    let key = crate::strategy::practice::track_key;
    if t.car != car || res.track.as_deref().map(key) != Some(key(&t.track)) {
        return None;
    }
    let session = session_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let before = t.laps.len();
    let radio = t.update(res, &session, session_dir);
    let finished = t.status != "active";
    if t.laps.len() == before && !finished {
        return None;
    }
    if finished {
        retire(&mut s, "done");
    }
    let _ = save(file, &s);
    radio
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coach::compare::{CornerInfo, LapCorner, Reference};

    fn st(v_min: f64, v_exit: f64) -> CornerStats {
        CornerStats {
            brake_at: Some(400.0),
            brake_max: 100.0,
            v_entry: 150.0,
            v_min,
            v_exit,
            full_at: Some(480.0),
            gear_min: Some(3),
            under: 0.0,
            over: 0.0,
            rear_slide: 2.0,
            glat: 2.0,
            tcs: 0.0,
            apex_at: Some(450.0),
            brake_kmh: Some(150.0),
            brake_off: Some(440.0),
            pickup_at: Some(460.0),
            coast_m: 5.0,
            gear_entry: Some(4),
        }
    }

    fn lap(file: &str, lap: i64, ms: i64, v_min: f64, v_exit: f64, dt: f64) -> LapResult {
        LapResult {
            file: file.into(),
            lap,
            ms,
            delta_ms: 0,
            corners: vec![LapCorner { n: 9, dt, stats: Some(st(v_min, v_exit)) }],
            incidents: vec![],
            shifts: vec![],
            run: 1,
        }
    }

    fn session(laps: Vec<LapResult>) -> Comparison {
        Comparison {
            reference: Some(Reference { file: "014_lap14.csv".into(), ms: 53_501, lap: 14 }),
            corners: vec![CornerInfo {
                n: 9,
                dir: "rechts".into(),
                x: 0.0,
                z: 0.0,
                s: 1344,
                reference: Some(st(80.0, 120.0)),
                brake_pos: None,
            }],
            laps,
            track: Some("Willow Springs International Raceway".into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_series_on_the_apex_speed() {
        let mut laps = vec![lap("014_lap14.csv", 14, 53_501, 80.0, 120.0, 0.0)];
        for (i, v) in [75.0, 75.5, 74.8, 76.0, 75.2].iter().enumerate() {
            laps.push(lap(&format!("0{}_lap0{i}.csv", 20 + i), 20 + i as i64, 54_000, *v, 120.0, 0.12));
        }
        let res = session(laps);
        let c = candidates(&res);
        assert_eq!(c[0].n, 9);
        assert_eq!(c[0].metrics[0].metric, "apex");
        let mut t = start(&res, "s1", "Willow Springs International Raceway", 2060, 9, "apex").unwrap();
        assert!((t.baseline + 4.8).abs() < 1e-9, "{}", t.baseline);
        assert!((t.target_loss - 3.6).abs() < 1e-9);
        assert_eq!(t.target_text(), "≥ −3,6 km/h");
        // New laps (the files do not exist, so their time is 0: count them by moving the start back)
        t.created_ts = -1;
        let new = session(vec![lap("030_lap30.csv", 30, 53_900, 77.0, 120.5, 0.05), lap("031_lap31.csv", 31, 53_850, 76.9, 120.0, 0.04)]);
        assert!(t.update(&new, "s2", Path::new("/nonexistent")).is_none());
        assert_eq!(t.view()["verdict"]["tone"], "good");
        let more = session(vec![lap("032_lap32.csv", 32, 53_800, 76.6, 119.5, 0.03)]);
        let radio = t.update(&more, "s2", Path::new("/nonexistent")).unwrap();
        assert_eq!(t.status, "done");
        assert!(radio["text"].as_str().unwrap().contains("9"), "{radio}");
    }

    #[test]
    fn a_faster_apex_bought_with_a_slower_exit_is_no_success() {
        let mut laps = vec![lap("014_lap14.csv", 14, 53_501, 80.0, 120.0, 0.0)];
        for i in 0..4 {
            laps.push(lap(&format!("02{i}_lap2{i}.csv"), 20 + i, 54_000, 75.0, 120.0, 0.12));
        }
        let mut t = start(&session(laps), "s1", "x", 1, 9, "apex").unwrap();
        t.created_ts = -1;
        let new: Vec<LapResult> = (0..5).map(|i| lap(&format!("04{i}_lap4{i}.csv"), 40 + i, 54_000, 79.0, 115.0, 0.05)).collect();
        let mut s = new.clone();
        s.truncate(3);
        assert!(t.update(&session(s), "s2", Path::new("/x")).is_none(), "3 laps: exit worse, keep going");
        assert_eq!(t.view()["verdict"]["tone"], "warn");
        t.update(&session(new), "s2", Path::new("/x")).unwrap();
        assert_eq!(t.status, "done_warn");
    }
}
