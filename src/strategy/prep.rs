//! Race preparation: what is known for the chosen race and what practice still has to deliver.
//!
//! Every item is "ok", "warn" (partly there, or an estimate) or "missing", with what was found
//! and what to do. The figures come from the race settings, the practice laps on the race track
//! with the race multipliers (the car's active setup), the measured pit loss, the setup card and
//! the shift points from driving.

use super::practice::{PracticeLap, Summary};
use super::Race;
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub label: String,
    /// "ok", "warn" or "missing"
    pub status: &'static str,
    pub detail: String,
    /// What to do (empty when done)
    pub todo: String,
}

/// Everything the checklist looks at.
pub struct Input<'a> {
    pub race: &'a Race,
    /// Practice figures of the car on the race track with the race multipliers
    pub summary: Option<&'a Summary>,
    /// Clean, counted practice laps of the car on the race track (any multipliers), oldest first
    pub laps: Vec<&'a PracticeLap>,
    /// "manual", "measured" or "default"
    pub pit_loss_source: &'a str,
    pub pit_loss: f32,
    /// Active setup card: name and whether it is BoP
    pub setup: Option<(String, bool)>,
    pub shift_points: bool,
    pub car_name: Option<String>,
}

fn item(id: &str, label: &str, status: &'static str, detail: String, todo: &str) -> Item {
    Item { id: id.into(), label: label.into(), status, detail, todo: if status == "ok" { String::new() } else { todo.into() } }
}

/// "1 kolo", "3 kola", "5 kol" (with an adjective: "1 čisté kolo", "3 čistá kola", "5 čistých kol")
fn laps_cz(n: usize, adj: Option<[&str; 3]>) -> String {
    let i = match n {
        1 => 0,
        2..=4 => 1,
        _ => 2,
    };
    let noun = ["kolo", "kola", "kol"][i];
    match adj {
        Some(a) => format!("{n} {} {noun}", a[i]),
        None => format!("{n} {noun}"),
    }
}

fn f1(v: f32) -> String {
    format!("{v:.1}").replace('.', ",")
}

fn lap_time(ms: i32) -> String {
    format!("{}:{:06.3}", ms / 60000, (ms % 60000) as f32 / 1000.0).replace('.', ",")
}

/// Compounds of the race: those in the race settings, else the kind of the mandatory/start one.
fn race_compounds(race: &Race) -> Vec<String> {
    if !race.tyres.is_empty() {
        return race.tyres.keys().cloned().collect();
    }
    let kind = race.mandatory.as_deref().or(race.start_compound.as_deref()).and_then(|c| c.chars().next()).unwrap_or('R');
    ['S', 'M', 'H'].iter().map(|c| format!("{kind}{c}")).collect()
}

