//! Activity and History: the change ledger as rows, with revert.

use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::changes;
use crate::plant_data::AttributeDefinition;
use crate::store::Store;
use crate::units::UnitPreference;

use super::page_header;
use super::profile_editors::{attribute_definitions, edit_row, month_name};
use super::shared::fields::humanize;
use crate::ui::profile_view;

pub(super) fn recent_changes_card(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    profile_id: i64,
) -> Option<gtk::Widget> {
    let history = store.changes(profile_id, 6).ok()?;
    if history.is_empty() {
        return None;
    }

    let definitions = attribute_definitions(store);
    let pref = UnitPreference::load(store);
    let group = adw::PreferencesGroup::new();
    group.set_title("Activity");
    group.set_description(Some(
        "The newest changes, with the value that changed. Revert puts the value from before back.",
    ));

    for change in history.iter().take(5) {
        group.add(&change_row(
            store,
            nav,
            root_page,
            profile_id,
            change,
            &definitions,
            false,
            pref,
        ));
    }

    group.add(&edit_row(
        "View all changes",
        "The full history, grouped by day.",
        {
            let store = store.clone();
            let nav = nav.clone();
            let root_page = root_page.clone();
            Rc::new(move || {
                nav.push(&history_page(&store, &nav, &root_page, profile_id));
            })
        },
    ));

    Some(group.upcast())
}

pub(super) fn history_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    profile_id: i64,
) -> adw::NavigationPage {
    let history = store.changes(profile_id, 500).unwrap_or_default();
    let definitions = attribute_definitions(store);
    let pref = UnitPreference::load(store);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(32);
    content.set_margin_start(24);
    content.set_margin_end(24);

    if history.is_empty() {
        content.append(&gtk::Label::new(Some("No changes recorded yet.")));
    } else {
        let mut groups: Vec<(String, Vec<&changes::Change>)> = Vec::new();
        for change in &history {
            let date = local_date(&change.created_at);
            if let Some((_, rows)) = groups.iter_mut().find(|(key, _)| *key == date) {
                rows.push(change);
            } else {
                groups.push((date, vec![change]));
            }
        }

        for (date, rows) in groups {
            let group = adw::PreferencesGroup::new();
            group.set_title(&date);
            for change in rows {
                group.add(&change_row(
                    store,
                    nav,
                    root_page,
                    profile_id,
                    change,
                    &definitions,
                    true,
                    pref,
                ));
            }
            content.append(&group);
        }
    }

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
    toolbar.add_top_bar(&page_header("History"));
    toolbar.set_content(Some(&scroller));

    adw::NavigationPage::new(&toolbar, "History")
}

#[allow(clippy::too_many_arguments)]
fn change_row(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    profile_id: i64,
    change: &changes::Change,
    definitions: &HashMap<String, AttributeDefinition>,
    stay_in_history: bool,
    pref: UnitPreference,
) -> adw::ActionRow {
    let subtitle = format!(
        "{} · {} · {} · {}",
        history_summary(change, definitions, pref),
        if change.actor == changes::ACTOR_AGENT {
            "agent"
        } else {
            "you"
        },
        source_short(change),
        local_time(&change.created_at)
    );

    let row = adw::ActionRow::builder()
        .title(change_title(change, definitions))
        .subtitle(&subtitle)
        .build();
    row.set_activatable(false);
    row.set_subtitle_lines(3);
    if let Some(source) = source_full(change) {
        row.set_tooltip_text(Some(&source));
    }

    let revert = gtk::Button::with_label("Revert");
    revert.add_css_class("flat");
    revert.set_valign(gtk::Align::Center);
    revert.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let change = change.clone();
        move |_| {
            let message = match changes::revert(&store, &change) {
                Ok(message) => message,
                Err(message) => {
                    let dialog = adw::AlertDialog::builder()
                        .heading("Could not revert this change")
                        .body(message)
                        .build();
                    dialog.add_response("close", "Close");
                    dialog.set_default_response(Some("close"));
                    dialog.set_close_response("close");
                    let window = nav
                        .root()
                        .and_then(|root| root.downcast::<gtk::Window>().ok());
                    dialog.present(window.as_ref());
                    return;
                }
            };

            if stay_in_history {
                // Stay in History: rebuild the profile below it and put a fresh History page on
                // top. The new rows are the feedback.
                let history = history_page(&store, &nav, &root_page, profile_id);
                match profile_view(&store, &nav, &root_page, profile_id, None) {
                    Some(view) => {
                        nav.replace(&[root_page.clone(), view, history]);
                    }
                    None => {
                        nav.pop();
                    }
                }
            } else {
                match profile_view(&store, &nav, &root_page, profile_id, Some(&message)) {
                    Some(view) => {
                        nav.replace(&[root_page.clone(), view]);
                    }
                    None => {
                        nav.pop();
                    }
                }
            }
        }
    });
    row.add_suffix(&revert);
    row
}

