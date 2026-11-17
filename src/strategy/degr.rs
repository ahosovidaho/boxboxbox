//! Tyre degradation from lap times, with the weight of the fuel taken out.
//!
//! A lap gets slower as the tyres age and faster as the tank empties. In a run from a full tank
//! the two cancel out almost exactly (Yas Marina, F3500: +0.64 s per lap of tyre age, −0.25 s per
//! lap of fuel burnt), so a plain line through lap time over tyre age says the tyres do not wear.
//!
//! - A race has stints starting with different fuel loads, so both effects can be told apart:
//!   [`race_fit`] fits lap time = base + deg × age + fuel_s × fuel.
//! - Practice runs mostly start full, age and fuel move together: [`run_fit`] takes the fuel
//!   effect as known (from a race, else [`DEFAULT_FUEL_S`]) and fits the slope within each run,
//!   so getting faster from one session to the next does not count as anything.

/// Lap time per litre of fuel on board, s, until a race measures it
pub const DEFAULT_FUEL_S: f32 = 0.04;
/// A lap this much slower than the median of its run is a mistake (spin, off, traffic)
const OUTLIER_S: f32 = 3.0;
/// Laps needed for a fit, and how far tyre age has to spread
const MIN_LAPS: usize = 5;
const MIN_SPAN: f32 = 3.0;
/// The race fit needs stints starting with different fuel (litres) to tell fuel from tyres
const MIN_FUEL_SPREAD: f32 = 15.0;

/// One lap: run (tyre set) it belongs to, age of the tyres at its start (laps), fuel at its start
/// (litres), lap time (s)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obs {
    pub run: usize,
    pub age: f32,
    pub fuel: f32,
    pub secs: f32,
}

/// Mistakes out: laps more than [`OUTLIER_S`] slower than the median of their run.
fn clean(obs: &[Obs]) -> Vec<Obs> {
    let mut out = Vec::new();
    let mut runs: Vec<usize> = obs.iter().map(|o| o.run).collect();
    runs.sort_unstable();
    runs.dedup();
    for r in runs {
        let mut t: Vec<f32> = obs.iter().filter(|o| o.run == r).map(|o| o.secs).collect();
        t.sort_by(f32::total_cmp);
        let med = t[t.len() / 2];
        out.extend(obs.iter().filter(|o| o.run == r && o.secs <= med + OUTLIER_S));
    }
    out
}

fn span(obs: &[Obs]) -> f32 {
    let (lo, hi) = obs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), o| (lo.min(o.age), hi.max(o.age)));
    hi - lo
}

/// Race: lap time = base + deg × age + fuel_s × fuel, one base for the whole race. Returns
/// (deg, fuel_s), or None when the stints do not tell fuel and tyres apart.
pub fn race_fit(obs: &[Obs]) -> Option<(f32, f32)> {
    let obs = clean(obs);
    if obs.len() < MIN_LAPS * 2 || span(&obs) < MIN_SPAN {
        return None;
    }
    // Fuel at the same tyre age has to differ between stints, or fuel and age are one variable
    let start_fuel = |r: usize| obs.iter().filter(|o| o.run == r).min_by(|a, b| a.age.total_cmp(&b.age)).map(|o| o.fuel);
    let mut starts: Vec<f32> =
        obs.iter().map(|o| o.run).collect::<std::collections::BTreeSet<_>>().into_iter().filter_map(start_fuel).collect();
    starts.sort_by(f32::total_cmp);
    if starts.len() < 2 || starts[starts.len() - 1] - starts[0] < MIN_FUEL_SPREAD {
        return None;
    }
    // Normal equations for [1, age, fuel]
    let mut a = [[0.0f64; 3]; 3];
    let mut b = [0.0f64; 3];
    for o in &obs {
        let x = [1.0, o.age as f64, o.fuel as f64];
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += x[i] * x[j];
            }
            b[i] += x[i] * o.secs as f64;
        }
    }
    let s = solve3(a, b)?;
    let (deg, fuel_s) = (s[1] as f32, s[2] as f32);
    // Out of what physics allows: the data did not separate them after all
    ((0.0..=3.0).contains(&deg) && (0.0..=0.15).contains(&fuel_s)).then_some((deg, fuel_s))
}

