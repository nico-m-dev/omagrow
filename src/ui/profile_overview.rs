//! The read-only profile overview: value cards, at-a-glance tiles and the temperature guide.

use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::attribute_seeds::ATTRIBUTE_CATEGORIES;
use crate::frost::{self, FrostDates};
use crate::plant_data::{
    Attribute, ProfileDetails, Requirement, RequirementResolution, Stage, TimingRule,
    format_number, metric_unit,
};
use crate::store::Store;
use crate::units::UnitPreference;

use super::profile_editors::{
    add_detail_row, attribute_definitions, attribute_value, attributes_page, container_value,
    edit_row, empty_row, format_range, frost_range, metric_order, photoperiod_value, profile_edit,
    requirement_range, requirement_title, requirements_page, stage_names, stages_page,
    timing_order, timing_page, timing_subtitle, timing_title,
};
use super::shared::fields::humanize;
use super::shared::provenance::provenance_control;

/// Read-only overview of the plant data. Editing happens on the editor pages.
pub(super) fn growing_data(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    profile_id: i64,
) -> gtk::Widget {
    let (details, stages, requirements, timing, attributes) =
        match read_profile_data(store, profile_id) {
            Ok(data) => data,
            Err(error) => {
                eprintln!("could not read plant data for profile {profile_id}: {error}");
                return unreadable_profile(&error.to_string());
            }
        };

    let container = gtk::Box::new(gtk::Orientation::Vertical, 18);
    container.append(&cycle_group(store, nav, profile_id, &stages));

    if let Some(group) = requirements_group(store, nav, profile_id, &stages, &requirements) {
        container.append(&group);
    }
    if let Some(group) = timing_group(store, nav, profile_id, &stages, &timing) {
        container.append(&group);
    }
    if let Some(group) = traits_group(store, nav, root_page, profile_id, details.as_ref()) {
        container.append(&group);
    }
    if let Some(group) = attributes_group(store, nav, profile_id, &attributes) {
        container.append(&group);
    }

    container.upcast()
}

type ProfileData = (
    Option<ProfileDetails>,
    Vec<Stage>,
    Vec<Requirement>,
    Vec<TimingRule>,
    Vec<Attribute>,
);

fn read_profile_data(store: &Store, profile_id: i64) -> crate::store::Result<ProfileData> {
    Ok((
        store.profile_details(profile_id)?,
        store.stages(profile_id)?,
        store.requirements(profile_id)?,
        store.timing_rules(profile_id)?,
        store.attributes(profile_id)?,
    ))
}

/// A database read failed. Say so instead of showing an empty cycle that invites re-entry.
fn unreadable_profile(error: &str) -> gtk::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Could not read this profile");
    group.set_description(Some(
        "Nothing was changed. Open the profile again; if it keeps failing, check the database.",
    ));
    group.add(&empty_row("The plant data could not be loaded", error));
    group.upcast()
}

