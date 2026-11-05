//! Plantings: the list and the picker. The setup form is in `planting_setup`, the page in
//! `planting_view`.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::pango;
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::calendar::Date;
use crate::clock::today;
use crate::planting::{PlantingState, PlantingSummary};
use crate::store::Store;

use super::page_header;
use super::planting_setup::setup_page;
use super::planting_view::planting_view;
use super::shared::avatar;
use super::shared::fields::{
    outcome_label, profile_avatar, profile_subtitle, short_date, species_subtitle,
};
use super::shared::layout::{Columns, Slot};

/// The current selection handler. The page installs it on every body rebuild; the calendar, the
/// banner and the back-to-today button all call it.
pub(super) type SelectSlot = Rc<RefCell<Option<Rc<dyn Fn(Date)>>>>;

pub(super) fn section(store: &Rc<Store>, open_profiles: Rc<dyn Fn()>) -> super::shell::Section {
    let nav = adw::NavigationView::new();
    nav.set_pop_on_escape(true);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_valign(gtk::Align::Start);
    content.set_margin_top(24);
    content.set_margin_bottom(32);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(780);
    clamp.set_tightening_threshold(900);
    let columns = Columns::widening(&clamp, 780, 1000);
    columns.push(&content, Slot::Main);
    columns.finish(super::shared::layout::window_width(&nav));
    clamp.set_child(Some(columns.widget()));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();

    let status = adw::StatusPage::new();
    status.add_css_class("omagrow-empty");
    status.set_icon_name(Some("omagrow-planting-symbolic"));
    status.set_title("No plantings yet");
    status.set_description(Some(
        "A planting is one plant or a group of the same plant in one place. Start when you sow, \
         plant or pot something.",
    ));

    let empty_create = gtk::Button::with_label("New planting");
    empty_create.add_css_class("suggested-action");
    empty_create.add_css_class("pill");
    empty_create.set_halign(gtk::Align::Center);
    status.set_child(Some(&empty_create));

    let pages = gtk::Stack::new();
    pages.add_named(&status, Some("empty"));
    pages.add_named(&scroller, Some("list"));

    let header = page_header("Plantings");
    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.add_css_class("flat");
    add.set_tooltip_text(Some("New planting"));
    header.pack_end(&add);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&pages));

    let root_page = adw::NavigationPage::new(&toolbar, "Plantings");
    nav.push(&root_page);

    let open_view: Rc<dyn Fn(i64)> = {
        let nav = nav.clone();
        let store = store.clone();
        let root_page = root_page.clone();
        Rc::new(move |id| {
            if let Some(page) = planting_view(&store, &nav, &root_page, id) {
                nav.push(&page);
            }
        })
    };

    let open_create: Rc<dyn Fn()> = {
        let nav = nav.clone();
        let store = store.clone();
        let root_page = root_page.clone();
        let open_profiles = open_profiles.clone();
        Rc::new(move || {
            nav.push(&picker_page(
                &store,
                &nav,
                &root_page,
                open_profiles.clone(),
            ));
        })
    };

    let refresh: Rc<dyn Fn()> = {
        let content = content.clone();
        let pages = pages.clone();
        let status = status.clone();
        let store = store.clone();
        let open_view = open_view.clone();
        // Keep the columns (and their width watcher) alive with the list.
        let columns = columns;
        Rc::new(move || {
            let _keep_columns = &columns;
            let today = today();

            match store.plantings() {
                Ok(rows) if rows.is_empty() => {
                    status.set_title("No plantings yet");
                    status.set_description(Some(
                        "A planting is one plant or a group of the same plant in one place. Start \
                         when you sow, plant or pot something.",
                    ));
                    pages.set_visible_child_name("empty");
                }
                Ok(rows) => {
                    rebuild_list(&content, &rows, &store, &open_view, today);
                    pages.set_visible_child_name("list");
                }
                Err(error) => {
                    eprintln!("could not load plantings: {error}");
                    status.set_title("Could not load plantings");
                    status.set_description(Some(&error.to_string()));
                    pages.set_visible_child_name("empty");
                }
            }
        })
    };

    add.connect_clicked({
        let open_create = open_create.clone();
        move |_| open_create()
    });
    empty_create.connect_clicked({
        let open_create = open_create.clone();
        move |_| open_create()
    });
    nav.connect_popped({
        let refresh = refresh.clone();
        move |_, _| refresh()
    });

    refresh();

    super::shell::Section::new(nav, root_page, refresh)
}