fn history_summary(
    change: &changes::Change,
    definitions: &HashMap<String, AttributeDefinition>,
    pref: UnitPreference,
) -> String {
    if change.entity_type == "attribute"
        && let Some(key) = change_key(change)
        && let Some(definition) = definitions.get(&key)
    {
        let same = change.old_value == change.new_value;
        if definition.value_type == "enum" {
            let old = change.old_value.as_deref().map(enum_value_text);
            let new = change.new_value.as_deref().map(enum_value_text);
            return match (old, new) {
                (None, Some(new)) => format!("added · {new}"),
                (Some(old), None) => format!("removed · {old}"),
                (Some(_), Some(new)) if same => new,
                (Some(old), Some(new)) => format!("{old} → {new}"),
                (None, None) => "changed".to_string(),
            };
        }

        if definition.value_type == "number" {
            let pref = if definition.is_custom {
                UnitPreference::Metric
            } else {
                pref
            };
            let unit = definition.unit.as_deref().unwrap_or("");
            let delta = key == "dif_c";
            let text = |value: &str| -> Option<String> {
                let value = serde_json::from_str::<serde_json::Value>(value).ok()?;
                let number = value.get("value_num").and_then(serde_json::Value::as_f64)?;
                Some(if delta {
                    format!("{} {}", pref.delta_number(unit, number), pref.label(unit))
                } else {
                    pref.text(unit, number)
                })
            };

            let old = change.old_value.as_deref().and_then(text);
            let new = change.new_value.as_deref().and_then(text);
            return match (old, new) {
                (None, Some(new)) => format!("added · {new}"),
                (Some(old), None) => format!("removed · {old}"),
                (Some(_), Some(new)) if same => new,
                (Some(old), Some(new)) => format!("{old} → {new}"),
                (None, None) => "changed".to_string(),
            };
        }
    }

    changes::summary(change, pref)
}

fn enum_value_text(value: &str) -> String {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|value| {
            value
                .get("value_text")
                .and_then(|text| text.as_str())
                .map(humanize)
        })
        .unwrap_or_else(|| value.to_string())
}

fn change_title(
    change: &changes::Change,
    definitions: &HashMap<String, AttributeDefinition>,
) -> String {
    if change.entity_type == "attribute"
        && let Some(key) = change_key(change)
        && let Some(definition) = definitions.get(&key)
    {
        return definition.label.clone();
    }

    changes::title(change)
}

fn change_key(change: &changes::Change) -> Option<String> {
    [change.new_value.as_deref(), change.old_value.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|value| {
            serde_json::from_str::<serde_json::Value>(value)
                .ok()
                .and_then(|value| {
                    value
                        .get("key")
                        .and_then(|key| key.as_str())
                        .map(str::to_string)
                })
        })
}

fn source_short(change: &changes::Change) -> String {
    match (change.source_type.as_deref(), change.source_ref.as_deref()) {
        (Some(kind), Some(reference)) => {
            let reference = if reference.starts_with("http") {
                reference
                    .split("://")
                    .nth(1)
                    .and_then(|rest| rest.split('/').next())
                    .unwrap_or(reference)
                    .to_string()
            } else {
                reference.to_string()
            };
            format!("{} · {}", humanize(kind), reference)
        }
        (Some(kind), None) => humanize(kind),
        _ => "no source".to_string(),
    }
}

fn source_full(change: &changes::Change) -> Option<String> {
    match (change.source_type.as_deref(), change.source_ref.as_deref()) {
        (Some(kind), Some(reference)) => Some(format!("{}: {reference}", humanize(kind))),
        (Some(kind), None) => Some(humanize(kind)),
        _ => None,
    }
}

fn local_time(timestamp: &str) -> String {
    if let Ok(time) =
        gtk::glib::DateTime::from_iso8601(timestamp, Some(&gtk::glib::TimeZone::local()))
        && let Ok(local) = time.to_timezone(&gtk::glib::TimeZone::local())
        && let Ok(formatted) = local.format("%d %b %H:%M")
    {
        return formatted.to_string();
    }

    fallback_time(timestamp)
}

fn local_date(timestamp: &str) -> String {
    if let Ok(time) =
        gtk::glib::DateTime::from_iso8601(timestamp, Some(&gtk::glib::TimeZone::local()))
        && let Ok(local) = time.to_timezone(&gtk::glib::TimeZone::local())
        && let Ok(formatted) = local.format("%d %B %Y")
    {
        return formatted.to_string();
    }

    timestamp.get(0..10).unwrap_or(timestamp).to_string()
}

