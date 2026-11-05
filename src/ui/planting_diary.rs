//! The planting diary: quick logging, the chronological list, photos and the actual stage starts.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::calendar::Date;
use crate::clock::today;
use crate::media;
use crate::observation::{self, Observation, ObservationInput};
use crate::plant_data::Stage;
use crate::planting::PlantingSummary;
use crate::schedule::Schedule;
use crate::store::Store;
use crate::units::UnitPreference;

use super::planting_schedule::CalendarEvent;
use super::shared::fields::short_date;
use super::shared::fields::{
    clean_text, combo_row, date_row, entry_row, number_entry_row, parse_optional_number,
};
use super::shell::Guard;
use super::{page_header, show_photo};

type RenderSlot = Rc<RefCell<Option<Weak<dyn Fn()>>>>;

pub(super) fn stage_names(stages: &[Stage]) -> HashMap<i64, String> {
    stages
        .iter()
        .map(|stage| (stage.id, stage.label.clone()))
        .collect()
}

pub(super) fn calendar_events(
    observations: &[Observation],
    stages: &[Stage],
    units: UnitPreference,
    schedule: &Schedule,
) -> Vec<CalendarEvent> {
    let names = stage_names(stages);
    let scheduled: Vec<(i64, Date)> = schedule
        .stages
        .iter()
        .filter(|window| window.actual_start)
        .map(|window| (window.stage.id, window.likely_start))
        .collect();

    observations
        .iter()
        .filter(|observation| {
            if observation.kind != "stage" {
                return true;
            }
            let Some(stage_id) = observation.stage_id else {
                return true;
            };
            !scheduled
                .iter()
                .any(|(id, date)| *id == stage_id && *date == observation.occurred_on)
        })
        .map(|observation| CalendarEvent {
            date: observation.occurred_on,
            end: None,
            kind: observation.kind.clone(),
            label: observation::label(
                observation,
                observation
                    .stage_id
                    .and_then(|id| names.get(&id))
                    .map(String::as_str),
                units,
            ),
        })
        .collect()
}

pub(super) fn card(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    summary: &PlantingSummary,
    observations: &[Observation],
    stages: &[Stage],
    units: UnitPreference,
    day: Date,
) -> gtk::Widget {
    let names = stage_names(stages);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("card");
    card.add_css_class("omagrow-diary-card");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("Diary"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("heading");

    let when = gtk::Label::new(Some(&if day == today() {
        "Today".to_string()
    } else {
        short_date(day)
    }));
    when.add_css_class("dim-label");
    when.add_css_class("omagrow-axis");

    let log = gtk::Button::with_label("Log");
    log.add_css_class("flat");
    log.set_tooltip_text(Some("Log what happened"));

    header.append(&title);
    header.append(&when);
    header.append(&log);
    card.append(&header);

    let entries: Vec<&Observation> = observations
        .iter()
        .filter(|observation| observation.occurred_on == day)
        .collect();

    if entries.is_empty() {
        let empty = gtk::Label::new(Some(&if observations.is_empty() {
            "No diary entries yet. Log the first watering, note or stage start.".to_string()
        } else if day == today() {
            "No diary entries today yet.".to_string()
        } else {
            format!("No diary entries for {}.", short_date(day))
        }));
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        empty.add_css_class("dim-label");
        card.append(&empty);
    } else {
        for observation in entries {
            card.append(&summary_row(observation, &names, units));
        }
    }

    let open = open_diary_row();
    card.append(&open);

    log.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let summary = summary.clone();
        move |_| {
            nav.push(&log_page(&store, &nav, &summary));
        }
    });

    open.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let id = summary.planting.id;
        move |_| {
            nav.push(&page(&store, &nav, id));
        }
    });

    card.upcast()
}

/// The way from the card to the whole diary: the same row-with-chevron shape as "All readings" on
/// a space, so the two cards read alike.
fn open_diary_row() -> gtk::Button {
    let row = gtk::Button::new();
    row.add_css_class("flat");
    row.set_halign(gtk::Align::Fill);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let text = gtk::Label::new(Some("Open diary"));
    text.set_xalign(0.0);
    text.set_hexpand(true);

    let chevron = gtk::Image::from_icon_name("go-next-symbolic");
    chevron.add_css_class("dim-label");

    content.append(&text);
    content.append(&chevron);
    row.set_child(Some(&content));

    row
}

