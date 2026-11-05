//! Weather-model summaries for the space page and the planting page.
//!
//! These use `weather_current` and `weather_daily`, never the local `climate_readings`. That is
//! deliberate: a model value and a hand or sensor measurement are different facts.

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::calendar::Date;
use crate::climate::ClimateReading;
use crate::clock::today;
use crate::planting::Space;
use crate::store::Store;
use crate::units::UnitPreference;
use crate::weather::{self, WeatherCurrent, WeatherDay};

const SUMMARY_DAYS: i64 = 7;

pub(super) fn section(store: &Store, space: &Space, units: UnitPreference) -> Option<gtk::Widget> {
    if space.kind != "outdoor" {
        return None;
    }

    let (current, days) = load(store);
    if current.is_none() && days.is_empty() && !crate::online::enabled(store) {
        return None;
    }

    let group = adw::PreferencesGroup::new();
    group.set_title("Weather");
    group.set_description(Some(
        "Open-Meteo model data for the location in Settings, not a measurement in this space.",
    ));

    if let Some(current) = &current {
        let row = adw::ActionRow::builder()
            .title(format!(
                "Now · {}",
                weather::condition_label(current.weather_code)
            ))
            .subtitle(current_line(current, units))
            .build();
        row.add_prefix(&weather_icon(current.weather_code));
        row.set_activatable(false);
        row.set_focusable(false);
        group.add(&row);
    } else {
        let row = adw::ActionRow::builder()
            .title("No weather data yet")
            .subtitle(empty_hint(store))
            .build();
        row.set_activatable(false);
        row.set_focusable(false);
        group.add(&row);
    }

    if crate::online::enabled(store)
        && let Some(error) = weather::last_error(store)
    {
        let row = adw::ActionRow::builder()
            .title("Last sync failed")
            .subtitle(error)
            .build();
        row.set_activatable(false);
        row.set_focusable(false);
        group.add(&row);
    }

    if !days.is_empty() {
        let row = adw::ActionRow::builder()
            .title(format!("Last {SUMMARY_DAYS} days"))
            .subtitle(days_line(&days, units))
            .build();
        row.set_activatable(false);
        row.set_focusable(false);
        group.add(&row);
    }

    if let Some(night) = last_night(&days) {
        let row = adw::ActionRow::builder()
            .title("Last night")
            .subtitle(night_line(night, units))
            .build();
        row.set_activatable(false);
        row.set_focusable(false);
        group.add(&row);
    }

    let section = gtk::Box::new(gtk::Orientation::Vertical, 12);
    section.append(&group);
    Some(section.upcast())
}

