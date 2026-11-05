//! Frost dates estimated from historical daily minima.
//!
//! Open-Meteo's archive serves ERA5-Land, a reanalysis on a grid of roughly 9 km. For the
//! 1991–2020 period we take the last freezing day before the warm season and the first one after
//! it, per year, and use the 9-in-10 date: in nine years out of ten the last spring frost is not
//! later than the estimate, and the first autumn frost is not earlier. That is a defensible
//! garden rule, not a measurement in the user's garden: a valley, a city centre and a balcony
//! differ from the grid cell. The year is split at 1 July, which assumes a winter around the turn
//! of the calendar year; a place without frost simply yields no date for that season.
//!
//! The result is a suggestion. The user can edit both dates and the source line disappears when
//! they do.

use std::collections::BTreeSet;
use std::time::Duration;

use crate::calendar::Date;

pub const FIRST_YEAR: i32 = 1991;
pub const LAST_YEAR: i32 = 2020;
/// Air temperature at 2 m; ground frost can happen with a warmer air temperature.
pub const FREEZING_C: f64 = 0.0;

/// A year is only used when both frost seasons have enough days; this skips a broken answer.
const MIN_SEASON_DAYS: usize = 150;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Estimate {
    pub last: Option<(u32, u32)>,
    pub first: Option<(u32, u32)>,
    /// Complete years in the archive answer.
    pub years: usize,
}

impl Estimate {
    pub fn source(&self) -> String {
        let dates = match (self.last, self.first) {
            (Some(_), Some(_)) => "last spring and first autumn frost",
            (Some(_), None) => "last spring frost only",
            (None, Some(_)) => "first autumn frost only",
            (None, None) => "no frost found",
        };

        format!("Open-Meteo ERA5-Land {FIRST_YEAR}–{LAST_YEAR} · {dates} · 9-in-10 rule")
    }
}

/// Fetches the archive and estimates both dates for a coordinate.
pub fn lookup(latitude: f64, longitude: f64) -> Result<Estimate, String> {
    let url = format!(
        "https://archive-api.open-meteo.com/v1/archive?latitude={latitude:.4}&longitude={longitude:.4}\
         &start_date={FIRST_YEAR}-01-01&end_date={LAST_YEAR}-12-31\
         &daily=temperature_2m_min&models=era5_land&timezone=auto"
    );

    let body = get(&url)?;
    let days = parse(&body)?;
    let estimate = estimate(&days, latitude >= 0.0);

    if estimate.years == 0 {
        return Err("the archive had no complete frost season for this place".to_string());
    }

    Ok(estimate)
}

fn get(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
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
        .map_err(|error| format!("could not read the Open-Meteo answer: {error}"))
}

#[derive(Clone, Copy, Debug)]
struct Day {
    year: i32,
    month: u32,
    day: u32,
    min: Option<f64>,
}

fn parse(body: &str) -> Result<Vec<Day>, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| "Open-Meteo returned an unexpected answer".to_string())?;

    let daily = value
        .get("daily")
        .ok_or_else(|| "the Open-Meteo answer has no daily data".to_string())?;

    let times = daily
        .get("time")
        .and_then(|times| times.as_array())
        .ok_or_else(|| "the Open-Meteo answer has no dates".to_string())?;

    let minima = daily
        .get("temperature_2m_min")
        .and_then(|minima| minima.as_array())
        .ok_or_else(|| "the Open-Meteo answer has no minimum temperatures".to_string())?;

    if times.len() != minima.len() {
        return Err("the Open-Meteo answer has mismatched dates and temperatures".to_string());
    }

    let mut days = Vec::with_capacity(times.len());

    for (time, min) in times.iter().zip(minima) {
        let time = time
            .as_str()
            .ok_or_else(|| "the Open-Meteo answer has an unreadable date".to_string())?;

        let mut day = parse_date(time)?;
        day.min = min.as_f64();
        days.push(day);
    }

    Ok(days)
}

fn parse_date(text: &str) -> Result<Day, String> {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3 {
        return Err(format!("unexpected date in the archive answer: {text}"));
    }

    let year: i32 = parts[0]
        .parse()
        .map_err(|_| format!("unexpected year in the archive answer: {text}"))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| format!("unexpected month in the archive answer: {text}"))?;
    let day: u32 = parts[2]
        .parse()
        .map_err(|_| format!("unexpected day in the archive answer: {text}"))?;

    Date::new(year, month, day)
        .ok_or_else(|| format!("impossible date in the archive answer: {text}"))?;

    Ok(Day {
        year,
        month,
        day,
        min: None,
    })
}

fn estimate(days: &[Day], northern: bool) -> Estimate {
    let years: BTreeSet<i32> = days.iter().map(|day| day.year).collect();
    let mut last_spring = Vec::new();
    let mut first_autumn = Vec::new();
    let mut complete_years = 0;

    for year in years {
        let spring: Vec<&Day> = days
            .iter()
            .filter(|day| day.year == year && in_spring(day.month, northern))
            .collect();
        let autumn: Vec<&Day> = days
            .iter()
            .filter(|day| day.year == year && !in_spring(day.month, northern))
            .collect();

        let spring_days = spring.iter().filter(|day| day.min.is_some()).count();
        let autumn_days = autumn.iter().filter(|day| day.min.is_some()).count();
        if spring_days < MIN_SEASON_DAYS || autumn_days < MIN_SEASON_DAYS {
            continue;
        }

        complete_years += 1;

        if let Some(day) = spring
            .iter()
            .filter(|day| day.min.is_some_and(|min| min < FREEZING_C))
            .max_by_key(|day| (day.month, day.day))
        {
            last_spring.push((day.month, day.day));
        }

        if let Some(day) = autumn
            .iter()
            .filter(|day| day.min.is_some_and(|min| min < FREEZING_C))
            .min_by_key(|day| (day.month, day.day))
        {
            first_autumn.push((day.month, day.day));
        }
    }

    Estimate {
        last: nine_in_ten(&mut last_spring, 0.9),
        first: nine_in_ten(&mut first_autumn, 0.1),
        years: complete_years,
    }
}

