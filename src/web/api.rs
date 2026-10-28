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
//! | GET  | `/api/fuelsave?save=` | where to lift and coast to save fuel per lap, priced in time (best session of the race track) |
//! | GET  | `/api/notebook?track=&car=` | track notebook: records, ideal lap, reserves, risk, tyres, long runs, starts, setups, notes |
//! | POST | `/api/notebook/note` | `{"track", "text"}` – the driver's notes for a track |
//! | GET  | `/api/briefing` | one-page briefing for the chosen race |
//! | GET/POST | `/api/references` | external reference laps from GT7 replays: capture state, captured laps, references; `{"action": "arm"/"stop"/"save"/"remove", …}` |
//! | GET  | `/api/training` | training task (one corner, one change): the active one, finished ones, corners worth a task |
//! | POST | `/api/training` | `{"action": "start", "n", "metric"}` / `{"action": "cancel"}` / `{"action": "again"}` |
//! | GET  | `/api/storage` | what takes how much space: sessions, bin, disk; the recommended clean-up |
//! | POST | `/api/storage/action` | `{"action": "pack"/"prune"/"remove"/"pin"/"unpin"/"restore"/"empty_trash"/"run_plan"/"config", …}` |
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
    /// Diagnostic capture of GT7 replays
    pub replay: crate::session::replay::Shared,
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
        .route("/api/fuelsave", get(fuelsave))
        .route("/api/notebook", get(notebook))
        .route("/api/notebook/note", axum::routing::post(notebook_note))
        .route("/api/briefing", get(briefing))
        .route("/api/training", get(training_state).post(training_action))
        .route("/api/references", get(references_state).post(references_action))
        .route("/api/storage", get(storage_state))
        .route("/api/storage/action", axum::routing::post(storage_action))
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

/// The session to take track knowledge from: the fastest one with the car on the track of the
/// race (practice track when no race is set), else the one the coach shows, else the newest.
/// Returns the session folder name, its track and car.
fn knowledge_session(s: &AppState, c: &mut crate::coach::compare::Comparer) -> Option<(String, Option<String>, Option<i32>)> {
    let rec = &s.settings.data.recordings;
    let (track_of, layouts) = track_matcher(&s.strategy);
    let (track, car) = {
        let st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
        let track = st
            .saved
            .race
            .as_ref()
            .map(|r| r.track.clone())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| st.practice.saved.track.clone());
        let car = st.practice_car(&track);
        (track, car)
    };
    let all = crate::coach::library::index(rec, c, &track_of, layouts);
    if !track.trim().is_empty() {
        let mine = crate::coach::library::of_track(&all, &track, car);
        if let Some(b) = crate::coach::library::best(&mine) {
            return Some((b.session.clone(), b.track.clone(), Some(b.car_code)));
        }
    }
    let feed: Option<String> = std::fs::read_to_string(&s.settings.data.coach_feed)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["session"].as_str().map(str::to_string));
    let pick = feed.and_then(|f| all.iter().find(|i| i.session == f)).or(all.first())?;
    Some((pick.session.clone(), pick.track.clone(), Some(pick.car_code)))
}

/// Notes per track, `notebook.json` in the data folder: track key → {track, text, updated}.
fn notes_path(s: &AppState) -> PathBuf {
    s.settings.data.root.join("notebook.json")
}

fn read_notes(s: &AppState) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(notes_path(s)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The track asked for, else the race track, else the practice track; and the car for it.
fn track_and_car(s: &AppState, track: Option<String>, car: Option<i32>) -> (String, Option<i32>) {
    let st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
    let track = track.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        st.saved.race.as_ref().map(|r| r.track.clone()).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| st.practice.saved.track.clone())
    });
    let car = car.or_else(|| st.practice_car(&track));
    (track, car)
}