pub(super) fn card(store: &Store, space: &Space, units: UnitPreference) -> Option<gtk::Widget> {
    if space.kind != "outdoor" {
        return None;
    }

    let (current, days) = load(store);
    if current.is_none() && days.is_empty() && !crate::online::enabled(store) {
        return None;
    }

    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    card.add_css_class("omagrow-diary-card");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("Weather"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("heading");

    let place = gtk::Label::new(space.display_name());
    place.add_css_class("dim-label");
    place.add_css_class("omagrow-axis");

    header.append(&title);
    header.append(&place);
    card.append(&header);

    if let Some(current) = &current {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.append(&weather_icon(current.weather_code));

        let label = gtk::Label::new(Some(&format!(
            "{} · {}",
            weather::condition_label(current.weather_code),
            current_line(current, units)
        )));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.add_css_class("dim-label");
        line.append(&label);
        card.append(&line);
    } else {
        let line = gtk::Label::new(Some("Waiting for the first weather sync."));
        line.set_xalign(0.0);
        line.set_wrap(true);
        line.add_css_class("dim-label");
        card.append(&line);
    }

    if crate::online::enabled(store)
        && let Some(error) = weather::last_error(store)
    {
        let line = gtk::Label::new(Some(&format!("Last sync failed: {error}")));
        line.set_xalign(0.0);
        line.set_wrap(true);
        line.add_css_class("dim-label");
        card.append(&line);
    }

    if !days.is_empty() {
        let line = gtk::Label::new(Some(&days_line(&days, units)));
        line.set_xalign(0.0);
        line.set_wrap(true);
        line.add_css_class("dim-label");
        card.append(&line);
    }

    if let Some(night) = last_night(&days) {
        let line = gtk::Label::new(Some(&night_line(night, units)));
        line.set_xalign(0.0);
        line.set_wrap(true);
        card.append(&line);
    }

    let source = gtk::Label::new(Some(&format!("{} model", weather::PROVIDER)));
    source.set_xalign(0.0);
    source.add_css_class("dim-label");
    source.add_css_class("omagrow-axis");
    card.append(&source);

    Some(card.upcast())
}

/// The weather of one day on a planting page. Today keeps the live view; a past day shows its
/// stored row and the local readings of that day; a future day has nothing stored and says so
/// instead of inventing a forecast.
pub(super) fn day_card(
    store: &Store,
    space: &Space,
    units: UnitPreference,
    date: Date,
    day: Option<&WeatherDay>,
    readings: &[ClimateReading],
) -> Option<gtk::Widget> {
    if space.kind != "outdoor" {
        return None;
    }

    let today = today();
    if date == today {
        return card(store, space, units);
    }

    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    card.add_css_class("omagrow-diary-card");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("Weather"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("heading");

    let when = gtk::Label::new(Some(&date.format_day_month(today.year)));
    when.add_css_class("dim-label");
    when.add_css_class("omagrow-axis");

    header.append(&title);
    header.append(&when);
    card.append(&header);

    if date > today {
        let line = gtk::Label::new(Some("No weather stored for a future day."));
        line.set_xalign(0.0);
        line.set_wrap(true);
        line.add_css_class("dim-label");
        card.append(&line);
    } else if let Some(day) = day {
        let condition = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        condition.append(&weather_icon(day.weather_code));

        let text = gtk::Label::new(Some(weather::condition_label(day.weather_code)));
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.add_css_class("dim-label");
        condition.append(&text);
        card.append(&condition);

        let values = gtk::Label::new(Some(&day_values(day, units)));
        values.set_xalign(0.0);
        values.set_wrap(true);
        values.add_css_class("dim-label");
        card.append(&values);

        if day.night_temp_avg_c.is_some()
            || day.night_temp_min_c.is_some()
            || day.night_humidity_avg_pct.is_some()
        {
            let night = gtk::Label::new(Some(&format!("Night · {}", night_line(day, units))));
            night.set_xalign(0.0);
            night.set_wrap(true);
            night.add_css_class("dim-label");
            card.append(&night);
        }
    } else {
        let line = gtk::Label::new(Some(&match unavailable_hint(store) {
            Some(hint) => hint.to_string(),
            None => format!(
                "No weather stored for {}. Weather history goes back {} days.",
                date.format_day_month(today.year),
                weather::HISTORY_DAYS
            ),
        }));
        line.set_xalign(0.0);
        line.set_wrap(true);
        line.add_css_class("dim-label");
        card.append(&line);
    }

    if !readings.is_empty() {
        let label = gtk::Label::new(Some("Local readings"));
        label.set_xalign(0.0);
        label.add_css_class("omagrow-axis");
        card.append(&label);

        for reading in readings {
            let line = gtk::Label::new(Some(&reading_line(reading, units)));
            line.set_xalign(0.0);
            line.set_wrap(true);
            card.append(&line);
        }
    }

    let source = gtk::Label::new(Some(&format!("{} model", weather::PROVIDER)));
    source.set_xalign(0.0);
    source.add_css_class("dim-label");
    source.add_css_class("omagrow-axis");
    card.append(&source);

    Some(card.upcast())
}

fn day_values(day: &WeatherDay, units: UnitPreference) -> String {
    let mut parts = Vec::new();

    if let (Some(min), Some(max)) = (day.temp_min_c, day.temp_max_c) {
        parts.push(format!(
            "{}–{}",
            temperature_number(units, min),
            temperature_text(units, max)
        ));
    } else if let Some(mean) = day.temp_mean_c {
        parts.push(temperature_text(units, mean));
    }

    if let Some(mean) = day.temp_mean_c {
        parts.push(format!("mean {}", temperature_text(units, mean)));
    }
    if let Some(rain) = day.precipitation_mm {
        parts.push(format!("{} rain", measure_text(units, "mm", rain)));
    }
    if let Some(sun) = day.sunshine_hours {
        parts.push(format!("{}\u{00a0}h sun", round_one(sun)));
    }
    if let Some(humidity) = day.humidity_mean_pct {
        parts.push(format!("{}% RH", round_one(humidity)));
    }

    if parts.is_empty() {
        "No daily values stored".to_string()
    } else {
        parts.join(" · ")
    }
}

fn reading_line(reading: &ClimateReading, units: UnitPreference) -> String {
    let mut parts = Vec::new();
    if let Some(time) = &reading.measured_time {
        parts.push(time.clone());
    }
    if let Some(temperature) = reading.temperature_c {
        parts.push(temperature_text(units, temperature));
    }
    if let Some(humidity) = reading.humidity_pct {
        parts.push(format!("{}% RH", round_one(humidity)));
    }
    parts.push(crate::climate::source_label(&reading.source).to_string());
    parts.join(" · ")
}

fn load(store: &Store) -> (Option<WeatherCurrent>, Vec<WeatherDay>) {
    let Some(location) = store.location().ok().flatten() else {
        return (None, Vec::new());
    };

    let current = store
        .weather_current_latest(location.latitude, location.longitude)
        .ok()
        .flatten();
    let days = store
        .weather_days(
            location.latitude,
            location.longitude,
            today().add_days(-(SUMMARY_DAYS - 1)),
        )
        .unwrap_or_default();

    (current, days)
}

/// Why there is no weather to show at all, when the reason is not the history window.
fn unavailable_hint(store: &Store) -> Option<&'static str> {
    if !crate::online::enabled(store) {
        Some("Open-Meteo is switched off in Settings.")
    } else if store.location().ok().flatten().is_none() {
        Some("Set a location in Settings; the next weather sync fills this section.")
    } else {
        None
    }
}

fn empty_hint(store: &Store) -> &'static str {
    unavailable_hint(store).unwrap_or("Waiting for the first weather sync.")
}

