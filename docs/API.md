# BOXBOXBOX – API pro klienty (iOS aplikace)

Server poslouchá na portu z `config.toml` (u tebe `http://192.168.1.156:8099`). Všechno je JSON přes
HTTP, živá data přes WebSocket. Bez přihlášení – je to v domácí síti.

## Verze

`GET /api/version` → `{"server": "0.8.0-fork1", "api": 1}`. Klient porovná `api` se svou verzí: když je
vyšší, než zná, upozorní („aktualizuj aplikaci“). `api` se zvyšuje jen při změně nebo odstranění
pole, na které klient spoléhá; nová pole ho nemění. Číslo je `API_VERSION` v `src/web/api.rs`.

## Živá data – `GET /ws` (WebSocket)

Server posílá nejvýš `hz`× za sekundu (výchozí 30) vždy jen poslední stav: jeden JSON objekt
`Snapshot` (textová zpráva). Klient nic neposílá. Při výpadku se znovu připojí; po připojení dostane
hned aktuální stav.

Pro závod je nejdůležitější `strategy` (objekt `Plan`): hlášky `calls`, okno zastávky, palivo,
gumy a plány. Pole `Snapshot` jsou popsaná níž; ta, která klient nepotřebuje, může ignorovat.

## Endpointy

Seznam je v hlavičce `src/web/api.rs` (zdroj pravdy). Nejčastější pro závod:

| Metoda | Cesta | K čemu |
|---|---|---|
| GET | `/api/version` | verze serveru a API |
| GET | `/ws` | živá data (`Snapshot`) |
| GET | `/api/strategy` | uložený stav závodu (`Saved`: závod, stinty, zastávky, kola, kalibrace) |
| GET | `/api/briefing` | brífink vybraného závodu: plány (`options`), vlastní plán (`custom`), podklady |
| POST | `/api/strategy/race` | nastavit závod (`Race`) |
| POST | `/api/strategy/custom` | vlastní plán `{"stints":[{"compound","laps"}], "follow": bool}` → `Custom` |
| POST | `/api/strategy/action` | `{"action": "compound", "compound": "RS"}` (potvrdit/opravit směs), `"new_tyres"`, `"same_tyres"`, `"no_stop"`, `"reset"`, `"forget_wear"`, trénink `"practice"` … |
| GET | `/api/races` | odjeté závody (`Summary[]`, nejnovější první) |
| GET | `/api/races?id=` | jeden závod celý (`Saved`) |
| POST | `/api/races` | `{"id","action":"compounds","compounds":[..]}` / `{"id","action":"remove"}` |
| GET | `/api/practice?track=&car=` | trénink: stav, čísla po směsích, poslední kola |

Chyby: HTTP 404 (neznámé id), 400 (neznámá akce); jinak 200 s JSON.

## Objekty

Tabulky jsou vygenerované ze struktur v Rustu (`scripts/api-doc.sh`, viz konec).
Typy: `Option<T>` = může být `null`, `Vec<T>` = pole, `&'static str` = řetězec.

### `Snapshot`

