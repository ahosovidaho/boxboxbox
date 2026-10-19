//! HTTP API of the dashboard.
//!
//! | Method | Path | Purpose |
//! |---|---|---|
//! | GET | `/api/coach` | coach feed (page 3) |
//! | GET | `/api/map` | track map |
//! | GET | `/api/perf?car=<code>` | measurements of a car (page 4) |
//! | GET/POST | `/api/settings` | server settings (`config.toml`) |
//! | GET | `/api/strategy` | race, stints, stops and calibration |
//! | POST | `/api/strategy/race` | set the race (JSON of [`strategy::Race`]) |
//! | POST | `/api/strategy/preview` | recommended + alternative strategy for race settings |
//! | POST | `/api/strategy/action` | `{"action": "compound"/"new_tyres"/"same_tyres"/"no_stop"/"wear"/"reset"/"clear", …}` |
//! | POST | `/api/strategy/action` | practice: `"practice"` (value "1"/"0"), `"practice_track"`, `"practice_compound"`, `"practice_new_set"`, `"practice_counting"`, `"practice_mults"`, `"practice_life"`, `"practice_toggle_lap"` |
//! | GET  | `/api/practice?track=&car=` | practice state, figures per compound, last laps, known tracks |
//! | GET/POST | `/api/setups?car=` | car setups (GT7 settings sheet) and the active one; POST saves a card |
//! | GET  | `/api/prep` | preparation checklist for the chosen race |
//! | GET  | `/api/traces?session=&lap=` | speed, pedals, steering and delta of a lap against the best lap of the session (corner graphs) |
//! | POST | `/api/setups/action` | `{"action": "activate"/"remove", "car", "id"}` |

use super::Feeds;
use crate::config::{Config, DataPaths, Language, Units};
use crate::perf;
use crate::strategy;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone)]
pub struct AppState {
    pub feeds: Feeds,
    /// Updates per second on the WebSocket
    pub hz: u32,
    /// Serve the dashboard from this folder instead of the built-in copy
    pub web_dir: Option<PathBuf>,
    pub perf: perf::Store,
    pub strategy: strategy::Shared,
    pub settings: Arc<Settings>,
}

/// Server settings that can be changed on the dashboard.
pub struct Settings {
    pub config: Mutex<Config>,
    pub config_file: PathBuf,
    pub data: DataPaths,
    /// PS5 address for the telemetry loop (`None` = search)
    pub ps5: watch::Sender<Option<Ipv4Addr>>,
    /// Port this server runs on (a changed port applies after a restart)
    pub port: u16,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/coach", get(coach))
        .route("/api/map", get(map))
        .route("/api/traces", get(traces))
        .route("/api/prep", get(prep))
        .route("/api/perf", get(perf_overview))
        .route("/api/settings", get(settings).post(update_settings))
        .route("/api/strategy", get(strategy_state))
        .route("/api/strategy/race", axum::routing::post(strategy_race))
        .route("/api/strategy/preview", axum::routing::post(strategy_preview))
        .route("/api/strategy/action", axum::routing::post(strategy_action))
        .route("/api/practice", get(practice_state))
        .route("/api/setups", get(setups_list).post(setups_save))
        .route("/api/setups/action", axum::routing::post(setups_action))
}

async fn strategy_state(State(s): State<AppState>) -> impl IntoResponse {
    json_body(s.strategy.lock().unwrap_or_else(|e| e.into_inner()).json())
}

async fn strategy_race(State(s): State<AppState>, Json(race): Json<strategy::Race>) -> impl IntoResponse {
    let mut st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
    st.set_race(race);
    json_body(st.json())
}