fn current_line(current: &WeatherCurrent, units: UnitPreference) -> String {
    let mut parts = Vec::new();

    if let Some(temperature) = current.temperature_c {
        parts.push(temperature_text(units, temperature));
    }
    if let Some(humidity) = current.humidity_pct {
        parts.push(format!("{}% RH", round_one(humidity)));
    }
    if let Some(precipitation) = current.precipitation_mm
        && precipitation > 0.0
    {
        parts.push(format!("{} rain", units.text("mm", precipitation)));
    }

    let mut line = parts.join(" · ");
    if !line.is_empty() {
        line.push_str(" · ");
    }
    line.push_str(&format!(
        "{} {}",
        current.observed_on.format_day_month(today().year),
        current.observed_time.as_deref().unwrap_or("")
    ));

    line.trim_end().to_string()
}

fn days_line(days: &[WeatherDay], units: UnitPreference) -> String {
    let rain: Vec<f64> = days.iter().filter_map(|day| day.precipitation_mm).collect();
    let sun: Vec<f64> = days.iter().filter_map(|day| day.sunshine_hours).collect();
    let min = days
        .iter()
        .filter_map(|day| day.temp_min_c)
        .reduce(f64::min);
    let max = days
        .iter()
        .filter_map(|day| day.temp_max_c)
        .reduce(f64::max);

    let mut parts = Vec::new();

    if !rain.is_empty() {
        parts.push(format!(
            "{} rain",
            measure_text(units, "mm", rain.iter().sum())
        ));
    }
    if !sun.is_empty() {
        parts.push(format!("{} h sun", round_one(sun.iter().sum())));
    }

    if let (Some(min), Some(max)) = (min, max) {
        parts.push(format!(
            "{}–{}",
            temperature_number(units, min),
            temperature_text(units, max)
        ));
    }

    if parts.is_empty() {
        "No daily values yet".to_string()
    } else {
        parts.join(" · ")
    }
}

fn night_line(day: &WeatherDay, units: UnitPreference) -> String {
    let mut parts = Vec::new();

    if let Some(average) = day.night_temp_avg_c {
        parts.push(format!("avg {}", temperature_text(units, average)));
    }
    if let Some(min) = day.night_temp_min_c {
        parts.push(format!("low {}", temperature_text(units, min)));
    }
    if let Some(humidity) = day.night_humidity_avg_pct {
        parts.push(format!("{}% RH", round_one(humidity)));
    }

    if parts.is_empty() {
        "No complete night yet".to_string()
    } else {
        let start = day.date.add_days(-1);
        let label = if start.month == day.date.month {
            format!(
                "{}–{} {}",
                start.day,
                day.date.day,
                crate::calendar::month_name(day.date.month)
            )
        } else {
            format!(
                "{}–{}",
                start.format_day_month(today().year),
                day.date.format_day_month(today().year)
            )
        };

        format!("{label} · {}", parts.join(" · "))
    }
}

fn last_night(days: &[WeatherDay]) -> Option<&WeatherDay> {
    days.iter().rev().find(|day| day.night_temp_avg_c.is_some())
}

fn weather_icon(code: Option<i64>) -> gtk::Image {
    let icon = gtk::Image::from_icon_name(weather::condition_icon(code));
    icon.set_pixel_size(18);
    icon.add_css_class("dim-label");
    icon
}

fn round_one(value: f64) -> String {
    let text = format!("{value:.1}");
    text.trim_end_matches(".0").to_string()
}

fn temperature_number(units: UnitPreference, value: f64) -> String {
    round_one(units.to_display("°C", value))
}

fn temperature_text(units: UnitPreference, value: f64) -> String {
    format!(
        "{}\u{00a0}{}",
        temperature_number(units, value),
        units.label("°C")
    )
}

fn measure_text(units: UnitPreference, unit: &str, value: f64) -> String {
    let displayed = units.to_display(unit, value);
    let number = if displayed > 0.0 && displayed < 0.1 {
        "<0.1".to_string()
    } else {
        round_one(displayed)
    };

    format!("{number}\u{00a0}{}", units.label(unit))
}
