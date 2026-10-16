//! Practice mode: laps driven in practice are kept per car, track, compound and race multipliers.
//! From them the pace of every compound, the time lost per lap of tyre age and the fuel per lap
//! are computed; the race planner uses them for a race on the same track with the same car.
//!
//! The telemetry knows neither the track nor the compound: the driver marks the compound, and the
//! track is recognised by the shape of the driven lap (GT7 positions are fixed per layout) or
//! typed in once. The name stands for one layout ("Nürburgring GP", "Nürburgring Nordschleife"):
//! a lap whose shape does not fit the shape stored for the name is not used, so two layouts are
//! never mixed. Laps driven by the AI ("automat") can be left out with `counting = false`.
//!
//! Saved in `practice.json` next to `strategy.json`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Shape points: every n-th point of the map (2 m apart → ~20 m)
const SHAPE_STEP: usize = 10;
/// Same layout when the points are on average this close (m)
const SHAPE_MATCH_M: f32 = 12.0;
/// A clean lap slower than the typical one by more than this (s or share) was a mistake
const OUTLIER_S: f32 = 2.0;
const OUTLIER_SHARE: f32 = 0.02;
/// Degradation is fitted from at least this many laps spanning this many laps of tyre age
const FIT_LAPS: usize = 4;
const FIT_SPAN: u32 = 3;
const MAX_LAPS: usize = 3000;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct PracticeLap {
    /// Local time when the lap ended, "2026-10-14 19:30"
    pub at: String,
    pub car: i32,
    /// Id of the car setup it was driven with ("" = none chosen)
    pub setup: String,
    pub track: String,
    pub compound: String,
    /// Timed laps on this set before this lap
    pub age: u32,
    pub ms: i32,
    /// Fuel burnt
    pub fuel: f32,
    pub wear_mult: f32,
    pub fuel_mult: f32,
    /// Full lap from the line, no pit stop, not the lap out of the pit
    pub clean: bool,
    /// Counted for the figures (false: AI driving, or excluded by the driver)
    pub counted: bool,
    /// The lap was driven on another layout than the track name says
    pub wrong_layout: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Shape {
    /// Track and layout, e.g. "Nürburgring GP"
    pub track: String,
    pub pts: Vec<[f32; 2]>,
    #[serde(default)]
    pub length_m: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct PracticeSaved {
    /// Practice mode on (otherwise laps go to the race strategy)
    pub active: bool,
    pub track: String,
    pub compound: Option<String>,
    /// Timed laps on the current set
    pub set_age: u32,
    pub counting: bool,
    pub wear_mult: f32,
    pub fuel_mult: f32,
    pub laps: Vec<PracticeLap>,
    pub shapes: Vec<Shape>,
    /// Laps a set lasts, entered by the driver; key "car|track|compound|wear multiplier"
    pub life: BTreeMap<String, u32>,
}

impl Default for PracticeSaved {
    fn default() -> Self {
        PracticeSaved {
            active: false,
            track: String::new(),
            compound: None,
            set_age: 0,
            counting: true,
            wear_mult: 1.0,
            fuel_mult: 1.0,
            laps: vec![],
            shapes: vec![],
            life: BTreeMap::new(),
        }
    }
}

/// Figures of one compound.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct CompoundStats {
    pub compound: String,
    /// Clean, counted laps used
    pub laps: usize,
    pub best_ms: Option<i32>,
    /// Median lap time, ms
    pub typical_ms: Option<i32>,
    /// Lap time on new tyres (fitted), s
    pub base: Option<f32>,
    /// Time lost per lap of tyre age, s (None: not enough laps yet)
    pub deg: Option<f32>,
    /// Slower than the fastest compound, s
    pub pace: Option<f32>,
    pub fuel_per_lap: Option<f32>,
    /// Longest run on one set, laps
    pub max_age: u32,
    /// Entered by the driver
    pub life: Option<u32>,
}

/// Practice figures for a car on a track with given multipliers.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub car: i32,
    pub track: String,
    pub wear_mult: f32,
    pub fuel_mult: f32,
    pub compounds: Vec<CompoundStats>,
    pub fuel_per_lap: Option<f32>,
}

