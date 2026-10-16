//! Car setups as in the GT7 settings sheet, stored per car under a name.
//!
//! One setup per car is active. Practice laps and the shift points from driving are kept per
//! car *and* setup, so changing the gearbox or the downforce does not mix old and new data.
//! The telemetry has none of these values: the driver types them in (or leaves fields empty).
//!
//! Saved in `setups.json` next to `strategy.json`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Setup {
    /// Short id, unique per car ("s1", "s2" …); part of file names
    pub id: String,
    pub car: i32,
    pub name: String,
    pub note: String,
    /// Field key → value as typed ("12", "-1.5", "RS"); see the dashboard for the keys
    pub values: BTreeMap<String, String>,
    pub created: String,
    pub updated: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
struct Saved {
    setups: Vec<Setup>,
    /// Car → id of the active setup
    active: BTreeMap<i32, String>,
    /// Car → last number given out (ids are never reused: laps keep pointing at their setup)
    last_id: BTreeMap<i32, u32>,
}

pub struct Setups {
    saved: Saved,
    file: Option<PathBuf>,
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

impl Setups {
    pub fn load(file: Option<PathBuf>) -> Self {
        let saved =
            file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Setups { saved, file }
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&self.saved).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, file);
        }
    }

    pub fn for_car(&self, car: i32) -> Vec<&Setup> {
        self.saved.setups.iter().filter(|s| s.car == car).collect()
    }

    /// Id of the active setup of a car ("" = none)
    pub fn active_id(&self, car: i32) -> String {
        self.saved.active.get(&car).cloned().unwrap_or_default()
    }

    #[cfg(test)]
    pub fn active(&self, car: i32) -> Option<&Setup> {
        let id = self.saved.active.get(&car)?;
        self.saved.setups.iter().find(|s| s.car == car && &s.id == id)
    }

    pub fn name_of(&self, car: i32, id: &str) -> Option<String> {
        self.saved.setups.iter().find(|s| s.car == car && s.id == id).map(|s| s.name.clone())
    }

    /// Creates (empty id) or updates a setup; a new one becomes active. Returns its id.
    pub fn upsert(&mut self, mut s: Setup) -> String {
        s.name = s.name.trim().to_string();
        if s.name.is_empty() {
            s.name = format!("Setup {}", self.for_car(s.car).len() + 1);
        }
        s.values.retain(|_, v| !v.trim().is_empty());
        s.updated = now();
        let id = if let Some(old) = self.saved.setups.iter_mut().find(|x| x.car == s.car && x.id == s.id && !s.id.is_empty()) {
            s.created.clone_from(&old.created);
            *old = s.clone();
            s.id
        } else {
            let n = self.saved.last_id.entry(s.car).or_insert(0);
            *n += 1;
            s.id = format!("s{n}");
            s.created = now();
            self.saved.active.insert(s.car, s.id.clone());
            self.saved.setups.push(s.clone());
            s.id
        };
        self.save();
        id
    }

    /// Makes a setup active ("" = none).
    pub fn set_active(&mut self, car: i32, id: &str) {
        if id.is_empty() {
            self.saved.active.remove(&car);
        } else if self.saved.setups.iter().any(|s| s.car == car && s.id == id) {
            self.saved.active.insert(car, id.to_string());
        }
        self.save();
    }

    /// Removes the card (the laps driven with it stay, under its id).
    pub fn remove(&mut self, car: i32, id: &str) {
        self.saved.setups.retain(|s| !(s.car == car && s.id == id));
        if self.saved.active.get(&car).is_some_and(|a| a == id) {
            self.saved.active.remove(&car);
        }
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_update_activate_remove() {
        let mut s = Setups::load(None);
        let mut a = Setup { car: 211, name: " Nürburgring A ".into(), ..Default::default() };
        a.values.insert("downforce_r".into(), "320".into());
        a.values.insert("ride_f".into(), " ".into());
        let id = s.upsert(a);
        assert_eq!(id, "s1");
        assert_eq!(s.active_id(211), "s1");
        let mut got = s.active(211).unwrap().clone();
        assert_eq!(got.name, "Nürburgring A");
        assert!(!got.values.contains_key("ride_f"));
        // Second card, then back to the first
        let id2 = s.upsert(Setup { car: 211, name: "B".into(), ..Default::default() });
        assert_eq!((id2.as_str(), s.active_id(211).as_str()), ("s2", "s2"));
        got.values.insert("lsd_accel_r".into(), "25".into());
        s.upsert(got);
        s.set_active(211, "s1");
        assert_eq!(s.active(211).unwrap().values["lsd_accel_r"], "25");
        s.remove(211, "s1");
        assert_eq!(s.active_id(211), "");
        assert_eq!(s.for_car(211).len(), 1);
        assert_eq!(s.upsert(Setup { car: 211, ..Default::default() }), "s3"); // ids are never reused
    }
}
