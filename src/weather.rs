//! Weather from Open-Meteo: current model values and a daily history.
//!
//! Weather is **model data for the location**, not a measurement in a space. It lives in its own
//! tables (`weather_current`, `weather_daily`, `weather_hours`) and never mixes with the manual or
//! sensor readings in `climate_readings`.
//!
//! One call returns current values plus 92 days of hourly and daily data. The hourly series feeds
//! the 24-hour and 7-day graph, the night average and the dominant daytime condition; the daily
//! series gives rain, sunshine, the temperature range and the sun times. The app can therefore fill
//! the days and hours it was closed the next time it opens.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::calendar::Date;
use crate::store::{Location, Store};

pub const PROVIDER: &str = "Open-Meteo";
/// How many past days one sync asks for. Three months keeps the 90-day graph honest; Open-Meteo
/// accepts up to 92 past days on the forecast endpoint.
pub const HISTORY_DAYS: i64 = 92;
/// The default while Open-Meteo is on: one sync every two hours.
pub const DEFAULT_PER_DAY: i64 = 12;
pub const MAX_PER_DAY: i64 = 96;
/// Open-Meteo refreshes its current values about every fifteen minutes; asking faster is useless.
pub const MIN_INTERVAL_MINUTES: i64 = 15;

const PER_DAY: &str = "weather_updates_per_day";
const CALLS_DATE: &str = "weather_calls_date";
const CALLS_COUNT: &str = "weather_calls_count";
const LAST_CALL: &str = "weather_last_call";
const LAST_ERROR: &str = "weather_last_error";
/// Consecutive failed calls, so a retry can wait a little longer each time without waiting the
/// full budget interval after one bad minute.
const FAILURES: &str = "weather_failures";

/// The weather budget. The Open-Meteo switch itself lives in `online`: weather is one of the
/// things that switch allows, not a separate permission.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub per_day: i64,
}

impl Settings {
    /// How many minutes must pass between two calls at this budget.
    pub fn interval_minutes(&self) -> i64 {
        ((24 * 60) / self.per_day.max(1)).max(MIN_INTERVAL_MINUTES)
    }
}

/// Today's spending against the budget.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Budget {
    pub calls_today: i64,
    /// The last call as a local date and minutes after midnight.
    pub last_call: Option<(Date, i64)>,
    /// Consecutive failures; a successful sync clears it.
    pub failures: i64,
}

pub fn settings(store: &Store) -> Settings {
    let per_day = store
        .setting(PER_DAY)
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_PER_DAY)
        .clamp(1, MAX_PER_DAY);

    Settings { per_day }
}

pub fn set_per_day(store: &Store, per_day: i64) -> crate::store::Result<()> {
    store.set_setting(PER_DAY, &per_day.clamp(1, MAX_PER_DAY).to_string())
}

pub fn budget(store: &Store, today: Date) -> Budget {
    let spent_on = store
        .setting(CALLS_DATE)
        .ok()
        .flatten()
        .and_then(|value| Date::parse_iso(value.trim()));

    let calls_today = if spent_on == Some(today) {
        store
            .setting(CALLS_COUNT)
            .ok()
            .flatten()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(0)
    } else {
        0
    };

    let last_call = store
        .setting(LAST_CALL)
        .ok()
        .flatten()
        .and_then(|value| parse_stamp(value.trim()).map(|(date, _, minutes)| (date, minutes)));

    let failures = store
        .setting(FAILURES)
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);

    Budget {
        calls_today,
        last_call,
        failures,
    }
}

/// Count a call before it is made, so a crash or a failure cannot turn into a burst of requests.
pub fn record_call(store: &Store, today: Date, minutes: i64) -> crate::store::Result<()> {
    let budget = budget(store, today);
    store.set_setting(CALLS_DATE, &today.to_iso())?;
    store.set_setting(CALLS_COUNT, &(budget.calls_today + 1).to_string())?;
    store.set_setting(
        LAST_CALL,
        &format!("{}T{:02}:{:02}", today.to_iso(), minutes / 60, minutes % 60),
    )
}

fn may_call(settings: &Settings, budget: &Budget, online: bool, has_location: bool) -> bool {
    if !online || !has_location {
        return false;
    }

    budget.calls_today < settings.per_day
}

