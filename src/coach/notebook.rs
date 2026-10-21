//! Track notebook: everything the recordings know about a track, gathered over all sessions –
//! records per car, the ideal lap, where the time is (typical losses with what to do), the risky
//! corners, where the tyres suffer, long runs, starts and the setups driven there. The briefing
//! before a race is built from it.

use super::compare::{Comparer, Comparison};
use super::library::{self, SessionInfo};
use serde_json::{json, Value};
use std::path::Path;

/// Sessions of the track looked at for long runs and starts (newest first)
const RECENT: usize = 6;

/// Corners with the largest average loss over the normal laps, with what to do there.
pub fn reserves(res: &Comparison, count: usize) -> Vec<Value> {
    let Some(reference) = res.reference_lap() else { return Vec::new() };
    let limit = reference.ms as f64 * 1.07;
    let normal: Vec<_> = res.laps.iter().filter(|l| l.file != reference.file && (l.ms as f64) <= limit).collect();
    if normal.is_empty() {
        return Vec::new();
    }
    let mut by: Vec<(usize, f64)> = res
        .corners
        .iter()
        .map(|c| (c.n, normal.iter().map(|l| l.corner(c.n).dt).sum::<f64>() / normal.len() as f64))
        .filter(|(_, d)| *d > 0.05)
        .collect();
    by.sort_by(|a, b| b.1.total_cmp(&a.1));
    by.into_iter()
        .take(count)
        .map(|(n, d)| {
            let tip = super::feedback::typical_tips(res, n, limit);
            json!({ "n": n, "loss": super::lap::round_to(d, 2), "tip": tip })
        })
        .collect()
}