/// Recommended and alternative strategy for race settings that are not saved yet.
async fn strategy_preview(State(s): State<AppState>, Json(race): Json<strategy::Race>) -> impl IntoResponse {
    let st = s.strategy.clone();
    let body = tokio::task::spawn_blocking(move || {
        let mut st = st.lock().unwrap_or_else(|e| e.into_inner());
        let (options, plan) = st.preview(&race);
        json!({ "options": options, "fuel_per_lap": plan.fuel_per_lap, "fuel_source": plan.fuel_source,
                "pit_loss": plan.pit_loss, "pit_loss_source": plan.pit_loss_source, "total_laps": plan.total_laps,
                "practice_use": plan.practice_use })
        .to_string()
    })
    .await
    .unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct StrategyAction {
    action: String,
    compound: Option<String>,
    pct: Option<f32>,
    /// Practice: track name, "1"/"0"
    value: Option<String>,
    wear: Option<f32>,
    fuel: Option<f32>,
    life: Option<u32>,
    index: Option<usize>,
}

#[derive(Deserialize)]
struct PracticeQuery {
    track: Option<String>,
    car: Option<i32>,
}

/// Practice: state, figures per compound for the car and track, the last laps, known tracks.
async fn practice_state(State(s): State<AppState>, Query(q): Query<PracticeQuery>) -> impl IntoResponse {
    let st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
    json_body(practice_json(&st, q.track, q.car))
}

fn practice_json(st: &strategy::Strategy, track: Option<String>, car: Option<i32>) -> String {
    let p = &st.practice.saved;
    let track = track.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| p.track.clone());
    let car = car.or_else(|| st.practice_car(&track));
    let setup = car.map(|c| st.setups.active_id(c)).unwrap_or_default();
    let summary = car.map(|c| st.practice.summary(c, &setup, &track, p.wear_mult, p.fuel_mult));
    let setup_name = car.and_then(|c| st.setups.name_of(c, &setup));
    // The last laps of this car on this track
    let key = strategy::practice::track_key(&track);
    let laps: Vec<_> = p
        .laps
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, l)| Some(l.car) == car && strategy::practice::track_key(&l.track) == key)
        .take(12)
        .map(|(i, l)| json!({ "index": i, "lap": l }))
        .collect();
    let mut tracks: Vec<String> = p.shapes.iter().map(|s| s.track.clone()).chain(p.laps.iter().map(|l| l.track.clone())).collect();
    tracks.sort();
    tracks.dedup();
    json!({ "state": st.practice_state(), "car": car, "summary": summary, "laps": laps, "tracks": tracks, "setup": setup_name }).to_string()
}

async fn strategy_action(State(s): State<AppState>, Json(a): Json<StrategyAction>) -> impl IntoResponse {
    let mut st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
    let compound = a.compound.as_deref().map(str::trim).filter(|c| !c.is_empty());
    let ok = match (a.action.as_str(), compound, a.pct) {
        ("compound", Some(c), _) => {
            st.set_compound(c);
            true
        }
        ("new_tyres", Some(c), _) => {
            st.new_tyres(c);
            true
        }
        ("same_tyres", ..) => {
            st.same_tyres();
            true
        }
        ("no_stop", ..) => {
            st.no_stop();
            true
        }
        ("wear", _, Some(p)) => {
            st.add_wear(p);
            true
        }
        ("reset", ..) => {
            st.reset_progress();
            st.save();
            true
        }
        ("practice", ..) => {
            st.set_practice(a.value.as_deref() == Some("1"));
            true
        }
        ("practice_track", ..) => {
            st.practice.saved.track = a.value.clone().unwrap_or_default().trim().to_string();
            st.practice.layout_warning = None;
            st.practice.save();
            true
        }
        ("practice_compound", Some(c), _) => {
            // Other tyres marked = a new set; the same compound again just confirms it
            let c = c.to_uppercase();
            if st.practice.saved.compound.as_deref() != Some(c.as_str()) {
                st.practice.new_set(Some(&c));
            }
            true
        }
        ("practice_new_set", c, _) => {
            st.practice.new_set(c);
            true
        }
        ("practice_counting", ..) => {
            st.practice.saved.counting = a.value.as_deref() != Some("0");
            st.practice.save();
            true
        }
        ("practice_mults", ..) => {
            let p = &mut st.practice.saved;
            p.wear_mult = a.wear.filter(|v| *v > 0.0).unwrap_or(p.wear_mult);
            p.fuel_mult = a.fuel.filter(|v| *v > 0.0).unwrap_or(p.fuel_mult);
            st.practice.save();
            true
        }
        ("practice_life", Some(c), _) => {
            let track = st.practice.saved.track.clone();
            let wear = st.practice.saved.wear_mult;
            if let Some(car) = st.practice_car(&track) {
                st.practice.set_life(car, &track, c, wear, a.life);
            }
            true
        }
        ("practice_toggle_lap", ..) => {
            if let Some(i) = a.index {
                st.practice.toggle_lap(i);
            }
            true
        }
        ("clear", ..) => {
            st.saved.race = None;
            st.reset_progress();
            st.save();
            true
        }
        _ => false,
    };
    if ok && a.action.starts_with("practice") {
        (StatusCode::OK, json_body(practice_json(&st, None, None))).into_response()
    } else if ok {
        (StatusCode::OK, json_body(st.json())).into_response()
    } else {
        (StatusCode::BAD_REQUEST, json_body(json!({ "error": "unknown action or missing value" }).to_string())).into_response()
    }
}

