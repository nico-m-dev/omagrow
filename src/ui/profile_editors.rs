//! The profile editors: identity, stages, requirements, timing and attributes.

use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::attribute_seeds::ATTRIBUTE_CATEGORIES;
use crate::calendar::Date;
use crate::changes;
use crate::plant_data::{
    Attribute, AttributeDefinition, DAY_PARTS, DETERMINACY, FOLIAGE, GROWTH_HABITS, LIFE_FORMS,
    LIFECYCLES, METRICS, PHOTOPERIOD_TYPES, ProfileDetails, Requirement, STAGE_KINDS,
    SUN_EXPOSURES, Stage, TIMING_ANCHORS, TIMING_KINDS, TimingRule, VALUE_TYPES, ValidationError,
    WATER_NEEDS, format_number, metric_label, metric_unit,
};
use crate::provenance;
use crate::store::Store;
use crate::units::UnitPreference;

use super::shared::fields::humanize;
use super::shared::provenance::provenance_control;
use super::shell::Guard;
use super::{page_header, profile_group, profile_view};

pub(super) fn metric_order(metric: &str) -> u8 {
    match metric {
        "air_temp" => 0,
        "rh" => 1,
        "soil_ph" => 2,
        "water_mm_week" | "water_l_week" => 3,
        "direct_sun_hours" | "photoperiod" | "light_dli" | "light_ppfd" => 4,
        _ => 5,
    }
}

pub(super) fn add_detail_row(
    group: &adw::PreferencesGroup,
    store: &Rc<Store>,
    profile_id: i64,
    fields: &[&str],
    title: &str,
    value: Option<String>,
) -> bool {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return false;
    };

    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(&value)
        .build();
    row.set_activatable(false);
    row.set_focusable(false);
    row.set_subtitle_lines(3);
    row.add_suffix(&provenance_control(
        store,
        "profile",
        profile_id,
        fields.iter().map(|field| (*field).to_string()).collect(),
    ));
    group.add(&row);
    true
}

pub(super) fn edit_row(title: &str, subtitle: &str, run: Rc<dyn Fn()>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    row.set_activatable(true);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row.connect_activated(move |_| run());
    row
}

pub(super) fn empty_row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    row.set_activatable(false);
    row.set_focusable(false);
    row
}

pub(super) fn stage_names(stages: &[Stage]) -> HashMap<i64, String> {
    stages
        .iter()
        .map(|stage| (stage.id, stage.label.clone()))
        .collect()
}

pub(super) fn attribute_definitions(store: &Store) -> HashMap<String, AttributeDefinition> {
    store
        .attribute_definitions()
        .unwrap_or_default()
        .into_iter()
        .map(|definition| (definition.key.clone(), definition))
        .collect()
}

/// A chip that opens the status menu. The chip itself is the affordance; confirming is never
/// hidden behind a menu that cannot be found.
pub(super) fn requirement_title(requirement: &Requirement) -> String {
    let metric = metric_label(&requirement.metric);
    match requirement.day_part.as_str() {
        "day" => format!("{metric} · day"),
        "night" => format!("{metric} · night"),
        _ => metric.to_string(),
    }
}