pub fn checklist(inp: &Input) -> Vec<Item> {
    let r = inp.race;
    let mut out = Vec::new();
    // A car that burns no fuel (karts): nothing to measure or set for fuel
    let no_fuel = inp.laps.len() >= 3 && inp.laps.iter().all(|l| l.fuel < 0.01);

    // 1. Race settings
    let mut missing = Vec::new();
    if r.laps.is_none() {
        missing.push("počet kol");
    }
    if r.tyre_wear.is_none() {
        missing.push("opotřebení gum");
    }
    if r.fuel_mult.is_none() && !no_fuel {
        missing.push("spotřeba paliva");
    }
    if r.refuel_rate.is_none() && !no_fuel {
        missing.push("rychlost tankování");
    }
    out.push(item(
        "race",
        "Nastavení závodu",
        if missing.is_empty() { "ok" } else { "missing" },
        if missing.is_empty() {
            format!(
                "{} kol · opotř. ×{}{}{}",
                r.laps.unwrap_or(0),
                f1(r.tyre_wear.unwrap_or(0.0)),
                r.fuel_mult.map(|f| format!(" · spotř. ×{}", f1(f))).unwrap_or_default(),
                r.mandatory.as_deref().map(|m| format!(" · povinně {m}")).unwrap_or_default()
            )
        } else {
            format!("chybí: {}", missing.join(", "))
        },
        "Doplň v „Závod…“ podle pravidel šampionátu.",
    ));

    // 2. Practice on the track with the race multipliers
    let compounds_laps: usize = inp.summary.map_or(0, |s| s.compounds.iter().map(|c| c.laps).sum());
    out.push(item(
        "practice",
        "Trénink na trati",
        if compounds_laps >= 5 {
            "ok"
        } else if compounds_laps > 0 {
            "warn"
        } else {
            "missing"
        },
        format!(
            "{} na {}{} se stejnými násobiči",
            laps_cz(compounds_laps, Some(["čisté", "čistá", "čistých"])),
            r.track,
            inp.car_name.as_deref().map(|c| format!(" ({c})")).unwrap_or_default()
        ),
        "V Tréninku dej „Trénovat na závod“ (trať a násobiče závodu) a odjeď aspoň 5 čistých kol.",
    ));

    // 3. Each compound: laps, wear per lap, life
    for c in race_compounds(r) {
        let st = inp.summary.and_then(|s| s.compounds.iter().find(|x| x.compound == c));
        let laps = st.map_or(0, |s| s.laps);
        let deg = st.and_then(|s| s.deg);
        let life = st.and_then(|s| s.life).or_else(|| r.tyres.get(&c).and_then(|t| t.life));
        let mandatory = r.mandatory.as_deref() == Some(c.as_str());
        let status = if laps >= 3 && deg.is_some() && life.is_some() {
            "ok"
        } else if laps > 0 || life.is_some() {
            "warn"
        } else {
            "missing"
        };
        let mut parts = vec![laps_cz(laps, None)];
        if let Some(b) = st.and_then(|s| s.best_ms) {
            parts.push(format!("nejlepší {}", lap_time(b)));
        }
        parts.push(match deg {
            Some(d) => format!("ztráta {} s/kolo", format!("{d:.2}").replace('.', ",")),
            None => "ztráta za kolo zatím neznámá".into(),
        });
        parts.push(match life {
            Some(l) => format!("vydrží {}", laps_cz(l as usize, None)),
            None => "výdrž nezadaná".into(),
        });
        let todo = if laps < 3 || deg.is_none() {
            "Odjeď na jedné sadě aspoň 4 kola v kuse (stáří gum ≥ 3), ať se změří ztráta za kolo."
        } else {
            "Zadej „Vydrží kol“ podle opotřebení v GT7 (Trénink → Výsledky)."
        };
        out.push(item(
            &format!("tyre-{c}"),
            &format!("Směs {c}{}", if mandatory { " (povinná)" } else { "" }),
            status,
            parts.join(" · "),
            todo,
        ));
    }

    // 4. Fuel per lap
    let fpl = inp.summary.and_then(|s| s.fuel_per_lap);
    out.push(item(
        "fuel",
        "Spotřeba na kolo",
        if fpl.is_some() || no_fuel { "ok" } else { "missing" },
        if no_fuel {
            "auto palivo nespotřebovává".into()
        } else {
            fpl.map_or("zatím neměřená".into(), |f| format!("{} l/kolo", format!("{f:.2}").replace('.', ",")))
        },
        "Změří se z celých kol bez tankování se stejnou spotřebou ×.",
    ));

    // 5. Pit loss
    out.push(item(
        "pit",
        "Ztráta zastávkou",
        match inp.pit_loss_source {
            "manual" | "measured" => "ok",
            _ => "warn",
        },
        format!(
            "{} s ({})",
            f1(inp.pit_loss),
            match inp.pit_loss_source {
                "manual" => "zadaná",
                "measured" => "změřená",
                _ => "odhad",
            }
        ),
        "Změří se při zastávce v závodě na této trati; nebo ji zadej v „Závod…“ (průjezd boxem proti kolu bez zastávky).",
    ));

    // 6. A long run (tyres and fuel over a whole stint)
    let longest = inp.summary.map_or(0, |s| s.compounds.iter().map(|c| c.max_age).max().unwrap_or(0));
    out.push(item(
        "stint",
        "Dlouhý stint",
        if longest >= 8 {
            "ok"
        } else if longest >= 4 {
            "warn"
        } else {
            "missing"
        },
        format!("nejdelší jízda na jedné sadě {}", laps_cz(longest as usize, None)),
        "Odjeď aspoň jeden stint 8+ kol na jedné sadě – ukáže opotřebení i spotřebu v závodním tempu.",
    ));

    // 7. Setup card and shift points
    out.push(item(
        "setup",
        "Karta setupu",
        if inp.setup.is_some() { "ok" } else { "warn" },
        match &inp.setup {
            Some((n, true)) => format!("„{n}“ (BoP)"),
            Some((n, false)) => format!("„{n}“"),
            None => "bez karty – kola se ukládají pod „bez setupu“".into(),
        },
        "Na stránce Nastavení auta vytvoř kartu (u závodu bez úprav zaškrtni BoP).",
    ));
    out.push(item(
        "shift",
        "Body řazení",
        if inp.shift_points { "ok" } else { "warn" },
        if inp.shift_points { "změřené z jízdy".into() } else { "jen podle kontrolky hry".into() },
        "V tréninku vytoč v každém stupni aspoň jednou plný plyn až k omezovači.",
    ));

    // 8. Consistency: the last 10 normal laps within 0.3 s
    let mut recent: Vec<i32> = inp.laps.iter().rev().take(10).map(|l| l.ms).collect();
    recent.sort_unstable();
    if let Some(&best) = recent.first() {
        recent.retain(|ms| (*ms as f32) <= best as f32 * 1.02);
    }
    let spread = if recent.len() >= 2 { (recent[recent.len() - 1] - recent[0]) as f32 / 1000.0 } else { 0.0 };
    out.push(item(
        "consistency",
        "Vyrovnanost",
        if recent.len() >= 10 && spread <= 0.3 {
            "ok"
        } else if recent.len() >= 5 && spread <= 0.8 {
            "warn"
        } else {
            "missing"
        },
        if recent.is_empty() {
            "zatím žádná čistá kola".into()
        } else {
            format!("{} v rozmezí {} s", laps_cz(recent.len(), None), format!("{spread:.2}").replace('.', ","))
        },
        "Cíl do závodu: 10 kol po sobě v rozmezí 0,3 s – nejdřív srovnej nejrozkolísanější zatáčky (Telemetrie → Vyrovnanost).",
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::super::practice::CompoundStats;
    use super::*;

    #[test]
    fn what_is_missing_for_abu_dhabi() {
        let race = Race {
            name: "F1 - VC Abu Dhabi".into(),
            track: "Yas Marina Circuit".into(),
            tyre_wear: Some(7.0),
            fuel_mult: Some(3.0),
            refuel_rate: Some(3.0),
            pit_required: true,
            mandatory: Some("RS".into()),
            ..Default::default()
        };
        let summary = Summary {
            compounds: vec![CompoundStats {
                compound: "RS".into(),
                laps: 4,
                best_ms: Some(94_218),
                typical_ms: Some(95_121),
                base: None,
                deg: None,
                pace: None,
                fuel_per_lap: Some(2.3),
                max_age: 7,
                life: None,
            }],
            fuel_per_lap: Some(2.3),
            ..Default::default()
        };
        let laps: Vec<PracticeLap> =
            [94_218, 95_121, 94_530, 96_072].iter().map(|ms| PracticeLap { ms: *ms, ..Default::default() }).collect();
        let items = checklist(&Input {
            race: &race,
            summary: Some(&summary),
            laps: laps.iter().collect(),
            pit_loss_source: "default",
            pit_loss: 25.0,
            setup: Some(("BoP".into(), true)),
            shift_points: false,
            car_name: None,
        });
        let st = |id: &str| items.iter().find(|i| i.id == id).map(|i| (i.status, i.detail.clone())).unwrap();
        assert_eq!(st("race").0, "missing", "laps not set");
        assert!(st("race").1.contains("počet kol"));
        assert_eq!(st("practice").0, "warn");
        assert_eq!(st("tyre-RS").0, "warn");
        assert!(st("tyre-RS").1.contains("nejlepší 1:34,218"), "{:?}", st("tyre-RS"));
        assert_eq!(st("tyre-RM").0, "missing");
        assert_eq!(st("fuel").0, "ok");
        assert_eq!(st("pit").0, "warn");
        assert_eq!(st("stint").0, "warn");
        assert_eq!(st("setup").0, "ok");
        assert_eq!(st("shift").0, "warn");
        assert_eq!(st("consistency").0, "missing", "{:?}", st("consistency"));
        assert!(items.iter().filter(|i| i.status == "ok").all(|i| i.todo.is_empty()));
    }
}