/// Same name, ignoring case, accents and punctuation ("Nürburgring GP" = "nurburgring-gp").
pub fn track_key(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'ě' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'ů' => 'u',
            'č' | 'ç' => 'c',
            'ř' => 'r',
            'š' => 's',
            'ž' => 'z',
            'ý' => 'y',
            'ň' | 'ñ' => 'n',
            'ť' => 't',
            'ď' => 'd',
            c => c,
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn same_mult(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn median(v: &mut [f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// Least squares t = base + deg·age.
fn fit(points: &[(f32, f32)]) -> Option<(f32, f32)> {
    let n = points.len() as f32;
    let (sx, sy) = points.iter().fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y));
    let (mx, my) = (sx / n, sy / n);
    let sxx: f32 = points.iter().map(|(x, _)| (x - mx).powi(2)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let sxy: f32 = points.iter().map(|(x, y)| (x - mx) * (y - my)).sum();
    let deg = sxy / sxx;
    Some((my - deg * mx, deg))
}

pub struct Practice {
    pub saved: PracticeSaved,
    file: Option<PathBuf>,
    /// The lap running now had a pit stop / the previous one ended in the pit
    pit_this_lap: bool,
    pit_last_lap: bool,
    /// The lap just finished fits the layout of the track name (set by [`Practice::shape`])
    layout_ok: bool,
    /// Why the last lap did not fit the track name
    pub layout_warning: Option<String>,
}

impl Practice {
    pub fn load(file: Option<PathBuf>) -> Self {
        let saved =
            file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Practice { saved, file, pit_this_lap: false, pit_last_lap: false, layout_ok: true, layout_warning: None }
    }

    pub fn save(&self) {
        let Some(file) = &self.file else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&self.saved).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, file);
        }
    }

    /// A pit stop (standing in the pit, refuelling) during the running lap.
    pub fn pit(&mut self) {
        self.pit_this_lap = true;
    }

    /// New tyres were fitted (detected after a stop, or pressed by the driver).
    pub fn new_set(&mut self, compound: Option<&str>) {
        if let Some(c) = compound {
            self.saved.compound = Some(c.to_uppercase());
        }
        self.saved.set_age = 0;
        self.save();
    }

    /// A timed lap ended. `closed`: it ended where it started (not the short first race lap).
    pub fn lap_done(&mut self, car: i32, setup: &str, used: f32, refuel: bool, ms: Option<i32>, closed: bool) {
        let pit = self.pit_this_lap || refuel;
        let out_lap = self.pit_last_lap;
        let wrong_layout = !std::mem::replace(&mut self.layout_ok, true);
        self.pit_last_lap = pit;
        self.pit_this_lap = false;
        let (Some(ms), Some(compound)) = (ms.filter(|m| *m > 0), self.saved.compound.clone()) else { return };
        if self.saved.track.is_empty() {
            return;
        }
        // Another car or track than the last lap: new tyres
        if self.saved.laps.last().is_some_and(|l| l.car != car || track_key(&l.track) != track_key(&self.saved.track)) {
            self.saved.set_age = 0;
        }
        let s = &mut self.saved;
        s.laps.push(PracticeLap {
            at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            car,
            setup: setup.to_string(),
            track: s.track.clone(),
            compound,
            age: s.set_age,
            ms,
            fuel: used.max(0.0),
            wear_mult: s.wear_mult,
            fuel_mult: s.fuel_mult,
            clean: closed && !pit && !out_lap && !wrong_layout,
            counted: s.counting,
            wrong_layout,
        });
        if s.laps.len() > MAX_LAPS {
            s.laps.remove(0);
        }
        s.set_age += 1;
        self.save();
    }

    /// Shape of the lap that just ended (called before [`Practice::lap_done`]): recognise the
    /// track when none is set, remember the shape of a new track name, and check that the lap
    /// fits the layout stored for the name.
    pub fn shape(&mut self, lap: &[(f32, f32)]) {
        if lap.len() < 50 {
            return;
        }
        let pts: Vec<[f32; 2]> = lap.iter().step_by(SHAPE_STEP).map(|(x, z)| [*x, *z]).collect();
        let length_m: f32 = lap.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
        let matching = self.saved.shapes.iter().find(|s| shape_distance(&pts, &s.pts) < SHAPE_MATCH_M).map(|s| s.track.clone());
        let name = self.saved.track.clone();
        if name.is_empty() {
            if let Some(t) = matching {
                self.saved.track = t;
                self.layout_warning = None;
                self.save();
            }
            return;
        }
        let key = track_key(&name);
        let stored = self.saved.shapes.iter().find(|s| track_key(&s.track) == key);
        let fits = stored.map(|s| shape_distance(&pts, &s.pts) < SHAPE_MATCH_M);
        // A shape stored by an older version has no length yet: take it from this lap
        if fits == Some(true) {
            if let Some(s) = self.saved.shapes.iter_mut().find(|s| track_key(&s.track) == key && s.length_m <= 0.0) {
                s.length_m = length_m;
                self.save();
            }
        }
        self.layout_warning = match (fits, matching) {
            (Some(true), _) => None,
            (Some(false), Some(other)) => {
                Some(format!("Kolo odpovídá trati „{other}“, ne „{name}“. Oprav název trati – kolo se nepočítá."))
            }
            (Some(false), None) => Some(format!(
                "Kolo má jiný tvar než uložená trať „{name}“ – jiná varianta? Pojmenuj ji jinak (např. „{name} – varianta 2“). Kolo se nepočítá."
            )),
            (None, Some(other)) => {
                Some(format!("Kolo odpovídá už uložené trati „{other}“. Vyber ji, ať se data nerozdělí. Kolo se nepočítá."))
            }
            (None, None) => {
                self.saved.shapes.push(Shape { track: name, pts, length_m });
                self.save();
                None
            }
        };
        self.layout_ok = self.layout_warning.is_none();
    }

    /// Length (m) of the layout stored for the track name.
    pub fn track_length(&self, track: &str) -> Option<f32> {
        let key = track_key(track);
        self.saved.shapes.iter().find(|s| track_key(&s.track) == key).map(|s| s.length_m).filter(|l| *l > 0.0)
    }

    pub fn set_life(&mut self, car: i32, track: &str, compound: &str, wear_mult: f32, life: Option<u32>) {
        let key = life_key(car, track, compound, wear_mult);
        match life.filter(|l| *l > 0) {
            Some(l) => self.saved.life.insert(key, l),
            None => self.saved.life.remove(&key),
        };
        self.save();
    }

    /// Leave a lap out of the figures (or take it back).
    pub fn toggle_lap(&mut self, index: usize) {
        if let Some(l) = self.saved.laps.get_mut(index) {
            l.counted = !l.counted;
            self.save();
        }
    }

    /// The car with the most practice laps on the track (when the race car is not known yet).
    pub fn main_car(&self, track: &str) -> Option<i32> {
        let key = track_key(track);
        let mut count: BTreeMap<i32, usize> = BTreeMap::new();
        for l in self.saved.laps.iter().filter(|l| track_key(&l.track) == key && l.counted && l.clean) {
            *count.entry(l.car).or_default() += 1;
        }
        count.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c)
    }

    /// Figures for a car on a track at the given multipliers.
    pub fn summary(&self, car: i32, setup: &str, track: &str, wear_mult: f32, fuel_mult: f32) -> Summary {
        let key = track_key(track);
        let laps: Vec<&PracticeLap> = self
            .saved
            .laps
            .iter()
            .filter(|l| {
                l.car == car
                    && l.setup == setup
                    && track_key(&l.track) == key
                    && same_mult(l.wear_mult, wear_mult)
                    && same_mult(l.fuel_mult, fuel_mult)
                    && l.clean
                    && l.counted
            })
            .collect();
        let mut names: Vec<String> = laps.iter().map(|l| l.compound.clone()).collect();
        names.sort();
        names.dedup();
        let mut compounds: Vec<CompoundStats> = names
            .iter()
            .map(|c| {
                let all: Vec<&&PracticeLap> = laps.iter().filter(|l| &l.compound == c).collect();
                let mut times: Vec<f32> = all.iter().map(|l| l.ms as f32 / 1000.0).collect();
                let typical = median(&mut times).unwrap_or(0.0);
                // Mistakes (spin, off, traffic) out
                let ok: Vec<&&&PracticeLap> =
                    all.iter().filter(|l| (l.ms as f32 / 1000.0) <= typical + OUTLIER_S.max(typical * OUTLIER_SHARE)).collect();
                let points: Vec<(f32, f32)> = ok.iter().map(|l| (l.age as f32, l.ms as f32 / 1000.0)).collect();
                let (min_age, max_age) = ok.iter().fold((u32::MAX, 0), |(lo, hi), l| (lo.min(l.age), hi.max(l.age)));
                let fitted = (points.len() >= FIT_LAPS && max_age.saturating_sub(min_age) >= FIT_SPAN).then(|| fit(&points)).flatten();
                let mean_age = points.iter().map(|p| p.0).sum::<f32>() / points.len().max(1) as f32;
                let (base, deg) = match fitted {
                    Some((b, d)) if d >= 0.0 => (Some(b), Some(d)),
                    // Getting faster on older tyres (warming up, learning the track): no wear seen
                    Some(_) => (Some(typical), Some(0.0)),
                    // Too few laps for a slope: typical time back to new tyres at the default rate
                    None if !points.is_empty() => (Some(typical - super::DEFAULT_DEG * mean_age), None),
                    None => (None, None),
                };
                let mut fuel: Vec<f32> = ok.iter().map(|l| l.fuel).filter(|f| *f > 0.0).collect();
                CompoundStats {
                    compound: c.clone(),
                    laps: ok.len(),
                    best_ms: ok.iter().map(|l| l.ms).min(),
                    typical_ms: (!ok.is_empty()).then_some((typical * 1000.0).round() as i32),
                    base,
                    deg,
                    pace: None,
                    fuel_per_lap: median(&mut fuel),
                    max_age: if ok.is_empty() { 0 } else { max_age + 1 },
                    life: self.saved.life.get(&life_key(car, track, c, wear_mult)).copied(),
                }
            })
            .collect();
        let fastest = compounds.iter().filter_map(|c| c.base).fold(f32::MAX, f32::min);
        for c in &mut compounds {
            c.pace = c.base.map(|b| b - fastest);
        }
        // Compounds with only a life entered (not driven yet) still show up
        let prefix = format!("{car}|{key}|");
        for (k, life) in &self.saved.life {
            if let Some(rest) = k.strip_prefix(&prefix) {
                let mut parts = rest.split('|');
                let (Some(c), Some(w)) = (parts.next(), parts.next().and_then(|w| w.parse::<f32>().ok())) else { continue };
                if same_mult(w, wear_mult) && !compounds.iter().any(|x| x.compound == c) {
                    compounds.push(CompoundStats { compound: c.to_string(), life: Some(*life), ..Default::default() });
                }
            }
        }
        compounds.sort_by_key(|c| order(&c.compound));
        let mut fuel: Vec<f32> = laps.iter().map(|l| l.fuel).filter(|f| *f > 0.0).collect();
        Summary { car, track: track.to_string(), wear_mult, fuel_mult, compounds, fuel_per_lap: median(&mut fuel) }
    }
}