pub(super) fn requirement_range(requirement: &Requirement, pref: UnitPreference) -> Option<String> {
    let unit = metric_unit(&requirement.metric);
    let mut parts = Vec::new();

    if let Some(range) = format_range(requirement.min, requirement.max, unit, pref) {
        parts.push(range);
    }
    if let Some(range) = format_range(requirement.opt_min, requirement.opt_max, unit, pref) {
        parts.push(format!("optimum {range}"));
    }
    if let Some(range) = format_range(
        requirement.survival_min,
        requirement.survival_max,
        unit,
        pref,
    ) {
        parts.push(format!("survival {range}"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

pub(super) fn timing_order(kind: &str) -> u8 {
    match kind {
        "sow_indoor" => 10,
        "sow_outdoor" => 11,
        "transplant" => 20,
        "feed" => 30,
        "support" => 35,
        "prune" => 40,
        "thinning" => 45,
        "deadheading" => 50,
        "flowering" => 60,
        "bloom" => 61,
        "fruiting" => 70,
        "harvest" => 80,
        "succession" => 85,
        "repot" => 90,
        "winter_protect" => 95,
        "dormancy" => 96,
        _ => 100,
    }
}

pub(super) fn timing_title(rule: &TimingRule) -> String {
    timing_kind_label(&rule.kind)
}

fn timing_kind_label(kind: &str) -> String {
    match kind {
        "sow_indoor" => "Sow indoors".to_string(),
        "sow_outdoor" => "Sow outdoors".to_string(),
        "transplant" => "Transplant".to_string(),
        "harvest" => "Harvest".to_string(),
        "prune" => "Prune".to_string(),
        "feed" => "Feed".to_string(),
        "support" => "Support".to_string(),
        "repot" => "Repot".to_string(),
        "winter_protect" => "Winter protection".to_string(),
        "flowering" => "Flowering".to_string(),
        "fruiting" => "Fruiting".to_string(),
        "dormancy" => "Dormancy".to_string(),
        "succession" => "Succession sowing".to_string(),
        "thinning" => "Thinning".to_string(),
        "deadheading" => "Deadheading".to_string(),
        "bloom" => "Bloom".to_string(),
        other => humanize(other),
    }
}

fn timing_anchor_label(anchor: &str) -> String {
    match anchor {
        "last_frost" => "Last frost".to_string(),
        "first_frost" => "First frost".to_string(),
        "sowing" => "Sowing".to_string(),
        "transplant" => "Transplant".to_string(),
        "stage_start" => "Stage start".to_string(),
        "day_length" => "Day length".to_string(),
        "gdd" => "Growing degree days".to_string(),
        "calendar" => "Calendar".to_string(),
        other => humanize(other),
    }
}

pub(super) fn frost_range(start: Date, end: Date, current_year: i32) -> String {
    if start == end {
        format!("≈ {}", start.format_day_month(current_year))
    } else {
        format!(
            "≈ {} – {}",
            start.format_day_month(current_year),
            end.format_day_month(current_year)
        )
    }
}

pub(super) fn timing_subtitle(rule: &TimingRule, names: &HashMap<i64, String>) -> String {
    let mut parts = Vec::new();

    if let Some(stage_id) = rule.stage_id {
        parts.push(format!(
            "Stage: {}",
            names.get(&stage_id).map(String::as_str).unwrap_or("stage")
        ));
    }

    let detail = match rule.anchor.as_str() {
        "calendar" => match (rule.month_start, rule.month_end) {
            (Some(start), Some(end)) => format!("{}–{}", month_name(start), month_name(end)),
            _ => "Calendar".to_string(),
        },
        "day_length" => match rule.day_length_hours {
            Some(hours) => format!("at {} day length", format_hours(hours)),
            None => "Day length".to_string(),
        },
        "gdd" => {
            let target = match rule.gdd_target {
                Some(target) => format!("at {} GDD", format_number(target)),
                None => "GDD".to_string(),
            };
            match rule.gdd_base_c {
                Some(base) => format!("{target} (base {} °C)", format_number(base)),
                None => target,
            }
        }
        _ => timing_offset(rule, &timing_anchor(rule)),
    };

    if !detail.is_empty() {
        parts.push(detail);
    }
    if let Some(days) = rule.repeat_days {
        parts.push(format!("every {days} days"));
    }

    if parts.is_empty() {
        "No details".to_string()
    } else {
        parts.join(" · ")
    }
}

fn timing_anchor(rule: &TimingRule) -> String {
    match rule.anchor.as_str() {
        "last_frost" => "the last frost".to_string(),
        "first_frost" => "the first frost".to_string(),
        "sowing" => "sowing".to_string(),
        "transplant" => "transplanting".to_string(),
        "stage_start" => "the start of the stage".to_string(),
        "day_length" => "day length".to_string(),
        "gdd" => "growing degree days".to_string(),
        "calendar" => "the calendar".to_string(),
        other => humanize(other).to_lowercase(),
    }
}

fn timing_offset(rule: &TimingRule, anchor: &str) -> String {
    match (rule.offset_min_days, rule.offset_max_days) {
        (Some(min), Some(max)) if min < 0 && max < 0 => format!(
            "{}–{} days before {anchor}",
            format_number((-max) as f64),
            format_number((-min) as f64)
        ),
        (Some(min), Some(max)) if min >= 0 && max >= 0 => {
            format!("{min}–{max} days after {anchor}")
        }
        (Some(min), Some(max)) => format!("{min} to {max} days relative to {anchor}"),
        (Some(min), None) => format!("{min} days relative to {anchor}"),
        (None, Some(max)) => format!("up to {max} days relative to {anchor}"),
        (None, None) => String::new(),
    }
}

pub(super) fn attribute_value(
    attribute: &Attribute,
    definition: Option<&AttributeDefinition>,
    pref: UnitPreference,
) -> Option<String> {
    let unit = definition
        .and_then(|definition| definition.unit.as_deref())
        .unwrap_or("");
    let pref = if definition
        .map(|definition| definition.is_custom)
        .unwrap_or(false)
    {
        UnitPreference::Metric
    } else {
        pref
    };

    if let Some(value) = attribute.value_num {
        Some(if attribute.key == "dif_c" {
            format!("{} {}", pref.delta_number(unit, value), pref.label(unit))
        } else {
            pref.text(unit, value)
        })
    } else if let Some(value) = attribute.value_text.as_deref() {
        Some(
            if definition
                .map(|definition| definition.value_type == "enum")
                .unwrap_or(false)
            {
                humanize(value)
            } else {
                value.to_string()
            },
        )
    } else {
        attribute.value_bool.map(|value| {
            if value {
                "Yes".to_string()
            } else {
                "No".to_string()
            }
        })
    }
}

pub(super) fn photoperiod_value(details: &ProfileDetails) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(kind) = details.photoperiod_type.as_deref() {
        parts.push(humanize(kind));
    }
    match details.photoperiod_obligate {
        Some(true) => parts.push("obligate".to_string()),
        Some(false) => parts.push("facultative".to_string()),
        None => {}
    }
    if let Some(hours) = details.critical_day_length_hours {
        parts.push(format!("{} critical", format_hours(hours)));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

pub(super) fn container_value(details: &ProfileDetails, pref: UnitPreference) -> Option<String> {
    let mut parts = Vec::new();

    match details.container_suitable {
        Some(true) => parts.push("Suitable".to_string()),
        Some(false) => parts.push("Not suitable".to_string()),
        None => {}
    }
    if let Some(range) = format_range(
        details.min_container_volume_l_min,
        details.min_container_volume_l_max,
        "L",
        pref,
    ) {
        parts.push(range);
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

pub(super) fn format_range(
    min: Option<f64>,
    max: Option<f64>,
    unit: &str,
    pref: UnitPreference,
) -> Option<String> {
    pref.range(min, max, unit)
}

fn format_hours(hours: f64) -> String {
    format!("{} h", format_number(hours))
}

pub(super) fn month_name(month: i64) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Unknown month",
    }
}

/// Writes the row, its provenance and its history row in one transaction.
fn write_with_history<T>(
    store: &Store,
    write: impl FnOnce(&Store) -> crate::store::Result<T>,
    record: impl FnOnce(&Store, &T) -> crate::store::Result<()>,
) -> crate::store::Result<T> {
    store.transaction(|store| {
        let value = write(store)?;
        record(store, &value)?;
        Ok(value)
    })
}

fn record_row_change(
    store: &Store,
    profile_id: i64,
    entity_type: &str,
    entity_id: i64,
    old: Option<serde_json::Value>,
    new: Option<serde_json::Value>,
) -> crate::store::Result<()> {
    // Pressing Save on an untouched form writes the same row back. That is not a change: it must
    // not turn `not_applicable` into `confirmed`, replace an agent's source, or leave no trace.
    if old == new {
        return Ok(());
    }

    let has_value = new.is_some();
    changes::record(
        store,
        &changes::NewChange {
            profile_id,
            entity_type,
            entity_id,
            field: "*",
            old,
            new,
            actor: changes::ACTOR_USER,
            source_type: Some("user"),
            source_ref: None,
        },
    )?;

    if has_value {
        provenance::mark_user(store, entity_type, entity_id, &["*"])?;
    } else {
        store.delete_provenance(entity_type, entity_id, "*")?;
    }

    Ok(())
}

/// Records the fields that actually differ between two profiles, one history row each.
fn record_detail_changes(
    store: &Store,
    profile_id: i64,
    before: Option<&ProfileDetails>,
    after: &ProfileDetails,
) -> crate::store::Result<()> {
    let Some(before) = before else {
        return Ok(());
    };

    for field in changes::DETAIL_FIELDS {
        let field = *field;
        let old = changes::detail(before, field);
        let new = changes::detail(after, field);
        if old == new {
            continue;
        }

        let has_value = !matches!(new, serde_json::Value::Null);
        changes::record(
            store,
            &changes::NewChange {
                profile_id,
                entity_type: "profile",
                entity_id: profile_id,
                field,
                old: Some(old),
                new: Some(new),
                actor: changes::ACTOR_USER,
                source_type: Some("user"),
                source_ref: None,
            },
        )?;

        // Identity has its own history row, but no provenance chip on the profile page.
        if matches!(field, "name" | "species" | "cultivar") {
            continue;
        }

        if has_value {
            provenance::mark_user(store, "profile", profile_id, &[field])?;
        } else {
            store.delete_provenance("profile", profile_id, field)?;
        }
    }

    Ok(())
}

/// The five newest changes, with the value in the row, and a way into the full history.
pub(super) fn profile_edit(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    id: i64,
) -> adw::NavigationPage {
    let profile = store.profile(id).ok().flatten();
    let details = store.profile_details(id).ok().flatten();

    let (identity_group, name_row, latin_row, cultivar_row) = profile_group(
        profile.as_ref(),
        "The name is required. Saving marks the values you changed as confirmed by you.",
    );
    let pref = UnitPreference::load(store);
    let (classification_group, classification) = classification_fields(details.as_ref());
    let (growing_group, growing) = growing_fields(details.as_ref(), pref);
    let cycle = edit_cycle_group(store, nav, id);
    let attributes = attributes_section(store, nav, id);

    let overlay = adw::ToastOverlay::new();

    let preferences = adw::PreferencesPage::new();
    preferences.add(&identity_group);
    preferences.add(&classification_group);
    preferences.add(&growing_group);
    preferences.add(&cycle);
    preferences.add(&attributes);

    let banner = adw::Banner::new(
        "An agent changed this profile while you were editing. Reloading discards unsaved edits.",
    );
    banner.set_button_label(Some("Reload"));
    banner.set_revealed(false);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&banner);
    content.append(&preferences);

    let guard = Guard::new(nav);
    let header = guard.header("Edit plant profile");

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.set_sensitive(!name_row.text().trim().is_empty());
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    overlay.set_child(Some(&content));

    let baseline = store
        .changes(id, 1)
        .ok()
        .and_then(|rows| rows.first().map(|change| change.id))
        .unwrap_or(0);
    gtk::glib::timeout_add_seconds_local(2, {
        let store = store.clone();
        let banner = banner.clone();
        move || {
            if banner.root().is_none() {
                return gtk::glib::ControlFlow::Break;
            }

            let changed_by_agent = store
                .changes(id, 20)
                .map(|rows| {
                    rows.into_iter()
                        .any(|change| change.id > baseline && change.actor == changes::ACTOR_AGENT)
                })
                .unwrap_or(false);
            if changed_by_agent {
                banner.set_revealed(true);
            }
            gtk::glib::ControlFlow::Continue
        }
    });

    banner.connect_button_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let guard = guard.clone();
        move |_| {
            let store = store.clone();
            let nav = nav.clone();
            let root_page = root_page.clone();
            guard.ask(Rc::new(move || {
                if let Some(view) = profile_view(&store, &nav, &root_page, id, Some("Reloaded")) {
                    nav.replace(&[root_page.clone(), view]);
                }
            }));
        }
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    name_row.connect_changed({
        let save = save.clone();
        move |row| save.set_sensitive(!row.text().trim().is_empty())
    });

    let do_save = {
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let name_row = name_row.clone();
        let latin_row = latin_row.clone();
        let cultivar_row = cultivar_row.clone();
        let overlay = overlay.clone();
        let classification = classification.clone();
        let growing = growing.clone();

        Rc::new(move || {
            let name = name_row.text().trim().to_string();
            if name.is_empty() {
                return;
            }

            let latin = latin_row.text().trim().to_string();
            let cultivar = cultivar_row.text().trim().to_string();

            let details = match collect_profile_details(
                id,
                &name,
                &latin,
                &cultivar,
                &classification,
                &growing,
            ) {
                Ok(details) => details,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };

            let before = match store.profile_details(id) {
                Ok(details) => details,
                Err(error) => {
                    overlay.add_toast(adw::Toast::new(&format!(
                        "Could not read the profile: {error}"
                    )));
                    return;
                }
            };

            let saved = store.transaction(|store| {
                store.update_profile_details(&details)?;
                store.update_profile(id, &name, Some(&latin), Some(&cultivar))?;
                record_detail_changes(store, id, before.as_ref(), &details)
            });

            if let Err(error) = saved {
                eprintln!("could not save plant profile {id}: {error}");
                let message =
                    save_error_message(error.as_ref(), "Could not save the plant profile");
                overlay.add_toast(adw::Toast::new(&message));
                return;
            }

            match profile_view(&store, &nav, &root_page, id, Some("Saved")) {
                Some(view) => nav.replace(&[root_page.clone(), view]),
                None => {
                    nav.pop();
                }
            }
        })
    };

    save.connect_clicked({
        let do_save = do_save.clone();
        move |_| do_save()
    });

    for row in [&name_row, &latin_row, &cultivar_row] {
        row.connect_entry_activated({
            let do_save = do_save.clone();
            move |_| do_save()
        });
    }

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    toolbar.connect_map({
        let name_row = name_row.clone();
        move |_| {
            name_row.grab_focus();
        }
    });

    let page = adw::NavigationPage::new(&toolbar, "Edit plant profile");
    guard.attach(&page, &toolbar);
    page
}

#[derive(Clone)]
struct ClassificationFields {
    family: adw::EntryRow,
    genus: adw::EntryRow,
    lifecycle: adw::ComboRow,
    life_form: adw::ComboRow,
    foliage: adw::ComboRow,
    determinacy: adw::ComboRow,
    growth_habit: adw::ComboRow,
}

#[derive(Clone)]
struct GrowingFields {
    light: adw::ComboRow,
    photoperiod: adw::ComboRow,
    photoperiod_obligate: adw::ComboRow,
    critical_day_length: adw::EntryRow,
    water_need: adw::ComboRow,
    container: adw::ComboRow,
    min_container_volume: UnitField,
    max_container_volume: UnitField,
    days_to_maturity_min: adw::EntryRow,
    days_to_maturity_max: adw::EntryRow,
    notes: adw::EntryRow,
}

fn classification_fields(
    details: Option<&ProfileDetails>,
) -> (adw::PreferencesGroup, ClassificationFields) {
    let group = adw::PreferencesGroup::new();
    group.set_title("Classification");
    group.set_description(Some(
        "What the plant is. Leave fields empty when they do not apply or you do not know.",
    ));

    let family = entry_row(
        "Family",
        details.and_then(|details| details.family.as_deref()),
    );
    let genus = entry_row(
        "Genus",
        details.and_then(|details| details.genus.as_deref()),
    );
    let lifecycle = choice_row(
        "Lifecycle",
        LIFECYCLES,
        details.and_then(|details| details.lifecycle.as_deref()),
    );
    let life_form = choice_row(
        "Life form",
        LIFE_FORMS,
        details.and_then(|details| details.life_form.as_deref()),
    );
    let foliage = choice_row(
        "Foliage",
        FOLIAGE,
        details.and_then(|details| details.foliage.as_deref()),
    );
    let determinacy = choice_row(
        "Determinacy",
        DETERMINACY,
        details.and_then(|details| details.determinacy.as_deref()),
    );
    let growth_habit = choice_row(
        "Growth habit",
        GROWTH_HABITS,
        details.and_then(|details| details.growth_habit.as_deref()),
    );

    group.add(&family);
    group.add(&genus);
    group.add(&lifecycle);
    group.add(&life_form);
    group.add(&foliage);
    group.add(&determinacy);
    group.add(&growth_habit);

    (
        group,
        ClassificationFields {
            family,
            genus,
            lifecycle,
            life_form,
            foliage,
            determinacy,
            growth_habit,
        },
    )
}

fn growing_fields(
    details: Option<&ProfileDetails>,
    pref: UnitPreference,
) -> (adw::PreferencesGroup, GrowingFields) {
    let group = adw::PreferencesGroup::new();
    group.set_title("Growing conditions");
    group.set_description(Some(
        "Core environment and timing traits. Stage-specific ranges live under Stages.",
    ));

    let light = choice_row(
        "Light",
        SUN_EXPOSURES,
        details.and_then(|details| details.sun_exposure.as_deref()),
    );
    let photoperiod = choice_row(
        "Photoperiod",
        PHOTOPERIOD_TYPES,
        details.and_then(|details| details.photoperiod_type.as_deref()),
    );
    let photoperiod_obligate =
        obligate_choice_row(details.and_then(|details| details.photoperiod_obligate));
    let critical_day_length = number_entry_row(
        "Critical day length (hours)",
        details.and_then(|details| details.critical_day_length_hours),
    );
    let water_need = choice_row(
        "Water need",
        WATER_NEEDS,
        details.and_then(|details| details.water_need.as_deref()),
    );
    let container = container_choice_row(details.and_then(|details| details.container_suitable));
    let min_container_volume = UnitField::new(
        "Minimum container volume",
        details.and_then(|details| details.min_container_volume_l_min),
        "L",
        pref,
    );
    let max_container_volume = UnitField::new(
        "Maximum container volume",
        details.and_then(|details| details.min_container_volume_l_max),
        "L",
        pref,
    );
    let days_to_maturity_min = number_entry_row(
        "Days to maturity, minimum",
        details.and_then(|details| details.days_to_maturity_min),
    );
    let days_to_maturity_max = number_entry_row(
        "Days to maturity, maximum",
        details.and_then(|details| details.days_to_maturity_max),
    );
    let notes = entry_row(
        "Notes",
        details.and_then(|details| details.notes.as_deref()),
    );

    group.add(&light);
    group.add(&photoperiod);
    group.add(&photoperiod_obligate);
    group.add(&critical_day_length);
    group.add(&water_need);
    group.add(&container);
    group.add(&min_container_volume.row);
    group.add(&max_container_volume.row);
    group.add(&days_to_maturity_min);
    group.add(&days_to_maturity_max);
    group.add(&notes);

    (
        group,
        GrowingFields {
            light,
            photoperiod,
            photoperiod_obligate,
            critical_day_length,
            water_need,
            container,
            min_container_volume,
            max_container_volume,
            days_to_maturity_min,
            days_to_maturity_max,
            notes,
        },
    )
}

fn edit_cycle_group(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Cycle");
    group.set_description(Some(
        "These rows save immediately. Reorder stages with the arrows; requirements and timing stay attached to their stage.",
    ));

    let stages = adw::ActionRow::builder()
        .title("Stages")
        .subtitle("Add, edit or remove the growing cycle.")
        .build();
    stages.set_activatable(true);
    stages.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    stages.connect_activated({
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            nav.push(&stages_page(&store, &nav, profile_id));
        }
    });
    group.add(&stages);

    let requirements = adw::ActionRow::builder()
        .title("Requirements")
        .subtitle("Ranges per stage and day part.")
        .build();
    requirements.set_activatable(true);
    requirements.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    requirements.connect_activated({
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            nav.push(&requirements_page(&store, &nav, profile_id));
        }
    });
    group.add(&requirements);

    let timing = adw::ActionRow::builder()
        .title("Timing")
        .subtitle("Sowing, harvest, pruning and other relative rules.")
        .build();
    timing.set_activatable(true);
    timing.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    timing.connect_activated({
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            nav.push(&timing_page(&store, &nav, profile_id));
        }
    });
    group.add(&timing);

    group
}

fn attributes_section(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Long tail");
    group.set_description(Some(
        "The long tail: propagation, pruning, harvest, pests and other details.",
    ));

    let row = adw::ActionRow::builder()
        .title("Attributes")
        .subtitle("Manage the long-tail values for this profile.")
        .build();
    row.set_activatable(true);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row.connect_activated({
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            nav.push(&attributes_page(&store, &nav, profile_id));
        }
    });

    group.add(&row);
    group
}

pub(super) fn stages_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) -> adw::NavigationPage {
    let on_add: Rc<dyn Fn()> = {
        let store = store.clone();
        let nav = nav.clone();
        Rc::new(move || nav.push(&stage_form(&store, &nav, profile_id, None)))
    };
    let refresh: Rc<dyn Fn(&gtk::ListBox)> = {
        let store = store.clone();
        let nav = nav.clone();
        Rc::new(move |list| refresh_stages(list, &store, &nav, profile_id))
    };
    super::shared::editor_list::page("Stages", "Add stage", on_add, refresh)
}

fn refresh_stages(
    list: &gtk::ListBox,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let stages = match store.stages(profile_id) {
        Ok(stages) => stages,
        Err(error) => {
            eprintln!("could not read stages for profile {profile_id}: {error}");
            Vec::new()
        }
    };

    if stages.is_empty() {
        list.append(&empty_row(
            "No stages yet",
            "Add the first stage of the growing cycle.",
        ));
        return;
    }

    let perennial = store
        .profile_details(profile_id)
        .ok()
        .flatten()
        .and_then(|profile| profile.lifecycle)
        .as_deref()
        == Some("perennial");

    for index in 0..stages.len() {
        list.append(&stage_row(
            list, store, nav, profile_id, &stages, index, perennial,
        ));
    }
}

