//! Climate readings for a space: what the air did, and where the number came from.
//!
//! A reading belongs to the space, not to a planting: two plants in one tent breathe the same air.
//! These are local measurements — typed in by hand or read by a sensor. Weather-model data lives
//! in its own tables and never appears as a reading here.

use crate::calendar::Date;
use crate::plant_data::ValidationError;

/// Where a local reading came from. `manual` is written by the app today; a sensor joins later.
pub const SOURCES: &[&str] = &["manual", "sensor"];

/// Reading air temperature should stay in this range; a house or a garden never leaves it.
pub const TEMPERATURE_MIN_C: f64 = -50.0;
pub const TEMPERATURE_MAX_C: f64 = 60.0;

/// How many days the space page and the planting card summarise.
pub const SUMMARY_DAYS: i64 = 7;

#[derive(Clone, Debug, PartialEq)]
pub struct ClimateReading {
    pub id: i64,
    pub space_id: i64,
    /// Local date, the way the user read it off the thermometer.
    pub measured_on: Date,
    /// Local `HH:MM`; `None` when only the day is known.
    pub measured_time: Option<String>,
    pub temperature_c: Option<f64>,
    pub humidity_pct: Option<f64>,
    pub source: String,
    /// A provider or a sensor name; `None` for a hand-read value.
    pub source_ref: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClimateInput {
    pub space_id: i64,
    pub measured_on: Date,
    pub measured_time: Option<String>,
    pub temperature_c: Option<f64>,
    pub humidity_pct: Option<f64>,
    pub source: String,
    pub source_ref: Option<String>,
}

impl ClimateInput {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !SOURCES.contains(&self.source.as_str()) {
            return Err(ValidationError(format!(
                "unknown reading source {:?}",
                self.source
            )));
        }

        if self.temperature_c.is_none() && self.humidity_pct.is_none() {
            return Err(ValidationError(
                "a reading needs a temperature, a humidity, or both".into(),
            ));
        }

        if let Some(temperature) = self.temperature_c
            && (!temperature.is_finite()
                || !(TEMPERATURE_MIN_C..=TEMPERATURE_MAX_C).contains(&temperature))
        {
            return Err(ValidationError(format!(
                "air temperature must be between {TEMPERATURE_MIN_C} and {TEMPERATURE_MAX_C} °C"
            )));
        }

        if let Some(humidity) = self.humidity_pct
            && (!humidity.is_finite() || !(0.0..=100.0).contains(&humidity))
        {
            return Err(ValidationError(
                "relative humidity is a percentage between 0 and 100".into(),
            ));
        }

        if let Some(time) = &self.measured_time
            && crate::observation::parse_time(time).is_err()
        {
            return Err(ValidationError("use a time like 14:30".into()));
        }

        Ok(())
    }
}

pub fn source_label(source: &str) -> &'static str {
    match source {
        "manual" => "Manual",
        "sensor" => "Sensor",
        _ => "Unknown source",
    }
}

/// The last few days of one space, ready for a line on the space page and a card on the planting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub readings: usize,
    pub temperature_min: Option<f64>,
    pub temperature_max: Option<f64>,
    pub humidity_average: Option<f64>,
    /// The newest reading, whether or not it falls inside the window.
    pub latest: Option<ClimateReading>,
}

impl Summary {
    pub fn is_empty(&self) -> bool {
        self.readings == 0 && self.latest.is_none()
    }

    /// "18.2–27.4 °C · 61 % RH · 12 readings", already converted for display.
    pub fn line(
        &self,
        units: crate::units::UnitPreference,
        temperature_label: &str,
        humidity_label: &str,
    ) -> String {
        let mut parts = Vec::new();

        if let (Some(min), Some(max)) = (self.temperature_min, self.temperature_max) {
            if (max - min).abs() < 0.05 {
                parts.push(format!("{} {temperature_label}", units.number("°C", min)));
            } else {
                parts.push(format!(
                    "{}–{} {temperature_label}",
                    units.number("°C", min),
                    units.number("°C", max)
                ));
            }
        }

        if let Some(humidity) = self.humidity_average {
            parts.push(format!(
                "{} {humidity_label}",
                crate::plant_data::format_number(humidity.round())
            ));
        }

        if self.readings > 0 {
            parts.push(match self.readings {
                1 => "1 reading".to_string(),
                count => format!("{count} readings"),
            });
        }

        parts.join(" · ")
    }
}

