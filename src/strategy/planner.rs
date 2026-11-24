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
//! * fuel on board: a lighter car is faster – every litre costs `fuel_s` seconds a lap (Yas
//!   Marina, F3500: 0.033 s/l, a full tank ≈ 3 s a lap). Refuelling just enough for the next
//!   stint keeps the car light; more stops with less fuel can win.
//!
//! Each option carries its risks ([`Option_::risks`]): fuel with little margin, a stint at the
//! end of the tyre life.
//!
//! The best plan is the recommendation. The alternative is the best plan with a different
//! number of stops – the real strategic choice (one stop less on harder tyres, one more on
//! softer); only when there is none, the best plan with other tyres. Each option carries what its
//! cost is made of ([`Parts`]), so the difference to the recommendation can be explained. The
//! search is small: 27 laps and 3 stops are about 2 600 splits × 27 tyre choices.

use serde::Serialize;

pub const MAX_STOPS: usize = 3;
/// When no plan with up to [`MAX_STOPS`] keeps the tyres within their life (short life, long
/// race), plans with up to this many stops are tried, with stints as even as the sets allow
pub const MAX_STOPS_LIFE: usize = 6;

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
    /// Fuel left on arriving at a stop, in laps
    pub stop_reserve: f32,
    /// Fuel per second when refuelling (0 = not counted)
    pub refuel_rate: f32,
    /// Time lost by a stop without refuelling, s
    pub pit_loss: f32,
    /// Seconds per lap per lap of tyre age
    pub deg: f32,
    /// Lap time per litre on board, s (0 = not counted)
    pub fuel_s: f32,
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