fn stage_row(
    list: &gtk::ListBox,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    index: usize,
    perennial: bool,
) -> adw::ActionRow {
    let stage = &stages[index];
    let number = index + 1;
    let count = stages.len();
    let stage_id = stage.id;
    let pref = UnitPreference::load(store);
    let mut subtitle = vec![format!("Stage {number}"), humanize(&stage.kind)];
    if let Some(range) = format_range(
        stage.duration_days_min,
        stage.duration_days_max,
        "days",
        pref,
    ) {
        subtitle.push(range);
    }
    if perennial {
        subtitle.push(if stage.repeats_each_year() {
            "repeats yearly".to_string()
        } else {
            "first cycle only".to_string()
        });
    }

    let row = adw::ActionRow::builder()
        .title(&stage.label)
        .subtitle(subtitle.join(" · "))
        .build();

    let up = gtk::Button::from_icon_name("go-up-symbolic");
    up.set_tooltip_text(Some("Move this stage up"));
    up.add_css_class("flat");
    up.set_sensitive(number > 1);
    up.connect_clicked({
        let list = list.clone();
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            if let Err(error) = store.move_stage(stage_id, -1) {
                eprintln!("could not move stage {stage_id}: {error}");
                return;
            }
            refresh_stages(&list, &store, &nav, profile_id);
        }
    });

    let down = gtk::Button::from_icon_name("go-down-symbolic");
    down.set_tooltip_text(Some("Move this stage down"));
    down.add_css_class("flat");
    down.set_sensitive(number < count);
    down.connect_clicked({
        let list = list.clone();
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            if let Err(error) = store.move_stage(stage_id, 1) {
                eprintln!("could not move stage {stage_id}: {error}");
                return;
            }
            refresh_stages(&list, &store, &nav, profile_id);
        }
    });

    let edit = gtk::Button::from_icon_name("document-edit-symbolic");
    edit.set_tooltip_text(Some("Edit stage"));
    edit.add_css_class("flat");
    edit.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            nav.push(&stage_form(&store, &nav, profile_id, Some(stage_id)));
        }
    });

    let delete = gtk::Button::from_icon_name("user-trash-symbolic");
    delete.set_tooltip_text(Some("Delete stage"));
    delete.add_css_class("flat");
    delete.add_css_class("destructive");
    let snapshot = changes::stage(stage);
    delete.connect_clicked({
        let list = list.clone();
        let store = store.clone();
        let nav = nav.clone();
        let name = stage.label.clone();
        let snapshot = snapshot.clone();
        move |_| {
            let window = nav
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok());
            let has_children = store.stage_has_children(stage_id).unwrap_or(false);

            if has_children {
                let dialog = adw::AlertDialog::builder()
                    .heading(format!("Cannot delete “{name}” yet"))
                    .body(
                        "This stage still has its own requirements, timing or attributes. Remove \
                         those first, so every value keeps a history and a revert.",
                    )
                    .build();
                dialog.add_response("close", "Close");
                dialog.set_default_response(Some("close"));
                dialog.set_close_response("close");
                dialog.present(window.as_ref());
                return;
            }

            let dialog = adw::AlertDialog::builder()
                .heading(format!("Delete “{name}”?"))
                .body("The stage is removed. You can revert it under Activity.")
                .build();
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("delete", "Delete");
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");

            let list = list.clone();
            let store = store.clone();
            let response_nav = nav.clone();
            let snapshot = snapshot.clone();
            dialog.connect_response(None, move |_, response| {
                if response != "delete" {
                    return;
                }

                let deleted = write_with_history(
                    &store,
                    |store| store.delete_stage(stage_id),
                    |store, _| {
                        record_row_change(
                            store,
                            profile_id,
                            "stage",
                            stage_id,
                            Some(snapshot.clone()),
                            None,
                        )
                    },
                );

                if let Err(error) = deleted {
                    eprintln!("could not delete stage {stage_id}: {error}");
                    return;
                }

                refresh_stages(&list, &store, &response_nav, profile_id);
            });

            dialog.present(window.as_ref());
        }
    });

    row.add_suffix(&up);
    row.add_suffix(&down);
    row.add_suffix(&edit);
    row.add_suffix(&delete);
    row.set_activatable(false);
    row
}

fn stage_form(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stage_id: Option<i64>,
) -> adw::NavigationPage {
    let stage = stage_id.and_then(|id| store.stage(id).ok().flatten());
    let position = stage
        .as_ref()
        .map(|stage| stage.position)
        .unwrap_or_else(|| {
            store
                .stages(profile_id)
                .map(|stages| stages.iter().map(|stage| stage.position).max().unwrap_or(0) + 1)
                .unwrap_or(1)
        });
    let perennial = store
        .profile_details(profile_id)
        .ok()
        .flatten()
        .and_then(|profile| profile.lifecycle)
        .as_deref()
        == Some("perennial");

    let group = adw::PreferencesGroup::new();
    group.set_title("Stage");
    group.set_description(Some(
        "Stages are saved immediately; the calendar and warnings will use them.",
    ));

    let kind = choice_row(
        "Kind",
        STAGE_KINDS,
        stage
            .as_ref()
            .map(|stage| stage.kind.as_str())
            .or(Some("vegetative")),
    );
    let label = entry_row("Name", stage.as_ref().map(|stage| stage.label.as_str()));
    let duration_min = number_entry_row(
        "Minimum duration (days)",
        stage.as_ref().and_then(|stage| stage.duration_days_min),
    );
    let duration_max = number_entry_row(
        "Maximum duration (days)",
        stage.as_ref().and_then(|stage| stage.duration_days_max),
    );
    let notes = entry_row(
        "Notes",
        stage.as_ref().and_then(|stage| stage.notes.as_deref()),
    );
    let repeats = perennial.then(|| {
        adw::SwitchRow::builder()
            .title("Repeats each year")
            .subtitle("Off: this stage belongs to the first cycle only.")
            .active(stage.as_ref().is_none_or(|stage| stage.repeats_each_year()))
            .build()
    });

    group.add(&kind);
    group.add(&label);
    group.add(&duration_min);
    group.add(&duration_max);
    group.add(&notes);
    if let Some(repeats) = &repeats {
        group.add(repeats);
    }

    let preferences = adw::PreferencesPage::new();
    preferences.add(&group);

    let title = if stage_id.is_some() {
        "Edit stage"
    } else {
        "New stage"
    };
    let guard = Guard::new(nav);
    let header = guard.header(title);

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.set_sensitive(!label.text().trim().is_empty());
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);
    let existing_repeats = stage.as_ref().and_then(|stage| stage.repeats);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    label.connect_changed({
        let save = save.clone();
        move |row| save.set_sensitive(!row.text().trim().is_empty())
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    save.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let kind = kind.clone();
        let label = label.clone();
        let duration_min = duration_min.clone();
        let duration_max = duration_max.clone();
        let notes = notes.clone();
        let repeats = repeats.clone();
        move |_| {
            let duration_min_value = match optional_number(&duration_min, "Minimum duration") {
                Ok(value) => value,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };
            let duration_max_value = match optional_number(&duration_max, "Maximum duration") {
                Ok(value) => value,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };

            let stage = match build_stage(
                stage_id,
                profile_id,
                position,
                choice_value(&kind, STAGE_KINDS),
                StageValues {
                    label: &label.text(),
                    duration: (duration_min_value, duration_max_value),
                    notes: &notes.text(),
                    repeats: repeats
                        .as_ref()
                        .map(|row| row.is_active())
                        .or(existing_repeats),
                },
            ) {
                Ok(stage) => stage,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };

            let before = stage_id
                .and_then(|id| store.stage(id).ok().flatten())
                .as_ref()
                .map(changes::stage);

            let result = write_with_history(
                &store,
                |store| {
                    if let Some(id) = stage_id {
                        store.update_stage(&stage)?;
                        Ok(id)
                    } else {
                        store.create_stage(&stage)
                    }
                },
                |store, id| {
                    let after = store.stage(*id)?;
                    record_row_change(
                        store,
                        profile_id,
                        "stage",
                        *id,
                        before,
                        after.as_ref().map(changes::stage),
                    )
                },
            );

            match result {
                Ok(_) => {
                    nav.pop();
                }
                Err(error) => {
                    eprintln!("could not save stage: {error}");
                    let message = save_error_message(error.as_ref(), "Could not save the stage");
                    overlay.add_toast(adw::Toast::new(&message));
                }
            }
        }
    });

    let page = adw::NavigationPage::new(&toolbar, title);
    guard.attach(&page, &toolbar);
    page
}

pub(super) fn requirements_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) -> adw::NavigationPage {
    let on_add: Rc<dyn Fn()> = {
        let store = store.clone();
        let nav = nav.clone();
        Rc::new(move || {
            let stages = store.stages(profile_id).unwrap_or_default();
            nav.push(&requirement_form(&store, &nav, profile_id, &stages, None));
        })
    };
    let refresh: Rc<dyn Fn(&gtk::ListBox)> = {
        let store = store.clone();
        let nav = nav.clone();
        Rc::new(move |list| refresh_requirements(list, &store, &nav, profile_id))
    };
    super::shared::editor_list::page("Requirements", "Add requirement", on_add, refresh)
}

fn refresh_requirements(
    list: &gtk::ListBox,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let stages = store.stages(profile_id).unwrap_or_default();
    let names = stage_names(&stages);
    let requirements = match store.requirements(profile_id) {
        Ok(requirements) => requirements,
        Err(error) => {
            eprintln!("could not read requirements for profile {profile_id}: {error}");
            Vec::new()
        }
    };

    if requirements.is_empty() {
        list.append(&empty_row(
            "No requirements yet",
            "Add the ranges the app should compare measurements against.",
        ));
        return;
    }

    for requirement in &requirements {
        let stage = match requirement.stage_id {
            Some(id) => names.get(&id).map(String::as_str).unwrap_or("Stage"),
            None => "Profile default",
        };
        list.append(&requirement_row(
            list,
            store,
            nav,
            profile_id,
            &stages,
            requirement,
            stage,
        ));
    }
}

fn requirement_row(
    list: &gtk::ListBox,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    requirement: &Requirement,
    stage: &str,
) -> adw::ActionRow {
    let requirement_id = requirement.id;
    let pref = UnitPreference::load(store);
    let range = requirement_range(requirement, pref).unwrap_or_else(|| "Not set".to_string());
    let row = adw::ActionRow::builder()
        .title(requirement_title(requirement))
        .subtitle(format!("{stage} · {range}"))
        .build();

    let edit = gtk::Button::from_icon_name("document-edit-symbolic");
    edit.set_tooltip_text(Some("Edit requirement"));
    edit.add_css_class("flat");
    edit.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let stages = stages.to_vec();
        let requirement = requirement.clone();
        move |_| {
            nav.push(&requirement_form(
                &store,
                &nav,
                profile_id,
                &stages,
                Some(&requirement),
            ));
        }
    });

    let delete = gtk::Button::from_icon_name("user-trash-symbolic");
    delete.set_tooltip_text(Some("Delete requirement"));
    delete.add_css_class("flat");
    delete.add_css_class("destructive");
    let snapshot = changes::requirement(requirement);
    delete.connect_clicked({
        let list = list.clone();
        let store = store.clone();
        let nav = nav.clone();
        let title = requirement_title(requirement);
        move |_| {
            let dialog = adw::AlertDialog::builder()
                .heading(format!("Delete “{title}”?"))
                .body("The range is removed from the profile.")
                .build();
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("delete", "Delete");
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");

            let list = list.clone();
            let store = store.clone();
            let response_nav = nav.clone();
            let snapshot = snapshot.clone();
            dialog.connect_response(None, move |_, response| {
                if response != "delete" {
                    return;
                }

                let deleted = write_with_history(
                    &store,
                    |store| store.delete_requirement(requirement_id),
                    |store, _| {
                        record_row_change(
                            store,
                            profile_id,
                            "requirement",
                            requirement_id,
                            Some(snapshot.clone()),
                            None,
                        )
                    },
                );

                if let Err(error) = deleted {
                    eprintln!("could not delete requirement {requirement_id}: {error}");
                    return;
                }

                refresh_requirements(&list, &store, &response_nav, profile_id);
            });

            let window = nav
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok());
            dialog.present(window.as_ref());
        }
    });

    row.add_suffix(&edit);
    row.add_suffix(&delete);
    row.set_activatable(false);
    row
}

