//! Gear changes in a lap and how they compare with the best shift points.
//!
//! Upshifts at full throttle are compared with the optimum of the car (shift points from
//! driving, else the game's shift light): too early costs pull, bouncing off the limiter costs
//! time too. Upshifts at part throttle (short shifting out of a slow corner, on purpose) are
//! listed but not judged. Downshifts are checked for over-revving (engine on the limiter after
//! the downshift: the rear axle locks up for a moment, the car gets nervous on the brakes).

use super::lap::{Row, HZ};
use serde::{Deserialize, Serialize};

/// Full throttle at the moment of the upshift (%)
const FULL: f64 = 90.0;
/// Shifted this much below the optimum counts as early (rpm)
const EARLY: f64 = 300.0;
/// Too late: within this share of the limiter for at least `LIMITER_MS`
const LIMITER_SHARE: f64 = 0.995;
const LIMITER_S: f64 = 0.15;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ShiftEv {
    /// Distance along the best lap (m)
    pub s: f64,
    pub from: u8,
    pub to: u8,
    /// Engine speed just before the change (upshift) / highest just after it (downshift)
    pub rpm: f64,
    /// Throttle at the change (%)
    pub thr: f64,
    pub speed: f64,
    /// Upshift: the engine sat on the limiter before the change
    #[serde(default)]
    pub limiter: bool,
}

/// All gear changes of a lap (`rows` with `s_ref` set). `max_rpm`: limiter, 0 = unknown.
pub fn detect(rows: &[Row], max_rpm: f64) -> Vec<ShiftEv> {
    let mut out = Vec::new();
    let valid = |g: f64| (1.0..=10.0).contains(&g);
    let lim_n = (HZ * LIMITER_S) as usize;
    for i in 1..rows.len() {
        let (a, b) = (&rows[i - 1], &rows[i]);
        if !valid(a.gear) || !valid(b.gear) || a.gear == b.gear || b.speed_kmh < 15.0 {
            continue;
        }
        let (from, to) = (a.gear as u8, b.gear as u8);
        if to > from {
            // Highest engine speed in the 0.1 s before the change (the gear value switches a
            // sample or two after the driver's paddle)
            let before = &rows[i.saturating_sub(6)..i];
            let rpm = before.iter().map(|r| r.rpm).fold(0.0, f64::max);
            let limiter =
                max_rpm > 0.0 && rows[i.saturating_sub(lim_n + 6)..i].iter().filter(|r| r.rpm >= max_rpm * LIMITER_SHARE).count() >= lim_n;
            // The game cuts the throttle for the change itself: the driver's throttle is the
            // highest in the 0.25 s before
            let thr = rows[i.saturating_sub(15)..i].iter().map(Row::throttle_pct).fold(0.0, f64::max);
            out.push(ShiftEv { s: a.s_ref.round(), from, to, rpm: rpm.round(), thr: thr.round(), speed: a.speed_kmh.round(), limiter });
        } else {
            let after = &rows[i..(i + (HZ * 0.4) as usize).min(rows.len())];
            let rpm = after.iter().map(|r| r.rpm).fold(0.0, f64::max);
            out.push(ShiftEv {
                s: a.s_ref.round(),
                from,
                to,
                rpm: rpm.round(),
                thr: a.throttle_pct().round(),
                speed: a.speed_kmh.round(),
                limiter: false,
            });
        }
    }
    out
}

/// Where the car should shift up: per gear (from), rpm and "as late as possible".
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Optimum {
    pub gears: Vec<(u8, f64, bool)>,
    pub max_rpm: f64,
    /// "drive" (shift points from driving) or "game" (shift light)
    pub source: &'static str,
}