fn fallback_time(timestamp: &str) -> String {
    let Some(date) = timestamp.get(0..10) else {
        return timestamp.to_string();
    };
    let time = timestamp.get(11..16).unwrap_or("");
    let mut parts = date.split('-');
    let _year = parts.next();
    let month = parts.next().and_then(|month| month.parse::<i64>().ok());
    let day = parts.next().and_then(|day| day.parse::<i64>().ok());

    match (day, month) {
        (Some(day), Some(month)) => format!("{day} {} {time}", month_name(month)),
        _ => timestamp.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{enum_value_text, history_summary};
    use crate::changes::Change;
    use crate::plant_data::AttributeDefinition;
    use crate::units::UnitPreference;
    use std::collections::HashMap;

    #[test]
    fn enum_values_read_as_words_in_history() {
        assert_eq!(enum_value_text(r#"{"value_text":"full_sun"}"#), "Full sun");
    }

    #[test]
    fn history_summary_uses_the_registry_label_for_an_enum() {
        let mut definitions = HashMap::new();
        definitions.insert(
            "climate_type".to_string(),
            AttributeDefinition {
                key: "climate_type".to_string(),
                label: "Climate type".to_string(),
                description: None,
                value_type: "enum".to_string(),
                unit: None,
                category: "Climate".to_string(),
                applies_to: "profile".to_string(),
                allowed_stages: None,
                allowed_lifecycles: None,
                enum_values: Some("tropical,subtropical".to_string()),
                is_custom: false,
            },
        );
        let change = Change {
            id: 1,
            profile_id: 1,
            entity_type: "attribute".to_string(),
            entity_id: 1,
            field: "*".to_string(),
            old_value: None,
            new_value: Some(r#"{"key":"climate_type","value_text":"subtropical"}"#.to_string()),
            source_type: Some("website".to_string()),
            source_ref: None,
            actor: "agent".to_string(),
            created_at: String::new(),
        };

        assert_eq!(
            history_summary(&change, &definitions, UnitPreference::Metric),
            "added · Subtropical"
        );
    }
    #[test]
    fn history_shows_units_and_follows_the_preference() {
        let definition = |key: &str, unit: &str| AttributeDefinition {
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
            is_custom: false,
        };
        let mut definitions = HashMap::new();
        definitions.insert(
            "mature_height_cm".to_string(),
            definition("mature_height_cm", "cm"),
        );
        definitions.insert("dif_c".to_string(), definition("dif_c", "°C"));

        let change = |key: &str, old: f64, new: f64| Change {
            id: 1,
            profile_id: 1,
            entity_type: "attribute".to_string(),
            entity_id: 1,
            field: "*".to_string(),
            old_value: Some(format!(r#"{{"key":"{key}","value_num":{old}}}"#)),
            new_value: Some(format!(r#"{{"key":"{key}","value_num":{new}}}"#)),
            source_type: Some("website".to_string()),
            source_ref: None,
            actor: "agent".to_string(),
            created_at: String::new(),
        };

        assert_eq!(
            history_summary(
                &change("mature_height_cm", 120.0, 150.0),
                &definitions,
                UnitPreference::Metric
            ),
            "120 cm → 150 cm"
        );
        assert_eq!(
            history_summary(
                &change("mature_height_cm", 120.0, 150.0),
                &definitions,
                UnitPreference::Imperial
            ),
            "47.2 in → 59.1 in"
        );
        assert_eq!(
            history_summary(
                &change("dif_c", 10.0, 12.0),
                &definitions,
                UnitPreference::Imperial
            ),
            "18 °F → 21.6 °F"
        );

        // A change smaller than the display precision still shows an arrow; canonical JSON decides.
        assert_eq!(
            history_summary(
                &change("dif_c", 10.0, 10.02),
                &definitions,
                UnitPreference::Imperial
            ),
            "18 °F → 18 °F"
        );

        let mut removed = change("mature_height_cm", 120.0, 120.0);
        removed.new_value = None;
        assert_eq!(
            history_summary(&removed, &definitions, UnitPreference::Metric),
            "removed · 120 cm"
        );

        let mut custom_definitions = HashMap::new();
        custom_definitions.insert(
            "custom_height".to_string(),
            AttributeDefinition {
                is_custom: true,
                ..definition("custom_height", "cm")
            },
        );
        assert_eq!(
            history_summary(
                &change("custom_height", 120.0, 150.0),
                &custom_definitions,
                UnitPreference::Imperial
            ),
            "120 cm → 150 cm"
        );
    }
}
