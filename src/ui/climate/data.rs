//! The data behind a climate graph: weather, local readings, placements and requirements.
//!
//! One section shares one time window, one source set and one selected day. The drawing layer only
//! reads this data; it never loads from the store itself.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::calendar::Date;
use crate::climate::ClimateReading;
use crate::clock::today;
use crate::daylight::{
    self, LightRule, SunTimes, is_day, rule_for_placement, rule_for_space, sun_times,
};
use crate::plant_data::RequirementResolution;
use crate::planting::{Planting, PlantingPlacement, Space};
use crate::schedule::Schedule;
use crate::store::Store;
use crate::units::UnitPreference;
use crate::weather::{WeatherDay, WeatherHour};

/// The date the page's calendar has selected. The section reads it on every draw.
pub(super) type SelectedDay = Rc<Cell<Option<Date>>>;

pub(super) fn selected_day(date: Date) -> SelectedDay {
    Rc::new(Cell::new(Some(date)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Metric {
    Temperature,
    Rain,
    Sunshine,
    Humidity,
}

pub(super) const METRICS: [Metric; 4] = [
    Metric::Temperature,
    Metric::Rain,
    Metric::Sunshine,
    Metric::Humidity,
];

impl Metric {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Temperature => "Temperature",
            Self::Rain => "Rain",
            Self::Sunshine => "Sunshine",
            Self::Humidity => "Humidity",
        }
    }

    pub(super) fn unit(self) -> &'static str {
        match self {
            Self::Temperature => "°C",
            Self::Rain => "mm",
            Self::Sunshine => "h",
            Self::Humidity => "%",
        }
    }

    pub(super) fn is_air(self) -> bool {
        matches!(self, Self::Temperature | Self::Humidity)
    }

    /// The guide key in the profile's requirements.
    pub(super) fn guide_key(self) -> Option<&'static str> {
        match self {
            Self::Temperature => Some("air_temp"),
            Self::Humidity => Some("rh"),
            Self::Rain | Self::Sunshine => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Range {
    Day,
    Days(i64),
    All,
}

pub(super) const RANGES: [Range; 5] = [
    Range::Day,
    Range::Days(7),
    Range::Days(30),
    Range::Days(90),
    Range::All,
];

impl Range {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Day => "24h",
            Self::Days(7) => "7d",
            Self::Days(30) => "30d",
            Self::Days(90) => "90d",
            Self::Days(_) | Self::All => "All",
        }
    }

    pub(super) fn hourly(self) -> bool {
        matches!(self, Self::Day | Self::Days(7))
    }
}

/// How a day is drawn above 7 days. Day and night are one average each; the default keeps the
/// extremes of the day and one mean, so the chart is not two unexplained lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DayNightView {
    Combined,
    Both,
    Day,
    Night,
}

impl DayNightView {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Combined => "Mean & range",
            Self::Both => "Day & night",
            Self::Day => "Day only",
            Self::Night => "Night only",
        }
    }

    pub(super) fn draws_day(self) -> bool {
        matches!(self, Self::Combined | Self::Both | Self::Day)
    }

    pub(super) fn draws_night(self) -> bool {
        matches!(self, Self::Both | Self::Night)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Sources {
    pub weather: bool,
    pub sensor: bool,
    pub manual: bool,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            weather: true,
            sensor: true,
            manual: true,
        }
    }
}

pub(super) type SourceState = Rc<Cell<Sources>>;
pub(super) type GraphRange = Rc<Cell<Range>>;
pub(super) type ViewState = Rc<Cell<DayNightView>>;

#[cfg(test)]
pub(super) fn default_sources() -> SourceState {
    Rc::new(Cell::new(Sources::default()))
}

#[cfg(test)]
pub(super) fn default_view() -> ViewState {
    Rc::new(Cell::new(DayNightView::Combined))
}

/// One stage window, used to name the selected day in the tooltip. The graph itself no longer draws
/// a ribbon; the timeline above the calendar already shows the stage.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct StageSpan {
    pub start: Date,
    pub end: Date,
    pub label: String,
}

/// One range of days with the same guide values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GuideSpan {
    pub start: Date,
    pub end: Date,
    pub reference: Reference,
}

/// The temperature and humidity ranges for one day part: what is comfortable and what it
/// survives.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Reference {
    pub opt_min: Option<f64>,
    pub opt_max: Option<f64>,
    pub survival_min: Option<f64>,
    pub survival_max: Option<f64>,
}

