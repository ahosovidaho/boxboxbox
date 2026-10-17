//! Derived state: lap times, fuel per lap, tyre slip, vehicle dynamics, track map, measurements.
//! Built from the packet stream and sent to the clients as a JSON snapshot.
//!
//! The tracker also decides the driving sessions (one session = one car on one track): a session
//! starts as soon as the car is driven and ends after a long pause / menu, when the track is left,
//! when no data arrives any more or when the car changes. Driving on with the same car on the same
//! track afterwards continues the session.

use super::recorder::{Msg, Recorder};
use super::track_map::Track;
use crate::perf;
use crate::strategy;
use crate::telemetry::packet::{flags, Packet};
use crate::telemetry::{cars, dynamics, Link};
use crate::tr;
use serde::Serialize;
use std::time::{Duration, Instant};

const TICKS_PER_SEC: f64 = 60.0;
/// Paused this long (pause menu, back in the main menu) = session over
const SESSION_PAUSE_END: Duration = Duration::from_secs(180);
/// Off the track or without any data this long = session over
const SESSION_OFF_END: Duration = Duration::from_secs(60);
/// Driving on this close to the known track (m) continues the session
const RESUME_DISTANCE: f32 = 50.0;
const FUEL_HISTORY: usize = 5;
/// Standing still this long in a race lap counts as a pit stop
const PIT_STILL: Duration = Duration::from_secs(5);
/// After a stop, the tyre temperature is watched this long while driving …
const TYRE_CHECK: Duration = Duration::from_secs(30);
/// … and a drop of at least this much (°C, mean of the four) means new tyres
const TYRE_DROP: f32 = 8.0;
const LAP_HISTORY: usize = 8;
/// Smoothing of the balance display (packets): ~0.25 s
const BALANCE_SMOOTHING: f32 = 15.0;

#[derive(Serialize, Default, Clone)]
pub struct Snapshot {
    pub live: bool,
    pub on_track: bool,
    pub paused: bool,
    pub loading: bool,

    pub speed_kmh: f32,
    pub rpm: f32,
    pub rpm_alert_min: i16,
    pub rpm_alert_max: i16,
    /// 0 = reverse, 15 = neutral (as GT7 sends it)
    pub gear: u8,
    pub suggested_gear: Option<u8>,
    pub throttle: f32,
    pub brake: f32,

    /// FL, FR, RL, RR
    pub tire_temp: [f32; 4],
    /// Wheel surface speed / car speed (1.0 = no slip), `null` when standing
    pub tire_slip: [Option<f32>; 4],
    pub wheel_speed_kmh: [f32; 4],
    pub tire_radius: [f32; 4],
    pub suspension: [f32; 4],

    pub fuel: f32,
    pub fuel_capacity: f32,
    pub fuel_per_lap: Option<f32>,
    pub fuel_laps_left: Option<f32>,

    pub lap: i16,
    pub total_laps: i16,
    pub current_lap_ms: Option<i64>,
    pub last_lap_ms: Option<i32>,
    pub best_lap_ms: Option<i32>,
    pub live_delta_ms: Option<i64>,
    pub laps: Vec<i32>,

    pub position: Option<i16>,
    pub cars: Option<i16>,

    pub water_temp: f32,
    pub oil_temp: f32,
    pub oil_pressure: f32,
    pub boost_bar: Option<f32>,
    pub tcs: bool,
    pub asm: bool,
    pub rev_limiter: bool,
    pub handbrake: bool,
    pub in_gear: bool,
    pub lights: bool,
    pub high_beam: bool,
    pub flags: u16,

    pub car_code: i32,
    pub car_name: Option<&'static str>,
    pub calc_max_speed: i16,
    pub transmission_top_speed_kmh: f32,
    pub gear_ratios: Vec<f32>,
    pub clutch_pedal: f32,
    pub clutch_engagement: f32,
    pub rpm_clutch_gearbox: f32,
    pub energy_recovery: f32,
    pub time_of_day_ms: i32,

    /// World position (m) and attitude (degrees)
    pub pos: [f32; 3],
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub body_height: f32,
    pub road_plane_distance: f32,
    pub map_version: u32,

    pub steering_deg: f32,
    pub yaw_rate_deg: f32,
    pub v_lat_kmh: f32,
    pub body_slip: Option<f32>,
    pub slip_front: Option<f32>,
    pub slip_rear: Option<f32>,
    /// Smoothed, degrees: > 0 understeer, < 0 oversteer
    pub balance: Option<f32>,
    /// Accelerations as GT7 sends them (lateral, vertical, longitudinal)
    pub steer_ratio: f32,
    pub sway: f32,
    pub heave: f32,
    pub surge: f32,
    pub extended: bool,
    pub recording: bool,