fn notebook_json(s: &AppState, track: Option<String>, car: Option<i32>) -> serde_json::Value {
    let (track, car) = track_and_car(s, track, car);
    let (track_of, layouts) = track_matcher(&s.strategy);
    let mut c = comparer();
    let rec = &s.settings.data.recordings;
    let all = crate::coach::library::index(rec, &mut c, &track_of, layouts);
    // Tracks known from the recordings (for the picker)
    let mut tracks: Vec<String> = all.iter().filter_map(|i| i.track.clone()).collect();
    tracks.sort();
    tracks.dedup();
    let names = {
        let st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<(i32, String, Option<String>)> = all
            .iter()
            .flat_map(|i| {
                let meta: serde_json::Value = std::fs::read_to_string(rec.join(&i.session).join("session.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_default();
                meta["lap_setups"]
                    .as_object()
                    .map(|m| m.values().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |id| (i.car_code, id))
            })
            .map(|(c, id)| {
                let n = st.setups.name_of(c, &id);
                (c, id, n)
            })
            .collect();
        ids
    };
    let setup_name = |c: i32, id: &str| names.iter().find(|n| n.0 == c && n.1 == id).and_then(|n| n.2.clone());
    let mut v = if track.trim().is_empty() {
        json!({ "track": "", "records": [] })
    } else {
        crate::coach::notebook::build(rec, &all, &track, car, &mut c, &setup_name)
    };
    v["tracks"] = json!(tracks);
    v["note"] = read_notes(s).get(&strategy::practice::track_key(&track)).cloned().unwrap_or(json!(null));
    v
}

#[derive(Deserialize)]
struct NotebookQuery {
    track: Option<String>,
    car: Option<i32>,
}

async fn notebook(State(s): State<AppState>, Query(q): Query<NotebookQuery>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || notebook_json(&s, q.track, q.car).to_string()).await.unwrap_or_default();
    json_body(body)
}

fn references_root(s: &AppState) -> PathBuf {
    crate::coach::reference::root(&s.settings.data.root)
}

fn references_json(s: &AppState) -> serde_json::Value {
    let root = references_root(s);
    let tracks: Vec<String> = {
        let st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
        st.practice.saved.shapes.iter().map(|sh| sh.track.clone()).collect()
    };
    // For each reference the newest own session on its track (the same car first): "compare"
    let (track_of, layouts) = track_matcher(&s.strategy);
    let all = crate::coach::library::index(&s.settings.data.recordings, &mut comparer(), &track_of, layouts);
    let refs: Vec<serde_json::Value> = crate::coach::reference::list(&root)
        .into_iter()
        .map(|r| {
            let on = crate::coach::library::of_track(&all, &r.track, None);
            let session = on.iter().find(|i| i.car_code == r.car_code).or(on.first()).map(|i| i.session.clone());
            let mut v = json!(r);
            v["session"] = json!(session);
            v
        })
        .collect();
    json!({
        "capture": crate::session::replay::status(&s.replay),
        "pending": crate::coach::reference::pending(&root),
        "references": refs,
        "tracks": tracks,
    })
}

async fn references_state(State(s): State<AppState>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || references_json(&s).to_string()).await.unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct ReferenceBody {
    action: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    driver: String,
    #[serde(default)]
    time: String,
    #[serde(default)]
    track: Option<String>,
}

/// `arm` / `stop` the capture from a replay, `save` a captured lap (driver, official time, track
/// when not recognised), `remove` a captured lap or a reference.
async fn references_action(State(s): State<AppState>, Json(b): Json<ReferenceBody>) -> impl IntoResponse {
    let res = tokio::task::spawn_blocking(move || -> Result<String, String> {
        use crate::coach::reference;
        let root = references_root(&s);
        match b.action.as_str() {
            "arm" => crate::session::replay::arm(&s.replay, true),
            "stop" => crate::session::replay::arm(&s.replay, false),
            "save" => {
                let ms = reference::parse_time(&b.time).ok_or("Zadej oficiální čas kola, např. 1:50,099.")?;
                let (track_of, _) = track_matcher(&s.strategy);
                // A track not stored yet: the reference lap gives its layout (so the own sessions
                // on it are recognised too)
                let pts = reference::pending_points(&root, &b.id);
                let track = match (track_of(&pts), b.track.as_deref().filter(|t| !t.trim().is_empty())) {
                    (Some(t), _) => Some(t),
                    (None, Some(name)) => s.strategy.lock().unwrap_or_else(|e| e.into_inner()).practice.learn_layout(name, &pts),
                    (None, None) => None,
                };
                reference::save(&root, &b.id, &b.driver, ms, track.as_deref(), &|_: &[[f32; 2]]| None)?;
            }
            "remove" => reference::remove(&root, &b.id)?,
            _ => return Err("unknown action".into()),
        }
        Ok(references_json(&s).to_string())
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(body) => json_body(body).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

fn training_file(s: &AppState) -> PathBuf {
    let dir = s.settings.data.coach_feed.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| s.settings.data.root.join("coach"));
    crate::coach::training::path(&dir)
}

fn coach_feed(s: &AppState) -> serde_json::Value {
    std::fs::read_to_string(&s.settings.data.coach_feed).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The active task, the finished ones and the corners of the latest session worth a task (with
/// the coach's suggestion when no task is running).
fn training_json(s: &AppState) -> serde_json::Value {
    let saved = crate::coach::training::load(&training_file(s));
    let feed = coach_feed(s);
    let cands = feed["candidates"].clone();
    let suggestion = if saved.active.is_none() {
        cands.as_array().and_then(|a| a.first()).map(|c| {
            let m = &c["metrics"][0];
            json!({ "n": c["n"], "metric": m["metric"], "label": m["label"], "short": m["short"], "text": m["text"],
                    "gap": m["gap"], "unit": m["unit"], "lost": c["lost"], "laps": c["laps"], "dt": c["dt"] })
        })
    } else {
        None
    };
    json!({
        "active": saved.active.as_ref().map(|t| t.view()),
        "done": saved.done.iter().take(10).map(|t| t.view()).collect::<Vec<_>>(),
        "candidates": if cands.is_null() { json!([]) } else { cands },
        "suggestion": suggestion,
        "session": feed["session"],
    })
}

async fn training_state(State(s): State<AppState>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || training_json(&s).to_string()).await.unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct TrainingBody {
    action: String,
    #[serde(default)]
    n: Option<usize>,
    #[serde(default)]
    metric: Option<String>,
}

/// Start (from the laps of the latest session), cancel, or start the same task again.
async fn training_action(State(s): State<AppState>, Json(b): Json<TrainingBody>) -> impl IntoResponse {
    let res = tokio::task::spawn_blocking(move || -> Result<String, String> {
        use crate::coach::training;
        let file = training_file(&s);
        let mut saved = training::load(&file);
        match b.action.as_str() {
            "cancel" => training::retire(&mut saved, "cancelled"),
            "start" | "again" => {
                let (n, metric) = match b.action.as_str() {
                    "again" => {
                        let last = saved.active.as_ref().or(saved.done.first()).ok_or("no task")?;
                        (last.corner, last.metric.clone())
                    }
                    _ => (b.n.ok_or("n missing")?, b.metric.clone().ok_or("metric missing")?),
                };
                let session = coach_feed(&s)["session"].as_str().map(str::to_string).ok_or("no session")?;
                let dir = s.settings.data.recordings.join(&session);
                let cmp = crate::storage::comparison(&dir, &mut comparer()).map_err(|e| e.to_string())?;
                let track = cmp.track.clone().ok_or_else(|| {
                    "Trať této session neznám – nahraj ji v Tréninku na stránce Strategie, ať ji poznám podle tvaru.".to_string()
                })?;
                let car = std::fs::read_to_string(dir.join("session.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .and_then(|m| m["car_code"].as_i64())
                    .unwrap_or(0) as i32;
                let task = training::start(&cmp, &session, &track, car, n, &metric)?;
                training::retire(&mut saved, "cancelled");
                saved.active = Some(task);
            }
            _ => return Err("unknown action".into()),
        }
        training::save(&file, &saved).map_err(|e| e.to_string())?;
        Ok(training_json(&s).to_string())
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    match res {
        Ok(body) => json_body(body).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

#[derive(Deserialize)]
struct NoteBody {
    track: String,
    text: String,
}

async fn notebook_note(State(s): State<AppState>, Json(b): Json<NoteBody>) -> impl IntoResponse {
    let key = strategy::practice::track_key(&b.track);
    if key.is_empty() {
        return (StatusCode::BAD_REQUEST, "track missing").into_response();
    }
    let mut notes = read_notes(&s);
    let text: String = b.text.chars().take(20_000).collect();
    if text.trim().is_empty() {
        notes.remove(&key);
    } else {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        notes.insert(key, json!({ "track": b.track, "text": text, "updated": now }));
    }
    let path = notes_path(&s);
    let tmp = path.with_extension("json.tmp");
    let ok = std::fs::write(&tmp, serde_json::to_string_pretty(&notes).unwrap_or_default()).and_then(|_| std::fs::rename(&tmp, &path));
    match ok {
        Ok(()) => json_body(json!({ "ok": true }).to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Briefing for the chosen race: the plan (strategy, fuel, tyres), the target lap times, where
/// the time is, where it gets dangerous, the start, and what is still open in the preparation.
async fn briefing(State(s): State<AppState>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || {
        let race = s.strategy.lock().unwrap_or_else(|e| e.into_inner()).saved.race.clone().filter(|r| !r.track.trim().is_empty());
        let Some(race) = race else { return json!({ "error": "no_race" }).to_string() };
        let (track, car) = track_and_car(&s, Some(race.track.clone()), None);
        let book = notebook_json(&s, Some(track.clone()), car);
        let (options, plan, setup, prep) = {
            let mut st = s.strategy.lock().unwrap_or_else(|e| e.into_inner());
            let (o, p) = st.preview(&race);
            let setup = car.and_then(|c| st.setups.active(c)).map(|x| json!({ "name": x.name, "bop": x.bop }));
            let perf = s.perf.clone();
            let prep = st.prep(|car, setup| crate::perf::track::read(perf.dir(), car, setup).is_some());
            (o, p, setup, prep)
        };
        // Fuel: what saving costs (lift and coast), for the amount that saves a stop if there is one
        let target = plan.no_stop_fpl.or(plan.one_stop_fpl);
        let save = match (plan.fuel_per_lap, target) {
            (Some(f), Some(t)) if f > t => Some((f - t) as f64),
            _ => None,
        };
        let fuel = if race.no_fuel {
            json!(null)
        } else {
            let mut c = comparer();
            let book_session = book["best"]["session"].as_str().map(str::to_string);
            book_session
                .and_then(|sess| crate::coach::fuelsave::analyse(&s.settings.data.recordings.join(sess), &mut c, save).ok())
                .unwrap_or(json!(null))
        };
        json!({
            "race": race,
            "track": track,
            "car": car,
            "car_name": car.and_then(crate::telemetry::cars::name),
            "setup": setup,
            "options": options,
            "plan": {
                "fuel_per_lap": plan.fuel_per_lap, "fuel_source": plan.fuel_source, "stops_needed": plan.stops_needed,
                "one_stop_fpl": plan.one_stop_fpl, "no_stop_fpl": plan.no_stop_fpl, "pit_loss": plan.pit_loss,
                "total_laps": plan.total_laps, "practice_use": plan.practice_use, "save": save,
            },
            "fuel": fuel,
            "book": book,
            "prep": prep,
        })
        .to_string()
    })
    .await
    .unwrap_or_default();
    json_body(body)
}

fn storage_json(s: &AppState) -> serde_json::Value {
    let p = crate::storage::Paths::new(&s.settings.data.root);
    let cfg = crate::storage::load_config(&p);
    let (track_of, layouts) = track_matcher(&s.strategy);
    let all = {
        let mut c = comparer();
        crate::coach::library::index(&p.recordings, &mut c, &track_of, layouts)
    };
    let rows = crate::storage::session_rows(&p, &cfg, &all);
    let plan = crate::storage::plan(&rows, &cfg);
    let trash: Vec<serde_json::Value> = crate::storage::trash_entries(&p)
        .into_iter()
        .map(|(date, name, bytes, files)| json!({ "date": date, "session": name, "bytes": bytes, "files": files }))
        .collect();
    let total_rec: u64 = rows.iter().map(|r| r.bytes).sum();
    let by_state = |st: &str| rows.iter().filter(|r| r.state == st).map(|r| r.bytes).sum::<u64>();
    let trash_bytes: u64 = trash.iter().map(|t| t["bytes"].as_u64().unwrap_or(0)).sum();
    let other = crate::storage::size_of(&p.root).saturating_sub(total_rec + trash_bytes);
    // Growth: what the sessions of the last 30 days take
    let month: u64 = rows.iter().filter(|r| r.age_days <= 30.0).map(|r| r.bytes).sum();
    let disk = crate::storage::disk(&p.root);
    json!({
        "config": cfg,
        "disk": disk.map(|(free, total)| json!({ "free": free, "total": total })),
        "totals": { "live": by_state("live"), "raw": by_state("raw"), "packed": by_state("packed"), "pruned": by_state("pruned"),
                    "trash": trash_bytes, "other": other, "recordings": total_rec, "month": month },
        "sessions": rows,
        "plan": plan,
        "plan_frees": plan.iter().map(|x| x.frees).sum::<u64>(),
        "trash": trash,
    })
}

async fn storage_state(State(s): State<AppState>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || storage_json(&s).to_string()).await.unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct StorageAction {
    action: String,
    session: Option<String>,
    /// Several sessions at once: "bulk_pack", "bulk_prune", "bulk_remove", "bulk_pin", "bulk_unpin"
    sessions: Option<Vec<String>>,
    date: Option<String>,
    config: Option<crate::storage::Config>,
}

/// Storage actions. Nothing is deleted for good except "empty_trash"; everything else moves to
/// the bin or packs.
async fn storage_action(State(s): State<AppState>, Json(a): Json<StorageAction>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || {
        let p = crate::storage::Paths::new(&s.settings.data.root);
        let mut cfg = crate::storage::load_config(&p);
        let session = a.session.clone().unwrap_or_default();
        let step_of = |action: &'static str| crate::storage::Step { session: session.clone(), action, frees: 0, why: String::new() };
        // Record sessions keep more laps when cleaned
        let is_record = |name: &str| {
            let (track_of, layouts) = track_matcher(&s.strategy);
            let all = crate::coach::library::index(&p.recordings, &mut comparer(), &track_of, layouts);
            crate::storage::session_rows(&p, &cfg, &all).iter().any(|r| r.session == name && r.record)
        };
        let result: std::io::Result<String> = match a.action.as_str() {
            "pack" => crate::storage::run_step(&p, &step_of("pack"), false, &mut comparer())
                .map(|b| format!("zabaleno, ušetřeno {} MB", b / 1_000_000)),
            "prune" => {
                let rec = is_record(&session);
                crate::storage::run_step(&p, &step_of("prune"), rec, &mut comparer())
                    .map(|b| format!("pročištěno, {} MB v koši", b / 1_000_000))
            }
            "remove" => {
                crate::storage::run_step(&p, &step_of("remove"), false, &mut comparer()).map(|b| format!("v koši ({} MB)", b / 1_000_000))
            }
            "pin" | "unpin" => {
                if a.action == "pin" {
                    cfg.pinned.insert(session.clone());
                } else {
                    cfg.pinned.remove(&session);
                }
                crate::storage::save_config(&p, &cfg).map(|_| "uloženo".into())
            }
            bulk if bulk.starts_with("bulk_") => {
                let what = &bulk[5..];
                let names = a.sessions.clone().unwrap_or_default();
                if what == "pin" || what == "unpin" {
                    for n in &names {
                        if what == "pin" {
                            cfg.pinned.insert(n.clone());
                        } else {
                            cfg.pinned.remove(n);
                        }
                    }
                    crate::storage::save_config(&p, &cfg).map(|_| format!("uloženo ({})", names.len()))
                } else {
                    let action: &'static str = match what {
                        "pack" => "pack",
                        "prune" => "prune",
                        "remove" => "remove",
                        _ => "",
                    };
                    let (track_of, layouts) = track_matcher(&s.strategy);
                    let all = crate::coach::library::index(&p.recordings, &mut comparer(), &track_of, layouts);
                    let rows = crate::storage::session_rows(&p, &cfg, &all);
                    let (mut done, mut bytes, mut skipped, mut errors) = (0, 0u64, 0, Vec::new());
                    for n in &names {
                        let Some(r) = rows.iter().find(|r| &r.session == n) else { continue };
                        // Pinned and live sessions are never cleaned or removed in bulk
                        if action.is_empty()
                            || r.state == "live"
                            || (r.pinned && action != "pack")
                            || (action == "prune" && (r.empty || r.state == "pruned"))
                        {
                            skipped += 1;
                            continue;
                        }
                        let step = crate::storage::Step { session: n.clone(), action, frees: 0, why: String::new() };
                        match crate::storage::run_step(&p, &step, r.record, &mut comparer()) {
                            Ok(b) => {
                                done += 1;
                                bytes += b;
                            }
                            Err(e) => errors.push(format!("{n}: {e}")),
                        }
                    }
                    let verb = match action {
                        "pack" => "zabaleno",
                        "prune" => "pročištěno",
                        _ => "do koše",
                    };
                    let skip = if skipped > 0 {
                        format!(", přeskočeno {skipped} (připíchnuté / nahrávaná / nejde)")
                    } else {
                        String::new()
                    };
                    if errors.is_empty() {
                        Ok(format!("{verb}: {done} sessions, {} MB{skip}", bytes / 1_000_000))
                    } else {
                        Err(std::io::Error::other(format!("{verb}: {done}{skip}; chyby: {}", errors.join("; "))))
                    }
                }
            }
            "restore" => crate::storage::restore(&p, a.date.as_deref().unwrap_or(""), &session).map(|_| "obnoveno".into()),
            "empty_trash" => crate::storage::purge(&p, None).map(|b| format!("koš vysypán ({} MB)", b / 1_000_000)),
            "config" => match a.config {
                Some(mut c) => {
                    c.pinned = cfg.pinned.clone();
                    c.keep_days = c.keep_days.clamp(7, 3650);
                    c.trash_days = c.trash_days.clamp(1, 90);
                    crate::storage::save_config(&p, &c).map(|_| "uloženo".into())
                }
                None => Ok(String::new()),
            },
            "run_plan" => {
                let (track_of, layouts) = track_matcher(&s.strategy);
                let all = crate::coach::library::index(&p.recordings, &mut comparer(), &track_of, layouts);
                let rows = crate::storage::session_rows(&p, &cfg, &all);
                let mut done = 0u64;
                let mut errors = Vec::new();
                for step in crate::storage::plan(&rows, &cfg) {
                    let rec = rows.iter().any(|r| r.session == step.session && r.record);
                    match crate::storage::run_step(&p, &step, rec, &mut comparer()) {
                        Ok(b) => done += b,
                        Err(e) => errors.push(format!("{}: {e}", step.session)),
                    }
                }
                if errors.is_empty() {
                    Ok(format!("hotovo, uvolněno ~{} MB", done / 1_000_000))
                } else {
                    Err(std::io::Error::other(errors.join("; ")))
                }
            }
            _ => Err(std::io::Error::other("neznámá akce")),
        };
        let mut v = storage_json(&s);
        match result {
            Ok(m) => v["message"] = json!(m),
            Err(e) => v["error"] = json!(e.to_string()),
        }
        v.to_string()
    })
    .await
    .unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct FuelSaveQuery {
    /// Fuel to save per lap; none: plans for 2, 4 and 6 %
    save: Option<f64>,
}

async fn fuelsave(State(s): State<AppState>, Query(q): Query<FuelSaveQuery>) -> impl IntoResponse {
    let body = tokio::task::spawn_blocking(move || {
        let mut c = comparer();
        let Some((session, track, car)) = knowledge_session(&s, &mut c) else { return json!({ "error": "no_session" }).to_string() };
        let dir = s.settings.data.recordings.join(&session);
        match crate::coach::fuelsave::analyse(&dir, &mut c, q.save.filter(|v| *v > 0.0)) {
            Ok(mut v) => {
                v["track"] = json!(track);
                v["car"] = json!(car);
                v.to_string()
            }
            Err(e) => json!({ "error": e.to_string() }).to_string(),
        }
    })
    .await
    .unwrap_or_default();
    json_body(body)
}

#[derive(Deserialize)]
struct TraceQuery {
    session: Option<String>,
    lap: Option<String>,
    /// Compare with: "ideal" (best corners of the session), "track" (best lap with this car on
    /// this layout, any session) or "<session>/<file>"; default the best lap of the session
    vs: Option<String>,
}

/// One comparer for all requests: it caches loaded laps and results.
fn comparer() -> std::sync::MutexGuard<'static, crate::coach::compare::Comparer> {
    static COMPARER: std::sync::OnceLock<Mutex<crate::coach::compare::Comparer>> = std::sync::OnceLock::new();
    COMPARER.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Recognises the layout of lap positions by the shapes stored in practice; and how many there are.
fn track_matcher(st: &strategy::Shared) -> (impl Fn(&[[f32; 2]]) -> Option<String>, usize) {
    let shapes = st.lock().unwrap_or_else(|e| e.into_inner()).practice.saved.shapes.clone();
    let n = shapes.len();
    (move |pts: &[[f32; 2]]| strategy::practice::track_of(&shapes, pts), n)
}

/// Lap comparison traces. Session: the given one (a folder name in the recordings), otherwise the
/// one the coach shows, otherwise the newest.
async fn traces(State(s): State<AppState>, Query(q): Query<TraceQuery>) -> impl IntoResponse {
    let rec = s.settings.data.recordings.clone();
    let (track_of, layouts) = track_matcher(&s.strategy);
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
        let mut c = comparer();
        // This session in the library: its track, and the best lap with the car there
        let all = crate::coach::library::index(&rec, &mut c, &track_of, layouts);
        let me = all.iter().find(|i| i.session == session).cloned();
        let track_best = me.as_ref().and_then(|m| {
            let same = crate::coach::library::of_track(&all, m.track.as_deref()?, Some(m.car_code));
            crate::coach::library::best(&same).cloned()
        });
        let safe_file = |n: &str| !n.is_empty() && !n.contains(['/', '\\']) && n != "..";
        let refs_root = crate::coach::reference::root(rec.parent().unwrap_or(&rec));
        // A reference only for a session on its track (28 Oct: the Barcelona reference held against
        // a lap of another track gave −94 s in a corner)
        let key = crate::strategy::practice::track_key;
        let wanted = q.vs.as_deref().and_then(|v| v.strip_prefix("ref:")).and_then(|id| crate::coach::reference::get(&refs_root, id));
        let same_track = |r: &crate::coach::reference::Reference| me.as_ref().and_then(|m| m.track.as_deref()).is_some_and(|t| key(t) == key(&r.track));
        let ext = wanted.clone().filter(|r| same_track(r));
        let vs = match q.vs.as_deref() {
            Some(v) if v.starts_with("ref:") => match ext.as_ref().and_then(|r| crate::coach::reference::file(&refs_root, &r.id)) {
                Some(p) => crate::coach::compare::Versus::Lap(p),
                None => crate::coach::compare::Versus::Session,
            },
            Some("ideal") => crate::coach::compare::Versus::Ideal,
            Some("track") => match &track_best {
                Some(b) if b.session != session => {
                    crate::coach::compare::Versus::Lap(rec.join(&b.session).join(b.best_file.clone().unwrap_or_default()))
                }
                _ => crate::coach::compare::Versus::Session,
            },
            Some(other) => match other.split_once('/') {
                Some((sess, file)) if safe(sess) && safe_file(file) && crate::coach::lap::exists(&rec.join(sess).join(file)) => {
                    crate::coach::compare::Versus::Lap(rec.join(sess).join(file))
                }
                _ => crate::coach::compare::Versus::Session,
            },
            None => crate::coach::compare::Versus::Session,
        };
        // Shift points from driving for the car and the setup the session was driven with
        let meta: serde_json::Value = std::fs::read_to_string(rec.join(&session).join("session.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let car = meta["car_code"].as_i64().unwrap_or(0) as i32;
        let setup = meta["lap_setups"].as_object().and_then(|m| m.values().next_back()).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let drive = crate::perf::track::read(&perf_dir, car, &setup).map(|t| crate::coach::shifts::Optimum {
            gears: t.dyno.shifts.iter().map(|s| (s.gear, s.rpm as f64, s.late)).collect(),
            max_rpm: t.dyno.max_rpm as f64,
            source: "drive",
        });
        match c.traces(&rec.join(&session), q.lap.as_deref(), drive, &vs) {
            Ok(mut v) => {
                v["track"] = json!(me.as_ref().and_then(|m| m.track.clone()));
                v["track_best"] = json!(track_best
                    .as_ref()
                    .map(|b| json!({ "session": b.session, "file": b.best_file, "ms": b.best_ms, "started": b.started, "same": b.session == session })));
                // External references on this track (the same car first)
                let track = me.as_ref().and_then(|m| m.track.clone());
                v["car_code"] = json!(car);
                if let Some(r) = wanted.as_ref().filter(|r| !same_track(r)) {
                    v["versus_note"] = json!(format!("Reference {} je z trati {} – tahle session je jinde.", r.driver, r.track));
                }
                v["references"] = json!(track.as_deref().map(|t| crate::coach::reference::of_track(&refs_root, t, Some(car))).unwrap_or_default());
                if let (Some(r), crate::coach::compare::Versus::Lap(_)) = (&ext, &vs) {
                    v["versus"]["kind"] = json!("ref");
                    v["versus"]["ms"] = json!(r.official_ms);
                    v["versus"]["ref"] = json!(r);
                } else if let crate::coach::compare::Versus::Lap(p) = &vs {
                    let sess = p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    v["versus"]["session"] = json!(sess);
                    let other = all.iter().find(|i| i.session == sess);
                    v["versus"]["started"] = json!(other.map(|i| i.started.clone()));
                    let file = Some(crate::coach::lap::lap_name(p));
                    if other.is_some_and(|o| o.best_file == file) {
                        v["versus"]["ms"] = json!(other.and_then(|o| o.best_ms));
                    }
                }
                v.to_string()
            }
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
    let kart = crate::telemetry::cars::name(car).is_some_and(|n| n.contains("Kart"));
    let fair = strategy::setup_compare::fair(&laps, kart);
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
            "wear_from_data": fair.wear_from_data, "fuel_from_data": fair.fuel_from_data, "fixed": fair.fixed })
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
