//! Race strategy planner: which tyres, how many stops, and at the end of which laps.
//!
//! Every possible plan (0–3 stops, every split of the remaining laps into stints, every compound
//! for every new set) is checked against the limits – tyre life per compound, fuel tank, mandatory
//! compound and stop – and scored by the time it costs:
//!
//! * per stop: the pit loss of the track (pit lane + standing) plus refuelling time,
//! * per lap: the pace difference of the compound,
//! * tyre degradation: every lap of tyre age costs `deg` seconds (a stint of n laps on new tyres
//!   costs deg·(0 + 1 + … + n−1)). A compound with a shorter life wears faster: its rate is scaled
//!   by (average life of the entered compounds / its life). A rate measured in practice replaces it.
//!
//! The best plan is the recommendation; the alternative is the best plan with other tyres or a
//! different number of stops. The search is small: 27 laps and 3 stops
//! are about 2 600 splits × 27 tyre choices.

use serde::Serialize;

pub const MAX_STOPS: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Compound {
    pub name: String,
    /// Laps a new set lasts (None = no limit)
    pub life: Option<u32>,
    /// Lap time compared with the reference compound, s (negative = faster)
    pub pace: f32,
    /// Degradation relative to the average (set by [`plan`] from the lives)
    pub wear_factor: f32,
    /// Measured degradation of this compound (practice), s per lap per lap of age; replaces
    /// `Input::deg × wear_factor`
    pub deg: Option<f32>,
}

impl Compound {
    pub fn new(name: &str, life: Option<u32>, pace: f32) -> Self {
        Compound { name: name.to_string(), life, pace, wear_factor: 1.0, deg: None }
    }
}