/// The last failed sync, kept so the interface can say why instead of "no data yet".
pub fn last_error(store: &Store) -> Option<String> {
    store
        .setting(LAST_ERROR)
        .ok()
        .flatten()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Remembers the failure, or clears it after a successful sync. A failure also adds to the retry
/// backoff; a success starts over.
pub fn set_last_error(store: &Store, message: Option<&str>) {
    if let Err(error) = store.set_setting(LAST_ERROR, message.unwrap_or("")) {
        eprintln!("could not record the weather error: {error}");
    }

    let failures = match message {
        None => 0,
        Some(_) => store
            .setting(FAILURES)
            .ok()
            .flatten()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(0)
            .saturating_add(1),
    };

    if let Err(error) = store.set_setting(FAILURES, &failures.to_string()) {
        eprintln!("could not record the weather failure count: {error}");
    }
}

/// A call when the app opens: a quick restart should not ask again, but an open after a real gap
/// may. The daily budget remains the hard limit.
pub fn due_on_open(
    settings: &Settings,
    budget: &Budget,
    today: Date,
    now_minutes: i64,
    online: bool,
    has_location: bool,
) -> bool {
    if !may_call(settings, budget, online, has_location) {
        return false;
    }

    match budget.last_call {
        None => true,
        Some((date, minutes)) => {
            // Compare across midnight: a call at 23:59 still blocks a poll at 00:05.
            let elapsed = (date.days_until(today) * 1_440 + now_minutes - minutes).max(0);
            elapsed >= MIN_INTERVAL_MINUTES
        }
    }
}

/// A deliberate location change may ask at once: the daily budget still holds, but the interval
/// between calls protects the old place's rhythm, not the new place's first data.
pub fn due_location_change(
    settings: &Settings,
    budget: &Budget,
    online: bool,
    has_location: bool,
) -> bool {
    may_call(settings, budget, online, has_location)
}

/// A call while the app stays open: spread over the day by the budget. After a failure the next
/// attempt comes sooner, with a small backoff, so one bad minute does not hide the weather for the
/// rest of the interval. The daily budget remains the hard limit.
pub fn due_poll(
    settings: &Settings,
    budget: &Budget,
    today: Date,
    now_minutes: i64,
    online: bool,
    has_location: bool,
) -> bool {
    if !may_call(settings, budget, online, has_location) {
        return false;
    }

    match budget.last_call {
        None => true,
        Some((date, minutes)) => {
            let elapsed = (date.days_until(today) * 1_440 + now_minutes - minutes).max(0);
            elapsed >= retry_minutes(settings, budget.failures)
        }
    }
}

/// The normal budget interval, or a shorter one while the last calls failed: 15, 30, 60 minutes,
/// then the normal interval. A high budget (a short normal interval) is never stretched by this.
fn retry_minutes(settings: &Settings, failures: i64) -> i64 {
    let normal = settings.interval_minutes();
    if failures <= 0 || failures >= 4 {
        return normal;
    }

    let backoff = MIN_INTERVAL_MINUTES * (1 << (failures.saturating_sub(1).min(3)));
    backoff.clamp(MIN_INTERVAL_MINUTES, normal)
}

/// One model snapshot of the current air at the location.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherCurrent {
    pub latitude: f64,
    pub longitude: f64,
    pub observed_on: Date,
    pub observed_time: Option<String>,
    pub temperature_c: Option<f64>,
    pub humidity_pct: Option<f64>,
    pub apparent_temperature_c: Option<f64>,
    pub weather_code: Option<i64>,
    pub precipitation_mm: Option<f64>,
    pub cloud_cover_pct: Option<f64>,
    pub wind_speed_kmh: Option<f64>,
    pub wind_gust_kmh: Option<f64>,
    pub is_day: Option<bool>,
}

/// One day of model data, including the night that ended that morning.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherDay {
    pub latitude: f64,
    pub longitude: f64,
    pub date: Date,
    pub temp_min_c: Option<f64>,
    pub temp_max_c: Option<f64>,
    pub temp_mean_c: Option<f64>,
    pub humidity_mean_pct: Option<f64>,
    pub humidity_min_pct: Option<f64>,
    pub humidity_max_pct: Option<f64>,
    pub precipitation_mm: Option<f64>,
    pub precipitation_hours: Option<f64>,
    pub weather_code: Option<i64>,
    pub sunshine_hours: Option<f64>,
    pub daylight_hours: Option<f64>,
    pub radiation_sum_mj_m2: Option<f64>,
    pub wind_max_kmh: Option<f64>,
    pub wind_gust_max_kmh: Option<f64>,
    pub et0_mm: Option<f64>,
    /// Local sun times for this day, `HH:MM`.
    pub sunrise: Option<String>,
    pub sunset: Option<String>,
    /// The night that ended on `date`: first and last dark hour, local time.
    pub night_start: Option<String>,
    pub night_end: Option<String>,
    pub night_temp_avg_c: Option<f64>,
    pub night_temp_min_c: Option<f64>,
    pub night_humidity_avg_pct: Option<f64>,
    pub night_hours: Option<f64>,
    /// Whether this response computed a complete night. False when the hourly block was missing,
    /// the night was unfinished, or the samples had a gap; the stored night is then preserved.
    pub night_complete: bool,
}