    /// Measurements (0–100, braking distances), page 4
    pub perf: perf::Live,
    /// Connection to the PS5 (search state while there is no data)
    pub link: Link,
    /// Race strategy (page 5), `null` without a strategy
    pub strategy: Option<strategy::Plan>,
    /// Track segments: result in the running lap and in the previous one (0 none, 1 purple,
    /// 2 green, 3 red), and the time to the best per segment (ms)
    pub seg_now: Vec<u8>,
    pub seg_last: Vec<u8>,
    pub seg_delta: Vec<Option<i32>>,
}

pub struct Tracker {
    snap: Snapshot,
    lap: i16,
    lap_start_tick: Option<i32>,
    last_tick: Option<i32>,
    fuel_at_lap_start: Option<f32>,
    fuel_used: Vec<f32>,
    balance: Option<f32>,
    steer_cal: dynamics::SteerCalibration,
    track: Track,
    recorder: Option<Recorder>,
    session_car: Option<i32>,
    session_open: bool,
    paused_since: Option<Instant>,
    off_track_since: Option<Instant>,
    last_packet: Option<Instant>,
    perf: perf::Detector,
    perf_store: Option<perf::Store>,
    /// Power curve from all full-throttle driving (shift points for every gear)
    track_dyno: perf::track::TrackDyno,
    /// Handling counts of the lap being driven (setup advice)
    handling: strategy::handling::Collector,
    /// Active car setup (id), the car and lap it was looked up for
    setup_id: String,
    setup_car: i32,
    setup_check: i16,
    strategy: Option<strategy::Shared>,
    /// Fuel of the previous packet (refuelling detection)
    prev_fuel: Option<f32>,
    /// Fuel burnt in the current lap (refuelling not counted)
    lap_burn: f32,
    lap_refuel: bool,
    /// The current lap was seen from the line (not joined mid-lap): its fuel figure is complete
    lap_full: bool,
    /// Standing still since (pit stop detection)
    still_since: Option<Instant>,
    /// The car has driven in this race (standing on the grid before the start is no pit stop)
    raced: bool,
    /// Mean tyre temperature when the car came to a stand
    still_temp: f32,
    /// After a stop: were the tyres changed? (new tyres come out clearly colder)
    tyre_check: Option<TyreCheck>,
}

struct TyreCheck {
    before: f32,
    moving_since: Option<Instant>,
    coldest: f32,
}

impl Tracker {
    pub fn new(recorder: Option<Recorder>, perf_store: Option<perf::Store>) -> Self {
        Tracker {
            snap: Snapshot::default(),
            lap: 0,
            lap_start_tick: None,
            last_tick: None,
            fuel_at_lap_start: None,
            fuel_used: Vec::new(),
            balance: None,
            steer_cal: Default::default(),
            track: Track::default(),
            recorder,
            session_car: None,
            session_open: false,
            paused_since: None,
            off_track_since: None,
            last_packet: None,
            perf: perf::Detector::default(),
            handling: Default::default(),
            track_dyno: perf::track::TrackDyno::new(perf_store.as_ref().map(|s| s.dir().to_path_buf())),
            setup_id: String::new(),
            setup_car: -1,
            setup_check: -1,
            perf_store,
            strategy: None,
            prev_fuel: None,
            lap_burn: 0.0,
            lap_refuel: false,
            lap_full: false,
            still_since: None,
            raced: false,
            still_temp: 0.0,
            tyre_check: None,
        }
    }

    pub fn set_strategy(&mut self, s: strategy::Shared) {
        self.strategy = Some(s);
    }

    fn with_strategy(&self, f: impl FnOnce(&mut strategy::Strategy)) {
        if let Some(s) = &self.strategy {
            f(&mut s.lock().unwrap_or_else(|e| e.into_inner()));
        }
    }

    pub fn map_json(&self) -> String {
        serde_json::to_string(&self.track.map()).unwrap_or_default()
    }

    pub fn update(&mut self, p: &Packet) -> &Snapshot {
        self.update_at(p, Instant::now())
    }

