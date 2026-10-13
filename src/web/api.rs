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
        .route("/api/perf", get(perf_overview))
        .route("/api/settings", get(settings).post(update_settings))
        .route("/api/strategy", get(strategy_state))
        .route("/api/strategy/race", axum::routing::post(strategy_race))
        .route("/api/strategy/preview", axum::routing::post(strategy_preview))
        .route("/api/strategy/action", axum::routing::post(strategy_action))
        .route("/api/practice", get(practice_state))
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
    let summary = car.map(|c| st.practice.summary(c, &track, p.wear_mult, p.fuel_mult));
    let laps: Vec<_> = p.laps.iter().enumerate().rev().take(12).map(|(i, l)| json!({ "index": i, "lap": l })).collect();
    let mut tracks: Vec<String> = p.shapes.iter().map(|s| s.track.clone()).chain(p.laps.iter().map(|l| l.track.clone())).collect();
    tracks.sort();
    tracks.dedup();
    json!({ "state": st.practice_state(), "car": car, "summary": summary, "laps": laps, "tracks": tracks }).to_string()
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

#[derive(Deserialize)]
struct PerfQuery {
    car: Option<i32>,
}

async fn perf_overview(State(s): State<AppState>, Query(q): Query<PerfQuery>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || s.perf.overview(q.car)).await.unwrap_or_default();
    json_body(body)
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
