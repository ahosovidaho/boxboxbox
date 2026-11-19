//! Gearing for a track: is the top gear right for the longest straight?
//!
//! From the laps of a session on the track: the fastest point of each lap (end of the longest
//! straight), the gear and engine speed there, and how long the engine sat on the limiter in the
//! top gear. The aim is to reach the limiter of the top gear just at the end of the longest
//! straight:
//!
//! - top gear far below the limiter there: the gearing is too long – a shorter final gear (a
//!   higher number in GT7) pulls harder everywhere;
//! - on the limiter in the top gear for a while: too short – the car could go faster;
//! - the top gear never used: the highest gears are wasted.
//!
//! The change is a proportion of the final gear: final × (target rpm / rpm reached), target 97 %
//! of the limiter. With the final gear of the setup card, the new value is given as well.

use serde::Serialize;
use std::path::Path;

/// Aim: this share of the limiter at the fastest point
const TARGET: f32 = 0.97;
/// Below this share of the limiter in the top gear: too long
const LONG: f32 = 0.92;
/// Full throttle on the limiter in the top gear for this long per lap (s): too short
const SHORT_S: f32 = 0.6;
const FULL: f32 = 250.0;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Gearing {
    /// Laps looked at
    pub laps: usize,
    /// Gears of the car (non-zero transmission ratios)
    pub gears: u8,
    /// Highest gear used at the fastest point (median of the laps)
    pub top_gear: u8,
    /// Speed and engine speed at the fastest point (median)
    pub kmh: f32,
    pub rpm: f32,
    /// Limiter (highest engine speed reached at full throttle)
    pub limiter: f32,
    /// Seconds per lap on the limiter in the top gear
    pub limiter_s: f32,
    /// "ok", "long", "short", "unused"
    pub verdict: String,
    /// Change of the final gear, % (positive = higher number = shorter)
    pub change_pct: f32,
    pub text: String,
}

struct Row {
    v: f32,
    rpm: f32,
    gear: u8,
    thr: f32,
    t_ms: f32,
}

fn rows(text: &str) -> (Vec<Row>, u8) {
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let col = |n: &str| head.iter().position(|h| *h == n);
    let (Some(v), Some(r), Some(g), Some(t), Some(ms)) = (col("speed_kmh"), col("rpm"), col("gear"), col("throttle"), col("lap_ms")) else {
        return (vec![], 0);
    };
    let ratios: Vec<usize> = (1..=8).filter_map(|i| col(&format!("gear_ratio_{i}"))).collect();
    let mut gears = 0u8;
    let mut out = Vec::new();
    for l in lines {
        let c: Vec<&str> = l.split(',').collect();
        let f = |i: usize| c.get(i).and_then(|x| x.parse::<f32>().ok());
        if gears == 0 {
            gears = ratios.iter().filter(|&&i| f(i).is_some_and(|x| x > 0.0)).count() as u8;
        }
        if let (Some(v), Some(rpm), Some(gear), Some(thr), Some(t_ms)) = (f(v), f(r), f(g), f(t), f(ms)) {
            out.push(Row { v, rpm, gear: gear as u8, thr, t_ms });
        }
    }
    (out, gears)
}