fn summary_row(
    observation: &Observation,
    names: &HashMap<i64, String>,
    units: UnitPreference,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_valign(gtk::Align::Center);
    row.append(&kind_dot(&observation.kind));

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);

    let label = gtk::Label::new(Some(&observation::label(
        observation,
        observation
            .stage_id
            .and_then(|id| names.get(&id))
            .map(String::as_str),
        units,
    )));
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let when = gtk::Label::new(Some(&when_text(observation)));
    when.set_xalign(0.0);
    when.add_css_class("omagrow-axis");

    text.append(&label);
    text.append(&when);
    row.append(&text);

    row.upcast()
}

pub(super) fn page(store: &Rc<Store>, nav: &adw::NavigationView, id: i64) -> adw::NavigationPage {
    let Some(summary) = store.planting(id).ok().flatten() else {
        return missing_page();
    };
    let stages = store
        .stages(summary.planting.profile_id)
        .unwrap_or_default();
    let units = UnitPreference::load(store);

    let header = page_header("Diary");
    let log = gtk::Button::with_label("Log");
    log.add_css_class("suggested-action");
    header.pack_end(&log);

    let overlay = adw::ToastOverlay::new();

    let filter = gtk::DropDown::from_strings(observation::FILTERS);
    filter.set_selected(0);
    filter.set_margin_start(24);
    filter.set_margin_end(24);
    filter.set_margin_top(18);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
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
    empty.set_title("No diary entries yet");
    empty.set_description(Some(
        "Log a watering, feeding, note, photo or stage start and it appears here.",
    ));

    let empty_log = gtk::Button::with_label("Log the first entry");
    empty_log.add_css_class("suggested-action");
    empty_log.add_css_class("pill");
    empty_log.set_halign(gtk::Align::Center);
    empty.set_child(Some(&empty_log));

    let pages = gtk::Stack::new();
    pages.add_named(&empty, Some("empty"));
    pages.add_named(&scroller, Some("list"));

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&filter);
    body.append(&pages);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));
    overlay.set_child(Some(&toolbar));

    let render_slot: RenderSlot = Rc::new(RefCell::new(None));

    let render: Rc<dyn Fn()> = Rc::new({
        let content = content.clone();
        let pages = pages.clone();
        let empty = empty.clone();
        let empty_log = empty_log.clone();
        let filter = filter.clone();
        let store = store.clone();
        let overlay = overlay.clone();
        let render_slot = render_slot.clone();
        let stages = stages.clone();

        move || {
            while let Some(child) = content.first_child() {
                content.remove(&child);
            }

            let observations = store.observations(id).unwrap_or_default();
            let filter_index = filter.selected() as usize;
            let filter_name = observation::FILTERS
                .get(filter_index)
                .copied()
                .unwrap_or("All");

            let filtered: Vec<&Observation> = observations
                .iter()
                .filter(|entry| observation::matches_filter(&entry.kind, filter_name))
                .collect();

            if filtered.is_empty() {
                if observations.is_empty() {
                    empty.set_title("No diary entries yet");
                    empty.set_description(Some(
                        "Log a watering, feeding, note, photo or stage start and it appears here.",
                    ));
                    empty_log.set_label("Log the first entry");
                } else {
                    empty.set_title("No entries in this filter");
                    empty.set_description(Some("Choose All to see the whole diary."));
                    empty_log.set_label("Log an entry");
                }
                pages.set_visible_child_name("empty");
                return;
            }

            let names = stage_names(&stages);
            let today = today();
            let mut last_date: Option<Date> = None;

            for entry in filtered {
                if last_date != Some(entry.occurred_on) {
                    last_date = Some(entry.occurred_on);
                    content.append(&day_header(entry.occurred_on, today));
                }

                content.append(&history_row(
                    &store,
                    &overlay,
                    &render_slot,
                    entry,
                    &names,
                    units,
                ));
            }

            pages.set_visible_child_name("list");
        }
    });

    *render_slot.borrow_mut() = Some(Rc::downgrade(&render));

    log.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let summary = summary.clone();
        move |_| {
            nav.push(&log_page(&store, &nav, &summary));
        }
    });

    empty_log.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let summary = summary.clone();
        move |_| {
            nav.push(&log_page(&store, &nav, &summary));
        }
    });

    filter.connect_selected_notify({
        let render_slot = render_slot.clone();
        move |_| {
            if let Some(render) = render_slot.borrow().as_ref().and_then(Weak::upgrade) {
                render();
            }
        }
    });

    let page = adw::NavigationPage::new(&overlay, "Diary");
    page.connect_showing({
        let render = render.clone();
        move |_| render()
    });

    render();
    page
}