fn rebuild_list(
    content: &gtk::Box,
    rows: &[PlantingSummary],
    store: &Store,
    open_view: &Rc<dyn Fn(i64)>,
    today: Date,
) {
    while let Some(child) = content.first_child() {
        content.remove(&child);
    }

    let mut active: Vec<&PlantingSummary> = rows
        .iter()
        .filter(|row| row.planting.state(today) != PlantingState::Finished)
        .collect();
    let mut finished: Vec<&PlantingSummary> = rows
        .iter()
        .filter(|row| row.planting.state(today) == PlantingState::Finished)
        .collect();

    active.sort_by_key(|a| a.title().to_lowercase());
    finished.sort_by_key(|a| std::cmp::Reverse(a.planting.ended_on));

    if !active.is_empty() {
        content.append(&section_header("Active", active.len()));
        for row in &active {
            content.append(&list_row(store, row, open_view.clone(), today, true));
        }
    }

    if !finished.is_empty() {
        content.append(&section_header("Finished", finished.len()));
        for row in &finished {
            content.append(&list_row(store, row, open_view.clone(), today, true));
        }
    }
}

fn section_header(title: &str, count: usize) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.add_css_class("omagrow-section-title");

    let count = gtk::Label::new(Some(&count.to_string()));
    count.add_css_class("omagrow-axis");

    row.append(&label);
    row.append(&count);
    row.upcast()
}

pub(super) fn list_row(
    store: &Store,
    summary: &PlantingSummary,
    open: Rc<dyn Fn(i64)>,
    today: Date,
    show_space: bool,
) -> gtk::Button {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    line.set_margin_top(12);
    line.set_margin_bottom(12);
    line.set_margin_start(14);
    line.set_margin_end(14);

    line.append(&avatar::widget(store, "planting", summary.planting.id, 56));

    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);

    let title = gtk::Label::new(Some(summary.title()));
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.add_css_class("heading");

    let subtitle = gtk::Label::new(Some(&planting_subtitle(summary, show_space)));
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(pango::EllipsizeMode::End);
    subtitle.add_css_class("dim-label");

    text.append(&title);
    text.append(&subtitle);
    line.append(&text);

    let status = gtk::Box::new(gtk::Orientation::Vertical, 4);
    status.set_valign(gtk::Align::Center);

    let chip = state_chip(summary, today);
    chip.set_halign(gtk::Align::End);

    let detail = gtk::Label::new(Some(&status_detail(summary, today)));
    detail.set_halign(gtk::Align::End);
    detail.add_css_class("omagrow-axis");

    status.append(&chip);
    status.append(&detail);
    line.append(&status);

    let button = gtk::Button::new();
    button.add_css_class("card");
    button.set_hexpand(true);
    button.set_child(Some(&line));

    let id = summary.planting.id;
    button.connect_clicked(move |_| open(id));

    button
}

fn planting_subtitle(summary: &PlantingSummary, show_space: bool) -> String {
    let mut parts = Vec::new();

    if show_space
        && let Some(space) = summary
            .space_name
            .as_deref()
            .map(str::trim)
            .filter(|space| !space.is_empty())
    {
        parts.push(space.to_string());
    }

    parts.push(quantity_text(summary.planting.quantity));

    if let Some(botanical) = species_subtitle(
        summary.profile_species.as_deref(),
        summary.profile_cultivar.as_deref(),
    ) {
        parts.push(botanical);
    }

    parts.join(" · ")
}

pub(super) fn quantity_text(quantity: i64) -> String {
    if quantity == 1 {
        "1 plant".to_string()
    } else {
        format!("{quantity} plants")
    }
}

pub(super) fn state_chip(summary: &PlantingSummary, today: Date) -> gtk::Label {
    let (text, class) = match summary.planting.state(today) {
        PlantingState::Growing => ("Growing", "growing"),
        PlantingState::Planned => ("Planned", "planned"),
        PlantingState::Finished => (
            summary
                .planting
                .outcome
                .as_deref()
                .map(outcome_label)
                .unwrap_or("Finished"),
            "finished",
        ),
    };

    let chip = gtk::Label::new(Some(text));
    chip.add_css_class("omagrow-chip");
    chip.add_css_class(class);
    chip
}

