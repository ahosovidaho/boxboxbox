//! Spins and saved slides in a lap, and what led to them.
//!
//! A **spin** is a body slip angle over 30° or the car almost stopping on track; a **slide** (a
//! "moment") is the rear axle sliding (rear slip angle over 12°) for at least a quarter of a
//! second without a spin. For each event the 1.5 s before it are looked at – brake, downshift,
//! throttle lifted or added, steering, speed against the best lap – and the most likely cause is
//! named with a tip what to do differently.

use super::lap::{Row, HZ};
use serde::{Deserialize, Serialize};

const SPIN_SLIP: f64 = 30.0;
const SLIDE_REAR: f64 = 12.0;
/// A slide must go this far (°) beyond the rear slip of the best lap in the same corner
const REF_MARGIN: f64 = 6.0;
const SLIDE_MIN: usize = (HZ * 0.25) as usize;
/// Events closer than this are one event (samples)
const MERGE: usize = (HZ * 3.0) as usize;
/// Wheelspin before a slide: rear wheels this much faster than the car, at least this long
const WHEELSPIN: f64 = 1.15;
const WHEELSPIN_MIN: usize = (HZ * 0.2) as usize;
const LOOK_BACK: usize = (HZ * 1.5) as usize;
/// Back to normal driving: this long at speed without sliding (samples)
const RECOVERED: usize = HZ as usize;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Incident {
    /// "spin" or "slide"
    pub kind: String,
    /// Distance along the best lap (m) where it started
    pub s: f64,
    /// Corner (section) it happened in
    pub corner: Option<usize>,
    pub cause: String,
    pub tip: String,
    pub speed: f64,
    /// Speed against the best lap at the same point (km/h)
    pub dv: Option<f64>,
}

/// Rows with `s_ref` set; `reference` = the best lap; `corners` = (number, from m, to m).
pub fn detect(rows: &[Row], reference: &[Row], corners: &[(usize, f64, f64)]) -> Vec<Incident> {
    let n = rows.len();
    if n < 120 {
        return Vec::new();
    }
    let length = reference.last().map_or(0.0, |r| r.s_ref);
    // What the best lap itself does in each corner: a car that always slides a little there (a
    // kart, a drift-happy setup) only has a "slide" when it goes clearly beyond that
    let ref_slide: Vec<(f64, f64, f64)> = corners
        .iter()
        .map(|&(_, a, b)| {
            let m = reference.iter().filter(|r| a <= r.s_ref && r.s_ref <= b).filter_map(|r| r.ar).map(f64::abs).fold(0.0, f64::max);
            (a, b, m)
        })
        .collect();
    let slide_limit = |s: f64| {
        let m = ref_slide.iter().find(|(a, b, _)| *a <= s && s <= *b).map_or(0.0, |c| c.2);
        SLIDE_REAR.max(m + REF_MARGIN)
    };
    let mut starts: Vec<(usize, bool, usize)> = Vec::new(); // onset, spin, end
    let mut i = 0;
    while i < n {
        let r = &rows[i];
        let on_lap = r.s_ref > 50.0 && r.s_ref < length - 50.0;
        // Almost stopped on track after being fast
        let stopped = on_lap && r.speed_kmh < 25.0 && rows[i.saturating_sub(180)..i].iter().any(|p| p.speed_kmh > 80.0);
        let spin = r.beta.is_some_and(|b| b.abs() > SPIN_SLIP) || stopped;
        let limit = slide_limit(r.s_ref);
        let slide =
            r.ar.is_some_and(|a| a.abs() > limit) && (i..(i + SLIDE_MIN).min(n)).all(|k| rows[k].ar.is_some_and(|a| a.abs() > limit * 0.7));
        if on_lap && (spin || slide) {
            // Onset: walk back to where the rear started to slide
            let mut k = i;
            while k > 0 && i - k < LOOK_BACK && rows[k - 1].ar.is_some_and(|a| a.abs() > 6.0) {
                k -= 1;
            }
            // The event lasts until the car is going normally again (spinning round, standing,
            // turning back and driving off are all part of it)
            let mut end = i;
            let mut calm = 0;
            let mut is_spin = spin;
            while end < n && calm < RECOVERED {
                let p = &rows[end];
                is_spin |= p.beta.is_some_and(|b| b.abs() > SPIN_SLIP) || p.speed_kmh < 25.0;
                let ok = p.speed_kmh > 50.0 && p.beta.is_some_and(|b| b.abs() < 5.0) && p.ar.is_some_and(|a| a.abs() < 6.0);
                calm = if ok { calm + 1 } else { 0 };
                end += 1;
            }
            match starts.last_mut() {
                Some(last) if k < last.2 + MERGE => {
                    last.1 |= is_spin;
                    last.2 = end;
                }
                _ => starts.push((k, is_spin, end)),
            }
            i = end.max(i + MERGE);
            continue;
        }
        i += 1;
    }
    starts.into_iter().map(|(k, spin, _)| describe(rows, reference, corners, k, spin)).collect()
}

fn ref_speed(reference: &[Row], s: f64) -> Option<f64> {
    let i = reference.partition_point(|r| r.s_ref < s);
    reference.get(i.min(reference.len().saturating_sub(1))).map(|r| r.speed_kmh)
}