fn day_header(date: Date, today: Date) -> gtk::Widget {
    let label = match today.days_until(date) {
        0 => "Today".to_string(),
        -1 => "Yesterday".to_string(),
        _ => short_date(date),
    };

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_top(8);

    let label = gtk::Label::new(Some(&label));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.add_css_class("omagrow-section-title");

    row.append(&label);
    row.upcast()
}

fn history_row(
    store: &Rc<Store>,
    overlay: &adw::ToastOverlay,
    render_slot: &RenderSlot,
    observation: &Observation,
    names: &HashMap<i64, String>,
    units: UnitPreference,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("omagrow-diary-row");
    row.append(&kind_dot(&observation.kind));

    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some(&observation::label(
        observation,
        observation
            .stage_id
            .and_then(|id| names.get(&id))
            .map(String::as_str),
        units,
    )));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_wrap(true);

    let when = gtk::Label::new(Some(&time_text(observation)));
    when.set_halign(gtk::Align::End);
    when.add_css_class("omagrow-axis");

    top.append(&label);
    top.append(&when);

    let delete = gtk::Button::from_icon_name("user-trash-symbolic");
    delete.add_css_class("flat");
    delete.set_tooltip_text(Some("Delete entry"));
    delete.set_valign(gtk::Align::Start);
    delete.connect_clicked({
        let store = store.clone();
        let overlay = overlay.clone();
        let render_slot = render_slot.clone();
        let observation = observation.clone();
        move |button| {
            let store = store.clone();
            let overlay = overlay.clone();
            let render_slot = render_slot.clone();
            let observation = observation.clone();

            super::shared::form::confirm_delete(
                button,
                "Delete this diary entry?",
                if observation.kind == "repot" {
                    "The entry and its photo are removed. The planting keeps its current pot size."
                } else {
                    "The entry and its photo are removed. This cannot be undone."
                },
                Rc::new(move || match store.delete_observation(observation.id) {
                    Ok(stems) => {
                        for stem in stems {
                            media::remove(&stem);
                        }
                        let render = render_slot.borrow().as_ref().and_then(Weak::upgrade);
                        if let Some(render) = render {
                            render();
                        }
                    }
                    Err(error) => {
                        eprintln!("could not delete diary entry {}: {error}", observation.id);
                        overlay.add_toast(adw::Toast::new("Could not delete the entry"));
                    }
                }),
            );
        }
    });
    top.append(&delete);
    text.append(&top);

    if let Some(note) = observation
        .note
        .as_deref()
        .map(str::trim)
        .filter(|note| !note.is_empty())
    {
        let note = gtk::Label::new(Some(note));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("dim-label");
        text.append(&note);
    }

    row.append(&text);

    if let Some(photo) = store
        .media("observation", observation.id, "photo")
        .ok()
        .flatten()
    {
        let thumb = media::thumb_path(&photo.file_stem).filter(|path| path.exists());
        let large = media::large_path(&photo.file_stem).filter(|path| path.exists());
        let display = thumb.or_else(|| large.clone());

        if let Some(display) = display {
            // The card shows the small copy; the viewer opens the large one. Falling back to the
            // other file keeps an old photo visible after a partial import.
            let detail = media::detail_path(&photo.file_stem).unwrap_or_else(|| display.clone());
            let picture = gtk::Picture::for_filename(&display);
            picture.set_content_fit(gtk::ContentFit::Cover);
            picture.set_size_request(64, 64);
            let thumb_button = gtk::Button::new();
            thumb_button.add_css_class("flat");
            thumb_button.add_css_class("omagrow-thumb");
            thumb_button.set_child(Some(&picture));
            thumb_button.set_tooltip_text(Some("Show the photo"));
            thumb_button.connect_clicked(move |button| {
                if let Some(window) = button
                    .root()
                    .and_then(|root| root.downcast::<gtk::Window>().ok())
                {
                    show_photo(&window, &detail);
                }
            });
            row.append(&thumb_button);
        }
    }

    row.upcast()
}

