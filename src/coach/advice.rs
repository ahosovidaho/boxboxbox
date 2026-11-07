//! What to do differently in a corner – the race engineer's sentence instead of a number.
//!
//! A lap's corner is compared with the same corner of the best lap: where the brake goes on and
//! off, how fast the apex is and where it is, the gear, when the throttle comes back and when it
//! is full, coasting without a pedal, the rear sliding. From the pattern one diagnosis is chosen –
//! the one that explains most of the time – and turned into an instruction with the place and
//! the numbers of the best lap ("brzdi o 9 m dřív, při ~142 km/h jako v nejlepším kole").
//!
//! For a session summary the corners of the normal laps are reduced to a typical lap (median of
//! every figure), so the advice says what usually happens there, not what happened once.

use super::compare::CornerStats;

#[derive(Clone, Debug, PartialEq)]
pub struct Tip {
    /// Which pattern ("overbrake", "early_brake", "slow_apex", "gear_low", …), to count repeats
    pub key: &'static str,
    /// The instruction, ready to show
    pub text: String,
    /// The same as a short call for the live hint while driving ("Zůstaň na 3.")
    pub short: String,
    /// Rough importance for ordering (bigger = more time)
    pub weight: f64,
}

fn m(v: f64) -> String {
    format!("{} m", v.abs().round() as i64)
}
fn kmh(v: f64) -> String {
    format!("{} km/h", v.abs().round() as i64)
}

