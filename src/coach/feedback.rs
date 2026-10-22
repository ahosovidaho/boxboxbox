//! Rule-based coaching texts: lap feedback, focus, session summary and the analysis files.
//! German, English or Czech and metric or imperial, see [`crate::i18n`].

use super::compare::{Comparison, CornerStats, LapResult};
use super::lap::{self, secs, t_loc};
use super::trailbrake::{self, TrailBraking};
use crate::i18n::{self, corner};
use crate::tr;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Display name of a trail-braking issue. The ids are the German names (they are stored in
/// `analysis/state.json`), so German shows them as they are.
pub fn issue_label(issue: &str) -> String {
    if i18n::german() {
        return issue.to_string();
    }
    if i18n::czech() {
        return match issue {
            "Rollphase" => "jízda bez pedálu",
            "kein Trail-Braking" => "bez trail brakingu",
            "G-Loch" => "propad gripu",
            "Blockieren" => "blokování",
            "Untersteuern" => "nedotáčivost",
            "Heck kommt" => "ujíždějící záď",
            "abrupt gelöst" => "prudce puštěná brzda",
            "nachgetreten" => "znovu přibrzděno",
            "zu viel Druck beim Einlenken" => "moc brzdy při zatočení",
            other => other,
        }
        .to_string();
    }
    match issue {
        "Rollphase" => "coasting",
        "kein Trail-Braking" => "no trail braking",
        "G-Loch" => "grip gap",
        "Blockieren" => "locking",
        "Untersteuern" => "understeer",
        "Heck kommt" => "rear stepping out",
        "abrupt gelöst" => "abrupt release",
        "nachgetreten" => "brake reapplied",
        "zu viel Druck beim Einlenken" => "too much pressure on turn-in",
        other => other,
    }
    .to_string()
}

/// Tips per trail-braking issue.
pub fn tip(issue: &str) -> String {
    match issue {
        "Rollphase" => tr!(
            "Bremse bis kurz vor den Scheitel ausschleichen und dann direkt ans Gas – keine Phase ohne Pedal.",
            "Trail the brake off until just before the apex, then straight to the throttle – no phase without a pedal.",
            "Brzdu povoluj plynule až těsně před vrchol, pak hned na plyn – žádná fáze bez pedálu."
        ),
        "kein Trail-Braking" => tr!(
            "Die Bremse nicht vor dem Einlenken ganz lösen, sondern mit 10–30 % in die Kurve mitnehmen.",
            "Don't release the brake fully before turning in – carry 10–30 % into the corner.",
            "Nepouštěj brzdu úplně před zatočením – vezmi do zatáčky 10–30 %."
        ),
        "G-Loch" => tr!(
            "Beim Einlenken die Bremse langsamer lösen, damit Bremsen und Lenken ineinander übergehen.",
            "Release the brake more slowly while turning in, so braking and steering blend into each other.",
            "Při zatáčení povoluj brzdu pomaleji, ať brzdění a řízení plynule přechází jedno v druhé."
        ),
        "Blockieren" => {
            tr!(
                "Mit zunehmendem Lenkeinschlag den Bremsdruck zurücknehmen.",
                "Ease off the brake pressure as you add steering.",
                "S přibývajícím natočením volantu ubírej tlak na brzdu."
            )
        }
        "Untersteuern" => tr!(
            "Weniger Bremsdruck beim Einlenken – die Vorderreifen sind überlastet.",
            "Less brake pressure on turn-in – the front tyres are overloaded.",
            "Méně brzdy při zatáčení – přední pneumatiky jsou přetížené."
        ),
        "Heck kommt" => tr!(
            "Beim Einlenken weniger Bremse – das Heck wird zu leicht.",
            "Less brake on turn-in – the rear gets too light.",
            "Méně brzdy při zatáčení – záď je příliš lehká."
        ),
        "abrupt gelöst" => tr!(
            "Die Bremse ausschleichen statt loszulassen.",
            "Trail the brake off instead of letting go.",
            "Brzdu povoluj plynule, nepouštěj ji najednou."
        ),
        "nachgetreten" => tr!(
            "Nach dem Einlenken nicht nachbremsen – lieber den Bremspunkt etwas früher setzen.",
            "Don't brake again after turning in – rather brake a little earlier.",
            "Po zatočení už znovu nebrzdi – raději začni brzdit o kousek dřív."
        ),
        "zu viel Druck beim Einlenken" => {
            tr!("Schon vor dem Einlenken Druck abbauen.", "Reduce the pressure before turning in.", "Ubírej tlak před zatočením.")
        }
        _ => String::new(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spin {
    pub lap_ms: Option<f64>,
    pub v: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tires {
    pub fl: i64,
    pub fr: i64,
    pub rl: i64,
    pub rr: i64,
}

/// Figures of one lap that don't need a reference lap.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LapMetrics {
    pub full_throttle: i64,
    pub braking: i64,
    pub coasting: i64,
    pub v_max: i64,
    pub spins: Vec<Spin>,
    pub tires: Tires,
    pub fuel_used: f64,
    pub race: bool,
    pub trail_braking: TrailBraking,
}

pub fn lap_metrics(path: &Path) -> std::io::Result<LapMetrics> {
    let mut rows = lap::load(path)?;
    let (ratio, _) = lap::steer_ratio(&rows);
    lap::add_dynamics(&mut rows, ratio);
    let n = rows.len().max(1) as f64;
    let pct = |pred: &dyn Fn(&lap::Row) -> bool| lap::round_i(rows.iter().filter(|r| pred(r)).count() as f64 / n * 100.0);
    let spins = lap::segments(&rows, |r| r.beta.is_some_and(|b| b.abs() > 20.0), 3, 6)
        .into_iter()
        .map(|(s, _)| Spin { lap_ms: rows[s].lap_ms, v: lap::round_i(rows[s].speed_kmh) })
        .collect();
    let temp = |w: usize| lap::round_i(lap::mean(rows.iter().map(|r| r.tire_temp[w])));
    Ok(LapMetrics {
        full_throttle: pct(&|r| r.throttle > 250.0),
        braking: pct(&|r| r.brake > 13.0),
        coasting: pct(&|r| r.throttle < 13.0 && r.brake < 13.0 && r.speed_kmh > 30.0),
        v_max: lap::round_i(rows.iter().map(|r| r.speed_kmh).fold(f64::MIN, f64::max)),
        spins,
        tires: Tires { fl: temp(0), fr: temp(1), rl: temp(2), rr: temp(3) },
        fuel_used: lap::round_to(rows.first().map_or(0.0, |r| r.fuel) - rows.last().map_or(0.0, |r| r.fuel), 2),
        race: rows.iter().take(5).any(|r| r.total_laps.unwrap_or(0.0) > 0.0),
        trail_braking: trailbrake::evaluate(&rows),
    })
}

/// What this lap did differently in a corner compared to the comparison lap.
pub fn causes(lc: &CornerStats, rc: &CornerStats) -> Vec<String> {
    let mut out = Vec::new();
    if let (Some(a), Some(b)) = (lc.brake_at, rc.brake_at) {
        let db = a - b;
        if db < -12.0 {
            let d = i18n::dist(-db);
            out.push(tr!("{d} früher gebremst", "braked {d} earlier", "brzdil o {d} dřív"));
        } else if db > 12.0 {
            let d = i18n::dist(db);
            out.push(tr!("{d} später gebremst", "braked {d} later", "brzdil o {d} později"));
        }
    }
    let dv = lc.v_min - rc.v_min;
    if dv.abs() >= 3.0 {
        let v = i18n::speed(dv.abs());
        out.push(if dv > 0.0 {
            tr!("Scheitel {v} schneller", "apex {v} faster", "vrchol o {v} rychleji")
        } else {
            tr!("Scheitel {v} langsamer", "apex {v} slower", "vrchol o {v} pomaleji")
        });
    }
    if let (Some(a), Some(b)) = (lc.full_at, rc.full_at) {
        let df = a - b;
        if df > 12.0 {
            let d = i18n::dist(df);
            out.push(tr!("{d} später Vollgas", "full throttle {d} later", "plný plyn o {d} později"));
        } else if df < -12.0 {
            let d = i18n::dist(-df);
            out.push(tr!("{d} früher Vollgas", "full throttle {d} earlier", "plný plyn o {d} dřív"));
        }
    }
    let de = lc.v_exit - rc.v_exit;
    if de.abs() >= 4.0 {
        let v = i18n::speed(de.abs());
        out.push(if de > 0.0 {
            tr!("Ausgang {v} schneller", "exit {v} faster", "výjezd o {v} rychleji")
        } else {
            tr!("Ausgang {v} langsamer", "exit {v} slower", "výjezd o {v} pomaleji")
        });
    }
    // Balance against the comparison lap in the same corner: a corner (or a car – karts) where the
    // rear always slides a little is no cause; only clearly more than in the best lap is
    if lc.under > 30.0 && lc.under > rc.under + 20.0 {
        let u = lc.under;
        out.push(tr!("{u:.0} % Untersteuern", "{u:.0} % understeer", "{u:.0} % nedotáčivost"));
    }
    if (lc.over > 25.0 && lc.over > rc.over + 20.0) || lc.rear_slide > 7.0f64.max(rc.rear_slide + 6.0) {
        out.push(issue_label("Heck kommt"));
    }
    out
}

fn with_causes(base: String, why: &[String], open: &str, close: &str) -> String {
    if why.is_empty() {
        format!("{base}.")
    } else {
        format!("{base}{open}{}{close}", why.join(", "))
    }
}

/// Most frequent issue name over trail-braking zones (first one wins ties, like `Counter`).
fn most_common<'a>(issues: impl Iterator<Item = &'a str>) -> Option<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for issue in issues {
        match counts.iter_mut().find(|(n, _)| n == issue) {
            Some(c) => c.1 += 1,
            None => counts.push((issue.to_string(), 1)),
        }
    }
    counts.into_iter().fold(None, |best: Option<(String, usize)>, c| match best {
        Some(b) if b.1 >= c.1 => Some(b),
        _ => Some(c),
    })
}

fn now_hm() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

pub fn lap_feedback(res: &Comparison, lap: &LapResult, metrics: &LapMetrics, prev_best: Option<i64>) -> Value {
    let (n, ms) = (lap.lap, lap.ms);
    let best_ms = res.laps.iter().map(|l| l.ms).min().unwrap_or(ms);
    let is_best = ms == best_ms;
    let mut tone = if is_best || ms - best_ms < 300 { "good" } else { "tip" };
    let t = t_loc(ms as f64);
    let (title, text) = match (is_best, prev_best.filter(|p| *p != 0)) {
        (true, Some(prev)) => {
            let gain = secs((prev - ms) as f64 / 1000.0, 3);
            (
                tr!("Runde {n}: {t} – neue Bestzeit!", "Lap {n}: {t} – new best!", "Kolo {n}: {t} – nové nejlepší!"),
                tr!(
                    "{gain} s schneller als deine bisherige Bestzeit.",
                    "{gain} s faster than your previous best.",
                    "O {gain} s rychleji než tvé dosavadní nejlepší."
                ),
            )
        }
        (true, None) => (
            tr!("Runde {n}: {t}", "Lap {n}: {t}", "Kolo {n}: {t}"),
            tr!(
                "Erste gezeitete Runde – ab der nächsten vergleiche ich Kurve für Kurve.",
                "First timed lap – from the next one on I compare corner by corner.",
                "První měřené kolo – od dalšího porovnávám zatáčku po zatáčce."
            ),
        ),
        (false, _) => {
            let gap = secs((ms - best_ms) as f64 / 1000.0, 3);
            let r = res.reference_lap().map_or(0, |l| l.lap);
            (
                tr!("Runde {n}: {t} (+{gap} s)", "Lap {n}: {t} (+{gap} s)", "Kolo {n}: {t} (+{gap} s)"),
                tr!(
                    "Vergleich mit deiner Bestzeit aus Runde {r}.",
                    "Compared with your best lap, lap {r}.",
                    "Ve srovnání s tvým nejlepším kolem, kolem {r}."
                ),
            )
        }
    };

    let mut points: Vec<String> = Vec::new();
    // The same advice structured for the dashboard: corner, short instruction, detail, seconds
    // ("tip": time to gain, "risk": spin/slide, "good": faster in another lap)
    let mut tips: Vec<Value> = Vec::new();
    for i in &lap.incidents {
        let what = if i.kind == "spin" { tr!("Dreher", "Spin", "Spin") } else { tr!("Rutscher", "Slide", "Ujetí zádi") };
        let at = i.corner.map(corner).unwrap_or_default();
        let (cause, tip) = (&i.cause, &i.tip);
        points.push(format!("⚠ {what} {at}: {cause}. {tip}"));
        tips.push(json!({ "n": i.corner, "kind": "risk", "short": format!("{what} – {cause}"), "text": tip }));
        tone = "warn";
    }
    if let Some(sp) = metrics.spins.first() {
        let (at, v) = (t_loc(sp.lap_ms.unwrap_or(0.0)), i18n::speed(sp.v as f64));
        points.push(tr!("Dreher/großer Rutscher bei {at} ({v}).", "Spin/big slide at {at} ({v}).", "Hodiny/velký smyk v {at} ({v})."));
        tone = "warn";
    }

    // Other normal laps (a lap with a spin is no example to follow)
    let others: Vec<&LapResult> = res.laps.iter().filter(|l| l.file != lap.file && (l.ms as f64) <= ms as f64 * INCIDENT).collect();
    if is_best && !others.is_empty() {
        // Where were other laps faster than this new best lap?
        let mut gains: Vec<(f64, usize, &LapResult)> = Vec::new();
        for c in &res.corners {
            let other = others.iter().copied().fold(None::<&LapResult>, |best, l| match best {
                Some(b) if b.corner(c.n).dt <= l.corner(c.n).dt => Some(b),
                _ => Some(l),
            });
            if let Some(other) = other {
                let oc = other.corner(c.n);
                if oc.dt < -0.05 && oc.stats.is_some() {
                    gains.push((oc.dt, c.n, other));
                }
            }
        }
        gains.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (dt, cn, other) in gains.into_iter().take(3) {
            if let (Some(o), Some(l), true) = (&other.corner(cn).stats, &lap.corner(cn).stats, i18n::czech()) {
                if let Some(t) = tips_text(l, o) {
                    let (c, ol, d) = (corner(cn), other.lap, secs(-dt, 2));
                    // The comparison here is that other lap, not the best one
                    let t = t.replace("nejlepším kole", &format!("kole {ol}")).replace("nejlepší kolo", &format!("kolo {ol}"));
                    let short = super::advice::corner_tips(l, o).first().map(|x| x.short.clone()).unwrap_or_default();
                    tips.push(json!({ "n": cn, "kind": "good", "short": short, "text": format!("V kole {ol} tu byl o {d} s rychlejší. {t}"), "dt": -dt }));
                    points.push(format!("{c}: v kole {ol} o {d} s rychleji – {t}"));
                    continue;
                }
            }
            let why = match (&other.corner(cn).stats, &lap.corner(cn).stats) {
                (Some(o), Some(l)) => causes(o, l),
                _ => Vec::new(),
            };
            let (c, l, d) = (corner(cn), other.lap, secs(-dt, 2));
            points.push(with_causes(
                tr!("{c}: In Runde {l} {d} s schneller", "{c}: {d} s faster in lap {l}", "{c}: o {d} s rychleji v kole {l}"),
                &why,
                " – ",
                ".",
            ));
        }
    } else if !is_best {
        if let Some(reference) = res.reference_lap() {
            let mut losses: Vec<(f64, usize)> = res.corners.iter().map(|c| (lap.corner(c.n).dt, c.n)).collect();
            losses.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
            if (lap.ms as f64) > reference.ms as f64 * INCIDENT {
                points.push(tr!(
                    "Runde mit Fehler oder Boxenstopp (über 107 % der Bestzeit): keine Kurvenanalyse.",
                    "Lap with a mistake or a pit stop (over 107 % of the best): no corner advice.",
                    "Kolo s chybou nebo zastávkou (o víc než 7 % pomalejší než nejlepší) – zatáčky nehodnotím, rady platí pro normální kola."
                ));
            }
            for (dt, cn) in losses.into_iter().take(3) {
                let Some(lc) = &lap.corner(cn).stats else { break };
                if dt < 0.05 {
                    break;
                }
                let (c, d) = (corner(cn), secs(dt, 2));
                // A spin / slide there: the warning above says it all, the figures are nonsense
                if lap.incidents.iter().any(|i| i.corner == Some(cn)) {
                    points.push(tr!("{c}: {d} s verloren (siehe oben).", "{c}: {d} s lost (see above).", "{c}: ztráta {d} s (viz výše)."));
                    continue;
                }
                // Czech: what to do (the engineer); other languages: what was different. Not for a
                // lap with a mistake or a pit stop (over 107 %): its corners are no lesson
                let normal = (lap.ms as f64) <= reference.ms as f64 * INCIDENT;
                if !normal {
                    points.push(tr!("{c}: {d} s verloren.", "{c}: {d} s lost.", "{c}: ztráta {d} s."));
                    continue;
                }
                if let Some(t) = reference.corner(cn).stats.as_ref().filter(|_| i18n::czech() && normal).and_then(|rc| tips_text(lc, rc)) {
                    let short = reference
                        .corner(cn)
                        .stats
                        .as_ref()
                        .and_then(|rc| super::advice::corner_tips(lc, rc).first().map(|x| x.short.clone()))
                        .unwrap_or_default();
                    tips.push(json!({ "n": cn, "kind": "tip", "short": short, "text": t, "dt": dt }));
                    points.push(format!("{c} (ztráta {d} s): {t}"));
                    continue;
                }
                let why = reference.corner(cn).stats.as_ref().map(|rc| causes(lc, rc)).unwrap_or_default();
                points.push(with_causes(tr!("{c}: {d} s verloren", "{c}: {d} s lost", "{c}: ztráta {d} s"), &why, " – ", "."));
            }
        }
    }

    let tb = &metrics.trail_braking;
    if let Some(score) = tb.score {
        let verdict = trailbrake::verdict(tb.score);
        let mut line =
            tr!("Trail-Braking {score}/100 ({verdict})", "Trail braking {score}/100 ({verdict})", "Trail braking {score}/100 ({verdict})");
        if let Some((issue, k)) = most_common(tb.zones.iter().flat_map(|z| z.issues.iter().map(|(i, _)| i.as_str()))) {
            if k >= 2 {
                let (label, zones, tip) = (issue_label(&issue), tb.zones.len(), tip(&issue));
                line += &tr!(
                    ". Häufigstes Thema: {label} ({k} von {zones} Bremszonen) – {tip}",
                    ". Most frequent issue: {label} ({k} of {zones} braking zones) – {tip}",
                    ". Nejčastější chyba: {label} ({k} z {zones} brzdných zón) – {tip}"
                );
            }
        }
        points.push(line);
    }

    let t = &metrics.tires;
    let mut stats = serde_json::Map::new();
    stats.insert(tr!("Vollgas", "Full throttle", "Plný plyn"), format!("{} %", metrics.full_throttle).into());
    stats.insert(tr!("Rollen", "Coasting", "Bez pedálu"), format!("{} %", metrics.coasting).into());
    let (front, rear) = (i18n::temp(t.fl.max(t.fr) as f64), i18n::temp(t.rl.max(t.rr) as f64));
    stats.insert(tr!("Reifen v/h", "Tyres f/r", "Pneu P/Z"), format!("{}/{rear}", front.split(' ').next().unwrap_or_default()).into());
    if let Some(th) = res.theoretical_ms.filter(|t| *t != 0.0) {
        stats.insert(tr!("Theoretisch", "Theoretical", "Teoreticky"), t_loc(th).into());
    }
    json!({
        "id": format!("lap-{n}-{ms}"), "kind": "lap", "lap": n, "tone": tone, "title": title, "text": text,
        "points": points, "tips": tips, "stats": stats, "time": now_hm(), "ms": ms, "best_ms": best_ms,
        "theoretical_ms": res.theoretical_ms.map(|t| t.round() as i64),
    })
}

/// The engineer's instructions for a corner (Czech): the two most important.
fn tips_text(l: &CornerStats, r: &CornerStats) -> Option<String> {
    let tips = super::advice::corner_tips(l, r);
    (!tips.is_empty()).then(|| tips.iter().take(2).map(|t| t.text.as_str()).collect::<Vec<_>>().join(" "))
}

/// The typical corner of the normal laps (not the best one) and the instruction for it, with how
/// many of the laps show the main pattern.
pub fn typical_tips(res: &Comparison, cn: usize, limit: f64) -> Option<String> {
    let reference = res.reference_lap()?;
    let rc = reference.corner(cn).stats.as_ref()?;
    let laps: Vec<&CornerStats> = res
        .laps
        .iter()
        .filter(|l| l.file != reference.file && (l.ms as f64) <= limit)
        .filter_map(|l| l.corner(cn).stats.as_ref())
        .collect();
    let typ = super::advice::typical(&laps)?;
    let tips = super::advice::corner_tips(&typ, rc);
    let main = tips.first()?;
    let n = laps.iter().filter(|l| super::advice::corner_tips(l, rc).iter().any(|t| t.key == main.key)).count();
    let text = tips.iter().take(2).map(|t| t.text.as_str()).collect::<Vec<_>>().join(" ");
    Some(if laps.len() >= 3 { format!("{text} (tak to vypadá v {n} z {} kol)", laps.len()) } else { text })
}

/// Laps slower than this × the best lap are incidents and left out of the focus.
const INCIDENT: f64 = 1.07;

/// Focus: the corner with the largest average loss over the normal laps (incidents left out);
/// cause taken from the latest lap.
pub fn focus_for(res: &Comparison, latest: &LapResult) -> Option<Value> {
    if res.laps.len() < 2 {
        return None;
    }
    let reference = res.reference_lap()?;
    // Laps with a spin, an excursion or a pit stop (over 107 % of the best) would dominate the
    // average: a 40 s spin is no lesson for that corner on the normal laps
    let limit = reference.ms as f64 * INCIDENT;
    let not_ref = || res.laps.iter().filter(|l| l.file != reference.file && (l.ms as f64) <= limit);
    not_ref().next()?;
    let (loss, cn) = res.corners.iter().map(|c| (lap::mean(not_ref().map(|l| l.corner(c.n).dt)), c.n)).fold(
        None::<(f64, usize)>,
        |best, p| match best {
            Some(b) if (b.0, b.1) >= (p.0, p.1) => Some(b),
            _ => Some(p),
        },
    )?;
    if loss < 0.05 {
        return Some(json!({"title": tr!("Fokus: konstant bleiben", "Focus: stay consistent", "Cíl: jezdi vyrovnaně"),
                           "text": tr!("Alle Kurven liegen nah an deiner Bestzeit – jetzt die Runde sauber wiederholen.",
                                       "All corners are close to your best – now repeat the lap cleanly.", "Všechny zatáčky jsou blízko tvého nejlepšího – teď kolo čistě zopakuj.")}));
    }
    // Cause from the latest lap; if that is the best lap itself, from the weakest lap there
    let src = if latest.file != reference.file && (latest.ms as f64) <= limit {
        latest
    } else {
        not_ref().fold(None::<&LapResult>, |best, l| match best {
            Some(b) if b.corner(cn).dt >= l.corner(cn).dt => Some(b),
            _ => Some(l),
        })?
    };
    let why = match (&src.corner(cn).stats, &reference.corner(cn).stats) {
        (Some(l), Some(r)) => causes(l, r),
        _ => Vec::new(),
    };
    let d = secs(loss, 2);
    if i18n::czech() {
        if let Some(t) = typical_tips(res, cn, limit) {
            let c = corner(cn);
            return Some(json!({"title": tr!("Fokus: {c}", "Focus: {c}", "Cíl: {c}"),
                               "text": format!("Tady ztrácíš na své nejlepší průměrně {d} s. {t}"), "corner": cn}));
        }
    }
    let text = with_causes(
        tr!(
            "Hier verlierst du im Schnitt {d} s auf deine Bestzeit",
            "Here you lose {d} s to your best on average",
            "Tady ztrácíš na své nejlepší průměrně {d} s"
        ),
        &why,
        &tr!(" (zuletzt: ", " (last time: ", " (minule: "),
        ").",
    );
    let c = corner(cn);
    Some(json!({"title": tr!("Fokus: {c}", "Focus: {c}", "Cíl: {c}"), "text": text, "corner": cn}))
}

pub fn save_lap(dir: &Path, lap: &LapResult, metrics: &LapMetrics, item: &Value) -> std::io::Result<()> {
    let stem = lap.file.trim_end_matches(".csv");
    let data = json!({"lap": lap, "metrics": metrics, "feedback": item});
    fs::write(dir.join(format!("{stem}.json")), serde_json::to_string_pretty(&data).unwrap_or_default())?;
    let s = |k: &str| item[k].as_str().unwrap_or_default().to_string();
    let mut md = vec![format!("# {}", s("title")), String::new(), s("text"), String::new()];
    md.extend(item["points"].as_array().into_iter().flatten().map(|p| format!("- {}", p.as_str().unwrap_or_default())));
    md.extend([String::new(), tr!("## Trail-Braking", "## Trail braking", "## Trail braking"), String::new()]);
    for z in &metrics.trail_braking.zones {
        let iss = z.issues.iter().map(|(a, b)| format!("{} ({b})", issue_label(a))).collect::<Vec<_>>().join("; ");
        let (at, v0, v1, unit, score) =
            (t_loc(z.lap_ms.unwrap_or(0.0)), i18n::speed_value(z.v_start), i18n::speed_value(z.v_apex), i18n::speed_unit(), z.score);
        let iss = if iss.is_empty() { tr!("sauber", "clean", "čisté") } else { iss };
        md.push(format!("- {at} {v0:.0}→{v1:.0} {unit}: {score}/100 – {iss}"));
    }
    md.extend([
        String::new(),
        tr!("## Kurven (Δ zur Bestzeit)", "## Corners (Δ to best lap)", "## Zatáčky (Δ k nejlepšímu kolu)"),
        String::new(),
    ]);
    for c in &lap.corners {
        if let Some(st) = &c.stats {
            let (k, dt, apex, exit, under, over) = (corner(c.n), c.dt, i18n::speed(st.v_min), i18n::speed(st.v_exit), st.under, st.over);
            md.push(tr!(
                "- {k}: {dt:+.3} s, Scheitel {apex}, Ausgang {exit}, Untersteuern {under:.0} %, Übersteuern {over:.0} %",
                "- {k}: {dt:+.3} s, apex {apex}, exit {exit}, understeer {under:.0} %, oversteer {over:.0} %",
                "- {k}: {dt:+.3} s, vrchol {apex}, výjezd {exit}, nedotáčivost {under:.0} %, přetáčivost {over:.0} %"
            ));
        }
    }
    fs::write(dir.join(format!("{stem}.md")), md.join("\n") + "\n")
}

fn session_date(name: &str) -> String {
    let format = match i18n::lang() {
        i18n::Lang::De => "%d.%m.%Y, %H:%M Uhr",
        i18n::Lang::Cs => "%-d. %-m. %Y, %H:%M",
        i18n::Lang::En => "%b %-d, %Y, %H:%M",
    };
    chrono::NaiveDateTime::parse_from_str(name.get(..19).unwrap_or(name), "%Y-%m-%d_%H-%M-%S")
        .map(|t| t.format(format).to_string())
        .unwrap_or_else(|_| name.to_string())
}

/// Car name: from session.json if present, otherwise from the folder name.
fn car_of(session: &Path, name: &str) -> String {
    let from_meta = fs::read_to_string(session.join("session.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|m| m["car"].as_str().map(str::to_string));
    from_meta.unwrap_or_else(|| {
        if name.chars().count() > 20 {
            name.chars().skip(20).collect::<String>().replace("__", " '").replace('_', " ").trim().to_string()
        } else {
            name.to_string()
        }
    })
}

pub fn summary(session: &Path, dir: &Path, res: &Comparison, metrics: &BTreeMap<String, LapMetrics>) -> std::io::Result<Value> {
    let name = session.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut laps: Vec<&LapResult> = res.laps.iter().collect();
    laps.sort_by_key(|l| l.lap);
    let times: Vec<f64> = laps.iter().map(|l| l.ms as f64).collect();
    let best = times.iter().copied().fold(f64::INFINITY, f64::min);
    let clean: Vec<f64> = times.iter().copied().filter(|t| *t < best * 1.03).collect();
    let race = metrics.values().any(|m| m.race);
    let kind = if race { tr!("Rennen", "race", "závod") } else { tr!("Zeitfahren", "time trial", "časovka") };
    let reference = laps[times.iter().position(|t| *t == best).unwrap_or(0)];
    let (count, best_t, best_lap) = (laps.len(), t_loc(best), reference.lap);
    let (avg, spread) = (t_loc(lap::mean(clean.iter().copied())), secs(lap::pstdev(&clean) / 1000.0, 2));
    let mut points = vec![
        tr!(
            "{count} gezeitete Runden, Bestzeit {best_t} (Runde {best_lap}).",
            "{count} timed laps, best {best_t} (lap {best_lap}).",
            "{count} měřených kol, nejlepší {best_t} (kolo {best_lap})."
        ),
        tr!(
            "Schnitt der sauberen Runden {avg}, Streuung ±{spread} s.",
            "Average of the clean laps {avg}, spread ±{spread} s.",
            "Průměr čistých kol {avg}, rozptyl ±{spread} s."
        ),
    ];
    if times.len() > 1 && times[0] > best {
        let d = secs((times[0] - best) / 1000.0, 2);
        points.push(tr!(
            "Verbesserung von der ersten zur besten Runde: {d} s.",
            "Improvement from the first to the best lap: {d} s.",
            "Zlepšení z prvního na nejlepší kolo: {d} s."
        ));
    }
    if let Some(th) = res.theoretical_ms.filter(|t| *t != 0.0 && *t < best - 50.0) {
        let (t, d) = (t_loc(th), secs((best - th) / 1000.0, 2));
        points.push(tr!(
            "Theoretische Bestzeit aus deinen besten Kurven: {t} (−{d} s).",
            "Theoretical best from your best corners: {t} (−{d} s).",
            "Teoretické nejlepší z tvých nejlepších zatáček: {t} (−{d} s)."
        ));
    }
    let mut pot: Vec<(f64, usize)> = res
        .corners
        .iter()
        .filter_map(|c| {
            let vals: Vec<f64> =
                laps.iter().filter(|l| l.file != reference.file && (l.ms as f64) <= best * INCIDENT).map(|l| l.corner(c.n).dt).collect();
            (!vals.is_empty()).then(|| (lap::mean(vals), c.n))
        })
        .collect();
    pot.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    let top: Vec<String> = pot
        .iter()
        .take(3)
        .filter(|(v, _)| *v > 0.03)
        .map(|(v, n)| {
            let (c, d) = (corner(*n), secs(*v, 2));
            tr!("{c} (Ø {d} s)", "{c} (avg {d} s)", "{c} (prům. {d} s)")
        })
        .collect();
    if !top.is_empty() {
        let top = top.join(", ");
        points.push(tr!("Größtes Potenzial: {top}.", "Biggest potential: {top}.", "Největší rezerva: {top}."));
        // … and what to do there
        if i18n::czech() {
            for (v, n) in pot.iter().take(3).filter(|(v, _)| *v > 0.03) {
                if let Some(t) = typical_tips(res, *n, best * INCIDENT) {
                    let (c, d) = (corner(*n), secs(*v, 2));
                    points.push(format!("{c} (prům. {d} s): {t}"));
                }
            }
        }
    }
    let tbs: Vec<(i64, i64)> =
        laps.iter().filter_map(|l| metrics.get(&l.file).and_then(|m| m.trail_braking.score).map(|s| (l.lap, s))).collect();
    if !tbs.is_empty() {
        let avg = lap::round_i(lap::mean(tbs.iter().map(|(_, s)| *s as f64)));
        let verdict = trailbrake::verdict(Some(avg));
        let mut line = tr!(
            "Trail-Braking Ø {avg}/100 ({verdict})",
            "Trail braking avg {avg}/100 ({verdict})",
            "Trail braking prům. {avg}/100 ({verdict})"
        );
        if tbs.len() > 1 {
            let ((l0, s0), (l1, s1)) = (tbs[0], tbs[tbs.len() - 1]);
            line += &tr!(", von {s0} (R{l0}) auf {s1} (R{l1})", ", from {s0} (L{l0}) to {s1} (L{l1})", ", od {s0} (K{l0}) po {s1} (K{l1})");
        }
        let issues = laps
            .iter()
            .filter_map(|l| metrics.get(&l.file))
            .flat_map(|m| m.trail_braking.zones.iter().flat_map(|z| z.issues.iter().map(|(i, _)| i.as_str())));
        if let Some((issue, _)) = most_common(issues) {
            let (label, tip) = (issue_label(&issue), tip(&issue));
            line += &tr!(". Hauptthema: {label} – {tip}", ". Main issue: {label} – {tip}", ". Hlavní chyba: {label} – {tip}");
        }
        points.push(line);
    }
    let spins: usize = metrics.values().map(|m| m.spins.len()).sum();
    if spins > 0 {
        points.push(tr!(
            "{spins} Dreher/große Rutscher in den gezeiteten Runden.",
            "{spins} spins/big slides in the timed laps.",
            "{spins}× hodiny/velký smyk v měřených kolech."
        ));
    }
    if race {
        let fuel: Vec<f64> = metrics.values().map(|m| m.fuel_used).filter(|f| *f > 0.0).collect();
        if !fuel.is_empty() {
            let f = i18n::fuel(lap::mean(fuel), 2);
            points.push(tr!("Spritverbrauch Ø {f} pro Runde.", "Fuel use avg {f} per lap.", "Spotřeba prům. {f} na kolo."));
        }
    }
    let (car, date) = (car_of(session, &name), session_date(&name));
    let mut stats = serde_json::Map::new();
    stats.insert(tr!("Runden", "Laps", "Kola"), laps.len().into());
    stats.insert(tr!("Bestzeit", "Best", "Nejlepší"), t_loc(best).into());
    let item = json!({
        "id": format!("summary-{name}-{}", laps.len()), "kind": "summary", "tone": "info",
        "title": tr!("Zusammenfassung {kind}: {car}", "Summary {kind}: {car}", "Souhrn {kind}: {car}"),
        "text": tr!("Session vom {date}.", "Session of {date}.", "Session z {date}."),
        "points": points, "stats": stats, "time": now_hm(),
    });
    let data = json!({
        "item": item,
        "laps": laps.iter().map(|l| json!({"lap": l.lap, "ms": l.ms})).collect::<Vec<_>>(),
        "trail_braking": tbs,
    });
    fs::write(dir.join("summary.json"), serde_json::to_string_pretty(&data).unwrap_or_default())?;
    let mut md = vec![format!("# {}", item["title"].as_str().unwrap_or_default()), String::new()];
    md.push(item["text"].as_str().unwrap_or_default().to_string());
    md.push(String::new());
    md.extend(points.iter().map(|p| format!("- {p}")));
    md.extend([
        String::new(),
        tr!("| Runde | Zeit | Δ Bestzeit |", "| Lap | Time | Δ best |", "| Kolo | Čas | Δ nejlepší |"),
        "|---|---|---|".into(),
    ]);
    md.extend(laps.iter().map(|l| format!("| {} | {} | {} |", l.lap, t_loc(l.ms as f64), secs((l.ms as f64 - best) / 1000.0, 3))));
    fs::write(dir.join("summary.md"), md.join("\n") + "\n")?;
    Ok(item)
}

/// Radio: what changed over the last laps, for the race page (and practice). Czech only.
/// - a corner that costs more in each of the last 3 normal laps than it usually does, with what
///   changed there (rear sliding more = rear tyres going, front pushing, braking earlier …);
/// - the pace of the last 3 normal laps against the best 3;
/// - a spin or slide in the same corner in two laps running.
pub fn radio(res: &Comparison) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(reference) = res.reference_lap() else { return out };
    let limit = reference.ms as f64 * INCIDENT;
    // Only the current run: a restart in GT7 (lap numbers start again) is a new stint
    let start = res.laps.windows(2).rposition(|w| w[1].lap <= w[0].lap).map_or(0, |i| i + 1);
    let normal: Vec<&LapResult> = res.laps[start..].iter().filter(|l| (l.ms as f64) <= limit).collect();
    let fmt = |v: f64| format!("{:.2}", v).replace('.', ",");
    // Two laps running with an incident in the same corner
    if let [.., a, b] = res.laps.as_slice() {
        for i in &b.incidents {
            if let Some(n) = i.corner.filter(|n| a.incidents.iter().any(|j| j.corner == Some(*n))) {
                out.push(json!({ "level": "warn", "corner": n,
                    "text": format!("T{n}: druhé kolo po sobě ti tu ujela záď – uber o kousek, ať nepřijde spin.") }));
                break;
            }
        }
    }
    if normal.len() >= 6 {
        let last = &normal[normal.len() - 3..];
        let earlier = &normal[..normal.len() - 3];
        let mut found: Vec<(f64, Value)> = Vec::new();
        for c in &res.corners {
            let mut usual: Vec<f64> = earlier.iter().map(|l| l.corner(c.n).dt).collect();
            usual.sort_by(f64::total_cmp);
            let med = usual[usual.len() / 2];
            let extra: Vec<f64> = last.iter().map(|l| l.corner(c.n).dt - med).collect();
            let mean = extra.iter().sum::<f64>() / 3.0;
            if extra.iter().all(|e| *e > 0.05) && mean >= 0.1 {
                let then: Vec<&CornerStats> = earlier.iter().filter_map(|l| l.corner(c.n).stats.as_ref()).collect();
                let now: Vec<&CornerStats> = last.iter().filter_map(|l| l.corner(c.n).stats.as_ref()).collect();
                let why = match (super::advice::typical(&then), super::advice::typical(&now)) {
                    (Some(t), Some(n)) if n.rear_slide > t.rear_slide + 3.0 => {
                        "záď klouže víc než dřív – zadní gumy odcházejí: plyn přidávej postupně, šetři je".to_string()
                    }
                    (Some(t), Some(n)) if n.under > t.under + 15.0 => {
                        "předek víc nedotáčí – přední gumy odcházejí: vjeď pomaleji, dřív pusť brzdu".to_string()
                    }
                    (Some(t), Some(n)) if matches!((n.brake_at, t.brake_at), (Some(a), Some(b)) if b - a > 8.0) => {
                        let d = t.brake_at.unwrap_or(0.0) - n.brake_at.unwrap_or(0.0);
                        format!("brzdíš o {} m dřív než na začátku", d.round() as i64)
                    }
                    (Some(t), Some(n)) if n.v_min < t.v_min - 3.0 => {
                        format!("vrcholem jedeš o {} km/h pomaleji než dřív", (t.v_min - n.v_min).round() as i64)
                    }
                    _ => "drž stopu z prvních kol".to_string(),
                };
                found.push((
                    mean,
                    json!({ "level": "info", "corner": c.n,
                    "text": format!("T{}: poslední 3 kola +{} s proti normálu – {why}.", c.n, fmt(mean)) }),
                ));
            }
        }
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.extend(found.into_iter().take(2).map(|f| f.1));
        // Pace
        let mut best: Vec<f64> = normal.iter().map(|l| l.ms as f64).collect();
        best.sort_by(f64::total_cmp);
        let best3 = best.iter().take(3).sum::<f64>() / 3.0;
        let last3 = last.iter().map(|l| l.ms as f64).sum::<f64>() / 3.0;
        let latest = last[2].ms as f64;
        if last3 - best3 > 400.0 && latest - best3 > 400.0 {
            out.push(json!({ "level": "info",
                "text": format!("Tempo: poslední 3 kola o {} s pomalejší než tvoje nejlepší tři.", fmt((last3 - best3) / 1000.0)) }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spin in one lap (40 s lost in Z14) must not make Z14 the focus for the normal laps.
    #[test]
    fn focus_ignores_incident_laps() {
        let lap = |file: &str, ms: i64, dts: [f64; 2]| {
            json!({ "file": file, "lap": 1, "ms": ms, "delta_ms": 0,
                    "corners": [{ "n": 1, "dt": dts[0] }, { "n": 14, "dt": dts[1] }] })
        };
        let corner = |n: usize| json!({ "n": n, "dir": "L", "x": 0.0, "z": 0.0, "s": 0, "ref": null });
        let res: Comparison = serde_json::from_value(json!({
            "reference": { "file": "a", "ms": 94_218, "lap": 4 },
            "steer_ratio": 15.0,
            "corners": [corner(1), corner(14)],
            "laps": [lap("a", 94_218, [0.0, 0.0]), lap("b", 94_530, [0.33, 0.17]), lap("c", 95_121, [0.30, 0.40]),
                     lap("spin", 156_554, [0.0, 40.97])],
            "theoretical_ms": null,
        }))
        .unwrap();
        let latest = res.laps.iter().find(|l| l.file == "spin").unwrap();
        let f = focus_for(&res, latest).unwrap();
        // Z1 loses 0.32 s on average on the normal laps, Z14 only 0.29 s (with the spin: 13.8 s)
        assert_eq!(f["corner"], 1, "{f}");
        assert!(f["text"].as_str().unwrap().contains(" 0,3"), "{f}");
        // Only incident laps besides the best: no focus
        let mut only = res.clone();
        only.laps.retain(|l| l.file == "a" || l.file == "spin");
        assert!(focus_for(&only, latest).is_none());
    }

    /// Rear tyres going off: Z2 loses more in each of the last 3 laps and the rear slides more.
    #[test]
    fn radio_names_a_corner_that_gets_worse() {
        let st = |slide: f64| {
            json!({ "brake_at": 100.0, "brake_max": 80.0, "v_entry": 200.0, "v_min": 90.0, "v_exit": 150.0,
            "full_at": 200.0, "gear_min": 3, "under": 0.0, "over": 0.0, "rear_slide": slide, "glat": 2.0, "tcs": 0.0 })
        };
        let lap = |n: i64, ms: i64, dt2: f64, slide: f64| {
            let mut c2 = st(slide);
            c2["n"] = json!(2);
            c2["dt"] = json!(dt2);
            json!({ "file": format!("{n:03}"), "lap": n, "ms": ms, "delta_ms": 0, "corners": [{ "n": 1, "dt": 0.0 }, c2] })
        };
        let corner = |n: usize| json!({ "n": n, "dir": "L", "x": 0.0, "z": 0.0, "s": 0, "ref": null });
        let mut laps = vec![];
        for n in 1..=5 {
            laps.push(lap(n, 90_000 + n * 10, 0.02, 3.0));
        }
        for n in 6..=8 {
            laps.push(lap(n, 90_300, 0.25, 9.0));
        }
        let res: Comparison = serde_json::from_value(json!({
            "reference": { "file": "001", "ms": 90_010, "lap": 1 }, "steer_ratio": 15.0,
            "corners": [corner(1), corner(2)], "laps": laps, "theoretical_ms": null,
        }))
        .unwrap();
        let r = radio(&res);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0]["corner"], 2);
        assert!(r[0]["text"].as_str().unwrap().contains("zadní gumy"), "{r:?}");
    }
}