fn describe(rows: &[Row], reference: &[Row], corners: &[(usize, f64, f64)], k: usize, spin: bool) -> Incident {
    let r = &rows[k];
    let from = k.saturating_sub(LOOK_BACK);
    let win = &rows[from..=k];
    let recent = &rows[k.saturating_sub((HZ * 0.5) as usize)..=k];
    let thr = |r: &Row| r.throttle_pct();
    let brake_now = recent.iter().map(|r| r.brake_pct()).fold(0.0, f64::max);
    let downshift = recent.windows(2).any(|w| w[1].gear >= 1.0 && w[0].gear > w[1].gear);
    let thr_max = win.iter().map(thr).fold(0.0, f64::max);
    let thr_min_recent = recent.iter().map(thr).fold(100.0, f64::min);
    let lift = thr_max > 60.0 && thr_min_recent < 15.0 && thr(r) < 30.0 && brake_now < 10.0;
    // Throttle added within 0.3 s before the onset
    let q = (HZ * 0.3) as usize;
    let thr_rise = (q..win.len()).map(|j| thr(&win[j]) - thr(&win[j - q])).fold(0.0, f64::max);
    let steer_max = rows.iter().map(|r| r.steering.abs()).fold(0.0, f64::max).max(0.1);
    let steer = recent.iter().map(|r| r.steering.abs()).fold(0.0, f64::max);
    let dv = ref_speed(reference, r.s_ref).map(|v| r.speed_kmh - v);
    let corner = corners.iter().find(|(_, a, b)| *a <= r.s_ref && r.s_ref <= *b).map(|c| c.0);
    // Rear wheels clearly turning faster than the car moves, on the throttle, for a while in the
    // 1.5 s before (short peaks up to ~1.1 are normal hard acceleration, also on the best laps)
    let rear_spin = win
        .iter()
        .filter(|p| thr(p) > 50.0 && p.speed_kmh > 20.0)
        .filter(|p| (2..4).map(|w| p.wheel_rps[w].abs() * p.tire_radius[w]).fold(0.0, f64::max) / (p.speed_kmh / 3.6) > WHEELSPIN)
        .count()
        >= WHEELSPIN_MIN;
    // Much throttle while the wheel was still turned a lot, in the last second
    let last_s = &rows[k.saturating_sub(HZ as usize)..=k];
    let thr_turned = last_s.iter().any(|p| thr(p) > 60.0 && p.steering.abs() > steer_max * 0.5);

    let (cause, tip) = if brake_now > 20.0 && downshift {
        ("podřazení na brzdách – záď se odlehčila", "Podřazuj až když auto zpomalí, a brzdu povoluj dřív, než začneš zatáčet.")
    } else if brake_now > 20.0 {
        (
            "brzdění s natočeným volantem – záď se odlehčila",
            "Většinu brzdění udělej rovně; do zatáčky už jen dobrzďuj a brzdu plynule pouštěj.",
        )
    } else if rear_spin {
        (
            "prokluz zadních kol na plynu",
            "Na výjezdu přidávej plyn postupněji, ve 2.–3. stupni zvlášť; pomůže i kontrola trakce o stupeň výš.",
        )
    } else if lift {
        ("prudké ubrání plynu v zatáčce", "Plyn v zatáčce ubírej postupně – náhlé puštění odlehčí záď.")
    } else if thr_turned || (thr(r) > 50.0 && steer > steer_max * 0.5) {
        ("plyn s ještě natočeným volantem", "Plný plyn až když narovnáváš volant; do té doby jen jemně a postupně.")
    } else if thr_rise > 50.0 {
        ("prudké přidání plynu", "Plyn přidávej plynule, ne skokem – zvlášť na nízkém stupni.")
    } else if dv.is_some_and(|d| d > 8.0) {
        (
            "rychlejší vjezd než v nejlepším kole",
            "Do zatáčky vjížděj stejně rychle jako v nejlepším kole – rychlost navíc zatáčkou neprojede.",
        )
    } else {
        ("ztráta gripu bez jasné chyby pedálů", "Možný obrubník, nečistota nebo studené pneumatiky – zkontroluj stopu a teplotu gum.")
    };
    Incident {
        kind: if spin { "spin" } else { "slide" }.into(),
        s: r.s_ref.round(),
        corner,
        cause: cause.into(),
        tip: tip.into(),
        speed: r.speed_kmh.round(),
        dv: dv.map(|d| d.round()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Straight at 150 km/h, then a corner where the throttle goes to 100 % with the wheel turned
    /// and the rear steps out until the car spins.
    #[test]
    fn spin_from_throttle_with_steering() {
        let mut rows = Vec::new();
        for i in 0..1200 {
            let s = i as f64 * 2.0;
            let mut r =
                Row { s_ref: s, speed_kmh: 150.0, throttle: 255.0, gear: 4.0, ar: Some(1.0), beta: Some(0.5), ..Default::default() };
            if (600..700).contains(&i) {
                r.steering = 1.5;
                r.speed_kmh = 110.0;
                r.throttle = if i < 640 { 60.0 } else { 255.0 };
            }
            if (650..700).contains(&i) {
                r.ar = Some(14.0 + (i - 650) as f64);
                r.beta = Some((i - 650) as f64);
            }
            rows.push(r);
        }
        let reference = rows.clone();
        let inc = detect(&rows, &reference, &[(1, 0.0, 1000.0), (2, 1000.0, 1600.0)]);
        assert_eq!(inc.len(), 1, "{inc:?}");
        assert_eq!(inc[0].kind, "spin");
        assert_eq!(inc[0].corner, Some(2));
        assert!(inc[0].cause.contains("natočeným volantem"), "{:?}", inc[0]);
    }

    #[test]
    fn short_twitch_is_nothing() {
        let rows: Vec<Row> = (0..1200)
            .map(|i| Row {
                s_ref: i as f64 * 2.0,
                speed_kmh: 150.0,
                ar: Some(if (500..508).contains(&i) { 13.0 } else { 2.0 }),
                beta: Some(1.0),
                ..Default::default()
            })
            .collect();
        assert!(detect(&rows, &rows, &[]).is_empty());
    }
}
