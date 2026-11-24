//! Races driven in race mode, kept for the recap.
//!
//! A race is archived to `strategy-history/` when it is restarted or another race is chosen
//! (see [`super::Strategy::reset_progress`]). Laps are recorded only in race mode (in practice
//! mode they go to the practice), so every archive with laps is a race driven as a race. The
//! list gives a summary per race; one race comes back whole, for the recap page to draw.

use super::Saved;
use serde::Serialize;
use std::path::Path;

/// Fewer laps than this: a start that was abandoned, not a race
const MIN_LAPS: usize = 2;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Summary {
    /// File name without `.json` ("current" for the race in `strategy.json`)
    pub id: String,
    /// "2026-11-23 22:43"
    pub at: String,
    pub name: String,
    pub track: String,
    pub car: Option<i32>,
    pub car_name: Option<String>,
    /// Laps driven, laps of the race
    pub laps: i16,
    pub race_laps: Option<u32>,
    pub finished: bool,
    /// Sum of the lap times, best lap, ms
    pub total_ms: i64,
    pub best_ms: Option<i32>,
    pub stops: usize,
    /// Compounds of the stints in order ("RM", "RS", "RS")
    pub compounds: Vec<String>,
}

/// Summary of a race, or None when it has too few laps.
pub fn summary(id: &str, at: &str, car: Option<i32>, s: &Saved) -> Option<Summary> {
    let mut laps: Vec<_> = s.fuel_laps.iter().filter(|l| l.ms.is_some_and(|m| m > 0)).collect();
    if laps.len() < MIN_LAPS {
        return None;
    }
    laps.sort_by_key(|l| l.lap);
    let race = s.race.as_ref();
    let done = laps.last().map_or(0, |l| l.lap);
    let race_laps = race.and_then(|r| r.laps);
    let mut compounds: Vec<String> = s.stints.iter().map(|x| x.compound.clone().unwrap_or_else(|| "?".into())).collect();
    if compounds.is_empty() {
        compounds.extend(race.and_then(|r| r.start_compound.clone()));
    }
    Some(Summary {
        id: id.into(),
        at: at.into(),
        name: race.map_or_else(|| "bez závodu".into(), |r| r.name.clone()),
        track: race.map(|r| r.track.clone()).unwrap_or_default(),
        car,
        car_name: car.and_then(crate::telemetry::cars::name).map(str::to_string),
        laps: done,
        race_laps,
        finished: race_laps.is_some_and(|n| done as u32 >= n),
        total_ms: laps.iter().map(|l| l.ms.unwrap_or(0) as i64).sum(),
        best_ms: laps.iter().filter_map(|l| l.ms).min(),
        stops: s.stops.len(),
        compounds,
    })
}

/// "2026-11-23_22-43-34_IMSA___Spa__5_5__auto3531" → ("2026-11-23 22:43", Some(3531))
fn parse_name(stem: &str) -> (String, Option<i32>) {
    let at = match (stem.get(..10), stem.get(11..16)) {
        (Some(d), Some(t)) if stem.as_bytes().get(10) == Some(&b'_') => format!("{d} {}", t.replace('-', ":")),
        _ => String::new(),
    };
    let car = stem.rsplit_once("_auto").and_then(|(_, c)| c.parse::<i32>().ok()).filter(|c| *c > 0);
    (at, car)
}

/// The archived races in `dir`, newest first. A race archived twice with nothing new (restarted
/// without driving) is listed once, at the time it was first kept.
pub fn list(dir: &Path) -> Vec<Summary> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<Summary> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let stem = path.file_stem()?.to_str()?.to_string();
            if path.extension()? != "json" {
                return None;
            }
            let saved: Saved = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            // The car in the file name is the one driven when the race was put away – the race
            // car only if it was not changed after it; newer files say it themselves
            let (at, car) = parse_name(&stem);
            summary(&stem, &at, saved.car.or(car), &saved)
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert((s.name.clone(), s.laps, s.total_ms, s.best_ms)));
    out.reverse();
    out
}

/// One archived race, or None (unknown id, or an id that is not a plain file name).
pub fn load(dir: &Path, id: &str) -> Option<Saved> {
    if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{id}.json"))).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::{FuelLap, Race, Stint, Stop};

    fn spa() -> Saved {
        let ms = [136_169, 131_142, 132_473, 147_941, 170_489, 129_969, 129_840];
        Saved {
            race: Some(Race { name: "IMSA - Spa (5/5)".into(), track: "Spa-Francorchamps".into(), laps: Some(7), ..Default::default() }),
            stints: ["RM", "RS"].iter().map(|c| Stint { compound: Some((*c).into()), ..Default::default() }).collect(),
            stops: vec![Stop { lap: 4, fuel_added: 99.0, tyres: Some(true) }],
            fuel_laps: ms
                .iter()
                .enumerate()
                .map(|(i, m)| FuelLap { lap: i as i16 + 1, used: 11.0, refuel: false, ms: Some(*m), fuel: None })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn races_are_listed_newest_first_without_abandoned_starts() {
        let dir = std::env::temp_dir().join(format!("history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("2026-11-23_22-43-34_IMSA___Spa__5_5__auto3531.json"), serde_json::to_string(&spa()).unwrap()).unwrap();
        let mut start = spa();
        start.fuel_laps.truncate(1);
        start.stops.clear();
        std::fs::write(dir.join("2026-11-23_21-51-57_IMSA___Spa__5_5__auto3531.json"), serde_json::to_string(&start).unwrap()).unwrap();
        let mut older = spa();
        older.fuel_laps.truncate(3);
        std::fs::write(dir.join("2026-11-17_22-28-29_F1___VC_Abu_Dhabi__5_5__auto0.json"), serde_json::to_string(&older).unwrap()).unwrap();
        // Archived again later with nothing new: listed once, the first time
        std::fs::write(dir.join("2026-11-18_19-00-00_F1___VC_Abu_Dhabi__5_5__auto3562.json"), serde_json::to_string(&older).unwrap())
            .unwrap();
        let l = list(&dir);
        assert_eq!(l.iter().map(|s| s.at.as_str()).collect::<Vec<_>>(), ["2026-11-23 22:43", "2026-11-17 22:28"]);
        let s = &l[0];
        assert_eq!((s.laps, s.finished, s.stops, s.car, s.best_ms), (7, true, 1, Some(3531), Some(129_840)));
        assert_eq!(s.compounds, ["RM", "RS"]);
        assert_eq!(s.total_ms, 978_023);
        assert!(!l[1].finished && l[1].car.is_none());
        assert!(load(&dir, &s.id).is_some());
        assert!(load(&dir, "../strategy").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