/// Corners where the tyres suffer most (typical lap): the rear sliding, the front pushing.
pub fn tyre_corners(res: &Comparison, kart: bool) -> Vec<Value> {
    if kart {
        return Vec::new();
    }
    let Some(reference) = res.reference_lap() else { return Vec::new() };
    let limit = reference.ms as f64 * 1.07;
    let mut out: Vec<(f64, Value)> = Vec::new();
    for c in &res.corners {
        let stats: Vec<_> = res.laps.iter().filter(|l| (l.ms as f64) <= limit).filter_map(|l| l.corner(c.n).stats.as_ref()).collect();
        let Some(t) = super::advice::typical(&stats) else { continue };
        if t.rear_slide > 6.0 {
            out.push((
                t.rear_slide,
                json!({ "n": c.n, "kind": "rear",
                    "text": format!("Z{}: záď tu klouže ~{}° – na výjezdu přidávej plyn postupně, šetříš zadní gumy.", c.n, t.rear_slide.round() as i64) }),
            ));
        } else if t.under > 30.0 {
            out.push((
                t.under / 5.0,
                json!({ "n": c.n, "kind": "front",
                    "text": format!("Z{}: předek nedotáčí (~{} % zatáčky) – vjeď o kousek pomaleji, šetříš přední gumy.", c.n, t.under.round() as i64) }),
            ));
        }
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.into_iter().take(3).map(|o| o.1).collect()
}

/// The notebook of a track. `car`: the car to look at closely (records are listed for all).
/// `setup_name(car, id)`: name of a setup card.
pub fn build(
    recordings: &Path,
    all: &[SessionInfo],
    track: &str,
    car: Option<i32>,
    comparer: &mut Comparer,
    setup_name: &dyn Fn(i32, &str) -> Option<String>,
) -> Value {
    let sessions = library::of_track(all, track, None);
    // Records per car
    let mut cars: Vec<i32> = sessions.iter().map(|s| s.car_code).collect();
    cars.sort();
    cars.dedup();
    let mut records: Vec<Value> = cars
        .iter()
        .map(|c| {
            let mine: Vec<&SessionInfo> = sessions.iter().filter(|s| s.car_code == *c).copied().collect();
            let best = library::best(&mine);
            json!({
                "car": c,
                "name": best.map(|b| b.car.clone()).unwrap_or_default(),
                "sessions": mine.len(),
                "laps": mine.iter().map(|s| s.laps).sum::<usize>(),
                "best_ms": best.and_then(|b| b.best_ms),
                "best_session": best.map(|b| b.session.clone()),
                "best_started": best.map(|b| b.started.clone()),
                "last": mine.first().map(|s| s.started.clone()),
            })
        })
        .collect();
    records.sort_by_key(|r| r["best_ms"].as_i64().unwrap_or(i64::MAX));
    // The car to look at: given, else the one with the most laps
    let car = car
        .filter(|c| cars.contains(c))
        .or_else(|| records.iter().max_by_key(|r| r["laps"].as_u64().unwrap_or(0)).and_then(|r| r["car"].as_i64()).map(|c| c as i32));
    let mine: Vec<&SessionInfo> = sessions.iter().filter(|s| Some(s.car_code) == car).copied().collect();
    let mut out = json!({ "track": track, "car": car, "records": records, "sessions": sessions.len() });
    let Some(best) = library::best(&mine) else { return out };
    let dir = recordings.join(&best.session);
    let Ok(res) = crate::storage::comparison(&dir, comparer) else { return out };
    let kart = best.best_file.as_ref().and_then(|f| super::lap::load(&dir.join(f)).ok()).is_some_and(|rows| super::lap::is_kart(&rows));
    out["kart"] = json!(kart);
    out["best"] = json!({ "session": best.session, "ms": best.best_ms, "started": best.started });
    out["ideal_ms"] = json!(res.theoretical_ms.map(super::lap::round_i));
    // Typical lap (median of the normal laps) of the best session: race pace
    let limit = best.best_ms.unwrap_or(0) as f64 * 1.07;
    let mut normal: Vec<i64> = res.laps.iter().filter(|l| (l.ms as f64) <= limit).map(|l| l.ms).collect();
    normal.sort();
    out["typical_ms"] = json!(normal.get(normal.len() / 2));
    out["normal_laps"] = json!(normal.len());
    out["reserves"] = json!(reserves(&res, 3));
    out["tyres"] = json!(tyre_corners(&res, kart));
    let mut risk: Vec<Value> = super::compare::risk(&res).into_iter().filter(|r| r["level"].as_i64().unwrap_or(0) > 0).collect();
    risk.sort_by_key(|r| {
        -(r["level"].as_i64().unwrap_or(0) * 1000 + r["spins"].as_i64().unwrap_or(0) * 10 + r["slides"].as_i64().unwrap_or(0))
    });
    risk.truncate(3);
    // What went wrong there (latest incident of the corner)
    for r in risk.iter_mut() {
        let n = r["n"].as_u64().unwrap_or(0) as usize;
        if let Some(i) = res.laps.iter().rev().flat_map(|l| &l.incidents).find(|i| i.corner == Some(n)) {
            r["cause"] = json!(i.cause);
            r["tip"] = json!(i.tip);
        }
    }
    out["risk"] = json!(risk);
    // Long runs and starts over the recent sessions of the car
    let mut runs: Vec<Value> = Vec::new();
    let mut starts: Vec<Value> = Vec::new();
    for s in mine.iter().take(RECENT) {
        let d = recordings.join(&s.session);
        if let Ok(r) = crate::storage::comparison(&d, comparer) {
            for run in super::stint::analyse(&r, !kart) {
                runs.push(json!({ "session": s.session, "started": s.started, "laps": run.laps, "per_lap": run.per_lap,
                                  "cliff": run.cliff, "corners": run.corners, "text": run.text }));
            }
        }
        match crate::storage::read_summary(&d).filter(|x| x.pruned) {
            Some(x) => starts.extend(x.starts),
            None => starts.extend(super::start::of_session(&d).into_iter().map(|st| json!(st))),
        }
    }
    runs.sort_by_key(|r| std::cmp::Reverse(r["laps"].as_u64().unwrap_or(0)));
    runs.truncate(3);
    out["runs"] = json!(runs);
    out["starts"] = json!(starts);
    // Setups driven here with the car: laps and best lap per setup card
    let mut setups: std::collections::BTreeMap<String, (usize, Option<i64>)> = Default::default();
    for s in &mine {
        let d = recordings.join(&s.session);
        let meta: Value =
            std::fs::read_to_string(d.join("session.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let Ok(r) = crate::storage::comparison(&d, comparer) else { continue };
        for l in &r.laps {
            let id = meta["lap_setups"][&l.file].as_str().unwrap_or("").to_string();
            let e = setups.entry(id).or_default();
            e.0 += 1;
            e.1 = Some(e.1.map_or(l.ms, |b| b.min(l.ms)));
        }
    }
    out["experiment"] = experiment(recordings, &mine, comparer, &|id| setup_name(car.unwrap_or(0), id));
    let c = car.unwrap_or(0);
    out["setups"] = json!(setups
        .iter()
        .map(|(id, (laps, best))| json!({ "id": id, "name": if id.is_empty() { None } else { setup_name(c, id) }, "laps": laps, "best_ms": best }))
        .collect::<Vec<_>>());
    out
}

/// Setup experiment: the newest session where two setups were driven – per corner, how much the
/// newer setup gains or loses on the older one (means over the normal laps of each).
fn experiment(recordings: &Path, sessions: &[&SessionInfo], comparer: &mut Comparer, name: &dyn Fn(&str) -> Option<String>) -> Value {
    for s in sessions {
        let d = recordings.join(&s.session);
        let meta: Value =
            std::fs::read_to_string(d.join("session.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let Ok(res) = crate::storage::comparison(&d, comparer) else { continue };
        let Some(reference) = res.reference_lap() else { continue };
        let limit = reference.ms as f64 * 1.07;
        let setup_of = |file: &str| meta["lap_setups"][file].as_str().unwrap_or("").to_string();
        let normal: Vec<_> = res.laps.iter().filter(|l| (l.ms as f64) <= limit).collect();
        // Setups in the order they were first driven
        let mut ids: Vec<String> = Vec::new();
        for l in &normal {
            let id = setup_of(&l.file);
            if !id.is_empty() && !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.len() < 2 {
            continue;
        }
        let (a, b) = (&ids[ids.len() - 2], &ids[ids.len() - 1]);
        let laps_of = |id: &str| normal.iter().filter(|l| setup_of(&l.file) == id).copied().collect::<Vec<_>>();
        let (la, lb) = (laps_of(a), laps_of(b));
        if la.len() < 2 || lb.len() < 2 {
            continue;
        }
        let mean = |v: &[&super::compare::LapResult], f: &dyn Fn(&super::compare::LapResult) -> f64| {
            v.iter().map(|l| f(l)).sum::<f64>() / v.len() as f64
        };
        let mut corners: Vec<(usize, f64)> =
            res.corners.iter().map(|c| (c.n, mean(&lb, &|l| l.corner(c.n).dt) - mean(&la, &|l| l.corner(c.n).dt))).collect();
        corners.retain(|(_, d)| d.abs() >= 0.03);
        corners.sort_by(|x, y| x.1.total_cmp(&y.1));
        let lap = (mean(&lb, &|l| l.ms as f64) - mean(&la, &|l| l.ms as f64)) / 1000.0;
        return json!({
            "session": s.session, "started": s.started,
            "from": { "id": a, "name": name(a), "laps": la.len() },
            "to": { "id": b, "name": name(b), "laps": lb.len() },
            "lap": super::lap::round_to(lap, 3),
            "corners": corners.iter().map(|(n, d)| json!({ "n": n, "d": super::lap::round_to(*d, 3) })).collect::<Vec<_>>(),
        });
    }
    Value::Null
}
