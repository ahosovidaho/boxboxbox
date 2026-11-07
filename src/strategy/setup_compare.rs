//! Fair comparison of car setups from the clean practice laps of a track.
//!
//! Raw lap times are not comparable: a lap on new tyres with an empty tank is faster than one at
//! the end of a stint with a full tank, whatever the setup. The laps are therefore fitted with
//!
//! ```text
//! lap time = base(setup, compound) + wear(compound) · tyre age + fuel effect · (fuel − reference)
//! ```
//!
//! by least squares; `base` is the lap time of the setup on new tyres with the reference fuel,
//! and that is what is compared. Wear and fuel effect are learned from the laps, held towards a
//! typical value (0.2 s per lap of tyre age, 0.03 s per litre) while there are few laps or the
//! laps do not separate the two (one stint: the tyres get older while the tank gets lighter).

use super::practice::PracticeLap;
use serde::Serialize;

/// Typical values the fit is held towards (s per lap of tyre age, s per litre)
const PRIOR_WEAR: f64 = 0.2;
const PRIOR_FUEL: f64 = 0.03;
/// Weight of the typical values (about one lap with 3 laps of age spread / 7 l fuel spread)
const LAMBDA_WEAR: f64 = 10.0;
const LAMBDA_FUEL: f64 = 50.0;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SetupRow {
    pub setup: String,
    pub compound: String,
    pub laps: usize,
    pub best_ms: i32,
    pub median_ms: i32,
    /// Fitted lap time on new tyres with the reference fuel
    pub fair_ms: i32,
    /// Standard error of `fair_ms` (ms)
    pub se_ms: i32,
    /// Against the fastest setup on the same compound (ms, 0 for it) and whether the gap is
    /// larger than twice its uncertainty
    pub gap_ms: i32,
    pub sure: bool,
    /// The only setup driven on this compound (nothing to compare it with)
    pub alone: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Fair {
    pub rows: Vec<SetupRow>,
    /// Fitted wear per compound (s per lap of tyre age) and fuel effect (s per litre)
    pub wear: Vec<(String, f64)>,
    pub fuel_effect: f64,
    pub reference_fuel: Option<f64>,
    /// The laps had enough spread in tyre age / fuel for the values to come from the data
    pub wear_from_data: bool,
    pub fuel_from_data: bool,
    /// No tyre wear and no fuel use (karts): plain average of the clean laps
    pub fixed: bool,
}

/// Solves `m · x = v` (small, dense; Gaussian elimination with partial pivoting).
fn solve(mut m: Vec<Vec<f64>>, mut v: Vec<f64>) -> Option<Vec<f64>> {
    let n = v.len();
    for c in 0..n {
        let p = (c..n).max_by(|a, b| m[*a][c].abs().total_cmp(&m[*b][c].abs()))?;
        if m[p][c].abs() < 1e-9 {
            return None;
        }
        m.swap(c, p);
        v.swap(c, p);
        let (top, rest) = m.split_at_mut(c + 1);
        let pivot = &top[c];
        for (k, row) in rest.iter_mut().enumerate() {
            let f = row[c] / pivot[c];
            for (a, b) in row[c..].iter_mut().zip(&pivot[c..]) {
                *a -= f * b;
            }
            v[c + 1 + k] -= f * v[c];
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let s: f64 = (r + 1..n).map(|k| m[r][k] * x[k]).sum();
        x[r] = (v[r] - s) / m[r][r];
    }
    Some(x)
}

/// `laps`: the clean, counted laps of one car on one track (laps with a mistake are left out here).
/// `fixed`: the tyres do not wear and no fuel is used (karts) – nothing to correct.
pub fn fair(laps: &[&PracticeLap], fixed: bool) -> Fair {
    // Laps with a mistake (spin, excursion) say nothing about the setup: like the practice
    // figures, a lap more than 2 s or 2 % slower than the typical lap of its setup is left out
    let typical = |l: &PracticeLap| {
        let mut ms: Vec<i32> = laps.iter().filter(|o| o.setup == l.setup && o.compound == l.compound).map(|o| o.ms).collect();
        ms.sort_unstable();
        ms[ms.len() / 2]
    };
    let kept: Vec<&PracticeLap> = laps.iter().copied().filter(|l| l.ms <= typical(l) + (typical(l) / 50).max(2000)).collect();
    let laps = &kept[..];
    let mut groups: Vec<(String, String)> = Vec::new();
    let mut compounds: Vec<String> = Vec::new();
    for l in laps {
        let g = (l.setup.clone(), l.compound.clone());
        if !groups.contains(&g) {
            groups.push(g);
        }
        if !compounds.contains(&l.compound) {
            compounds.push(l.compound.clone());
        }
    }
    if laps.is_empty() {
        return Fair::default();
    }
    let mut fuels: Vec<f64> = laps.iter().filter_map(|l| l.fuel_start).map(f64::from).collect();
    fuels.sort_by(f64::total_cmp);
    let reference_fuel = fuels.get(fuels.len() / 2).copied();
    let (ng, nc) = (groups.len(), compounds.len());
    let n = ng + nc + 1;
    let mut m = vec![vec![0.0; n]; n];
    let mut v = vec![0.0; n];
    for l in laps {
        let mut x = vec![0.0; n];
        x[groups.iter().position(|g| g.0 == l.setup && g.1 == l.compound).unwrap_or(0)] = 1.0;
        x[ng + compounds.iter().position(|c| *c == l.compound).unwrap_or(0)] = l.age as f64;
        // Laps without the fuel recorded (older versions) count as reference fuel
        x[n - 1] = match (l.fuel_start, reference_fuel) {
            (Some(f), Some(r)) => f as f64 - r,
            _ => 0.0,
        };
        let y = l.ms as f64 / 1000.0;
        for i in 0..n {
            v[i] += x[i] * y;
            for j in 0..n {
                m[i][j] += x[i] * x[j];
            }
        }
    }
    // Hold wear and fuel effect towards the typical values; without wear and fuel (karts), or for
    // a compound whose laps got faster with age (the driver warming up, not the tyres), held at 0
    let fit = |zero_wear: &[bool]| {
        let (mut m, mut v) = (m.clone(), v.clone());
        for c in 0..nc {
            let (lam, prior) = if fixed || zero_wear[c] { (1e9, 0.0) } else { (LAMBDA_WEAR, PRIOR_WEAR) };
            m[ng + c][ng + c] += lam;
            v[ng + c] += lam * prior;
        }
        let (lam, prior) = if fixed { (1e9, 0.0) } else { (LAMBDA_FUEL, PRIOR_FUEL) };
        m[n - 1][n - 1] += lam;
        v[n - 1] += lam * prior;
        solve(m, v)
    };
    let Some(mut theta) = fit(&vec![false; nc]) else { return Fair::default() };
    let negative: Vec<bool> = (0..nc).map(|c| theta[ng + c] < 0.0).collect();
    if negative.iter().any(|x| *x) {
        if let Some(t) = fit(&negative) {
            theta = t;
        }
    }

    // Residual spread → uncertainty of each setup's base time
    let predict = |l: &PracticeLap| {
        let g = groups.iter().position(|g| g.0 == l.setup && g.1 == l.compound).unwrap_or(0);
        let c = compounds.iter().position(|c| *c == l.compound).unwrap_or(0);
        let df = match (l.fuel_start, reference_fuel) {
            (Some(f), Some(r)) => f as f64 - r,
            _ => 0.0,
        };
        theta[g] + theta[ng + c] * l.age as f64 + theta[n - 1] * df
    };
    let res: Vec<f64> = laps.iter().map(|l| l.ms as f64 / 1000.0 - predict(l)).collect();
    let dof = (laps.len() as f64 - ng as f64).max(1.0);
    let sigma = (res.iter().map(|r| r * r).sum::<f64>() / dof).sqrt().max(0.05);

    let mut rows: Vec<SetupRow> = groups
        .iter()
        .enumerate()
        .map(|(gi, (setup, compound))| {
            let mut ms: Vec<i32> = laps.iter().filter(|l| l.setup == *setup && l.compound == *compound).map(|l| l.ms).collect();
            ms.sort_unstable();
            SetupRow {
                setup: setup.clone(),
                compound: compound.clone(),
                laps: ms.len(),
                best_ms: ms[0],
                median_ms: ms[ms.len() / 2],
                fair_ms: (theta[gi] * 1000.0).round() as i32,
                se_ms: (sigma / (ms.len() as f64).sqrt() * 1000.0).round() as i32,
                gap_ms: 0,
                sure: false,
                alone: false,
            }
        })
        .collect();
    for c in &compounds {
        let alone = rows.iter().filter(|r| r.compound == *c).count() == 1;
        rows.iter_mut().filter(|r| r.compound == *c).for_each(|r| r.alone = alone);
        let Some(best) = rows.iter().filter(|r| r.compound == *c).min_by_key(|r| r.fair_ms).cloned() else { continue };
        for r in rows.iter_mut().filter(|r| r.compound == *c) {
            r.gap_ms = r.fair_ms - best.fair_ms;
            let se = ((r.se_ms as f64).powi(2) + (best.se_ms as f64).powi(2)).sqrt();
            r.sure = r.setup != best.setup && r.gap_ms as f64 > 2.0 * se;
        }
    }
    rows.sort_by(|a, b| a.compound.cmp(&b.compound).then(a.fair_ms.cmp(&b.fair_ms)));

    let spread = |vals: &[f64]| vals.iter().copied().fold(f64::NEG_INFINITY, f64::max) - vals.iter().copied().fold(f64::INFINITY, f64::min);
    let ages: Vec<f64> = laps.iter().map(|l| l.age as f64).collect();
    Fair {
        rows,
        wear: compounds.iter().enumerate().map(|(i, c)| (c.clone(), (theta[ng + i] * 1000.0).round() / 1000.0)).collect(),
        fuel_effect: (theta[n - 1] * 1000.0).round() / 1000.0,
        reference_fuel: reference_fuel.map(|f| f.round()),
        wear_from_data: laps.len() >= 6 && spread(&ages) >= 4.0,
        fuel_from_data: fuels.len() >= 6 && spread(&fuels) >= 10.0,
        fixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(setup: &str, age: u32, fuel: f32, ms: i32) -> PracticeLap {
        PracticeLap {
            setup: setup.into(),
            compound: "RS".into(),
            age,
            fuel_start: Some(fuel),
            ms,
            clean: true,
            counted: true,
            ..Default::default()
        }
    }

    /// Setup B looks slower on raw times because it was driven on old tyres with a full tank,
    /// but on equal terms it is 0.3 s faster.
    #[test]
    fn corrects_tyre_age_and_fuel() {
        // True model: 95.0 s (A) / 94.7 s (B) + 0.15 s per lap of age + 0.04 s per litre over 40 l
        let t = |base: f64, age: u32, fuel: f32| ((base + 0.15 * age as f64 + 0.04 * (fuel as f64 - 40.0)) * 1000.0) as i32;
        let mut laps = Vec::new();
        for (age, fuel) in [(0, 20.0), (1, 14.0), (2, 8.0), (0, 60.0), (1, 54.0), (2, 48.0)] {
            laps.push(lap("A", age, fuel, t(95.0, age, fuel)));
        }
        for (age, fuel) in [(5, 80.0), (6, 74.0), (7, 68.0), (8, 62.0), (3, 30.0), (4, 24.0)] {
            laps.push(lap("B", age, fuel, t(94.7, age, fuel)));
        }
        let refs: Vec<&PracticeLap> = laps.iter().collect();
        let f = fair(&refs, false);
        let a = f.rows.iter().find(|r| r.setup == "A").unwrap();
        let b = f.rows.iter().find(|r| r.setup == "B").unwrap();
        assert!(b.median_ms > a.median_ms, "raw: B looks slower");
        assert_eq!(b.gap_ms, 0, "{f:?}");
        assert!((a.gap_ms - 300).abs() < 100, "A about 0.3 s slower (the typical wear holds the fit a little): {f:?}");
        assert!(a.sure, "{f:?}");
        assert!((f.fuel_effect - 0.04).abs() < 0.01 && f.fuel_from_data, "{f:?}");
    }

    #[test]
    fn karts_and_drivers_warming_up() {
        // Kart: lap times fall with "age" (the driver warming up), nothing to correct
        let laps = [lap("A", 0, 5.0, 56_000), lap("A", 1, 5.0, 55_600), lap("A", 2, 5.0, 55_400), lap("B", 0, 5.0, 55_200)];
        let refs: Vec<&PracticeLap> = laps.iter().collect();
        let f = fair(&refs, true);
        let a = f.rows.iter().find(|r| r.setup == "A").unwrap();
        assert!((a.fair_ms - 55_667).abs() <= 2, "plain mean: {f:?}");
        assert!(f.wear.iter().all(|w| w.1 == 0.0) && f.fuel_effect == 0.0 && f.fixed);
        // Not a kart: a negative wear is not used either
        let f = fair(&refs, false);
        assert!(f.wear.iter().all(|w| w.1 >= 0.0), "{f:?}");
        // One setup per compound: nothing to compare
        let one = [lap("A", 0, 40.0, 95_000), lap("A", 1, 34.0, 95_200)];
        let refs: Vec<&PracticeLap> = one.iter().collect();
        assert!(fair(&refs, false).rows[0].alone);
    }

    #[test]
    fn few_laps_are_not_sure() {
        let laps = [lap("A", 0, 40.0, 95_000), lap("A", 1, 34.0, 95_400), lap("B", 0, 40.0, 94_900)];
        let refs: Vec<&PracticeLap> = laps.iter().collect();
        let f = fair(&refs, false);
        assert!(f.rows.iter().all(|r| !r.sure), "{f:?}");
        assert!(!f.fuel_from_data && !f.wear_from_data);
    }
}