| Pole | Typ | Význam |
|---|---|---|
| `live` | `bool` |  |
| `on_track` | `bool` |  |
| `paused` | `bool` |  |
| `loading` | `bool` |  |
| `speed_kmh` | `f32` |  |
| `rpm` | `f32` |  |
| `rpm_alert_min` | `i16` |  |
| `rpm_alert_max` | `i16` |  |
| `gear` | `u8` | 0 = reverse, 15 = neutral (as GT7 sends it) |
| `suggested_gear` | `Option<u8>` |  |
| `throttle` | `f32` |  |
| `brake` | `f32` |  |
| `tire_temp` | `[f32; 4]` | FL, FR, RL, RR |
| `tire_slip` | `[Option<f32>; 4]` | Wheel surface speed / car speed (1.0 = no slip), `null` when standing |
| `wheel_speed_kmh` | `[f32; 4]` |  |
| `tire_radius` | `[f32; 4]` |  |
| `suspension` | `[f32; 4]` |  |
| `fuel` | `f32` |  |
| `fuel_capacity` | `f32` |  |
| `fuel_per_lap` | `Option<f32>` |  |
| `fuel_laps_left` | `Option<f32>` |  |
| `lap` | `i16` |  |
| `total_laps` | `i16` |  |
| `current_lap_ms` | `Option<i64>` |  |
| `last_lap_ms` | `Option<i32>` |  |
| `best_lap_ms` | `Option<i32>` |  |
| `live_delta_ms` | `Option<i64>` |  |
| `laps` | `Vec<i32>` |  |
| `laps_full` | `Vec<bool>` | Per lap of `laps`: driven from the line to the line (false: the first lap from the pit exit or of a race with separate start and finish lines – shorter, not comparable) |
| `position` | `Option<i16>` |  |
| `cars` | `Option<i16>` |  |
| `water_temp` | `f32` |  |
| `oil_temp` | `f32` |  |
| `oil_pressure` | `f32` |  |
| `boost_bar` | `Option<f32>` |  |
| `tcs` | `bool` |  |
| `asm` | `bool` |  |
| `rev_limiter` | `bool` |  |
| `handbrake` | `bool` |  |
| `in_gear` | `bool` |  |
| `lights` | `bool` |  |
| `high_beam` | `bool` |  |
| `flags` | `u16` |  |
| `car_code` | `i32` |  |
| `car_name` | `Option<&'static str>` |  |
| `calc_max_speed` | `i16` |  |
| `transmission_top_speed_kmh` | `f32` |  |
| `gear_ratios` | `Vec<f32>` |  |
| `clutch_pedal` | `f32` |  |
| `clutch_engagement` | `f32` |  |
| `rpm_clutch_gearbox` | `f32` |  |
| `energy_recovery` | `f32` |  |
| `time_of_day_ms` | `i32` |  |
| `pos` | `[f32; 3]` | World position (m) and attitude (degrees) |
| `heading` | `f32` |  |
| `pitch` | `f32` |  |
| `roll` | `f32` |  |
| `body_height` | `f32` |  |
| `road_plane_distance` | `f32` |  |
| `map_version` | `u32` |  |
| `steering_deg` | `f32` |  |
| `yaw_rate_deg` | `f32` |  |
| `v_lat_kmh` | `f32` |  |
| `body_slip` | `Option<f32>` |  |
| `slip_front` | `Option<f32>` |  |
| `slip_rear` | `Option<f32>` |  |
| `balance` | `Option<f32>` | Smoothed, degrees: > 0 understeer, < 0 oversteer |
| `steer_ratio` | `f32` | Accelerations as GT7 sends them (lateral, vertical, longitudinal) |
| `sway` | `f32` |  |
| `heave` | `f32` |  |
| `surge` | `f32` |  |
| `extended` | `bool` |  |
| `recording` | `bool` |  |
| `perf` | `perf::Live` | Measurements (0–100, braking distances), page 4 |
| `link` | `Link` | Connection to the PS5 (search state while there is no data) |
| `strategy` | `Option<strategy::Plan>` | Race strategy (page 5), `null` without a strategy |
| `seg_now` | `Vec<u8>` | Track segments: result in the running lap and in the previous one (0 none, 1 purple, 2 green, 3 red), and the time to the best per segment (ms) |
| `seg_last` | `Vec<u8>` |  |
| `seg_delta` | `Vec<Option<i32>>` |  |

### `Plan`