/// The kinds the log form offers. A stage start is logged from the grow timeline — the diary keeps
/// the entry it creates — and a repot only exists in a pot.
fn log_kinds(container: &str) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = observation::KINDS
        .iter()
        .copied()
        .filter(|kind| *kind != "stage")
        .filter(|kind| *kind != "repot" || container == "pot")
        .collect();

    if kinds.is_empty() {
        kinds.push("note");
    }

    kinds
}

fn log_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    summary: &PlantingSummary,
) -> adw::NavigationPage {
    let Some(current) = store.planting(summary.planting.id).ok().flatten() else {
        return missing_page();
    };
    let planting = &current.planting;
    let units = UnitPreference::load(store);

    let guard = Guard::new(nav);
    let header = guard.header("Log entry");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let kinds = log_kinds(&planting.container);

    let kind_labels: Vec<&str> = kinds
        .iter()
        .map(|kind| observation::kind_label(kind))
        .collect();
    let kind_row = combo_row("What happened", &kind_labels, 0);

    let (date_row, date_value) = date_row("When", today());

    let time_row = entry_row("Time");
    time_row.set_tooltip_text(Some("Optional, for example 14:30"));

    let event_group = adw::PreferencesGroup::new();
    event_group.set_title("Entry");
    event_group.add(&kind_row);
    event_group.add(&date_row);
    event_group.add(&time_row);

    let amount_row = number_entry_row("Amount", None);
    let amount_unit = gtk::Label::new(None);
    amount_unit.add_css_class("dim-label");
    amount_row.add_suffix(&amount_unit);
    amount_row.set_visible(false);

    let details_group = adw::PreferencesGroup::new();
    details_group.set_title("Details");
    details_group.add(&amount_row);
    details_group.set_visible(false);

    let note_view = gtk::TextView::new();
    note_view.set_wrap_mode(gtk::WrapMode::WordChar);
    note_view.set_accepts_tab(false);
    note_view.set_top_margin(8);
    note_view.set_bottom_margin(8);
    note_view.set_left_margin(10);
    note_view.set_right_margin(10);

    let note_scroller = gtk::ScrolledWindow::builder()
        .height_request(96)
        .child(&note_view)
        .build();
    let note_frame = gtk::Frame::new(None);
    note_frame.set_child(Some(&note_scroller));
    note_frame.add_css_class("card");

    let note_group = adw::PreferencesGroup::new();
    note_group.set_title("Note");
    note_group.add(&note_frame);

    let photo_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    let add_photo = gtk::Button::with_label("Choose a photo");
    add_photo.add_css_class("flat");

    let preview = gtk::Picture::new();
    preview.set_content_fit(gtk::ContentFit::Cover);
    preview.set_size_request(96, 96);

    let preview_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    preview_box.set_size_request(96, 96);
    preview_box.set_overflow(gtk::Overflow::Hidden);
    preview_box.add_css_class("omagrow-thumb");
    preview_box.append(&preview);
    preview_box.set_visible(false);

    let remove_photo = gtk::Button::with_label("Remove photo");
    remove_photo.add_css_class("flat");
    remove_photo.set_visible(false);

    let photo_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    photo_box.set_margin_top(8);
    photo_box.set_margin_bottom(8);
    photo_box.set_margin_start(12);
    photo_box.set_margin_end(12);
    photo_box.append(&add_photo);
    photo_box.append(&preview_box);
    photo_box.append(&remove_photo);

    let photo_group = adw::PreferencesGroup::new();
    photo_group.set_title("Photo");
    photo_group.add(&photo_box);

    let details = DetailRows {
        group: details_group.clone(),
        amount_row: amount_row.clone(),
        amount_unit: amount_unit.clone(),
        unit: Cell::new(None),
        current_volume: planting.container_volume_l,
    };

    update_details(&kind_row, &kinds, &details, units);

    kind_row.connect_selected_notify({
        let kinds = kinds.clone();
        let details = details.clone();
        move |row| {
            update_details(row, &kinds, &details, units);
        }
    });

    add_photo.connect_clicked({
        let photo_path = photo_path.clone();
        let preview_box = preview_box.clone();
        let preview = preview.clone();
        let remove_photo = remove_photo.clone();
        let add_photo = add_photo.clone();
        let guard = guard.clone();
        move |button| {
            let Some(window) = button
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            else {
                return;
            };

            let dialog = gtk::FileDialog::builder().title("Choose a photo").build();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Images"));
            for mime in ["image/jpeg", "image/png", "image/webp"] {
                filter.add_mime_type(mime);
            }
            let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));
            dialog.set_default_filter(Some(&filter));

            let photo_path = photo_path.clone();
            let preview_box = preview_box.clone();
            let preview = preview.clone();
            let remove_photo = remove_photo.clone();
            let add_photo = add_photo.clone();
            let guard = guard.clone();

            dialog.open(Some(&window), gtk::gio::Cancellable::NONE, move |result| {
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    return;
                };

                preview.set_filename(Some(&path));
                *photo_path.borrow_mut() = Some(path);
                preview_box.set_visible(true);
                remove_photo.set_visible(true);
                add_photo.set_label("Change photo");
                guard.mark();
            });
        }
    });

    remove_photo.connect_clicked({
        let photo_path = photo_path.clone();
        let preview_box = preview_box.clone();
        let remove_photo = remove_photo.clone();
        let add_photo = add_photo.clone();
        let guard = guard.clone();
        move |_| {
            *photo_path.borrow_mut() = None;
            preview_box.set_visible(false);
            remove_photo.set_visible(false);
            add_photo.set_label("Choose a photo");
            guard.mark();
        }
    });

    let preferences = adw::PreferencesPage::new();
    preferences.add(&event_group);
    preferences.add(&details_group);
    preferences.add(&note_group);
    preferences.add(&photo_group);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    save.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let kinds = kinds.clone();
        let kind_row = kind_row.clone();
        let planting_id = planting.id;
        let amount_row = amount_row.clone();
        let date_value = date_value.clone();
        let time_row = time_row.clone();
        let note_view = note_view.clone();
        let photo_path = photo_path.clone();

        move |_| {
            let kind = kinds
                .get(kind_row.selected() as usize)
                .copied()
                .unwrap_or("note");

            let time = match observation::parse_time(&time_row.text()) {
                Ok(time) => time,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };

            let amount = match parse_optional_number(&amount_row.text()) {
                Ok(value) => value,
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };
            let unit = observation::unit_for_kind(kind);
            let amount = amount.map(|value| units.input_to_canonical(unit.unwrap_or("L"), value));

            let buffer = note_view.buffer();
            let note = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            let note = clean_text(&note);

            if kind == "photo" && photo_path.borrow().is_none() {
                overlay.add_toast(adw::Toast::new("Choose a photo first."));
                return;
            }

            let input = ObservationInput {
                planting_id,
                kind: kind.to_string(),
                occurred_on: date_value.get(),
                occurred_time: time,
                note,
                amount,
                unit: unit.map(str::to_string),
                stage_id: None,
            };

            if let Err(error) = input.validate() {
                overlay.add_toast(adw::Toast::new(&error.to_string()));
                return;
            }

            match store.create_observation(&input) {
                Ok(observation_id) => {
                    if let Some(path) = photo_path.borrow().clone() {
                        let stem = media::stem_for("observation", observation_id, "photo");
                        let failure = match media::import(&path, &stem) {
                            Ok(imported) => {
                                let media = media::Media {
                                    id: 0,
                                    owner_type: "observation".to_string(),
                                    owner_id: observation_id,
                                    role: "photo".to_string(),
                                    file_stem: stem.clone(),
                                    width: imported.width,
                                    height: imported.height,
                                    bytes: imported.bytes,
                                };
                                match store.set_media(&media) {
                                    Ok(_) => None,
                                    Err(error) => {
                                        media::remove(&stem);
                                        Some(error.to_string())
                                    }
                                }
                            }
                            Err(error) => Some(error),
                        };

                        if let Some(message) = failure {
                            eprintln!("could not add the diary photo: {message}");
                            if let Err(error) = store.discard_observation(observation_id) {
                                eprintln!(
                                    "could not discard the diary entry after the photo failed: {error}"
                                );
                            }
                            overlay.add_toast(adw::Toast::new(
                                "Could not add the photo; the entry was not saved.",
                            ));
                            return;
                        }
                    }

                    nav.pop();
                }
                Err(error) => {
                    eprintln!("could not save the diary entry: {error}");
                    overlay.add_toast(adw::Toast::new(&error.to_string()));
                }
            }
        }
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    let page = adw::NavigationPage::new(&toolbar, "Log entry");
    guard.attach(&page, &toolbar);
    page
}