/// One hourly model value for the location. The graph uses this for a 24-hour or 7-day view.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherHour {
    pub latitude: f64,
    pub longitude: f64,
    pub date: Date,
    /// Local `HH:MM`.
    pub time: String,
    pub minutes: i64,
    pub temperature_c: Option<f64>,
    pub humidity_pct: Option<f64>,
    pub precipitation_mm: Option<f64>,
    pub weather_code: Option<i64>,
    pub cloud_cover_pct: Option<f64>,
    pub sunshine_hours: Option<f64>,
    pub is_day: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeatherData {
    pub current: WeatherCurrent,
    pub days: Vec<WeatherDay>,
    pub hours: Vec<WeatherHour>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApplyReport {
    pub current_written: bool,
    pub days_written: usize,
    pub hours_written: usize,
}

/// Fetch the current values plus the last `HISTORY_DAYS` days at the location.
pub fn fetch(location: &Location) -> Result<WeatherData, String> {
    let url = format!(
        "https://historical-forecast-api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}\
         &current=temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,\
         weather_code,cloud_cover,wind_speed_10m,wind_gusts_10m\
         &hourly=temperature_2m,relative_humidity_2m,precipitation,weather_code,cloud_cover,\
         sunshine_duration,is_day\
         &daily=temperature_2m_max,temperature_2m_min,temperature_2m_mean,\
         relative_humidity_2m_mean,relative_humidity_2m_min,relative_humidity_2m_max,\
         precipitation_sum,precipitation_hours,weather_code,sunshine_duration,daylight_duration,\
         sunrise,sunset,shortwave_radiation_sum,wind_speed_10m_max,wind_gusts_10m_max,\
         et0_fao_evapotranspiration\
         &past_days={HISTORY_DAYS}&forecast_days=1&timezone=auto",
        location.latitude, location.longitude
    );

    let body = get(&url)?;
    parse(&body, location)
}

fn parse(body: &str, location: &Location) -> Result<WeatherData, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("could not read the weather: {error}"))?;

    let current = parse_current(&value, location)?;
    let days = parse_days(&value, location, &current)?;
    let hours = parse_weather_hours(&value, location);

    Ok(WeatherData {
        current,
        days,
        hours,
    })
}

fn parse_current(value: &serde_json::Value, location: &Location) -> Result<WeatherCurrent, String> {
    let current = value
        .get("current")
        .ok_or("the weather answer had no current values")?;

    let stamp = current
        .get("time")
        .and_then(serde_json::Value::as_str)
        .ok_or("the weather answer had no time")?;
    let (observed_on, observed_time, _) =
        parse_stamp(stamp).ok_or("the weather answer had an unexpected time")?;

    let temperature_c = current
        .get("temperature_2m")
        .and_then(serde_json::Value::as_f64);
    let humidity_pct = current
        .get("relative_humidity_2m")
        .and_then(serde_json::Value::as_f64);

    if temperature_c.is_none() && humidity_pct.is_none() {
        return Err("the weather answer had no temperature and no humidity".to_string());
    }

    Ok(WeatherCurrent {
        latitude: location.latitude,
        longitude: location.longitude,
        observed_on,
        observed_time: Some(observed_time),
        temperature_c,
        humidity_pct,
        apparent_temperature_c: current
            .get("apparent_temperature")
            .and_then(serde_json::Value::as_f64),
        weather_code: current
            .get("weather_code")
            .and_then(serde_json::Value::as_i64),
        precipitation_mm: current
            .get("precipitation")
            .and_then(serde_json::Value::as_f64),
        cloud_cover_pct: current
            .get("cloud_cover")
            .and_then(serde_json::Value::as_f64),
        wind_speed_kmh: current
            .get("wind_speed_10m")
            .and_then(serde_json::Value::as_f64),
        wind_gust_kmh: current
            .get("wind_gusts_10m")
            .and_then(serde_json::Value::as_f64),
        is_day: current
            .get("is_day")
            .and_then(serde_json::Value::as_i64)
            .map(|value| value == 1),
    })
}

#[derive(Clone, Debug)]
struct Hour {
    date: Date,
    time: String,
    minutes: i64,
    temperature_c: Option<f64>,
    humidity_pct: Option<f64>,
    precipitation_mm: Option<f64>,
    weather_code: Option<i64>,
    cloud_cover_pct: Option<f64>,
    sunshine_hours: Option<f64>,
    is_day: Option<bool>,
}