fn cycle_group(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
) -> gtk::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Growing cycle");

    if stages.is_empty() {
        group.set_description(Some(
            "The cycle is still empty. Add the stages in order; requirements and timing can then \
             attach to them.",
        ));
        group.add(&empty_row(
            "No stages yet",
            "Add germination, seedling, flowering and harvest, or whatever this plant does.",
        ));
        group.add(&edit_row("Add stages", "Build the growing cycle.", {
            let store = store.clone();
            let nav = nav.clone();
            Rc::new(move || {
                nav.push(&stages_page(&store, &nav, profile_id));
            })
        }));
        return group.upcast();
    }

    let longest = stages
        .iter()
        .filter_map(|stage| stage.duration_days_max.or(stage.duration_days_min))
        .fold(0.0_f64, f64::max);

    let total_min: f64 = stages
        .iter()
        .map(|stage| stage.duration_days_min.unwrap_or(0.0))
        .sum();
    let total_max: f64 = stages
        .iter()
        .map(|stage| stage.duration_days_max.unwrap_or(0.0))
        .sum();

    let pref = UnitPreference::load(store);
    let perennial = store
        .profile_details(profile_id)
        .ok()
        .flatten()
        .and_then(|profile| profile.lifecycle)
        .as_deref()
        == Some("perennial");

    let mut description = "The cycle in order. The bar follows each stage's duration.".to_string();
    if total_min > 0.0 && total_max > 0.0 {
        description.push_str(&format!(
            " About {}–{} days in total.",
            format_number(total_min),
            format_number(total_max)
        ));
    } else if stages
        .iter()
        .any(|stage| stage.duration_days_min.is_none() && stage.duration_days_max.is_none())
    {
        description.push_str(
            " Some stages have no duration yet; the timeline stops there and later stages are not \
             repeated.",
        );
    }
    if perennial {
        let repeating: Vec<&Stage> = stages
            .iter()
            .filter(|stage| stage.repeats_each_year())
            .collect();
        let mature_continues = repeating.len() == 1
            && repeating[0].kind == "mature"
            && stages.last().is_some_and(|last| last.id == repeating[0].id)
            && stages.iter().all(|stage| {
                stage
                    .duration_days_min
                    .or(stage.duration_days_max)
                    .is_some()
            });
        if repeating.is_empty() {
            description.push_str(
                " No stage repeats each year, so the timeline stops after the first cycle.",
            );
        } else if mature_continues {
            description.push_str(
                " The final mature stage continues until the end; it is not restarted each year.",
            );
        } else {
            description.push_str(&format!(
                " {} of {} stages return each year.",
                repeating.len(),
                stages.len()
            ));
        }
    }
    group.set_description(Some(&description));

    for (index, stage) in stages.iter().enumerate() {
        let title = format!("{}. {}", index + 1, stage.label);
        let mut subtitle = vec![humanize(&stage.kind)];
        if let Some(range) = format_range(
            stage.duration_days_min,
            stage.duration_days_max,
            "days",
            pref,
        ) {
            subtitle.push(range);
        } else {
            subtitle.push("no duration yet".to_string());
        }
        if perennial {
            subtitle.push(if stage.repeats_each_year() {
                "repeats yearly".to_string()
            } else {
                "first cycle only".to_string()
            });
        }

        let row = adw::ActionRow::builder()
            .title(&title)
            .subtitle(subtitle.join(" · "))
            .build();
        row.set_activatable(false);
        row.set_focusable(false);

        let suffix = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        suffix.set_valign(gtk::Align::Center);

        if longest > 0.0
            && let Some(days) = stage.duration_days_max.or(stage.duration_days_min)
        {
            let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            bar.add_css_class("omagrow-stage-bar");
            bar.set_valign(gtk::Align::Center);
            bar.set_width_request((12.0 + (days / longest) * 108.0).round() as i32);
            suffix.append(&bar);
        }

        suffix.append(&provenance_control(
            store,
            "stage",
            stage.id,
            vec!["*".to_string()],
        ));
        row.add_suffix(&suffix);
        group.add(&row);
    }

    group.add(&edit_row(
        "Edit cycle",
        "Reorder stages, add durations and adjust the steps.",
        {
            let store = store.clone();
            let nav = nav.clone();
            Rc::new(move || {
                nav.push(&stages_page(&store, &nav, profile_id));
            })
        },
    ));

    group.upcast()
}

