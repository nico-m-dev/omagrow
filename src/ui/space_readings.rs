//! A space's own readings: the card in the rail, the page with all of them, and the form that
//! adds one. The weather model has its own card and never lands in these rows.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::climate::{self, ClimateInput, ClimateReading};
use crate::clock::today;
use crate::planting::Space;
use crate::store::Store;
use crate::units::UnitPreference;

use super::page_header;
use super::shared::fields::short_date;
use super::shared::fields::{date_row, entry_row, number_entry_row, parse_optional_number};
use super::shell::Guard;

/// How many readings the rail card shows. The page behind "All readings" has the rest.
const LATEST: usize = 3;

/// A page's re-render, handed to a row that removes itself.
type RenderSlot = Rc<RefCell<Option<Weak<dyn Fn()>>>>;

/// The rail card: what the last days did, the newest readings, and the way to the rest.
pub(super) fn card(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    space: &Space,
    units: UnitPreference,
) -> gtk::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Readings");
    group.set_description(Some("Measured in this space by hand."));

    let readings = store.climate_readings(space.id).unwrap_or_default();
    let summary = climate::summarize(&readings, today());

    group.add(&summary_row(&summary, units));
    for reading in readings.iter().take(LATEST) {
        group.add(&reading_row(reading, units));
    }

    let all = adw::ActionRow::builder().title("All readings").build();
    all.set_activatable(true);
    let arrow = gtk::Image::from_icon_name("go-next-symbolic");
    arrow.add_css_class("dim-label");
    all.add_suffix(&arrow);
    all.connect_activated({
        let store = store.clone();
        let nav = nav.clone();
        let space_id = space.id;
        move |_| {
            nav.push(&page(&store, &nav, space_id));
        }
    });
    group.add(&all);

    let add = gtk::Button::builder()
        .label("Add a reading")
        .halign(gtk::Align::Start)
        .build();
    add.add_css_class("suggested-action");
    add.add_css_class("pill");
    add.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let space_id = space.id;
        move |_| {
            nav.push(&form(&store, &nav, space_id));
        }
    });

    let section = gtk::Box::new(gtk::Orientation::Vertical, 12);
    section.append(&group);
    section.append(&add);
    section.upcast()
}

fn summary_row(summary: &climate::Summary, units: UnitPreference) -> adw::ActionRow {
    let subtitle = if summary.is_empty() {
        "No readings yet".to_string()
    } else if summary.readings == 0 {
        // Only readings older than the window: the line would be empty.
        format!("No readings in the last {} days", climate::SUMMARY_DAYS)
    } else {
        summary.line(units, &units.label("°C"), "% RH")
    };

    let row = adw::ActionRow::builder()
        .title(format!("Last {} days", climate::SUMMARY_DAYS))
        .subtitle(subtitle)
        .build();
    row.set_activatable(false);
    row.set_focusable(false);
    row
}

/// The page with every reading of the space, newest first, and a way to delete a wrong one.
fn page(store: &Rc<Store>, nav: &adw::NavigationView, space_id: i64) -> adw::NavigationPage {
    if store.space(space_id).ok().flatten().is_none() {
        return super::shared::form::missing_page(
            "Space",
            "Readings",
            "This space no longer exists",
            Some("It may have been removed together with its last planting."),
        );
    }

    let units = UnitPreference::load(store);

    let header = page_header("Readings");
    let add = gtk::Button::with_label("Add");
    add.add_css_class("suggested-action");
    header.pack_end(&add);

    let overlay = adw::ToastOverlay::new();

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_valign(gtk::Align::Start);
    content.set_margin_top(18);
    content.set_margin_bottom(32);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(760);
    clamp.set_tightening_threshold(900);
    clamp.set_child(Some(&content));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();

    let empty = adw::StatusPage::new();
    empty.add_css_class("omagrow-empty");
    empty.set_icon_name(Some("document-edit-symbolic"));
    empty.set_title("No readings yet");
    empty.set_description(Some(
        "A reading is a temperature or humidity you measured in this space by hand.",
    ));

    let empty_add = gtk::Button::with_label("Add the first reading");
    empty_add.add_css_class("suggested-action");
    empty_add.add_css_class("pill");
    empty_add.set_halign(gtk::Align::Center);
    empty.set_child(Some(&empty_add));

    let pages = gtk::Stack::new();
    pages.add_named(&empty, Some("empty"));
    pages.add_named(&scroller, Some("list"));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&pages));
    overlay.set_child(Some(&toolbar));

    let render_slot: RenderSlot = Rc::new(RefCell::new(None));

    let render: Rc<dyn Fn()> = Rc::new({
        let content = content.clone();
        let pages = pages.clone();
        let store = store.clone();
        let overlay = overlay.clone();
        let render_slot = render_slot.clone();
        move || {
            while let Some(child) = content.first_child() {
                content.remove(&child);
            }

            let readings = store.climate_readings(space_id).unwrap_or_default();
            if readings.is_empty() {
                pages.set_visible_child_name("empty");
                return;
            }

            let group = adw::PreferencesGroup::new();
            group.set_title("All readings");
            group.set_description(Some(
                "Newest first. A reading without a time sits at noon in the graphs.",
            ));

            for reading in &readings {
                group.add(&delete_row(&store, &overlay, &render_slot, reading, units));
            }

            content.append(&group);
            pages.set_visible_child_name("list");
        }
    });

    *render_slot.borrow_mut() = Some(Rc::downgrade(&render));

    for button in [&add, &empty_add] {
        button.connect_clicked({
            let store = store.clone();
            let nav = nav.clone();
            move |_| {
                nav.push(&form(&store, &nav, space_id));
            }
        });
    }

    let page = adw::NavigationPage::new(&overlay, "Readings");
    page.connect_showing({
        let render = render.clone();
        move |_| render()
    });

    render();
    page
}