fn requirement_form(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    existing: Option<&Requirement>,
) -> adw::NavigationPage {
    let stage_ids: Vec<i64> = stages.iter().map(|stage| stage.id).collect();
    let stage = stage_choice_row(
        stages,
        existing.and_then(|requirement| requirement.stage_id),
    );
    let metric = metric_choice_row(existing.map(|requirement| requirement.metric.as_str()));
    let pref = UnitPreference::load(store);
    let metric_value = metric_choice_value(&metric).unwrap_or_else(|| "air_temp".to_string());
    let unit = metric_unit(&metric_value);
    let day_part = required_choice_row(
        "Day part",
        DAY_PARTS,
        existing
            .map(|requirement| requirement.day_part.as_str())
            .or(Some("day")),
    );

    let key_group = adw::PreferencesGroup::new();
    key_group.set_title("Requirement");
    key_group.set_description(Some(
        "Stage, metric and day part identify the range. They are fixed when editing.",
    ));
    key_group.add(&stage);
    key_group.add(&metric);
    key_group.add(&day_part);

    let (survival_min, survival_min_label) = number_entry_row_with_unit(
        "Minimum",
        existing.and_then(|requirement| requirement.survival_min),
        unit,
        pref,
    );
    let (survival_max, survival_max_label) = number_entry_row_with_unit(
        "Maximum",
        existing.and_then(|requirement| requirement.survival_max),
        unit,
        pref,
    );
    let survival_group = adw::PreferencesGroup::new();
    survival_group.set_title("Survival limits");
    survival_group.set_description(Some("Outside these limits the plant is damaged or dies."));
    survival_group.add(&survival_min);
    survival_group.add(&survival_max);

    let (min, min_label) = number_entry_row_with_unit(
        "Minimum",
        existing.and_then(|requirement| requirement.min),
        unit,
        pref,
    );
    let (max, max_label) = number_entry_row_with_unit(
        "Maximum",
        existing.and_then(|requirement| requirement.max),
        unit,
        pref,
    );
    let growth_group = adw::PreferencesGroup::new();
    growth_group.set_title("Growth limits");
    growth_group.set_description(Some(
        "Outside these limits growth stops or is seriously impaired.",
    ));
    growth_group.add(&min);
    growth_group.add(&max);

    let (opt_min, opt_min_label) = number_entry_row_with_unit(
        "Minimum",
        existing.and_then(|requirement| requirement.opt_min),
        unit,
        pref,
    );
    let (opt_max, opt_max_label) = number_entry_row_with_unit(
        "Maximum",
        existing.and_then(|requirement| requirement.opt_max),
        unit,
        pref,
    );
    let optimum_group = adw::PreferencesGroup::new();
    optimum_group.set_title("Optimum");
    optimum_group.set_description(Some("The ideal range for growth and production."));
    optimum_group.add(&opt_min);
    optimum_group.add(&opt_max);

    let preferences = adw::PreferencesPage::new();
    preferences.add(&key_group);
    preferences.add(&survival_group);
    preferences.add(&growth_group);
    preferences.add(&optimum_group);

    let title = if existing.is_some() {
        "Edit requirement"
    } else {
        "New requirement"
    };
    let guard = Guard::new(nav);
    let header = guard.header(title);

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    if existing.is_some() {
        stage.set_sensitive(false);
        metric.set_sensitive(false);
        day_part.set_sensitive(false);
    }

    let originals = [
        existing.and_then(|requirement| requirement.survival_min),
        existing.and_then(|requirement| requirement.survival_max),
        existing.and_then(|requirement| requirement.min),
        existing.and_then(|requirement| requirement.max),
        existing.and_then(|requirement| requirement.opt_min),
        existing.and_then(|requirement| requirement.opt_max),
    ];
    let shown = [
        survival_min.text().to_string(),
        survival_max.text().to_string(),
        min.text().to_string(),
        max.text().to_string(),
        opt_min.text().to_string(),
        opt_max.text().to_string(),
    ];
    let unit_labels: Rc<Vec<gtk::Label>> = Rc::new(vec![
        survival_min_label,
        survival_max_label,
        min_label,
        max_label,
        opt_min_label,
        opt_max_label,
    ]);
    let entries: Rc<Vec<adw::EntryRow>> = Rc::new(vec![
        survival_min.clone(),
        survival_max.clone(),
        min.clone(),
        max.clone(),
        opt_min.clone(),
        opt_max.clone(),
    ]);
    metric.connect_selected_notify({
        let unit_labels = unit_labels.clone();
        let entries = entries.clone();
        move |row| {
            let metric = metric_choice_value(row).unwrap_or_else(|| "air_temp".to_string());
            let label = pref.label(metric_unit(&metric));
            for unit_label in unit_labels.iter() {
                unit_label.set_text(&label);
            }
            // 25 °C is not 25 %. A different metric starts from empty fields.
            for entry in entries.iter() {
                entry.set_text("");
            }
        }
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    save.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let stage = stage.clone();
        let metric = metric.clone();
        let day_part = day_part.clone();
        let survival_min = survival_min.clone();
        let survival_max = survival_max.clone();
        let min = min.clone();
        let max = max.clone();
        let opt_min = opt_min.clone();
        let opt_max = opt_max.clone();
        let stage_ids = stage_ids.clone();
        let existing_id = existing.map(|requirement| requirement.id);
        let editing = existing.is_some();

        move |_| {
            let stage_id = stage_choice_value(&stage, &stage_ids);
            let metric_value =
                metric_choice_value(&metric).unwrap_or_else(|| "air_temp".to_string());
            let day_part_value =
                required_choice_value(&day_part, DAY_PARTS).unwrap_or_else(|| "day".to_string());

            let fields = [
                (&survival_min, "Survival minimum"),
                (&survival_max, "Survival maximum"),
                (&min, "Growth minimum"),
                (&max, "Growth maximum"),
                (&opt_min, "Optimum minimum"),
                (&opt_max, "Optimum maximum"),
            ];
            let mut values = [None; 6];
            for (index, (row, label)) in fields.iter().enumerate() {
                values[index] = if let Some(original) =
                    keep_original(&row.text(), &shown[index], originals[index])
                {
                    original
                } else {
                    match optional_quantity(row, label, metric_unit(&metric_value), pref) {
                        Ok(value) => value,
                        Err(message) => {
                            overlay.add_toast(adw::Toast::new(&message));
                            return;
                        }
                    }
                };
            }

            if matches!(metric_value.as_str(), "air_temp" | "soil_temp")
                && values.iter().flatten().any(|value| {
                    !(crate::plant_data::TEMPERATURE_MIN_C..=crate::plant_data::TEMPERATURE_MAX_C)
                        .contains(value)
                })
            {
                overlay.add_toast(adw::Toast::new(&format!(
                    "Temperature must be between {} and {}",
                    pref.text("°C", crate::plant_data::TEMPERATURE_MIN_C),
                    pref.text("°C", crate::plant_data::TEMPERATURE_MAX_C)
                )));
                return;
            }

            if !editing && let Ok(requirements) = store.requirements(profile_id) {
                let duplicate = requirements.iter().any(|requirement| {
                    requirement.stage_id == stage_id
                        && requirement.metric == metric_value
                        && requirement.day_part == day_part_value
                });

                if duplicate {
                    overlay.add_toast(adw::Toast::new(
                        "A requirement for this stage, metric and day part already exists",
                    ));
                    return;
                }
            }

            let requirement = build_requirement(
                existing_id,
                profile_id,
                stage_id,
                metric_value,
                day_part_value,
                values,
            );

            let before = existing_id
                .and_then(|id| {
                    store
                        .requirements(profile_id)
                        .ok()?
                        .into_iter()
                        .find(|row| row.id == id)
                })
                .as_ref()
                .map(changes::requirement);

            let result = write_with_history(
                &store,
                |store| store.set_requirement(&requirement),
                |store, id| {
                    let after = store
                        .requirements(profile_id)?
                        .into_iter()
                        .find(|row| row.id == *id);
                    record_row_change(
                        store,
                        profile_id,
                        "requirement",
                        *id,
                        before,
                        after.as_ref().map(changes::requirement),
                    )
                },
            );

            match result {
                Ok(_) => {
                    nav.pop();
                }
                Err(error) => {
                    eprintln!("could not save requirement: {error}");
                    let message =
                        save_error_message(error.as_ref(), "Could not save the requirement");
                    overlay.add_toast(adw::Toast::new(&message));
                }
            }
        }
    });

    let page = adw::NavigationPage::new(&toolbar, title);
    guard.attach(&page, &toolbar);
    page
}

fn stage_choice_row(stages: &[Stage], selected: Option<i64>) -> adw::ComboRow {
    let mut labels = vec!["Profile default".to_string()];
    labels.extend(stages.iter().map(|stage| stage.label.clone()));
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&labels);
    let row = adw::ComboRow::builder()
        .title("Stage")
        .model(&model)
        .build();
    row.set_selected(match selected {
        Some(id) => stages
            .iter()
            .position(|stage| stage.id == id)
            .map(|index| index as u32 + 1)
            .unwrap_or(0),
        None => 0,
    });
    row
}

fn stage_choice_value(row: &adw::ComboRow, stage_ids: &[i64]) -> Option<i64> {
    let selected = row.selected();
    if selected == 0 {
        return None;
    }

    stage_ids.get(selected as usize - 1).copied()
}

fn metric_choice_row(selected: Option<&str>) -> adw::ComboRow {
    let labels: Vec<&str> = METRICS.iter().map(|metric| metric_label(metric)).collect();
    let model = gtk::StringList::new(&labels);
    let row = adw::ComboRow::builder()
        .title("Metric")
        .model(&model)
        .build();
    row.set_selected(
        selected
            .and_then(|selected| METRICS.iter().position(|metric| *metric == selected))
            .map(|index| index as u32)
            .unwrap_or(0),
    );
    row
}

fn metric_choice_value(row: &adw::ComboRow) -> Option<String> {
    METRICS
        .get(row.selected() as usize)
        .map(|metric| (*metric).to_string())
}

fn required_choice_row(title: &str, values: &[&str], selected: Option<&str>) -> adw::ComboRow {
    let labels: Vec<String> = values.iter().map(|value| humanize(value)).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&labels);
    let row = adw::ComboRow::builder().title(title).model(&model).build();
    row.set_selected(
        selected
            .and_then(|selected| values.iter().position(|value| *value == selected))
            .map(|index| index as u32)
            .unwrap_or(0),
    );
    row
}

fn required_choice_value(row: &adw::ComboRow, values: &[&str]) -> Option<String> {
    values
        .get(row.selected() as usize)
        .map(|value| (*value).to_string())
}

pub(super) fn timing_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) -> adw::NavigationPage {
    let on_add: Rc<dyn Fn()> = {
        let store = store.clone();
        let nav = nav.clone();
        Rc::new(move || {
            let stages = store.stages(profile_id).unwrap_or_default();
            nav.push(&timing_form(&store, &nav, profile_id, &stages, None));
        })
    };
    let refresh: Rc<dyn Fn(&gtk::ListBox)> = {
        let store = store.clone();
        let nav = nav.clone();
        Rc::new(move |list| refresh_timing(list, &store, &nav, profile_id))
    };
    super::shared::editor_list::page("Timing", "Add timing rule", on_add, refresh)
}

fn refresh_timing(
    list: &gtk::ListBox,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    let stages = store.stages(profile_id).unwrap_or_default();
    let names = stage_names(&stages);
    let rules = match store.timing_rules(profile_id) {
        Ok(rules) => rules,
        Err(error) => {
            eprintln!("could not read timing rules for profile {profile_id}: {error}");
            Vec::new()
        }
    };

    if rules.is_empty() {
        list.append(&empty_row(
            "No timing rules yet",
            "Add sowing, harvest, pruning or feeding rules relative to a season, stage or trigger.",
        ));
        return;
    }

    for rule in &rules {
        list.append(&timing_row(
            list, store, nav, profile_id, &stages, rule, &names,
        ));
    }
}

fn timing_row(
    list: &gtk::ListBox,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    rule: &TimingRule,
    names: &HashMap<i64, String>,
) -> adw::ActionRow {
    let rule_id = rule.id;
    let row = adw::ActionRow::builder()
        .title(timing_title(rule))
        .subtitle(timing_subtitle(rule, names))
        .build();
    row.set_subtitle_lines(2);

    let edit = gtk::Button::from_icon_name("document-edit-symbolic");
    edit.set_tooltip_text(Some("Edit timing rule"));
    edit.add_css_class("flat");
    edit.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let stages = stages.to_vec();
        let rule = rule.clone();
        move |_| {
            nav.push(&timing_form(&store, &nav, profile_id, &stages, Some(&rule)));
        }
    });

    let delete = gtk::Button::from_icon_name("user-trash-symbolic");
    delete.set_tooltip_text(Some("Delete timing rule"));
    delete.add_css_class("flat");
    delete.add_css_class("destructive");
    let snapshot = changes::timing(rule);
    delete.connect_clicked({
        let list = list.clone();
        let store = store.clone();
        let nav = nav.clone();
        let title = timing_title(rule);
        move |_| {
            let dialog = adw::AlertDialog::builder()
                .heading(format!("Delete “{title}”?"))
                .body("The timing rule is removed from the profile.")
                .build();
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("delete", "Delete");
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");

            let list = list.clone();
            let store = store.clone();
            let response_nav = nav.clone();
            let snapshot = snapshot.clone();
            dialog.connect_response(None, move |_, response| {
                if response != "delete" {
                    return;
                }

                let deleted = write_with_history(
                    &store,
                    |store| store.delete_timing_rule(rule_id),
                    |store, _| {
                        record_row_change(
                            store,
                            profile_id,
                            "timing_rule",
                            rule_id,
                            Some(snapshot.clone()),
                            None,
                        )
                    },
                );

                if let Err(error) = deleted {
                    eprintln!("could not delete timing rule {rule_id}: {error}");
                    return;
                }

                refresh_timing(&list, &store, &response_nav, profile_id);
            });

            let window = nav
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok());
            dialog.present(window.as_ref());
        }
    });

    row.add_suffix(&edit);
    row.add_suffix(&delete);
    row.set_activatable(false);
    row
}

struct TimingValues {
    stage_id: Option<i64>,
    kind: String,
    anchor: String,
    offset_min_days: Option<i64>,
    offset_max_days: Option<i64>,
    month_start: Option<i64>,
    month_end: Option<i64>,
    day_length_hours: Option<f64>,
    gdd_base_c: Option<f64>,
    gdd_target: Option<f64>,
    repeat_days: Option<i64>,
    notes: Option<String>,
}

fn build_timing_rule(id: Option<i64>, profile_id: i64, values: TimingValues) -> TimingRule {
    TimingRule {
        id: id.unwrap_or(0),
        profile_id,
        stage_id: values.stage_id,
        kind: values.kind,
        anchor: values.anchor,
        offset_min_days: values.offset_min_days,
        offset_max_days: values.offset_max_days,
        month_start: values.month_start,
        month_end: values.month_end,
        day_length_hours: values.day_length_hours,
        gdd_base_c: values.gdd_base_c,
        gdd_target: values.gdd_target,
        repeat_days: values.repeat_days,
        notes: values.notes,
    }
}

