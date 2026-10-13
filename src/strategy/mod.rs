//! Race strategy: fuel to the finish, pit window, mandatory pit stop and compound, tyre wear.
//!
//! The race settings come from the dashboard (picked from the championship calendar or typed in).
//! The tracker reports finished laps (fuel burnt) and refuelling; the driver reports the compound
//! and, now and then, the tyre wear shown in GT7 (the telemetry has no wear). From that, [`Plan`]
//! is computed for every snapshot.
//!
//! Lap arithmetic: `driven` is the race distance covered in laps (lap 3 half done = 2.5). A pit
//! stop "at lap L" means entering the pit at the end of lap L.
//!
//! Saved in `strategy.json` in the data folder, so a restart keeps the race and the calibration.

pub mod planner;
pub mod practice;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type Shared = Arc<Mutex<Strategy>>;

/// Fuel left over at the finish, in laps
const DEFAULT_RESERVE: f32 = 0.3;
/// Tyre wear (%) at which the tyres count as finished
const DEFAULT_WEAR_LIMIT: f32 = 70.0;
/// Fuel per lap: average of this many recent clean laps
const FUEL_LAPS: usize = 5;
/// Arrive at the pit with at least this much fuel left (laps)
const PIT_MARGIN: f32 = 0.15;
/// Tyre degradation when nothing is known: s per lap per lap of age (Barcelona 2026-10: 0.16–0.31)
pub const DEFAULT_DEG: f32 = 0.2;
/// Pit loss when the track has not been measured yet, s (Barcelona: ~25 + refuelling)
pub const DEFAULT_PIT_LOSS: f32 = 25.0;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Race {
    /// Firestore document id; empty = typed in by hand
    pub id: String,
    pub name: String,
    pub track: String,
    pub laps: Option<u32>,
    pub tyre_wear: Option<f32>,
    pub fuel_mult: Option<f32>,
    /// Refuelling speed (fuel units per second)
    pub refuel_rate: Option<f32>,
    pub pit_required: bool,
    /// Compound that has to be used once, e.g. "RS"
    pub mandatory: Option<String>,
    /// Tyres at the start; after a tyre change the same compound is assumed
    pub start_compound: Option<String>,
    /// Fuel per lap to plan with before a lap has been measured
    pub fuel_per_lap: Option<f32>,
    pub reserve_laps: Option<f32>,
    pub wear_limit: Option<f32>,
    /// Per compound ("RS", "RM" …): laps a set lasts and pace difference
    pub tyres: BTreeMap<String, TyreSpec>,
    /// Lap time lost per lap of tyre age, s (default [`DEFAULT_DEG`])
    pub deg: Option<f32>,
    /// Time lost by a stop without refuelling, s (default: measured on this track, else
    /// [`DEFAULT_PIT_LOSS`])
    pub pit_loss: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct TyreSpec {
    /// Laps a new set lasts
    pub life: Option<u32>,
    /// Lap time compared with the others, s (negative = faster)
    pub pace: Option<f32>,
    /// Time lost per lap of tyre age, s (default: [`Race::deg`])
    pub deg: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct WearMark {
    /// Race distance (laps) when the wear was read
    pub at: f32,
    pub pct: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Stint {
    /// Race distance (laps) when these tyres went on
    pub start: f32,
    pub compound: Option<String>,
    pub wear: Vec<WearMark>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Stop {
    /// Lap during which the car was in the pit
    pub lap: i16,
    pub fuel_added: f32,
    /// New tyres? `None` = not answered yet
    pub tyres: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FuelLap {
    pub lap: i16,
    /// Fuel burnt (refuelling not counted)
    pub used: f32,
    pub refuel: bool,
    /// Lap time from GT7, ms
    #[serde(default)]
    pub ms: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Saved {
    pub race: Option<Race>,
    pub stints: Vec<Stint>,
    pub stops: Vec<Stop>,
    pub fuel_laps: Vec<FuelLap>,
    /// Tyre wear per lap (%), key "car|track|compound|multiplier"; pit loss (s), key "pit|track"
    pub calibration: BTreeMap<String, f32>,
}

/// What the tracker knows right now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Live {
    pub lap: i16,
    /// Laps of the race according to GT7 (0 = not a race / unknown)
    pub total_laps: i16,
    /// Progress of the current lap 0..1
    pub frac: f32,
    pub fuel: f32,
    pub capacity: f32,
    pub car_code: i32,
    /// The car stands still (in the pit box, on the grid …)
    pub standing: bool,
}

impl Live {
    pub fn driven(&self) -> f32 {
        (self.lap.max(1) - 1) as f32 + self.frac.clamp(0.0, 0.99)
    }
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub race: Option<String>,
    pub total_laps: Option<u32>,
    pub laps_left: Option<f32>,
    pub fuel_per_lap: Option<f32>,
    /// "measured", "manual" or "so_far" (estimated from the fuel burnt since the start)
    pub fuel_source: &'static str,
    /// Fuel needed to the finish, reserve included
    pub fuel_needed: Option<f32>,
    /// Laps the fuel on board reaches beyond the finish (negative = short)
    pub fuel_margin_laps: Option<f32>,
    pub stops_needed: u32,
    /// Two stops needed now, but one would do at this fuel per lap (saving fuel)
    pub one_stop_fpl: Option<f32>,
    /// Next stop: enter the pit at the end of one of these laps
    pub window: Option<[i16; 2]>,
    /// Next stop of the recommended strategy (box at the end of this lap)
    pub target_lap: Option<i16>,
    /// Laps until that stop (0 = box at the end of this lap)
    pub laps_to_box: Option<i16>,
    /// Fuel to add at the next planned stop
    pub next_stop_fuel: Option<f32>,
    /// In the pit box right now: fuel to add, and the laps it is meant for
    pub stop_add: Option<f32>,
    pub stop_add_laps: Option<u32>,
    /// Recommended and alternative strategy for the rest of the race
    pub options: planner::Options,
    pub pit_loss: f32,
    /// "manual", "measured" or "default"
    pub pit_loss_source: &'static str,
    /// Practice figures the plan uses (car, laps per compound), and the practice mode state
    pub practice_use: Option<PracticeUse>,
    pub practice: PracticeState,
    /// Refuel amount when boxing at the end of this lap
    pub add_now: Option<f32>,
    pub refuel_s: Option<f32>,
    pub reserve_laps: f32,
    pub pit_required: bool,
    pub pit_done: bool,
    pub mandatory: Option<String>,
    pub mandatory_done: bool,
    pub compound: Option<String>,
    pub stint_laps: f32,
    pub wear_now: Option<f32>,
    pub wear_per_lap: Option<f32>,
    /// "stint" (read this stint) or "calibration" (earlier runs)
    pub wear_source: &'static str,
    pub wear_limit: f32,
    pub wear_laps_left: Option<f32>,
    pub wear_at_finish: Option<f32>,
    pub tyre_question: bool,
    /// Advice for the driver, most important first
    pub calls: Vec<Call>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Call {
    /// "box", "warn" or "info"
    pub level: &'static str,
    pub text: String,
}

/// Which practice figures went into the plan.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct PracticeUse {
    pub car: i32,
    pub compounds: Vec<PracticeCompound>,
    pub fuel_per_lap: Option<f32>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct PracticeCompound {
    pub compound: String,
    /// Clean practice laps
    pub laps: usize,
    pub life: Option<u32>,
    pub pace: Option<f32>,
    pub deg: Option<f32>,
}

/// Practice mode as shown on the dashboard.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct PracticeState {
    pub active: bool,
    pub track: String,
    pub compound: Option<String>,
    pub set_age: u32,
    pub counting: bool,
    pub wear_mult: f32,
    pub fuel_mult: f32,
    /// Laps stored (changes when a lap was added: the dashboard reloads the figures)
    pub laps: usize,
}

fn call(level: &'static str, text: String) -> Call {
    Call { level, text }
}

pub struct Strategy {
    pub saved: Saved,
    file: Option<PathBuf>,
    live: Live,
    /// Planner result and the input it was computed for (the planner runs once per lap, not per packet)
    cache: Option<(String, planner::Options)>,
    pub practice: practice::Practice,
    /// Race with the practice figures filled in, and what it was computed from
    filled: Option<(String, Race, Option<PracticeUse>)>,
}

impl Strategy {
    /// `file = None`: nothing is stored (demo).
    pub fn load(file: Option<PathBuf>) -> Self {
        let saved =
            file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let practice = practice::Practice::load(file.as_ref().map(|f| f.with_file_name("practice.json")));
        Strategy { saved, file, live: Live::default(), cache: None, practice, filled: None }
    }

    pub fn shared(self) -> Shared {
        Arc::new(Mutex::new(self))
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

    pub fn json(&self) -> String {
        serde_json::to_string(&self.saved).unwrap_or_default()
    }

    /// New race settings; a different race starts from scratch.
    pub fn set_race(&mut self, race: Race) {
        // Picking the race during the session keeps what was measured so far
        let other = self.saved.race.as_ref().is_some_and(|r| r.id != race.id || r.name != race.name);
        if other {
            self.reset_progress();
        }
        self.saved.race = Some(race);
        self.save();
    }

    /// Race restarted: stints, stops and fuel laps go, race and calibration stay. What was
    /// recorded is kept in `strategy-history/` next to `strategy.json`.
    pub fn reset_progress(&mut self) {
        self.archive();
        self.saved.stints.clear();
        self.saved.stops.clear();
        self.saved.fuel_laps.clear();
    }

    fn archive(&self) {
        let (Some(file), false) = (&self.file, self.saved.fuel_laps.len() < 2 && self.saved.stops.is_empty()) else { return };
        let Some(dir) = file.parent().map(|d| d.join("strategy-history")) else { return };
        let name: String = self
            .saved
            .race
            .as_ref()
            .map_or("bez-zavodu".into(), |r| r.name.clone())
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let path = dir.join(format!("{}_{}_auto{}.json", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"), name, self.live.car_code));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(path, serde_json::to_string_pretty(&self.saved).unwrap_or_default());
    }

    pub fn update_live(&mut self, live: Live) {
        self.live = live;
    }

    #[cfg(test)]
    pub fn lap_done(&mut self, lap: i16, used: f32, refuel: bool, ms: Option<i32>) {
        self.lap_finished(lap, used, refuel, ms, true);
    }

    /// A lap ended at the line. `closed`: it ended where it started (not the short first race lap
    /// of a track with separate start and finish lines). In practice mode it goes to the practice.
    pub fn lap_finished(&mut self, lap: i16, used: f32, refuel: bool, ms: Option<i32>, closed: bool) {
        if self.practice.saved.active {
            self.practice.lap_done(self.live.car_code, used, refuel, ms, closed);
            return;
        }
        if lap < 1 || used <= 0.0 {
            return;
        }
        self.saved.fuel_laps.retain(|l| l.lap != lap);
        self.saved.fuel_laps.push(FuelLap { lap, used, refuel, ms: ms.filter(|m| *m > 0) });
        if self.saved.fuel_laps.len() > 200 {
            self.saved.fuel_laps.remove(0);
        }
        self.measure_pit_loss(lap);
        self.save();
    }

    /// After the out-lap: (in-lap + out-lap) − 2 × typical lap − refuelling time = pit loss of the
    /// track. Kept per track (average with earlier measurements).
    fn measure_pit_loss(&mut self, lap: i16) {
        let Some(race) = &self.saved.race else { return };
        let Some(rate) = race.refuel_rate.filter(|r| *r > 0.0) else { return };
        let Some(stop) = self.saved.stops.iter().find(|s| s.lap + 1 == lap) else { return };
        let ms = |l: i16| self.saved.fuel_laps.iter().find(|f| f.lap == l).and_then(|f| f.ms);
        let (Some(inlap), Some(outlap)) = (ms(stop.lap), ms(stop.lap + 1)) else { return };
        let near_stop = |l: i16| self.saved.stops.iter().any(|s| l == s.lap || l == s.lap + 1);
        let mut clean: Vec<i32> =
            self.saved.fuel_laps.iter().filter(|f| f.lap > 1 && !f.refuel && !near_stop(f.lap)).filter_map(|f| f.ms).collect();
        if clean.len() < 3 {
            return;
        }
        clean.sort_unstable();
        let typical = clean[clean.len() / 2] as f32 / 1000.0;
        let loss = (inlap + outlap) as f32 / 1000.0 - 2.0 * typical - stop.fuel_added / rate;
        if !(5.0..=90.0).contains(&loss) {
            return;
        }
        let key = format!("pit|{}", race.track);
        let v = self.saved.calibration.get(&key).map_or(loss, |old| (old + loss) / 2.0);
        self.saved.calibration.insert(key, v);
    }

    /// Fuel went up during `lap` (in the pit).
    /// GT7 counts the lap while the car stands in the box: a stop that began in the previous lap is
    /// the same stop.
    pub fn refuelled(&mut self, lap: i16, added: f32) {
        if self.practice.saved.active {
            self.practice.pit();
            return;
        }
        match self.saved.stops.last_mut() {
            Some(s) if s.lap == lap || s.lap == lap - 1 => s.fuel_added += added,
            _ => self.saved.stops.push(Stop { lap, fuel_added: added, tyres: None }),
        }
    }

    /// The car stood still in a race lap long enough to be a pit stop (tyres only, no fuel).
    pub fn stopped(&mut self, lap: i16) {
        if self.practice.saved.active {
            self.practice.pit();
            return;
        }
        if !self.saved.stops.last().is_some_and(|s| s.lap == lap || s.lap == lap - 1) {
            self.saved.stops.push(Stop { lap, fuel_added: 0.0, tyres: None });
            self.save();
        }
    }

    fn stint_mut(&mut self) -> &mut Stint {
        if self.saved.stints.is_empty() {
            let compound = self.saved.race.as_ref().and_then(|r| r.start_compound.clone());
            self.saved.stints.push(Stint { compound, ..Default::default() });
        }
        self.saved.stints.last_mut().unwrap()
    }

    /// After a stop the tracker saw whether the tyres came out cold (new) or not. A choice the
    /// driver made on the dashboard is kept; new tyres are assumed to be the same compound.
    pub fn tyres_detected(&mut self, changed: bool) {
        if self.practice.saved.active {
            if changed {
                self.practice.new_set(None);
            }
            return;
        }
        let Some(stop) = self.saved.stops.last_mut() else { return };
        if stop.tyres.is_some() {
            return;
        }
        stop.tyres = Some(changed);
        let lap = stop.lap;
        if changed {
            let compound = effective_stints(&self.saved).last().and_then(|s| s.compound.clone());
            if self.saved.stints.is_empty() {
                self.saved.stints = effective_stints(&self.saved);
            }
            self.saved.stints.push(Stint { start: lap as f32, compound, wear: vec![] });
        }
        self.save();
    }

    /// Compound of the tyres on the car now (corrects the current stint).
    pub fn set_compound(&mut self, compound: &str) {
        self.stint_mut().compound = Some(compound.to_uppercase());
        self.save();
    }

    /// New tyres now: a new stint starts. Answers an open pit stop question, or records a stop.
    pub fn new_tyres(&mut self, compound: &str) {
        let at = self.live.driven();
        let lap = self.live.lap;
        match self.saved.stops.last_mut() {
            Some(s) if s.tyres.is_none() => s.tyres = Some(true),
            _ => self.saved.stops.push(Stop { lap, fuel_added: 0.0, tyres: Some(true) }),
        }
        if self.saved.stints.is_empty() && at < 0.5 {
            // Before the start: just the starting tyres
            self.saved.stints.push(Stint { start: 0.0, compound: Some(compound.to_uppercase()), wear: vec![] });
            self.saved.stops.pop();
        } else {
            self.saved.stints.push(Stint { start: at, compound: Some(compound.to_uppercase()), wear: vec![] });
        }
        self.save();
    }

    /// The last detected stop was none (the car only stood still): forget it.
    pub fn no_stop(&mut self) {
        if self.saved.stops.last().is_some_and(|s| s.tyres.is_none() && s.fuel_added <= 0.0) {
            self.saved.stops.pop();
        }
        self.save();
    }

    pub fn same_tyres(&mut self) {
        if let Some(s) = self.saved.stops.last_mut() {
            s.tyres.get_or_insert(false);
        }
        self.save();
    }

    /// Tyre wear read in GT7 (the most worn tyre, %).
    pub fn add_wear(&mut self, pct: f32) {
        let at = self.live.driven();
        self.stint_mut().wear.push(WearMark { at, pct: pct.clamp(0.0, 100.0) });
        if let (Some(rate), Some(key)) = (wear_rate(self.saved.stints.last().unwrap()), self.calibration_key()) {
            self.saved.calibration.insert(key, rate);
        }
        self.save();
    }

    fn calibration_key(&self) -> Option<String> {
        let race = self.saved.race.as_ref()?;
        let compound = effective_stints(&self.saved).last()?.compound.clone()?;
        Some(format!("{}|{}|{}|{}", self.live.car_code, race.track, compound, race.tyre_wear.unwrap_or(1.0)))
    }

    /// Clean laps (no refuelling, not the first lap from the grid); the first lap only if nothing else.
    pub fn fuel_per_lap(&self) -> Option<f32> {
        // The lap with a stop and the one after it (pit exit) burn less than a racing lap
        let pit = |l: &FuelLap| l.refuel || self.saved.stops.iter().any(|s| s.lap == l.lap || s.lap == l.lap - 1);
        let clean: Vec<f32> = self.saved.fuel_laps.iter().filter(|l| !pit(l) && l.lap > 1).map(|l| l.used).collect();
        // No clean lap yet: better a lap next to a stop than no figure at all
        let pick =
            if clean.is_empty() { self.saved.fuel_laps.iter().filter(|l| !l.refuel).map(|l| l.used).collect::<Vec<_>>() } else { clean };
        let recent = &pick[pick.len().saturating_sub(FUEL_LAPS)..];
        (!recent.is_empty()).then(|| recent.iter().sum::<f32>() / recent.len() as f32)
    }

    /// Shape of the last full lap (practice: recognise or remember the track).
    pub fn lap_shape(&mut self, map: &[(f32, f32)]) {
        if self.practice.saved.active {
            self.practice.shape(map);
        }
    }

    /// Practice mode on/off. Switching on takes the multipliers and the track of the race set.
    pub fn set_practice(&mut self, on: bool) {
        let p = &mut self.practice.saved;
        if on && !p.active {
            if let Some(r) = &self.saved.race {
                p.wear_mult = r.tyre_wear.unwrap_or(1.0);
                p.fuel_mult = r.fuel_mult.unwrap_or(1.0);
                if p.track.is_empty() || !r.track.is_empty() {
                    p.track.clone_from(&r.track);
                }
            }
        }
        p.active = on;
        self.practice.save();
    }

    pub fn practice_state(&self) -> PracticeState {
        let p = &self.practice.saved;
        PracticeState {
            active: p.active,
            track: p.track.clone(),
            compound: p.compound.clone(),
            set_age: p.set_age,
            counting: p.counting,
            wear_mult: p.wear_mult,
            fuel_mult: p.fuel_mult,
            laps: p.laps.len(),
        }
    }

    /// Car the practice figures are taken for: the car driven now if it has practice laps on the
    /// track, else the car with the most laps there.
    pub fn practice_car(&self, track: &str) -> Option<i32> {
        let key = practice::track_key(track);
        let has = |c: i32| self.practice.saved.laps.iter().any(|l| l.car == c && practice::track_key(&l.track) == key);
        if self.live.car_code > 0 && has(self.live.car_code) {
            return Some(self.live.car_code);
        }
        self.practice.main_car(track).or((self.live.car_code > 0).then_some(self.live.car_code))
    }

    /// The race with what is missing taken from practice on the same track with the same car and
    /// multipliers: laps a set lasts, pace, degradation per compound, fuel per lap.
    fn race_with_practice(&mut self, race: &Race) -> (Race, Option<PracticeUse>) {
        let car = self.practice_car(&race.track);
        let key = format!(
            "{:?}|{}|{}|{}|{car:?}|{}",
            race,
            self.practice.saved.laps.len(),
            self.practice.saved.life.len(),
            self.practice.saved.life.values().sum::<u32>(),
            self.practice.saved.laps.iter().filter(|l| l.counted).count()
        );
        if let Some((k, r, u)) = &self.filled {
            if *k == key {
                return (r.clone(), u.clone());
            }
        }
        let result = fill_from_practice(&self.practice, race, car);
        self.filled = Some((key, result.0.clone(), result.1.clone()));
        result
    }

    pub fn plan(&mut self) -> Plan {
        let fpl = self.fuel_per_lap();
        let original = self.saved.race.clone();
        let practice_use = match &original {
            Some(r) => {
                let (filled, used) = self.race_with_practice(r);
                self.saved.race = Some(filled);
                used
            }
            None => None,
        };
        let p = self.plan_inner(fpl);
        self.saved.race = original;
        let mut p = p;
        if practice_use.as_ref().is_some_and(|u| u.fuel_per_lap.is_some()) && p.fuel_source == "manual" {
            p.fuel_source = "practice";
        }
        p.practice_use = practice_use;
        p.practice = self.practice_state();
        p
    }

    fn plan_inner(&mut self, fpl: Option<f32>) -> Plan {
        let mut p = plan(&self.saved, &self.live, fpl, self.calibration_key().and_then(|k| self.saved.calibration.get(&k).copied()));
        let (pit_loss, source) = self.pit_loss();
        p.pit_loss = pit_loss;
        p.pit_loss_source = source;
        if let Some(inp) = planner_input(&self.saved, &self.live, &p, pit_loss) {
            // Recompute when something that matters changes, not for every packet
            let r = |v: f32, step: f32| (v / step).round() as i32;
            let key = format!(
                "{} {} {} {:?} {} {:?} {} {} {} {} {} {} {} {:?}",
                inp.first_lap,
                inp.laps,
                inp.stop_now,
                inp.current,
                inp.current_age,
                inp.need_compound,
                inp.need_stop,
                r(inp.fuel, 1.0),
                r(inp.fuel_per_lap, 0.05),
                r(inp.pit_loss, 0.5),
                r(inp.deg, 0.01),
                r(inp.reserve, 0.1),
                r(inp.refuel_rate, 0.1),
                inp.compounds,
            );
            let options = match &self.cache {
                Some((k, o)) if *k == key => o.clone(),
                _ => {
                    let o = planner::plan(&inp);
                    self.cache = Some((key, o.clone()));
                    o
                }
            };
            apply_options(&mut p, &inp, options, &self.live);
        }
        p
    }

    /// Strategy for a race before the start (race settings dialog).
    pub fn preview(&mut self, race: &Race) -> (planner::Options, Plan) {
        let (race, practice_use) = self.race_with_practice(race);
        let saved = Saved { race: Some(race.clone()), calibration: self.saved.calibration.clone(), ..Default::default() };
        let cap = if self.live.capacity > 0.0 { self.live.capacity } else { 100.0 };
        let live = Live { lap: 0, total_laps: 0, frac: 0.0, fuel: cap, capacity: cap, car_code: self.live.car_code, standing: true };
        let fpl = race.fuel_per_lap.or(self.fuel_per_lap());
        let mut p = plan(&saved, &live, fpl, None);
        let (pit_loss, source) = pit_loss_of(&saved);
        p.pit_loss = pit_loss;
        p.pit_loss_source = source;
        let options = planner_input(&saved, &live, &p, pit_loss).map(|i| planner::plan(&i)).unwrap_or_default();
        // Before the race the practice figure is what the plan is based on
        if practice_use.as_ref().is_some_and(|u| u.fuel_per_lap.is_some()) {
            p.fuel_source = "practice";
        }
        p.practice_use = practice_use;
        (options, p)
    }

    fn pit_loss(&self) -> (f32, &'static str) {
        pit_loss_of(&self.saved)
    }
}

/// See [`Strategy::race_with_practice`]. What the driver entered for the race wins.
fn fill_from_practice(practice: &practice::Practice, race: &Race, car: Option<i32>) -> (Race, Option<PracticeUse>) {
    let mut r = race.clone();
    let Some(car) = car else { return (r, None) };
    if race.track.trim().is_empty() {
        return (r, None);
    }
    let s = practice.summary(car, &race.track, race.tyre_wear.unwrap_or(1.0), race.fuel_mult.unwrap_or(1.0));
    if s.compounds.is_empty() && s.fuel_per_lap.is_none() {
        return (r, None);
    }
    for c in &s.compounds {
        let spec = r.tyres.entry(c.compound.clone()).or_default();
        spec.life = spec.life.or(c.life);
        spec.pace = spec.pace.or(c.pace);
        if race.deg.is_none() {
            spec.deg = spec.deg.or(c.deg);
        }
    }
    r.fuel_per_lap = r.fuel_per_lap.or(s.fuel_per_lap);
    let used = PracticeUse {
        car,
        compounds: s
            .compounds
            .iter()
            .map(|c| PracticeCompound { compound: c.compound.clone(), laps: c.laps, life: c.life, pace: c.pace, deg: c.deg })
            .collect(),
        fuel_per_lap: s.fuel_per_lap.filter(|_| race.fuel_per_lap.is_none()),
    };
    (r, Some(used))
}

/// The stints, with the starting tyres from the race settings when nothing was picked yet.
fn effective_stints(saved: &Saved) -> Vec<Stint> {
    let start = saved.race.as_ref().and_then(|r| r.start_compound.clone());
    let mut stints = saved.stints.clone();
    match stints.first_mut() {
        None if start.is_some() => stints.push(Stint { compound: start, ..Default::default() }),
        Some(first) if first.compound.is_none() => first.compound = start,
        _ => {}
    }
    stints
}

fn pit_loss_of(saved: &Saved) -> (f32, &'static str) {
    let race = saved.race.clone().unwrap_or_default();
    match (race.pit_loss, saved.calibration.get(&format!("pit|{}", race.track))) {
        (Some(v), _) => (v, "manual"),
        (None, Some(v)) => (*v, "measured"),
        _ => (DEFAULT_PIT_LOSS, "default"),
    }
}

/// The compounds the planner may use: the kind (R/S/C) of the race, only those with entered data
/// when any was entered (plus the mandatory one and the one on the car).
fn compounds(race: &Race, current: Option<&str>) -> Vec<planner::Compound> {
    let kind = race
        .mandatory
        .as_deref()
        .or(race.start_compound.as_deref())
        .or(current)
        .and_then(|c| c.chars().next())
        .unwrap_or('R')
        .to_ascii_uppercase();
    let names: Vec<String> = ['S', 'M', 'H'].iter().map(|c| format!("{kind}{c}")).collect();
    let spec = |n: &str| race.tyres.get(n).cloned().unwrap_or_default();
    let entered = names.iter().any(|n| spec(n).life.is_some() || spec(n).pace.is_some() || spec(n).deg.is_some());
    names
        .iter()
        .filter(|n| {
            !entered
                || spec(n).life.is_some()
                || spec(n).pace.is_some()
                || spec(n).deg.is_some()
                || race.mandatory.as_deref() == Some(n.as_str())
                || current == Some(n.as_str())
                || race.start_compound.as_deref() == Some(n.as_str())
        })
        .map(|n| planner::Compound { deg: spec(n).deg, ..planner::Compound::new(n, spec(n).life, spec(n).pace.unwrap_or(0.0)) })
        .collect()
}

fn planner_input(saved: &Saved, live: &Live, p: &Plan, pit_loss: f32) -> Option<planner::Input> {
    let race = saved.race.as_ref()?;
    let total = p.total_laps? as i16;
    let fpl = p.fuel_per_lap.filter(|f| *f > 0.0)?;
    let cap = if live.capacity > 0.0 { live.capacity } else { 100.0 };
    let current = p.compound.clone();
    let mut inp = planner::Input {
        first_lap: live.lap.max(1),
        laps: (total - live.lap.max(1) + 1).max(0) as u32,
        fuel: (live.fuel + fpl * live.frac).min(cap),
        capacity: cap,
        fuel_per_lap: fpl,
        reserve: p.reserve_laps,
        refuel_rate: race.refuel_rate.unwrap_or(0.0),
        pit_loss,
        deg: race.deg.unwrap_or(DEFAULT_DEG),
        current: current.clone(),
        current_age: p.stint_laps.floor() as u32,
        stop_now: false,
        need_compound: race.mandatory.clone().filter(|_| !p.mandatory_done),
        need_stop: race.pit_required && !p.pit_done,
        compounds: compounds(race, current.as_deref()),
    };
    if live.lap <= 0 {
        // Before the start: full race, fuel on board (full tank if unknown)
        inp.first_lap = 1;
        inp.laps = total.max(0) as u32;
        inp.fuel = if live.fuel > 0.0 { live.fuel } else { cap };
        inp.current_age = 0;
    }
    // Standing in the box: plan the stop being made now
    if let Some(stop) = saved.stops.last().filter(|s| live.standing && live.lap > 0 && (s.lap == live.lap || s.lap == live.lap - 1)) {
        inp.stop_now = true;
        inp.first_lap = stop.lap + 1;
        inp.laps = (total - stop.lap).max(0) as u32;
        inp.fuel = (live.fuel - stop.fuel_added).max(0.0);
        inp.current = None;
        inp.current_age = 0;
        inp.need_stop = false;
    }
    (inp.laps > 0).then_some(inp)
}

fn apply_options(p: &mut Plan, inp: &planner::Input, options: planner::Options, live: &Live) {
    if let Some(best) = &options.best {
        if inp.stop_now {
            if let Some(first) = best.stops.first() {
                p.stop_add = Some(first.fuel);
                p.stop_add_laps = best.stints.first().map(|s| s.laps);
            }
            p.target_lap = None;
            p.laps_to_box = None;
        } else if let Some(next) = best.stops.first() {
            p.target_lap = Some(next.lap);
            p.laps_to_box = Some((next.lap - live.lap.max(1)).max(0));
            p.next_stop_fuel = Some(next.fuel);
        } else {
            p.target_lap = None;
            p.laps_to_box = None;
        }
    }
    p.options = options;
    p.calls = calls(p, live);
}

/// Wear per lap of a stint: line through the origin (new tyres = 0 %) fitted to the readings.
fn wear_rate(stint: &Stint) -> Option<f32> {
    let pts: Vec<(f32, f32)> = stint.wear.iter().map(|m| (m.at - stint.start, m.pct)).filter(|(x, _)| *x >= 0.5).collect();
    let sxx: f32 = pts.iter().map(|(x, _)| x * x).sum();
    (sxx > 0.0).then(|| pts.iter().map(|(x, y)| x * y).sum::<f32>() / sxx)
}

fn fmt2(v: f32) -> String {
    format!("{v:.2}").replace('.', ",")
}

fn fmt1(v: f32) -> String {
    format!("{v:.1}").replace('.', ",")
}

pub fn plan(saved: &Saved, live: &Live, measured_fpl: Option<f32>, calibrated_wear: Option<f32>) -> Plan {
    let race = saved.race.clone().unwrap_or_default();
    let mut p = Plan {
        race: saved.race.as_ref().map(|r| r.name.clone()),
        reserve_laps: race.reserve_laps.unwrap_or(DEFAULT_RESERVE),
        wear_limit: race.wear_limit.unwrap_or(DEFAULT_WEAR_LIMIT),
        pit_required: race.pit_required,
        pit_done: !saved.stops.is_empty(),
        mandatory: race.mandatory.clone(),
        ..Default::default()
    };
    let driven = live.driven();
    let total = if live.total_laps > 0 { Some(live.total_laps as u32) } else { race.laps };
    p.total_laps = total;
    let laps_left = total.map(|t| (t as f32 - driven).max(0.0));
    p.laps_left = laps_left;

    // Without a measured lap (e.g. the server started mid-race): fuel burnt so far, assuming a
    // full tank at the start and no refuelling yet
    let refuelled = saved.stops.iter().any(|s| s.fuel_added > 0.0);
    let so_far = (driven >= 1.0 && !refuelled && live.capacity > live.fuel).then(|| (live.capacity - live.fuel) / driven);
    let (fpl, source) = match (measured_fpl, race.fuel_per_lap, so_far) {
        (Some(f), ..) => (Some(f), "measured"),
        (None, Some(f), _) => (Some(f), "manual"),
        (None, None, Some(f)) => (Some(f), "so_far"),
        _ => (None, ""),
    };
    p.fuel_per_lap = fpl;
    p.fuel_source = source;

    // ---- Tyres ----
    let stints = effective_stints(saved);
    let stint = stints.last();
    p.compound = stint.and_then(|s| s.compound.clone());
    p.stint_laps = stint.map_or(driven, |s| (driven - s.start).max(0.0));
    p.mandatory_done = match &race.mandatory {
        Some(m) => stints.iter().any(|s| s.compound.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(m))),
        None => true,
    };
    p.tyre_question = saved.stops.last().is_some_and(|s| s.tyres.is_none());
    let (rate, wsource) = match (stint.and_then(wear_rate), calibrated_wear) {
        (Some(r), _) => (Some(r), "stint"),
        (None, Some(r)) => (Some(r), "calibration"),
        _ => (None, ""),
    };
    p.wear_per_lap = rate;
    p.wear_source = wsource;
    if let Some(r) = rate.filter(|r| *r > 0.0) {
        let now = r * p.stint_laps;
        p.wear_now = Some(now);
        p.wear_laps_left = Some(((p.wear_limit - now) / r).max(0.0));
        p.wear_at_finish = laps_left.map(|l| now + r * l);
    }

    // ---- Fuel ----
    if let (Some(fpl), Some(left)) = (fpl.filter(|f| *f > 0.0), laps_left) {
        let cap = if live.capacity > 0.0 { live.capacity } else { 100.0 };
        let need = (left + p.reserve_laps) * fpl;
        p.fuel_needed = Some(need);
        p.fuel_margin_laps = Some(live.fuel / fpl - left);
        let total = total.unwrap_or(0) as f32;
        if need > live.fuel {
            p.stops_needed = ((need - live.fuel) / cap).ceil().max(1.0) as u32;
            // Latest: arrive in the pit before the tank is empty
            let latest = ((driven + live.fuel / fpl - PIT_MARGIN).floor() as i16).min(total as i16 - 1);
            // Earliest (one stop): what is still needed after the stop must fit into the tank
            let earliest = if p.stops_needed == 1 { (total + p.reserve_laps - cap / fpl).ceil() as i16 } else { live.lap };
            let earliest = earliest.max(live.lap.max(1));
            let one_stop = |f: f32| {
                let latest = ((driven + live.fuel / f - PIT_MARGIN).floor() as i16).min(total as i16 - 1);
                let earliest = ((total + p.reserve_laps - cap / f).ceil() as i16).max(live.lap.max(1));
                (earliest <= latest && latest >= live.lap).then_some([earliest, latest])
            };
            if p.stops_needed == 1 && earliest > latest {
                p.stops_needed = 2;
            }
            if p.stops_needed == 2 {
                // How much less fuel per lap would make one stop enough (lift and coast)?
                p.one_stop_fpl = (1..=150).map(|i| fpl - i as f32 * 0.005).find(|f| one_stop(*f).is_some());
                p.window = Some([live.lap.max(1).min(latest), latest]);
            } else {
                // latest < current lap: the fuel runs out before the pit ("box now")
                p.window = Some([earliest.min(latest), latest]);
            }
        } else if race.pit_required && !p.pit_done {
            p.window = Some([live.lap.max(1), (total as i16 - 1).max(live.lap.max(1))]);
        }
        // Boxing at the end of this lap
        let fuel_at_box = (live.fuel - fpl * (1.0 - live.frac)).max(0.0);
        let after = (total - live.lap.max(1) as f32 + p.reserve_laps) * fpl;
        let add = (after - fuel_at_box).clamp(0.0, (cap - fuel_at_box).max(0.0));
        p.add_now = Some(add);
        p.refuel_s = race.refuel_rate.filter(|r| *r > 0.0).map(|r| add / r);
    }

    p.calls = calls(&p, live);
    p
}

fn calls(p: &Plan, live: &Live) -> Vec<Call> {
    let fpl = p.fuel_per_lap;
    let lap = live.lap;
    let mut calls = Vec::new();
    if let Some(add) = p.stop_add {
        let laps = p.stop_add_laps.map_or(String::new(), |n| format!(" (na {n} kol)"));
        calls.push(call("box", format!("DOTANKUJ +{}{laps}", fmt1(add))));
    }
    if p.tyre_question && p.stop_add.is_none() {
        calls.push(call("warn", "Zastávka? Zvol nové gumy, „Stejné gumy“, nebo „Nebyla to zastávka“.".into()));
    }
    if p.race.as_deref().unwrap_or("").is_empty() {
        calls.push(call("info", "Vyber závod, ať vím počet kol a pravidla.".into()));
    }
    let add = |v: Option<f32>| v.map_or(String::new(), |v| format!(" · dotankovat +{}", fmt1(v)));
    let out_of_fuel = matches!(p.window, Some([_, b]) if p.stops_needed > 0 && b < lap);
    if out_of_fuel && p.stop_add.is_none() {
        calls.push(call("box", "BOX TEĎ – palivo nevystačí na další kolo!".into()));
    } else if let (Some(t), None) = (p.target_lap, p.stop_add) {
        if t <= lap {
            calls.push(call("box", format!("BOX na konci tohoto kola{}", add(p.next_stop_fuel))));
        } else {
            let window = match p.window {
                Some([a, b]) if p.stops_needed == 1 && a != b => format!(" (palivo: okno {a}–{b})"),
                Some([_, b]) if p.stops_needed > 1 => format!(" (palivo: nejpozději {b})"),
                _ => String::new(),
            };
            calls.push(call("info", format!("Box v kole {t}{window}")));
        }
    } else if p.stops_needed == 0 && p.stop_add.is_none() {
        match p.fuel_margin_laps {
            Some(m) => calls.push(call("info", format!("Palivo do cíle stačí (+{} kola)", fmt1(m)))),
            None if p.total_laps.is_some() && fpl.is_none() => calls.push(call("info", "Spotřeba: změřím po prvním kole".into())),
            None => {}
        }
    }
    if p.stops_needed > 1 {
        if let (Some(target), Some(f)) = (p.one_stop_fpl, fpl) {
            calls.push(call("info", format!("1 zastávka stačí při spotřebě {} / kolo (ušetřit {})", fmt2(target), fmt2(f - target))));
        }
    }
    if p.pit_required && !p.pit_done {
        calls.push(call("warn", "Povinná zastávka ještě neproběhla".into()));
    }
    if let (Some(m), false) = (&p.mandatory, p.mandatory_done) {
        calls.push(call("warn", format!("Povinná směs {m} ještě nebyla použita")));
    }
    if p.compound.is_none() && p.race.is_some() {
        calls.push(call("info", "Nastav startovní směs v „Závod…“".into()));
    }
    if let (Some(w), Some(l)) = (p.wear_laps_left, p.laps_left) {
        if w < l {
            calls.push(call("warn", format!("Gumy na limitu {} % za ~{} kol", p.wear_limit as i32, fmt1(w))));
        }
    }
    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    fn barcelona(laps: u32) -> Saved {
        Saved {
            race: Some(Race {
                id: "x".into(),
                name: "IMSA Barcelona".into(),
                track: "Barcelona".into(),
                laps: Some(laps),
                tyre_wear: Some(6.0),
                fuel_mult: Some(3.0),
                refuel_rate: Some(3.0),
                pit_required: true,
                mandatory: Some("RS".into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Plan including the planner (as `Strategy::plan`), with a given fuel per lap.
    fn full(saved: &Saved, live: &Live, fpl: f32) -> Plan {
        let mut p = plan(saved, live, Some(fpl), None);
        let (loss, _) = pit_loss_of(saved);
        if let Some(inp) = planner_input(saved, live, &p, loss) {
            apply_options(&mut p, &inp, planner::plan(&inp), live);
        }
        p
    }

    fn live(lap: i16, frac: f32, fuel: f32) -> Live {
        Live { lap, total_laps: 0, frac, fuel, capacity: 100.0, car_code: 1, standing: false }
    }

    #[test]
    fn one_stop_window_matches_hand_calculation() {
        // 23 laps, 7.16 per lap, full tank at the start of lap 1
        let p = full(&barcelona(23), &live(1, 0.0, 100.0), 7.16);
        assert_eq!(p.stops_needed, 1);
        // earliest: 23 + 0.3 − 100/7.16 = 9.33 → 10; latest: 100/7.16 − 0.15 = 13.8 → 13
        assert_eq!(p.window, Some([10, 13]));
        let t = p.target_lap.unwrap();
        assert!((10..=13).contains(&t), "{t}");
        assert!(p.calls.iter().any(|c| c.text == format!("Box v kole {t} (palivo: okno 10–13)")), "{:?}", p.calls);
    }

    #[test]
    fn in_window_tells_how_much_to_add() {
        // Lap 11 just started: 10 laps done, fuel 100 − 10 × 7.16
        let mut saved = barcelona(23);
        saved.race.as_mut().unwrap().start_compound = Some("RM".into());
        let p = full(&saved, &live(11, 0.0, 28.4), 7.16);
        assert_eq!(p.window, Some([11, 13]));
        // 10 laps on old mediums: the planner boxes now
        assert_eq!(p.target_lap, Some(11));
        // At the box: 28.4 − 7.16 = 21.24; needed after: (23 − 11 + 0.3) × 7.16 = 88.07 → +66.8
        let add = p.add_now.unwrap();
        assert!((add - 66.83).abs() < 0.05, "{add}");
        assert!((p.refuel_s.unwrap() - 22.28).abs() < 0.05);
        assert_eq!(p.calls[0].level, "box");
    }

    #[test]
    fn enough_fuel_but_mandatory_stop_and_compound() {
        let p = plan(&barcelona(10), &live(1, 0.0, 100.0), Some(7.16), None);
        assert_eq!(p.stops_needed, 0);
        assert_eq!(p.window, Some([1, 9]));
        assert!(p.calls.iter().any(|c| c.text.starts_with("Povinná zastávka")));
        assert!(p.calls.iter().any(|c| c.text.contains("RS")));
    }

    #[test]
    fn telemetry_lap_count_wins() {
        let mut l = live(1, 0.0, 100.0);
        l.total_laps = 12;
        assert_eq!(plan(&barcelona(23), &l, Some(7.16), None).total_laps, Some(12));
    }

    #[test]
    fn fuel_per_lap_skips_first_and_refuel_laps() {
        let mut s = Strategy::load(None);
        s.lap_done(1, 6.73, false, None);
        assert!((s.fuel_per_lap().unwrap() - 6.73).abs() < 1e-4); // only lap 1 so far
        s.lap_done(2, 7.18, false, None);
        s.lap_done(3, 7.08, false, None);
        s.lap_done(4, 3.0, true, None);
        assert!((s.fuel_per_lap().unwrap() - 7.13).abs() < 1e-4);
    }

    #[test]
    fn wear_rate_and_calibration() {
        let mut s = Strategy::load(None);
        s.set_race(barcelona(23).race.unwrap());
        s.update_live(live(1, 0.0, 100.0));
        s.new_tyres("rm"); // before the start: starting tyres, no stop
        assert!(s.saved.stops.is_empty());
        s.update_live(live(9, 0.0, 40.0)); // 8 laps done
        s.add_wear(12.0);
        let p = s.plan();
        assert!((p.wear_per_lap.unwrap() - 1.5).abs() < 1e-4);
        assert_eq!(p.wear_source, "stint");
        assert!((p.wear_laps_left.unwrap() - (70.0 - 12.0) / 1.5).abs() < 1e-3);
        assert_eq!(s.saved.calibration.len(), 1);

        // Pit stop with refuelling, new softs: mandatory compound done, wear from calibration? no –
        // other compound, so none yet
        s.refuelled(12, 60.0);
        s.update_live(live(13, 0.0, 90.0));
        assert!(s.plan().tyre_question);
        s.new_tyres("rs");
        let p = s.plan();
        assert!(!p.tyre_question && p.pit_done && p.mandatory_done);
        assert_eq!(p.compound.as_deref(), Some("RS"));
        assert_eq!(p.wear_per_lap, None);
    }

    #[test]
    fn estimate_from_fuel_burnt_when_started_mid_race() {
        // Server started in lap 11 of 27: nothing measured, 22.3 left after 10.8 laps
        let mut l = live(11, 0.8, 22.3);
        l.total_laps = 27;
        let p = plan(&barcelona(27), &l, None, None);
        assert_eq!(p.fuel_source, "so_far");
        assert!((p.fuel_per_lap.unwrap() - 7.194).abs() < 0.01);
        assert!(p.stops_needed >= 1 && p.window.is_some());
    }

    #[test]
    fn picking_the_race_keeps_measured_laps() {
        let mut s = Strategy::load(None);
        s.lap_done(2, 7.1, false, None);
        s.set_race(barcelona(23).race.unwrap());
        assert_eq!(s.saved.fuel_laps.len(), 1);
        let mut other = barcelona(23).race.unwrap();
        other.id = "y".into();
        s.set_race(other);
        assert!(s.saved.fuel_laps.is_empty());
    }

    #[test]
    fn tyre_only_stop_counts_and_pit_laps_leave_the_average() {
        let mut s = Strategy::load(None);
        for (lap, used) in [(2, 7.13), (3, 7.17), (4, 7.33), (5, 6.25), (6, 6.78), (7, 7.13)] {
            s.lap_done(lap, used, false, None);
        }
        s.stopped(5);
        s.stopped(5); // still standing: no second stop
        assert_eq!(s.saved.stops.len(), 1);
        assert!(s.saved.stops[0].tyres.is_none());
        // laps 5 (stop) and 6 (pit exit) are left out
        assert!((s.fuel_per_lap().unwrap() - (7.13 + 7.17 + 7.33 + 7.13) / 4.0).abs() < 1e-4);
    }

    #[test]
    fn single_lap_window_reads_naturally() {
        // 27 laps at 7.0: one stop only at the end of lap 14
        let mut l = live(1, 0.0, 99.7);
        l.total_laps = 27;
        let p = full(&barcelona(27), &l, 7.0);
        assert_eq!(p.window, Some([14, 14]));
        assert!(p.calls.iter().any(|c| c.text == "Box v kole 14"));
    }

    #[test]
    fn two_stops_but_one_with_fuel_saving() {
        // Today's grid: 98.4 on board, 7.06 per lap, 27 laps → one stop misses by a hair
        let mut l = live(0, 0.0, 98.4);
        l.total_laps = 27;
        let p = plan(&barcelona(27), &l, Some(7.06), None);
        assert_eq!(p.stops_needed, 2);
        assert_eq!(p.window, Some([1, 13]));
        let target = p.one_stop_fpl.unwrap();
        assert!(target < 7.06 && target > 6.9, "{target}");
        assert!(p.calls.iter().any(|c| c.text.starts_with("1 zastávka stačí při spotřebě")));
    }

    #[test]
    fn false_stop_can_be_dismissed_and_fuel_still_estimated() {
        let mut s = Strategy::load(None);
        s.set_race(barcelona(27).race.unwrap());
        s.stopped(1);
        s.lap_done(1, 6.9, false, None);
        s.lap_done(2, 7.1, false, None);
        assert!(s.fuel_per_lap().is_some(), "laps next to a stop still give a figure");
        assert!(s.plan().pit_done);
        s.no_stop();
        assert!(s.saved.stops.is_empty() && !s.plan().pit_done);
    }

    #[test]
    fn two_stops_no_alarm_before_last_chance() {
        // Lap 4 of 27, 76.2 on board, 7.37 per lap (today's refresh)
        let mut l = live(4, 0.2, 76.2);
        l.total_laps = 27;
        let mut saved = barcelona(27);
        saved.race.as_mut().unwrap().start_compound = Some("RM".into());
        let p = full(&saved, &l, 7.37);
        assert_eq!(p.stops_needed, 2);
        assert!(p.calls.iter().all(|c| c.level != "box"), "{:?}", p.calls);
        assert!(p.calls.iter().any(|c| c.text.starts_with("Box v kole")), "{:?}", p.calls);
    }

    #[test]
    fn tyre_change_detected_keeps_the_start_compound() {
        let mut s = Strategy::load(None);
        let mut race = barcelona(11).race.unwrap();
        race.start_compound = Some("RS".into());
        s.set_race(race);
        let p = s.plan();
        assert_eq!(p.compound.as_deref(), Some("RS"));
        assert!(p.mandatory_done);
        s.stopped(7);
        assert!(s.plan().tyre_question);
        s.tyres_detected(true);
        let p = s.plan();
        assert!(!p.tyre_question && p.pit_done);
        assert_eq!(s.saved.stints.len(), 2);
        assert_eq!(s.saved.stints[1].start, 7.0);
        assert_eq!(p.compound.as_deref(), Some("RS"));
        // A later detection does not overwrite the answer
        s.tyres_detected(false);
        assert_eq!(s.saved.stops[0].tyres, Some(true));
    }

    #[test]
    fn suggested_stop_splits_the_race() {
        // 23 laps, one stop, window 10–13: halfway would be 11.5 → 12
        let p = full(&barcelona(23), &live(1, 0.0, 100.0), 7.16);
        assert_eq!(p.laps_to_box, Some(p.target_lap.unwrap() - 1));
        // Mandatory stop only (10 laps, enough fuel): somewhere in the race, no fuel needed
        let p = full(&barcelona(10), &live(3, 0.0, 85.0), 7.16);
        assert!((3..=9).contains(&p.target_lap.unwrap()));
        assert!(p.next_stop_fuel.unwrap() < 0.5);
        // Out of fuel: box now
        let p = full(&barcelona(23), &live(14, 0.5, 4.0), 7.16);
        assert_eq!(p.laps_to_box, Some(0));
    }

    #[test]
    fn stop_across_the_lap_line_is_one_stop() {
        let mut s = Strategy::load(None);
        s.set_race(barcelona(27).race.unwrap());
        s.stopped(11);
        s.refuelled(12, 30.0);
        s.refuelled(12, 36.6);
        assert_eq!(s.saved.stops.len(), 1);
        assert!((s.saved.stops[0].fuel_added - 66.6).abs() < 1e-3);
    }

    #[test]
    fn standing_in_the_box_shows_fuel_to_add() {
        // Today, 2nd stop: in the box after lap 21 of 27, 7.0 in the tank, 7.71 per lap
        let mut saved = barcelona(27);
        saved.race.as_mut().unwrap().start_compound = Some("RM".into());
        saved.stops = vec![Stop { lap: 11, fuel_added: 66.6, tyres: Some(true) }, Stop { lap: 21, fuel_added: 0.0, tyres: None }];
        let mut l = live(22, 0.0, 7.0);
        l.total_laps = 27;
        l.standing = true;
        let p = full(&saved, &l, 7.71);
        // (6 + 0.3) × 7.71 − 7.0 = 41.6
        assert!((p.stop_add.unwrap() - 41.57).abs() < 0.05, "{:?}", p.stop_add);
        assert_eq!(p.stop_add_laps, Some(6));
        assert!(p.calls[0].text.starts_with("DOTANKUJ +41,6"), "{:?}", p.calls);
        // Already refuelled 20: the target stays the same, the add shrinks
        saved.stops[1].fuel_added = 20.0;
        let mut l2 = l;
        l2.fuel = 27.0;
        let p = full(&saved, &l2, 7.71);
        assert!((p.stop_add.unwrap() - 41.57).abs() < 0.05);
    }

    #[test]
    fn pit_loss_is_measured_per_track() {
        let mut s = Strategy::load(None);
        s.set_race(barcelona(27).race.unwrap());
        for lap in 2..=10 {
            s.lap_done(lap, 7.7, false, Some(104_200));
        }
        s.stopped(11);
        s.refuelled(12, 66.6);
        s.lap_done(11, 6.9, true, Some(119_507));
        s.lap_done(12, 7.1, true, Some(136_414));
        // 119.5 + 136.4 − 2 × 104.2 − 66.6 / 3 = 25.3 s
        let v = s.saved.calibration["pit|Barcelona"];
        assert!((v - 25.3).abs() < 0.1, "{v}");
        assert_eq!(s.plan().pit_loss_source, "measured");
    }

    #[test]
    fn a_new_race_keeps_the_old_one_in_the_history() {
        let dir = std::env::temp_dir().join(format!("boxbox-hist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Strategy::load(Some(dir.join("strategy.json")));
        s.set_race(barcelona(27).race.unwrap());
        for lap in 1..=5 {
            s.lap_done(lap, 7.7, false, Some(104_000));
        }
        s.reset_progress();
        let files: Vec<_> = std::fs::read_dir(dir.join("strategy-history")).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        let text = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(text.contains("IMSA Barcelona") && text.contains("104000"));
        assert!(s.saved.fuel_laps.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn running_dry_calls_box_now() {
        let p = plan(&barcelona(23), &live(14, 0.5, 4.0), Some(7.16), None);
        assert_eq!(p.calls[0].text, "BOX TEĎ – palivo nevystačí na další kolo!");
    }

    #[test]
    fn practice_laps_feed_the_race_plan() {
        let mut s = Strategy::load(None);
        let race = Race {
            name: "Bazarovky".into(),
            track: "Nürburgring GP".into(),
            laps: Some(20),
            tyre_wear: Some(6.0),
            fuel_mult: Some(3.0),
            refuel_rate: Some(3.0),
            pit_required: true,
            ..Default::default()
        };
        s.set_race(race.clone());
        s.update_live(Live { car_code: 3475, capacity: 65.0, fuel: 65.0, ..Default::default() });
        s.set_practice(true);
        assert!((s.practice.saved.wear_mult - 6.0).abs() < 1e-6);
        assert_eq!(s.practice.saved.track, "Nürburgring GP");
        // Practice: soft 8 laps (0.30 s/lap), medium 10 laps (0.10 s/lap, 1 s slower)
        s.practice.new_set(Some("SS"));
        for age in 0..8 {
            s.lap_finished(age as i16 + 1, 6.5, false, Some(160_000 + 300 * age), true);
        }
        s.practice.new_set(Some("SM"));
        for age in 0..10 {
            s.lap_finished(age as i16 + 9, 6.5, false, Some(161_000 + 100 * age), true);
        }
        assert!(s.saved.fuel_laps.is_empty(), "practice laps must not go to the race");
        s.practice.set_life(3475, "Nürburgring GP", "SS", 6.0, Some(8));
        s.practice.set_life(3475, "Nürburgring GP", "SM", 6.0, Some(14));
        s.set_practice(false);

        let (options, plan) = s.preview(&race);
        assert_eq!(plan.fuel_source, "practice");
        assert!((plan.fuel_per_lap.unwrap() - 6.5).abs() < 1e-4);
        let used = plan.practice_use.unwrap();
        assert_eq!(used.car, 3475);
        let got: Vec<(&str, usize, Option<u32>)> = used.compounds.iter().map(|c| (c.compound.as_str(), c.laps, c.life)).collect();
        assert_eq!(got, vec![("SS", 8, Some(8)), ("SM", 10, Some(14))]);
        let best = options.best.unwrap();
        assert!(!best.over_limit, "{best:?}");
        assert!(best.stints.iter().all(|t| t.laps <= if t.compound == "SS" { 8 } else { 14 }), "{best:?}");
        // What was entered for the race wins over practice
        let mut manual = race.clone();
        manual.tyres.insert("SS".into(), TyreSpec { life: Some(20), ..Default::default() });
        let (filled, _) = fill_from_practice(&s.practice, &manual, Some(3475));
        assert_eq!(filled.tyres["SS"].life, Some(20));
        assert!((filled.tyres["SS"].deg.unwrap() - 0.30).abs() < 0.01);
    }
}