fn requirements_group(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    requirements: &[Requirement],
) -> Option<gtk::Widget> {
    if requirements.is_empty() {
        return None;
    }

    let pref = UnitPreference::load(store);
    let group = adw::PreferencesGroup::new();
    group.set_title("What it needs");
    group.set_description(Some(
        "Ranges per stage and day part. A stage value overrides the profile default.",
    ));

    let names = stage_names(stages);
    let positions: std::collections::HashMap<i64, i64> = stages
        .iter()
        .map(|stage| (stage.id, stage.position))
        .collect();
    let mut ordered: Vec<&Requirement> = requirements.iter().collect();
    ordered.sort_by_key(|requirement| {
        (
            metric_order(&requirement.metric),
            requirement
                .stage_id
                .and_then(|id| positions.get(&id).copied())
                .unwrap_or(0),
            requirement.day_part.clone(),
        )
    });

    for requirement in ordered {
        let stage = match requirement.stage_id {
            Some(id) => names.get(&id).map(String::as_str).unwrap_or("Stage"),
            None => "Profile default",
        };
        let range = requirement_range(requirement, pref).unwrap_or_else(|| "Not set".to_string());
        let current_status = store
            .provenance("requirement", requirement.id, "*")
            .ok()
            .flatten()
            .map(|provenance| provenance.status);
        let override_note =
            if requirement.stage_id.is_some() && current_status.as_deref() == Some("estimated") {
                let parts: &[&str] = match requirement.day_part.as_str() {
                    "any" => &["day", "night"],
                    part => &[part],
                };
                let overrides = parts.iter().any(|part| {
                    matches!(
                        store
                            .resolved_requirement(profile_id, None, &requirement.metric, part)
                            .ok()
                            .flatten(),
                        Some(RequirementResolution::Resolved(default))
                            if store
                                .provenance("requirement", default.id, "*")
                                .ok()
                                .flatten()
                                .is_some_and(|provenance| provenance.status == "confirmed")
                    )
                });
                if overrides {
                    " · overrides your confirmed default"
                } else {
                    ""
                }
            } else {
                ""
            };
        let subtitle = format!("{stage} · {range}{override_note}");

        let row = adw::ActionRow::builder()
            .title(requirement_title(requirement))
            .subtitle(&subtitle)
            .build();
        row.set_activatable(false);
        row.set_focusable(false);
        row.set_subtitle_lines(2);
        row.add_suffix(&provenance_control(
            store,
            "requirement",
            requirement.id,
            vec!["*".to_string()],
        ));
        group.add(&row);
    }

    group.add(&requirements_action_row(
        store,
        nav,
        profile_id,
        "Edit requirements",
    ));
    Some(group.upcast())
}

fn timing_group(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    stages: &[Stage],
    rules: &[TimingRule],
) -> Option<gtk::Widget> {
    if rules.is_empty() {
        return None;
    }

    let group = adw::PreferencesGroup::new();
    group.set_title("Timing");

    let frost_dates = FrostDates::load(store).unwrap_or_default();
    let needs_frost = rules
        .iter()
        .any(|rule| matches!(rule.anchor.as_str(), "last_frost" | "first_frost"));

    group.set_description(Some(if needs_frost && frost_dates.is_empty() {
        "Relative rules such as sowing before the last frost. Set your frost dates in Settings to \
         see calendar dates."
    } else {
        "Relative rules such as sowing before the last frost or harvesting after transplant."
    }));

    let today = crate::clock::today();
    let names = stage_names(stages);
    let positions: HashMap<i64, i64> = stages
        .iter()
        .map(|stage| (stage.id, stage.position))
        .collect();
    let mut ordered: Vec<&TimingRule> = rules.iter().collect();
    ordered.sort_by_key(|rule| {
        let stage_order = rule
            .stage_id
            .and_then(|id| positions.get(&id).copied())
            .unwrap_or(0);
        (timing_order(&rule.kind), stage_order, rule.id)
    });

    for rule in ordered {
        let mut subtitle = match frost::resolve(rule, &frost_dates, today.year) {
            Some((start, end)) => format!(
                "{} · {}",
                frost_range(start, end, today.year),
                timing_subtitle(rule, &names)
            ),
            None => timing_subtitle(rule, &names),
        };

        match rule.anchor.as_str() {
            "sowing" | "transplant" if !rules.iter().any(|other| other.kind == rule.anchor) => {
                subtitle.push_str(&format!(
                    " · no {} rule yet to resolve this date",
                    rule.anchor
                ));
            }
            "day_length" => subtitle.push_str(" · needs a day-length source"),
            "gdd" => subtitle.push_str(" · needs weather data"),
            _ => {}
        }

        let row = adw::ActionRow::builder()
            .title(timing_title(rule))
            .subtitle(&subtitle)
            .build();
        row.set_activatable(false);
        row.set_focusable(false);
        row.set_subtitle_lines(3);
        if let Some(notes) = rule
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
        {
            row.set_tooltip_text(Some(notes));
        }
        row.add_suffix(&provenance_control(
            store,
            "timing_rule",
            rule.id,
            vec!["*".to_string()],
        ));
        group.add(&row);
    }

    group.add(&edit_row(
        "Edit timing",
        "Sowing, transplanting, harvest and other relative rules.",
        {
            let store = store.clone();
            let nav = nav.clone();
            Rc::new(move || {
                nav.push(&timing_page(&store, &nav, profile_id));
            })
        },
    ));
    Some(group.upcast())
}