fn timing_form(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    existing: Option<&TimingRule>,
) -> adw::NavigationPage {
    let stage_ids: Vec<i64> = stages.iter().map(|stage| stage.id).collect();
    let stage = stage_choice_row(stages, existing.and_then(|rule| rule.stage_id));
    let kind = timing_kind_row(
        existing
            .map(|rule| rule.kind.as_str())
            .or(Some("sow_indoor")),
    );
    let anchor = timing_anchor_row(
        existing
            .map(|rule| rule.anchor.as_str())
            .or(Some("last_frost")),
    );

    let key_group = adw::PreferencesGroup::new();
    key_group.set_title("Timing");
    key_group.set_description(Some(
        "The kind names the action; the anchor and offset place it in the season.",
    ));
    key_group.add(&kind);
    key_group.add(&anchor);
    key_group.add(&stage);

    let offset_min = integer_entry_row(
        "Minimum offset (days)",
        existing.and_then(|rule| rule.offset_min_days),
    );
    let offset_max = integer_entry_row(
        "Maximum offset (days)",
        existing.and_then(|rule| rule.offset_max_days),
    );
    let offset_group = adw::PreferencesGroup::new();
    offset_group.set_title("Offset");
    offset_group.set_description(Some(
        "Relative to the anchor. Negative means before, positive means after.",
    ));
    offset_group.add(&offset_min);
    offset_group.add(&offset_max);

    let month_start = month_choice_row(
        "Start month",
        existing.and_then(|rule| rule.month_start).or(Some(1)),
    );
    let month_end = month_choice_row(
        "End month",
        existing.and_then(|rule| rule.month_end).or(Some(12)),
    );
    let calendar_group = adw::PreferencesGroup::new();
    calendar_group.set_title("Calendar");
    calendar_group.set_description(Some("Used when the anchor is Calendar."));
    calendar_group.add(&month_start);
    calendar_group.add(&month_end);

    let day_length_hours =
        number_entry_row("Hours", existing.and_then(|rule| rule.day_length_hours));
    let day_length_group = adw::PreferencesGroup::new();
    day_length_group.set_title("Day length");
    day_length_group.set_description(Some("Used when the anchor is Day length."));
    day_length_group.add(&day_length_hours);

    let gdd_base = number_entry_row_unit(
        "Base temperature",
        existing.and_then(|rule| rule.gdd_base_c),
        "°C",
        UnitPreference::Metric,
    );
    let gdd_target = number_entry_row("Target GDD", existing.and_then(|rule| rule.gdd_target));
    let gdd_group = adw::PreferencesGroup::new();
    gdd_group.set_title("Growing degree days");
    gdd_group.set_description(Some("Used when the anchor is Growing degree days."));
    gdd_group.add(&gdd_base);
    gdd_group.add(&gdd_target);

    let repeat_days = integer_entry_row("Every (days)", existing.and_then(|rule| rule.repeat_days));
    let repeat_group = adw::PreferencesGroup::new();
    repeat_group.set_title("Repeat");
    repeat_group.set_description(Some(
        "Optional. Repeats care rules such as feeding every two weeks.",
    ));
    repeat_group.add(&repeat_days);

    let notes = entry_row("Notes", existing.and_then(|rule| rule.notes.as_deref()));
    let notes_group = adw::PreferencesGroup::new();
    notes_group.set_title("Notes");
    notes_group.add(&notes);

    let preferences = adw::PreferencesPage::new();
    preferences.add(&key_group);
    preferences.add(&offset_group);
    preferences.add(&calendar_group);
    preferences.add(&day_length_group);
    preferences.add(&gdd_group);
    preferences.add(&repeat_group);
    preferences.add(&notes_group);

    let title = if existing.is_some() {
        "Edit timing rule"
    } else {
        "New timing rule"
    };
    let guard = Guard::new(nav);
    let header = guard.header(title);

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    let initial_anchor = timing_anchor_value(&anchor).unwrap_or_default();
    set_timing_group_visibility(
        &initial_anchor,
        &offset_group,
        &calendar_group,
        &day_length_group,
        &gdd_group,
    );
    anchor.connect_selected_notify({
        let offset_group = offset_group.clone();
        let calendar_group = calendar_group.clone();
        let day_length_group = day_length_group.clone();
        let gdd_group = gdd_group.clone();
        move |row| {
            let anchor = timing_anchor_value(row).unwrap_or_default();
            set_timing_group_visibility(
                &anchor,
                &offset_group,
                &calendar_group,
                &day_length_group,
                &gdd_group,
            );
        }
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    save.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let stage = stage.clone();
        let kind = kind.clone();
        let anchor = anchor.clone();
        let offset_min = offset_min.clone();
        let offset_max = offset_max.clone();
        let month_start = month_start.clone();
        let month_end = month_end.clone();
        let day_length_hours = day_length_hours.clone();
        let gdd_base = gdd_base.clone();
        let gdd_target = gdd_target.clone();
        let repeat_days = repeat_days.clone();
        let notes = notes.clone();
        let stage_ids = stage_ids.clone();
        let existing_id = existing.map(|rule| rule.id);

        move |_| {
            let stage_id = stage_choice_value(&stage, &stage_ids);
            let kind_value = timing_kind_value(&kind).unwrap_or_else(|| "sow_indoor".to_string());
            let anchor_value =
                timing_anchor_value(&anchor).unwrap_or_else(|| "last_frost".to_string());

            let offset_anchor = matches!(
                anchor_value.as_str(),
                "last_frost" | "first_frost" | "sowing" | "transplant" | "stage_start"
            );
            let offset_min_value = if offset_anchor {
                match optional_integer(&offset_min, "Minimum offset") {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            } else {
                None
            };
            let offset_max_value = if offset_anchor {
                match optional_integer(&offset_max, "Maximum offset") {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            } else {
                None
            };
            let day_length_value = if anchor_value == "day_length" {
                match optional_number(&day_length_hours, "Day length") {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            } else {
                None
            };
            let gdd_base_value = if anchor_value == "gdd" {
                match optional_number(&gdd_base, "GDD base temperature") {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            } else {
                None
            };
            let gdd_target_value = if anchor_value == "gdd" {
                match optional_number(&gdd_target, "GDD target") {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            } else {
                None
            };
            let repeat_value = match optional_integer(&repeat_days, "Repeat interval") {
                Ok(value) => value,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };
            let month_start_value = if anchor_value == "calendar" {
                month_choice_value(&month_start)
            } else {
                None
            };
            let month_end_value = if anchor_value == "calendar" {
                month_choice_value(&month_end)
            } else {
                None
            };

            let rule = build_timing_rule(
                existing_id,
                profile_id,
                TimingValues {
                    stage_id,
                    kind: kind_value,
                    anchor: anchor_value,
                    offset_min_days: offset_min_value,
                    offset_max_days: offset_max_value,
                    month_start: month_start_value,
                    month_end: month_end_value,
                    day_length_hours: day_length_value,
                    gdd_base_c: gdd_base_value,
                    gdd_target: gdd_target_value,
                    repeat_days: repeat_value,
                    notes: clean_optional(&notes.text()),
                },
            );

            let before = existing_id
                .and_then(|id| {
                    store
                        .timing_rules(profile_id)
                        .ok()?
                        .into_iter()
                        .find(|row| row.id == id)
                })
                .as_ref()
                .map(changes::timing);

            let result = write_with_history(
                &store,
                |store| {
                    if let Some(id) = existing_id {
                        store.update_timing_rule(&rule)?;
                        Ok(id)
                    } else {
                        store.create_timing_rule(&rule)
                    }
                },
                |store, id| {
                    let after = store
                        .timing_rules(profile_id)?
                        .into_iter()
                        .find(|row| row.id == *id);
                    record_row_change(
                        store,
                        profile_id,
                        "timing_rule",
                        *id,
                        before,
                        after.as_ref().map(changes::timing),
                    )
                },
            );

            match result {
                Ok(_) => {
                    nav.pop();
                }
                Err(error) => {
                    eprintln!("could not save timing rule: {error}");
                    let message =
                        save_error_message(error.as_ref(), "Could not save the timing rule");
                    overlay.add_toast(adw::Toast::new(&message));
                }
            }
        }
    });

    let page = adw::NavigationPage::new(&toolbar, title);
    guard.attach(&page, &toolbar);
    page
}

fn timing_kind_row(selected: Option<&str>) -> adw::ComboRow {
    let labels: Vec<String> = TIMING_KINDS
        .iter()
        .map(|kind| timing_kind_label(kind))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&labels);
    let row = adw::ComboRow::builder().title("Kind").model(&model).build();
    row.set_selected(
        selected
            .and_then(|selected| TIMING_KINDS.iter().position(|kind| *kind == selected))
            .map(|index| index as u32)
            .unwrap_or(0),
    );
    row
}

fn timing_kind_value(row: &adw::ComboRow) -> Option<String> {
    TIMING_KINDS
        .get(row.selected() as usize)
        .map(|kind| (*kind).to_string())
}

fn timing_anchor_row(selected: Option<&str>) -> adw::ComboRow {
    let labels: Vec<String> = TIMING_ANCHORS
        .iter()
        .map(|anchor| timing_anchor_label(anchor))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&labels);
    let row = adw::ComboRow::builder()
        .title("Anchor")
        .model(&model)
        .build();
    row.set_selected(
        selected
            .and_then(|selected| TIMING_ANCHORS.iter().position(|anchor| *anchor == selected))
            .map(|index| index as u32)
            .unwrap_or(0),
    );
    row
}

fn timing_anchor_value(row: &adw::ComboRow) -> Option<String> {
    TIMING_ANCHORS
        .get(row.selected() as usize)
        .map(|anchor| (*anchor).to_string())
}

fn month_choice_row(title: &str, selected: Option<i64>) -> adw::ComboRow {
    let labels: Vec<&str> = (1..=12).map(month_name).collect();
    let model = gtk::StringList::new(&labels);
    let row = adw::ComboRow::builder().title(title).model(&model).build();
    row.set_selected(
        selected
            .map(|month| (month.clamp(1, 12) - 1) as u32)
            .unwrap_or(0),
    );
    row
}

fn month_choice_value(row: &adw::ComboRow) -> Option<i64> {
    Some(row.selected() as i64 + 1)
}

fn set_timing_group_visibility(
    anchor: &str,
    offset: &adw::PreferencesGroup,
    calendar: &adw::PreferencesGroup,
    day_length: &adw::PreferencesGroup,
    gdd: &adw::PreferencesGroup,
) {
    offset.set_sensitive(matches!(
        anchor,
        "last_frost" | "first_frost" | "sowing" | "transplant" | "stage_start"
    ));
    calendar.set_sensitive(anchor == "calendar");
    day_length.set_sensitive(anchor == "day_length");
    gdd.set_sensitive(anchor == "gdd");
}

pub(super) fn attributes_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) -> adw::NavigationPage {
    let header = page_header("Attributes");

    let add = gtk::Button::with_label("Add attribute");
    add.add_css_class("suggested-action");
    header.pack_end(&add);

    let custom = gtk::Button::with_label("Custom key");
    custom.add_css_class("flat");
    header.pack_end(&custom);

    let hint = gtk::Label::new(Some(
        "Registered attributes cover most needs. Create a custom key when nothing fits.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("dim-label");

    let groups = gtk::Box::new(gtk::Orientation::Vertical, 18);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(32);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&hint);
    content.append(&groups);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(720);
    clamp.set_tightening_threshold(900);
    clamp.set_child(Some(&content));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroller));

    let page = adw::NavigationPage::new(&toolbar, "Attributes");

    custom.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            nav.push(&custom_key_form(&store, &nav));
        }
    });

    add.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            let stages = store.stages(profile_id).unwrap_or_default();
            nav.push(&attribute_form(&store, &nav, profile_id, &stages, None));
        }
    });

    refresh_attributes(&groups, store, nav, profile_id);
    scroller.connect_map({
        let groups = groups.clone();
        let store = store.clone();
        let nav = nav.clone();
        move |_| refresh_attributes(&groups, &store, &nav, profile_id)
    });

    page
}

fn refresh_attributes(
    groups: &gtk::Box,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) {
    while let Some(child) = groups.first_child() {
        groups.remove(&child);
    }

    let stages = store.stages(profile_id).unwrap_or_default();
    let definitions = attribute_definitions(store);
    let attributes = match store.attributes(profile_id) {
        Ok(attributes) => attributes,
        Err(error) => {
            eprintln!("could not read attributes for profile {profile_id}: {error}");
            Vec::new()
        }
    };

    if attributes.is_empty() {
        let group = adw::PreferencesGroup::new();
        group.add(&empty_row(
            "No attributes yet",
            "Add a registered attribute or create a custom key for the long tail.",
        ));
        groups.append(&group);
        return;
    }

    for category in ATTRIBUTE_CATEGORIES {
        let mut rows: Vec<&Attribute> = attributes
            .iter()
            .filter(|attribute| {
                definitions
                    .get(&attribute.key)
                    .map(|definition| definition.category == *category)
                    .unwrap_or(*category == "Other")
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by_key(|attribute| {
            definitions
                .get(&attribute.key)
                .map(|definition| definition.label.clone())
                .unwrap_or_else(|| humanize(&attribute.key))
        });

        let group = adw::PreferencesGroup::new();
        // The group title is markup, and a category such as "Care & training" is not.
        group.set_title(&format!(
            "{} · {}",
            gtk::glib::markup_escape_text(category),
            rows.len()
        ));
        for attribute in rows {
            group.add(&attribute_row(
                groups,
                store,
                nav,
                profile_id,
                &stages,
                attribute,
                definitions.get(&attribute.key),
            ));
        }
        groups.append(&group);
    }
}

fn attribute_row(
    groups: &gtk::Box,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    attribute: &Attribute,
    definition: Option<&AttributeDefinition>,
) -> adw::ActionRow {
    let attribute_id = attribute.id;
    let title = definition
        .map(|definition| definition.label.clone())
        .unwrap_or_else(|| humanize(&attribute.key));
    let pref = UnitPreference::load(store);
    let value =
        attribute_value(attribute, definition, pref).unwrap_or_else(|| "Not set".to_string());
    let stage = match attribute.stage_id {
        Some(id) => stages
            .iter()
            .find(|stage| stage.id == id)
            .map(|stage| stage.label.as_str())
            .unwrap_or("Stage"),
        None => "Profile",
    };
    let mut subtitle = format!("{stage} · {value}");
    if definition
        .map(|definition| definition.is_custom)
        .unwrap_or(false)
    {
        subtitle.push_str(" · Custom");
    }

    let row = adw::ActionRow::builder()
        .title(&title)
        .subtitle(&subtitle)
        .build();
    row.set_subtitle_lines(3);
    if let Some(definition) = definition {
        row.set_tooltip_text(definition.description.as_deref());
    }

    let edit = gtk::Button::from_icon_name("document-edit-symbolic");
    edit.set_tooltip_text(Some("Edit attribute"));
    edit.add_css_class("flat");
    edit.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let stages = stages.to_vec();
        let attribute = attribute.clone();
        move |_| {
            nav.push(&attribute_form(
                &store,
                &nav,
                profile_id,
                &stages,
                Some(&attribute),
            ));
        }
    });

    let delete = gtk::Button::from_icon_name("user-trash-symbolic");
    delete.set_tooltip_text(Some("Delete attribute"));
    delete.add_css_class("flat");
    delete.add_css_class("destructive");
    let snapshot = changes::attribute(attribute);
    delete.connect_clicked({
        let groups = groups.clone();
        let store = store.clone();
        let nav = nav.clone();
        let title = title.clone();
        move |_| {
            let dialog = adw::AlertDialog::builder()
                .heading(format!("Delete “{title}”?"))
                .body("The attribute value is removed from the profile.")
                .build();
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("delete", "Delete");
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");

            let groups = groups.clone();
            let store = store.clone();
            let response_nav = nav.clone();
            let snapshot = snapshot.clone();
            dialog.connect_response(None, move |_, response| {
                if response != "delete" {
                    return;
                }

                let deleted = write_with_history(
                    &store,
                    |store| store.delete_attribute(attribute_id),
                    |store, _| {
                        record_row_change(
                            store,
                            profile_id,
                            "attribute",
                            attribute_id,
                            Some(snapshot.clone()),
                            None,
                        )
                    },
                );

                if let Err(error) = deleted {
                    eprintln!("could not delete attribute {attribute_id}: {error}");
                    return;
                }

                refresh_attributes(&groups, &store, &response_nav, profile_id);
            });

            let window = nav
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok());
            dialog.present(window.as_ref());
        }
    });

    row.add_suffix(&edit);
    row.add_suffix(&delete);
    row.set_activatable(false);
    row
}