    fn update_at(&mut self, p: &Packet, now: Instant) -> &Snapshot {
        let on_track = p.flags & flags::ON_TRACK != 0;
        let paused = p.flags & flags::PAUSED != 0;

        self.last_packet = Some(now);
        if paused {
            self.paused_since.get_or_insert(now);
        } else {
            self.paused_since = None;
        }
        if on_track {
            self.off_track_since = None;
        } else {
            self.off_track_since.get_or_insert(now);
        }

        // Different car: end the session right away and drop everything
        let car_changed = self.session_car.is_some_and(|c| c != p.car_code);
        if car_changed {
            if self.session_open {
                self.end_session("car_change");
            }
            self.reset(true);
        } else if p.current_lap < self.lap || (p.current_lap == 0 && self.lap != 0) {
            // Race/time trial restarted: new lap file, map and best lap stay
            self.reset(false);
            self.with_strategy(|s| s.reset_progress());
            if let (Some(r), true) = (&self.recorder, self.session_open) {
                r.send(Msg::Lap(p.current_lap));
            }
        }
        if !self.session_open && on_track && !paused {
            self.start_session(p);
        }

        if p.current_lap != self.lap {
            let crossed_line = self.last_tick.is_some();
            let finished = (self.lap > 0 && p.last_lap_ms > 0).then_some(p.last_lap_ms);
            if let Some(ms) = finished {
                self.snap.laps.push(ms);
                if self.snap.laps.len() > LAP_HISTORY {
                    self.snap.laps.remove(0);
                }
            }
            // Only complete laps without refuelling (a lap joined midway, e.g. after a server
            // restart, or with a pit stop would distort the average; so would the short first race
            // lap on tracks with separate start and finish lines)
            if self.fuel_at_lap_start.is_some() && self.lap_full && !self.lap_refuel && self.lap_burn > 0.0 && self.track.current_closed() {
                self.fuel_used.push(self.lap_burn);
                if self.fuel_used.len() > FUEL_HISTORY {
                    self.fuel_used.remove(0);
                }
            }
            let closed = self.track.current_closed();
            if self.lap > 0 && crossed_line && self.lap_full && closed {
                // Practice: which track (layout) the lap was driven on, before the lap is stored
                let pts = self.track.current_points();
                self.with_strategy(|s| s.lap_shape(&pts));
            }
            let handling = self.handling.take();
            if self.lap > 0 && crossed_line && self.lap_full && closed && !self.lap_refuel {
                let car = p.car_code;
                self.with_strategy(|s| s.handling_lap(car, handling));
            }
            if self.lap > 0 && crossed_line && self.lap_full {
                let (lap, burn, refuel) = (self.lap, self.lap_burn, self.lap_refuel);
                let fuel_start = self.fuel_at_lap_start;
                self.with_strategy(|s| {
                    s.practice.fuel_start = fuel_start;
                    s.lap_finished(lap, burn, refuel, finished, closed)
                });
            }
            self.lap_burn = 0.0;
            self.lap_refuel = false;
            self.lap_full = crossed_line;
            self.track.lap_started(finished, crossed_line && p.current_lap > 0);
            if let (Some(r), true) = (&self.recorder, self.session_open) {
                r.send(Msg::Lap(p.current_lap));
            }
            self.lap = p.current_lap;
            self.lap_start_tick = (p.current_lap > 0).then_some(p.packet_id);
            self.fuel_at_lap_start = (p.current_lap > 0).then_some(p.fuel_level);
        }

        // Fuel: burnt per lap, and refuelling in the pit (fuel goes up)
        let loading = p.flags & flags::LOADING != 0;
        if let (Some(prev), true) = (self.prev_fuel, on_track && !loading && p.current_lap > 0) {
            let diff = p.fuel_level - prev;
            // Refuelling: GT7 adds fuel in steps of ~0.05, part of them just below 0.05, so in the
            // pit (standing) every rise counts; on the move only clear jumps (a reset)
            if diff > 0.05 || (diff > 0.0 && p.speed_ms < 1.0) {
                self.lap_refuel = true;
                let lap = p.current_lap;
                self.with_strategy(|s| s.refuelled(lap, diff));
            } else if diff < 0.0 && !paused {
                self.lap_burn -= diff;
            }
        }
        self.prev_fuel = Some(p.fuel_level);

        // Pit stop without refuelling: standing still in a race lap for a while. A spin stops the
        // car for a second or two, a stop for new tyres takes 15 s or more.
        if p.current_lap <= 0 {
            self.raced = false;
        } else if p.speed_ms > 15.0 {
            self.raced = true;
        }
        let tyre_temp = p.tire_temp.iter().sum::<f32>() / 4.0;
        if self.raced && on_track && !paused && !loading && p.current_lap > 0 && p.speed_ms < 0.2 {
            if self.still_since.is_none() {
                self.still_since = Some(now);
                self.still_temp = tyre_temp;
            }
            if self.still_since.is_some_and(|since| now.duration_since(since) >= PIT_STILL) {
                let lap = p.current_lap;
                self.lap_refuel = true; // this lap's fuel figure is not a racing lap
                self.with_strategy(|s| s.stopped(lap));
                self.tyre_check.get_or_insert(TyreCheck { before: self.still_temp, moving_since: None, coldest: f32::MAX });
            }
        } else {
            self.still_since = None;
        }
        // New tyres: clearly colder than the old ones were when the car stopped (today ~12 °C)
        if let Some(tc) = self.tyre_check.as_mut().filter(|_| p.speed_ms > 5.0 && !paused) {
            let since = *tc.moving_since.get_or_insert(now);
            tc.coldest = tc.coldest.min(tyre_temp);
            if now.duration_since(since) >= TYRE_CHECK {
                let changed = tc.before - tc.coldest >= TYRE_DROP;
                self.tyre_check = None;
                self.with_strategy(|s| s.tyres_detected(changed));
            }
        }

        // Time spent paused must not count towards the lap time
        if let (true, Some(start), Some(last)) = (paused, self.lap_start_tick.as_mut(), self.last_tick) {
            *start += p.packet_id - last;
        }
        self.last_tick = Some(p.packet_id);

        let lap_ms = self.lap_start_tick.map(|t| ((p.packet_id - t) as f64 / TICKS_PER_SEC * 1000.0) as i64);
        let dyn_ = dynamics::compute(p, self.steer_cal.ratio);
        self.steer_cal.update(p, &dyn_);
        self.handling.tick(p, &dyn_);
        let sample = perf::Sample::from_packet(p);
        // The active setup changes rarely: looked up once per lap (and when the car changes)
        if self.setup_car != p.car_code || self.setup_check != self.lap {
            self.setup_car = p.car_code;
            self.setup_check = self.lap;
            let mut id = String::new();
            self.with_strategy(|s| id = s.setups.active_id(p.car_code));
            if id != self.setup_id {
                if let Some(r) = &self.recorder {
                    r.send(Msg::Setup(id.clone()));
                }
            }
            self.setup_id = id;
        }
        self.track_dyno.update(p.car_code, &self.setup_id, &sample, p.angular_velocity[1]);
        if let Some(run) = self.perf.update(sample) {
            self.perf_finished(run, p.car_code);
        }
        let (x, z) = (p.position[0], p.position[2]);
        let mut delta = None;
        if on_track && !paused {
            // The map grows even before the line is crossed the first time (lap 0)
            self.track.add(x, z, lap_ms.unwrap_or(0));
            if let Some(ms) = lap_ms {
                delta = self.track.delta(x, z, ms);
                self.track.segment(x, z, ms);
            }
            if let (Some(r), true) = (&self.recorder, self.session_open) {
                r.send(Msg::Row(Recorder::row(p, &dyn_, lap_ms, delta)));
            }
        }
        self.balance = match (dyn_.balance, self.balance) {
            (Some(b), Some(prev)) => Some(prev + (b - prev) / BALANCE_SMOOTHING),
            (b, _) => b,
        };

        let speed = p.speed_ms;
        let mut slip = [None; 4];
        let mut wheel_speed = [0.0; 4];
        for i in 0..4 {
            let v = p.wheel_rps[i].abs() * p.tire_radius[i];
            wheel_speed[i] = v * 3.6;
            if speed > 2.0 {
                slip[i] = Some(v / speed);
            }
        }

        let fuel_per_lap = (!self.fuel_used.is_empty()).then(|| self.fuel_used.iter().sum::<f32>() / self.fuel_used.len() as f32);

        let s = &mut self.snap;
        s.live = true;
        s.on_track = on_track;
        s.paused = paused;
        s.loading = p.flags & flags::LOADING != 0;
        s.speed_kmh = speed * 3.6;
        s.rpm = p.engine_rpm;
        s.rpm_alert_min = p.rpm_alert_min;
        s.rpm_alert_max = p.rpm_alert_max;
        s.gear = p.gear;
        s.suggested_gear = (p.suggested_gear != 15).then_some(p.suggested_gear);
        s.throttle = p.throttle as f32 / 2.55;
        s.brake = p.brake as f32 / 2.55;
        s.tire_temp = p.tire_temp;
        s.tire_slip = slip;
        s.wheel_speed_kmh = wheel_speed;
        s.tire_radius = p.tire_radius;
        s.suspension = p.suspension_height;
        s.fuel = p.fuel_level;
        s.fuel_capacity = p.fuel_capacity;
        s.fuel_per_lap = fuel_per_lap;
        s.fuel_laps_left = fuel_per_lap.filter(|f| *f > 0.0).map(|f| p.fuel_level / f);
        s.lap = p.current_lap;
        s.total_laps = p.total_laps;
        s.current_lap_ms = lap_ms;
        s.last_lap_ms = (p.last_lap_ms > 0).then_some(p.last_lap_ms);
        s.best_lap_ms = (p.best_lap_ms > 0).then_some(p.best_lap_ms);
        s.live_delta_ms = delta;
        s.position = (p.start_position > 0).then_some(p.start_position);
        s.cars = (p.cars_in_race > 0).then_some(p.cars_in_race);
        s.water_temp = p.water_temp;
        s.oil_temp = p.oil_temp;
        s.oil_pressure = p.oil_pressure;
        s.boost_bar = (p.flags & flags::HAS_TURBO != 0).then_some(p.boost_bar);
        s.tcs = p.flags & flags::TCS_ACTIVE != 0;
        s.asm = p.flags & flags::ASM_ACTIVE != 0;
        s.rev_limiter = p.flags & flags::REV_LIMITER != 0;
        s.handbrake = p.flags & flags::HANDBRAKE != 0;
        s.in_gear = p.flags & flags::IN_GEAR != 0;
        s.lights = p.flags & flags::LIGHTS != 0;
        s.high_beam = p.flags & flags::HIGH_BEAM != 0;
        s.flags = p.flags;

        s.car_code = p.car_code;
        s.car_name = cars::name(p.car_code);
        s.calc_max_speed = p.calc_max_speed;
        s.transmission_top_speed_kmh = p.transmission_top_speed * 3.6;
        s.gear_ratios = p.gear_ratios.iter().copied().filter(|r| *r > 0.0).collect();
        s.clutch_pedal = p.clutch_pedal;
        s.clutch_engagement = p.clutch_engagement;
        s.rpm_clutch_gearbox = p.rpm_clutch_gearbox;
        s.energy_recovery = p.energy_recovery;
        s.time_of_day_ms = p.time_of_day_ms;

        s.pos = p.position;
        s.heading = dyn_.heading;
        s.pitch = dyn_.pitch;
        s.roll = dyn_.roll;
        s.body_height = p.body_height;
        s.road_plane_distance = p.road_plane_distance;
        s.map_version = self.track.map_version;
        s.seg_now.clone_from(&self.track.seg_now);
        s.seg_last.clone_from(&self.track.seg_last);
        s.seg_delta.clone_from(&self.track.seg_delta);

        s.steering_deg = p.steering.to_degrees();
        s.yaw_rate_deg = dyn_.yaw_rate.to_degrees();
        s.v_lat_kmh = dyn_.v_lat * 3.6;
        s.body_slip = dyn_.body_slip;
        s.slip_front = dyn_.slip_front;
        s.slip_rear = dyn_.slip_rear;
        s.balance = self.balance;
        s.steer_ratio = self.steer_cal.ratio;
        s.sway = p.sway;
        s.heave = p.heave;
        s.surge = p.surge;
        s.extended = p.extended;
        s.recording = self.recorder.is_some() && self.session_open;
        s.perf.clone_from(&self.perf.live);
        if let Some(st) = &self.strategy {
            // Progress in the lap from the typical lap time (median of recent laps, else best)
            let mut laps: Vec<i32> = s.laps.iter().copied().filter(|v| *v > 0).collect();
            laps.sort_unstable();
            let reference = laps.get(laps.len() / 2).copied().or(s.best_lap_ms).filter(|v| *v > 0);
            let frac = match (lap_ms, reference) {
                (Some(ms), Some(r)) => (ms as f32 / r as f32).min(0.99),
                _ => 0.0,
            };
            let live = strategy::Live {
                lap: p.current_lap,
                total_laps: p.total_laps,
                frac,
                fuel: p.fuel_level,
                capacity: p.fuel_capacity,
                car_code: p.car_code,
                standing: p.speed_ms < 0.3,
                lap_start_fuel: self.fuel_at_lap_start,
            };
            let mut st = st.lock().unwrap_or_else(|e| e.into_inner());
            st.update_live(live);
            s.strategy = Some(st.plan());
        }
        s.perf.saved = self.perf_store.as_ref().map_or(0, perf::Store::saved);
        s
    }