fn traits_group(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    profile_id: i64,
    details: Option<&ProfileDetails>,
) -> Option<gtk::Widget> {
    let details = details?;
    let pref = UnitPreference::load(store);

    let mut rows: Vec<(Vec<&str>, &str, Option<String>)> = Vec::new();

    if details.family.is_some() {
        rows.push((vec!["family"], "Family", details.family.clone()));
    }
    if details.genus.is_some() {
        rows.push((vec!["genus"], "Genus", details.genus.clone()));
    }
    if details.life_form.is_some() {
        rows.push((
            vec!["life_form"],
            "Life form",
            details.life_form.as_deref().map(humanize),
        ));
    }
    if details.foliage.is_some() {
        rows.push((
            vec!["foliage"],
            "Foliage",
            details.foliage.as_deref().map(humanize),
        ));
    }
    if details.determinacy.is_some() {
        rows.push((
            vec!["determinacy"],
            "Determinacy",
            details.determinacy.as_deref().map(humanize),
        ));
    }
    if details.growth_habit.is_some() {
        rows.push((
            vec!["growth_habit"],
            "Growth habit",
            details.growth_habit.as_deref().map(humanize),
        ));
    }

    let mut photoperiod_fields = Vec::new();
    if details.photoperiod_type.is_some() {
        photoperiod_fields.push("photoperiod_type");
    }
    if details.photoperiod_obligate.is_some() {
        photoperiod_fields.push("photoperiod_obligate");
    }
    if details.critical_day_length_hours.is_some() {
        photoperiod_fields.push("critical_day_length_hours");
    }
    if !photoperiod_fields.is_empty() {
        rows.push((
            photoperiod_fields,
            "Photoperiod",
            photoperiod_value(details),
        ));
    }

    let mut container_fields = Vec::new();
    if details.min_container_volume_l_min.is_some() {
        container_fields.push("min_container_volume_l_min");
    }
    if details.min_container_volume_l_max.is_some() {
        container_fields.push("min_container_volume_l_max");
    }
    if !container_fields.is_empty() {
        rows.push((
            container_fields,
            "Container volume",
            format_range(
                details.min_container_volume_l_min,
                details.min_container_volume_l_max,
                "L",
                pref,
            ),
        ));
    }

    if details.notes.is_some() {
        rows.push((vec!["notes"], "Notes", details.notes.clone()));
    }

    let group = adw::PreferencesGroup::new();
    let mut count = 0;
    for (fields, title, value) in rows {
        if add_detail_row(&group, store, profile_id, &fields, title, value) {
            count += 1;
        }
    }

    if count == 0 {
        return None;
    }

    group.add(&edit_row(
        "Edit plant traits",
        "Family, form, light and the other core traits.",
        {
            let store = store.clone();
            let nav = nav.clone();
            let root_page = root_page.clone();
            Rc::new(move || {
                nav.push(&profile_edit(&store, &nav, &root_page, profile_id));
            })
        },
    ));

    let expander = gtk::Expander::new(Some(&format!("Plant traits · {count} values")));
    expander.set_child(Some(&group));
    Some(expander.upcast())
}