fn parse_days(
    value: &serde_json::Value,
    location: &Location,
    current: &WeatherCurrent,
) -> Result<Vec<WeatherDay>, String> {
    let daily = value
        .get("daily")
        .ok_or("the weather answer had no daily values")?;
    let Some(times) = daily.get("time").and_then(serde_json::Value::as_array) else {
        return Ok(Vec::new());
    };

    let hourly = parse_hours(value);
    let hourly_start = hourly.first().map(|hour| (hour.date, hour.minutes));
    let current_minutes = current
        .observed_time
        .as_deref()
        .and_then(|time| parse_stamp(&format!("{}T{time}", current.observed_on.to_iso())))
        .map(|(_, _, minutes)| minutes)
        .unwrap_or(0);

    let mut nights: BTreeMap<Date, Vec<&Hour>> = BTreeMap::new();
    for hour in hourly.iter().filter(|hour| hour.is_day == Some(false)) {
        nights
            .entry(night_date(hour.date, hour.minutes))
            .or_default()
            .push(hour);
    }

    let mut days = Vec::new();

    for (index, time) in times.iter().enumerate() {
        let Some(date) = time.as_str().and_then(Date::parse_iso) else {
            continue;
        };

        let daily_code = int_at(daily, "weather_code", index);
        let weather_code = dominant_code(&hourly, date).or(daily_code);

        let mut day = WeatherDay {
            latitude: location.latitude,
            longitude: location.longitude,
            date,
            temp_min_c: number_at(daily, "temperature_2m_min", index),
            temp_max_c: number_at(daily, "temperature_2m_max", index),
            temp_mean_c: number_at(daily, "temperature_2m_mean", index),
            humidity_mean_pct: number_at(daily, "relative_humidity_2m_mean", index),
            humidity_min_pct: number_at(daily, "relative_humidity_2m_min", index),
            humidity_max_pct: number_at(daily, "relative_humidity_2m_max", index),
            precipitation_mm: number_at(daily, "precipitation_sum", index),
            precipitation_hours: number_at(daily, "precipitation_hours", index),
            weather_code,
            sunshine_hours: number_at(daily, "sunshine_duration", index).map(seconds_to_hours),
            daylight_hours: number_at(daily, "daylight_duration", index).map(seconds_to_hours),
            radiation_sum_mj_m2: number_at(daily, "shortwave_radiation_sum", index),
            wind_max_kmh: number_at(daily, "wind_speed_10m_max", index),
            wind_gust_max_kmh: number_at(daily, "wind_gusts_10m_max", index),
            et0_mm: number_at(daily, "et0_fao_evapotranspiration", index),
            sunrise: time_at(daily, "sunrise", index),
            sunset: time_at(daily, "sunset", index),
            night_start: None,
            night_end: None,
            night_temp_avg_c: None,
            night_temp_min_c: None,
            night_humidity_avg_pct: None,
            night_hours: None,
            night_complete: false,
        };

        if let Some(samples) = nights.get(&date)
            && night_is_complete(date, current, current_minutes, hourly_start)
        {
            day.night_complete = apply_night(&mut day, samples);
        }

        days.push(day);
    }

    Ok(days)
}

fn parse_hours(value: &serde_json::Value) -> Vec<Hour> {
    let Some(hourly) = value.get("hourly") else {
        return Vec::new();
    };
    let Some(times) = hourly.get("time").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };

    let mut hours = Vec::new();
    for (index, stamp) in times.iter().enumerate() {
        let Some(stamp) = stamp.as_str() else {
            continue;
        };
        let Some((date, time, minutes)) = parse_stamp(stamp) else {
            continue;
        };

        hours.push(Hour {
            date,
            time,
            minutes,
            temperature_c: number_at(hourly, "temperature_2m", index),
            humidity_pct: number_at(hourly, "relative_humidity_2m", index),
            precipitation_mm: number_at(hourly, "precipitation", index),
            weather_code: int_at(hourly, "weather_code", index),
            cloud_cover_pct: number_at(hourly, "cloud_cover", index),
            sunshine_hours: number_at(hourly, "sunshine_duration", index).map(seconds_to_hours),
            is_day: int_at(hourly, "is_day", index).map(|value| value == 1),
        });
    }

    hours
}

fn parse_weather_hours(root: &serde_json::Value, location: &Location) -> Vec<WeatherHour> {
    parse_hours(root)
        .into_iter()
        .map(|hour| WeatherHour {
            latitude: location.latitude,
            longitude: location.longitude,
            date: hour.date,
            time: hour.time,
            minutes: hour.minutes,
            temperature_c: hour.temperature_c,
            humidity_pct: hour.humidity_pct,
            precipitation_mm: hour.precipitation_mm,
            weather_code: hour.weather_code,
            cloud_cover_pct: hour.cloud_cover_pct,
            sunshine_hours: hour.sunshine_hours,
            is_day: hour.is_day,
        })
        .collect()
}

/// A daily sunrise or sunset as local `HH:MM`. Open-Meteo returns an ISO timestamp; a bare time
/// is accepted too so an archived answer or a test can omit the date.
fn time_at(section: &serde_json::Value, key: &str, index: usize) -> Option<String> {
    let value = section
        .get(key)
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.get(index))
        .and_then(serde_json::Value::as_str)?;

    if let Some((_, time, _)) = parse_stamp(value) {
        return Some(time);
    }

    let time = value.get(0..5)?;
    let (hours, minutes) = time.split_once(':')?;
    let hours: i64 = hours.parse().ok()?;
    let minutes: i64 = minutes.parse().ok()?;
    (hours < 24 && minutes < 60).then(|| time.to_string())
}

fn night_date(date: Date, minutes: i64) -> Date {
    if minutes < 12 * 60 {
        date
    } else {
        date.add_days(1)
    }
}

fn night_is_complete(
    date: Date,
    current: &WeatherCurrent,
    current_minutes: i64,
    hourly_start: Option<(Date, i64)>,
) -> bool {
    let Some(start) = hourly_start else {
        return false;
    };

    // The hourly window must include the previous evening; otherwise the evening half of
    // this night was never in the answer (the oldest backfilled day).
    if start > (date.add_days(-1), 12 * 60) {
        return false;
    }

    if date < current.observed_on {
        true
    } else if date == current.observed_on {
        current.is_day == Some(true) || current_minutes >= 12 * 60
    } else {
        false
    }
}

