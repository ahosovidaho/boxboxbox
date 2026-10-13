//! Track map, live delta and track segments.
//!
//! GT7 reports no track, only the world position. The map is built from the driven line of every
//! complete lap; the fastest lap is the reference for the live delta.
//!
//! Segments: the first complete lap is cut into [`SEGMENTS`] pieces of equal length. Every later
//! lap is timed per segment and compared with the best time of that segment in the session:
//! purple = new best, green = within [`GREEN_MS`], red = slower (like the F1 timing colours).

use serde::Serialize;

/// Distance (m) between stored points of a lap
const SPACING: f32 = 2.0;
/// Farther away (m) the reference counts as lost (shortcut, pit, reset)
const MAX_OFF_LINE: f32 = 40.0;
const SEARCH_WINDOW: usize = 60;
/// Number of track segments
pub const SEGMENTS: usize = 20;
/// Up to this much slower than the best (ms) still counts as green
pub const GREEN_MS: i64 = 100;
/// A lap must end within this distance (m) of where it started to count as a full lap
/// (the first race lap on Nürburgring GP starts on the grid, ~350 m after the finish line)
const CLOSED_MAX: f32 = 80.0;

/// Result of a segment in a lap
pub const SEG_NONE: u8 = 0;
pub const SEG_PURPLE: u8 = 1;
pub const SEG_GREEN: u8 = 2;
pub const SEG_RED: u8 = 3;

#[derive(Clone, Copy, Serialize)]
pub struct Point {
    pub x: f32,
    pub z: f32,
    #[serde(skip)]
    pub t_ms: i64,
}

#[derive(Default)]
pub struct Track {
    current: Vec<Point>,
    /// true if the running lap is recorded from the start/finish line on
    current_complete: bool,
    best: Option<(i32, Vec<Point>)>,
    map: Vec<Point>,
    pub map_version: u32,
    ref_idx: usize,
    /// Geometry the segments are cut from, and where each segment starts in it
    seg_line: Vec<Point>,
    seg_starts: Vec<usize>,
    seg_idx: usize,
    /// Segment the car is in now
    seg_pos: Option<usize>,
    /// Segment being timed, and the lap time when the car entered it
    seg_timing: Option<(usize, i64)>,
    seg_best: Vec<Option<i64>>,
    /// Results of the running lap and of the previous lap
    pub seg_now: Vec<u8>,
    pub seg_last: Vec<u8>,
    /// Time of each segment in the running lap minus the best before it (ms)
    pub seg_delta: Vec<Option<i32>>,
}

#[derive(Serialize)]
pub struct MapData<'a> {
    pub version: u32,
    pub points: &'a [Point],
    /// Segment geometry and the index in it where each segment starts
    pub seg_line: &'a [Point],
    pub seg_starts: &'a [usize],
}

impl Track {
    pub fn reset(&mut self) {
        *self = Track { map_version: self.map_version + 1, ..Default::default() };
    }

    /// A new lap starts. `finished_ms` is the official time of the lap just finished.
    pub fn lap_started(&mut self, finished_ms: Option<i32>, from_line: bool) {
        // The last segment ends on the line
        if let (Some((k, t0)), Some(ms)) = (self.seg_timing, finished_ms) {
            if k + 1 == self.seg_starts.len() {
                self.seg_result(k, ms as i64 - t0);
            }
        }
        let closed = self.current_closed();
        let lap = std::mem::take(&mut self.current);
        if self.seg_line.is_empty() && self.current_complete && closed && finished_ms.is_some() && lap.len() > 20 {
            self.cut_segments(&lap);
        }
        if !self.seg_starts.is_empty() {
            self.seg_last = std::mem::replace(&mut self.seg_now, vec![SEG_NONE; self.seg_starts.len()]);
            self.seg_delta = vec![None; self.seg_starts.len()];
        }
        self.seg_idx = 0;
        self.seg_pos = (from_line && !self.seg_starts.is_empty()).then_some(0);
        self.seg_timing = (from_line && !self.seg_starts.is_empty()).then_some((0, 0));
        if let (true, Some(ms)) = (self.current_complete && closed, finished_ms) {
            if lap.len() > 20 {
                if self.best.as_ref().is_none_or(|(b, _)| ms <= *b) {
                    self.best = Some((ms, lap.clone()));
                }
                self.map = lap;
                self.map_version += 1;
            }
        }
        self.current_complete = from_line;
        self.ref_idx = 0;
    }