fn attributes_group(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    attributes: &[Attribute],
) -> Option<gtk::Widget> {
    if attributes.is_empty() {
        return None;
    }

    let pref = UnitPreference::load(store);
    let definitions = attribute_definitions(store);
    let stage_names = store
        .stages(profile_id)
        .map(|stages| stage_names(&stages))
        .unwrap_or_default();
    let groups = gtk::Box::new(gtk::Orientation::Vertical, 18);

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
        group.set_title(&gtk::glib::markup_escape_text(category));
        for attribute in rows {
            let definition = definitions.get(&attribute.key);
            let title = definition
                .map(|definition| definition.label.clone())
                .unwrap_or_else(|| humanize(&attribute.key));
            let value = attribute_value(attribute, definition, pref)
                .unwrap_or_else(|| "Not set".to_string());
            let stage = match attribute.stage_id {
                Some(id) => stage_names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| "Stage".to_string()),
                None => "Profile".to_string(),
            };

            let row = adw::ActionRow::builder()
                .title(&title)
                .subtitle(format!("{stage} · {value}"))
                .build();
            row.set_activatable(false);
            row.set_focusable(false);
            row.set_subtitle_lines(3);
            if let Some(definition) = definition {
                row.set_tooltip_text(definition.description.as_deref());
            }
            row.add_suffix(&provenance_control(
                store,
                "attribute",
                attribute.id,
                vec!["*".to_string()],
            ));
            group.add(&row);
        }
        groups.append(&group);
    }

    let edit_group = adw::PreferencesGroup::new();
    edit_group.add(&edit_row(
        "Edit attributes",
        "The long tail: propagation, pruning, harvest and the rest.",
        {
            let store = store.clone();
            let nav = nav.clone();
            Rc::new(move || {
                nav.push(&attributes_page(&store, &nav, profile_id));
            })
        },
    ));
    groups.append(&edit_group);

    let expander = gtk::Expander::new(Some(&format!(
        "About this plant · {} values",
        attributes.len()
    )));
    expander.set_child(Some(&groups));
    Some(expander.upcast())
}

/// Where an at-a-glance tile comes from, so its chip confirms the right value.
enum TileSource {
    Profile(Vec<String>),
    Requirement(i64),
}

pub(super) fn stat_tiles(store: &Rc<Store>, profile_id: i64) -> Option<gtk::Widget> {
    let details = store.profile_details(profile_id).ok().flatten()?;
    let requirements = store.requirements(profile_id).unwrap_or_default();
    let pref = UnitPreference::load(store);

    let mut entries: Vec<(&str, String, TileSource)> = Vec::new();

    if let Some(lifecycle) = details.lifecycle.as_deref() {
        entries.push((
            "Lifecycle",
            humanize(lifecycle),
            TileSource::Profile(vec!["lifecycle".to_string()]),
        ));
    }
    if let Some((light, source)) = light_tile(&details, &requirements, pref) {
        entries.push(("Light", light, source));
    }
    if let Some((water, source)) = water_tile(&details, &requirements, pref) {
        entries.push(("Water", water, source));
    }
    if let Some(requirement) = requirements
        .iter()
        .find(|requirement| requirement.metric == "air_temp")
        && let Some(temperature) = requirement_value(&requirements, "air_temp", pref)
    {
        entries.push((
            "Temperature",
            temperature,
            TileSource::Requirement(requirement.id),
        ));
    }
    if let Some(container) = container_value(&details, pref) {
        let mut fields = Vec::new();
        if details.container_suitable.is_some() {
            fields.push("container_suitable".to_string());
        }
        if details.min_container_volume_l_min.is_some() {
            fields.push("min_container_volume_l_min".to_string());
        }
        if details.min_container_volume_l_max.is_some() {
            fields.push("min_container_volume_l_max".to_string());
        }
        entries.push(("Container", container, TileSource::Profile(fields)));
    }
    if let Some(maturity) = format_range(
        details.days_to_maturity_min,
        details.days_to_maturity_max,
        "days",
        pref,
    ) {
        let mut fields = Vec::new();
        if details.days_to_maturity_min.is_some() {
            fields.push("days_to_maturity_min".to_string());
        }
        if details.days_to_maturity_max.is_some() {
            fields.push("days_to_maturity_max".to_string());
        }
        entries.push(("Days to maturity", maturity, TileSource::Profile(fields)));
    }

    if entries.len() < 2 {
        return None;
    }

    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_max_children_per_line(3);
    flow.set_min_children_per_line(1);
    flow.set_column_spacing(10);
    flow.set_row_spacing(10);

    for (caption, value, source) in entries {
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 2);
        tile.add_css_class("omagrow-tile");
        tile.set_hexpand(true);

        let caption_label = gtk::Label::new(Some(caption));
        caption_label.set_xalign(0.0);
        caption_label.add_css_class("omagrow-tile-caption");
        tile.append(&caption_label);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let value_label = gtk::Label::new(Some(&value));
        value_label.set_xalign(0.0);
        value_label.set_wrap(true);
        value_label.set_hexpand(true);
        value_label.add_css_class("omagrow-tile-value");
        row.append(&value_label);

        let control = match source {
            TileSource::Profile(fields) => provenance_control(store, "profile", profile_id, fields),
            TileSource::Requirement(id) => {
                provenance_control(store, "requirement", id, vec!["*".to_string()])
            }
        };
        row.append(&control);
        tile.append(&row);

        flow.insert(&tile, -1);
    }

    Some(flow.upcast())
}