fn attribute_form(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    existing: Option<&Attribute>,
) -> adw::NavigationPage {
    let definitions = store.attribute_definitions().unwrap_or_default();
    let stage_ids: Vec<i64> = stages.iter().map(|stage| stage.id).collect();
    let stage = stage_choice_row(stages, existing.and_then(|attribute| attribute.stage_id));

    let labels: Vec<&str> = definitions
        .iter()
        .map(|definition| definition.label.as_str())
        .collect();
    let model = gtk::StringList::new(&labels);
    let definition = adw::ComboRow::builder()
        .title("Attribute")
        .model(&model)
        .build();
    let initial_index = existing
        .and_then(|attribute| {
            definitions
                .iter()
                .position(|definition| definition.key == attribute.key)
        })
        .unwrap_or(0);
    definition.set_selected(initial_index as u32);

    let pref = UnitPreference::load(store);
    let initial_unit = definitions
        .get(initial_index)
        .and_then(|definition| definition.unit.as_deref())
        .unwrap_or("");
    let initial_pref = if definitions
        .get(initial_index)
        .map(|definition| definition.is_custom)
        .unwrap_or(false)
    {
        UnitPreference::Metric
    } else {
        pref
    };
    let initial_delta = definitions
        .get(initial_index)
        .map(|definition| definition.key == "dif_c")
        .unwrap_or(false);

    let key_group = adw::PreferencesGroup::new();
    key_group.set_title("Attribute");
    key_group.set_description(Some(
        "The key and stage are fixed when editing. Values are saved as you add or edit them.",
    ));
    key_group.add(&definition);
    key_group.add(&stage);

    let number_original = existing.and_then(|attribute| attribute.value_num);
    let number_shown = number_original
        .map(|value| {
            if initial_delta {
                initial_pref.delta_number(initial_unit, value)
            } else {
                initial_pref.number(initial_unit, value)
            }
        })
        .unwrap_or_default();
    let number_value = number_entry_row("Value", None);
    number_value.set_text(&number_shown);
    let text_value = entry_row(
        "Value",
        existing.and_then(|attribute| attribute.value_text.as_deref()),
    );
    let boolean_value = adw::SwitchRow::builder().title("Value").build();
    boolean_value.set_active(
        existing
            .and_then(|attribute| attribute.value_bool)
            .unwrap_or(false),
    );
    let enum_value_row = adw::ComboRow::builder().title("Value").build();
    let unit = gtk::Label::new(None);
    unit.add_css_class("dim-label");
    number_value.add_suffix(&unit);

    let value_group = adw::PreferencesGroup::new();
    value_group.set_title("Value");
    value_group.set_description(Some(
        "The input follows the attribute's value type; the unit comes from the definition.",
    ));
    value_group.add(&number_value);
    value_group.add(&text_value);
    value_group.add(&boolean_value);
    value_group.add(&enum_value_row);

    apply_attribute_definition(
        definitions.get(initial_index),
        &number_value,
        &text_value,
        &boolean_value,
        &enum_value_row,
        &unit,
        pref,
    );

    if let (Some(attribute), Some(definition)) = (existing, definitions.get(initial_index))
        && definition.value_type == "enum"
        && let Some(value) = attribute.value_text.as_deref()
        && let Some(enum_values) = definition.enum_values.as_deref()
    {
        let values: Vec<&str> = enum_values
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();

        if let Some(position) = values.iter().position(|item| *item == value) {
            enum_value_row.set_selected(position as u32);
        }
    }

    let preferences = adw::PreferencesPage::new();
    preferences.add(&key_group);
    preferences.add(&value_group);

    let title = if existing.is_some() {
        "Edit attribute"
    } else {
        "New attribute"
    };
    let guard = Guard::new(nav);
    let header = guard.header(title);

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    if existing.is_some() {
        definition.set_sensitive(false);
    }
    apply_attribute_stage(definitions.get(initial_index), &stage, existing.is_some());

    definition.connect_selected_notify({
        let definitions = definitions.clone();
        let stage = stage.clone();
        let number_value = number_value.clone();
        let text_value = text_value.clone();
        let boolean_value = boolean_value.clone();
        let enum_value_row = enum_value_row.clone();
        let unit = unit.clone();
        move |row| {
            apply_attribute_definition(
                definitions.get(row.selected() as usize),
                &number_value,
                &text_value,
                &boolean_value,
                &enum_value_row,
                &unit,
                pref,
            );
            apply_attribute_stage(definitions.get(row.selected() as usize), &stage, false);
        }
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    save.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let definition = definition.clone();
        let stage = stage.clone();
        let number_value = number_value.clone();
        let text_value = text_value.clone();
        let boolean_value = boolean_value.clone();
        let enum_value_row = enum_value_row.clone();
        let definitions = definitions.clone();
        let stage_ids = stage_ids.clone();
        let existing_id = existing.map(|attribute| attribute.id);

        move |_| {
            let selected_definition = match definitions.get(definition.selected() as usize) {
                Some(definition) => definition.clone(),
                None => {
                    overlay.add_toast(adw::Toast::new("Choose an attribute"));
                    return;
                }
            };
            let stage_id = stage_choice_value(&stage, &stage_ids);

            let mut value_num = None;
            let mut value_text = None;
            let mut value_bool = None;

            match selected_definition.value_type.as_str() {
                "number" => {
                    let unit = selected_definition.unit.as_deref().unwrap_or("");
                    let effective_pref = if selected_definition.is_custom {
                        UnitPreference::Metric
                    } else {
                        pref
                    };

                    if let Some(original) =
                        keep_original(&number_value.text(), &number_shown, number_original)
                    {
                        value_num = original;
                    } else {
                        let delta = selected_definition.key == "dif_c";
                        match optional_number(&number_value, "Value").map(|value| {
                            value.map(|value| {
                                if delta {
                                    effective_pref.delta_input_to_canonical(unit, value)
                                } else {
                                    effective_pref.input_to_canonical(unit, value)
                                }
                            })
                        }) {
                            Ok(Some(value)) => value_num = Some(value),
                            Ok(None) => {
                                overlay.add_toast(adw::Toast::new("Enter a number"));
                                return;
                            }
                            Err(message) => {
                                overlay.add_toast(adw::Toast::new(&message));
                                return;
                            }
                        }
                    }
                }
                "text" | "json" => match clean_optional(&text_value.text()) {
                    Some(value) => value_text = Some(value),
                    None => {
                        overlay.add_toast(adw::Toast::new("Enter a value"));
                        return;
                    }
                },
                "boolean" => {
                    value_bool = Some(boolean_value.is_active());
                }
                "enum" => match enum_value(&enum_value_row, &selected_definition) {
                    Some(value) => value_text = Some(value),
                    None => {
                        overlay.add_toast(adw::Toast::new("Choose a value"));
                        return;
                    }
                },
                _ => {
                    overlay.add_toast(adw::Toast::new("Unsupported attribute type"));
                    return;
                }
            }

            if existing_id.is_none()
                && let Ok(attributes) = store.attributes(profile_id)
            {
                let duplicate = attributes.iter().any(|attribute| {
                    attribute.stage_id == stage_id && attribute.key == selected_definition.key
                });

                if duplicate {
                    overlay.add_toast(adw::Toast::new(
                        "This attribute is already set for this stage",
                    ));
                    return;
                }
            }

            let attribute = build_attribute(
                existing_id,
                profile_id,
                stage_id,
                selected_definition.key.clone(),
                value_num,
                value_text,
                value_bool,
            );

            let before = existing_id
                .and_then(|id| {
                    store
                        .attributes(profile_id)
                        .ok()?
                        .into_iter()
                        .find(|row| row.id == id)
                })
                .as_ref()
                .map(changes::attribute);

            let result = write_with_history(
                &store,
                |store| store.set_attribute(&attribute),
                |store, id| {
                    let after = store
                        .attributes(profile_id)?
                        .into_iter()
                        .find(|row| row.id == *id);
                    record_row_change(
                        store,
                        profile_id,
                        "attribute",
                        *id,
                        before,
                        after.as_ref().map(changes::attribute),
                    )
                },
            );

            match result {
                Ok(_) => {
                    nav.pop();
                }
                Err(error) => {
                    eprintln!("could not save attribute: {error}");
                    let message =
                        save_error_message(error.as_ref(), "Could not save the attribute");
                    overlay.add_toast(adw::Toast::new(&message));
                }
            }
        }
    });

    let page = adw::NavigationPage::new(&toolbar, title);
    guard.attach(&page, &toolbar);
    page
}

fn apply_attribute_stage(
    definition: Option<&AttributeDefinition>,
    stage: &adw::ComboRow,
    locked: bool,
) {
    let stage_scoped = definition.is_some_and(|definition| definition.applies_to == "stage");

    if !stage_scoped && !locked {
        stage.set_selected(0);
    }
    stage.set_sensitive(!locked && stage_scoped);
}