pub(super) fn status_detail(summary: &PlantingSummary, today: Date) -> String {
    match summary.planting.state(today) {
        PlantingState::Growing => format!("Day {}", summary.planting.day_number(today)),
        PlantingState::Planned => {
            let days = today.days_until(summary.planting.started_on);
            match days {
                1 => "Starts tomorrow".to_string(),
                days => format!("Starts in {days} days"),
            }
        }
        PlantingState::Finished => summary
            .planting
            .ended_on
            .map(short_date)
            .unwrap_or_default(),
    }
}

fn picker_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    open_profiles: Rc<dyn Fn()>,
) -> adw::NavigationPage {
    let header = page_header("Choose a plant");
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    cancel.connect_clicked({
        let nav = nav.clone();
        move |_| {
            nav.pop();
        }
    });

    let profiles = match store.profiles() {
        Ok(profiles) => profiles,
        Err(error) => {
            let status = adw::StatusPage::builder()
                .icon_name("dialog-error-symbolic")
                .title("Could not load plant profiles")
                .description(error.to_string())
                .build();
            toolbar.set_content(Some(&status));
            return adw::NavigationPage::new(&toolbar, "Choose a plant");
        }
    };

    if profiles.is_empty() {
        let status = adw::StatusPage::builder()
            .icon_name("omagrow-plant-symbolic")
            .title("No plant profiles yet")
            .description(
                "A planting starts from a plant profile. Create one first, then come back here.",
            )
            .build();

        let create = gtk::Button::with_label("Go to plant profiles");
        create.add_css_class("suggested-action");
        create.add_css_class("pill");
        create.set_halign(gtk::Align::Center);
        status.set_child(Some(&create));

        create.connect_clicked({
            let nav = nav.clone();
            let open_profiles = open_profiles.clone();
            move |_| {
                nav.pop();
                open_profiles();
            }
        });

        toolbar.set_content(Some(&status));
        return adw::NavigationPage::new(&toolbar, "Choose a plant");
    }

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search plants"));
    search.set_margin_bottom(6);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");

    let ids: Rc<Vec<i64>> = Rc::new(profiles.iter().map(|profile| profile.id).collect());
    let searchable: Rc<Vec<String>> = Rc::new(
        profiles
            .iter()
            .map(|profile| {
                [
                    profile.name.clone(),
                    profile.species.clone().unwrap_or_default(),
                    profile.cultivar.clone().unwrap_or_default(),
                ]
                .join(" ")
                .to_lowercase()
            })
            .collect(),
    );

    for profile in &profiles {
        let row = adw::ActionRow::builder()
            .title(profile.name.clone())
            .activatable(true)
            .build();

        if let Some(subtitle) = profile_subtitle(profile) {
            row.set_subtitle(&subtitle);
        }
        row.add_prefix(&profile_avatar(store, profile.id, 40));
        list.append(&row);
    }

    list.set_filter_func({
        let search = search.clone();
        let searchable = searchable.clone();
        move |row| {
            let needle = search.text().trim().to_lowercase();
            if needle.is_empty() {
                return true;
            }

            searchable
                .get(row.index() as usize)
                .map(|text| text.contains(&needle))
                .unwrap_or(true)
        }
    });

    search.connect_search_changed({
        let list = list.clone();
        move |_| list.invalidate_filter()
    });

    list.connect_row_activated({
        let ids = ids.clone();
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        move |_, row| {
            let Some(profile_id) = ids.get(row.index() as usize).copied() else {
                return;
            };
            nav.push(&setup_page(&store, &nav, &root_page, profile_id, None));
        }
    });

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_valign(gtk::Align::Start);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&search);
    content.append(&list);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(720);
    clamp.set_tightening_threshold(900);
    clamp.set_child(Some(&content));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();

    toolbar.set_content(Some(&scroller));
    adw::NavigationPage::new(&toolbar, "Choose a plant")
}

#[cfg(test)]
mod tests {
    use super::short_date;
    use crate::calendar::Date;
    use crate::ui::shared::fields::parse_optional_number;

    #[test]
    fn formats_a_short_date() {
        let today = crate::clock::today();
        let date = Date::new(today.year, 10, 30).unwrap();
        assert_eq!(short_date(date), "30 Oct");
    }

    #[test]
    fn reads_numbers_with_a_comma() {
        assert_eq!(parse_optional_number("3").unwrap(), Some(3.0));
        assert_eq!(parse_optional_number("3,5").unwrap(), Some(3.5));
        assert_eq!(parse_optional_number("  ").unwrap(), None);
        assert!(parse_optional_number("three").is_err());
    }
}