#[derive(Clone)]
struct DetailRows {
    group: adw::PreferencesGroup,
    amount_row: adw::EntryRow,
    amount_unit: gtk::Label,
    unit: Cell<Option<&'static str>>,
    current_volume: Option<f64>,
}

fn update_details(
    kind_row: &adw::ComboRow,
    kinds: &[&str],
    details: &DetailRows,
    units: UnitPreference,
) {
    let kind = kinds
        .get(kind_row.selected() as usize)
        .copied()
        .unwrap_or("note");

    let unit = observation::unit_for_kind(kind);
    let previous_unit = details.unit.replace(unit);
    if previous_unit != unit {
        details.amount_row.set_text("");
    }

    details.amount_row.set_visible(unit.is_some());
    if let Some(unit) = unit {
        details.amount_row.set_title(match kind {
            "repot" => "New pot volume",
            "harvest" => "Yield",
            _ => "Amount",
        });
        details.amount_unit.set_text(&units.label(unit));
    }

    if kind == "repot" {
        let hint = match details.current_volume {
            Some(volume) => format!(
                "Current pot: {} {}. Leave empty if you do not know the new size.",
                units.number("L", volume),
                units.label("L")
            ),
            None => "Leave empty if you do not know the new size.".to_string(),
        };
        details.amount_row.set_tooltip_text(Some(&hint));
    } else {
        details.amount_row.set_tooltip_text(None);
    }

    details.group.set_visible(unit.is_some());
}