fn light_tile(
    details: &ProfileDetails,
    requirements: &[Requirement],
    pref: UnitPreference,
) -> Option<(String, TileSource)> {
    if let Some(exposure) = details.sun_exposure.as_deref() {
        return Some((
            humanize(exposure),
            TileSource::Profile(vec!["sun_exposure".to_string()]),
        ));
    }
    if let Some(photoperiod) = details.photoperiod_type.as_deref() {
        let mut fields = vec!["photoperiod_type".to_string()];
        if details.photoperiod_obligate.is_some() {
            fields.push("photoperiod_obligate".to_string());
        }
        if details.critical_day_length_hours.is_some() {
            fields.push("critical_day_length_hours".to_string());
        }
        return Some((humanize(photoperiod), TileSource::Profile(fields)));
    }

    for metric in ["photoperiod", "direct_sun_hours", "light_dli"] {
        if let Some(requirement) = requirements
            .iter()
            .find(|requirement| requirement.metric == metric)
            && let Some(value) = requirement_value(requirements, metric, pref)
        {
            return Some((value, TileSource::Requirement(requirement.id)));
        }
    }

    None
}

fn water_tile(
    details: &ProfileDetails,
    requirements: &[Requirement],
    pref: UnitPreference,
) -> Option<(String, TileSource)> {
    if let Some(need) = details.water_need.as_deref() {
        return Some((
            humanize(need),
            TileSource::Profile(vec!["water_need".to_string()]),
        ));
    }

    for metric in ["water_l_week", "water_mm_week"] {
        if let Some(requirement) = requirements
            .iter()
            .find(|requirement| requirement.metric == metric)
            && let Some(value) = requirement_value(requirements, metric, pref)
        {
            return Some((value, TileSource::Requirement(requirement.id)));
        }
    }

    None
}

/// The optimum range for a metric, falling back to the growth range and then survival.
pub(in crate::ui) fn requirement_value(
    requirements: &[Requirement],
    metric: &str,
    pref: UnitPreference,
) -> Option<String> {
    let requirement = requirements
        .iter()
        .find(|requirement| requirement.metric == metric)?;
    let unit = metric_unit(metric);

    format_range(requirement.opt_min, requirement.opt_max, unit, pref)
        .or_else(|| format_range(requirement.min, requirement.max, unit, pref))
        .or_else(|| {
            format_range(
                requirement.survival_min,
                requirement.survival_max,
                unit,
                pref,
            )
        })
}

