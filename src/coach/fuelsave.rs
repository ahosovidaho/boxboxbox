//! Where to save fuel: lift and coast before the braking points, priced in time.
//!
//! Fuel flow is fitted from the recorded laps (fuel used ≈ k · throttle · rpm per second, R² ~0.9
//! on the F3500), the coasting deceleration from the samples without a pedal (drag and engine
//! braking: a + b·v²). Lifting `L` metres before a braking point at speed `v` saves the fuel the
//! engine would have burnt over those metres; the time lost grows with the square of `L` and is
//! small at high speed: the speed falls by (a_throttle + a_coast)·L/v, so the loss is about
//! (a_throttle + a_coast)·L² / (2·v³). The braking that follows starts slower and can start a bit
//! later, which gets some of it back – so the figure is an upper bound.
//!
//! The fuel to save per lap is shared out so that the total time lost is the smallest: every
//! zone is used up to the point where a further litre costs the same time everywhere.

use super::lap::{self, Row, HZ};
use serde::Serialize;

/// Full throttle (raw 0–255)
const FULL: f64 = 0.95 * 255.0;
/// Shorter lifts are not worth calling (m)
const MIN_LIFT: f64 = 20.0;
/// Longest lift before a braking point (m)
const MAX_LIFT: f64 = 200.0;

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Model {
    /// Fuel per second per (throttle share × rpm)
    pub k: f64,
    /// Coasting deceleration (m/s²) = c0 + c2·v² (v in m/s)
    pub coast: [f64; 2],
    /// Fuel per lap the model gives for the reference lap, and the one measured there
    pub lap_model: f64,
    pub lap_measured: f64,
}