fn apply_night(day: &mut WeatherDay, samples: &[&Hour]) -> bool {
    if samples.len() < 2 {
        return false;
    }

    // The night must end on the morning it belongs to. An evening-only group is incomplete; a
    // high-latitude summer night may legitimately start after midnight.
    if !samples.last().is_some_and(|hour| hour.date == day.date) {
        return false;
    }
    for pair in samples.windows(2) {
        let gap =
            pair[0].date.days_until(pair[1].date) * 24 * 60 + pair[1].minutes - pair[0].minutes;
        if gap != 60 {
            return false;
        }
    }

    let temperatures: Vec<f64> = samples
        .iter()
        .filter_map(|hour| hour.temperature_c)
        .collect();
    let humidities: Vec<f64> = samples
        .iter()
        .filter_map(|hour| hour.humidity_pct)
        .collect();

    if temperatures.is_empty() && humidities.is_empty() {
        return false;
    }

    let Some(first) = samples.first() else {
        return false;
    };
    let Some(last) = samples.last() else {
        return false;
    };

    day.night_start = Some(format!("{}T{}", first.date.to_iso(), first.time));
    day.night_end = Some(format!("{}T{}", last.date.to_iso(), last.time));
    day.night_hours = Some(samples.len() as f64);
    day.night_temp_avg_c = average(&temperatures);
    day.night_temp_min_c = temperatures.iter().copied().reduce(f64::min);
    day.night_humidity_avg_pct = average(&humidities);

    true
}

fn dominant_code(hours: &[Hour], date: Date) -> Option<i64> {
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for hour in hours
        .iter()
        .filter(|hour| hour.date == date && hour.is_day == Some(true))
    {
        if let Some(code) = hour.weather_code {
            *counts.entry(code).or_default() += 1;
        }
    }

    counts
        .into_iter()
        .max_by(|(left_code, left_count), (right_code, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_code.cmp(left_code))
        })
        .map(|(code, _)| code)
}

fn average(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn seconds_to_hours(seconds: f64) -> f64 {
    seconds / 3600.0
}

fn number_at(section: &serde_json::Value, key: &str, index: usize) -> Option<f64> {
    section
        .get(key)
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.get(index))
        .and_then(serde_json::Value::as_f64)
}

fn int_at(section: &serde_json::Value, key: &str, index: usize) -> Option<i64> {
    section
        .get(key)
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.get(index))
        .and_then(serde_json::Value::as_i64)
}

/// Write one sync: the current snapshot plus every daily row it returned.
pub fn apply(store: &Store, data: &WeatherData) -> Result<ApplyReport, String> {
    store
        .transaction(|store| {
            let current_written = store.insert_weather_current(&data.current)?;

            let mut days_written = 0;
            for day in &data.days {
                store.upsert_weather_day(day)?;
                days_written += 1;
            }

            // The provider returns the whole of today, including hours that have not happened yet.
            // Those are a forecast; storing them would let a graph draw the future as if it were
            // measured, so the sync writes only the hours that have passed.
            let now_minutes = data
                .current
                .observed_time
                .as_deref()
                .and_then(|time| {
                    parse_stamp(&format!("{}T{time}", data.current.observed_on.to_iso()))
                })
                .map(|(_, _, minutes)| minutes);

            let mut hours_written = 0;
            for hour in &data.hours {
                if hour.date == data.current.observed_on
                    && now_minutes.is_some_and(|now| hour.minutes > now)
                {
                    continue;
                }
                store.upsert_weather_hour(hour)?;
                hours_written += 1;
            }

            Ok(ApplyReport {
                current_written,
                days_written,
                hours_written,
            })
        })
        .map_err(|error| format!("could not store the weather: {error}"))
}

/// The WMO weather code as a short label for the interface.
pub fn condition_label(code: Option<i64>) -> &'static str {
    match code {
        Some(0) => "Clear sky",
        Some(1) => "Mainly clear",
        Some(2) => "Partly cloudy",
        Some(3) => "Overcast",
        Some(45 | 48) => "Fog",
        Some(51 | 53 | 55) => "Drizzle",
        Some(56 | 57) => "Freezing drizzle",
        Some(61 | 63 | 65) => "Rain",
        Some(66 | 67) => "Freezing rain",
        Some(71 | 73 | 75 | 77) => "Snow",
        Some(80..=82) => "Rain showers",
        Some(85 | 86) => "Snow showers",
        Some(95 | 96 | 97 | 99) => "Thunderstorm",
        _ => "Weather",
    }
}

/// A symbolic icon for the weather code, from the Adwaita icon set.
pub fn condition_icon(code: Option<i64>) -> &'static str {
    match code {
        Some(0) => "weather-clear-symbolic",
        Some(1 | 2) => "weather-few-clouds-symbolic",
        Some(3) => "weather-overcast-symbolic",
        Some(45 | 48) => "weather-fog-symbolic",
        Some(51 | 53 | 55 | 56 | 57 | 61 | 63 | 65 | 66 | 67 | 80 | 81 | 82) => {
            "weather-showers-symbolic"
        }
        Some(71 | 73 | 75 | 77 | 85 | 86) => "weather-snow-symbolic",
        Some(95 | 96 | 97 | 99) => "weather-storm-symbolic",
        _ => "weather-few-clouds-symbolic",
    }
}