/// The temperature card: every band on one scale, including the bands that are still unknown.
pub(super) fn temperature_bands(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
) -> Option<gtk::Widget> {
    let requirements = store.requirements(profile_id).ok()?;
    let temperature: Vec<&Requirement> = requirements
        .iter()
        .filter(|requirement| requirement.metric == "air_temp")
        .collect();
    if temperature.is_empty() {
        return None;
    }

    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for requirement in &temperature {
        for value in [
            requirement.survival_min,
            requirement.survival_max,
            requirement.min,
            requirement.max,
            requirement.opt_min,
            requirement.opt_max,
        ]
        .into_iter()
        .flatten()
        {
            low = low.min(value);
            high = high.max(value);
        }
    }

    if !low.is_finite() || !high.is_finite() {
        return None;
    }
    if high <= low {
        high = low + 1.0;
    }

    // Give the extremes some air so a bar never touches the edge of its track. Round the axis
    // bounds to one decimal; the raw padding produces values like 2.1999999999999997.
    let padding = (high - low) * 0.08;
    let low = round_to_tenth(low - padding);
    let high = round_to_tenth(high + padding);

    let unit = metric_unit("air_temp");
    let pref = UnitPreference::load(store);
    let group = adw::PreferencesGroup::new();
    group.set_title("Temperature");
    group.set_description(Some(
        "Day and night ranges share one scale. An empty band is simply not known yet.",
    ));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(8);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let show_labels = temperature.len() > 1;
    for requirement in &temperature {
        let label = if show_labels {
            Some(match requirement.day_part.as_str() {
                "night" => "Night",
                "day" => "Day",
                other => other,
            })
        } else {
            None
        };

        content.append(&temperature_section(
            requirement,
            low,
            high,
            unit,
            label,
            pref,
        ));
    }

    if !temperature
        .iter()
        .any(|requirement| requirement.day_part == "night")
    {
        let note = gtk::Label::new(Some("Night is not set yet."));
        note.set_xalign(0.0);
        note.add_css_class("dim-label");
        note.add_css_class("caption");
        content.append(&note);
    }

    let actions = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    actions.add_css_class("boxed-list");
    actions.append(&requirements_action_row(
        store,
        nav,
        profile_id,
        "Temperature ranges",
    ));
    content.append(&actions);
    group.add(&content);
    Some(group.upcast())
}

