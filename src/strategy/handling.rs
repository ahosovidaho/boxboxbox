//! How the car behaves, lap by lap, and what to change in the setup (or in the driving).
//!
//! Every packet on track goes into a [`Collector`]: corner phases (braking into the corner,
//! middle, on the throttle out of it) with the balance from [`crate::telemetry::dynamics`]
//! (understeer above +2°, oversteer below −2°), locking wheels under braking, wheelspin on the
//! throttle and the engine at the limiter in the top gear. At the end of a lap the counts are
//! stored per car, setup and track (`handling.json`).
//!
//! [`advice`] looks at the last laps of the active setup and turns clear findings into one change
//! at a time for the setup sheet – with the current value when it was typed in – and a tip for
//! the driving. A BoP car (setup locked) only gets the driving tips.

use super::practice::track_key;
use super::setups::Setup;
use crate::telemetry::dynamics::Dynamics;
use crate::telemetry::packet::{flags, Packet};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Balance (degrees) from which a tick counts as understeer (+) or oversteer (−)
const BALANCE: f32 = 2.0;
/// Corner: yaw rate at least this (rad/s)
const MIN_YAW: f32 = 0.12;
/// Below this speed (m/s) nothing is counted (hairpins at walking pace, pit lane)
const MIN_SPEED: f32 = 15.0;
/// Pedal thresholds (0–255)
const BRAKING: u8 = 25;
const HARD_BRAKING: u8 = 76;
const FULL_THROTTLE: u8 = 178;
/// Wheel speed / car speed below this under braking = locking, above `SPIN` on the throttle = wheelspin
const LOCK: f32 = 0.88;
const SPIN: f32 = 1.08;
/// Laps looked at for the advice, and how many are needed at least
const LAST_LAPS: usize = 10;
const MIN_LAPS: usize = 3;
const MAX_STORED: usize = 600;

/// Ticks of one corner phase: all, understeering, oversteering
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Phase {
    pub n: u32,
    pub us: u32,
    pub os: u32,
}