impl Reference {
    pub(super) fn is_empty(self) -> bool {
        self.opt_min.is_none()
            && self.opt_max.is_none()
            && self.survival_min.is_none()
            && self.survival_max.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DayPart {
    Day,
    Night,
}

impl DayPart {
    fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Night => "night",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Guides {
    pub temperature: DayNightGuides,
    pub humidity: DayNightGuides,
}

impl Guides {
    pub(super) fn for_metric(&self, metric: Metric) -> Option<&DayNightGuides> {
        match metric {
            Metric::Temperature => Some(&self.temperature),
            Metric::Humidity => Some(&self.humidity),
            Metric::Rain | Metric::Sunshine => None,
        }
    }

    fn reference(&self, metric: Metric, part: DayPart, date: Date) -> Option<Reference> {
        let guides = self.for_metric(metric)?;
        let spans = match part {
            DayPart::Day => &guides.day,
            DayPart::Night => &guides.night,
        };
        spans
            .iter()
            .rev()
            .find(|span| span.start <= date && date <= span.end)
            .map(|span| span.reference)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct DayNightGuides {
    pub day: Vec<GuideSpan>,
    pub night: Vec<GuideSpan>,
}

/// The requirement for one metric and day part, at the profile default or at a stage.
fn requirement_reference(
    store: &Store,
    profile_id: i64,
    stage_id: Option<i64>,
    metric: &str,
    part: DayPart,
) -> Option<Reference> {
    let resolution = store
        .resolved_requirement(profile_id, stage_id, metric, part.as_str())
        .ok()
        .flatten()?;
    let RequirementResolution::Resolved(requirement) = resolution else {
        return None;
    };

    let reference = Reference {
        opt_min: requirement.opt_min,
        opt_max: requirement.opt_max,
        survival_min: requirement.survival_min,
        survival_max: requirement.survival_max,
    };

    (!reference.is_empty()).then_some(reference)
}

fn stage_guide_spans(
    store: &Store,
    profile_id: i64,
    schedule: &Schedule,
    start: Date,
    end: Date,
    metric: &str,
    part: DayPart,
) -> Vec<GuideSpan> {
    let default = requirement_reference(store, profile_id, None, metric, part);
    let mut cache: HashMap<i64, Option<Reference>> = HashMap::new();
    let mut stages = Vec::new();

    for window in &schedule.stages {
        if window.likely_end < start || window.likely_start > end {
            continue;
        }

        let reference = *cache.entry(window.stage.id).or_insert_with(|| {
            requirement_reference(store, profile_id, Some(window.stage.id), metric, part)
        });
        stages.push((window.likely_start, window.likely_end, reference));
    }

    guide_spans(start, end, default, &stages)
}

/// Fills a per-day series from the stage windows and merges equal neighbours into spans.
fn guide_spans(
    start: Date,
    end: Date,
    default: Option<Reference>,
    stages: &[(Date, Date, Option<Reference>)],
) -> Vec<GuideSpan> {
    let total = start.days_until(end).max(0) as usize + 1;
    let mut days: Vec<Option<Option<Reference>>> = vec![None; total];

    for (from, to, reference) in stages {
        let from = (*from).max(start);
        let to = (*to).min(end);
        if from > to {
            continue;
        }

        let first = start.days_until(from) as usize;
        let last = start.days_until(to) as usize;
        for day in days.iter_mut().take(last + 1).skip(first) {
            *day = Some(*reference);
        }
    }

    let mut spans: Vec<GuideSpan> = Vec::new();
    for (index, day) in days.into_iter().enumerate() {
        let reference = match day {
            Some(reference) => reference,
            None => default,
        };
        let Some(reference) = reference else {
            continue;
        };

        let date = start.add_days(index as i64);
        match spans.last_mut() {
            Some(span) if span.end.add_days(1) == date && span.reference == reference => {
                span.end = date;
            }
            _ => spans.push(GuideSpan {
                start: date,
                end: date,
                reference,
            }),
        }
    }

    spans
}

fn stage_spans(schedule: &Schedule, start: Date, end: Date) -> Vec<StageSpan> {
    schedule
        .stages
        .iter()
        .filter_map(|window| {
            let from = window.likely_start.max(start);
            let to = window.likely_end.min(end);
            (from <= to).then(|| StageSpan {
                start: from,
                end: to,
                label: window.stage.label.clone(),
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct DailyWeather {
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// The provider's day mean, or the mean of the hours that passed today.
    pub mean: Option<f64>,
    pub day: Option<f64>,
    pub night: Option<f64>,
    /// True when `day` is the 24-hour mean because no day part could be derived.
    pub day_is_mean: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct WeatherAggregate {
    pub temperature: DailyWeather,
    pub humidity: DailyWeather,
}

impl WeatherAggregate {
    pub(super) fn values(self, metric: Metric) -> DailyWeather {
        match metric {
            Metric::Temperature => self.temperature,
            Metric::Humidity => self.humidity,
            Metric::Rain | Metric::Sunshine => DailyWeather::default(),
        }
    }
}

fn derived_day(mean: Option<f64>, night: Option<f64>, night_hours: Option<f64>) -> Option<f64> {
    match (mean, night, night_hours) {
        (Some(mean), Some(night), Some(hours)) if hours > 0.0 && hours < 24.0 => {
            Some((mean * 24.0 - night * hours) / (24.0 - hours))
        }
        (Some(mean), _, _) => Some(mean),
        _ => None,
    }
}

pub(super) fn aggregate_weather(
    day: &WeatherDay,
    hours: &[&WeatherHour],
    is_today: bool,
) -> WeatherAggregate {
    let temperatures: Vec<f64> = hours.iter().filter_map(|hour| hour.temperature_c).collect();
    let humidities: Vec<f64> = hours.iter().filter_map(|hour| hour.humidity_pct).collect();
    let hourly_min = temperatures.iter().copied().reduce(f64::min);
    let hourly_max = temperatures.iter().copied().reduce(f64::max);
    let hourly_humidity_min = humidities.iter().copied().reduce(f64::min);
    let hourly_humidity_max = humidities.iter().copied().reduce(f64::max);

    // Today's daily row is a forecast for the whole day. The graph only shows what has already
    // happened, so for today the extremes and the mean come from the hours that passed.
    let mut temperature = DailyWeather {
        min: if is_today {
            hourly_min
        } else {
            day.temp_min_c.or(hourly_min)
        },
        max: if is_today {
            hourly_max
        } else {
            day.temp_max_c.or(hourly_max)
        },
        mean: if is_today {
            average(&temperatures)
        } else {
            day.temp_mean_c.or_else(|| average(&temperatures))
        },
        day: None,
        night: None,
        day_is_mean: false,
    };
    let mut humidity = DailyWeather {
        min: if is_today {
            hourly_humidity_min
        } else {
            day.humidity_min_pct.or(hourly_humidity_min)
        },
        max: if is_today {
            hourly_humidity_max
        } else {
            day.humidity_max_pct.or(hourly_humidity_max)
        },
        mean: if is_today {
            average(&humidities)
        } else {
            day.humidity_mean_pct.or_else(|| average(&humidities))
        },
        day: None,
        night: None,
        day_is_mean: false,
    };

    let sun = sun_times(day);
    let day_temp: Vec<f64> = hours
        .iter()
        .filter(|hour| {
            hour.is_day
                .or_else(|| is_day(LightRule::Sun, sun, hour.minutes))
                == Some(true)
        })
        .filter_map(|hour| hour.temperature_c)
        .collect();
    let night_temp: Vec<f64> = hours
        .iter()
        .filter(|hour| {
            hour.is_day
                .or_else(|| is_day(LightRule::Sun, sun, hour.minutes))
                == Some(false)
        })
        .filter_map(|hour| hour.temperature_c)
        .collect();
    let day_humidity: Vec<f64> = hours
        .iter()
        .filter(|hour| {
            hour.is_day
                .or_else(|| is_day(LightRule::Sun, sun, hour.minutes))
                == Some(true)
        })
        .filter_map(|hour| hour.humidity_pct)
        .collect();
    let night_humidity: Vec<f64> = hours
        .iter()
        .filter(|hour| {
            hour.is_day
                .or_else(|| is_day(LightRule::Sun, sun, hour.minutes))
                == Some(false)
        })
        .filter_map(|hour| hour.humidity_pct)
        .collect();

    temperature.day = average(&day_temp);
    temperature.night = average(&night_temp).or(day.night_temp_avg_c);
    humidity.day = average(&day_humidity);
    humidity.night = average(&night_humidity).or(day.night_humidity_avg_pct);

    // A day that has not finished has no honest daily mean to derive a missing half from.
    if !is_today {
        if temperature.day.is_none() {
            temperature.day = derived_day(day.temp_mean_c, temperature.night, day.night_hours);
            temperature.day_is_mean = temperature.day.is_some() && temperature.night.is_none();
        }
        if humidity.day.is_none() {
            humidity.day = derived_day(day.humidity_mean_pct, humidity.night, day.night_hours);
            humidity.day_is_mean = humidity.day.is_some() && humidity.night.is_none();
        }
    }

    WeatherAggregate {
        temperature,
        humidity,
    }
}

pub(super) fn average(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

pub(super) struct ClimateData {
    pub weather_days: Vec<WeatherDay>,
    pub weather_hours: Vec<WeatherHour>,
    pub readings: Vec<ClimateReading>,
    pub placements: Vec<PlantingPlacement>,
    pub labels: Vec<String>,
    pub units: UnitPreference,
    pub selected: SelectedDay,
    pub range: GraphRange,
    pub sources: SourceState,
    pub view: ViewState,
    pub guides: Guides,
    pub stages: Vec<StageSpan>,
    pub grow: Option<(Date, Option<Date>)>,
    pub space: Option<Space>,
    pub weather_series: bool,
    pub anchor_selected: bool,
    pub days_by_date: HashMap<Date, usize>,
    pub hours_by_date: HashMap<Date, Vec<usize>>,
    pub aggregates: HashMap<Date, WeatherAggregate>,
    /// The last sync failure, so an empty range can say why instead of "no data".
    pub weather_error: Option<String>,
}

impl ClimateData {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        weather_days: Vec<WeatherDay>,
        weather_hours: Vec<WeatherHour>,
        readings: Vec<ClimateReading>,
        placements: Vec<PlantingPlacement>,
        labels: Vec<String>,
        units: UnitPreference,
        selected: SelectedDay,
        range: GraphRange,
        sources: SourceState,
        view: ViewState,
        guides: Guides,
        stages: Vec<StageSpan>,
        grow: Option<(Date, Option<Date>)>,
        space: Option<Space>,
        weather_series: bool,
        anchor_selected: bool,
        weather_error: Option<String>,
    ) -> Self {
        let mut days_by_date = HashMap::new();
        for (index, day) in weather_days.iter().enumerate() {
            days_by_date.insert(day.date, index);
        }

        let mut hours_by_date: HashMap<Date, Vec<usize>> = HashMap::new();
        for (index, hour) in weather_hours.iter().enumerate() {
            hours_by_date.entry(hour.date).or_default().push(index);
        }

        let mut aggregates = HashMap::new();
        let now = crate::clock::now();
        for day in &weather_days {
            let is_today = now.is_some_and(|(today, _)| day.date == today);
            let indices = hours_by_date.get(&day.date).cloned().unwrap_or_default();
            let hours: Vec<&WeatherHour> = indices
                .iter()
                .filter_map(|index| weather_hours.get(*index))
                .filter(|hour| {
                    now.is_none_or(|(today, minutes)| day.date != today || hour.minutes <= minutes)
                })
                .collect();
            aggregates.insert(day.date, aggregate_weather(day, &hours, is_today));
        }

        Self {
            weather_days,
            weather_hours,
            readings,
            placements,
            labels,
            units,
            selected,
            range,
            sources,
            view,
            guides,
            stages,
            grow,
            space,
            weather_series,
            anchor_selected,
            days_by_date,
            hours_by_date,
            aggregates,
            weather_error,
        }
    }

    pub(super) fn window(&self) -> (Date, Date) {
        let anchor = if self.anchor_selected {
            self.selected.get().unwrap_or_else(today)
        } else {
            today()
        };

        match self.range.get() {
            Range::Day => (anchor, anchor),
            Range::Days(days) => (anchor.add_days(-(days - 1)), anchor),
            Range::All => {
                let first_data = self
                    .weather_days
                    .first()
                    .map(|day| day.date)
                    .into_iter()
                    .chain(self.readings.iter().map(|reading| reading.measured_on))
                    .min();
                let start = match self.grow {
                    Some((grow_start, _)) => first_data
                        .map(|first| first.max(grow_start))
                        .unwrap_or(grow_start),
                    None => first_data.unwrap_or(anchor),
                };
                (start.min(anchor), anchor)
            }
        }
    }

    pub(super) fn total_days(&self) -> i64 {
        let (start, end) = self.window();
        start.days_until(end).max(0) + 1
    }

    pub(super) fn weather_day(&self, date: Date) -> Option<&WeatherDay> {
        self.days_by_date
            .get(&date)
            .and_then(|index| self.weather_days.get(*index))
    }

    pub(super) fn hours(&self, date: Date) -> Vec<&WeatherHour> {
        self.hours_by_date
            .get(&date)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|index| self.weather_hours.get(*index))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn aggregate(&self, date: Date) -> Option<WeatherAggregate> {
        self.aggregates.get(&date).copied()
    }

    pub(super) fn sun(&self, date: Date) -> Option<SunTimes> {
        self.weather_day(date).and_then(sun_times)
    }

    pub(super) fn rule_at(&self, date: Date) -> LightRule {
        if let Some(space) = &self.space {
            return rule_for_space(space);
        }

        self.placements
            .iter()
            .find(|placement| placement.covers(date))
            .map(rule_for_placement)
            .unwrap_or(LightRule::Unknown)
    }

    pub(super) fn placement_at(&self, date: Date) -> Option<&PlantingPlacement> {
        self.placements
            .iter()
            .find(|placement| placement.covers(date))
    }

    pub(super) fn placement_label(&self, date: Date) -> Option<&str> {
        if self.placements.len() < 2 {
            return None;
        }
        let index = self
            .placements
            .iter()
            .position(|placement| placement.covers(date))?;
        self.labels.get(index).map(String::as_str)
    }

    pub(super) fn weather_visible(&self, date: Date) -> bool {
        if self.space.is_some() {
            self.weather_series
        } else {
            self.placements
                .iter()
                .any(|placement| placement.space_kind == "outdoor" && placement.covers(date))
        }
    }

    pub(super) fn stage_at(&self, date: Date) -> Option<&StageSpan> {
        self.stages
            .iter()
            .rev()
            .find(|span| span.start <= date && date <= span.end)
    }

    pub(super) fn stage_label(&self, date: Date) -> Option<&str> {
        self.stage_at(date).map(|span| span.label.as_str())
    }

    pub(super) fn guide(&self, metric: Metric, part: DayPart, date: Date) -> Option<Reference> {
        self.guides.reference(metric, part, date)
    }

    pub(super) fn has_weather(&self, metric: Metric, start: Date, end: Date) -> bool {
        let hourly = self.range.get() == Range::Day;
        self.weather_days.iter().any(|day| {
            if !(start <= day.date && day.date <= end) || !self.weather_visible(day.date) {
                return false;
            }
            if hourly {
                if self.hours(day.date).iter().any(|hour| {
                    !is_future_hour(day.date, hour.minutes) && hourly_value(metric, hour).is_some()
                }) {
                    return true;
                }
                // A past day older than the hourly history may only have its daily row. The
                // hourly series already falls back to it; the section must agree.
                day.date != today()
                    && match metric {
                        Metric::Temperature | Metric::Humidity => {
                            self.aggregate(day.date).is_some_and(|aggregate| {
                                let values = aggregate.values(metric);
                                values.min.is_some()
                                    || values.max.is_some()
                                    || values.mean.is_some()
                                    || values.day.is_some()
                                    || values.night.is_some()
                            })
                        }
                        Metric::Rain | Metric::Sunshine => {
                            self.day_value(metric, day.date).is_some()
                        }
                    }
            } else {
                match metric {
                    Metric::Temperature | Metric::Humidity => {
                        self.aggregate(day.date).is_some_and(|aggregate| {
                            let values = aggregate.values(metric);
                            values.min.is_some()
                                || values.max.is_some()
                                || values.mean.is_some()
                                || values.day.is_some()
                                || values.night.is_some()
                        })
                    }
                    Metric::Rain | Metric::Sunshine => self.day_value(metric, day.date).is_some(),
                }
            }
        })
    }

    /// The value for one day in a collapsed view. Today's rain and sunshine are summed from the
    /// hours that passed, so a daily forecast never becomes a bar.
    pub(super) fn day_value(&self, metric: Metric, date: Date) -> Option<f64> {
        if date == today() && matches!(metric, Metric::Rain | Metric::Sunshine) {
            // Today's daily row is a forecast. Rain and sunshine above 7 days are summed from the
            // hours that already passed; no hour means no value, not a forecast bar.
            let values: Vec<f64> = self
                .hours(date)
                .iter()
                .filter(|hour| !is_future_hour(date, hour.minutes))
                .filter_map(|hour| hourly_value(metric, hour))
                .collect();
            return (!values.is_empty()).then(|| values.iter().sum());
        }

        match metric {
            Metric::Rain => self.weather_day(date).and_then(|day| day.precipitation_mm),
            Metric::Sunshine => self.weather_day(date).and_then(|day| day.sunshine_hours),
            Metric::Temperature | Metric::Humidity => None,
        }
    }

    pub(super) fn has_readings(
        &self,
        source: &str,
        metric: Metric,
        start: Date,
        end: Date,
    ) -> bool {
        self.readings.iter().any(|reading| {
            start <= reading.measured_on
                && reading.measured_on <= end
                && reading.source == source
                && reading_value(metric, reading).is_some()
        })
    }

    pub(super) fn has_guides(&self, metric: Metric, start: Date, end: Date) -> bool {
        self.guides.for_metric(metric).is_some_and(|guides| {
            guides
                .day
                .iter()
                .chain(&guides.night)
                .any(|span| span.end >= start && span.start <= end)
        })
    }

    pub(super) fn has_data(&self, metric: Metric, start: Date, end: Date) -> bool {
        self.has_weather(metric, start, end)
            || self.has_readings("manual", metric, start, end)
            || self.has_readings("sensor", metric, start, end)
    }

    /// Data that the current source switches actually show. A graph with only weather data and
    /// the weather switch off is empty, even though the rows exist.
    pub(super) fn has_visible_data(&self, metric: Metric, start: Date, end: Date) -> bool {
        let sources = self.sources.get();
        (sources.weather && self.has_weather(metric, start, end))
            || (sources.sensor && self.has_readings("sensor", metric, start, end))
            || (sources.manual && self.has_readings("manual", metric, start, end))
    }

    pub(super) fn selected_date(&self) -> Option<Date> {
        self.selected.get()
    }

    /// Why this range is empty, in the user's terms: the last day that has weather and the last
    /// failed sync. Empty when there is nothing useful to say.
    pub(super) fn empty_note(&self, start: Date, end: Date) -> Option<String> {
        let mut parts = Vec::new();

        if let Some(last) = self.weather_days.last()
            && last.date < start
        {
            parts.push(format!("Last data: {}.", short_date(last.date)));
        } else if let Some(first) = self.weather_days.first()
            && first.date > end
        {
            parts.push(format!("Data starts: {}.", short_date(first.date)));
        }

        if let Some(error) = &self.weather_error {
            parts.push(format!("Last sync failed: {error}."));
        }

        if start == end
            && start <= today()
            && !self.weather_visible(start)
            && self.placements.len() <= 1
            && self.space.is_some()
        {
            // A space graph has no placement; this branch is for the indoor space.
            parts.push("The weather graph is shown for an outdoor space only.".to_string());
        }

        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

pub(super) fn reading_value(metric: Metric, reading: &ClimateReading) -> Option<f64> {
    match metric {
        Metric::Temperature => reading.temperature_c,
        Metric::Humidity => reading.humidity_pct,
        Metric::Rain | Metric::Sunshine => None,
    }
}

pub(super) fn reading_minutes(reading: &ClimateReading) -> i64 {
    reading
        .measured_time
        .as_deref()
        .and_then(daylight::parse_clock)
        .unwrap_or(12 * 60)
}

pub(super) fn hourly_value(metric: Metric, hour: &WeatherHour) -> Option<f64> {
    match metric {
        Metric::Temperature => hour.temperature_c,
        Metric::Humidity => hour.humidity_pct,
        Metric::Rain => hour.precipitation_mm,
        Metric::Sunshine => hour.sunshine_hours,
    }
}

/// Today's later hours are model forecast; the graph does not draw them.
pub(super) fn is_future_hour(date: Date, minutes: i64) -> bool {
    crate::clock::now().is_some_and(|(today, now_minutes)| date == today && minutes > now_minutes)
}

pub(super) fn dates_between(start: Date, end: Date) -> Vec<Date> {
    let mut dates = Vec::new();
    let mut date = start;
    while date <= end {
        dates.push(date);
        date = date.add_days(1);
    }
    dates
}

pub(super) fn short_date(date: Date) -> String {
    super::super::shared::graph_style::short_date(date, today())
}

pub(super) fn display_value(metric: Metric, units: UnitPreference, value: f64) -> String {
    format!(
        "{} {}",
        display_number(metric, units, value),
        units.label(metric.unit())
    )
}

pub(super) fn display_number(metric: Metric, units: UnitPreference, value: f64) -> String {
    let displayed = units.to_display(metric.unit(), value);
    let decimals = if metric == Metric::Rain && units.is_imperial() {
        100.0
    } else {
        10.0
    };
    let rounded = (displayed * decimals).round() / decimals;
    let text = format!("{rounded:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub(super) fn source_label(source: &str) -> &'static str {
    match source {
        "manual" => "Manual",
        "sensor" => "Sensor",
        _ => "Unknown source",
    }
}

pub(super) fn kinds_with_data(data: &ClimateData) -> Vec<Metric> {
    // A graph without data is not a graph. A planting or space with no weather and no readings
    // therefore has no Climate section; the requirements stay on the profile and the timeline.
    // The current window may still be empty (a selected indoor day); the panel then says why, and
    // the metric stays because the grow has data elsewhere.
    let start = Date::new(1900, 1, 1).expect("a valid date");
    let end = Date::new(2100, 12, 31).expect("a valid date");
    METRICS
        .into_iter()
        .filter(|metric| {
            data.has_weather(*metric, start, end)
                || data
                    .readings
                    .iter()
                    .any(|reading| reading_value(*metric, reading).is_some())
        })
        .collect()
}

/// The planting's climate data, with the section state it was last given.
pub(super) fn planting_data(
    store: &Store,
    planting: &Planting,
    units: UnitPreference,
    selected: SelectedDay,
    schedule: &Schedule,
    state: super::SectionState,
) -> Option<ClimateData> {
    let placements = store.planting_placements(planting.id).ok()?;
    if placements.is_empty() {
        return None;
    }

    let (weather_days, weather_hours) = location_weather(store, planting.started_on);
    let readings = store.planting_climate_readings(planting.id).ok()?;
    let labels = placements
        .iter()
        .map(|placement| space_label(store, placement.space_id))
        .collect::<Vec<_>>();

    let grow_start = planting.started_on;
    let grow_end = planting.ended_on.unwrap_or_else(today);
    let metric_guides = |metric: Metric| -> DayNightGuides {
        let key = metric.guide_key().unwrap_or_default();
        DayNightGuides {
            day: stage_guide_spans(
                store,
                planting.profile_id,
                schedule,
                grow_start,
                grow_end,
                key,
                DayPart::Day,
            ),
            night: stage_guide_spans(
                store,
                planting.profile_id,
                schedule,
                grow_start,
                grow_end,
                key,
                DayPart::Night,
            ),
        }
    };

    let guides = Guides {
        temperature: metric_guides(Metric::Temperature),
        humidity: metric_guides(Metric::Humidity),
    };

    let weather_series = placements
        .iter()
        .any(|placement| placement.space_kind == "outdoor");

    Some(ClimateData::new(
        weather_days,
        weather_hours,
        readings,
        placements,
        labels,
        units,
        selected,
        state.range,
        state.sources,
        state.view,
        guides,
        stage_spans(schedule, grow_start, grow_end),
        Some((grow_start, planting.ended_on)),
        None,
        weather_series,
        true,
        weather_error(store),
    ))
}

/// The space graph is the climate timeline of a place, not only of its active plantings.
pub(super) fn space_data(
    store: &Store,
    space: &Space,
    units: UnitPreference,
    state: super::SectionState,
) -> Option<ClimateData> {
    let (weather_days, weather_hours) =
        location_weather(store, Date::new(1900, 1, 1).expect("a valid date"));
    let readings = store.climate_readings(space.id).unwrap_or_default();

    Some(ClimateData::new(
        weather_days,
        weather_hours,
        readings,
        Vec::new(),
        Vec::new(),
        units,
        Rc::new(Cell::new(None)),
        state.range,
        state.sources,
        state.view,
        Guides::default(),
        Vec::new(),
        None,
        Some(space.clone()),
        space.kind == "outdoor",
        false,
        weather_error(store),
    ))
}

/// A failed sync is only worth reporting while Open-Meteo is on; offline, the note would suggest
/// that a retry is coming.
fn weather_error(store: &Store) -> Option<String> {
    if crate::online::enabled(store) {
        crate::weather::last_error(store)
    } else {
        None
    }
}

fn location_weather(store: &Store, since: Date) -> (Vec<WeatherDay>, Vec<WeatherHour>) {
    let Some(location) = store.location().ok().flatten() else {
        return (Vec::new(), Vec::new());
    };

    let days = store
        .weather_days(location.latitude, location.longitude, since)
        .unwrap_or_default();
    let hours = store
        .weather_hours(location.latitude, location.longitude, since, today())
        .unwrap_or_default();
    (days, hours)
}

fn space_label(store: &Store, space_id: i64) -> String {
    store
        .space(space_id)
        .ok()
        .flatten()
        .and_then(|space| space.display_name().map(str::to_string))
        .unwrap_or_else(|| "Private space".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(temperature: Option<f64>, humidity: Option<f64>) -> ClimateReading {
        ClimateReading {
            id: 1,
            space_id: 1,
            measured_on: Date::new(2026, 5, 12).unwrap(),
            measured_time: None,
            temperature_c: temperature,
            humidity_pct: humidity,
            source: "manual".into(),
            source_ref: None,
        }
    }

    #[test]
    fn the_axis_label_does_not_convert_a_display_value_twice() {
        assert_eq!(
            display_value(Metric::Temperature, UnitPreference::Imperial, 10.0),
            "50 °F"
        );
        assert_eq!(
            display_value(Metric::Temperature, UnitPreference::Metric, 12.34),
            "12.3 °C"
        );
        assert_eq!(
            display_value(Metric::Rain, UnitPreference::Imperial, 1.0),
            "0.04 in"
        );
    }

    fn reference(opt_min: f64, opt_max: f64) -> Reference {
        Reference {
            opt_min: Some(opt_min),
            opt_max: Some(opt_max),
            survival_min: None,
            survival_max: None,
        }
    }

    #[test]
    fn guide_spans_fill_gaps_with_the_default_and_merge_neighbours() {
        let start = Date::new(2026, 5, 1).unwrap();
        let end = Date::new(2026, 5, 10).unwrap();
        let default = reference(10.0, 20.0);
        let seedling = reference(15.0, 25.0);

        let spans = guide_spans(
            start,
            end,
            Some(default),
            &[
                (
                    Date::new(2026, 5, 3).unwrap(),
                    Date::new(2026, 5, 5).unwrap(),
                    Some(seedling),
                ),
                (
                    Date::new(2026, 5, 6).unwrap(),
                    Date::new(2026, 5, 7).unwrap(),
                    Some(seedling),
                ),
            ],
        );

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].reference, default);
        assert_eq!(spans[1].reference, seedling);
        assert_eq!(spans[2].reference, default);
    }

    #[test]
    fn a_stage_without_a_reference_leaves_a_gap() {
        let start = Date::new(2026, 5, 1).unwrap();
        let end = Date::new(2026, 5, 5).unwrap();
        let default = reference(10.0, 20.0);

        let spans = guide_spans(
            start,
            end,
            Some(default),
            &[(
                Date::new(2026, 5, 2).unwrap(),
                Date::new(2026, 5, 3).unwrap(),
                None,
            )],
        );

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].start, start);
        assert_eq!(spans[1].end, end);
    }

    #[test]
    fn the_derived_day_average_keeps_the_night_out() {
        assert_eq!(derived_day(Some(12.0), Some(8.0), Some(8.0)), Some(14.0));
        assert_eq!(derived_day(Some(12.0), None, Some(8.0)), Some(12.0));
        assert_eq!(derived_day(None, Some(8.0), Some(8.0)), None);
    }

    fn graph_data(
        range: Range,
        selected: Option<Date>,
        grow: Option<(Date, Option<Date>)>,
        weather_days: Vec<WeatherDay>,
        readings: Vec<ClimateReading>,
        anchor_selected: bool,
    ) -> ClimateData {
        let placements = if anchor_selected {
            vec![PlantingPlacement {
                id: 1,
                planting_id: 1,
                space_id: 1,
                started_on: Date::new(2000, 1, 1).unwrap(),
                ended_on: None,
                space_kind: "outdoor".into(),
                light_type: None,
                light_hours: None,
                light_start: None,
            }]
        } else {
            Vec::new()
        };
        ClimateData::new(
            weather_days,
            Vec::new(),
            readings,
            placements,
            Vec::new(),
            UnitPreference::Metric,
            Rc::new(Cell::new(selected)),
            Rc::new(Cell::new(range)),
            default_sources(),
            default_view(),
            Guides::default(),
            Vec::new(),
            grow,
            None,
            false,
            anchor_selected,
            None,
        )
    }

    fn weather_day(date: Date, min: f64, max: f64, mean: f64) -> WeatherDay {
        WeatherDay {
            latitude: 52.0,
            longitude: 6.0,
            date,
            temp_min_c: Some(min),
            temp_max_c: Some(max),
            temp_mean_c: Some(mean),
            humidity_mean_pct: Some(70.0),
            humidity_min_pct: Some(50.0),
            humidity_max_pct: Some(90.0),
            precipitation_mm: Some(0.0),
            precipitation_hours: None,
            weather_code: None,
            sunshine_hours: None,
            daylight_hours: None,
            radiation_sum_mj_m2: None,
            wind_max_kmh: None,
            wind_gust_max_kmh: None,
            et0_mm: None,
            sunrise: Some("08:00".into()),
            sunset: Some("18:00".into()),
            night_start: Some(format!("{}T18:00", date.add_days(-1).to_iso())),
            night_end: Some(format!("{}T08:00", date.to_iso())),
            night_temp_avg_c: Some(6.0),
            night_temp_min_c: Some(4.0),
            night_humidity_avg_pct: Some(85.0),
            night_hours: Some(14.0),
            night_complete: true,
        }
    }

    fn weather_hour(date: Date, hour: i64, temperature: f64) -> WeatherHour {
        WeatherHour {
            latitude: 52.0,
            longitude: 6.0,
            date,
            time: format!("{hour:02}:00"),
            minutes: hour * 60,
            temperature_c: Some(temperature),
            humidity_pct: Some(70.0),
            precipitation_mm: Some(0.0),
            weather_code: None,
            cloud_cover_pct: None,
            sunshine_hours: None,
            is_day: Some((8..=18).contains(&hour)),
        }
    }

    #[test]
    fn a_window_ends_on_the_selected_day() {
        let selected = Date::new(2026, 10, 16).unwrap();
        let day = graph_data(
            Range::Day,
            Some(selected),
            None,
            Vec::new(),
            Vec::new(),
            true,
        );
        assert_eq!(day.window(), (selected, selected));

        let week = graph_data(
            Range::Days(7),
            Some(selected),
            None,
            Vec::new(),
            Vec::new(),
            true,
        );
        assert_eq!(week.window(), (Date::new(2026, 10, 10).unwrap(), selected));
    }

    #[test]
    fn all_starts_at_the_first_real_data_not_years_before_it() {
        let selected = Date::new(2026, 10, 16).unwrap();
        let grow_start = Date::new(2025, 1, 1).unwrap();
        let day = weather_day(Date::new(2026, 3, 1).unwrap(), 5.0, 15.0, 10.0);
        let data = graph_data(
            Range::All,
            Some(selected),
            Some((grow_start, None)),
            vec![day.clone()],
            Vec::new(),
            true,
        );
        assert_eq!(data.window().0, day.date);

        let old_reading = reading(Some(12.0), None);
        let mut old_reading = old_reading;
        old_reading.measured_on = Date::new(2026, 2, 1).unwrap();
        let space = graph_data(
            Range::All,
            None,
            None,
            vec![day],
            vec![old_reading.clone()],
            false,
        );
        assert_eq!(space.window().0, old_reading.measured_on);
    }

    #[test]
    fn a_day_is_split_into_day_and_night_averages() {
        let date = Date::new(2026, 5, 12).unwrap();
        let day = weather_day(date, 8.0, 20.0, 14.0);
        let hours: Vec<WeatherHour> = (0..=23)
            .map(|hour| {
                let value = if (8..=18).contains(&hour) { 20.0 } else { 10.0 };
                weather_hour(date, hour, value)
            })
            .collect();
        let refs: Vec<&WeatherHour> = hours.iter().collect();
        let aggregate = aggregate_weather(&day, &refs, false);
        assert_eq!(aggregate.temperature.day, Some(20.0));
        assert_eq!(aggregate.temperature.night, Some(10.0));
        assert_eq!(aggregate.temperature.min, Some(8.0));
        assert_eq!(aggregate.temperature.max, Some(20.0));
        assert_eq!(aggregate.temperature.mean, Some(14.0));
    }

    #[test]
    fn today_uses_the_passed_hours_not_the_daily_forecast() {
        let date = Date::new(2026, 5, 12).unwrap();
        // The daily row is a forecast: 8–20 °C. Only two daylight hours have passed (16 and 17 °C).
        let day = weather_day(date, 8.0, 20.0, 14.0);
        let hours: Vec<WeatherHour> = (8..=9)
            .map(|hour| weather_hour(date, hour, 16.0 + (hour - 8) as f64))
            .collect();
        let refs: Vec<&WeatherHour> = hours.iter().collect();
        let aggregate = aggregate_weather(&day, &refs, true);
        assert_eq!(aggregate.temperature.min, Some(16.0));
        assert_eq!(aggregate.temperature.max, Some(17.0));
        assert!(aggregate.temperature.day.is_some());

        // With no passed hour there is no value at all: the daily mean would be a forecast.
        let empty = aggregate_weather(&day, &[], true);
        assert_eq!(empty.temperature.min, None);
        assert_eq!(empty.temperature.max, None);
        assert_eq!(empty.temperature.day, None);
        assert_eq!(empty.temperature.night, Some(6.0)); // last night did happen
    }

    #[test]
    fn an_hourly_reading_is_placed_by_its_minutes() {
        let mut row = reading(Some(12.0), None);
        row.measured_time = Some("06:30".into());
        assert_eq!(reading_minutes(&row), 390);
        row.measured_time = None;
        assert_eq!(reading_minutes(&row), 720);
    }

    #[test]
    fn hiding_the_only_source_makes_the_graph_empty() {
        let date = Date::new(2026, 5, 12).unwrap();
        let data = graph_data(
            Range::Day,
            Some(date),
            None,
            vec![weather_day(date, 8.0, 16.0, 12.0)],
            Vec::new(),
            true,
        );

        assert!(data.has_data(Metric::Temperature, date, date));
        assert!(data.has_visible_data(Metric::Temperature, date, date));

        data.sources.set(Sources {
            weather: false,
            sensor: true,
            manual: true,
        });

        assert!(data.has_data(Metric::Temperature, date, date));
        assert!(!data.has_visible_data(Metric::Temperature, date, date));
    }

    #[test]
    fn an_indoor_planting_without_readings_has_no_graphs() {
        let date = Date::new(2026, 5, 12).unwrap();
        let mut data = graph_data(
            Range::Day,
            Some(date),
            None,
            vec![weather_day(date, 8.0, 16.0, 12.0)],
            Vec::new(),
            true,
        );
        data.placements[0].space_kind = "indoor".into();

        assert!(kinds_with_data(&data).is_empty());
    }

    #[test]
    fn an_indoor_planting_with_one_reading_shows_only_that_metric() {
        let date = Date::new(2026, 5, 12).unwrap();
        let mut data = graph_data(
            Range::Day,
            Some(date),
            None,
            vec![weather_day(date, 8.0, 16.0, 12.0)],
            vec![reading(Some(12.0), None)],
            true,
        );
        data.placements[0].space_kind = "indoor".into();

        assert_eq!(kinds_with_data(&data), vec![Metric::Temperature]);
    }

    #[test]
    fn an_outdoor_planting_offers_the_weather_metrics_with_data() {
        let date = Date::new(2026, 5, 12).unwrap();
        let mut day = weather_day(date, 8.0, 16.0, 12.0);
        day.sunshine_hours = Some(4.0);
        let data = graph_data(Range::Day, Some(date), None, vec![day], Vec::new(), true);

        let kinds = kinds_with_data(&data);
        assert!(kinds.contains(&Metric::Temperature));
        assert!(kinds.contains(&Metric::Humidity));
        assert!(kinds.contains(&Metric::Rain));
        assert!(kinds.contains(&Metric::Sunshine));
    }
}
