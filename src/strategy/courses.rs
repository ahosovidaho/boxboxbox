//! Catalogue of the GT7 track layouts from the community database (github.com/ddm999/gt7info,
//! `course.csv` and `crsbase.csv`): every layout with its base track, length, layout number,
//! direction (reverse), number of corners and pit lane delta.
//!
//! GT7 does not send the track in its telemetry: a layout is recognised by the shape of a lap
//! ([`super::practice`]). The catalogue names the layouts (so "GP" and "GP without chicane", or a
//! layout and its reverse, never mix), offers candidates for an unknown lap by its length, and
//! knows names as the championship website writes them ("Okruh Yas Marina", the base track only).

use super::practice::track_key;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Course {
    pub id: i32,
    pub name: String,
    pub base: i32,
    pub base_name: String,
    pub length_m: f32,
    pub layout: i32,
    pub reverse: bool,
    pub corners: Option<i32>,
    /// Time lost driving through the pit lane, s
    pub pit_delta: Option<f32>,
    pub category: String,
}

static COURSES: OnceLock<Vec<Course>> = OnceLock::new();

fn load() -> Vec<Course> {
    let bases: Vec<(i32, String)> = include_str!("../../data/crsbase.csv")
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut f = l.split(',');
            Some((f.next()?.parse().ok()?, f.next()?.to_string()))
        })
        .collect();
    let num = |s: &str| s.parse::<f32>().ok();
    let mut out: Vec<Course> = include_str!("../../data/course.csv")
        .lines()
        .skip(1)
        .filter_map(|l| {
            // ID,Name,Base,Country,Category,Length,LongestStraight,ElevationDiff,Altitude,MinTimeH,
            // MinTimeM,MinTimeS,MaxTimeH,MaxTimeM,MaxTimeS,LayoutNumber,IsReverse,PitLaneDelta,IsOval,
            // NumCorners,NoRain
            let f: Vec<&str> = l.split(',').collect();
            if f.len() < 21 {
                return None;
            }
            let base: i32 = f[2].parse().ok()?;
            Some(Course {
                id: f[0].parse().ok()?,
                name: f[1].to_string(),
                base,
                base_name: bases.iter().find(|b| b.0 == base).map(|b| b.1.clone()).unwrap_or_default(),
                length_m: num(f[5])?,
                layout: f[15].parse().unwrap_or(1),
                reverse: f[16] == "1",
                corners: f[19].parse().ok(),
                pit_delta: num(f[17]).filter(|v| *v > 0.0),
                category: f[4].to_string(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.base_name.cmp(&b.base_name).then(a.layout.cmp(&b.layout)));
    out
}

pub fn all() -> &'static [Course] {
    COURSES.get_or_init(load)
}

pub fn get(id: i32) -> Option<&'static Course> {
    all().iter().find(|c| c.id == id)
}

/// Names as the championship website and earlier versions write them, by base track.
const ALIASES: &[(&str, i32)] = &[
    ("barcelona", 10),
    ("barcelonacatalunya", 10),
    ("catalunya", 10),
    ("okruhyasmarina", 40),
    ("yasmarina", 40),
    ("okruhgillesvilleneuve", 41),
    ("gillesvilleneuve", 41),
    ("circuitdelasarthelemans", 1),
    ("lemans", 1),
    ("fujispeedway", 18),
    ("spafrancorchamps", 12),
    ("autodromojosecarlospaceinterlagos", 5),
    ("interlagos", 5),
    ("bathurstmountpanorama", 22),
    ("michelinracewayroadatlanta", 37),
    ("watkinsglen", 35),
    ("lakelouisesnow", 38),
];

/// The layout a track name stands for, if the name is one of the catalogue (ignoring case and
/// punctuation; "Nürburgring GP" = "Nurburgring GP").
pub fn by_name(name: &str) -> Option<&'static Course> {
    let k = track_key(name);
    if k.is_empty() {
        return None;
    }
    all().iter().find(|c| track_key(&c.name) == k)
}

/// The base track a name stands for: a layout name, a base name, a known alias, or a name that
/// starts with a base name ("Circuit de Barcelona-Catalunya – GP bez šikany").
pub fn base_of(name: &str) -> Option<i32> {
    if let Some(c) = by_name(name) {
        return Some(c.base);
    }
    let k = track_key(name);
    if k.is_empty() {
        return None;
    }
    if let Some((_, b)) = ALIASES.iter().find(|(a, _)| track_key(a) == k) {
        return Some(*b);
    }
    // The longest base name the name starts with (or that starts with the name)
    all()
        .iter()
        .map(|c| (c.base, track_key(&c.base_name)))
        .filter(|(_, b)| !b.is_empty() && (k.starts_with(b.as_str()) || b.starts_with(k.as_str()) && k.len() >= 6))
        .max_by_key(|(_, b)| b.len())
        .map(|(b, _)| b)
}

/// Layouts a lap of `length_m` may be (the driven line is a little shorter than the official
/// length), closest first.
pub fn candidates(length_m: f32, base: Option<i32>) -> Vec<&'static Course> {
    let mut v: Vec<&Course> = all()
        .iter()
        .filter(|c| base.is_none_or(|b| c.base == b))
        .filter(|c| {
            let r = length_m / c.length_m;
            (0.93..=1.03).contains(&r)
        })
        .collect();
    v.sort_by(|a, b| ((length_m / a.length_m) - 0.98).abs().total_cmp(&((length_m / b.length_m) - 0.98).abs()));
    v
}

/// Time lost driving through the pit lane on a track (s): of the layout the name stands for, or of
/// a base track whose layouts all share one pit lane delta.
pub fn pit_delta_of(name: &str) -> Option<f32> {
    if let Some(c) = by_name(name) {
        return c.pit_delta;
    }
    let base = base_of(name)?;
    let v: Vec<f32> = all().iter().filter(|c| c.base == base).filter_map(|c| c.pit_delta).collect();
    (!v.is_empty() && v.iter().all(|d| (d - v[0]).abs() < 0.5)).then(|| v[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_and_names() {
        assert!(all().len() > 100);
        let bcn = get(1249).unwrap();
        assert_eq!(bcn.base_name, "Circuit de Barcelona-Catalunya");
        assert!(!bcn.reverse && bcn.layout == 2);
        assert!(get(452).unwrap().reverse);
        assert_eq!(by_name("Nürburgring GP").map(|c| c.id), Some(349));
        assert_eq!(base_of("Circuit de Barcelona-Catalunya – GP bez šikany"), Some(10));
        assert_eq!(base_of("Circuit de Barcelona-Catalunya"), Some(10));
        assert_eq!(base_of("Okruh Yas Marina"), Some(40));
        assert_eq!(base_of("Yas Marina Circuit"), Some(40));
        assert_eq!(base_of("Willow Springs International Raceway"), Some(34));
        assert_eq!(base_of("Nürburgring"), Some(24));
        assert_eq!(base_of("Barcelona"), Some(10));
        // Our Barcelona lap (4575 m driven): GP without chicane and GP are both candidates
        let c: Vec<i32> = candidates(4575.0, Some(10)).iter().map(|c| c.id).collect();
        assert!(c.contains(&1249) && c.contains(&874), "{c:?}");
        // Kart on Willow Springs (~1575 m): Horse Thief Mile, both directions
        let w: Vec<i32> = candidates(1575.0, Some(34)).iter().map(|c| c.id).collect();
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(pit_delta_of("Circuit de Barcelona-Catalunya"), Some(28.0));
        assert_eq!(pit_delta_of("Nürburgring GP"), Some(23.0));
    }
}
