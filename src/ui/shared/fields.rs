//! Form rows and small value helpers used by more than one screen.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::calendar::Date;
use crate::plant_data::format_number;
use crate::planting::{CONTAINERS, LIGHT_TYPES, SPACE_KINDS};
use crate::store::Store;

use super::graph_style::short_date as shared_short_date;

pub(in crate::ui) fn short_date(date: Date) -> String {
    shared_short_date(date, crate::clock::today())
}

pub(in crate::ui) fn combo_row(title: &str, labels: &[&str], selected: u32) -> adw::ComboRow {
    let model = gtk::StringList::new(labels);
    let row = adw::ComboRow::builder().title(title).model(&model).build();
    if !labels.is_empty() {
        row.set_selected(selected.min(labels.len() as u32 - 1));
    }
    row
}

pub(in crate::ui) fn entry_row(title: &str) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row
}

pub(in crate::ui) fn number_entry_row(title: &str, value: Option<f64>) -> adw::EntryRow {
    let row = entry_row(title);
    row.set_input_purpose(gtk::InputPurpose::Number);
    if let Some(value) = value {
        row.set_text(&format_number(value));
    }
    row
}

pub(in crate::ui) fn date_row(title: &str, date: Date) -> (adw::ActionRow, Rc<Cell<Date>>) {
    let value = Rc::new(Cell::new(date));

    let row = adw::ActionRow::new();
    row.set_title(title);

    let button = gtk::MenuButton::new();
    button.set_label(&short_date(date));
    button.add_css_class("flat");
    row.add_suffix(&button);

    let calendar = gtk::Calendar::new();
    if let Ok(start) = gtk::glib::DateTime::new(
        &gtk::glib::TimeZone::local(),
        date.year,
        date.month as i32,
        date.day as i32,
        12,
        0,
        0.0,
    ) {
        calendar.select_day(&start);
    }

    let popover = gtk::Popover::new();
    popover.set_child(Some(&calendar));
    popover.set_has_arrow(false);
    button.set_popover(Some(&popover));

    calendar.connect_day_selected({
        let value = value.clone();
        let button = button.clone();
        let popover = popover.clone();
        move |calendar| {
            let Some(date) = Date::new(
                calendar.year(),
                calendar.month() as u32 + 1,
                calendar.day() as u32,
            ) else {
                return;
            };

            value.set(date);
            button.set_label(&short_date(date));
            popover.popdown();
        }
    });

    (row, value)
}

pub(in crate::ui) fn update_light_rows(
    kind_row: &adw::ComboRow,
    light_type_row: &adw::ComboRow,
    light_hours_row: &adw::EntryRow,
    light_start_row: &adw::EntryRow,
) {
    let outdoor = kind_row.selected() == choice_index(SPACE_KINDS, "outdoor");
    let powered = matches!(
        light_type_value(light_type_row.selected()).as_deref(),
        Some("led" | "hps" | "mixed")
    );

    light_type_row.set_visible(!outdoor);
    light_hours_row.set_visible(!outdoor && powered);
    light_start_row.set_visible(!outdoor && powered);
}

pub(in crate::ui) fn light_start_text(minutes: Option<i64>) -> String {
    minutes
        .map(|minutes| format!("{:02}:{:02}", minutes / 60, minutes % 60))
        .unwrap_or_default()
}

pub(in crate::ui) fn parse_light_start(text: &str) -> Result<Option<i64>, String> {
    let time = crate::observation::parse_time(text)?;
    Ok(time.map(|time| {
        let (hours, minutes) = time
            .split_once(':')
            .expect("parse_time always returns HH:MM");
        hours.parse::<i64>().unwrap_or(0) * 60 + minutes.parse::<i64>().unwrap_or(0)
    }))
}

pub(in crate::ui) fn update_volume_row(container_row: &adw::ComboRow, volume_row: &adw::EntryRow) {
    let pot = CONTAINERS.get(container_row.selected() as usize) == Some(&"pot");
    volume_row.set_visible(pot);
}

pub(in crate::ui) fn choice_index(values: &[&str], value: &str) -> u32 {
    values.iter().position(|known| *known == value).unwrap_or(0) as u32
}

pub(in crate::ui) fn light_type_index(value: Option<&str>) -> u32 {
    value
        .and_then(|value| LIGHT_TYPES.iter().position(|known| *known == value))
        .map(|index| index as u32 + 1)
        .unwrap_or(0)
}

pub(in crate::ui) fn light_type_value(index: u32) -> Option<String> {
    if index == 0 {
        return None;
    }

    LIGHT_TYPES
        .get(index as usize - 1)
        .map(|value| (*value).to_string())
}

pub(in crate::ui) fn outcome_label(outcome: &str) -> &'static str {
    match outcome {
        "harvested" => "Harvested",
        "removed" => "Removed",
        "died" => "Died",
        "failed" => "Failed",
        _ => "Finished",
    }
}

pub(in crate::ui) fn start_kind_text(kind: &str) -> &'static str {
    match kind {
        "planted" => "Planted",
        "sown_indoor" => "Sown indoors",
        "sown_outdoor" => "Sown outdoors",
        "transplanted" => "Transplanted",
        _ => "Planted",
    }
}

pub(in crate::ui) fn space_kind_text(kind: &str) -> &'static str {
    match kind {
        "outdoor" => "Outdoor",
        "greenhouse" => "Greenhouse",
        "indoor" => "Indoor",
        _ => "Outdoor",
    }
}

pub(in crate::ui) fn light_type_text(light: &str) -> &'static str {
    match light {
        "natural" => "Natural light",
        "led" => "LED",
        "hps" => "HPS",
        "mixed" => "Mixed light",
        _ => "Light",
    }
}

pub(in crate::ui) fn clean_text(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

pub(in crate::ui) fn parse_optional_number(text: &str) -> std::result::Result<Option<f64>, String> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return Ok(None);
    }

    text.parse::<f64>()
        .map(Some)
        .map_err(|_| format!("“{text}” is not a number"))
}

pub(in crate::ui) fn profile_avatar(store: &Store, profile_id: i64, size: i32) -> gtk::Widget {
    super::avatar::widget(store, "profile", profile_id, size)
}

pub(in crate::ui) fn profile_subtitle(profile: &crate::store::Profile) -> Option<String> {
    species_subtitle(profile.species.as_deref(), profile.cultivar.as_deref())
}

pub(in crate::ui) fn species_subtitle(
    species: Option<&str>,
    cultivar: Option<&str>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(species) = species.map(str::trim).filter(|text| !text.is_empty()) {
        parts.push(species.to_string());
    }
    if let Some(cultivar) = cultivar.map(str::trim).filter(|text| !text.is_empty()) {
        parts.push(cultivar.to_string());
    }

    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub(in crate::ui) fn humanize(value: &str) -> String {
    let text = value.replace('_', " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