impl Phase {
    fn add(&mut self, o: &Phase) {
        self.n += o.n;
        self.us += o.us;
        self.os += o.os;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct LapStats {
    pub at: String,
    pub car: i32,
    pub setup: String,
    pub track: String,
    pub entry: Phase,
    pub mid: Phase,
    pub exit: Phase,
    /// Hard braking ticks, and of them with locking front / rear wheels
    pub brake: u32,
    pub lock_f: u32,
    pub lock_r: u32,
    /// Full throttle ticks, and of them with spinning front / rear wheels, with TCS working
    pub throttle: u32,
    pub spin_f: u32,
    pub spin_r: u32,
    pub tcs: u32,
    /// Full throttle ticks in the top gear, and of them at the limiter
    pub top: u32,
    pub limiter: u32,
    pub max_gear: u8,
    pub gears: u8,
}

impl LapStats {
    fn add(&mut self, o: &LapStats) {
        self.entry.add(&o.entry);
        self.mid.add(&o.mid);
        self.exit.add(&o.exit);
        self.brake += o.brake;
        self.lock_f += o.lock_f;
        self.lock_r += o.lock_r;
        self.throttle += o.throttle;
        self.spin_f += o.spin_f;
        self.spin_r += o.spin_r;
        self.tcs += o.tcs;
        self.top += o.top;
        self.limiter += o.limiter;
        self.max_gear = self.max_gear.max(o.max_gear);
        self.gears = self.gears.max(o.gears);
    }
}

/// Counts of the lap being driven
#[derive(Default)]
pub struct Collector {
    lap: LapStats,
}

impl Collector {
    pub fn tick(&mut self, p: &Packet, d: &Dynamics) {
        if p.flags & flags::ON_TRACK == 0 || p.flags & (flags::PAUSED | flags::LOADING) != 0 || d.v_fwd < MIN_SPEED {
            return;
        }
        let s = &mut self.lap;
        let gears = p.gear_ratios.iter().filter(|r| **r > 0.0).count() as u8;
        s.gears = s.gears.max(gears);
        if (1..=10).contains(&p.gear) {
            s.max_gear = s.max_gear.max(p.gear);
        }
        let speed = p.speed_ms.max(1.0);
        let ws: Vec<f32> = (0..4).map(|i| p.wheel_rps[i].abs() * p.tire_radius[i] / speed).collect();
        if p.brake >= HARD_BRAKING {
            s.brake += 1;
            s.lock_f += u32::from(ws[0].min(ws[1]) < LOCK);
            s.lock_r += u32::from(ws[2].min(ws[3]) < LOCK);
        }
        if p.throttle >= FULL_THROTTLE {
            s.throttle += 1;
            s.spin_f += u32::from(ws[0].max(ws[1]) > SPIN);
            s.spin_r += u32::from(ws[2].max(ws[3]) > SPIN);
            s.tcs += u32::from(p.flags & flags::TCS_ACTIVE != 0);
            if gears >= 4 && p.gear == gears {
                s.top += 1;
                s.limiter += u32::from(p.rpm_alert_max > 0 && p.engine_rpm >= p.rpm_alert_max as f32);
            }
        }
        let Some(b) = d.balance else { return };
        if d.yaw_rate.abs() < MIN_YAW {
            return;
        }
        let ph = if p.brake >= BRAKING {
            &mut s.entry
        } else if p.throttle >= FULL_THROTTLE {
            &mut s.exit
        } else {
            &mut s.mid
        };
        ph.n += 1;
        ph.us += u32::from(b > BALANCE);
        ph.os += u32::from(b < -BALANCE);
    }

    /// Ends the lap: its counts (the collector starts empty again).
    pub fn take(&mut self) -> LapStats {
        std::mem::take(&mut self.lap)
    }
}

/// Stored laps of all cars (`handling.json`)
pub struct Handling {
    laps: Vec<LapStats>,
    file: Option<PathBuf>,
}

impl Handling {
    pub fn load(file: Option<PathBuf>) -> Self {
        let laps =
            file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Handling { laps, file }
    }

    pub fn add(&mut self, mut lap: LapStats) {
        // Laps with hardly any corners (a lap in the pits, a drag strip) say nothing
        if lap.entry.n + lap.mid.n + lap.exit.n < 200 {
            return;
        }
        lap.at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        self.laps.push(lap);
        if self.laps.len() > MAX_STORED {
            self.laps.remove(0);
        }
        let Some(file) = &self.file else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string(&self.laps).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, file);
        }
    }

    /// Track of the latest stored lap of a car with a setup
    pub fn last_track(&self, car: i32, setup: &str) -> Option<String> {
        self.laps.iter().rev().find(|l| l.car == car && l.setup == setup).map(|l| l.track.clone())
    }

    /// The last laps of a car with a setup on a track ("" track = any)
    pub fn last(&self, car: i32, setup: &str, track: &str) -> Vec<&LapStats> {
        let key = track_key(track);
        let mut v: Vec<&LapStats> = self
            .laps
            .iter()
            .rev()
            .filter(|l| l.car == car && l.setup == setup && (track.is_empty() || track_key(&l.track) == key))
            .take(LAST_LAPS)
            .collect();
        v.reverse();
        v
    }
}

/// One finding with what to try
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Advice {
    /// "entry", "mid", "exit", "brakes", "traction", "gearing"
    pub area: &'static str,
    /// What the data show, e.g. "Nedotáčivost ve středu zatáčky (22 % průjezdu)"
    pub finding: String,
    /// One change in the setup sheet (none for BoP)
    pub setup: Option<String>,
    pub driving: Option<String>,
    /// Share of the phase (0–1), for sorting
    pub share: f32,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub laps: usize,
    pub needed: usize,
    pub advice: Vec<Advice>,
    /// Shares per phase for the dashboard: [understeer, oversteer] in %
    pub entry: [f32; 2],
    pub mid: [f32; 2],
    pub exit: [f32; 2],
    pub lock_f: f32,
    pub lock_r: f32,
    pub spin: f32,
    /// The balance cannot be judged for this car (shown instead of the balance advice)
    pub balance_note: Option<String>,
}

fn pct(a: u32, n: u32) -> f32 {
    if n == 0 {
        0.0
    } else {
        a as f32 / n as f32
    }
}