fn kind_dot(kind: &str) -> gtk::Widget {
    let dot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dot.set_size_request(8, 8);
    dot.set_valign(gtk::Align::Start);
    dot.add_css_class("omagrow-diary-dot");
    dot.add_css_class(kind_class(kind));
    dot.upcast()
}

pub(super) fn kind_class(kind: &str) -> &'static str {
    match kind {
        "water" | "feed" | "prune" | "repot" | "support" => "care",
        "pest" | "disease" => "problem",
        "note" | "photo" | "other" => "note",
        "harvest" => "harvest",
        "stage" => "stage",
        _ => "note",
    }
}

fn when_text(observation: &Observation) -> String {
    let mut text = short_date(observation.occurred_on);
    if let Some(time) = &observation.occurred_time {
        text.push_str(" · ");
        text.push_str(time);
    }
    text
}

fn time_text(observation: &Observation) -> String {
    observation.occurred_time.clone().unwrap_or_default()
}

fn missing_page() -> adw::NavigationPage {
    super::shared::form::missing_page("Diary", "Diary", "This planting no longer exists", None)
}

#[cfg(test)]
mod tests {
    use super::log_kinds;

    #[test]
    fn the_log_form_leaves_a_stage_start_to_the_timeline() {
        let kinds = log_kinds("ground");
        assert!(!kinds.contains(&"stage"));
        assert!(kinds.contains(&"water"));
        // A repot is only offered in a pot.
        assert!(!kinds.contains(&"repot"));
        assert!(log_kinds("pot").contains(&"repot"));
    }
}