    fn perf_finished(&mut self, mut run: perf::Run, car_code: i32) {
        let car = cars::name(car_code).map_or_else(|| format!("car{car_code}"), str::to_string);
        let start = chrono::Local::now() - chrono::TimeDelta::milliseconds((run.duration_s * 1000.0) as i64);
        run.stamp(car_code, &car, start);
        let h = perf::cli::headline(&run);
        println!("{}", tr!("Messung: {h}", "Measurement: {h}", "Měření: {h}"));
        if let Some(store) = &self.perf_store {
            store.save_in_background(run);
        }
    }

    pub fn set_link(&mut self, link: Link) {
        self.snap.link = link;
    }

    /// No packets any more (PS5 off, menu, network gone).
    pub fn mark_offline(&mut self) -> &Snapshot {
        self.snap.live = false;
        &self.snap
    }

    /// Called every second: ends the session after a long pause, when the track was left or when
    /// no data arrives any more.
    pub fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    fn tick_at(&mut self, now: Instant) {
        if !self.session_open {
            return;
        }
        let since = |t: Option<Instant>, limit: Duration| t.is_some_and(|t| now.duration_since(t) >= limit);
        let reason = if self.last_packet.is_none_or(|t| now.duration_since(t) >= SESSION_OFF_END) {
            Some("no_data")
        } else if since(self.paused_since, SESSION_PAUSE_END) {
            Some("pause")
        } else if since(self.off_track_since, SESSION_OFF_END) {
            Some("off_track")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.end_session(reason);
        }
    }