/// What the cost of a plan is made of, s
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Parts {
    /// Pit lane and standing (pit loss × stops)
    pub stops: f32,
    /// Refuelling
    pub refuel: f32,
    /// Pace of the compounds (s per lap × laps)
    pub pace: f32,
    /// Tyre degradation
    pub wear: f32,
    /// Weight of the fuel on board
    pub weight: f32,
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
    pub parts: Parts,
    /// What could go wrong ("palivo na hraně …"), for the briefing
    pub risks: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub best: Option<Option_>,
    pub alternative: Option<Option_>,
    /// A second alternative (another number of stops or other tyres than both above)
    pub second: Option<Option_>,
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

/// Stints as even as the tyres allow, none longer than its set lasts: laps shared in proportion
/// to the life, the laps left over going to the first stints, or to the last ones. Empty when
/// the sets cannot cover the race. (Many stops: trying every split would take too long.)
fn balanced_splits(inp: &Input, tyres: &[&Compound]) -> Vec<Vec<u32>> {
    let caps: Vec<u32> = tyres
        .iter()
        .enumerate()
        .map(|(i, t)| t.life.unwrap_or(inp.laps).saturating_sub(if i == 0 && !inp.stop_now { inp.current_age } else { 0 }))
        .collect();
    let sum: u32 = caps.iter().sum();
    if sum < inp.laps || caps.contains(&0) {
        return vec![];
    }
    let base: Vec<u32> = caps.iter().map(|c| ((inp.laps as u64 * *c as u64 / sum as u64) as u32).clamp(1, *c)).collect();
    let mut out = Vec::new();
    for back in [false, true] {
        let mut lens = base.clone();
        let mut left = inp.laps as i64 - lens.iter().sum::<u32>() as i64;
        while left > 0 {
            let order: Vec<usize> = if back { (0..lens.len()).rev().collect() } else { (0..lens.len()).collect() };
            let mut moved = false;
            for i in order {
                if left > 0 && lens[i] < caps[i] {
                    lens[i] += 1;
                    left -= 1;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        if left == 0 && !out.contains(&lens) {
            out.push(lens);
        }
    }
    out
}

fn deg_cost(deg: f32, age: u32, laps: u32) -> f32 {
    // age + (age+1) + … + (age+laps−1)
    deg * (laps as f32 * age as f32 + (laps as f32 * (laps as f32 - 1.0)) / 2.0)
}

struct Eval {
    cost: f32,
    parts: Parts,
    over: f32,
    stops: Vec<PlannedStop>,
    risks: Vec<String>,
}

/// Fuel that has to be left on arriving at a stop, laps, whatever the reserve set
const MIN_STOP_RESERVE: f32 = 0.05;
/// Cost of each lap of stop reserve not kept with the fuel on board, s: any plan that keeps it
/// is taken first, unless it costs more than about a stop
const SHORT_RESERVE_S: f32 = 30.0;

/// Fuel per lap a stint could still take (tank or fuel on board at its start) minus the planned
/// figure: how much more the car may burn before the stint runs dry.
fn fuel_margin(available: f32, laps: u32, reserve: f32, fpl: f32) -> f32 {
    available / (laps as f32 + reserve) - fpl
}

fn evaluate(inp: &Input, lens: &[u32], tyres: &[&Compound]) -> Option<Eval> {
    let fpl = inp.fuel_per_lap;
    let mut fuel = inp.fuel;
    let mut cost = 0.0;
    let mut parts = Parts::default();
    let mut over = 0.0;
    let mut lap = inp.first_lap - 1; // last lap done
    let mut stops = Vec::new();
    let mut risks = Vec::new();
    let last = lens.len() - 1;
    for (i, (&n, t)) in lens.iter().zip(tyres).enumerate() {
        let fresh = i > 0 || inp.stop_now || inp.current.is_none();
        let age = if fresh { 0 } else { inp.current_age };
        if i > 0 || inp.stop_now {
            // Stop before this stint: just enough fuel for it and the reserve (to the next stop or the finish)
            let need = (n as f32 + if i == last { inp.reserve } else { inp.stop_reserve }) * fpl;
            let add = (need - fuel).max(0.0);
            if fuel + add > inp.capacity + 0.01 {
                return None;
            }
            fuel += add;
            let margin = fuel_margin(inp.capacity, n, if i == last { inp.reserve } else { inp.stop_reserve }, fpl);
            if fpl > 0.01 && margin < fpl * 0.04 {
                risks.push(format!("stint {} ({} kol) se do nádrže vejde jen s rezervou {:.2} l/kolo", i + 1, n, margin.max(0.0)));
            }
            let refuel = if inp.refuel_rate > 0.0 { add / inp.refuel_rate } else { 0.0 };
            cost += inp.pit_loss + refuel;
            parts.stops += inp.pit_loss;
            parts.refuel += refuel;
            stops.push(PlannedStop { lap, compound: t.name.clone(), fuel: add });
        } else {
            // The fuel on board is what it is: to the next stop the reserve is a wish (a cost when
            // it is not kept), only a twentieth of a lap is a must
            let hard = if i == last { inp.reserve } else { inp.stop_reserve.min(MIN_STOP_RESERVE) };
            if fuel + 0.01 < (n as f32 + hard) * fpl {
                return None;
            }
            if i != last && fpl > 0.01 {
                let left = fuel / fpl - n as f32;
                if left + 0.001 < inp.stop_reserve {
                    let short = inp.stop_reserve - left;
                    cost += short * SHORT_RESERVE_S;
                    parts.stops += short * SHORT_RESERVE_S;
                    risks.push(format!(
                        "do boxu po {n} kolech dojedeš jen s {:.1} l – méně než rezerva {:.1} kola",
                        left.max(0.0) * fpl,
                        inp.stop_reserve
                    ));
                }
            }
            let margin = fuel_margin(fuel, n, if i == last { inp.reserve } else { hard }, fpl);
            if fpl > 0.01 && margin < fpl * 0.04 {
                risks.push(format!("{} kol bez tankování: rezerva jen {:.2} l/kolo", n, margin.max(0.0)));
            }
        }
        // Weight: litres on board, lap by lap (fuel at the start of each lap)
        let weight = inp.fuel_s * (n as f32 * fuel - fpl * (n as f32 * (n as f32 - 1.0)) / 2.0);
        cost += weight;
        parts.weight += weight;
        if let Some(life) = t.life {
            over += (age + n).saturating_sub(life) as f32;
            if age + n > life {
                risks.push(format!("{} {} kol – {} nad výdrží sady", t.name, age + n, age + n - life));
            } else if age + n == life && life > 2 {
                risks.push(format!("{} {} kol – sada až na hranici výdrže", t.name, age + n));
            }
        }
        let (pace, wear) = (t.pace * n as f32, deg_cost(t.deg.unwrap_or(inp.deg * t.wear_factor), age, n));
        cost += pace + wear;
        parts.pace += pace;
        parts.wear += wear;
        // Tiny preference for keeping the same compound when everything else is equal
        if i > 0 && tyres[i - 1].name != t.name {
            cost += 0.001;
        }
        fuel -= n as f32 * fpl;
        lap += n as i16;
    }
    Some(Eval { cost, parts, over, stops, risks })
}

/// Plans with `ks` stops and stints of at most `max` laps, added to `found` as (stops, cost,
/// laps over the tyre life, plan).
fn search(
    inp: &Input,
    current_compound: &Option<Compound>,
    ks: std::ops::RangeInclusive<usize>,
    balanced: bool,
    found: &mut Vec<(usize, f32, f32, Option_)>,
) {
    for k in ks {
        let parts = if inp.stop_now { k } else { k + 1 };
        if parts == 0 || parts as u32 > inp.laps {
            continue;
        }
        let mut all = Vec::new();
        if !balanced {
            splits(inp.laps, parts, 1, &mut all, &mut Vec::new());
        }
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
            let even = if balanced { balanced_splits(inp, &tyres) } else { vec![] };
            for lens in if balanced { &even } else { &all } {
                if let Some(e) = evaluate(inp, lens, &tyres) {
                    let opt = Option_ {
                        stints: lens.iter().zip(&tyres).map(|(&laps, t)| Stint { compound: t.name.clone(), laps }).collect(),
                        stops: e.stops,
                        cost: e.cost,
                        delta: 0.0,
                        over_limit: e.over > 0.0,
                        parts: e.parts,
                        risks: e.risks,
                    };
                    found.push((k, e.cost, e.over, opt));
                }
            }
        }
    }
}

/// Shorter life = faster wear: the wear factor of each compound against the average life.
fn with_wear_factors(inp: &Input) -> Input {
    let lives: Vec<f32> = inp.compounds.iter().filter_map(|c| c.life).map(|l| l as f32).collect();
    let avg = if lives.is_empty() { 0.0 } else { lives.iter().sum::<f32>() / lives.len() as f32 };
    let compounds: Vec<Compound> = inp
        .compounds
        .iter()
        .map(|c| Compound { wear_factor: c.life.filter(|l| *l > 0 && avg > 0.0).map_or(1.0, |l| avg / l as f32), ..c.clone() })
        .collect();
    Input { compounds, ..inp.clone() }
}

/// A plan the driver put together (compound and laps per stint): costed like the planner's own,
/// with the fuel to add at each stop. Err: why it cannot be driven, in Czech.
pub fn evaluate_custom(inp: &Input, stints: &[(String, u32)]) -> Result<Option_, String> {
    if stints.is_empty() {
        return Err("Zadej aspoň jeden stint.".into());
    }
    let sum: u32 = stints.iter().map(|s| s.1).sum();
    if sum != inp.laps {
        return Err(if sum < inp.laps {
            format!("Stinty mají {sum} kol – chybí {} do {}.", inp.laps - sum, inp.laps)
        } else {
            format!("Stinty mají {sum} kol – o {} víc než {}.", sum - inp.laps, inp.laps)
        });
    }
    if let Some(i) = stints.iter().position(|s| s.1 == 0) {
        return Err(format!("Stint {} nemá žádné kolo.", i + 1));
    }
    let inp = with_wear_factors(inp);
    let mut tyres = Vec::new();
    for (name, _) in stints {
        match inp.compounds.iter().find(|c| &c.name == name) {
            Some(c) => tyres.push(c),
            None => return Err(format!("Pro {name} nejsou data (tempo ani výdrž) – zadej je v Parametrech strategie.")),
        }
    }
    let lens: Vec<u32> = stints.iter().map(|s| s.1).collect();
    let Some(e) = evaluate(&inp, &lens, &tyres) else {
        // The only way to fail: a stint the tank cannot cover
        let fpl = inp.fuel_per_lap.max(0.001);
        let max = ((inp.capacity / fpl) - inp.stop_reserve).floor().max(0.0) as u32;
        let first = ((inp.fuel / fpl) - MIN_STOP_RESERVE).floor().max(0.0) as u32;
        let long = stints.iter().enumerate().find(|(i, s)| s.1 > if *i == 0 && !inp.stop_now { first } else { max });
        return Err(match long {
            Some((i, s)) => format!(
                "Stint {} ({} kol) se do nádrže nevejde – nejvýš {} kol.",
                i + 1,
                s.1,
                if i == 0 && !inp.stop_now { first } else { max }
            ),
            None => "Na tenhle plán palivo nevyjde.".into(),
        });
    };
    Ok(Option_ {
        stints: stints.iter().map(|(c, laps)| Stint { compound: c.clone(), laps: *laps }).collect(),
        stops: e.stops,
        cost: e.cost,
        delta: 0.0,
        over_limit: e.over > 0.0,
        parts: e.parts,
        risks: e.risks,
    })
}

pub fn plan(inp: &Input) -> Options {
    if inp.laps == 0 || inp.fuel_per_lap <= 0.0 || inp.compounds.is_empty() {
        return Options::default();
    }
    let inp = &with_wear_factors(inp);
    let current = inp.current.as_ref().and_then(|c| inp.compounds.iter().find(|x| &x.name == c)).cloned();
    let current_compound = current.clone().or_else(|| inp.current.as_ref().map(|c| Compound::new(c, None, 0.0)));
    // (stops, cost, over, option)
    let mut found: Vec<(usize, f32, f32, Option_)> = Vec::new();
    let min_stops = usize::from(inp.stop_now || inp.need_stop);
    let first_k = min_stops.max(if inp.stop_now { 1 } else { 0 });
    search(inp, &current_compound, first_k..=MAX_STOPS, false, &mut found);
    // Nothing within the tyre life: more stops, stints as even as the sets allow
    if found.iter().all(|f| f.2 > 0.0) && inp.compounds.iter().any(|c| c.life.is_some()) {
        search(inp, &current_compound, (MAX_STOPS + 1).max(first_k)..=MAX_STOPS_LIFE, true, &mut found);
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
    // A real alternative: another number of stops (not just a lap earlier or later). Other tyres
    // with the same stops only when there is no such plan – mostly that is the recommendation
    // with a short stint on slower tyres, a worse plan rather than a choice.
    let cheapest = |f: &dyn Fn(usize) -> bool| {
        (0..found.len()).filter(|&i| f(i)).min_by(|&a, &b| key(&found[a]).partial_cmp(&key(&found[b])).unwrap())
    };
    let alt_i = cheapest(&|i| found[i].0 != best_stops).or_else(|| cheapest(&|i| tyres_of(&found[i].3) != tyres_of(&best)));
    let with_delta = |i: usize| {
        let mut o = found[i].3.clone();
        o.delta = o.cost - best.cost;
        o
    };
    // The second alternative: differs from both in stops or tyres, and no stint of one or two
    // laps (a "plan" that only bolts a short stint on)
    let sig = |i: usize| (found[i].0, tyres_of(&found[i].3));
    let second_i = alt_i.and_then(|a| {
        cheapest(&|i| sig(i) != sig(best_i) && sig(i) != sig(a) && found[i].3.stints.iter().all(|s| s.laps >= 3 || inp.laps < 10))
    });
    let (alternative, second) = (alt_i.map(with_delta), second_i.map(with_delta));
    Options { best: Some(best), alternative, second }
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
            stop_reserve: 0.05,
            refuel_rate: 3.0,
            pit_loss: 25.0,
            deg: 0.2,
            fuel_s: 0.0,
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
    fn stop_reserve_is_left_on_arriving_at_each_stop() {
        // Spa 19 Nov: 21 laps, 12.08 per lap, a 100 l tank, RS only (life 8)
        let fpl = 12.08;
        let mut inp = race(21, fpl);
        inp.compounds = vec![Compound::new("RS", Some(8), 0.0)];
        inp.current = Some("RS".into());
        inp.need_compound = None;
        for reserve in [0.05, 0.3, 0.5] {
            inp.stop_reserve = reserve;
            let best = plan(&inp).best.unwrap();
            assert!(best.stops.len() >= 2, "{best:?}");
            let mut fuel = 100.0;
            for (i, st) in best.stints.iter().enumerate() {
                if i > 0 {
                    fuel += best.stops[i - 1].fuel;
                }
                fuel -= st.laps as f32 * fpl;
                let want = if i + 1 == best.stints.len() { inp.reserve } else { reserve };
                assert!(fuel >= want * fpl - 0.05, "stint {i}, reserve {reserve}: {fuel} l left · {best:?}");
            }
        }
    }

    #[test]
    fn own_plan_gets_its_fuel_and_cost() {
        // Spa: 21 laps, 11.26 l per lap, RM 8 → RM 8 → RS 5
        let mut inp = race(21, 11.26);
        inp.compounds = vec![Compound::new("RS", Some(6), 0.0), Compound::new("RM", Some(11), 1.0)];
        inp.current = Some("RM".into());
        inp.need_compound = None;
        inp.stop_reserve = 0.3;
        let st = |v: &[(&str, u32)]| v.iter().map(|(c, l)| (c.to_string(), *l)).collect::<Vec<_>>();
        let o = evaluate_custom(&inp, &st(&[("RM", 8), ("RM", 8), ("RS", 5)])).unwrap();
        let fuel: Vec<(i16, &str, i32)> = o.stops.iter().map(|s| (s.lap, s.compound.as_str(), s.fuel.round() as i32)).collect();
        assert_eq!(fuel, [(8, "RM", 84), (16, "RS", 56)]);
        assert!(!o.over_limit);
        // Wrong sums, a stint the tank cannot cover, a compound without data
        assert!(evaluate_custom(&inp, &st(&[("RM", 8), ("RS", 5)])).unwrap_err().contains("chybí 8"));
        assert!(evaluate_custom(&inp, &st(&[("RM", 2), ("RM", 14), ("RS", 5)]))
            .unwrap_err()
            .contains("Stint 2 (14 kol) se do nádrže nevejde"));
        assert!(evaluate_custom(&inp, &st(&[("RM", 16), ("RH", 5)])).unwrap_err().contains("RH"));
    }

    #[test]
    fn short_life_takes_more_stops_than_three() {
        // Spa 20 Nov: 21 laps, RS only, a set lasts 5 laps → 5 stints, none over the life
        let mut inp = race(21, 11.94);
        inp.compounds = vec![Compound::new("RS", Some(5), 0.0)];
        inp.current = Some("RS".into());
        inp.need_compound = None;
        inp.stop_reserve = 0.3;
        let t = std::time::Instant::now();
        let o = plan(&inp);
        assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
        let best = o.best.unwrap();
        assert!(!best.over_limit, "{best:?}");
        assert_eq!(best.stops.len(), 4, "{best:?}");
        assert!(best.stints.iter().all(|s| s.laps <= 5), "{best:?}");
        // Three compounds with a life each: still quick
        let mut inp = race(40, 7.0);
        inp.compounds = tyres(Some(5), Some(7), Some(9));
        let t = std::time::Instant::now();
        let best = plan(&inp).best.unwrap();
        assert!(t.elapsed().as_millis() < 1500, "{:?}", t.elapsed());
        assert!(!best.over_limit, "{best:?}");
    }

    #[test]
    fn too_short_tyre_life_still_gives_a_plan() {
        let mut inp = race(27, 7.71);
        inp.compounds = tyres(Some(3), Some(3), Some(3));
        let best = plan(&inp).best.unwrap();
        assert!(best.over_limit);
    }

    #[test]
    fn abu_dhabi_alternative_saves_a_stop() {
        // 30 laps, RS mandatory, RS lasts 9 laps, RM 13 but 2.75 s slower: 3 stops on RS is
        // best; the alternative is one stop less with RM, not the same stops with a short RM stint
        let mut c = vec![Compound::new("RS", Some(9), 0.08), Compound::new("RM", Some(13), 2.83)];
        c[0].deg = Some(0.01);
        c[1].deg = Some(0.0);
        let inp = Input { laps: 30, fuel_per_lap: 6.96, pit_loss: 19.7, current: None, compounds: c, ..race(30, 6.96) };
        let o = plan(&inp);
        let best = o.best.unwrap();
        assert_eq!(best.stops.len(), 3, "{best:?}");
        assert!(best.stints.iter().all(|s| s.compound == "RS"));
        let alt = o.alternative.unwrap();
        assert_eq!(alt.stops.len(), 2, "{alt:?}");
        // One long stint on RM (12–13 laps, where in the race does not matter much), the rest on RS
        let rm: Vec<u32> = alt.stints.iter().filter(|s| s.compound == "RM").map(|s| s.laps).collect();
        assert!(rm.len() == 1 && rm[0] >= 12, "{alt:?}");
        // +15.7 s: RM costs 13 × 2.75 = 35.75 s, the stop saved gives back 19.7 s
        assert!(alt.delta > 10.0 && alt.delta < 20.0, "{alt:?}");
        assert!((alt.parts.stops - best.parts.stops + 19.7).abs() < 0.01);
        assert!((alt.parts.pace - best.parts.pace - rm[0] as f32 * 2.75).abs() < 0.01);
    }

    #[test]
    fn lighter_car_favours_less_fuel_on_board() {
        // 20 laps, 6 l a lap, tank 100: one stop either way. Without the weight the stop lap
        // hardly matters; with it the planner keeps the second stint short of fuel by stopping
        // late (the fuel added at the stop is carried for fewer laps).
        let mut inp = race(20, 6.0);
        inp.need_compound = None;
        inp.compounds = vec![Compound::new("RM", None, 0.0)];
        inp.deg = 0.0;
        inp.fuel_s = 0.04;
        let o = plan(&inp);
        let best = o.best.unwrap();
        assert_eq!(best.stops.len(), 1);
        assert!(best.parts.weight > 0.0);
        // The latest stop the tank allows: 100 l / 6 = 16.6 laps → box after lap 16
        assert_eq!(best.stops[0].lap, 16, "{best:?}");
    }

    #[test]
    fn options_carry_risks_and_a_second_alternative() {
        let mut c = vec![Compound::new("RS", Some(9), 0.08), Compound::new("RM", Some(13), 1.5), Compound::new("RH", Some(18), 2.5)];
        c[0].deg = Some(0.5);
        let inp = Input { laps: 30, fuel_per_lap: 7.6, pit_loss: 17.0, compounds: c, fuel_s: 0.033, ..race(30, 7.6) };
        let o = plan(&inp);
        let (best, alt, second) = (o.best.unwrap(), o.alternative.unwrap(), o.second.unwrap());
        assert_ne!(alt.stops.len(), best.stops.len());
        // A third, different plan
        let sig = |x: &Option_| {
            (x.stops.len(), {
                let mut t: Vec<_> = x.stints.iter().map(|s| s.compound.clone()).collect();
                t.sort();
                t
            })
        };
        assert!(sig(&second) != sig(&best) && sig(&second) != sig(&alt), "{second:?}");
        // 13 laps on a 100 l tank at 7.6 l a lap is right at the edge
        let all = [&best, &alt, &second];
        assert!(all.iter().any(|x| !x.risks.is_empty()), "{all:?}");
    }

    #[test]
    fn splits_cover_everything() {
        let mut all = Vec::new();
        splits(5, 2, 1, &mut all, &mut Vec::new());
        assert_eq!(all, vec![vec![1, 4], vec![2, 3], vec![3, 2], vec![4, 1]]);
    }
}