/// Whether a month belongs to the cold-to-warm half of the year (the "spring" frost season).
fn in_spring(month: u32, northern: bool) -> bool {
    if northern { month <= 6 } else { month >= 7 }
}

/// The date at the `p` quantile, sorted by month and day.
fn nine_in_ten(values: &mut [(u32, u32)], p: f64) -> Option<(u32, u32)> {
    if values.is_empty() {
        return None;
    }

    values.sort_unstable();

    let index = ((values.len() as f64) * p).ceil() as usize;
    values
        .get(index.saturating_sub(1).min(values.len() - 1))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::{Estimate, FREEZING_C, estimate, nine_in_ten, parse, parse_date};
    use crate::calendar::Date;

    /// Every day of a year with a frost on `last_spring` and on `first_autumn`, both month/day.
    fn year_days(year: i32, last_spring: (u32, u32), first_autumn: (u32, u32)) -> Vec<super::Day> {
        let start = Date::new(year, 1, 1).unwrap();
        let mut days = Vec::new();

        for offset in 0..366 {
            let date = start.add_days(offset);
            if date.year != year {
                break;
            }

            let frost =
                (date.month, date.day) == last_spring || (date.month, date.day) == first_autumn;

            days.push(super::Day {
                year: date.year,
                month: date.month,
                day: date.day,
                min: Some(if frost { FREEZING_C - 1.0 } else { 8.0 }),
            });
        }

        days
    }

    fn ten_northern_years() -> Vec<super::Day> {
        let mut days = Vec::new();

        for index in 0..10 {
            let year = 1991 + index;
            let spring = Date::new(year, 3, 20).unwrap().add_days(index as i64);
            let autumn = Date::new(year, 11, 10).unwrap().add_days(-(index as i64));
            days.extend(year_days(
                year,
                (spring.month, spring.day),
                (autumn.month, autumn.day),
            ));
        }

        days
    }

    #[test]
    fn takes_the_nine_in_ten_date() {
        let estimate = estimate(&ten_northern_years(), true);

        assert_eq!(estimate.years, 10);
        assert_eq!(estimate.last, Some((3, 28)));
        assert_eq!(estimate.first, Some((11, 1)));
    }

    #[test]
    fn reads_the_southern_hemisphere_the_other_way() {
        let mut days = Vec::new();

        for index in 0..10 {
            let year = 1991 + index;
            // Spring is the second half of the year; autumn the first.
            let spring = Date::new(year, 9, 5).unwrap().add_days(index as i64);
            let autumn = Date::new(year, 5, 10).unwrap().add_days(-(index as i64));
            days.extend(year_days(
                year,
                (spring.month, spring.day),
                (autumn.month, autumn.day),
            ));
        }

        let estimate = estimate(&days, false);
        assert_eq!(estimate.last, Some((9, 13)));
        assert_eq!(estimate.first, Some((5, 1)));
    }

    #[test]
    fn skips_a_year_without_a_full_season() {
        let days = ten_northern_years();
        let without_1991: Vec<super::Day> =
            days.into_iter().filter(|day| day.year != 1991).collect();

        assert_eq!(estimate(&without_1991, true).years, 9);
    }

    #[test]
    fn a_mild_climate_has_no_frost_dates() {
        let mut days = ten_northern_years();
        for day in &mut days {
            day.min = Some(15.0);
        }

        let estimate = estimate(&days, true);
        assert_eq!(
            estimate,
            Estimate {
                last: None,
                first: None,
                years: 10
            }
        );
    }

    #[test]
    fn reads_the_archive_answer() {
        let body = r#"{
            "daily": {
                "time": ["1991-01-01", "1991-01-02", "1991-01-03"],
                "temperature_2m_min": [1.5, -2.0, null]
            }
        }"#;

        let days = parse(body).unwrap();
        assert_eq!(days.len(), 3);
        assert_eq!((days[1].month, days[1].day), (1, 2));
        assert_eq!(days[1].min, Some(-2.0));
        assert_eq!(days[2].min, None);
    }

    #[test]
    fn rejects_a_broken_answer() {
        assert!(parse("{}").is_err());
        assert!(parse(r#"{"daily": {"time": ["1991-01-01"]}}"#).is_err());
        assert!(parse_date("1991-02-30").is_err());
        assert!(parse_date("January").is_err());
    }

    #[test]
    fn percentiles_need_data() {
        assert_eq!(nine_in_ten(&mut [], 0.9), None);

        let mut dates = [(4, 15), (3, 1), (5, 2)];
        assert_eq!(nine_in_ten(&mut dates, 0.9), Some((5, 2)));
        assert_eq!(nine_in_ten(&mut dates, 0.1), Some((3, 1)));
    }

    /// Live check, not part of the normal test run: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_a_real_place() {
        let estimate = super::lookup(52.52, 13.405).unwrap();
        assert!(estimate.years >= 25);
        assert!(estimate.last.is_some());
        assert!(estimate.first.is_some());
    }
}