    /// Points of the running lap.
    pub fn current_points(&self) -> Vec<(f32, f32)> {
        self.current.iter().map(|p| (p.x, p.z)).collect()
    }

    /// Does the running lap end where it started (a full lap, not the short first race lap)?
    pub fn current_closed(&self) -> bool {
        match (self.current.first(), self.current.last()) {
            (Some(a), Some(b)) => dist(a, b.x, b.z) <= CLOSED_MAX,
            _ => false,
        }
    }

    pub fn add(&mut self, x: f32, z: f32, t_ms: i64) {
        let far = self.current.last().is_none_or(|p| dist(p, x, z) >= SPACING);
        if far {
            self.current.push(Point { x, z, t_ms });
            // Until there is a complete lap, the map grows live
            if self.map.is_empty() && self.current.len().is_multiple_of(25) {
                self.map_version += 1;
            }
        }
    }

    /// Is (x, z) at most `max` metres away from the known track?
    pub fn is_near(&self, x: f32, z: f32, max: f32) -> bool {
        self.map.iter().chain(&self.current).any(|p| dist(p, x, z) <= max)
    }

    pub fn map(&self) -> MapData<'_> {
        let points = if self.map.is_empty() { &self.current } else { &self.map };
        MapData { version: self.map_version, points, seg_line: &self.seg_line, seg_starts: &self.seg_starts }
    }

    fn cut_segments(&mut self, lap: &[Point]) {
        let mut cum = vec![0.0f32];
        for w in lap.windows(2) {
            cum.push(cum.last().unwrap() + dist(&w[0], w[1].x, w[1].z));
        }
        let total = *cum.last().unwrap();
        if total < 200.0 {
            return;
        }
        self.seg_starts = (0..SEGMENTS).map(|k| cum.iter().position(|d| *d >= total * k as f32 / SEGMENTS as f32).unwrap_or(0)).collect();
        self.seg_line = lap.to_vec();
        self.seg_best = vec![None; SEGMENTS];
        self.seg_now = vec![SEG_NONE; SEGMENTS];
        self.seg_last = vec![SEG_NONE; SEGMENTS];
        self.seg_delta = vec![None; SEGMENTS];
        self.map_version += 1;
    }

    fn seg_result(&mut self, k: usize, ms: i64) {
        if ms <= 0 {
            return;
        }
        let best = self.seg_best[k];
        self.seg_delta[k] = best.map(|b| (ms - b) as i32);
        self.seg_now[k] = match best {
            Some(b) if ms >= b + GREEN_MS => SEG_RED,
            Some(b) if ms >= b => SEG_GREEN,
            _ => SEG_PURPLE,
        };
        if best.is_none_or(|b| ms < b) {
            self.seg_best[k] = Some(ms);
        }
    }

    /// Times the segments; call with every position of a lap timed from the line.
    pub fn segment(&mut self, x: f32, z: f32, t_ms: i64) {
        if self.seg_line.len() < 2 {
            return;
        }
        let line = &self.seg_line;
        let lo = self.seg_idx.saturating_sub(5);
        let hi = (self.seg_idx + SEARCH_WINDOW).min(line.len() - 1);
        let nearest = |range: std::ops::RangeInclusive<usize>| range.map(|i| (i, dist(&line[i], x, z))).min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((mut idx, mut d)) = nearest(lo..=hi) else { return };
        if d > MAX_OFF_LINE / 2.0 {
            let Some(global) = nearest(0..=line.len() - 1) else { return };
            (idx, d) = global;
        }
        if d > MAX_OFF_LINE {
            // Off the line (pit lane, shortcut): this segment is not timed
            self.seg_timing = None;
            return;
        }
        self.seg_idx = idx;
        let seg = self.seg_starts.iter().rposition(|s| *s <= idx).unwrap_or(0);
        match self.seg_pos {
            Some(p) if p == seg => {}
            Some(p) if seg == p + 1 => {
                if let Some((k, t0)) = self.seg_timing.filter(|(k, _)| *k == p) {
                    self.seg_result(k, t_ms - t0);
                }
                self.seg_timing = Some((seg, t_ms));
            }
            // Jumped (reset, wrong way, lost track): time again from the next boundary
            _ => self.seg_timing = None,
        }
        self.seg_pos = Some(seg);
    }

    /// Time difference (ms) to the best lap at the current position; negative = faster.
    pub fn delta(&mut self, x: f32, z: f32, t_ms: i64) -> Option<i64> {
        let (_, best) = self.best.as_ref()?;
        if best.len() < 2 {
            return None;
        }
        let lo = self.ref_idx.saturating_sub(5);
        let hi = (self.ref_idx + SEARCH_WINDOW).min(best.len() - 1);
        let nearest = |range: std::ops::RangeInclusive<usize>| range.map(|i| (i, dist(&best[i], x, z))).min_by(|a, b| a.1.total_cmp(&b.1));
        let (mut idx, mut d) = nearest(lo..=hi)?;
        if d > MAX_OFF_LINE / 2.0 {
            (idx, d) = nearest(0..=best.len() - 1)?;
        }
        if d > MAX_OFF_LINE {
            return None;
        }
        self.ref_idx = idx;

        // Project onto the segment to the neighbouring point
        let (a, b) = if idx + 1 < best.len() { (&best[idx], &best[idx + 1]) } else { (&best[idx - 1], &best[idx]) };
        let (sx, sz) = (b.x - a.x, b.z - a.z);
        let len2 = sx * sx + sz * sz;
        let f = if len2 > 0.0 { (((x - a.x) * sx + (z - a.z) * sz) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let ref_t = a.t_ms as f32 + f * (b.t_ms - a.t_ms) as f32;
        Some(t_ms - ref_t as i64)
    }
}

fn dist(p: &Point, x: f32, z: f32) -> f32 {
    ((p.x - x).powi(2) + (p.z - z).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One lap on a circle of 1 km: `speed(fraction of lap)` in m/s, sampled at 60 Hz.
    fn lap(t: &mut Track, speed: impl Fn(f32) -> f32) -> i32 {
        let r = 1000.0 / std::f32::consts::TAU;
        let (mut s, mut ms) = (0.0f32, 0.0f64);
        while s < 1000.0 {
            let a = s / r;
            let (x, z) = (r * a.cos(), r * a.sin());
            t.add(x, z, ms as i64);
            t.segment(x, z, ms as i64);
            s += speed(s / 1000.0) / 60.0;
            ms += 1000.0 / 60.0;
        }
        ms as i32
    }

    #[test]
    fn segments_are_coloured_like_f1() {
        let mut t = Track::default();
        t.lap_started(None, true);
        let ms = lap(&mut t, |_| 50.0); // first lap: cuts the segments
        t.lap_started(Some(ms), true);
        assert_eq!(t.seg_starts.len(), SEGMENTS);
        let ms = lap(&mut t, |_| 50.0); // reference times: all purple
        t.lap_started(Some(ms), true);
        assert!(t.seg_last.iter().all(|s| *s == SEG_PURPLE), "{:?}", t.seg_last);
        // Faster in the first quarter, a bit slower (green) in the middle, much slower at the end
        let ms = lap(&mut t, |f| {
            if f < 0.25 {
                55.0
            } else if f < 0.5 {
                49.8
            } else {
                40.0
            }
        });
        let now = t.seg_now.clone();
        t.lap_started(Some(ms), true);
        assert_eq!(&now[..5], &[SEG_PURPLE; 5], "{now:?}");
        assert!(now[6..10].iter().all(|s| *s == SEG_GREEN), "{now:?}");
        assert!(t.seg_last[12..].iter().all(|s| *s == SEG_RED), "{:?}", t.seg_last);
        assert!(t.seg_now.iter().all(|s| *s == SEG_NONE));
    }

    #[test]
    fn short_first_race_lap_is_no_reference() {
        // Nürburgring GP: the race starts on the grid, the first lap ends at the finish line
        // ~350 m earlier – it must not become the reference or the segment geometry.
        let mut t = Track::default();
        t.lap_started(None, true);
        let r = 1000.0 / std::f32::consts::TAU;
        let mut ms = 0i64;
        let mut s = 350.0f32;
        while s < 1000.0 {
            let a = s / r;
            t.add(r * a.cos(), r * a.sin(), ms);
            s += 50.0 / 60.0;
            ms += 17;
        }
        assert!(!t.current_closed());
        t.lap_started(Some(ms as i32), true);
        assert!(t.seg_starts.is_empty() && t.best.is_none());
        let full = lap(&mut t, |_| 50.0);
        assert!(t.current_closed());
        t.lap_started(Some(full), true);
        assert_eq!(t.seg_starts.len(), SEGMENTS);
        assert_eq!(t.best.as_ref().map(|b| b.0), Some(full));
    }
}