| Pole | Typ | Význam |
|---|---|---|
| `race` | `Option<String>` |  |
| `total_laps` | `Option<u32>` |  |
| `laps_left` | `Option<f32>` |  |
| `fuel_per_lap` | `Option<f32>` |  |
| `fuel_source` | `&'static str` | "measured", "manual" or "so_far" (estimated from the fuel burnt since the start) |
| `fuel_needed` | `Option<f32>` | Fuel needed to the finish, reserve included |
| `fuel_margin_laps` | `Option<f32>` | Laps the fuel on board reaches beyond the finish (negative = short) |
| `stops_needed` | `u32` |  |
| `one_stop_fpl` | `Option<f32>` | Two stops needed now, but one would do at this fuel per lap (saving fuel) |
| `no_stop_fpl` | `Option<f32>` | One stop needed now, but none at this fuel per lap (no mandatory stop) |
| `fuel_target` | `Option<f32>` | The fuel per lap to drive to: the saving target when saving saves a stop, else the measured figure |
| `lap_fuel_used` | `Option<f32>` | This lap so far: fuel used, and what the target allows at this point of the lap |
| `lap_fuel_allowed` | `Option<f32>` |  |
| `last_lap_fuel` | `Option<f32>` | Fuel used in the last full lap without refuelling |
| `fuel_per_lap_max` | `Option<f32>` | Flat out (fuel map 1): the highest steady consumption known for this car and track (this race or earlier ones), when clearly above `fuel_per_lap` |
| `fuel_max_source` | `&'static str` | "race" (measured in this race) or "history" (earlier races with the car) |
| `fuel_margin_laps_max` | `Option<f32>` | Laps the fuel on board reaches beyond the finish at `fuel_per_lap_max` |
| `stop_add_max` | `Option<f32>` | The same refuel amounts as `stop_add`, `next_stop_fuel` and `add_now`, for flat out |
| `next_stop_fuel_max` | `Option<f32>` |  |
| `add_now_max` | `Option<f32>` |  |
| `fuel_step_lap` | `Option<i16>` | The consumption changed during the race (new stretch since this lap) |
| `track_mismatch` | `Option<String>` | The laps are driven on another track than the race's (see [`Strategy::track_mismatch`]) |
| `window` | `Option<[i16; 2]>` | Next stop: enter the pit at the end of one of these laps |
| `target_lap` | `Option<i16>` | Next stop of the recommended strategy (box at the end of this lap) |
| `laps_to_box` | `Option<i16>` | Laps until that stop (0 = box at the end of this lap) |
| `next_stop_fuel` | `Option<f32>` | Fuel to add at the next planned stop |
| `stop_add` | `Option<f32>` | In the pit box right now: fuel to add, and the laps it is meant for |
| `stop_add_laps` | `Option<u32>` |  |
| `options` | `planner::Options` | Recommended and alternative strategy for the rest of the race. Following the own plan: `best` is the own plan (the rest of it), `alternative` the recommendation. |
| `following` | `&'static str` | "custom" when the calls follow the driver's own plan |
| `custom_note` | `Option<String>` | Why the own plan is not followed now (it cannot be kept any more) |
| `pit_loss` | `f32` |  |
| `pit_loss_source` | `&'static str` | "manual", "measured", "catalogue" (pit lane delta of the GT7 track database) or "default" |
| `pit_loss_n` | `usize` | Stops the measured pit loss comes from |
| `practice_use` | `Option<PracticeUse>` | Practice figures the plan uses (car, laps per compound), and the practice mode state |
| `practice` | `PracticeState` |  |
| `add_now` | `Option<f32>` | Refuel amount when boxing at the end of this lap |
| `refuel_s` | `Option<f32>` |  |
| `reserve_laps` | `f32` |  |
| `stop_reserve_laps` | `f32` |  |
| `pit_required` | `bool` |  |
| `pit_done` | `bool` |  |
| `no_wear` | `bool` | Race without tyre wear / fuel use |
| `no_fuel` | `bool` |  |
| `mandatory` | `Option<String>` |  |
| `mandatory_done` | `bool` |  |
| `compound` | `Option<String>` |  |
| `stint_laps` | `f32` |  |
| `wear_now` | `Option<f32>` |  |
| `wear_per_lap` | `Option<f32>` |  |
| `wear_source` | `&'static str` | "stint" (read this stint) or "calibration" (earlier runs) |
| `wear_limit` | `f32` |  |
| `wear_laps_left` | `Option<f32>` |  |
| `wear_at_finish` | `Option<f32>` |  |
| `tyre_question` | `bool` |  |
| `compound_ask` | `bool` | New tyres went on, their compound is only the plan's guess: ask the driver |
| `calls` | `Vec<Call>` | Advice for the driver, most important first |