/// Summarise the readings of the last `SUMMARY_DAYS` days up to `today`, and keep the newest
/// reading even when it is older than that.
pub fn summarize(readings: &[ClimateReading], today: Date) -> Summary {
    let from = today.add_days(-(SUMMARY_DAYS - 1));
    let mut summary = Summary::default();
    let mut humidity_sum = 0.0;
    let mut humidity_count = 0;

    for reading in readings {
        if reading.measured_on < from || reading.measured_on > today {
            continue;
        }

        summary.readings += 1;

        if let Some(temperature) = reading.temperature_c {
            summary.temperature_min = Some(
                summary
                    .temperature_min
                    .map_or(temperature, |value| value.min(temperature)),
            );
            summary.temperature_max = Some(
                summary
                    .temperature_max
                    .map_or(temperature, |value| value.max(temperature)),
            );
        }

        if let Some(humidity) = reading.humidity_pct {
            humidity_sum += humidity;
            humidity_count += 1;
        }
    }

    if humidity_count > 0 {
        summary.humidity_average = Some(humidity_sum / humidity_count as f64);
    }

    summary.latest = readings
        .iter()
        .filter(|reading| reading.measured_on <= today)
        .max_by(|left, right| {
            (left.measured_on, left.measured_time.as_deref())
                .cmp(&(right.measured_on, right.measured_time.as_deref()))
        })
        .cloned();

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(
        day: i32,
        time: Option<&str>,
        temperature: Option<f64>,
        humidity: Option<f64>,
    ) -> ClimateReading {
        ClimateReading {
            id: day as i64,
            space_id: 1,
            measured_on: Date::new(2026, 10, day as u32).unwrap(),
            measured_time: time.map(str::to_string),
            temperature_c: temperature,
            humidity_pct: humidity,
            source: "manual".to_string(),
            source_ref: None,
        }
    }

    #[test]
    fn a_reading_needs_one_value() {
        let input = ClimateInput {
            space_id: 1,
            measured_on: Date::new(2026, 10, 30).unwrap(),
            measured_time: None,
            temperature_c: None,
            humidity_pct: None,
            source: "manual".to_string(),
            source_ref: None,
        };
        assert!(input.validate().is_err());
    }

    #[test]
    fn temperature_and_humidity_ranges_are_checked() {
        let base = ClimateInput {
            space_id: 1,
            measured_on: Date::new(2026, 10, 30).unwrap(),
            measured_time: Some("14:30".to_string()),
            temperature_c: Some(21.0),
            humidity_pct: Some(60.0),
            source: "manual".to_string(),
            source_ref: None,
        };
        assert!(base.validate().is_ok());

        let mut hot = base.clone();
        hot.temperature_c = Some(80.0);
        assert!(hot.validate().is_err());

        let mut wet = base.clone();
        wet.humidity_pct = Some(101.0);
        assert!(wet.validate().is_err());

        let mut unknown = base.clone();
        unknown.source = "guess".to_string();
        assert!(unknown.validate().is_err());
    }

    #[test]
    fn one_reading_does_not_print_a_range() {
        let today = Date::new(2026, 10, 30).unwrap();
        let summary = summarize(&[reading(30, Some("14:30"), Some(14.2), Some(72.0))], today);

        assert_eq!(
            summary.line(crate::units::UnitPreference::Metric, "°C", "% RH"),
            "14.2 °C · 72 % RH · 1 reading"
        );
    }

    #[test]
    fn the_summary_keeps_the_window_and_the_newest_reading() {
        let today = Date::new(2026, 10, 30).unwrap();
        let readings = vec![
            reading(30, Some("18:00"), Some(22.0), Some(60.0)),
            reading(29, Some("09:00"), Some(18.0), Some(70.0)),
            reading(18, Some("09:00"), Some(5.0), Some(90.0)),
        ];

        let summary = summarize(&readings, today);
        assert_eq!(summary.readings, 2);
        assert_eq!(summary.temperature_min, Some(18.0));
        assert_eq!(summary.temperature_max, Some(22.0));
        assert_eq!(summary.humidity_average, Some(65.0));
        assert_eq!(
            summary.latest.and_then(|reading| reading.measured_time),
            Some("18:00".to_string())
        );
    }

    #[test]
    fn an_empty_space_has_an_empty_summary() {
        let summary = summarize(&[], Date::new(2026, 10, 30).unwrap());
        assert!(summary.is_empty());
        assert_eq!(
            summary.line(crate::units::UnitPreference::Metric, "°C", "% RH"),
            ""
        );
    }
}