/// 10.7 → "10,7"
fn cz(v: f32) -> String {
    format!("{:.1}", v).replace('.', ",")
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// The gearing verdict from lap files (whole laps of one car on one track).
pub fn analyse(laps: &[&Path]) -> Option<Gearing> {
    let mut tops = Vec::new();
    let mut gears = 0u8;
    let mut limiter = 0.0f32;
    let mut on_limiter = Vec::new();
    let mut all: Vec<Vec<Row>> = Vec::new();
    for p in laps {
        let Ok(text) = crate::coach::lap::read_text(p) else { continue };
        let (r, g) = rows(&text);
        if r.len() < 200 {
            continue;
        }
        gears = gears.max(g);
        limiter = limiter.max(r.iter().filter(|x| x.thr >= FULL).map(|x| x.rpm).fold(0.0, f32::max));
        let fast = r.iter().max_by(|a, b| a.v.total_cmp(&b.v))?;
        tops.push((fast.gear, fast.v, fast.rpm));
        all.push(r);
    }
    if tops.is_empty() || limiter <= 0.0 {
        return None;
    }
    let top_gear = {
        let mut g: Vec<u8> = tops.iter().map(|t| t.0).collect();
        g.sort_unstable();
        g[g.len() / 2]
    };
    // Time on the limiter in that gear at full throttle, per lap
    for r in &all {
        let mut s = 0.0;
        for w in r.windows(2) {
            if w[0].gear == top_gear && w[0].thr >= FULL && w[0].rpm >= limiter * 0.985 {
                s += ((w[1].t_ms - w[0].t_ms) / 1000.0).clamp(0.0, 0.1);
            }
        }
        on_limiter.push(s);
    }
    let kmh = median(tops.iter().map(|t| t.1).collect());
    let rpm = median(tops.iter().filter(|t| t.0 == top_gear).map(|t| t.2).collect());
    let limiter_s = median(on_limiter);
    let share = rpm / limiter;
    let pct = |x: f32| (x * 1000.0).round() / 10.0;
    let (verdict, change, text) = if gears > 0 && top_gear + 1 < gears {
        (
            "unused",
            0.0,
            format!(
                "Na nejdelší rovince končíš na {top_gear}. stupni z {gears} – {}. a vyšší nevyužiješ. Pokud jde převodovka nastavit, zkrať poměry (nebo stálý převod), ať jsou rychlosti blíž u sebe.",
                top_gear + 1
            ),
        )
    } else if limiter_s >= SHORT_S {
        let c = -(0.01 + 0.01 * limiter_s).min(0.06);
        (
            "short",
            c,
            format!(
                "Na konci rovinky jsi na {top_gear}. stupni {} s v omezovači ({:.0} km/h) – prodluž stálý převod asi o {} % (nižší číslo).",
                cz(limiter_s),
                kmh,
                cz(pct(-c))
            ),
        )
    } else if share < LONG {
        let c = TARGET * limiter / rpm - 1.0;
        (
            "long",
            c,
            format!(
                "Na nejdelší rovince vytočíš {top_gear}. stupeň jen na {:.0} ot. z {:.0} ({:.0} %) – zkrať stálý převod asi o {} % (vyšší číslo), auto bude všude zrychlovat líp.",
                rpm,
                limiter,
                share * 100.0,
                cz(pct(c))
            ),
        )
    } else {
        (
            "ok",
            0.0,
            format!("Převodovka sedí: na konci nejdelší rovinky {:.0} km/h na {top_gear}. stupni, {:.0} % omezovače.", kmh, share * 100.0),
        )
    };
    Some(Gearing {
        laps: all.len(),
        gears,
        top_gear,
        kmh: kmh.round(),
        rpm: rpm.round(),
        limiter: limiter.round(),
        limiter_s: (limiter_s * 10.0).round() / 10.0,
        verdict: verdict.to_string(),
        change_pct: pct(change),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(dir: &Path, name: &str, top_rpm: f32, limiter_ticks: usize) -> std::path::PathBuf {
        let mut s = String::from("lap,lap_ms,speed_kmh,rpm,gear,throttle,gear_ratio_1,gear_ratio_2,gear_ratio_3,gear_ratio_4,gear_ratio_5,gear_ratio_6,gear_ratio_7,gear_ratio_8\n");
        let mut t = 0.0;
        // Accelerate through 4th to 6th, then the straight's end in 6th
        for i in 0..600 {
            let gear = if i < 200 {
                4
            } else if i < 400 {
                5
            } else {
                6
            };
            let frac = (i % 200) as f32 / 200.0;
            let rpm = if gear == 6 { 7000.0 + (top_rpm - 7000.0) * frac } else { 6500.0 + 2000.0 * frac };
            let v = 150.0 + i as f32 * 0.25;
            s += &format!("1,{t},{v},{rpm},{gear},255,3.5,2.5,1.9,1.5,1.25,1.05,0,0\n");
            t += 16.7;
        }
        for _ in 0..limiter_ticks {
            s += &format!("1,{t},300,8500,6,255,3.5,2.5,1.9,1.5,1.25,1.05,0,0\n");
            t += 16.7;
        }
        let p = dir.join(name);
        std::fs::write(&p, s).unwrap();
        p
    }

    #[test]
    fn long_short_and_ok() {
        let dir = std::env::temp_dir().join(format!("gearing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Limiter 8 500 (reached in 4th and 5th); top gear only to 7 400 → long, ~+11 %
        let a = lap(&dir, "a.csv", 7400.0, 0);
        let g = analyse(&[a.as_path()]).unwrap();
        assert_eq!((g.verdict.as_str(), g.top_gear, g.gears), ("long", 6, 6), "{g:?}");
        assert!((g.change_pct - 11.4).abs() < 0.5, "{g:?}");
        // A second on the limiter in 6th → short
        let b = lap(&dir, "b.csv", 8400.0, 60);
        let g = analyse(&[b.as_path()]).unwrap();
        assert_eq!(g.verdict, "short", "{g:?}");
        assert!(g.change_pct < 0.0);
        // Right at 96 % of the limiter → ok
        let c = lap(&dir, "c.csv", 8200.0, 0);
        assert_eq!(analyse(&[c.as_path()]).unwrap().verdict, "ok");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
