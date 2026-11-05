//! The provenance chip and its actions, shared by every value row.

use std::rc::Rc;

use gtk::prelude::*;

use super::fields::humanize;
use crate::plant_data::Provenance;
use crate::provenance;
use crate::store::Store;

pub(in crate::ui) fn provenance_control(
    store: &Rc<Store>,
    entity_type: &str,
    entity_id: i64,
    fields: Vec<String>,
) -> gtk::Widget {
    let button = gtk::MenuButton::new();
    button.add_css_class("flat");
    button.add_css_class("omagrow-status-button");
    button.set_always_show_arrow(false);
    button.set_valign(gtk::Align::Center);

    let label = gtk::Label::new(None);
    button.set_child(Some(&label));

    let popover = gtk::Popover::new();
    let actions = gtk::Box::new(gtk::Orientation::Vertical, 2);
    actions.set_margin_top(6);
    actions.set_margin_bottom(6);
    actions.set_margin_start(6);
    actions.set_margin_end(6);
    popover.set_child(Some(&actions));
    button.set_popover(Some(&popover));

    refresh_provenance_control(
        &button,
        &label,
        &actions,
        &popover,
        store,
        entity_type,
        entity_id,
        &fields,
    );

    button.upcast()
}

#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn refresh_provenance_control(
    button: &gtk::MenuButton,
    label: &gtk::Label,
    actions: &gtk::Box,
    popover: &gtk::Popover,
    store: &Rc<Store>,
    entity_type: &str,
    entity_id: i64,
    fields: &[String],
) {
    while let Some(child) = actions.first_child() {
        actions.remove(&child);
    }

    let provenances: Vec<Provenance> = fields
        .iter()
        .filter_map(|field| {
            store
                .provenance(entity_type, entity_id, field)
                .ok()
                .flatten()
        })
        .collect();
    let summary = provenance_summary(&provenances, fields.len());
    let (text, class) = match summary.as_str() {
        "confirmed" => ("Confirmed", "confirmed"),
        "estimated" => ("Estimated", "estimated"),
        "not_applicable" => ("N/A", "not-applicable"),
        "mixed" => ("Mixed", "mixed"),
        _ => ("Unverified", "unverified"),
    };

    label.set_text(text);
    label.set_css_classes(&["omagrow-chip", class]);

    let tooltip = if provenances.is_empty() {
        "No status or source yet. Click to confirm or mark this value not applicable.".to_string()
    } else {
        distinct_provenances(&provenances)
            .iter()
            .filter_map(provenance_tooltip)
            .collect::<Vec<_>>()
            .join(" · ")
    };
    button.set_tooltip_text(Some(&tooltip));

    let add_action = |text: &str, run: Rc<dyn Fn()>| {
        let action = gtk::Button::with_label(text);
        action.add_css_class("flat");
        action.set_halign(gtk::Align::Start);
        action.connect_clicked({
            let popover = popover.clone();
            move |_| {
                run();
                popover.popdown();
            }
        });
        actions.append(&action);
    };

    if summary != "confirmed" {
        let store = store.clone();
        let button = button.clone();
        let label = label.clone();
        let actions = actions.clone();
        let popover = popover.clone();
        let entity_type = entity_type.to_string();
        let fields = fields.to_vec();
        add_action(
            "Confirm this value",
            Rc::new(move || {
                let references: Vec<&str> = fields.iter().map(String::as_str).collect();
                if let Err(error) =
                    provenance::confirm(&store, &entity_type, entity_id, &references)
                {
                    eprintln!("could not confirm the value: {error}");
                }
                refresh_provenance_control(
                    &button,
                    &label,
                    &actions,
                    &popover,
                    &store,
                    &entity_type,
                    entity_id,
                    &fields,
                );
            }),
        );
    }

    if summary == "confirmed" || summary == "mixed" {
        let store = store.clone();
        let button = button.clone();
        let label = label.clone();
        let actions = actions.clone();
        let popover = popover.clone();
        let entity_type = entity_type.to_string();
        let fields = fields.to_vec();
        add_action(
            "Unconfirm",
            Rc::new(move || {
                let references: Vec<&str> = fields.iter().map(String::as_str).collect();
                if let Err(error) =
                    provenance::unconfirm(&store, &entity_type, entity_id, &references)
                {
                    eprintln!("could not unconfirm the value: {error}");
                }
                refresh_provenance_control(
                    &button,
                    &label,
                    &actions,
                    &popover,
                    &store,
                    &entity_type,
                    entity_id,
                    &fields,
                );
            }),
        );
    }

    if summary != "not_applicable" {
        let store = store.clone();
        let button = button.clone();
        let label = label.clone();
        let actions = actions.clone();
        let popover = popover.clone();
        let entity_type = entity_type.to_string();
        let fields = fields.to_vec();
        add_action(
            "Mark as not applicable",
            Rc::new(move || {
                let references: Vec<&str> = fields.iter().map(String::as_str).collect();
                if let Err(error) =
                    provenance::mark_not_applicable(&store, &entity_type, entity_id, &references)
                {
                    eprintln!("could not mark the value not applicable: {error}");
                }
                refresh_provenance_control(
                    &button,
                    &label,
                    &actions,
                    &popover,
                    &store,
                    &entity_type,
                    entity_id,
                    &fields,
                );
            }),
        );
    }
}