/// A braking point preceded by full throttle.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Zone {
    /// The corner the braking is for
    pub corner: Option<usize>,
    /// Braking point along the lap (m) and speed there (km/h)
    pub brake_at: f64,
    pub kmh: f64,
    /// Full throttle before it (m), capped
    pub max_m: f64,
    /// Fuel per metre lifted and time per metre² lifted
    pub per_m: f64,
    pub cost: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Lift {
    pub corner: Option<usize>,
    /// Lift this far before the braking point (m)
    pub lift_m: f64,
    pub kmh: f64,
    pub saves: f64,
    pub cost_s: f64,
    /// Where to lift (x, z)
    pub x: f64,
    pub z: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Plan {
    pub lifts: Vec<Lift>,
    pub saves: f64,
    pub cost_s: f64,
    /// Lifting alone does not save that much
    pub short: bool,
}

/// Least squares for y = a·x (through the origin).
fn slope(pairs: &[(f64, f64)]) -> Option<f64> {
    let xx: f64 = pairs.iter().map(|p| p.0 * p.0).sum();
    (xx > 0.0).then(|| pairs.iter().map(|p| p.0 * p.1).sum::<f64>() / xx)
}

/// Fits the fuel flow and the coasting deceleration from laps (raw rows, any order).
pub fn fit(laps: &[Vec<Row>], reference: &[Row]) -> Option<Model> {
    let w = HZ as usize;
    let mut flow: Vec<(f64, f64)> = Vec::new();
    let mut coast: Vec<(f64, f64)> = Vec::new();
    for rows in laps {
        for c in rows.chunks(w).filter(|c| c.len() == w) {
            let used = c[0].fuel - c[w - 1].fuel;
            // Refuelling, a reset or a fuel-free event
            if !(0.0..1.0).contains(&used) {
                continue;
            }
            let x: f64 = c.iter().map(|r| r.throttle / 255.0 * r.rpm).sum::<f64>() / HZ;
            flow.push((x, used));
        }
        for r in rows.iter().filter(|r| r.throttle < 8.0 && r.brake < 8.0 && r.speed_kmh > 70.0) {
            coast.push((r.speed_kmh / 3.6, -r.surge));
        }
    }
    let k = slope(&flow)?;
    if k <= 0.0 || flow.len() < 60 || coast.len() < 60 {
        return None;
    }
    // Coasting: c0 + c2·v² by least squares (2×2 normal equations)
    let n = coast.len() as f64;
    let (sx, sy) = (coast.iter().map(|p| p.0 * p.0).sum::<f64>(), coast.iter().map(|p| p.1).sum::<f64>());
    let (sxx, sxy) = (coast.iter().map(|p| p.0.powi(4)).sum::<f64>(), coast.iter().map(|p| p.0 * p.0 * p.1).sum::<f64>());
    let det = n * sxx - sx * sx;
    let (c0, c2) = if det.abs() > 1e-9 { ((sy * sxx - sx * sxy) / det, (n * sxy - sx * sy) / det) } else { (sy / n, 0.0) };
    let lap_model = reference.iter().map(|r| k * r.throttle / 255.0 * r.rpm).sum::<f64>() / HZ;
    let lap_measured = reference.first().map_or(0.0, |a| a.fuel) - reference.last().map_or(0.0, |b| b.fuel);
    Some(Model { k, coast: [c0.max(0.5), c2.max(0.0)], lap_model, lap_measured })
}

/// Braking points of a lap (rows with distance `s`) preceded by at least 30 m of full throttle.
/// `corner_at(s)`: the corner a braking point at `s` belongs to.
pub fn zones(rows: &[Row], m: &Model, corner_at: &dyn Fn(f64) -> Option<usize>) -> Vec<Zone> {
    let s = |i: usize| rows[i].s.unwrap_or(0.0);
    let mut out = Vec::new();
    let mut i = 1;
    while i < rows.len() {
        if rows[i].brake > 25.0 && rows[i - 1].brake <= 25.0 {
            let mut j = i - 1;
            while j > 0 && rows[j].throttle >= FULL && s(i) - s(j) < MAX_LIFT {
                j -= 1;
            }
            let len = s(i) - s(j + 1);
            let v = rows[i].speed_kmh / 3.6;
            if len >= 30.0 && v > 15.0 {
                let seg = &rows[j + 1..i];
                let per_m = lap::mean(seg.iter().map(|r| m.k * r.throttle / 255.0 * r.rpm / (r.speed_kmh / 3.6).max(1.0)));
                let a_thr = lap::mean(seg.iter().map(|r| r.surge)).max(0.0);
                let a_coast = m.coast[0] + m.coast[1] * v * v;
                out.push(Zone {
                    corner: corner_at(s(i)),
                    brake_at: s(i).round(),
                    kmh: rows[i].speed_kmh.round(),
                    max_m: len.min(MAX_LIFT).floor(),
                    per_m,
                    cost: (a_thr + a_coast) / (2.0 * v.powi(3)),
                });
            }
            while i < rows.len() && rows[i].brake > 13.0 {
                i += 1;
            }
        }
        i += 1;
    }
    out
}

/// Lifts that save `save` fuel per lap for the least time. `pos(s)`: position at distance s.
pub fn plan(zones: &[Zone], save: f64, pos: &dyn Fn(f64) -> (f64, f64)) -> Plan {
    let mut usable: Vec<bool> = zones.iter().map(|z| z.max_m >= MIN_LIFT && z.cost > 0.0 && z.per_m > 0.0).collect();
    let mut lens = vec![0.0; zones.len()];
    // Equal marginal cost: L = λ·f / (2c), capped; λ by bisection. Lifts too short to call are
    // dropped and the rest shared out again.
    for _ in 0..zones.len().max(1) {
        let lens_for = |lambda: f64| -> Vec<f64> {
            zones.iter().zip(&usable).map(|(z, u)| if *u { (lambda * z.per_m / (2.0 * z.cost)).clamp(0.0, z.max_m) } else { 0.0 }).collect()
        };
        let saved = |l: &[f64]| l.iter().zip(zones).map(|(l, z)| l * z.per_m).sum::<f64>();
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        while saved(&lens_for(hi)) < save && hi < 1e6 {
            hi *= 2.0;
        }
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if saved(&lens_for(mid)) < save {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lens = lens_for(hi);
        let short: Vec<usize> = (0..zones.len()).filter(|&i| usable[i] && lens[i] > 0.0 && lens[i] < MIN_LIFT).collect();
        if short.is_empty() {
            break;
        }
        // Drop the shortest one and share again
        let k = short.into_iter().min_by(|a, b| lens[*a].total_cmp(&lens[*b])).unwrap_or(0);
        usable[k] = false;
    }
    let mut lifts: Vec<Lift> = zones
        .iter()
        .zip(&lens)
        .filter(|(_, l)| **l >= MIN_LIFT - 0.5)
        .map(|(z, l)| {
            let l = ((l / 10.0).round() * 10.0).min(z.max_m);
            let (x, zz) = pos(z.brake_at - l);
            Lift {
                corner: z.corner,
                lift_m: l,
                kmh: z.kmh,
                saves: lap::round_to(l * z.per_m, 3),
                cost_s: lap::round_to(z.cost * l * l, 3),
                x: lap::round_to(x, 1),
                z: lap::round_to(zz, 1),
            }
        })
        .collect();
    lifts.sort_by(|a, b| b.saves.total_cmp(&a.saves));
    let saves: f64 = lifts.iter().map(|l| l.saves).sum();
    let cost_s: f64 = lifts.iter().map(|l| l.cost_s).sum();
    Plan { lifts, saves: lap::round_to(saves, 3), cost_s: lap::round_to(cost_s, 3), short: saves < save * 0.95 }
}

/// Fuel saving for a session: the model, the zones on the best lap and the plan for `save` per
/// lap (none: plans for a few typical amounts).
pub fn analyse(
    session: &std::path::Path,
    comparer: &mut super::compare::Comparer,
    save: Option<f64>,
) -> std::io::Result<serde_json::Value> {
    let res = comparer.compare(session)?;
    let Some(reference) = res.reference.clone() else { return Ok(serde_json::json!({ "error": "no_laps" })) };
    let mut ref_rows = lap::load(&session.join(&reference.file))?;
    lap::with_distance(&mut ref_rows);
    // The normal laps for the fit (at most 12, the latest ones)
    let normal: Vec<&super::compare::LapResult> = res.laps.iter().filter(|l| l.ms as f64 <= reference.ms as f64 * 1.07).collect();
    let mut laps = Vec::new();
    for l in normal.iter().rev().take(12) {
        laps.push(lap::load(&session.join(&l.file))?);
    }
    let Some(model) = fit(&laps, &ref_rows) else { return Ok(serde_json::json!({ "error": "no_fuel" })) };
    // The braking point belongs to the next apex within 300 m
    let corner_at = |s: f64| res.corners.iter().filter(|c| (c.s as f64) >= s && (c.s as f64) - s < 300.0).min_by_key(|c| c.s).map(|c| c.n);
    let zones = zones(&ref_rows, &model, &corner_at);
    let pos = |s: f64| ref_rows.iter().find(|r| r.s.unwrap_or(0.0) >= s).or(ref_rows.last()).map_or((0.0, 0.0), |r| (r.pos_x, r.pos_z));
    let amounts: Vec<f64> = match save {
        Some(v) => vec![v],
        None => [0.02, 0.04, 0.06].iter().map(|f| lap::round_to(model.lap_measured.max(model.lap_model) * f, 2)).collect(),
    };
    let plans: Vec<serde_json::Value> = amounts
        .iter()
        .filter(|a| **a > 0.0)
        .map(|a| {
            let p = plan(&zones, *a, &pos);
            serde_json::json!({ "save": a, "plan": p })
        })
        .collect();
    Ok(serde_json::json!({
        "session": session.file_name().map(|n| n.to_string_lossy().into_owned()),
        "reference": reference,
        "model": model,
        "zones": zones,
        "plans": plans,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(corner: usize, kmh: f64, max_m: f64) -> Zone {
        let v: f64 = kmh / 3.6;
        Zone { corner: Some(corner), brake_at: 1000.0, kmh, max_m, per_m: 0.0013, cost: 8.0 / (2.0 * v.powi(3)) }
    }

    #[test]
    fn saving_goes_to_the_fast_zones_first() {
        let zones = vec![zone(1, 300.0, 200.0), zone(2, 150.0, 200.0), zone(3, 280.0, 25.0)];
        let p = plan(&zones, 0.15, &|_| (0.0, 0.0));
        assert!(!p.short, "{p:?}");
        assert!((p.saves - 0.15).abs() < 0.02, "{p:?}");
        let l1 = p.lifts.iter().find(|l| l.corner == Some(1)).unwrap();
        let l2 = p.lifts.iter().find(|l| l.corner == Some(2)).map_or(0.0, |l| l.lift_m);
        assert!(l1.lift_m > l2, "{p:?}");
        // Asking the impossible: everything used, flagged short
        let p = plan(&zones, 5.0, &|_| (0.0, 0.0));
        assert!(p.short);
        assert_eq!(p.lifts.iter().map(|l| l.lift_m).sum::<f64>(), 200.0 + 200.0 + 25.0);
    }
}