fn get(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        // The historical endpoint computes 93 days of hourly data; a slow moment can take a while.
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(concat!("omagrow/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();

    let mut response = agent.get(url).call().map_err(|error| match error {
        ureq::Error::StatusCode(code) => format!("Open-Meteo answered with HTTP {code}"),
        ureq::Error::Timeout(_) => "Open-Meteo did not answer in time".to_string(),
        other => format!("could not reach Open-Meteo: {other}"),
    })?;

    response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("could not read the weather answer: {error}"))
}

/// Parse an ISO timestamp into a local date, `HH:MM` and minutes after midnight.
fn parse_stamp(value: &str) -> Option<(Date, String, i64)> {
    let (date, time) = value.split_once('T')?;
    let date = Date::parse_iso(date)?;
    let time = time.get(0..5)?.to_string();
    let (hours, minutes) = time.split_once(':')?;
    let hours: i64 = hours.parse().ok()?;
    let minutes: i64 = minutes.parse().ok()?;

    Some((date, time, hours * 60 + minutes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings { per_day: 12 }
    }

    fn day() -> Date {
        Date::new(2026, 10, 31).unwrap()
    }

    fn location() -> Location {
        Location {
            latitude: 51.2089,
            longitude: 3.2242,
            name: Some("Bruges".to_string()),
            region: None,
            country: Some("Belgium".to_string()),
            timezone: Some("Europe/Brussels".to_string()),
        }
    }

    #[test]
    fn the_interval_follows_the_budget_and_keeps_a_floor() {
        assert_eq!(settings().interval_minutes(), 120);
        assert_eq!(
            Settings { per_day: 96 }.interval_minutes(),
            MIN_INTERVAL_MINUTES
        );
    }

    #[test]
    fn no_weather_calls_while_open_meteo_is_off() {
        let budget = Budget::default();

        assert!(!due_on_open(&settings(), &budget, day(), 0, false, true));
        assert!(!due_poll(&settings(), &budget, day(), 0, false, true));
        assert!(due_on_open(&settings(), &budget, day(), 0, true, true));
    }

    #[test]
    fn a_location_change_may_fetch_before_the_interval() {
        let recent = Budget {
            calls_today: 1,
            last_call: Some((day(), 12 * 60)),
            failures: 0,
        };

        assert!(!due_poll(
            &settings(),
            &recent,
            day(),
            12 * 60 + 1,
            true,
            true
        ));
        assert!(due_location_change(&settings(), &recent, true, true));

        let spent = Budget {
            calls_today: 12,
            ..recent
        };
        assert!(!due_location_change(&settings(), &spent, true, true));
        assert!(!due_location_change(
            &settings(),
            &Budget::default(),
            false,
            true
        ));
    }

    #[test]
    fn the_old_weather_switch_key_is_ignored() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("weather_enabled", "off").unwrap();

        assert_eq!(
            super::settings(&store),
            Settings {
                per_day: DEFAULT_PER_DAY
            }
        );
    }

    #[test]
    fn opening_and_polling_use_different_freshness_rules() {
        let now = 12 * 60;
        let budget = Budget {
            calls_today: 1,
            last_call: Some((day(), 11 * 60 + 30)),
            failures: 0,
        };

        // Half an hour later is too soon for a poll, but a reopen may ask again.
        assert!(!due_poll(&settings(), &budget, day(), now, true, true));
        assert!(due_on_open(&settings(), &budget, day(), now, true, true));

        let very_recent = Budget {
            calls_today: 1,
            last_call: Some((day(), 11 * 60 + 55)),
            failures: 0,
        };
        assert!(!due_on_open(
            &settings(),
            &very_recent,
            day(),
            now,
            true,
            true
        ));
    }

    #[test]
    fn the_budget_and_the_interval_hold() {
        let spent = Budget {
            calls_today: 12,
            last_call: Some((day(), 6 * 60)),
            failures: 0,
        };
        assert!(!due_poll(&settings(), &spent, day(), 12 * 60, true, true));

        let old = Budget {
            calls_today: 1,
            last_call: Some((day(), 5 * 60)),
            failures: 0,
        };
        assert!(due_poll(&settings(), &old, day(), 12 * 60, true, true));
        assert!(due_poll(&settings(), &old, day(), 12 * 60, true, true));
    }

    #[test]
    fn yesterday_does_not_block_today() {
        let yesterday = Budget {
            calls_today: 0,
            last_call: Some((Date::new(2026, 10, 30).unwrap(), 20 * 60)),
            failures: 0,
        };
        assert!(due_on_open(&settings(), &yesterday, day(), 60, true, true));
        assert!(due_poll(&settings(), &yesterday, day(), 60, true, true));
    }

    #[test]
    fn the_minimum_interval_holds_across_midnight() {
        let minutes_before = Budget {
            calls_today: 0,
            last_call: Some((Date::new(2026, 10, 30).unwrap(), 23 * 60 + 59)),
            failures: 0,
        };
        assert!(!due_on_open(
            &settings(),
            &minutes_before,
            day(),
            5,
            true,
            true
        ));
        assert!(!due_poll(
            &settings(),
            &minutes_before,
            day(),
            5,
            true,
            true
        ));

        let hours_before = Budget {
            calls_today: 0,
            last_call: Some((Date::new(2026, 10, 30).unwrap(), 20 * 60)),
            failures: 0,
        };
        assert!(due_on_open(
            &settings(),
            &hours_before,
            day(),
            5,
            true,
            true
        ));
    }

    #[test]
    fn a_failed_sync_retries_before_the_full_interval() {
        let failed = Budget {
            calls_today: 2,
            last_call: Some((day(), 12 * 60)),
            failures: 1,
        };
        assert!(!due_poll(
            &settings(),
            &failed,
            day(),
            12 * 60 + 14,
            true,
            true
        ));
        assert!(due_poll(
            &settings(),
            &failed,
            day(),
            12 * 60 + 15,
            true,
            true
        ));

        let second = Budget {
            failures: 2,
            ..failed.clone()
        };
        assert!(!due_poll(
            &settings(),
            &second,
            day(),
            12 * 60 + 15,
            true,
            true
        ));
        assert!(due_poll(
            &settings(),
            &second,
            day(),
            12 * 60 + 30,
            true,
            true
        ));

        let third = Budget {
            failures: 3,
            ..failed.clone()
        };
        assert!(!due_poll(
            &settings(),
            &third,
            day(),
            12 * 60 + 59,
            true,
            true
        ));
        assert!(due_poll(&settings(), &third, day(), 13 * 60, true, true));

        // After four failures the normal budget interval returns; it is not stretched.
        let daily = Settings { per_day: 1 };
        let many = Budget {
            calls_today: 0,
            last_call: Some((day(), 8 * 60)),
            failures: 4,
        };
        assert!(!due_poll(
            &daily,
            &many,
            day().add_days(1),
            7 * 60 + 59,
            true,
            true
        ));
        assert!(due_poll(
            &daily,
            &many,
            day().add_days(1),
            8 * 60,
            true,
            true
        ));
    }

    #[test]
    fn a_failure_count_rises_and_a_success_clears_it() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(budget(&store, day()).failures, 0);

        set_last_error(&store, Some("timeout"));
        set_last_error(&store, Some("timeout again"));
        assert_eq!(budget(&store, day()).failures, 2);

        set_last_error(&store, None);
        assert_eq!(budget(&store, day()).failures, 0);
        assert!(last_error(&store).is_none());
    }

    #[test]
    fn a_sync_does_not_store_hours_that_have_not_happened() {
        let store = Store::open_in_memory().unwrap();
        let data = parse(&answer(), &location()).unwrap();
        apply(&store, &data).unwrap();

        let today = Date::new(2026, 10, 31).unwrap();
        let hours = store.weather_hours(51.2089, 3.2242, today, today).unwrap();
        assert!(hours.iter().all(|hour| hour.minutes < 10 * 60));
        assert_eq!(hours.len(), 2); // 00:00 and 01:00; 10:00 is still ahead at 09:15
    }

    #[test]
    fn the_daily_count_starts_again_on_a_new_day() {
        let store = Store::open_in_memory().unwrap();
        let today = day();
        let yesterday = Date::new(2026, 10, 30).unwrap();

        store
            .set_setting("weather_calls_date", &yesterday.to_iso())
            .unwrap();
        store.set_setting("weather_calls_count", "12").unwrap();
        store
            .set_setting("weather_last_call", "2026-10-30T20:00")
            .unwrap();

        let budget = budget(&store, today);
        assert_eq!(budget.calls_today, 0);
        assert_eq!(budget.last_call, Some((yesterday, 20 * 60)));
    }

    #[test]
    fn counting_a_call_updates_the_day_the_count_and_the_time() {
        let store = Store::open_in_memory().unwrap();
        let today = day();

        record_call(&store, today, 9 * 60 + 30).unwrap();
        record_call(&store, today, 10 * 60).unwrap();

        let budget = budget(&store, today);
        assert_eq!(budget.calls_today, 2);
        assert_eq!(budget.last_call, Some((today, 10 * 60)));
    }

    fn answer() -> String {
        r#"{
            "current": {
                "time": "2026-10-31T09:15",
                "temperature_2m": 12.4,
                "relative_humidity_2m": 87,
                "apparent_temperature": 11.0,
                "is_day": 1,
                "precipitation": 0.2,
                "weather_code": 2,
                "cloud_cover": 60,
                "wind_speed_10m": 12.0,
                "wind_gusts_10m": 24.0
            },
            "hourly": {
                "time": [
                    "2026-10-30T12:00", "2026-10-30T22:00", "2026-10-30T23:00",
                    "2026-10-31T00:00", "2026-10-31T01:00", "2026-10-31T10:00"
                ],
                "temperature_2m": [15.0, 10.0, 9.5, 9.0, 8.5, 14.0],
                "relative_humidity_2m": [70, 80, 82, 84, 86, 65],
                "precipitation": [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                "weather_code": [2, 2, 1, 2, 1, 3],
                "cloud_cover": [50, 50, 50, 40, 30, 70],
                "sunshine_duration": [3600.0, 0.0, 0.0, 0.0, 0.0, 1800.0],
                "is_day": [1, 0, 0, 0, 0, 1]
            },
            "daily": {
                "time": ["2026-10-31"],
                "temperature_2m_max": [14.5],
                "temperature_2m_min": [7.9],
                "temperature_2m_mean": [11.2],
                "relative_humidity_2m_mean": [78],
                "relative_humidity_2m_min": [65],
                "relative_humidity_2m_max": [85],
                "precipitation_sum": [0.2],
                "precipitation_hours": [1.0],
                "weather_code": [61],
                "sunshine_duration": [7200.0],
                "daylight_duration": [36000.0],
                "sunrise": ["2026-10-31T07:35"],
                "sunset": ["2026-10-31T17:20"],
                "shortwave_radiation_sum": [6.5],
                "wind_speed_10m_max": [18.0],
                "wind_gusts_10m_max": [30.0],
                "et0_fao_evapotranspiration": [1.2]
            }
        }"#
        .to_string()
    }

    #[test]
    fn reads_current_daily_and_night_values() {
        let data = parse(&answer(), &location()).unwrap();
        assert_eq!(data.current.temperature_c, Some(12.4));
        assert_eq!(data.current.weather_code, Some(2));
        assert_eq!(data.current.is_day, Some(true));
        assert_eq!(data.days.len(), 1);

        let day = &data.days[0];
        assert_eq!(day.weather_code, Some(3)); // dominant daytime code, not the daily severity
        assert_eq!(day.sunshine_hours, Some(2.0));
        assert_eq!(day.daylight_hours, Some(10.0));
        assert_eq!(day.sunrise.as_deref(), Some("07:35"));
        assert_eq!(day.sunset.as_deref(), Some("17:20"));
        assert_eq!(data.hours.len(), 6);
        assert_eq!(data.hours[0].time, "12:00");
        assert_eq!(data.hours[0].precipitation_mm, Some(0.0));
        assert_eq!(data.hours[0].sunshine_hours, Some(1.0));
        assert_eq!(day.night_temp_avg_c, Some(9.25));
        assert_eq!(day.night_temp_min_c, Some(8.5));
        assert_eq!(day.night_hours, Some(4.0));
        assert_eq!(day.night_start.as_deref(), Some("2026-10-30T22:00"));
        assert_eq!(day.night_end.as_deref(), Some("2026-10-31T01:00"));
    }

    #[test]
    fn a_gapped_night_is_not_stored() {
        let mut value: serde_json::Value = serde_json::from_str(&answer()).unwrap();
        let hourly = value.get_mut("hourly").unwrap().as_object_mut().unwrap();
        for key in [
            "time",
            "temperature_2m",
            "relative_humidity_2m",
            "precipitation",
            "weather_code",
            "cloud_cover",
            "is_day",
        ] {
            hourly
                .get_mut(key)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .remove(2);
        }

        let data = parse(&value.to_string(), &location()).unwrap();
        assert!(data.days[0].night_temp_avg_c.is_none());
        assert!(!data.days[0].night_complete);
    }

    #[test]
    fn an_unfinished_night_is_not_stored() {
        let body = answer()
            .replace(
                "\"time\": \"2026-10-31T09:15\"",
                "\"time\": \"2026-10-31T02:30\"",
            )
            .replace("\"is_day\": 1", "\"is_day\": 0");
        let data = parse(&body, &location()).unwrap();
        assert!(data.days[0].night_temp_avg_c.is_none());
        assert!(data.days[0].night_end.is_none());
    }

    #[test]
    fn a_summer_night_may_start_after_midnight() {
        // A high-latitude summer night can be dark only after midnight. The hourly window still
        // covers the previous evening, so the night is complete.
        let body = answer().replace(
            "\"is_day\": [1, 0, 0, 0, 0, 1]",
            "\"is_day\": [1, 1, 1, 0, 0, 1]",
        );
        let data = parse(&body, &location()).unwrap();
        let day = &data.days[0];
        assert!(day.night_complete);
        assert_eq!(day.night_hours, Some(2.0));
        assert_eq!(day.night_temp_avg_c, Some(8.75));
    }

    #[test]
    fn an_answer_without_values_is_refused() {
        assert!(
            parse(
                r#"{"current":{"time":"2026-10-31T09:15"},"daily":{"time":[]}}"#,
                &location()
            )
            .is_err()
        );
        assert!(parse("not json", &location()).is_err());
    }

    #[test]
    fn maps_weather_codes_to_labels_and_icons() {
        assert_eq!(condition_label(Some(0)), "Clear sky");
        assert_eq!(condition_label(Some(61)), "Rain");
        assert_eq!(condition_label(Some(95)), "Thunderstorm");
        assert_eq!(condition_icon(Some(0)), "weather-clear-symbolic");
        assert_eq!(condition_icon(Some(61)), "weather-showers-symbolic");
    }
}