pub(in crate::ui) fn round_to_tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[allow(deprecated)]
fn temperature_section(
    requirement: &Requirement,
    low: f64,
    high: f64,
    unit: &str,
    label: Option<&str>,
    pref: UnitPreference,
) -> gtk::Widget {
    let boxed = gtk::Box::new(gtk::Orientation::Vertical, 4);

    if let Some(label) = label {
        let heading = gtk::Label::new(Some(label));
        heading.set_xalign(0.0);
        heading.add_css_class("heading");
        boxed.append(&heading);
    }

    let grid = gtk::Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(6);

    let bands = [
        (
            "Survives",
            requirement.survival_min,
            requirement.survival_max,
        ),
        ("Grows", requirement.min, requirement.max),
        ("Ideal", requirement.opt_min, requirement.opt_max),
    ];

    for (index, (band, minimum, maximum)) in bands.into_iter().enumerate() {
        let text = match (minimum, maximum) {
            (None, None) => format!("{band} · not set"),
            _ => format!(
                "{band} · {}",
                format_range(minimum, maximum, unit, pref).unwrap_or_else(|| "not set".to_string())
            ),
        };

        let name = gtk::Label::new(Some(&text));
        name.set_xalign(1.0);
        name.set_size_request(150, -1);
        name.add_css_class("dim-label");
        if minimum.is_none() && maximum.is_none() {
            name.add_css_class("omagrow-empty-band");
        }
        grid.attach(&name, 0, index as i32, 1, 1);

        let area = gtk::DrawingArea::new();
        area.set_content_height(16);
        area.set_hexpand(true);
        area.set_valign(gtk::Align::Center);
        let band_name = band.to_string();
        area.set_draw_func(move |area, cr, width, height| {
            let width = f64::from(width);
            let height = f64::from(height);
            let bar = 8.0;
            let top = (height - bar) / 2.0;
            let span = high - low;

            let foreground = area.color();
            let accent = area
                .style_context()
                .lookup_color("accent_color")
                .unwrap_or(foreground);

            cr.set_source_rgba(
                f64::from(foreground.red()),
                f64::from(foreground.green()),
                f64::from(foreground.blue()),
                0.10,
            );
            rounded_rectangle(cr, 0.0, top, width, bar, bar / 2.0);
            let _ = cr.fill();

            match (minimum, maximum) {
                (Some(minimum), Some(maximum)) => {
                    let start = ((minimum - low) / span).clamp(0.0, 1.0) * width;
                    let end = ((maximum - low) / span).clamp(0.0, 1.0) * width;
                    let (red, green, blue, alpha) = match band_name.as_str() {
                        "Ideal" => (
                            f64::from(accent.red()),
                            f64::from(accent.green()),
                            f64::from(accent.blue()),
                            1.0,
                        ),
                        "Grows" => (
                            f64::from(accent.red()),
                            f64::from(accent.green()),
                            f64::from(accent.blue()),
                            0.55,
                        ),
                        _ => (
                            f64::from(foreground.red()),
                            f64::from(foreground.green()),
                            f64::from(foreground.blue()),
                            0.32,
                        ),
                    };

                    cr.set_source_rgba(red, green, blue, alpha);
                    rounded_rectangle(cr, start, top, (end - start).max(2.0), bar, bar / 2.0);
                    let _ = cr.fill();
                }
                (Some(value), None) | (None, Some(value)) => {
                    let x = ((value - low) / span).clamp(0.0, 1.0) * width;
                    cr.set_source_rgba(
                        f64::from(accent.red()),
                        f64::from(accent.green()),
                        f64::from(accent.blue()),
                        0.8,
                    );
                    cr.rectangle((x - 1.5).max(0.0), top - 2.0, 3.0, bar + 4.0);
                    let _ = cr.fill();
                }
                (None, None) => {
                    cr.set_dash(&[3.0, 3.0], 0.0);
                    cr.set_source_rgba(
                        f64::from(foreground.red()),
                        f64::from(foreground.green()),
                        f64::from(foreground.blue()),
                        0.28,
                    );
                    cr.move_to(0.0, height / 2.0);
                    cr.line_to(width, height / 2.0);
                    let _ = cr.stroke();
                    cr.set_dash(&[], 0.0);
                }
            }
        });

        grid.attach(&area, 1, index as i32, 1, 1);
    }

    let axis = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let low_label = gtk::Label::new(Some(&format!(
        "{} {}",
        pref.number(unit, low),
        pref.label(unit)
    )));
    low_label.set_xalign(0.0);
    low_label.add_css_class("omagrow-axis");
    let high_label = gtk::Label::new(Some(&format!(
        "{} {}",
        pref.number(unit, high),
        pref.label(unit)
    )));
    high_label.set_xalign(1.0);
    high_label.set_hexpand(true);
    high_label.add_css_class("omagrow-axis");
    axis.append(&low_label);
    axis.append(&high_label);
    grid.attach(&axis, 1, bands.len() as i32, 1, 1);

    boxed.append(&grid);
    boxed.upcast()
}

fn requirements_action_row(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    profile_id: i64,
    title: &str,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle("Add or change the ranges behind this card.")
        .build();
    row.set_activatable(true);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row.connect_activated({
        let store = store.clone();
        let nav = nav.clone();
        move |_| {
            nav.push(&requirements_page(&store, &nav, profile_id));
        }
    });
    row
}

fn rounded_rectangle(
    cr: &gtk::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    let right = x + width - radius;
    let bottom = y + height - radius;

    cr.new_sub_path();
    cr.arc(right, y + radius, radius, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(right, bottom, radius, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + radius,
        bottom,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
}