### `Call`

| Pole | Typ | Význam |
|---|---|---|
| `level` | `&'static str` | "box", "warn" or "info" |
| `text` | `String` |  |

### `Option_`

| Pole | Typ | Význam |
|---|---|---|
| `stints` | `Vec<Stint>` |  |
| `stops` | `Vec<PlannedStop>` |  |
| `cost` | `f32` | Estimated cost in seconds (only differences between options mean something) |
| `delta` | `f32` | Slower than the recommendation by, s |
| `over_limit` | `bool` | A limit had to be broken (tyre life): no plan fits the entered limits |
| `parts` | `Parts` |  |
| `risks` | `Vec<String>` | What could go wrong ("palivo na hraně …"), for the briefing |

### `Stint`

| Pole | Typ | Význam |
|---|---|---|
| `compound` | `String` |  |
| `laps` | `u32` |  |

### `PlannedStop`

| Pole | Typ | Význam |
|---|---|---|
| `lap` | `i16` | Box at the end of this lap (with `stop_now`: the lap before `first_lap`) |
| `compound` | `String` |  |
| `fuel` | `f32` | Fuel to add |

### `Custom`

| Pole | Typ | Význam |
|---|---|---|
| `plan` | `Option<planner::Option_>` |  |
| `error` | `Option<String>` |  |
| `compounds` | `Vec<String>` | Compounds the plan may use (those with data), laps of the race |
| `laps` | `u32` |  |
| `rules` | `Vec<String>` | Rules of the race the plan breaks |
| `follow` | `bool` | The race calls follow this plan |

### `Saved`

| Pole | Typ | Význam |
|---|---|---|
| `race` | `Option<Race>` |  |
| `stints` | `Vec<Stint>` |  |
| `stops` | `Vec<Stop>` |  |
| `fuel_laps` | `Vec<FuelLap>` |  |
| `calibration` | `BTreeMap<String, f32>` | Tyre wear per lap (%), key "car\|track\|compound\|multiplier"; pit loss (s), key "pit\|track" |
| `pit_samples` | `BTreeMap<String, Vec<f32>>` | Measured pit losses per track (key "pit\|track"), the last few; `calibration` holds their median |
| `archived` | `bool` | Already kept in `strategy-history/` (at the finish): a restart does not keep it again |
| `car` | `Option<i32>` | Car the race laps were driven with (for the race history) |

### `Race`

| Pole | Typ | Význam |
|---|---|---|
| `id` | `String` | Firestore document id; empty = typed in by hand |
| `name` | `String` |  |
| `track` | `String` |  |
| `laps` | `Option<u32>` |  |
| `tyre_wear` | `Option<f32>` |  |
| `fuel_mult` | `Option<f32>` |  |
| `refuel_rate` | `Option<f32>` | Refuelling speed (fuel units per second) |
| `pit_required` | `bool` |  |
| `mandatory` | `Option<String>` | Compound that has to be used once, e.g. "RS" |
| `start_compound` | `Option<String>` | Tyres at the start; after a tyre change the same compound is assumed |
| `fuel_per_lap` | `Option<f32>` | Fuel per lap to plan with before a lap has been measured |
| `reserve_laps` | `Option<f32>` |  |
| `stop_reserve_laps` | `Option<f32>` | Fuel left on arriving at a planned stop, in laps (default [`DEFAULT_STOP_RESERVE`]) |
| `custom` | `Vec<CustomStint>` | The driver's own plan: compound and laps per stint (the third plan of the briefing) |
| `follow_custom` | `bool` | In the race the calls (box lap, fuel to add, tyres) follow the own plan, not the recommendation |
| `car` | `Option<i32>` | Car the race is planned with (its practice laps on the track); None = the car driven now, else the one with the most laps there |
| `wear_limit` | `Option<f32>` |  |
| `tyres` | `BTreeMap<String, TyreSpec>` | Per compound ("RS", "RM" …): laps a set lasts and pace difference |
| `deg` | `Option<f32>` | Lap time lost per lap of tyre age, s (default [`DEFAULT_DEG`]) |
| `pit_loss` | `Option<f32>` | Time lost by a stop without refuelling, s (default: measured on this track, else [`DEFAULT_PIT_LOSS`]) |
| `no_wear` | `bool` | The tyres do not wear / no fuel is used (karts, snow and dirt events): no tyre or fuel strategy, only the rules (mandatory stop, mandatory compound) |
| `no_fuel` | `bool` |  |
| `date` | `Option<String>` | Date of the race from the championship calendar ("2026-11-23T20:00:00…") |