#[derive(Clone, Debug)]
pub struct Input {
    /// First lap still to be driven (the current lap)
    pub first_lap: i16,
    /// Laps still to be driven, the current one included
    pub laps: u32,
    /// Fuel at the start of `first_lap`
    pub fuel: f32,
    pub capacity: f32,
    pub fuel_per_lap: f32,
    /// Fuel left at the finish, in laps
    pub reserve: f32,
    /// Fuel per second when refuelling (0 = not counted)
    pub refuel_rate: f32,
    /// Time lost by a stop without refuelling, s
    pub pit_loss: f32,
    /// Seconds per lap per lap of tyre age
    pub deg: f32,
    /// Tyres on the car (None: free choice, before the start without a set starting compound)
    pub current: Option<String>,
    /// Laps already driven on the current tyres
    pub current_age: u32,
    /// The car is in the pit right now: the first stop happens before `first_lap` is driven
    pub stop_now: bool,
    /// Compound that still has to be used
    pub need_compound: Option<String>,
    /// A stop is still mandatory
    pub need_stop: bool,
    pub compounds: Vec<Compound>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PlannedStop {
    /// Box at the end of this lap (with `stop_now`: the lap before `first_lap`)
    pub lap: i16,
    pub compound: String,
    /// Fuel to add
    pub fuel: f32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Stint {
    pub compound: String,
    pub laps: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Option_ {
    pub stints: Vec<Stint>,
    pub stops: Vec<PlannedStop>,
    /// Estimated cost in seconds (only differences between options mean something)
    pub cost: f32,
    /// Slower than the recommendation by, s
    pub delta: f32,
    /// A limit had to be broken (tyre life): no plan fits the entered limits
    pub over_limit: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub best: Option<Option_>,
    pub alternative: Option<Option_>,
}

/// All ways to split `total` laps into `parts` stints of at least `min` laps (the first may be 0
/// only when `first_min` is 0).
fn splits(total: u32, parts: usize, first_min: u32, out: &mut Vec<Vec<u32>>, cur: &mut Vec<u32>) {
    if parts == 1 {
        let min = if cur.is_empty() { first_min } else { 1 };
        if total >= min {
            cur.push(total);
            out.push(cur.clone());
            cur.pop();
        }
        return;
    }
    let min = if cur.is_empty() { first_min } else { 1 };
    for n in min..=total.saturating_sub(parts as u32 - 1) {
        cur.push(n);
        splits(total - n, parts - 1, first_min, out, cur);
        cur.pop();
    }
}

fn deg_cost(deg: f32, age: u32, laps: u32) -> f32 {
    // age + (age+1) + … + (age+laps−1)
    deg * (laps as f32 * age as f32 + (laps as f32 * (laps as f32 - 1.0)) / 2.0)
}

struct Eval {
    cost: f32,
    over: f32,
    stops: Vec<PlannedStop>,
}

fn evaluate(inp: &Input, lens: &[u32], tyres: &[&Compound]) -> Option<Eval> {
    let fpl = inp.fuel_per_lap;
    let mut fuel = inp.fuel;
    let mut cost = 0.0;
    let mut over = 0.0;
    let mut lap = inp.first_lap - 1; // last lap done
    let mut stops = Vec::new();
    let last = lens.len() - 1;
    for (i, (&n, t)) in lens.iter().zip(tyres).enumerate() {
        let fresh = i > 0 || inp.stop_now || inp.current.is_none();
        let age = if fresh { 0 } else { inp.current_age };
        if i > 0 || inp.stop_now {
            // Stop before this stint: just enough fuel for it (and the reserve at the end)
            let need = (n as f32 + if i == last { inp.reserve } else { 0.05 }) * fpl;
            let add = (need - fuel).max(0.0);
            if fuel + add > inp.capacity + 0.01 {
                return None;
            }
            fuel += add;
            cost += inp.pit_loss + if inp.refuel_rate > 0.0 { add / inp.refuel_rate } else { 0.0 };
            stops.push(PlannedStop { lap, compound: t.name.clone(), fuel: add });
        } else {
            let need = (n as f32 + if i == last { inp.reserve } else { 0.05 }) * fpl;
            if fuel + 0.01 < need {
                return None;
            }
        }
        if let Some(life) = t.life {
            over += (age + n).saturating_sub(life) as f32;
        }
        cost += t.pace * n as f32 + deg_cost(t.deg.unwrap_or(inp.deg * t.wear_factor), age, n);
        // Tiny preference for keeping the same compound when everything else is equal
        if i > 0 && tyres[i - 1].name != t.name {
            cost += 0.001;
        }
        fuel -= n as f32 * fpl;
        lap += n as i16;
    }
    Some(Eval { cost, over, stops })
}

pub fn plan(inp: &Input) -> Options {
    if inp.laps == 0 || inp.fuel_per_lap <= 0.0 || inp.compounds.is_empty() {
        return Options::default();
    }
    // Shorter life = faster wear
    let lives: Vec<f32> = inp.compounds.iter().filter_map(|c| c.life).map(|l| l as f32).collect();
    let avg = if lives.is_empty() { 0.0 } else { lives.iter().sum::<f32>() / lives.len() as f32 };
    let compounds: Vec<Compound> = inp
        .compounds
        .iter()
        .map(|c| Compound { wear_factor: c.life.filter(|l| *l > 0 && avg > 0.0).map_or(1.0, |l| avg / l as f32), ..c.clone() })
        .collect();
    let inp = &Input { compounds, ..inp.clone() };
    let current = inp.current.as_ref().and_then(|c| inp.compounds.iter().find(|x| &x.name == c)).cloned();
    let current_compound = current.clone().or_else(|| inp.current.as_ref().map(|c| Compound::new(c, None, 0.0)));
    // (stops, cost, over, option)
    let mut found: Vec<(usize, f32, f32, Option_)> = Vec::new();
    let min_stops = usize::from(inp.stop_now || inp.need_stop);
    for k in min_stops.max(if inp.stop_now { 1 } else { 0 })..=MAX_STOPS {
        let parts = if inp.stop_now { k } else { k + 1 };
        if parts == 0 || parts as u32 > inp.laps {
            continue;
        }
        let mut all = Vec::new();
        splits(inp.laps, parts, 1, &mut all, &mut Vec::new());
        // Tyre choices: the first stint keeps the current tyres (unless free / in the pit)
        let free_first = inp.stop_now || current_compound.is_none();
        let choices = inp.compounds.len();
        let combos = choices.pow(if free_first { parts } else { parts - 1 } as u32);
        for combo in 0..combos {
            let mut tyres: Vec<&Compound> = Vec::with_capacity(parts);
            let mut c = combo;
            for i in 0..parts {
                if i == 0 && !free_first {
                    tyres.push(current_compound.as_ref().unwrap());
                } else {
                    tyres.push(&inp.compounds[c % choices]);
                    c /= choices;
                }
            }
            if let Some(m) = &inp.need_compound {
                if !tyres.iter().any(|t| &t.name == m) {
                    continue;
                }
            }
            for lens in &all {
                if let Some(e) = evaluate(inp, lens, &tyres) {
                    let opt = Option_ {
                        stints: lens.iter().zip(&tyres).map(|(&laps, t)| Stint { compound: t.name.clone(), laps }).collect(),
                        stops: e.stops,
                        cost: e.cost,
                        delta: 0.0,
                        over_limit: e.over > 0.0,
                    };
                    found.push((k, e.cost, e.over, opt));
                }
            }
        }
    }
    if found.is_empty() {
        return Options::default();
    }
    // Plans within the tyre limits first; if none fits, the one breaking them the least
    let min_over = found.iter().map(|f| f.2).fold(f32::MAX, f32::min);
    found.retain(|f| f.2 <= min_over);
    let key = |f: &(usize, f32, f32, Option_)| (f.1, f.0);
    let best_i = (0..found.len()).min_by(|&a, &b| key(&found[a]).partial_cmp(&key(&found[b])).unwrap()).unwrap();
    let best = found[best_i].3.clone();
    let best_stops = found[best_i].0;
    // Which sets are used, regardless of the order (only the order is no real alternative)
    let tyres_of = |o: &Option_| {
        let mut t = o.stints.iter().map(|s| s.compound.clone()).collect::<Vec<_>>();
        t.sort();
        t
    };
    // A real alternative: other tyres or another number of stops (not just a lap earlier or later)
    let alt_i = (0..found.len())
        .filter(|&i| found[i].0 != best_stops || tyres_of(&found[i].3) != tyres_of(&best))
        .min_by(|&a, &b| key(&found[a]).partial_cmp(&key(&found[b])).unwrap());
    let alternative = alt_i.map(|i| {
        let mut o = found[i].3.clone();
        o.delta = o.cost - best.cost;
        o
    });
    Options { best: Some(best), alternative }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tyres(rs: Option<u32>, rm: Option<u32>, rh: Option<u32>) -> Vec<Compound> {
        vec![Compound::new("RS", rs, 0.0), Compound::new("RM", rm, 0.0), Compound::new("RH", rh, 0.0)]
    }

    fn race(laps: u32, fpl: f32) -> Input {
        Input {
            first_lap: 1,
            laps,
            fuel: 100.0,
            capacity: 100.0,
            fuel_per_lap: fpl,
            reserve: 0.3,
            refuel_rate: 3.0,
            pit_loss: 25.0,
            deg: 0.2,
            current: Some("RM".into()),
            current_age: 0,
            stop_now: false,
            need_compound: Some("RS".into()),
            need_stop: true,
            compounds: tyres(Some(10), Some(14), None),
        }
    }

    #[test]
    fn amg_barcelona_two_stops_respecting_tyre_life() {
        // Today's race: 27 laps, 7.71 per lap, start on RM, RS mandatory
        let o = plan(&race(27, 7.71));
        let best = o.best.unwrap();
        assert!(!best.over_limit);
        assert_eq!(best.stops.len(), 2, "{best:?}");
        assert_eq!(best.stints[0].compound, "RM");
        assert!(best.stints.iter().any(|s| s.compound == "RS"));
        for s in &best.stints {
            let life = match s.compound.as_str() {
                "RS" => 10,
                "RM" => 14,
                _ => 99,
            };
            assert!(s.laps <= life, "{best:?}");
        }
        // No stop in lap 8 with these tyres: the first stint uses the medium tyres for long
        assert!(best.stops[0].lap >= 10, "{best:?}");
        // Fuel at the stops: exactly what the next stint needs
        let total_fuel: f32 = best.stops.iter().map(|s| s.fuel).sum();
        assert!((100.0 + total_fuel - (27.3 * 7.71)).abs() < 0.5, "{total_fuel}");
        let alt = o.alternative.unwrap();
        assert!(alt.delta >= 0.0);
    }

    #[test]
    fn faster_softs_are_used_as_much_as_possible() {
        let mut inp = race(27, 7.71);
        inp.compounds[0].pace = -0.8;
        let best = plan(&inp).best.unwrap();
        let rs: u32 = best.stints.iter().filter(|s| s.compound == "RS").map(|s| s.laps).sum();
        assert!(rs >= 18, "{best:?}");
    }

    #[test]
    fn ferrari_one_stop_is_possible() {
        let mut inp = race(27, 7.05);
        inp.compounds = tyres(None, None, None);
        let o = plan(&inp);
        let all = [o.best.clone().unwrap(), o.alternative.clone().unwrap()];
        assert!(all.iter().any(|x| x.stops.len() == 1), "{all:?}");
    }

    #[test]
    fn stop_now_tells_how_much_to_add() {
        // In the pit after lap 21 of 27: 6 laps to go, 7.0 in the tank
        let mut inp = race(6, 7.71);
        inp.first_lap = 22;
        inp.fuel = 7.0;
        inp.stop_now = true;
        inp.need_compound = None;
        inp.need_stop = false;
        let best = plan(&inp).best.unwrap();
        assert_eq!(best.stops.len(), 1);
        assert_eq!(best.stops[0].lap, 21);
        // (6 + 0.3) × 7.71 − 7.0 = 41.6 (today 48.8 went in: 7 too much)
        assert!((best.stops[0].fuel - 41.57).abs() < 0.05, "{best:?}");
    }

    #[test]
    fn too_short_tyre_life_still_gives_a_plan() {
        let mut inp = race(27, 7.71);
        inp.compounds = tyres(Some(3), Some(3), Some(3));
        let best = plan(&inp).best.unwrap();
        assert!(best.over_limit);
    }

    #[test]
    fn splits_cover_everything() {
        let mut all = Vec::new();
        splits(5, 2, 1, &mut all, &mut Vec::new());
        assert_eq!(all, vec![vec![1, 4], vec![2, 3], vec![3, 2], vec![4, 1]]);
    }
}