fn life_key(car: i32, track: &str, compound: &str, wear_mult: f32) -> String {
    format!("{car}|{}|{}|{}", track_key(track), compound.to_uppercase(), wear_mult)
}

/// Racing before sport before comfort, soft before hard.
fn order(c: &str) -> (usize, usize) {
    let mut ch = c.chars();
    let kind = ch.next().map_or(9, |k| "RSC".find(k).unwrap_or(9));
    let hard = ch.next().map_or(9, |h| "SMH".find(h).unwrap_or(9));
    (kind, hard)
}

/// Average distance from the points of `a` to the nearest point of `b` (m).
fn shape_distance(a: &[[f32; 2]], b: &[[f32; 2]]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return f32::MAX;
    }
    let step = (a.len() / 60).max(1);
    let sample: Vec<&[f32; 2]> = a.iter().step_by(step).collect();
    let total: f32 =
        sample.iter().map(|p| b.iter().map(|q| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()).fold(f32::MAX, f32::min)).sum();
    total / sample.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn practice() -> Practice {
        let mut p = Practice::load(None);
        p.saved.active = true;
        p.saved.track = "Nürburgring GP".into();
        p.saved.wear_mult = 6.0;
        p.saved.fuel_mult = 3.0;
        p
    }

    /// `n` laps on a new set: base + deg·age, `fuel` per lap
    fn stint(p: &mut Practice, compound: &str, n: u32, base: f32, deg: f32, fuel: f32) {
        p.new_set(Some(compound));
        for age in 0..n {
            p.lap_done(3475, "", fuel, false, Some(((base + deg * age as f32) * 1000.0) as i32), true);
        }
    }

    #[test]
    fn pace_degradation_and_fuel_per_compound() {
        let mut p = practice();
        stint(&mut p, "SS", 8, 160.0, 0.30, 6.9);
        stint(&mut p, "SM", 10, 161.0, 0.12, 6.9);
        // A spin: way off, ignored
        p.lap_done(3475, "", 6.9, false, Some(175_000), true);
        let s = p.summary(3475, "", "nurburgring gp", 6.0, 3.0);
        let ss = s.compounds.iter().find(|c| c.compound == "SS").unwrap();
        let sm = s.compounds.iter().find(|c| c.compound == "SM").unwrap();
        assert_eq!(s.compounds[0].compound, "SS"); // soft first
        assert!((ss.deg.unwrap() - 0.30).abs() < 0.01, "{ss:?}");
        assert!((sm.deg.unwrap() - 0.12).abs() < 0.01, "{sm:?}");
        assert!(ss.pace.unwrap().abs() < 0.01 && (sm.pace.unwrap() - 1.0).abs() < 0.02, "{s:?}");
        assert!((s.fuel_per_lap.unwrap() - 6.9).abs() < 1e-4);
        assert_eq!(sm.laps, 10);
        // Other multipliers: nothing
        assert!(p.summary(3475, "", "Nürburgring GP", 1.0, 1.0).compounds.is_empty());
    }

    #[test]
    fn pit_and_out_laps_are_not_clean_and_ai_laps_not_counted() {
        let mut p = practice();
        p.new_set(Some("RM"));
        p.lap_done(1, "", 7.0, false, Some(100_000), false); // short first lap
        p.pit();
        p.lap_done(1, "", 5.0, false, Some(130_000), true); // in-lap
        p.lap_done(1, "", 7.0, false, Some(110_000), true); // out-lap
        p.lap_done(1, "", 7.0, false, Some(101_000), true); // clean
        p.saved.counting = false;
        p.lap_done(1, "", 7.0, false, Some(99_000), true); // automat
        let clean: Vec<bool> = p.saved.laps.iter().map(|l| l.clean && l.counted).collect();
        assert_eq!(clean, [false, false, false, true, false]);
        // Another car: the set starts from new
        p.saved.counting = true;
        p.lap_done(2, "", 7.0, false, Some(99_000), true);
        assert_eq!(p.saved.laps.last().unwrap().age, 0);
        let s = p.summary(1, "", "Nürburgring GP", 6.0, 3.0);
        assert_eq!(s.compounds[0].laps, 1);
        assert_eq!(s.compounds[0].deg, None); // not enough laps
    }

    #[test]
    fn track_is_recognised_by_its_shape() {
        let circle = |r: f32, dx: f32| -> Vec<(f32, f32)> {
            (0..1000).map(|i| i as f32 / 1000.0 * std::f32::consts::TAU).map(|a| (r * a.cos() + dx, r * a.sin())).collect()
        };
        let mut p = practice();
        p.shape(&circle(500.0, 0.0));
        assert_eq!(p.saved.shapes.len(), 1);
        assert!((p.track_length("nurburgring gp").unwrap() - 3141.0).abs() < 5.0);
        // A shape without a length (older version) gets one from the next lap that fits
        p.saved.shapes[0].length_m = 0.0;
        assert_eq!(p.track_length("Nürburgring GP"), None);
        p.shape(&circle(500.0, 1.0));
        assert!(p.track_length("Nürburgring GP").is_some());
        p.saved.track.clear();
        p.shape(&circle(500.0, 3.0)); // same layout, slightly different line
        assert_eq!(p.saved.track, "Nürburgring GP");
        p.saved.track.clear();
        p.shape(&circle(800.0, 0.0)); // another track: not recognised, nothing stored without a name
        assert_eq!((p.saved.track.as_str(), p.saved.shapes.len()), ("", 1));
    }

    #[test]
    fn life_is_kept_per_car_track_compound_and_wear() {
        let mut p = practice();
        p.set_life(7, "Nürburgring GP", "ss", 6.0, Some(9));
        let s = p.summary(7, "", "NURBURGRING GP", 6.0, 3.0);
        assert_eq!(s.compounds.len(), 1);
        assert_eq!((s.compounds[0].compound.as_str(), s.compounds[0].life), ("SS", Some(9)));
        assert!(p.summary(7, "", "Nürburgring GP", 1.0, 3.0).compounds.is_empty());
    }

    #[test]
    fn another_layout_under_the_same_name_is_not_mixed() {
        let circle = |r: f32| -> Vec<(f32, f32)> {
            (0..1000).map(|i| i as f32 / 1000.0 * std::f32::consts::TAU).map(|a| (r * a.cos(), r * a.sin())).collect()
        };
        let mut p = practice();
        p.new_set(Some("SS"));
        p.shape(&circle(500.0)); // GP layout, stored
        p.lap_done(1, "", 7.0, false, Some(100_000), true);
        p.shape(&circle(900.0)); // other layout, same name
        p.lap_done(1, "", 7.0, false, Some(200_000), true);
        assert!(p.layout_warning.as_deref().unwrap().contains("jiný tvar"));
        let flags: Vec<(bool, bool)> = p.saved.laps.iter().map(|l| (l.clean, l.wrong_layout)).collect();
        assert_eq!(flags, [(true, false), (false, true)]);
        // Named as its own layout: stored separately, laps count again
        p.saved.track = "Nürburgring Nordschleife".into();
        p.shape(&circle(900.0));
        p.lap_done(1, "", 7.0, false, Some(200_000), true);
        assert_eq!(p.saved.shapes.len(), 2);
        assert!(p.layout_warning.is_none() && p.saved.laps[2].clean);
        // Back on GP but still named Nordschleife: the lap belongs to the other name
        p.shape(&circle(500.0));
        assert!(p.layout_warning.as_deref().unwrap().contains("„Nürburgring GP“"));
    }
}