impl Optimum {
    pub fn rpm(&self, from: u8) -> Option<(f64, bool)> {
        self.gears.iter().find(|g| g.0 == from).map(|g| (g.1, g.2))
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct GearAdvice {
    pub from: u8,
    pub to: u8,
    /// Full-throttle upshifts looked at, their median and spread (p90 − p10) in rpm
    pub count: usize,
    pub median: f64,
    pub spread: f64,
    pub optimum: Option<f64>,
    pub late: bool,
    /// Upshifts that sat on the limiter
    pub limiter: usize,
    /// "ok", "early", "late", "limiter" or "uneven"
    pub verdict: &'static str,
    pub text: String,
}

fn fmt_rpm(v: f64) -> String {
    let v = (v / 10.0).round() as i64 * 10;
    if v >= 1000 {
        format!("{} {:03}", v / 1000, v % 1000)
    } else {
        v.to_string()
    }
}

/// Upshift advice per gear over the normal laps of a session; plus over-revving downshifts.
pub fn advice(laps: &[&[ShiftEv]], opt: &Optimum) -> (Vec<GearAdvice>, [usize; 2]) {
    let mut by: std::collections::BTreeMap<(u8, u8), Vec<&ShiftEv>> = Default::default();
    let (mut overrev, mut downs) = (0, 0);
    for lap in laps {
        for e in lap.iter() {
            if e.to > e.from && e.thr >= FULL {
                by.entry((e.from, e.to)).or_default().push(e);
            } else if e.to < e.from {
                downs += 1;
                if opt.max_rpm > 0.0 && e.rpm >= opt.max_rpm * 0.99 {
                    overrev += 1;
                }
            }
        }
    }
    let out = by
        .into_iter()
        .filter(|(_, v)| v.len() >= 2)
        .map(|((from, to), v)| {
            let mut r: Vec<f64> = v.iter().map(|e| e.rpm).collect();
            r.sort_by(f64::total_cmp);
            let q = |p: f64| r[((r.len() - 1) as f64 * p).round() as usize];
            let (median, spread) = (q(0.5), q(0.9) - q(0.1));
            let limiter = v.iter().filter(|e| e.limiter).count();
            let (optimum, late) = opt.rpm(from).map_or((None, false), |(o, l)| (Some(if l { opt.max_rpm.max(o) } else { o }), l));
            let (verdict, text) = match optimum {
                _ if limiter * 2 > v.len() => (
                    "limiter",
                    format!("{from}→{to}: motor stojí na omezovači – řaď o kousek dřív (~{})", fmt_rpm(optimum.unwrap_or(median) - 150.0)),
                ),
                Some(o) if median < o - EARLY => (
                    "early",
                    if late {
                        format!(
                            "{from}→{to}: řadíš při {} – nižší stupeň táhne až do omezovače, vytoč ho (~{})",
                            fmt_rpm(median),
                            fmt_rpm(o)
                        )
                    } else {
                        format!(
                            "{from}→{to}: řadíš při {}, optimum {} – řaď o ~{} ot. později",
                            fmt_rpm(median),
                            fmt_rpm(o),
                            fmt_rpm(o - median)
                        )
                    },
                ),
                Some(o) if !late && median > o + EARLY => (
                    "late",
                    format!("{from}→{to}: řadíš při {}, optimum {} – vyšší stupeň už táhne víc, řaď dřív", fmt_rpm(median), fmt_rpm(o)),
                ),
                _ if spread > 800.0 => ("uneven", format!("{from}→{to}: řadíš nepravidelně (rozptyl {} ot.)", fmt_rpm(spread))),
                Some(_) => ("ok", format!("{from}→{to}: v pořádku ({})", fmt_rpm(median))),
                None => ("ok", format!("{from}→{to}: řadíš při {} (optimum zatím neznámé)", fmt_rpm(median))),
            };
            GearAdvice { from, to, count: v.len(), median, spread, optimum, late, limiter, verdict, text }
        })
        .collect();
    (out, [overrev, downs])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(s: f64, gear: f64, rpm: f64, thr: f64) -> Row {
        Row { s_ref: s, gear, rpm, throttle: thr * 2.55, speed_kmh: 100.0, ..Default::default() }
    }

    #[test]
    fn upshifts_downshifts_and_advice() {
        // 2nd up to 12 000, shift; 3rd up to the limiter (15 000) for 0.5 s, shift; brake, 4→3
        // with the engine thrown to the limiter
        let mut rows = Vec::new();
        let mut s = 0.0;
        for k in 0..60 {
            rows.push(row(s, 2.0, 9_000.0 + 50.0 * k as f64, 100.0));
            s += 1.0;
        }
        for k in 0..60 {
            rows.push(row(s, 3.0, 10_000.0 + 100.0 * k as f64, 100.0));
            s += 1.0;
        }
        for _ in 0..30 {
            rows.push(row(s, 3.0, 15_000.0, 100.0));
            s += 1.0;
        }
        for _ in 0..30 {
            rows.push(row(s, 4.0, 12_000.0, 0.0));
            s += 1.0;
        }
        for _ in 0..30 {
            rows.push(row(s, 3.0, 15_000.0, 0.0));
            s += 1.0;
        }
        let ev = detect(&rows, 15_000.0);
        assert_eq!(ev.iter().map(|e| (e.from, e.to)).collect::<Vec<_>>(), [(2, 3), (3, 4), (4, 3)]);
        assert_eq!(ev[0].rpm, 11_950.0);
        assert!(!ev[0].limiter && ev[1].limiter);
        let opt = Optimum { gears: vec![(2, 14_000.0, false), (3, 14_500.0, false)], max_rpm: 15_000.0, source: "drive" };
        let laps: Vec<&[ShiftEv]> = vec![&ev, &ev];
        let (adv, overrev) = advice(&laps, &opt);
        assert_eq!(overrev, [2, 2]);
        assert_eq!(adv[0].verdict, "early", "{adv:?}");
        assert!(adv[0].text.contains("řaď o ~2 050 ot. později"), "{}", adv[0].text);
        assert_eq!(adv[1].verdict, "limiter", "{adv:?}");
    }
}