    /// New session, or the last one continued (same car, position on the known track).
    fn start_session(&mut self, p: &Packet) {
        let resume = self.session_car == Some(p.car_code) && self.track.is_near(p.position[0], p.position[2], RESUME_DISTANCE);
        if !resume {
            self.reset(true);
        }
        self.session_car = Some(p.car_code);
        self.session_open = true;
        if let Some(r) = &self.recorder {
            let car = cars::name(p.car_code).map_or_else(|| format!("car{}", p.car_code), str::to_string);
            r.send(Msg::Session { car, car_code: p.car_code, resume });
            // On a lap change in the same packet, the lap change block creates the file
            if p.current_lap == self.lap {
                r.send(Msg::Lap(p.current_lap));
            }
        }
        let car = cars::name(p.car_code).unwrap_or("?");
        println!(
            "{}",
            if resume {
                tr!("Session fortgesetzt ({car})", "Session continued ({car})", "Session pokračuje ({car})")
            } else {
                tr!("Session gestartet ({car})", "Session started ({car})", "Session začala ({car})")
            }
        );
    }

    fn end_session(&mut self, reason: &'static str) {
        self.session_open = false;
        if let Some(r) = &self.recorder {
            r.send(Msg::End { reason });
        }
        let why = match reason {
            "pause" => tr!("Pause oder Menü", "pause or menu", "pauza nebo menu"),
            "off_track" => tr!("Strecke verlassen", "left the track", "opuštění trati"),
            "no_data" => tr!("keine Daten mehr", "no more data", "žádná další data"),
            "car_change" => tr!("Autowechsel", "car changed", "změna auta"),
            other => other.to_string(),
        };
        println!("{}", tr!("Session beendet: {why}", "Session ended: {why}", "Session skončila: {why}"));
    }