fn apply_attribute_definition(
    definition: Option<&AttributeDefinition>,
    number: &adw::EntryRow,
    text: &adw::EntryRow,
    boolean: &adw::SwitchRow,
    enumeration: &adw::ComboRow,
    unit: &gtk::Label,
    pref: UnitPreference,
) {
    let value_type = definition
        .map(|definition| definition.value_type.as_str())
        .unwrap_or("text");

    number.set_visible(value_type == "number");
    text.set_visible(value_type == "text" || value_type == "json");
    text.set_title(if value_type == "json" {
        "JSON value"
    } else {
        "Value"
    });
    boolean.set_visible(value_type == "boolean");
    enumeration.set_visible(value_type == "enum");

    let unit_text = definition
        .and_then(|definition| definition.unit.as_deref())
        .unwrap_or("");
    let custom = definition
        .map(|definition| definition.is_custom)
        .unwrap_or(false);
    unit.set_text(&if custom {
        unit_text.to_string()
    } else {
        pref.label(unit_text)
    });
    unit.set_visible(value_type == "number" && !unit_text.is_empty());

    if value_type == "enum"
        && let Some(definition) = definition
    {
        let values: Vec<&str> = definition
            .enum_values
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        let labels: Vec<String> = values.iter().map(|value| humanize(value)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let model = gtk::StringList::new(&labels);
        enumeration.set_model(Some(&model));
        enumeration.set_selected(0);
    }
}

fn enum_value(row: &adw::ComboRow, definition: &AttributeDefinition) -> Option<String> {
    let values: Vec<&str> = definition
        .enum_values
        .as_deref()?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect();

    values
        .get(row.selected() as usize)
        .map(|value| (*value).to_string())
}

fn custom_key_form(store: &Rc<Store>, nav: &adw::NavigationView) -> adw::NavigationPage {
    let group = adw::PreferencesGroup::new();
    group.set_title("Custom key");
    group.set_description(Some(
        "Use this when no registered attribute fits. The key is stable and used by the app and agents.",
    ));

    let key = entry_row("Key", None);
    key.set_tooltip_text(Some("Lowercase letters, digits and underscores"));
    let label = entry_row("Label", None);
    let description = entry_row("Description", None);
    let category = choice_row("Category", ATTRIBUTE_CATEGORIES, Some("Other"));
    let value_type = value_type_row(Some("number"));
    let unit = entry_row("Unit", None);
    let enum_values = entry_row("Allowed values (comma-separated)", None);

    group.add(&key);
    group.add(&label);
    group.add(&description);
    group.add(&category);
    group.add(&value_type);
    group.add(&unit);
    group.add(&enum_values);

    let preferences = adw::PreferencesPage::new();
    preferences.add(&group);

    let guard = Guard::new(nav);
    let header = guard.header("New custom key");

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    apply_value_type_visibility(&value_type, &enum_values);

    value_type.connect_selected_notify({
        let enum_values = enum_values.clone();
        move |row| {
            apply_value_type_visibility(row, &enum_values);
        }
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    save.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let key = key.clone();
        let label = label.clone();
        let description = description.clone();
        let category = category.clone();
        let value_type = value_type.clone();
        let unit = unit.clone();
        let enum_values = enum_values.clone();

        move |_| {
            let key_text = key.text().trim().to_string();
            if !valid_custom_key(&key_text) {
                overlay.add_toast(adw::Toast::new(
                    "Use lowercase letters, digits and underscores for the key",
                ));
                return;
            }

            let label_text = label.text().trim().to_string();
            if label_text.is_empty() {
                overlay.add_toast(adw::Toast::new("Give the custom key a label"));
                return;
            }

            let value_type_value =
                value_type_value(&value_type).unwrap_or_else(|| "number".to_string());
            let enum_values_text = if value_type_value == "enum" {
                clean_optional(&enum_values.text())
            } else {
                None
            };

            if value_type_value == "enum" && enum_values_text.is_none() {
                overlay.add_toast(adw::Toast::new("List the allowed values"));
                return;
            }

            let definition = AttributeDefinition {
                key: key_text,
                label: label_text,
                description: clean_optional(&description.text()),
                value_type: value_type_value,
                unit: clean_optional(&unit.text()),
                category: choice_value(&category, ATTRIBUTE_CATEGORIES)
                    .unwrap_or_else(|| "Other".to_string()),
                applies_to: "profile".to_string(),
                allowed_stages: None,
                allowed_lifecycles: None,
                enum_values: enum_values_text,
                is_custom: true,
            };

            match store.add_custom_attribute_definition(&definition) {
                Ok(()) => {
                    nav.pop();
                }
                Err(error) => {
                    eprintln!("could not create custom attribute key: {error}");
                    let message =
                        save_error_message(error.as_ref(), "Could not create the custom key");
                    overlay.add_toast(adw::Toast::new(&message));
                }
            }
        }
    });

    let page = adw::NavigationPage::new(&toolbar, "New custom key");
    guard.attach(&page, &toolbar);
    page
}

fn value_type_row(selected: Option<&str>) -> adw::ComboRow {
    let labels: Vec<String> = VALUE_TYPES
        .iter()
        .map(|value_type| {
            if *value_type == "json" {
                "JSON".to_string()
            } else {
                humanize(value_type)
            }
        })
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&labels);
    let row = adw::ComboRow::builder()
        .title("Value type")
        .model(&model)
        .build();
    row.set_selected(
        selected
            .and_then(|selected| {
                VALUE_TYPES
                    .iter()
                    .position(|value_type| *value_type == selected)
            })
            .map(|index| index as u32)
            .unwrap_or(0),
    );
    row
}

fn value_type_value(row: &adw::ComboRow) -> Option<String> {
    VALUE_TYPES
        .get(row.selected() as usize)
        .map(|value_type| (*value_type).to_string())
}

fn apply_value_type_visibility(value_type: &adw::ComboRow, enum_values: &adw::EntryRow) {
    let is_enum = value_type_value(value_type).as_deref() == Some("enum");
    enum_values.set_visible(is_enum);
}

fn valid_custom_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

fn choice_row(title: &str, values: &[&str], selected: Option<&str>) -> adw::ComboRow {
    let mut labels = Vec::with_capacity(values.len() + 1);
    labels.push("Not set".to_string());
    labels.extend(values.iter().map(|value| humanize(value)));
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&labels);

    let row = adw::ComboRow::builder().title(title).model(&model).build();
    row.set_selected(selected_index(values, selected));
    row
}

fn selected_index(values: &[&str], selected: Option<&str>) -> u32 {
    selected
        .and_then(|selected| values.iter().position(|value| *value == selected))
        .map(|index| index as u32 + 1)
        .unwrap_or(0)
}

fn choice_value(row: &adw::ComboRow, values: &[&str]) -> Option<String> {
    let selected = row.selected();
    if selected == 0 {
        return None;
    }

    values
        .get(selected as usize - 1)
        .map(|value| (*value).to_string())
}

fn obligate_choice_row(value: Option<bool>) -> adw::ComboRow {
    let model = gtk::StringList::new(&["Not set", "Facultative", "Obligate"]);
    let row = adw::ComboRow::builder()
        .title("Photoperiod requirement")
        .model(&model)
        .build();
    row.set_selected(match value {
        Some(false) => 1,
        Some(true) => 2,
        None => 0,
    });
    row
}

fn obligate_value(row: &adw::ComboRow) -> Option<bool> {
    match row.selected() {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

fn container_choice_row(value: Option<bool>) -> adw::ComboRow {
    let model = gtk::StringList::new(&["Not set", "Suitable", "Not suitable"]);
    let row = adw::ComboRow::builder()
        .title("Container")
        .model(&model)
        .build();
    row.set_selected(match value {
        Some(true) => 1,
        Some(false) => 2,
        None => 0,
    });
    row
}

fn container_choice_value(row: &adw::ComboRow) -> Option<bool> {
    match row.selected() {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

fn entry_row(title: &str, text: Option<&str>) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    if let Some(text) = text {
        row.set_text(text);
    }
    row
}

fn number_entry_row(title: &str, value: Option<f64>) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row.set_input_purpose(gtk::InputPurpose::Number);
    if let Some(value) = value {
        row.set_text(&format_number(value));
    }
    row
}

fn number_entry_row_with_unit(
    title: &str,
    value: Option<f64>,
    unit: &str,
    pref: UnitPreference,
) -> (adw::EntryRow, gtk::Label) {
    let row = number_entry_row(title, None);
    if let Some(value) = value {
        row.set_text(&pref.number(unit, value));
    }
    let label = gtk::Label::new(Some(&pref.label(unit)));
    label.add_css_class("dim-label");
    if !unit.is_empty() {
        row.add_suffix(&label);
    }
    (row, label)
}

fn number_entry_row_unit(
    title: &str,
    value: Option<f64>,
    unit: &str,
    pref: UnitPreference,
) -> adw::EntryRow {
    number_entry_row_with_unit(title, value, unit, pref).0
}

/// `Some(original)` when the text still matches what was shown, so an untouched field keeps the
/// exact canonical value instead of round-tripping through the display unit.
fn keep_original(text: &str, shown: &str, original: Option<f64>) -> Option<Option<f64>> {
    (text.trim() == shown.trim()).then_some(original)
}

/// A numeric entry in the user's unit that remembers the canonical value it started with.
/// Saving an untouched row must not round the stored value through the display unit.
#[derive(Clone)]
struct UnitField {
    row: adw::EntryRow,
    unit: String,
    pref: UnitPreference,
    original: Option<f64>,
    shown: String,
}

impl UnitField {
    fn new(title: &str, value: Option<f64>, unit: &str, pref: UnitPreference) -> Self {
        let shown = value
            .map(|value| pref.number(unit, value))
            .unwrap_or_default();
        let row = number_entry_row(title, None);
        row.set_text(&shown);

        if !unit.is_empty() {
            let label = gtk::Label::new(Some(&pref.label(unit)));
            label.add_css_class("dim-label");
            row.add_suffix(&label);
        }

        Self {
            row,
            unit: unit.to_string(),
            pref,
            original: value,
            shown,
        }
    }

    fn value(&self, label: &str) -> Result<Option<f64>, String> {
        if let Some(original) = keep_original(&self.row.text(), &self.shown, self.original) {
            return Ok(original);
        }

        optional_quantity(&self.row, label, &self.unit, self.pref)
    }
}

fn integer_entry_row(title: &str, value: Option<i64>) -> adw::EntryRow {
    let row = adw::EntryRow::new();
    row.set_title(title);
    row.set_input_purpose(gtk::InputPurpose::Number);
    if let Some(value) = value {
        row.set_text(&value.to_string());
    }
    row
}

fn build_attribute(
    id: Option<i64>,
    profile_id: i64,
    stage_id: Option<i64>,
    key: String,
    value_num: Option<f64>,
    value_text: Option<String>,
    value_bool: Option<bool>,
) -> Attribute {
    Attribute {
        id: id.unwrap_or(0),
        profile_id,
        stage_id,
        key,
        value_num,
        value_text,
        value_bool,
    }
}

fn build_requirement(
    id: Option<i64>,
    profile_id: i64,
    stage_id: Option<i64>,
    metric: String,
    day_part: String,
    values: [Option<f64>; 6],
) -> Requirement {
    Requirement {
        id: id.unwrap_or(0),
        profile_id,
        stage_id,
        metric,
        day_part,
        survival_min: values[0],
        survival_max: values[1],
        min: values[2],
        max: values[3],
        opt_min: values[4],
        opt_max: values[5],
    }
}

struct StageValues<'a> {
    label: &'a str,
    duration: (Option<f64>, Option<f64>),
    notes: &'a str,
    repeats: Option<bool>,
}

fn build_stage(
    stage_id: Option<i64>,
    profile_id: i64,
    position: i64,
    kind: Option<String>,
    values: StageValues<'_>,
) -> Result<Stage, String> {
    let label = values.label.trim();
    if label.is_empty() {
        return Err("Give the stage a name".to_string());
    }

    let kind = kind.ok_or_else(|| "Choose a stage kind".to_string())?;

    Ok(Stage {
        id: stage_id.unwrap_or(0),
        profile_id,
        position,
        kind,
        label: label.to_string(),
        duration_days_min: values.duration.0,
        duration_days_max: values.duration.1,
        notes: clean_optional(values.notes),
        repeats: values.repeats,
    })
}

fn collect_profile_details(
    id: i64,
    name: &str,
    species: &str,
    cultivar: &str,
    classification: &ClassificationFields,
    growing: &GrowingFields,
) -> Result<ProfileDetails, String> {
    Ok(ProfileDetails {
        id,
        name: name.to_string(),
        species: clean_optional(species),
        cultivar: clean_optional(cultivar),
        family: clean_optional(&classification.family.text()),
        genus: clean_optional(&classification.genus.text()),
        lifecycle: choice_value(&classification.lifecycle, LIFECYCLES),
        life_form: choice_value(&classification.life_form, LIFE_FORMS),
        foliage: choice_value(&classification.foliage, FOLIAGE),
        determinacy: choice_value(&classification.determinacy, DETERMINACY),
        growth_habit: choice_value(&classification.growth_habit, GROWTH_HABITS),
        sun_exposure: choice_value(&growing.light, SUN_EXPOSURES),
        photoperiod_type: choice_value(&growing.photoperiod, PHOTOPERIOD_TYPES),
        photoperiod_obligate: obligate_value(&growing.photoperiod_obligate),
        critical_day_length_hours: optional_number(
            &growing.critical_day_length,
            "Critical day length",
        )?,
        water_need: choice_value(&growing.water_need, WATER_NEEDS),
        container_suitable: container_choice_value(&growing.container),
        min_container_volume_l_min: growing
            .min_container_volume
            .value("Minimum container volume")?,
        min_container_volume_l_max: growing
            .max_container_volume
            .value("Maximum container volume")?,
        days_to_maturity_min: optional_number(
            &growing.days_to_maturity_min,
            "Days to maturity minimum",
        )?,
        days_to_maturity_max: optional_number(
            &growing.days_to_maturity_max,
            "Days to maturity maximum",
        )?,
        notes: clean_optional(&growing.notes.text()),
    })
}

fn optional_number(row: &adw::EntryRow, label: &str) -> Result<Option<f64>, String> {
    parse_optional_number(&row.text())
        .map_err(|_| format!("{label}: enter a number or leave it blank"))
}

fn optional_quantity(
    row: &adw::EntryRow,
    label: &str,
    unit: &str,
    pref: UnitPreference,
) -> Result<Option<f64>, String> {
    optional_number(row, label).map(|value| value.map(|value| pref.input_to_canonical(unit, value)))
}

fn optional_integer(row: &adw::EntryRow, label: &str) -> Result<Option<i64>, String> {
    parse_optional_integer(&row.text())
        .map_err(|_| format!("{label}: enter a whole number or leave it blank"))
}

fn parse_optional_number(text: &str) -> Result<Option<f64>, ()> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return Ok(None);
    }

    let value: f64 = text.parse().map_err(|_| ())?;
    if !value.is_finite() {
        return Err(());
    }

    Ok(Some(value))
}

fn parse_optional_integer(text: &str) -> Result<Option<i64>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }

    text.parse().map(Some).map_err(|_| ())
}