### `CustomStint`

| Pole | Typ | Význam |
|---|---|---|
| `compound` | `String` |  |
| `laps` | `u32` |  |

### `Stint`

| Pole | Typ | Význam |
|---|---|---|
| `start` | `f32` | Race distance (laps) when these tyres went on |
| `compound` | `Option<String>` |  |
| `wear` | `Vec<WearMark>` |  |
| `confirmed` | `bool` | The compound was chosen by the driver (start, „Nasazeno“, new tyres); false: guessed from the plan after a detected tyre change – asked on the dashboard, left out of the wear fit |

### `Stop`

| Pole | Typ | Význam |
|---|---|---|
| `lap` | `i16` | Lap during which the car was in the pit |
| `fuel_added` | `f32` |  |
| `tyres` | `Option<bool>` | New tyres? `None` = not answered yet |

### `FuelLap`

| Pole | Typ | Význam |
|---|---|---|
| `lap` | `i16` |  |
| `used` | `f32` | Fuel burnt (refuelling not counted) |
| `refuel` | `bool` |  |
| `ms` | `Option<i32>` | Lap time from GT7, ms |
| `fuel` | `Option<f32>` | Fuel at the start of the lap, litres |

### `Summary`

| Pole | Typ | Význam |
|---|---|---|
| `id` | `String` | File name without `.json` ("current" for the race in `strategy.json`) |
| `at` | `String` | "2026-11-23 22:43" |
| `name` | `String` |  |
| `track` | `String` |  |
| `car` | `Option<i32>` |  |
| `car_name` | `Option<String>` |  |
| `laps` | `i16` | Laps driven, laps of the race |
| `race_laps` | `Option<u32>` |  |
| `finished` | `bool` |  |
| `total_ms` | `i64` | Sum of the lap times, best lap, ms |
| `best_ms` | `Option<i32>` |  |
| `stops` | `usize` |  |
| `compounds` | `Vec<String>` | Compounds of the stints in order ("RM", "RS", "RS") |

### `PracticeState`

| Pole | Typ | Význam |
|---|---|---|
| `active` | `bool` |  |
| `track` | `String` |  |
| `compound` | `Option<String>` |  |
| `set_age` | `u32` |  |
| `counting` | `bool` |  |
| `wear_mult` | `f32` |  |
| `fuel_mult` | `f32` |  |
| `laps` | `usize` | Laps stored (changes when a lap was added: the dashboard reloads the figures) |
| `layout_warning` | `Option<String>` | The last lap does not fit the layout of the track name |
| `track_length_m` | `Option<f32>` | Length of the stored layout, m |
| `missed_laps` | `u32` | Laps not kept because no compound is chosen |
| `question` | `Option<TrackQuestionView>` | A full lap on a layout not stored yet: its length and the catalogue layouts it may be |

## Jak dokument obnovit

```
scripts/api-doc.sh
```

Úvod je v `docs/API.intro.md`, tabulky se generují z `///` komentářů struktur.