/// One entry per distinct status, in field order. The two bounds of a range that share a status
/// share one entry; bounds that disagree show both instead of hiding one.
pub(in crate::ui) fn distinct_provenances(provenances: &[Provenance]) -> Vec<Provenance> {
    let mut distinct: Vec<Provenance> = Vec::new();

    for provenance in provenances {
        if distinct.iter().any(|item| item.status == provenance.status) {
            continue;
        }
        distinct.push(provenance.clone());
    }

    distinct
}

/// One word for a group of fields: all the same, mixed, or nothing recorded yet.
pub(in crate::ui) fn provenance_summary(provenances: &[Provenance], field_count: usize) -> String {
    if provenances.is_empty() {
        return "unverified".to_string();
    }

    let mut statuses: Vec<&str> = provenances
        .iter()
        .map(|provenance| provenance.status.as_str())
        .collect();
    statuses.sort_unstable();
    statuses.dedup();

    if statuses.len() == 1 && provenances.len() == field_count {
        statuses[0].to_string()
    } else {
        "mixed".to_string()
    }
}

pub(in crate::ui) fn provenance_tooltip(provenance: &Provenance) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(source_type) = provenance.source_type.as_deref() {
        parts.push(humanize(source_type));
    }
    if let Some(source_ref) = provenance.source_ref.as_deref() {
        parts.push(source_ref.to_string());
    }
    if let Some(retrieved_at) = provenance.retrieved_at.as_deref() {
        parts.push(retrieved_at.to_string());
    }
    if let Some(confidence) = provenance.confidence.as_deref() {
        parts.push(format!("{confidence} confidence"));
    }
    if let Some(notes) = provenance.notes.as_deref() {
        parts.push(notes.to_string());
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

#[cfg(test)]
mod tests {
    use super::distinct_provenances;
    use crate::plant_data::Provenance;

    #[test]
    fn keeps_one_chip_per_status() {
        let provenance = |field: &str, status: &str| Provenance {
            entity_type: "profile".to_string(),
            entity_id: 1,
            field: field.to_string(),
            status: status.to_string(),
            source_type: None,
            source_ref: None,
            retrieved_at: None,
            confidence: None,
            notes: None,
        };

        let same = distinct_provenances(&[
            provenance("days_to_maturity_min", "estimated"),
            provenance("days_to_maturity_max", "estimated"),
        ]);
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].status, "estimated");

        let mixed = distinct_provenances(&[
            provenance("min", "confirmed"),
            provenance("max", "estimated"),
            provenance("survival_max", "confirmed"),
        ]);
        assert_eq!(
            mixed
                .iter()
                .map(|item| item.status.as_str())
                .collect::<Vec<_>>(),
            vec!["confirmed", "estimated"]
        );
    }
}
