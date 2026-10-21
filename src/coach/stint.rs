//! Long runs: how the lap time and every corner develop over a run on one set of tyres.
//!
//! A run is the laps between two restarts of GT7 (the lap count starts again). Over a run two
//! things act against each other: the tyres wear (slower) and the fuel burns off (faster). What is
//! measured is the sum; the corners that get worse most show where the tyres go first – that is
//! where to take care of them in the race. A cliff is a sudden step in the lap time after which it
//! does not come back.

use super::compare::{Comparison, LapResult};
use serde::Serialize;

/// Laps of a run needed for a trend
const MIN_LAPS: usize = 6;
/// Step in the 3-lap average that counts as a cliff (ms)
const CLIFF_MS: f64 = 600.0;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CornerTrend {
    pub n: usize,
    /// Change of the time lost in the corner per lap of the run (s)
    pub per_lap: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Run {
    pub run: usize,
    /// All laps of the run and the normal ones the trend is fitted to
    pub laps: usize,
    pub normal: usize,
    /// Lap time trend (s per lap), average of the first and of the last 3 normal laps (ms)
    pub per_lap: f64,
    pub start_ms: f64,
    pub end_ms: f64,
    /// Lap of the run (1 = first) after which the lap time stepped up for good
    pub cliff: Option<usize>,
    /// Corners getting worse fastest
    pub corners: Vec<CornerTrend>,
    pub text: String,
}

/// Slope of y over x (least squares).
fn slope(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len() as f64;
    let (mx, my) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx <= 0.0 {
        return 0.0;
    }
    pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>() / sxx
}

fn fmt_lap(ms: f64) -> String {
    let ms = ms.round() as i64;
    format!("{}:{:02},{}", ms / 60_000, (ms % 60_000) / 1000, format!("{:03}", ms % 1000).chars().next().unwrap_or('0'))
}

fn sgn(v: f64, d: usize) -> String {
    format!("{}{:.*}", if v >= 0.0 { "+" } else { "−" }, d, v.abs()).replace('.', ",")
}

/// Runs of a session with enough normal laps, longest first. `tyres`: the tyres wear and fuel
/// burns (false for karts).
pub fn analyse(res: &Comparison, tyres: bool) -> Vec<Run> {
    let Some(reference) = res.reference_lap() else { return Vec::new() };
    let limit = reference.ms as f64 * 1.07;
    // Split into runs where the lap number starts again
    let mut runs: Vec<Vec<&LapResult>> = Vec::new();
    for l in &res.laps {
        match runs.last_mut() {
            Some(r) if r.last().is_some_and(|p| l.lap > p.lap) => r.push(l),
            _ => runs.push(vec![l]),
        }
    }
    let mut out: Vec<Run> = runs
        .iter()
        .enumerate()
        .filter_map(|(i, laps)| {
            // Lap of the run = its position (incident laps count: the tyres did those laps too)
            let normal: Vec<(f64, &LapResult)> =
                laps.iter().enumerate().filter(|(_, l)| (l.ms as f64) <= limit).map(|(k, l)| ((k + 1) as f64, *l)).collect();
            if normal.len() < MIN_LAPS {
                return None;
            }
            let per_lap = slope(&normal.iter().map(|(k, l)| (*k, l.ms as f64 / 1000.0)).collect::<Vec<_>>());
            let avg = |s: &[(f64, &LapResult)]| s.iter().map(|(_, l)| l.ms as f64).sum::<f64>() / s.len() as f64;
            let (start_ms, end_ms) = (avg(&normal[..3]), avg(&normal[normal.len() - 3..]));
            // Cliff: the 3-lap average from some lap on is clearly above everything before it
            let cliff = (3..normal.len().saturating_sub(2)).find(|&j| {
                let before = avg(&normal[..j]);
                let after = &normal[j..];
                // Every lap from there on clearly slower (one exception allowed), the first one too
                (after[0].1.ms as f64) - before > CLIFF_MS * 0.7
                    && after.iter().filter(|(_, l)| (l.ms as f64) - before <= CLIFF_MS * 0.7).count() <= 1
                    && avg(after) - before > CLIFF_MS
            });
            let cliff = cliff.map(|j| normal[j - 1].0 as usize);
            let mut corners: Vec<CornerTrend> = res
                .corners
                .iter()
                .map(|c| CornerTrend {
                    n: c.n,
                    per_lap: super::lap::round_to(slope(&normal.iter().map(|(k, l)| (*k, l.corner(c.n).dt)).collect::<Vec<_>>()), 3),
                })
                // While the driver is still getting faster overall, small trends are noise
                .filter(|c| c.per_lap >= 0.02 && per_lap > -0.05)
                .collect();
            corners.sort_by(|a, b| b.per_lap.total_cmp(&a.per_lap));
            corners.truncate(3);
            let mut text = format!("{} kol: tempo {} s/kolo (z {} na {})", laps.len(), sgn(per_lap, 2), fmt_lap(start_ms), fmt_lap(end_ms));
            if per_lap < -0.02 {
                text.push_str(if tyres { " – zrychluješ (ubývá palivo, rozjíždíš se)" } else { " – rozjíždíš se" });
            } else if per_lap > 0.05 {
                text.push_str(if tyres { " – gumy odcházejí" } else { " – zpomaluješ (únava, koncentrace?)" });
            }
            text.push('.');
            if !corners.is_empty() {
                let list: Vec<String> = corners.iter().map(|c| format!("Z{} {} s/kolo", c.n, sgn(c.per_lap, 2))).collect();
                text.push_str(&format!(
                    " Nejvíc se zhoršuje {}{}.",
                    list.join(", "),
                    if tyres { " – tam v závodě šetři gumy" } else { "" }
                ));
            }
            if let Some(c) = cliff {
                text.push_str(&format!(" Zlom po {c}. kole{}.", if tyres { " – dál už gumy nedrží" } else { "" }));
            }
            Some(Run {
                run: i + 1,
                laps: laps.len(),
                normal: normal.len(),
                per_lap: super::lap::round_to(per_lap, 3),
                start_ms: start_ms.round(),
                end_ms: end_ms.round(),
                cliff,
                corners,
                text,
            })
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.laps));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trend_worst_corner_and_cliff() {
        let corner = |n: usize| json!({ "n": n, "dir": "L", "x": 0.0, "z": 0.0, "s": 0, "ref": null });
        let mut laps = Vec::new();
        for k in 1..=12i64 {
            // +0.05 s per lap, Z2 gets 0.04 s worse per lap; from lap 9 on +1 s more
            let ms = 90_000 + 50 * k + if k >= 9 { 1000 } else { 0 };
            laps.push(json!({ "file": format!("{k:03}"), "lap": k, "ms": ms, "delta_ms": 0,
                "corners": [{ "n": 1, "dt": 0.0 }, { "n": 2, "dt": 0.04 * k as f64 }] }));
        }
        let res: Comparison = serde_json::from_value(json!({
            "reference": { "file": "001", "ms": 90_050, "lap": 1 }, "steer_ratio": 15.0,
            "corners": [corner(1), corner(2)], "laps": laps, "theoretical_ms": null,
        }))
        .unwrap();
        let r = analyse(&res, true);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].corners[0].n, 2, "{r:?}");
        assert_eq!(r[0].cliff, Some(8), "{r:?}");
        assert!(r[0].text.contains("Zlom po 8. kole"), "{}", r[0].text);
    }
}