fn clean_optional(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn save_error_message(error: &(dyn std::error::Error + 'static), fallback: &str) -> String {
    error
        .downcast_ref::<ValidationError>()
        .map(|validation| validation.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        StageValues, TimingValues, attribute_value, build_attribute, build_requirement,
        build_stage, build_timing_rule, format_number, format_range, humanize,
        parse_optional_integer, parse_optional_number, record_row_change, requirement_range,
        selected_index, timing_offset, valid_custom_key,
    };
    use crate::plant_data::{Attribute, AttributeDefinition, Provenance, Requirement, TimingRule};
    use crate::ui::profile_overview::{requirement_value, round_to_tenth};
    use crate::units::UnitPreference;

    #[test]
    fn temperature_axis_rounds_float_noise() {
        assert_eq!(round_to_tenth(2.1999999999999997), 2.2);
        assert_eq!(round_to_tenth(42.8), 42.8);
        assert_eq!(round_to_tenth(-1.25), -1.3);
    }

    #[test]
    fn an_untouched_field_keeps_its_canonical_value() {
        assert_eq!(
            super::keep_original(" 2.64 ", "2.64", Some(10.0)),
            Some(Some(10.0))
        );
        assert_eq!(super::keep_original("", "", None), Some(None));
        assert_eq!(super::keep_original("2.65", "2.64", Some(10.0)), None);
    }

    #[test]
    fn requirement_ranges_and_attributes_follow_the_preference() {
        let requirement = Requirement {
            id: 0,
            profile_id: 1,
            stage_id: None,
            metric: "air_temp".to_string(),
            day_part: "day".to_string(),
            min: Some(10.0),
            max: Some(30.0),
            opt_min: Some(20.0),
            opt_max: Some(26.0),
            survival_min: Some(0.0),
            survival_max: Some(35.0),
        };

        assert_eq!(
            requirement_range(&requirement, UnitPreference::Imperial).as_deref(),
            Some("50–86 °F · optimum 68–78.8 °F · survival 32–95 °F")
        );

        let definition = |key: &str, unit: &str, custom: bool| AttributeDefinition {
            key: key.to_string(),
            label: key.to_string(),
            description: None,
            value_type: "number".to_string(),
            unit: Some(unit.to_string()),
            category: "About".to_string(),
            applies_to: "profile".to_string(),
            allowed_stages: None,
            allowed_lifecycles: None,
            enum_values: None,
            is_custom: custom,
        };
        let attribute = |key: &str, value: f64| Attribute {
            id: 1,
            profile_id: 1,
            stage_id: None,
            key: key.to_string(),
            value_num: Some(value),
            value_text: None,
            value_bool: None,
        };

        let height = definition("mature_height_cm", "cm", false);
        assert_eq!(
            attribute_value(
                &attribute("mature_height_cm", 120.0),
                Some(&height),
                UnitPreference::Imperial
            )
            .as_deref(),
            Some("47.2 in")
        );

        let difference = definition("dif_c", "°C", false);
        assert_eq!(
            attribute_value(
                &attribute("dif_c", 10.0),
                Some(&difference),
                UnitPreference::Imperial
            )
            .as_deref(),
            Some("18 °F")
        );

        let custom = definition("custom_height", "cm", true);
        assert_eq!(
            attribute_value(
                &attribute("custom_height", 120.0),
                Some(&custom),
                UnitPreference::Imperial
            )
            .as_deref(),
            Some("120 cm")
        );
    }

    #[test]
    fn a_tile_prefers_the_optimum_range_then_the_growth_range() {
        let mut requirement = Requirement {
            id: 1,
            profile_id: 1,
            stage_id: None,
            metric: "air_temp".to_string(),
            day_part: "day".to_string(),
            min: Some(10.0),
            max: Some(30.0),
            opt_min: Some(20.0),
            opt_max: Some(26.0),
            survival_min: Some(0.0),
            survival_max: Some(35.0),
        };

        assert_eq!(
            requirement_value(&[requirement.clone()], "air_temp", UnitPreference::Metric)
                .as_deref(),
            Some("20–26 °C")
        );

        requirement.opt_min = None;
        requirement.opt_max = None;
        assert_eq!(
            requirement_value(&[requirement.clone()], "air_temp", UnitPreference::Metric)
                .as_deref(),
            Some("10–30 °C")
        );

        requirement.min = None;
        requirement.max = None;
        assert_eq!(
            requirement_value(&[requirement], "air_temp", UnitPreference::Metric).as_deref(),
            Some("0–35 °C")
        );

        assert_eq!(
            requirement_value(&[], "air_temp", UnitPreference::Metric),
            None
        );
    }

    #[test]
    fn formats_numbers_and_ranges() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(80.0), "80");
        assert_eq!(format_number(1.5), "1.5");
        assert_eq!(format_number(1.25), "1.25");
        assert_eq!(format_number(1.2345), "1.2345");
        assert_eq!(
            format_range(Some(10.0), Some(20.0), "L", UnitPreference::Metric).as_deref(),
            Some("10–20 L")
        );
        assert_eq!(
            format_range(None, Some(50.0), "%", UnitPreference::Metric).as_deref(),
            Some("up to 50%")
        );
        assert_eq!(
            format_range(Some(6.0), None, "pH", UnitPreference::Metric).as_deref(),
            Some("from 6 pH")
        );
        assert_eq!(format_range(None, None, "°C", UnitPreference::Metric), None);
    }

    #[test]
    fn humanizes_enum_values() {
        assert_eq!(humanize("full_sun"), "Full sun");
        assert_eq!(humanize("day_neutral"), "Day neutral");
        assert_eq!(humanize("not_applicable"), "Not applicable");
        assert_eq!(humanize(""), "");
    }

    #[test]
    fn formats_requirement_ranges() {
        let requirement = Requirement {
            id: 1,
            profile_id: 1,
            stage_id: None,
            metric: "air_temp".to_string(),
            day_part: "day".to_string(),
            min: Some(10.0),
            max: Some(30.0),
            opt_min: Some(20.0),
            opt_max: Some(26.0),
            survival_min: Some(0.0),
            survival_max: Some(35.0),
        };

        assert_eq!(
            requirement_range(&requirement, UnitPreference::Metric).as_deref(),
            Some("10–30 °C · optimum 20–26 °C · survival 0–35 °C")
        );
    }

    #[test]
    fn maps_choice_indexes_and_optional_numbers() {
        let values = ["annual", "perennial"];

        assert_eq!(selected_index(&values, None), 0);
        assert_eq!(selected_index(&values, Some("annual")), 1);
        assert_eq!(selected_index(&values, Some("perennial")), 2);
        assert_eq!(selected_index(&values, Some("unknown")), 0);

        assert_eq!(parse_optional_number(""), Ok(None));
        assert_eq!(parse_optional_number(" 12,5 "), Ok(Some(12.5)));
        assert!(parse_optional_number("abc").is_err());
    }

    #[test]
    fn builds_a_stage_from_form_values() {
        let stage = build_stage(
            Some(4),
            2,
            3,
            Some("flowering".to_string()),
            StageValues {
                label: " Bloom ",
                duration: (Some(14.0), Some(28.0)),
                notes: " note ",
                repeats: Some(true),
            },
        )
        .unwrap();

        assert_eq!(stage.id, 4);
        assert_eq!(stage.profile_id, 2);
        assert_eq!(stage.position, 3);
        assert_eq!(stage.kind, "flowering");
        assert_eq!(stage.label, "Bloom");
        assert_eq!(stage.duration_days_min, Some(14.0));
        assert_eq!(stage.duration_days_max, Some(28.0));
        assert_eq!(stage.notes.as_deref(), Some("note"));
        assert_eq!(stage.repeats, Some(true));

        assert!(
            build_stage(
                None,
                1,
                1,
                Some("vegetative".into()),
                StageValues {
                    label: " ",
                    duration: (None, None),
                    notes: "",
                    repeats: None,
                },
            )
            .is_err()
        );
        assert!(
            build_stage(
                None,
                1,
                1,
                None,
                StageValues {
                    label: "Bloom",
                    duration: (None, None),
                    notes: "",
                    repeats: None,
                },
            )
            .is_err()
        );
    }

    #[test]
    fn builds_a_requirement_from_form_values() {
        let requirement = build_requirement(
            Some(7),
            2,
            Some(4),
            "air_temp".to_string(),
            "day".to_string(),
            [
                Some(0.0),
                Some(35.0),
                Some(10.0),
                Some(30.0),
                Some(20.0),
                Some(26.0),
            ],
        );

        assert_eq!(requirement.id, 7);
        assert_eq!(requirement.profile_id, 2);
        assert_eq!(requirement.stage_id, Some(4));
        assert_eq!(requirement.metric, "air_temp");
        assert_eq!(requirement.day_part, "day");
        assert_eq!(requirement.survival_min, Some(0.0));
        assert_eq!(requirement.survival_max, Some(35.0));
        assert_eq!(requirement.min, Some(10.0));
        assert_eq!(requirement.max, Some(30.0));
        assert_eq!(requirement.opt_min, Some(20.0));
        assert_eq!(requirement.opt_max, Some(26.0));
    }

    #[test]
    fn builds_a_timing_rule_from_form_values() {
        let rule = build_timing_rule(
            Some(5),
            2,
            TimingValues {
                stage_id: Some(3),
                kind: "sow_indoor".to_string(),
                anchor: "last_frost".to_string(),
                offset_min_days: Some(-56),
                offset_max_days: Some(-42),
                month_start: Some(3),
                month_end: Some(5),
                day_length_hours: None,
                gdd_base_c: None,
                gdd_target: None,
                repeat_days: Some(14),
                notes: Some("Start under cover.".to_string()),
            },
        );

        assert_eq!(rule.id, 5);
        assert_eq!(rule.profile_id, 2);
        assert_eq!(rule.stage_id, Some(3));
        assert_eq!(rule.kind, "sow_indoor");
        assert_eq!(rule.anchor, "last_frost");
        assert_eq!(rule.offset_min_days, Some(-56));
        assert_eq!(rule.offset_max_days, Some(-42));
        assert_eq!(rule.month_start, Some(3));
        assert_eq!(rule.month_end, Some(5));
        assert_eq!(rule.repeat_days, Some(14));
        assert_eq!(rule.notes.as_deref(), Some("Start under cover."));
    }

    #[test]
    fn builds_an_attribute_from_form_values() {
        let attribute = build_attribute(
            Some(9),
            2,
            Some(3),
            "soil_drainage".to_string(),
            None,
            Some("free".to_string()),
            None,
        );

        assert_eq!(attribute.id, 9);
        assert_eq!(attribute.profile_id, 2);
        assert_eq!(attribute.stage_id, Some(3));
        assert_eq!(attribute.key, "soil_drainage");
        assert_eq!(attribute.value_num, None);
        assert_eq!(attribute.value_text.as_deref(), Some("free"));
        assert_eq!(attribute.value_bool, None);
    }

    #[test]
    fn validates_custom_keys() {
        assert!(valid_custom_key("pot_width_cm"));
        assert!(valid_custom_key("ec_target_2"));
        assert!(!valid_custom_key(""));
        assert!(!valid_custom_key("Pot Width"));
        assert!(!valid_custom_key("pot-width"));
    }

    #[test]
    fn parses_optional_integers() {
        assert_eq!(parse_optional_integer(""), Ok(None));
        assert_eq!(parse_optional_integer(" 14 "), Ok(Some(14)));
        assert_eq!(parse_optional_integer("-56"), Ok(Some(-56)));
        assert!(parse_optional_integer("1.5").is_err());
        assert!(parse_optional_integer("abc").is_err());
    }

    #[test]
    fn formats_timing_offsets() {
        let mut rule = TimingRule {
            id: 1,
            profile_id: 1,
            stage_id: None,
            kind: "sow_indoor".to_string(),
            anchor: "last_frost".to_string(),
            offset_min_days: Some(-56),
            offset_max_days: Some(-42),
            month_start: None,
            month_end: None,
            day_length_hours: None,
            gdd_base_c: None,
            gdd_target: None,
            repeat_days: None,
            notes: None,
        };

        assert_eq!(
            timing_offset(&rule, "the last frost"),
            "42–56 days before the last frost"
        );

        rule.offset_min_days = Some(0);
        rule.offset_max_days = Some(14);
        assert_eq!(
            timing_offset(&rule, "the last frost"),
            "0–14 days after the last frost"
        );

        rule.offset_min_days = Some(-7);
        rule.offset_max_days = Some(7);
        assert_eq!(
            timing_offset(&rule, "transplanting"),
            "-7 to 7 days relative to transplanting"
        );
    }

    #[test]
    fn saving_an_unchanged_row_keeps_its_provenance() {
        use crate::store::Store;

        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Chilli", None, None).unwrap();
        let stage = crate::plant_data::Stage {
            id: 0,
            profile_id: profile,
            position: 1,
            kind: "germination".to_string(),
            label: "Germination".to_string(),
            duration_days_min: Some(3.0),
            duration_days_max: Some(5.0),
            notes: None,
            repeats: None,
        };
        let id = store.create_stage(&stage).unwrap();
        store
            .set_provenance(&Provenance {
                entity_type: "stage".to_string(),
                entity_id: id,
                field: "*".to_string(),
                status: "estimated".to_string(),
                source_type: Some("book".to_string()),
                source_ref: Some("RHS".to_string()),
                retrieved_at: None,
                confidence: Some("high".to_string()),
                notes: None,
            })
            .unwrap();

        let snapshot = crate::changes::stage(&store.stage(id).unwrap().unwrap());
        record_row_change(
            &store,
            profile,
            "stage",
            id,
            Some(snapshot.clone()),
            Some(snapshot),
        )
        .unwrap();

        let after = store.provenance("stage", id, "*").unwrap().unwrap();
        assert_eq!(after.status, "estimated");
        assert_eq!(after.source_ref.as_deref(), Some("RHS"));
        assert_eq!(after.confidence.as_deref(), Some("high"));
        assert!(store.changes(profile, 10).unwrap().is_empty());

        let before = crate::changes::stage(&store.stage(id).unwrap().unwrap());
        let mut changed = store.stage(id).unwrap().unwrap();
        changed.label = "Sprouting".to_string();
        store.update_stage(&changed).unwrap();
        let after = crate::changes::stage(&store.stage(id).unwrap().unwrap());
        record_row_change(&store, profile, "stage", id, Some(before), Some(after)).unwrap();

        let confirmed = store.provenance("stage", id, "*").unwrap().unwrap();
        assert_eq!(confirmed.status, "confirmed");
        assert_eq!(confirmed.source_type.as_deref(), Some("user"));
        assert_eq!(store.changes(profile, 10).unwrap().len(), 1);
    }
}