    /// Restart: `full = false` (race/time trial restarted) keeps map, best lap and steering ratio;
    /// `full = true` (new car / new track) drops everything.
    fn reset(&mut self, full: bool) {
        self.track_dyno.save();
        let mut track = std::mem::take(&mut self.track);
        if full {
            track.reset();
        } else {
            track.lap_started(None, false);
        }
        let session_car = if full { None } else { self.session_car };
        let steer_cal = if full { Default::default() } else { std::mem::take(&mut self.steer_cal) };
        let snap = Snapshot { link: std::mem::take(&mut self.snap.link), ..Default::default() };
        *self = Tracker {
            snap,
            track,
            session_car,
            steer_cal,
            session_open: self.session_open,
            paused_since: self.paused_since,
            off_track_since: self.off_track_since,
            last_packet: self.last_packet,
            strategy: self.strategy.take(),
            ..Tracker::new(self.recorder.take(), self.perf_store.take())
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn packet(car: i32, flags: u16, x: f32) -> Packet {
        Packet { car_code: car, flags, current_lap: 1, position: [x, 0.0, 0.0], speed_ms: 30.0, packet_id: 1, ..Default::default() }
    }

    fn sessions(root: &Path) -> Vec<(PathBuf, serde_json::Value)> {
        std::thread::sleep(Duration::from_millis(300)); // the recorder writes asynchronously
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root).unwrap().flatten().map(|e| e.path()).collect();
        dirs.sort();
        dirs.into_iter()
            .map(|d| {
                let meta = serde_json::from_str(&std::fs::read_to_string(d.join("session.json")).unwrap()).unwrap();
                (d, meta)
            })
            .collect()
    }

    /// Drives `secs` seconds of packets (60/s) with the given flags.
    fn drive(t: &mut Tracker, car: i32, flags: u16, start: Instant, secs: u64, x: &mut f32, id: &mut i32) -> Instant {
        let mut now = start;
        for _ in 0..secs * 60 {
            *id += 1;
            if flags & flags::PAUSED == 0 {
                *x += 0.5; // moving, so the map grows
            }
            let mut p = packet(car, flags, *x);
            p.packet_id = *id;
            t.update_at(&p, now);
            now += Duration::from_micros(16_667);
            if *id % 60 == 0 {
                t.tick_at(now);
            }
        }
        now
    }

    #[test]
    fn strategy_follows_a_race() {
        let st = strategy::Strategy::load(None).shared();
        st.lock().unwrap().set_race(strategy::Race {
            name: "Test".into(),
            track: "Barcelona".into(),
            laps: Some(23),
            refuel_rate: Some(3.0),
            pit_required: true,
            ..Default::default()
        });
        let mut t = Tracker::new(None, None);
        t.set_strategy(st.clone());
        let (mut fuel, mut id) = (100.0f32, 0i32);
        let mut now = Instant::now();
        // 13 laps of 120 s, 7.16 fuel per lap; in lap 12 the car refuels by 60
        for lap in 1..=13i16 {
            for k in 0..120 {
                id += 60; // one second of ticks
                if lap == 12 && (40..60).contains(&k) {
                    fuel += 3.0; // standing in the pit: no fuel burnt
                } else {
                    fuel -= 7.16 / 120.0;
                }
                let p = Packet {
                    car_code: 1,
                    flags: flags::ON_TRACK,
                    current_lap: lap,
                    total_laps: 23,
                    fuel_level: fuel,
                    fuel_capacity: 100.0,
                    speed_ms: 50.0,
                    packet_id: id,
                    last_lap_ms: if lap > 1 { 120_000 } else { 0 },
                    position: [k as f32, 0.0, lap as f32],
                    ..Default::default()
                };
                now += Duration::from_secs(1);
                t.update_at(&p, now);
            }
        }
        let s = st.lock().unwrap();
        let fpl = s.fuel_per_lap().unwrap();
        assert!((fpl - 7.16).abs() < 0.01, "fuel per lap {fpl}");
        assert_eq!(s.saved.stops.len(), 1);
        assert_eq!(s.saved.stops[0].lap, 12);
        assert!((s.saved.stops[0].fuel_added - 60.0).abs() < 0.01);
        assert!(s.saved.fuel_laps.iter().any(|l| l.lap == 12 && l.refuel));
        let plan = t.snap.strategy.clone().unwrap();
        assert!(plan.pit_done && plan.tyre_question);
        // 12.99 laps driven, fuel 100 − 13 × 7.16 + 60 ≈ 66.9: 10 laps left need ~74 → one more stop
        assert_eq!(plan.stops_needed, 1);
        assert_eq!(plan.total_laps, Some(23));
    }

    #[test]
    fn tyre_only_stop_is_detected_but_not_a_spin() {
        let st = strategy::Strategy::load(None).shared();
        st.lock().unwrap().set_race(strategy::Race { name: "Test".into(), laps: Some(10), pit_required: true, ..Default::default() });
        let mut t = Tracker::new(None, None);
        t.set_strategy(st.clone());
        let mut now = Instant::now();
        let mut id = 0;
        let mut step = |t: &mut Tracker, lap: i16, speed: f32, secs: u32| {
            for _ in 0..secs * 60 {
                id += 1;
                now += Duration::from_micros(16_667);
                let p = Packet {
                    car_code: 1,
                    flags: flags::ON_TRACK,
                    current_lap: lap,
                    total_laps: 10,
                    fuel_level: 70.0,
                    fuel_capacity: 100.0,
                    speed_ms: speed,
                    packet_id: id,
                    ..Default::default()
                };
                t.update_at(&p, now);
            }
        };
        step(&mut t, 1, 0.0, 10); // standing start: lap 1 on the grid before the lights
        step(&mut t, 4, 50.0, 3);
        step(&mut t, 4, 0.0, 2); // spin: 2 s standing
        step(&mut t, 4, 50.0, 3);
        assert!(st.lock().unwrap().saved.stops.is_empty());
        step(&mut t, 5, 50.0, 3);
        step(&mut t, 5, 0.0, 18); // tyre change
        step(&mut t, 5, 20.0, 3);
        let mut s = st.lock().unwrap();
        assert_eq!(s.saved.stops.len(), 1);
        assert_eq!(s.saved.stops[0].lap, 5);
        assert!(s.plan().pit_done && s.plan().tyre_question);
    }

    #[test]
    fn tyre_change_is_seen_in_the_temperatures() {
        for (after, changed) in [(62.0f32, true), (78.0, false)] {
            let st = strategy::Strategy::load(None).shared();
            st.lock().unwrap().set_race(strategy::Race {
                name: "Test".into(),
                laps: Some(11),
                start_compound: Some("RS".into()),
                ..Default::default()
            });
            let mut t = Tracker::new(None, None);
            t.set_strategy(st.clone());
            let (mut now, mut id) = (Instant::now(), 0);
            let mut step = |t: &mut Tracker, lap: i16, speed: f32, temp: f32, secs: u32| {
                for _ in 0..secs * 60 {
                    id += 1;
                    now += Duration::from_micros(16_667);
                    let p = Packet {
                        car_code: 1,
                        flags: flags::ON_TRACK,
                        current_lap: lap,
                        total_laps: 11,
                        fuel_level: 60.0,
                        fuel_capacity: 100.0,
                        speed_ms: speed,
                        packet_id: id,
                        tire_temp: [temp; 4],
                        ..Default::default()
                    };
                    t.update_at(&p, now);
                }
            };
            step(&mut t, 7, 50.0, 81.0, 5);
            step(&mut t, 7, 0.0, 80.0, 18); // in the box
            step(&mut t, 8, 20.0, after, 35); // pit exit and the first part of the lap
            let mut s = st.lock().unwrap();
            assert_eq!(s.saved.stops[0].tyres, Some(changed), "after {after} °C");
            assert_eq!(s.saved.stints.len(), if changed { 2 } else { 0 });
            assert!(!s.plan().tyre_question);
        }
    }

    #[test]
    fn session_lifecycle() {
        let root = std::env::temp_dir().join(format!("apexline-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut t = Tracker::new(Some(Recorder::start(root.clone(), None)), None);
        let (mut x, mut id) = (0.0, 0);
        let on = flags::ON_TRACK;

        // Driving starts a session
        let now = drive(&mut t, 3475, on, Instant::now(), 5, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 1);
        assert!(s[0].1["ended"].is_null());

        // A short pause ends nothing, 3 min pause/menu does
        let now = drive(&mut t, 3475, on | flags::PAUSED, now, 60, &mut x, &mut id);
        assert!(sessions(&root)[0].1["ended"].is_null());
        let now = drive(&mut t, 3475, on | flags::PAUSED, now, 125, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s[0].1["end_reason"], "pause");

        // Driving on with the same car on the same track continues
        let now = drive(&mut t, 3475, on, now, 2, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 1);
        assert!(s[0].1["ended"].is_null());
        assert_eq!(s[0].1["resumed"], 1);

        // Changing the car ends right away and starts a new session
        std::thread::sleep(Duration::from_millis(1100)); // new folder name (seconds)
        let now = drive(&mut t, 3350, on, now, 2, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].1["end_reason"], "car_change");
        assert!(s[1].1["ended"].is_null());

        // Same car but a different track (far away) after the menu: new session
        let now = drive(&mut t, 3350, 0, now, 61, &mut x, &mut id);
        assert_eq!(sessions(&root)[1].1["end_reason"], "off_track");
        std::thread::sleep(Duration::from_millis(1100));
        x += 5000.0;
        let _ = drive(&mut t, 3350, on, now, 2, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 3);

        // Server restart (without an end) with the same car continues the open session
        drop(t);
        let mut t2 = Tracker::new(Some(Recorder::start(root.clone(), None)), None);
        let _ = drive(&mut t2, 3350, on, Instant::now(), 2, &mut x, &mut id);
        let s = sessions(&root);
        assert_eq!(s.len(), 3);
        assert_eq!(s[2].1["resumed"], 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
