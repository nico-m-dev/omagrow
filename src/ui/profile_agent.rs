//! The proposals card and the proposal review page.

use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::proposal;
use crate::store::Store;
use crate::ui::profile_view;
use crate::units::UnitPreference;

use super::page_header;
use super::shared::fields::humanize;

/// Proposals waiting in the inbox for this profile. Nothing is written until the user reviews it.
pub(super) fn proposals_card(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    profile_id: i64,
) -> Option<gtk::Widget> {
    let pending = proposal::pending(store, profile_id);
    if pending.is_empty() {
        return None;
    }

    let group = adw::PreferencesGroup::new();
    group.set_title("Proposals");
    group.set_description(Some(
        "An agent proposed values for this profile. Nothing is written until you accept it.",
    ));

    for (path, result) in pending {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "proposal".to_string());

        let (title, subtitle) = match &result {
            Ok(proposal) => {
                let count = proposal.changes.len();
                (
                    name,
                    format!("{count} value{}", if count == 1 { "" } else { "s" }),
                )
            }
            Err(error) => (name, format!("Could not read this file: {error}")),
        };

        let row = adw::ActionRow::builder()
            .title(&title)
            .subtitle(&subtitle)
            .build();
        row.set_activatable(false);

        if let Ok(proposal) = result {
            let review = gtk::Button::with_label("Review");
            review.add_css_class("suggested-action");
            review.set_valign(gtk::Align::Center);
            review.connect_clicked({
                let nav = nav.clone();
                let store = store.clone();
                let root_page = root_page.clone();
                let path = path.clone();
                move |_| {
                    nav.push(&proposal_page(
                        &store, &nav, &root_page, profile_id, &path, &proposal,
                    ));
                }
            });
            row.add_suffix(&review);
        }

        group.add(&row);
    }

    Some(group.upcast())
}

fn proposal_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    profile_id: i64,
    path: &std::path::Path,
    proposal: &proposal::Proposal,
) -> adw::NavigationPage {
    let pref = UnitPreference::load(store);
    let group = adw::PreferencesGroup::new();
    group.set_title(&format!(
        "Proposal for {}",
        gtk::glib::markup_escape_text(&proposal.profile_name)
    ));
    group.set_description(Some(
        "Accepted values are stored as estimated with their source. Values you confirmed are refused.",
    ));

    let mut checks: Vec<gtk::CheckButton> = Vec::new();

    for change in &proposal.changes {
        let subtitle = match change.source() {
            Some(source) => source_text(source),
            None => "Structure; no source needed".to_string(),
        };

        let row = adw::ActionRow::builder()
            .title(change.describe_for(pref))
            .subtitle(&subtitle)
            .build();
        row.set_activatable(false);
        row.set_subtitle_lines(3);

        let check = gtk::CheckButton::new();
        check.set_active(true);
        check.set_valign(gtk::Align::Center);
        row.add_suffix(&check);

        group.add(&row);
        checks.push(check);
    }

    let preferences = adw::PreferencesPage::new();
    preferences.add(&group);

    let header = page_header("Review proposal");

    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    header.pack_end(&apply);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    cancel.connect_clicked({
        let nav = nav.clone();
        move |_| {
            nav.pop();
        }
    });

    apply.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let overlay = overlay.clone();
        let proposal = proposal.clone();
        let path = path.to_path_buf();

        move |_| {
            let changes: Vec<proposal::Change> = proposal
                .changes
                .iter()
                .enumerate()
                .filter(|(index, _)| checks[*index].is_active())
                .map(|(_, change)| change.clone())
                .collect();

            if changes.is_empty() {
                overlay.add_toast(adw::Toast::new("Nothing is selected"));
                return;
            }

            let selected = proposal::Proposal {
                profile_id,
                profile_name: proposal.profile_name.clone(),
                changes,
            };

            let changed = match proposal::check(&store, &selected) {
                Ok(changed) => changed,
                Err(message) => {
                    let toast = adw::Toast::new(&message);
                    toast.set_use_markup(false);
                    overlay.add_toast(toast);
                    return;
                }
            };

            if let Err(message) = proposal::apply(&store, &selected) {
                let toast = adw::Toast::new(&message);
                toast.set_use_markup(false);
                overlay.add_toast(toast);
                return;
            }

            // Keep the file when only part of it was accepted, so the rest can still be reviewed.
            if selected.changes.len() == proposal.changes.len()
                && let Err(error) = proposal::archive(&path)
            {
                eprintln!("could not archive {}: {error}", path.display());
            }

            let applied = format!(
                "Applied {} value{}",
                changed.len(),
                if changed.len() == 1 { "" } else { "s" }
            );

            match profile_view(&store, &nav, &root_page, profile_id, Some(&applied)) {
                Some(view) => {
                    nav.replace(&[root_page.clone(), view]);
                }
                None => {
                    nav.pop();
                }
            }
        }
    });

    adw::NavigationPage::new(&toolbar, "Review proposal")
}

fn source_text(source: &proposal::Source) -> String {
    let kind = match source.source_type.as_str() {
        "agent" => "Model knowledge".to_string(),
        other => humanize(other),
    };
    let reference = source.source_ref.as_deref().unwrap_or("no source given");
    let confidence = match (source.source_type.as_str(), source.confidence.as_deref()) {
        ("agent", _) => "low",
        (_, Some(confidence)) => confidence,
        (_, None) => "unknown",
    };

    format!("{kind} · {reference} · {confidence} confidence")
}