/// Practice: the slope of lap time over tyre age within each run, with the fuel effect taken
/// out. Each run has its own base (sessions on different days, the driver improving).
pub fn run_fit(obs: &[Obs], fuel_s: f32) -> Option<f32> {
    let obs = clean(obs);
    // Only runs with at least three laps say anything about a slope
    let runs: Vec<usize> = {
        let mut r: Vec<usize> = obs.iter().map(|o| o.run).collect();
        r.sort_unstable();
        r.dedup();
        r.into_iter().filter(|r| obs.iter().filter(|o| o.run == *r).count() >= 3).collect()
    };
    let obs: Vec<Obs> = obs.into_iter().filter(|o| runs.contains(&o.run)).collect();
    if obs.len() < MIN_LAPS || runs.iter().all(|r| span(&obs.iter().filter(|o| o.run == *r).copied().collect::<Vec<_>>()) < MIN_SPAN - 1.0)
    {
        return None;
    }
    // Within-run deviations from the run means
    let (mut sxy, mut sxx) = (0.0f64, 0.0f64);
    for r in &runs {
        let mine: Vec<&Obs> = obs.iter().filter(|o| o.run == *r).collect();
        let n = mine.len() as f64;
        let y = |o: &Obs| (o.secs - fuel_s * o.fuel) as f64;
        let (mx, my) = (mine.iter().map(|o| o.age as f64).sum::<f64>() / n, mine.iter().map(|o| y(o)).sum::<f64>() / n);
        for o in mine {
            sxx += (o.age as f64 - mx).powi(2);
            sxy += (o.age as f64 - mx) * (y(o) - my);
        }
    }
    (sxx > 0.0).then(|| ((sxy / sxx) as f32).max(0.0))
}

fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for c in 0..3 {
        let p = (c..3).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-9 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in 0..3 {
            if r != c {
                let f = a[r][c] / a[c][c];
                let pivot = a[c];
                for (x, p) in a[r].iter_mut().zip(pivot) {
                    *x -= f * p;
                }
                b[r] -= f * b[c];
            }
        }
    }
    Some([b[0] / a[0][0], b[1] / a[1][1], b[2] / a[2][2]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The race at Yas Marina on 17 Nov (F3500-A, RS, wear ×7): lap, run, age, fuel, time
    fn yas() -> Vec<Obs> {
        let laps: [(usize, f32, f32, f32); 24] = [
            (0, 1., 92.5, 90.765),
            (0, 2., 84.5, 90.937),
            (0, 3., 76.7, 92.302),
            (0, 4., 69.4, 91.954),
            (0, 5., 61.9, 92.709),
            (0, 6., 54.6, 95.267),
            (0, 7., 47.3, 94.477),
            (1, 0., 75.5, 90.222),
            (1, 1., 67.8, 91.2),
            (1, 2., 60.2, 90.902),
            (1, 3., 52.6, 92.146),
            (1, 4., 45.0, 91.401),
            (1, 6., 30.4, 91.744),
            (1, 7., 23.1, 92.497),
            (1, 8., 15.7, 92.975),
            (2, 0., 49.7, 90.11),
            (2, 1., 42.3, 89.935),
            (2, 2., 34.8, 90.687),
            (2, 3., 27.3, 90.472),
            (2, 4., 19.8, 92.024),
            (3, 0., 33.5, 88.516),
            (3, 1., 25.9, 89.342),
            (3, 2., 18.3, 89.572),
            (3, 3., 10.8, 89.123),
        ];
        laps.iter().map(|&(run, age, fuel, secs)| Obs { run, age, fuel, secs }).collect()
    }

    #[test]
    fn race_separates_fuel_from_tyres() {
        let (deg, k) = race_fit(&yas()).unwrap();
        assert!((deg - 0.64).abs() < 0.05, "{deg}");
        assert!((k - 0.0325).abs() < 0.005, "{k}");
    }

    #[test]
    fn plain_slope_hides_the_wear_fuel_corrected_does_not() {
        // A run from a full tank: tyres +0.6 s per lap, fuel −0.3 s per lap → flat lap times
        let obs: Vec<Obs> = (0..8)
            .map(|a| Obs { run: 0, age: a as f32, fuel: 100.0 - 7.5 * a as f32, secs: 90.0 + 0.6 * a as f32 - 0.04 * 7.5 * a as f32 })
            .collect();
        assert!(run_fit(&obs, 0.0).unwrap() < 0.35);
        assert!((run_fit(&obs, 0.04).unwrap() - 0.6).abs() < 1e-3);
        // One stint in a race cannot tell them apart
        assert_eq!(race_fit(&obs), None);
    }

    #[test]
    fn runs_on_different_days_do_not_count_as_wear() {
        // Second run 2 s faster (driver improved), the same slope of 0.3 within each run
        let mut obs = Vec::new();
        for (run, base) in [(0, 95.0), (1, 93.0)] {
            for a in 0..5 {
                obs.push(Obs { run, age: (a + run * 5) as f32, fuel: 50.0, secs: base + 0.3 * a as f32 });
            }
        }
        assert!((run_fit(&obs, 0.04).unwrap() - 0.3).abs() < 1e-3);
    }

    #[test]
    fn mistakes_are_left_out() {
        let mut obs = yas();
        obs.push(Obs { run: 1, age: 6., fuel: 37.5, secs: 100.506 });
        obs.push(Obs { run: 2, age: 6., fuel: 12.2, secs: 101.321 });
        let (deg, _) = race_fit(&obs).unwrap();
        assert!((deg - 0.64).abs() < 0.05, "{deg}");
    }
}
