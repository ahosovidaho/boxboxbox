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
    /// Fuel per lap to plan with before a lap has been measured
    pub fuel_per_lap: Option<f32>,
    pub reserve_laps: Option<f32>,
    pub wear_limit: Option<f32>,
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
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Saved {
    pub race: Option<Race>,
    pub stints: Vec<Stint>,
    pub stops: Vec<Stop>,
    pub fuel_laps: Vec<FuelLap>,
    /// Tyre wear per lap (%), key "car|track|compound|multiplier"
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
    /// Next stop: enter the pit at the end of one of these laps
    pub window: Option<[i16; 2]>,
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

fn call(level: &'static str, text: String) -> Call {
    Call { level, text }
}

pub struct Strategy {
    pub saved: Saved,
    file: Option<PathBuf>,
    live: Live,
}

impl Strategy {
    /// `file = None`: nothing is stored (demo).
    pub fn load(file: Option<PathBuf>) -> Self {
        let saved =
            file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Strategy { saved, file, live: Live::default() }
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

    /// Race restarted: stints, stops and fuel laps go, race and calibration stay.
    pub fn reset_progress(&mut self) {
        self.saved.stints.clear();
        self.saved.stops.clear();
        self.saved.fuel_laps.clear();
    }

    pub fn update_live(&mut self, live: Live) {
        self.live = live;
    }

    pub fn lap_done(&mut self, lap: i16, used: f32, refuel: bool) {
        if lap < 1 || used <= 0.0 {
            return;
        }
        self.saved.fuel_laps.retain(|l| l.lap != lap);
        self.saved.fuel_laps.push(FuelLap { lap, used, refuel });
        if self.saved.fuel_laps.len() > 200 {
            self.saved.fuel_laps.remove(0);
        }
        self.save();
    }

    /// Fuel went up during `lap` (in the pit).
    pub fn refuelled(&mut self, lap: i16, added: f32) {
        match self.saved.stops.last_mut() {
            Some(s) if s.lap == lap => s.fuel_added += added,
            _ => self.saved.stops.push(Stop { lap, fuel_added: added, tyres: None }),
        }
    }

    /// The car stood still in a race lap long enough to be a pit stop (tyres only, no fuel).
    pub fn stopped(&mut self, lap: i16) {
        if !self.saved.stops.last().is_some_and(|s| s.lap == lap) {
            self.saved.stops.push(Stop { lap, fuel_added: 0.0, tyres: None });
            self.save();
        }
    }

    fn stint_mut(&mut self) -> &mut Stint {
        if self.saved.stints.is_empty() {
            self.saved.stints.push(Stint::default());
        }
        self.saved.stints.last_mut().unwrap()
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
        let compound = self.saved.stints.last()?.compound.clone()?;
        Some(format!("{}|{}|{}|{}", self.live.car_code, race.track, compound, race.tyre_wear.unwrap_or(1.0)))
    }

    /// Clean laps (no refuelling, not the first lap from the grid); the first lap only if nothing else.
    pub fn fuel_per_lap(&self) -> Option<f32> {
        // The lap with a stop and the one after it (pit exit) burn less than a racing lap
        let pit = |l: &FuelLap| l.refuel || self.saved.stops.iter().any(|s| s.lap == l.lap || s.lap == l.lap - 1);
        let clean: Vec<f32> = self.saved.fuel_laps.iter().filter(|l| !pit(l) && l.lap > 1).map(|l| l.used).collect();
        let pick =
            if clean.is_empty() { self.saved.fuel_laps.iter().filter(|l| !pit(l)).map(|l| l.used).collect::<Vec<_>>() } else { clean };
        let recent = &pick[pick.len().saturating_sub(FUEL_LAPS)..];
        (!recent.is_empty()).then(|| recent.iter().sum::<f32>() / recent.len() as f32)
    }

    pub fn plan(&self) -> Plan {
        plan(&self.saved, &self.live, self.fuel_per_lap(), self.calibration_key().and_then(|k| self.saved.calibration.get(&k).copied()))
    }
}

/// Wear per lap of a stint: line through the origin (new tyres = 0 %) fitted to the readings.
fn wear_rate(stint: &Stint) -> Option<f32> {
    let pts: Vec<(f32, f32)> = stint.wear.iter().map(|m| (m.at - stint.start, m.pct)).filter(|(x, _)| *x >= 0.5).collect();
    let sxx: f32 = pts.iter().map(|(x, _)| x * x).sum();
    (sxx > 0.0).then(|| pts.iter().map(|(x, y)| x * y).sum::<f32>() / sxx)
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
    let so_far = (driven >= 1.0 && saved.stops.is_empty() && live.capacity > live.fuel).then(|| (live.capacity - live.fuel) / driven);
    let (fpl, source) = match (measured_fpl, race.fuel_per_lap, so_far) {
        (Some(f), ..) => (Some(f), "measured"),
        (None, Some(f), _) => (Some(f), "manual"),
        (None, None, Some(f)) => (Some(f), "so_far"),
        _ => (None, ""),
    };
    p.fuel_per_lap = fpl;
    p.fuel_source = source;

    // ---- Tyres ----
    let stint = saved.stints.last();
    p.compound = stint.and_then(|s| s.compound.clone());
    p.stint_laps = stint.map_or(driven, |s| (driven - s.start).max(0.0));
    p.mandatory_done = match &race.mandatory {
        Some(m) => saved.stints.iter().any(|s| s.compound.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(m))),
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
            if p.stops_needed == 1 && earliest > latest {
                p.stops_needed = 2;
            }
            // latest < current lap: the fuel runs out before the pit ("box now")
            p.window = Some([earliest.min(latest), latest]);
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

    // ---- Calls ----
    let mut calls = Vec::new();
    if p.tyre_question {
        calls.push(call("warn", "Zastávka: nasadil jsi nové gumy? Zvol směs, nebo „stejné“.".into()));
    }
    if race.name.is_empty() {
        calls.push(call("info", "Vyber závod, ať vím počet kol a pravidla.".into()));
    }
    match (p.window, p.fuel_margin_laps) {
        (Some([a, b]), _) if p.stops_needed > 0 => {
            let lap = live.lap;
            let add = p.add_now.map_or(String::new(), |v| format!(" · dotankovat {}", fmt1(v)));
            if b < lap {
                calls.push(call("box", "BOX TEĎ – palivo nevystačí na další kolo!".into()));
            } else if (a..=b).contains(&lap) {
                calls.push(call("box", format!("Okno otevřené: box na konci tohoto kola{add}")));
            } else {
                let laps = if a == b { format!("{a}") } else { format!("{a}–{b}") };
                calls.push(call("info", format!("Box v kole {laps}")));
            }
            if p.stops_needed > 1 {
                calls.push(call("warn", format!("Palivo vyžaduje {} zastávky", p.stops_needed)));
            }
        }
        (_, Some(m)) => calls.push(call("info", format!("Palivo do cíle stačí (+{} kola)", fmt1(m)))),
        _ if total.is_some() && fpl.is_none() => calls.push(call("info", "Spotřeba: změřím po prvním kole".into())),
        _ => {}
    }
    if race.pit_required && !p.pit_done {
        calls.push(call("warn", "Povinná zastávka ještě neproběhla".into()));
    }
    if let (Some(m), false) = (&race.mandatory, p.mandatory_done) {
        calls.push(call("warn", format!("Povinná směs {m} ještě nebyla použita")));
    }
    if p.compound.is_none() && !race.name.is_empty() {
        calls.push(call("info", "Zvol směs gum, na kterých jedeš".into()));
    }
    if let (Some(w), Some(l)) = (p.wear_laps_left, laps_left) {
        if w < l {
            calls.push(call("warn", format!("Gumy na limitu {} % za ~{} kol", p.wear_limit as i32, fmt1(w))));
        }
    }
    p.calls = calls;
    p
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

    fn live(lap: i16, frac: f32, fuel: f32) -> Live {
        Live { lap, total_laps: 0, frac, fuel, capacity: 100.0, car_code: 1 }
    }

    #[test]
    fn one_stop_window_matches_hand_calculation() {
        // 23 laps, 7.16 per lap, full tank at the start of lap 1
        let p = plan(&barcelona(23), &live(1, 0.0, 100.0), Some(7.16), None);
        assert_eq!(p.stops_needed, 1);
        // earliest: 23 + 0.3 − 100/7.16 = 9.33 → 10; latest: 100/7.16 − 0.15 = 13.8 → 13
        assert_eq!(p.window, Some([10, 13]));
        assert!(p.calls.iter().any(|c| c.text == "Box v kole 10–13"));
    }

    #[test]
    fn in_window_tells_how_much_to_add() {
        // Lap 11 just started: 10 laps done, fuel 100 − 10 × 7.16
        let p = plan(&barcelona(23), &live(11, 0.0, 28.4), Some(7.16), None);
        assert_eq!(p.window, Some([11, 13]));
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
        s.lap_done(1, 6.73, false);
        assert!((s.fuel_per_lap().unwrap() - 6.73).abs() < 1e-4); // only lap 1 so far
        s.lap_done(2, 7.18, false);
        s.lap_done(3, 7.08, false);
        s.lap_done(4, 3.0, true);
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
        s.lap_done(2, 7.1, false);
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
            s.lap_done(lap, used, false);
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
        let p = plan(&barcelona(27), &l, Some(7.0), None);
        assert_eq!(p.window, Some([14, 14]));
        assert!(p.calls.iter().any(|c| c.text == "Box v kole 14"));
    }

    #[test]
    fn running_dry_calls_box_now() {
        let p = plan(&barcelona(23), &live(14, 0.5, 4.0), Some(7.16), None);
        assert_eq!(p.calls[0].text, "BOX TEĎ – palivo nevystačí na další kolo!");
    }
}