/// Instructions for one corner of a lap against the best lap, most important first.
pub fn corner_tips(l: &CornerStats, r: &CornerStats) -> Vec<Tip> {
    let mut out: Vec<Tip> = Vec::new();
    // A corner with a spin, an excursion or the pit lane in it: the figures say nothing
    if (l.v_min - r.v_min).abs() > 40.0 || (l.v_exit - r.v_exit).abs() > 50.0 || l.v_min < 15.0 {
        return out;
    }
    // Differences of points along the track beyond this are a different line through the section
    // (or no full throttle at all in one lap), not something to copy
    let near = |d: f64| (d.abs() <= 80.0).then_some(d);
    let mut tip = |key: &'static str, weight: f64, short: String, text: String| out.push(Tip { key, text, short, weight });

    let dv = l.v_min - r.v_min; // apex speed, + = faster than best
    let dexit = l.v_exit - r.v_exit;
    let db = match (l.brake_at, r.brake_at) {
        (Some(a), Some(b)) => near(a - b), // + = brakes later
        _ => None,
    };
    let dfull = match (l.full_at, r.full_at) {
        (Some(a), Some(b)) => near(a - b), // + = full throttle later
        _ => None,
    };
    let dpick = match (l.pickup_at, r.pickup_at) {
        (Some(a), Some(b)) => near(a - b),
        _ => None,
    };
    let dapex = match (l.apex_at, r.apex_at) {
        (Some(a), Some(b)) => near(a - b), // + = apex later
        _ => None,
    };
    let ref_brake = r.brake_kmh.map(|v| format!(" (nejlepší kolo začíná brzdit při ~{})", kmh(v))).unwrap_or_default();
    // A fast corner taken without the brake (in both laps): it is about lift and line, not braking
    let flat = l.brake_at.is_none() && r.brake_at.is_none();
    let slide_more = l.rear_slide > 7.0f64.max(r.rear_slide + 6.0) || (l.over > 25.0 && l.over > r.over + 20.0);

    // --- Entry and apex ---
    match db {
        // Braking later, yet slower through the apex: overdriving the entry
        Some(d) if d > 6.0 && dv < -3.0 => tip(
            "overbrake",
            -dv * 0.6 + d * 0.02,
            format!("Brzdi o {} dřív, brzdu pouštěj plynule", m(d)),
            format!(
                "Přejíždíš vjezd: brzdíš o {} později, ale vrcholem projedeš o {} pomaleji. Brzdi dřív{ref_brake}, pusť brzdu plynule do zatáčky a vrcholem pak projedeš rychleji.",
                m(d),
                kmh(dv)
            ),
        ),
        // Braking earlier and not faster: time given away before the corner
        Some(d) if d < -8.0 && dv < 3.0 => tip(
            "early_brake",
            -d * 0.03 + (-dv).max(0.0) * 0.3,
            format!("Brzdi o {} později", m(d)),
            format!("Brzdíš o {} dřív, než je potřeba – nejlepší kolo brzdí až o {} dál{ref_brake}.", m(d), m(d)),
        ),
        _ => {}
    }
    if flat && dv < -2.0 {
        tip(
            "flat_lift",
            -dv * 0.4,
            "Neubírej plyn, plynulá stopa".into(),
            format!(
                "Rychlou zatáčkou bez brzdy projíždíš o {} pomaleji ({} místo {}) – neubírej plyn (nebo jen krátce) a jeď plynulou stopou bez korekcí volantu.",
                kmh(dv),
                kmh(l.v_min),
                kmh(r.v_min)
            ),
        );
    } else if dv < -3.0 && !matches!(db, Some(d) if d > 6.0) {
        // Same braking point but slower apex: too much brake or released too late
        let release = match (l.brake_off, r.brake_off) {
            (Some(a), Some(b)) if a - b > 8.0 => format!(" Brzdu pouštíš o {} později než v nejlepším kole.", m(a - b)),
            _ => String::new(),
        };
        tip(
            "slow_apex",
            -dv * 0.5,
            format!("Méně brzdy – vrcholem o {} rychleji", kmh(dv)),
            format!(
                "Do vrcholu moc zpomalíš – o {} víc než v nejlepším kole ({} místo {}).{release} Brzdi méně nebo pusť brzdu dřív a nech auto do vrcholu dojet rychleji.",
                kmh(dv),
                kmh(l.v_min),
                kmh(r.v_min)
            ),
        );
    }
    // Gear in the corner
    if let (Some(lg), Some(rg)) = (l.gear_min, r.gear_min) {
        if lg < rg {
            tip(
                "gear_low",
                1.5 + (-dv).max(0.0) * 0.1,
                format!("Zůstaň na {rg}. stupni"),
                format!("Zůstaň na {rg}. stupni – podřazuješ až na {lg}., nejlepší kolo tu jede na {rg}. a vyjíždí s menším prokluzem a vyšší rychlostí."),
            );
        } else if lg > rg {
            tip(
                "gear_high",
                1.5 + (-dexit).max(0.0) * 0.1,
                format!("Podřaď na {rg}. stupeň"),
                format!("Podřaď na {rg}. stupeň – jedeš tu na {lg}., nejlepší kolo na {rg}. a má na výjezdu víc tahu."),
            );
        }
    }
    // Apex position
    if let Some(d) = dapex {
        if d < -15.0 && dexit < -3.0 {
            tip(
                "early_apex",
                -dexit * 0.3,
                format!("Zatoč později – vrchol o {} dál", m(d)),
                format!("Vrchol bereš o {} dřív – zatáčej později, ať máš rovnější výjezd a můžeš dřív na plyn.", m(d)),
            );
        } else if d > 15.0 && dv < -2.0 {
            tip(
                "late_apex",
                -dv * 0.3,
                "Zatoč dřív, drž rychlost do vrcholu".into(),
                format!("Vrchol bereš o {} později – zatoč dřív, ať neztrácíš rychlost uprostřed zatáčky.", m(d)),
            );
        }
    }
    // Coasting without a pedal
    if l.coast_m > r.coast_m + 15.0 {
        tip(
            "coast",
            (l.coast_m - r.coast_m) * 0.03,
            "Z brzdy rovnou na plyn, nevaluj".into(),
            format!(
                "Jedeš {} bez pedálu (nejlepší kolo {}) – z brzdy přejdi plynule rovnou na plyn, ať auto nevalí.",
                m(l.coast_m),
                m(r.coast_m)
            ),
        );
    }

    // --- Exit ---
    if slide_more && dexit < 0.0 {
        tip(
            "exit_slide",
            2.0 + (-dexit) * 0.3,
            "Plyn postupně – záď ujíždí".into(),
            format!(
                "Na výjezdu ti ujíždí záď a vyjedeš o {} pomaleji – plyn přidávej postupně a plný až s narovnaným volantem.",
                kmh(dexit)
            ),
        );
    } else if let Some(d) = dfull {
        if d > 10.0 {
            let pick = match dpick {
                Some(p) if p.abs() < 8.0 => {
                    " Plyn bereš stejně jako v nejlepším kole, ale otevíráš ho pomalu – po vrcholu přidávej rozhodněji.".to_string()
                }
                Some(p) if p > 8.0 => format!(" Na plyn sahneš o {} později.", m(p)),
                _ => String::new(),
            };
            tip(
                "late_throttle",
                d * 0.03 + (-dexit).max(0.0) * 0.3,
                format!("Plný plyn o {} dřív", m(d)),
                format!("Na plný plyn přejdi o {} dřív.{pick}", m(d)),
            );
        } else if d < -10.0 && dexit < -3.0 {
            tip(
                "early_throttle",
                -dexit * 0.3,
                "S plným plynem počkej na rovný volant".into(),
                format!(
                    "Plný plyn dáváš o {} dřív, ale výjezd je o {} pomalejší – auto se nestihne srovnat; s plným plynem počkej na narovnání volantu.",
                    m(d),
                    kmh(dexit)
                ),
            );
        }
    }
    if l.under > 30.0 && l.under > r.under + 20.0 {
        tip(
            "understeer",
            1.0,
            "Vjeď pomaleji, pusť brzdu dřív".into(),
            "Auto ti tu nedotáčí víc než v nejlepším kole – vjeď pomaleji a dřív pusť brzdu, ať se předek zakousne.".into(),
        );
    }
    let _ = tip;
    if out.is_empty() && dexit < -4.0 {
        out.push(Tip {
            key: "slow_exit",
            weight: -dexit * 0.2,
            short: "Širší výjezd, dřív na plyn".into(),
            text: format!("Výjezd je o {} pomalejší – širší výjezd a dřívější plyn, rychlost se nese až na rovinku.", kmh(dexit)),
        });
    }
    // Nothing clear: name the biggest difference, small as it is, so there is something to try
    if out.is_empty() {
        let mut cands: Vec<(f64, String, String)> = Vec::new();
        if dv < -1.0 {
            let how = if flat { "drž plný plyn a plynulou stopu" } else { "zkus trochu méně brzdy" };
            cands.push((
                -dv / 3.0,
                format!("Vrcholem o {} rychleji", kmh(dv)),
                format!("vrcholem projíždíš o {} pomaleji – {how}", kmh(dv)),
            ));
        }
        if dexit < -1.5 {
            cands.push((
                -dexit / 4.0,
                "Plyn o kousek dřív a plynuleji".into(),
                format!("výjezd je o {} pomalejší – plyn o kousek dřív a plynuleji", kmh(dexit)),
            ));
        }
        if let Some(d) = db.filter(|d| *d < -3.0) {
            cands.push((-d / 8.0, format!("Brzdi o {} později", m(d)), format!("brzdíš o {} dřív – zkus brzdit o kousek později", m(d))));
        }
        if let Some(d) = dfull.filter(|d| *d > 4.0) {
            cands.push((d / 10.0, format!("Plný plyn o {} dřív", m(d)), format!("plný plyn o {} později – zkus ho otevřít dřív", m(d))));
        }
        cands.sort_by(|a, b| b.0.total_cmp(&a.0));
        if let Some((w, short, t)) = cands.into_iter().next() {
            out.push(Tip { key: "small", weight: w * 0.1, short, text: format!("Malý rozdíl: {t}.") });
        }
    }
    out.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    out
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

/// The typical corner of several laps: the median of every figure.
pub fn typical(laps: &[&CornerStats]) -> Option<CornerStats> {
    let first = *laps.first()?;
    let med = |f: &dyn Fn(&CornerStats) -> f64| median(laps.iter().map(|c| f(c)).collect()).unwrap_or(0.0);
    let med_opt = |f: &dyn Fn(&CornerStats) -> Option<f64>| median(laps.iter().filter_map(|c| f(c)).collect());
    let gear = |f: &dyn Fn(&CornerStats) -> Option<i64>| {
        median(laps.iter().filter_map(|c| f(c)).map(|g| g as f64).collect()).map(|g| g.round() as i64)
    };
    Some(CornerStats {
        brake_at: med_opt(&|c| c.brake_at),
        brake_max: med(&|c| c.brake_max),
        v_entry: med(&|c| c.v_entry),
        v_min: med(&|c| c.v_min),
        v_exit: med(&|c| c.v_exit),
        full_at: med_opt(&|c| c.full_at),
        gear_min: gear(&|c| c.gear_min),
        under: med(&|c| c.under),
        over: med(&|c| c.over),
        rear_slide: med(&|c| c.rear_slide),
        glat: med(&|c| c.glat),
        tcs: med(&|c| c.tcs),
        apex_at: med_opt(&|c| c.apex_at),
        brake_kmh: med_opt(&|c| c.brake_kmh),
        brake_off: med_opt(&|c| c.brake_off),
        pickup_at: med_opt(&|c| c.pickup_at),
        coast_m: med(&|c| c.coast_m),
        gear_entry: gear(&|c| c.gear_entry).or(first.gear_entry),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> CornerStats {
        CornerStats {
            brake_at: Some(400.0),
            brake_max: 100.0,
            v_entry: 180.0,
            v_min: 80.0,
            v_exit: 140.0,
            full_at: Some(480.0),
            gear_min: Some(3),
            under: 5.0,
            over: 5.0,
            rear_slide: 3.0,
            glat: 2.0,
            tcs: 0.0,
            apex_at: Some(450.0),
            brake_kmh: Some(178.0),
            brake_off: Some(440.0),
            pickup_at: Some(460.0),
            coast_m: 5.0,
            gear_entry: Some(5),
        }
    }

    #[test]
    fn overdriving_the_entry_and_a_gear_too_low() {
        let r = base();
        let l = CornerStats { brake_at: Some(410.0), v_min: 72.0, gear_min: Some(2), full_at: Some(500.0), ..base() };
        let t = corner_tips(&l, &r);
        assert_eq!(t[0].key, "overbrake", "{t:?}");
        assert!(t[0].text.contains("brzdíš o 10 m později") && t[0].text.contains("178 km/h"), "{}", t[0].text);
        assert!(t.iter().any(|x| x.key == "gear_low" && x.text.contains("Zůstaň na 3. stupni")), "{t:?}");
        assert_eq!(t[0].short, "Brzdi o 10 m dřív, brzdu pouštěj plynule");
        assert!(t.iter().any(|x| x.short == "Zůstaň na 3. stupni"), "{t:?}");
        assert!(t.iter().any(|x| x.key == "late_throttle"), "{t:?}");
    }

    #[test]
    fn early_braking_coasting_and_nothing_for_the_same_lap() {
        let r = base();
        assert!(corner_tips(&r, &r).is_empty());
        let l = CornerStats { brake_at: Some(385.0), coast_m: 30.0, ..base() };
        let keys: Vec<_> = corner_tips(&l, &r).iter().map(|t| t.key).collect();
        assert!(keys.contains(&"early_brake") && keys.contains(&"coast"), "{keys:?}");
        // The typical lap of three
        let a = CornerStats { v_min: 70.0, ..base() };
        let b = CornerStats { v_min: 74.0, ..base() };
        let c = CornerStats { v_min: 90.0, ..base() };
        assert_eq!(typical(&[&a, &b, &c]).unwrap().v_min, 74.0);
    }
}