/// A change of a numeric field: "zadní stabilizátor 6 → 5" when the value is known, otherwise
/// the direction in words.
fn change(setup: Option<&Setup>, key: &str, label: &str, delta: f32, range: (f32, f32), words: &str) -> String {
    let cur = setup.and_then(|s| s.values.get(key)).and_then(|v| v.trim().replace(',', ".").parse::<f32>().ok());
    match cur {
        Some(c) => {
            let n = (c + delta).clamp(range.0, range.1);
            if (n - c).abs() < 1e-6 {
                format!("{label} je na hraně rozsahu ({}) – zkus jinou úpravu", fmt(c))
            } else {
                format!("{label} {} → {}", fmt(c), fmt(n))
            }
        }
        None => format!("{label} {words}"),
    }
}

fn fmt(v: f32) -> String {
    if (v - v.round()).abs() < 1e-6 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}").replace('.', ",")
    }
}

/// What to try for the laps. `bop`: the setup cannot be changed, driving tips only.
/// `balance`: understeer/oversteer can be judged for the car (not for karts: the slip angles
/// assume a car's wheelbase).
pub fn advice(laps: &[&LapStats], setup: Option<&Setup>, bop: bool, balance: bool) -> Report {
    let mut r = Report { laps: laps.len(), needed: MIN_LAPS, ..Default::default() };
    let mut t = LapStats::default();
    for l in laps {
        t.add(l);
    }
    r.entry = [pct(t.entry.us, t.entry.n) * 100.0, pct(t.entry.os, t.entry.n) * 100.0];
    r.mid = [pct(t.mid.us, t.mid.n) * 100.0, pct(t.mid.os, t.mid.n) * 100.0];
    r.exit = [pct(t.exit.us, t.exit.n) * 100.0, pct(t.exit.os, t.exit.n) * 100.0];
    r.lock_f = pct(t.lock_f, t.brake) * 100.0;
    r.lock_r = pct(t.lock_r, t.brake) * 100.0;
    r.spin = pct(t.spin_f.max(t.spin_r), t.throttle) * 100.0;
    if laps.len() < MIN_LAPS {
        return r;
    }
    let s = setup;
    let mut out: Vec<Advice> = Vec::new();
    let mut push = |area, finding: String, setup: String, driving: &str, share: f32| {
        out.push(Advice {
            area,
            finding,
            setup: (!bop).then_some(setup),
            driving: (!driving.is_empty()).then(|| driving.to_string()),
            share,
        });
    };
    let p = |x: f32| format!("{:.0} %", x * 100.0);

    // Balance per phase: clear when one side dominates
    let phases =
        [("entry", &t.entry, "na brzdách při vjezdu"), ("mid", &t.mid, "ve středu zatáčky"), ("exit", &t.exit, "na plynu při výjezdu")];
    if !balance {
        r.balance_note = Some("U motokáry nedotáčivost a přetáčivost neměřím spolehlivě (výpočet počítá s rozvorem auta).".into());
    }
    for (area, ph, where_) in phases {
        if ph.n < 300 || !balance {
            continue;
        }
        let (us, os) = (pct(ph.us, ph.n), pct(ph.os, ph.n));
        if us >= 0.15 && us >= 2.0 * os {
            let finding = format!("Nedotáčivost {where_} ({} průjezdu)", p(us));
            let (setup, driving) = match area {
                "entry" => (
                    format!(
                        "{}; nebo vyvážení brzd o 1 krok dozadu",
                        change(s, "lsd_brake_r", "citlivost brzdění diferenciálu (zadní)", -5.0, (5.0, 60.0), "o 5 níž")
                    ),
                    "Dobrzďuj dřív a do zatáčky vjížděj pomaleji; brzdu povoluj plynule, jak přidáváš volant.",
                ),
                "mid" => (
                    format!(
                        "{}; nebo {}",
                        change(s, "arb_f", "přední stabilizátor", -1.0, (1.0, 10.0), "o 1 měkčí"),
                        change(s, "arb_r", "zadní stabilizátor", 1.0, (1.0, 10.0), "o 1 tvrdší")
                    ),
                    "Pomalejší vjezd a dřívější vrchol; když auto nezatáčí, nepřidávej volant, uber plyn.",
                ),
                _ => (
                    format!(
                        "{}; nebo {}",
                        change(s, "lsd_accel_r", "citlivost akcelerace diferenciálu (zadní)", -5.0, (5.0, 60.0), "o 5 níž"),
                        change(s, "arb_f", "přední stabilizátor", -1.0, (1.0, 10.0), "o 1 měkčí")
                    ),
                    "Plný plyn až když začneš povolovat volant; na výjezdu nech auto doběhnout na vnější stranu.",
                ),
            };
            push(area, finding, setup, driving, us);
        } else if os >= 0.12 && os >= 2.0 * us {
            let finding = format!("Přetáčivost {where_} ({} průjezdu)", p(os));
            let (setup, driving) = match area {
                "entry" => (
                    format!(
                        "{}; nebo vyvážení brzd o 1 krok dopředu",
                        change(s, "lsd_brake_r", "citlivost brzdění diferenciálu (zadní)", 5.0, (5.0, 60.0), "o 5 výš")
                    ),
                    "Většinu brzdění udělej v přímém směru; brzdu při zatáčení povoluj pomaleji a nepodřazuj tak brzy.",
                ),
                "mid" => (
                    format!(
                        "{}; nebo {}",
                        change(s, "arb_r", "zadní stabilizátor", -1.0, (1.0, 10.0), "o 1 měkčí"),
                        change(s, "downforce_r", "zadní přítlak", 10.0, (0.0, 2000.0), "o kus víc")
                    ),
                    "Středem zatáčky drž lehký plyn, ať je záď zatížená; neuber plyn najednou.",
                ),
                _ => (
                    format!(
                        "{}; nebo {}",
                        change(s, "arb_r", "zadní stabilizátor", -1.0, (1.0, 10.0), "o 1 měkčí"),
                        change(s, "tcs", "kontrola trakce", 1.0, (0.0, 5.0), "o 1 výš")
                    ),
                    "Na výjezdu přidávej plyn postupně a plný až s rovnými koly.",
                ),
            };
            push(area, finding, setup, driving, os);
        }
    }

    // Brakes
    if t.brake >= 200 {
        let (f, rr) = (pct(t.lock_f, t.brake), pct(t.lock_r, t.brake));
        if f >= 0.08 && f >= 2.0 * rr {
            push(
                "brakes",
                format!("Blokují přední kola ({} tvrdého brzdění)", p(f)),
                "vyvážení brzd o 1 krok dozadu".into(),
                "Na začátku brzdění plný tlak, pak ho s klesající rychlostí ubírej.",
                f,
            );
        } else if rr >= 0.05 && rr > f {
            push(
                "brakes",
                format!("Blokují zadní kola ({} tvrdého brzdění)", p(rr)),
                "vyvážení brzd o 1 krok dopředu".into(),
                "Méně tlaku na brzdu ke konci brzdění a při podřazování.",
                rr,
            );
        }
    }

    // Traction
    if t.throttle >= 300 {
        let spin = pct(t.spin_f.max(t.spin_r), t.throttle);
        if spin >= 0.08 {
            let axle = if t.spin_r >= t.spin_f { "zadní" } else { "přední" };
            let soft = if axle == "zadní" { ("arb_r", "zadní stabilizátor") } else { ("arb_f", "přední stabilizátor") };
            push(
                "traction",
                format!("Prokluz kol ({axle}) na plném plynu ({} času)", p(spin)),
                format!(
                    "{}; nebo {}",
                    change(s, "tcs", "kontrola trakce", 1.0, (0.0, 5.0), "o 1 výš"),
                    change(s, soft.0, soft.1, -1.0, (1.0, 10.0), "o 1 měkčí")
                ),
                "V pomalých zatáčkách vyšší rychlostní stupeň a plyn dávkuj.",
                spin,
            );
        }
    }

    // Gearing: at the limiter in the top gear, or the top gear never used
    if t.top >= 60 && pct(t.limiter, t.top) >= 0.15 {
        push(
            "gearing",
            format!("Na konci rovinky omezovač v nejvyšším stupni ({} plného plynu v něm)", p(pct(t.limiter, t.top))),
            change(s, "top_speed", "max. rychlost převodovky", 10.0, (0.0, 600.0), "o 10 km/h výš (delší převody)"),
            "",
            pct(t.limiter, t.top),
        );
    } else if t.gears >= 5 && t.max_gear > 0 && t.max_gear + 1 < t.gears {
        push(
            "gearing",
            format!("Nejvyšší použitý stupeň je {}. z {} – převody jsou dlouhé", t.max_gear, t.gears),
            change(s, "top_speed", "max. rychlost převodovky", -10.0, (0.0, 600.0), "o 10 km/h níž (kratší převody)"),
            "",
            0.1,
        );
    }

    if bop {
        out.retain(|a| a.driving.is_some());
    }
    out.sort_by(|a, b| b.share.total_cmp(&a.share));
    r.advice = out;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(entry: Phase, mid: Phase, exit: Phase) -> LapStats {
        LapStats { car: 1, track: "Yas Marina Circuit".into(), entry, mid, exit, gears: 6, max_gear: 6, ..Default::default() }
    }

    #[test]
    fn understeer_mid_corner_with_values() {
        let l = lap(Phase { n: 400, us: 10, os: 20 }, Phase { n: 900, us: 200, os: 20 }, Phase { n: 800, us: 30, os: 30 });
        let laps = [&l, &l, &l];
        let mut s = Setup::default();
        s.values.insert("arb_f".into(), "6".into());
        let r = advice(&laps, Some(&s), false, true);
        assert_eq!(r.advice.len(), 1, "{:?}", r.advice);
        let a = &r.advice[0];
        assert_eq!(a.area, "mid");
        assert!(a.finding.contains("22 %"), "{}", a.finding);
        assert!(a.setup.as_deref().unwrap().contains("přední stabilizátor 6 → 5"), "{:?}", a.setup);
        assert!(a.setup.as_deref().unwrap().contains("zadní stabilizátor o 1 tvrdší"), "{:?}", a.setup);
        // BoP: the same finding, driving tip only
        let r = advice(&laps, Some(&s), true, true);
        assert!(r.advice[0].setup.is_none() && r.advice[0].driving.is_some());
    }

    #[test]
    fn balanced_car_and_too_few_laps() {
        let l = lap(Phase { n: 400, us: 10, os: 30 }, Phase { n: 900, us: 80, os: 45 }, Phase { n: 800, us: 48, os: 16 });
        assert!(advice(&[&l, &l, &l], None, false, true).advice.is_empty());
        let mut over = lap(Phase { n: 400, us: 0, os: 120 }, Phase::default(), Phase::default());
        assert!(advice(&[&over, &over], None, false, true).advice.is_empty(), "2 laps are not enough");
        over.brake = 300;
        over.lock_f = 40;
        let r = advice(&[&over, &over, &over], None, false, true);
        let areas: Vec<_> = r.advice.iter().map(|a| a.area).collect();
        assert_eq!(areas, ["entry", "brakes"]);
        assert!(r.advice[0].finding.starts_with("Přetáčivost"));
    }

    #[test]
    fn gearing_limiter_and_long_gears() {
        let mut l = lap(Phase::default(), Phase::default(), Phase::default());
        l.top = 100;
        l.limiter = 30;
        let mut s = Setup::default();
        s.values.insert("top_speed".into(), "280".into());
        let r = advice(&[&l, &l, &l], Some(&s), false, true);
        assert_eq!(r.advice[0].setup.as_deref(), Some("max. rychlost převodovky 280 → 290"));
        assert!(advice(&[&l, &l, &l], Some(&s), true, true).advice.is_empty(), "BoP: no gearing tip");
        let mut l = lap(Phase::default(), Phase::default(), Phase::default());
        l.max_gear = 4;
        assert!(advice(&[&l, &l, &l], None, false, true).advice[0].finding.contains("4. z 6"));
    }

    #[test]
    fn stored_per_car_setup_and_track() {
        let mut h = Handling::load(None);
        let mut l = lap(Phase { n: 300, us: 0, os: 0 }, Phase::default(), Phase::default());
        h.add(l.clone());
        l.setup = "s2".into();
        h.add(l.clone());
        l.track = "Spa".into();
        h.add(l);
        h.add(LapStats { car: 1, ..Default::default() }); // no corners: ignored
        assert_eq!(h.last(1, "", "yas marina circuit").len(), 1);
        assert_eq!(h.last(1, "s2", "").len(), 2);
        assert_eq!(h.last(2, "", "").len(), 0);
    }
}