fn json_body(body: String) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], body)
}

/// The coach feed is read fresh from its file on every request.
async fn coach(State(s): State<AppState>) -> impl IntoResponse {
    let path = s.settings.data.coach_feed.clone();
    json_body(std::fs::read_to_string(path).unwrap_or_else(|_| "{\"items\":[]}".into()))
}

async fn map(State(s): State<AppState>) -> impl IntoResponse {
    json_body(s.feeds.map.borrow().to_string())
}

/// What is known for the chosen race and what practice still has to deliver.
async fn prep(State(s): State<AppState>) -> impl IntoResponse {
    let perf = s.perf.clone();
    let st = s.strategy.clone();
    let body = tokio::task::spawn_blocking(move || {
        let st = st.lock().unwrap_or_else(|e| e.into_inner());
        st.prep(|car, setup| crate::perf::track::read(perf.dir(), car, setup).is_some())
            .map_or_else(|| json!({ "items": [] }).to_string(), |v| v.to_string())
    })
    .await
    .unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct TraceQuery {
    session: Option<String>,
    lap: Option<String>,
}

/// Lap comparison traces. Session: the given one (a folder name in the recordings), otherwise the
/// one the coach shows, otherwise the newest.
async fn traces(State(s): State<AppState>, Query(q): Query<TraceQuery>) -> impl IntoResponse {
    static COMPARER: std::sync::OnceLock<Mutex<crate::coach::compare::Comparer>> = std::sync::OnceLock::new();
    let rec = s.settings.data.recordings.clone();
    let feed = s.settings.data.coach_feed.clone();
    let perf_dir = s.perf.dir().to_path_buf();
    let body = tokio::task::spawn_blocking(move || {
        let safe = |n: &str| !n.is_empty() && !n.contains(['/', '\\']) && n != ".." && rec.join(n).is_dir();
        let from_feed = || {
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&feed).ok()?).ok()?;
            v["session"].as_str().map(str::to_string)
        };
        let newest = || {
            let mut d: Vec<String> = std::fs::read_dir(&rec)
                .ok()?
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            d.sort();
            d.pop()
        };
        let session = q.session.filter(|n| safe(n)).or_else(|| from_feed().filter(|n| safe(n))).or_else(newest);
        let Some(session) = session else { return json!({ "laps": [] }).to_string() };
        let mut c = COMPARER.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        // Shift points from driving for the car and the setup the session was driven with
        let meta: serde_json::Value = std::fs::read_to_string(rec.join(&session).join("session.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let car = meta["car_code"].as_i64().unwrap_or(0) as i32;
        let setup = meta["lap_setups"].as_object().and_then(|m| m.values().last()).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let drive = crate::perf::track::read(&perf_dir, car, &setup).map(|t| crate::coach::shifts::Optimum {
            gears: t.dyno.shifts.iter().map(|s| (s.gear, s.rpm as f64, s.late)).collect(),
            max_rpm: t.dyno.max_rpm as f64,
            source: "drive",
        });
        match c.traces(&rec.join(&session), q.lap.as_deref(), drive) {
            Ok(v) => v.to_string(),
            Err(e) => json!({ "laps": [], "error": e.to_string() }).to_string(),
        }
    })
    .await
    .unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct PerfQuery {
    car: Option<i32>,
}

async fn perf_overview(State(s): State<AppState>, Query(q): Query<PerfQuery>) -> impl IntoResponse {
    let st = s.strategy.clone();
    let body = tokio::task::spawn_blocking(move || {
        // Shift points from driving belong to the setup that is active for the car
        s.perf.overview(q.car, |car| st.lock().unwrap_or_else(|e| e.into_inner()).setups.active_id(car))
    })
    .await
    .unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct SetupQuery {
    car: i32,
}

/// The setups of a car and which one is active.
async fn setups_list(State(s): State<AppState>, Query(q): Query<SetupQuery>) -> impl IntoResponse {
    let st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
    json_body(setups_json(&st, q.car))
}

fn setups_json(st: &strategy::Strategy, car: i32) -> String {
    let active = st.setups.active_id(car);
    // The current track, or where the car was last driven with this setup
    let mut track = st.advice_track();
    if st.handling.last(car, &active, &track).is_empty() {
        if let Some(t) = st.handling.last_track(car, &active) {
            track = t;
        }
    }
    let setup = st.setups.active(car);
    let laps = st.handling.last(car, &active, &track);
    let kart = crate::telemetry::cars::name(car).is_some_and(|n| n.contains("Kart"));
    let report = strategy::handling::advice(&laps, setup, setup.is_some_and(|s| s.bop), !kart);
    json!({
        "car": car,
        "active": active,
        "setups": st.setups.for_car(car),
        "track": track,
        "advice": report,
        "compare": setup_compare(st, car, &track),
    })
    .to_string()
}

/// Clean practice laps per setup on the track: raw best and median, and the fair lap time on
/// new tyres with the same fuel (see [`strategy::setup_compare`]).
fn setup_compare(st: &strategy::Strategy, car: i32, track: &str) -> serde_json::Value {
    let key = strategy::practice::track_key(track);
    let laps: Vec<&strategy::practice::PracticeLap> = st
        .practice
        .saved
        .laps
        .iter()
        .filter(|l| l.car == car && l.clean && l.counted && !l.wrong_layout && l.ms > 0 && strategy::practice::track_key(&l.track) == key)
        .collect();
    let fair = strategy::setup_compare::fair(&laps);
    let name = |id: &str| {
        if id.is_empty() {
            "bez setupu".to_string()
        } else {
            st.setups.name_of(car, id).unwrap_or_else(|| format!("smazaný ({id})"))
        }
    };
    let rows: Vec<serde_json::Value> = fair
        .rows
        .iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or_default();
            v["id"] = r.setup.clone().into();
            v["name"] = name(&r.setup).into();
            v
        })
        .collect();
    json!({ "rows": rows, "wear": fair.wear, "fuel_effect": fair.fuel_effect, "reference_fuel": fair.reference_fuel,
            "wear_from_data": fair.wear_from_data, "fuel_from_data": fair.fuel_from_data })
}

/// Saves a setup card (new without `id`); a new card becomes active.
async fn setups_save(State(s): State<AppState>, Json(setup): Json<strategy::setups::Setup>) -> impl IntoResponse {
    let mut st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
    let car = setup.car;
    st.setups.upsert(setup);
    json_body(setups_json(&st, car))
}

#[derive(Deserialize)]
struct SetupAction {
    action: String,
    car: i32,
    #[serde(default)]
    id: String,
}

async fn setups_action(State(s): State<AppState>, Json(a): Json<SetupAction>) -> impl IntoResponse {
    let mut st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
    match a.action.as_str() {
        "activate" => st.setups.set_active(a.car, &a.id),
        "remove" => st.setups.remove(a.car, &a.id),
        _ => return (StatusCode::BAD_REQUEST, json_body(json!({ "error": "unknown action" }).to_string())).into_response(),
    }
    (StatusCode::OK, json_body(setups_json(&st, a.car))).into_response()
}

fn settings_json(s: &Settings) -> String {
    let c = s.config.lock().unwrap_or_else(|e| e.into_inner()).clone();
    json!({
        "ps5_ip": c.ps5_ip,
        "http_port": c.http_port,
        "language": c.language,
        "units": c.units,
        "port_in_use": s.port,
        "config_file": s.config_file.display().to_string(),
        "data_dir": s.data.root.display().to_string(),
    })
    .to_string()
}

async fn settings(State(s): State<AppState>) -> impl IntoResponse {
    json_body(settings_json(&s.settings))
}

/// Fields that may be changed from the dashboard; missing fields stay as they are. The data
/// folder is only set in the file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsUpdate {
    ps5_ip: Option<String>,
    http_port: Option<u16>,
    language: Option<Language>,
    units: Option<Units>,
}

async fn update_settings(State(s): State<AppState>, Json(u): Json<SettingsUpdate>) -> impl IntoResponse {
    let st = s.settings.clone();
    let res = tokio::task::spawn_blocking(move || {
        let mut cfg = st.config.lock().unwrap_or_else(|e| e.into_inner());
        let mut new = cfg.clone();
        if let Some(ip) = u.ps5_ip {
            new.ps5_ip = ip.trim().to_string();
        }
        new.http_port = u.http_port.unwrap_or(new.http_port);
        new.language = u.language.unwrap_or(new.language);
        new.units = u.units.unwrap_or(new.units);
        new.validate()?;
        if new != *cfg {
            new.save(&st.config_file).map_err(|e| format!("{}: {e}", st.config_file.display()))?;
            if new.ps5() != cfg.ps5() {
                st.ps5.send_replace(new.ps5());
            }
            // Coach texts and the log follow the new language right away
            crate::i18n::set(new.language, new.units);
            *cfg = new;
        }
        drop(cfg);
        Ok::<_, String>(settings_json(&st))
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(body) => (StatusCode::OK, json_body(body)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, json_body(json!({ "error": e }).to_string())).into_response(),
    }
}
