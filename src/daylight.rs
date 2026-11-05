//! Where the day ends and the night begins, for a space.
//!
//! The weather model supplies the sun times for the location. A powered light supplies its own
//! window, and a mixed greenhouse uses the union of the two. The graph uses these intervals for
//! the day/night background and for the guide that follows the day part. A missing schedule is
//! `None`, not a guess.

use crate::planting::{PlantingPlacement, Space};
use crate::weather::WeatherDay;

pub const MINUTES_PER_DAY: i64 = 24 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SunTimes {
    pub sunrise: i64,
    pub sunset: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightRule {
    /// Sun times from the weather model.
    Sun,
    /// One on/off window from the space's timer.
    Timer { start: i64, duration: i64 },
    /// A greenhouse: daylight plus supplemental light.
    SunAndTimer { start: i64, duration: i64 },
    /// A powered light without a start time, or no rule at all.
    Unknown,
}

/// The rule a space gives its plantings. An outdoor space and natural light follow the sun; a
/// powered light needs both its start and its hours; mixed light needs the timer only when it has
/// one and otherwise falls back to the sun.
pub fn light_rule(
    kind: &str,
    light_type: Option<&str>,
    light_start: Option<i64>,
    light_hours: Option<f64>,
) -> LightRule {
    if kind == "outdoor" || light_type == Some("natural") {
        return LightRule::Sun;
    }

    let timer = match (light_start, light_hours) {
        (Some(start), Some(hours)) if hours.is_finite() => Some((
            start.rem_euclid(MINUTES_PER_DAY),
            (hours * 60.0).round().clamp(0.0, MINUTES_PER_DAY as f64) as i64,
        )),
        _ => None,
    };

    match (light_type, timer) {
        (Some("led" | "hps"), Some((start, duration))) => LightRule::Timer { start, duration },
        (Some("led" | "hps"), None) => LightRule::Unknown,
        (Some("mixed"), Some((start, duration))) => LightRule::SunAndTimer { start, duration },
        (Some("mixed"), None) => LightRule::Sun,
        (_, _) => LightRule::Unknown,
    }
}

pub fn rule_for_space(space: &Space) -> LightRule {
    light_rule(
        &space.kind,
        space.light_type.as_deref(),
        space.light_start,
        space.light_hours,
    )
}

pub fn rule_for_placement(placement: &PlantingPlacement) -> LightRule {
    light_rule(
        &placement.space_kind,
        placement.light_type.as_deref(),
        placement.light_start,
        placement.light_hours,
    )
}

/// The sun times of a day, from the stored sunrise and sunset. A day without them falls back to
/// the night boundaries the weather sync already computed.
pub fn sun_times(day: &WeatherDay) -> Option<SunTimes> {
    let sunrise = day
        .sunrise
        .as_deref()
        .and_then(parse_clock)
        .or_else(|| day.night_end.as_deref().and_then(stamp_clock))?;
    let sunset = day
        .sunset
        .as_deref()
        .and_then(parse_clock)
        .or_else(|| day.night_start.as_deref().and_then(stamp_clock))?;

    (sunrise < sunset).then_some(SunTimes { sunrise, sunset })
}

/// The intervals of the day (minutes after midnight) in which it is light. `None` when the rule
/// cannot place the boundary; an empty list means the whole day is dark.
pub fn day_intervals(rule: LightRule, sun: Option<SunTimes>) -> Option<Vec<(i64, i64)>> {
    match rule {
        LightRule::Sun => {
            let sun = sun?;
            Some(vec![(sun.sunrise, sun.sunset)])
        }
        LightRule::Timer { start, duration } => Some(timer_intervals(start, duration)),
        LightRule::SunAndTimer { start, duration } => {
            let mut intervals = timer_intervals(start, duration);
            if let Some(sun) = sun {
                intervals.push((sun.sunrise, sun.sunset));
            }
            Some(merge(&intervals))
        }
        LightRule::Unknown => None,
    }
}

pub fn night_intervals(rule: LightRule, sun: Option<SunTimes>) -> Option<Vec<(i64, i64)>> {
    let day = day_intervals(rule, sun)?;
    Some(complement(&day))
}

/// Whether a minute falls in the light. `None` when the boundary is unknown.
pub fn is_day(rule: LightRule, sun: Option<SunTimes>, minutes: i64) -> Option<bool> {
    let intervals = day_intervals(rule, sun)?;
    let minutes = minutes.rem_euclid(MINUTES_PER_DAY);
    Some(
        intervals
            .iter()
            .any(|(start, end)| *start <= minutes && minutes < *end),
    )
}

fn timer_intervals(start: i64, duration: i64) -> Vec<(i64, i64)> {
    let start = start.rem_euclid(MINUTES_PER_DAY);
    let end = start + duration;

    if duration <= 0 {
        Vec::new()
    } else if duration >= MINUTES_PER_DAY {
        vec![(0, MINUTES_PER_DAY)]
    } else if end <= MINUTES_PER_DAY {
        vec![(start, end)]
    } else {
        vec![(start, MINUTES_PER_DAY), (0, end - MINUTES_PER_DAY)]
    }
}

fn merge(intervals: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut sorted: Vec<(i64, i64)> = intervals
        .iter()
        .copied()
        .filter(|(start, end)| start < end)
        .collect();
    sorted.sort_by_key(|(start, _)| *start);

    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (start, end) in sorted {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

fn complement(intervals: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut result = Vec::new();
    let mut cursor = 0;

    for (start, end) in intervals {
        if *start > cursor {
            result.push((cursor, *start));
        }
        cursor = cursor.max(*end);
    }
    if cursor < MINUTES_PER_DAY {
        result.push((cursor, MINUTES_PER_DAY));
    }
    result
}

/// `HH:MM` to minutes after midnight.
pub fn parse_clock(value: &str) -> Option<i64> {
    let value = value.trim();
    let (hours, minutes) = value.split_once(':')?;
    let hours: i64 = hours.parse().ok()?;
    let minutes: i64 = minutes.get(0..2)?.parse().ok()?;
    ((0..24).contains(&hours) && (0..60).contains(&minutes)).then_some(hours * 60 + minutes)
}

/// `YYYY-MM-DDTHH:MM` to its time of day.
fn stamp_clock(value: &str) -> Option<i64> {
    parse_clock(value.split_once('T').map_or(value, |(_, time)| time))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Date;

    fn sun() -> SunTimes {
        SunTimes {
            sunrise: 8 * 60,
            sunset: 18 * 60 + 30,
        }
    }

    #[test]
    fn an_outdoor_space_follows_the_sun() {
        let rule = light_rule("outdoor", None, None, None);
        assert_eq!(rule, LightRule::Sun);
        assert_eq!(
            day_intervals(rule, Some(sun())),
            Some(vec![(8 * 60, 18 * 60 + 30)])
        );
        assert_eq!(
            night_intervals(rule, Some(sun())),
            Some(vec![(0, 8 * 60), (18 * 60 + 30, 1440)])
        );
    }

    #[test]
    fn a_timer_can_cross_midnight() {
        let rule = LightRule::Timer {
            start: 18 * 60,
            duration: 12 * 60,
        };
        assert_eq!(
            day_intervals(rule, None),
            Some(vec![(18 * 60, 1440), (0, 6 * 60)])
        );
        assert_eq!(is_day(rule, None, 2 * 60), Some(true));
        assert_eq!(is_day(rule, None, 8 * 60), Some(false));
    }

    #[test]
    fn a_mixed_space_unions_sun_and_timer() {
        let rule = LightRule::SunAndTimer {
            start: 6 * 60,
            duration: 14 * 60,
        };
        assert_eq!(
            day_intervals(rule, Some(sun())),
            Some(vec![(6 * 60, 20 * 60)])
        );
    }

    #[test]
    fn a_powered_light_without_a_start_is_unknown() {
        assert_eq!(
            light_rule("indoor", Some("led"), None, Some(16.0)),
            LightRule::Unknown
        );
        assert_eq!(
            light_rule("indoor", Some("led"), Some(6 * 60), Some(16.0)),
            LightRule::Timer {
                start: 6 * 60,
                duration: 16 * 60
            }
        );
    }

    #[test]
    fn a_natural_day_without_sun_times_has_no_boundary() {
        assert_eq!(day_intervals(LightRule::Sun, None), None);
        assert_eq!(is_day(LightRule::Sun, None, 12 * 60), None);
    }

    #[test]
    fn zero_and_full_days_are_honest() {
        assert_eq!(
            day_intervals(
                LightRule::Timer {
                    start: 6 * 60,
                    duration: 0
                },
                None
            ),
            Some(Vec::new())
        );
        assert_eq!(
            day_intervals(
                LightRule::Timer {
                    start: 6 * 60,
                    duration: 1440
                },
                None
            ),
            Some(vec![(0, 1440)])
        );
    }

    #[test]
    fn reads_the_stored_sun_times_and_night_fallback() {
        let mut day = WeatherDay {
            latitude: 51.0,
            longitude: 3.0,
            date: Date::new(2026, 10, 31).unwrap(),
            temp_min_c: None,
            temp_max_c: None,
            temp_mean_c: None,
            humidity_mean_pct: None,
            humidity_min_pct: None,
            humidity_max_pct: None,
            precipitation_mm: None,
            precipitation_hours: None,
            weather_code: None,
            sunshine_hours: None,
            daylight_hours: None,
            radiation_sum_mj_m2: None,
            wind_max_kmh: None,
            wind_gust_max_kmh: None,
            et0_mm: None,
            sunrise: Some("07:35".into()),
            sunset: Some("17:20".into()),
            night_start: None,
            night_end: None,
            night_temp_avg_c: None,
            night_temp_min_c: None,
            night_humidity_avg_pct: None,
            night_hours: None,
            night_complete: false,
        };
        assert_eq!(
            sun_times(&day),
            Some(SunTimes {
                sunrise: 7 * 60 + 35,
                sunset: 17 * 60 + 20
            })
        );

        day.sunrise = None;
        day.sunset = None;
        day.night_end = Some("2026-10-31T07:00".into());
        day.night_start = Some("2026-10-30T18:00".into());
        assert_eq!(
            sun_times(&day),
            Some(SunTimes {
                sunrise: 7 * 60,
                sunset: 18 * 60
            })
        );
    }
}