fn reading_values(reading: &ClimateReading, units: UnitPreference) -> String {
    let mut parts = Vec::new();

    if let Some(temperature) = reading.temperature_c {
        parts.push(format!(
            "{} {}",
            units.number("°C", temperature),
            units.label("°C")
        ));
    }

    if let Some(humidity) = reading.humidity_pct {
        parts.push(format!(
            "{} % RH",
            crate::plant_data::format_number(humidity)
        ));
    }

    parts.join(" · ")
}

fn reading_when(reading: &ClimateReading) -> String {
    let mut parts = vec![short_date(reading.measured_on)];

    if let Some(time) = &reading.measured_time {
        parts.push(time.clone());
    }

    parts.push(climate::source_label(&reading.source).to_string());
    parts.join(" · ")
}

fn reading_row(reading: &ClimateReading, units: UnitPreference) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(reading_values(reading, units))
        .subtitle(reading_when(reading))
        .build();
    row.set_activatable(false);
    row.set_focusable(false);
    row
}

fn delete_row(
    store: &Rc<Store>,
    overlay: &adw::ToastOverlay,
    render_slot: &RenderSlot,
    reading: &ClimateReading,
    units: UnitPreference,
) -> adw::ActionRow {
    let row = reading_row(reading, units);

    let delete = gtk::Button::from_icon_name("user-trash-symbolic");
    delete.add_css_class("flat");
    delete.set_tooltip_text(Some("Delete reading"));
    delete.set_valign(gtk::Align::Center);

    let id = reading.id;
    delete.connect_clicked({
        let store = store.clone();
        let overlay = overlay.clone();
        let render_slot = render_slot.clone();
        move |button| {
            let store = store.clone();
            let overlay = overlay.clone();
            let render_slot = render_slot.clone();

            super::shared::form::confirm_delete(
                button,
                "Delete this reading?",
                "The reading is removed for good. The space and its plantings stay.",
                Rc::new(move || match store.delete_climate_reading(id) {
                    Ok(()) => {
                        if let Some(render) = render_slot.borrow().as_ref().and_then(Weak::upgrade)
                        {
                            render();
                        }
                    }
                    Err(error) => {
                        eprintln!("could not delete reading {id}: {error}");
                        overlay.add_toast(adw::Toast::new("Could not delete the reading"));
                    }
                }),
            );
        }
    });
    row.add_suffix(&delete);

    row
}

fn form(store: &Rc<Store>, nav: &adw::NavigationView, space_id: i64) -> adw::NavigationPage {
    let units = UnitPreference::load(store);
    let guard = Guard::new(nav);
    let header = guard.header("New reading");

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let group = adw::PreferencesGroup::new();
    group.set_title("Reading");
    group.set_description(Some(
        "At least a temperature or a humidity. The time is optional; leave it empty when you only \
         know the day.",
    ));

    let (date_row, date_value) = date_row("When", today());

    let time_row = entry_row("Time");
    time_row.set_tooltip_text(Some("Local time, for example 14:30"));

    let temperature_row = number_entry_row("Air temperature", None);
    let temperature_unit = gtk::Label::new(Some(&units.label("°C")));
    temperature_unit.add_css_class("dim-label");
    temperature_row.add_suffix(&temperature_unit);

    let humidity_row = number_entry_row("Relative humidity", None);
    let humidity_unit = gtk::Label::new(Some("%"));
    humidity_unit.add_css_class("dim-label");
    humidity_row.add_suffix(&humidity_unit);

    group.add(&date_row);
    group.add(&time_row);
    group.add(&temperature_row);
    group.add(&humidity_row);

    let preferences = adw::PreferencesPage::new();
    preferences.add(&group);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    save.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let time_row = time_row.clone();
        let temperature_row = temperature_row.clone();
        let humidity_row = humidity_row.clone();
        let date_value = date_value.clone();
        move |_| {
            let time = match crate::observation::parse_time(&time_row.text()) {
                Ok(time) => time,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };

            let temperature = match parse_optional_number(&temperature_row.text()) {
                Ok(value) => value.map(|value| units.input_to_canonical("°C", value)),
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };

            let humidity = match parse_optional_number(&humidity_row.text()) {
                Ok(value) => value,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };

            let input = ClimateInput {
                space_id,
                measured_on: date_value.get(),
                measured_time: time,
                temperature_c: temperature,
                humidity_pct: humidity,
                source: "manual".to_string(),
                source_ref: None,
            };

            match store.create_climate_reading(&input) {
                Ok(_) => {
                    nav.pop();
                }
                Err(error) => {
                    overlay.add_toast(adw::Toast::new(&error.to_string()));
                }
            }
        }
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    let page = adw::NavigationPage::new(&toolbar, "New reading");
    guard.attach(&page, &toolbar);
    page
}
