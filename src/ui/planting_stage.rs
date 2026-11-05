//! Logging the start of a stage, opened from the grow timeline.
//!
//! A stage start is a correction to the plan, not a diary note: it moves that stage's block and
//! ends the previous stage that day. The entry itself stays a diary entry, so it still shows
//! in the log and can be removed there.

use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::calendar::Date;
use crate::observation::ObservationInput;
use crate::plant_data::Stage;
use crate::store::Store;

use super::shared::fields::{clean_text, combo_row, date_row};
use super::shell::Guard;

pub(super) fn page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    planting_id: i64,
    stages: &[Stage],
    default_stage: i64,
    default_day: Date,
) -> adw::NavigationPage {
    let guard = Guard::new(nav);
    let header = guard.header("Stage started");

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let stage_group = adw::PreferencesGroup::new();
    stage_group.set_title("Stage");
    stage_group.set_description(Some(
        "This moves that stage's block; the stage before it ends that day. A stage start belongs to the \
         cycle it is nearest to, so this year's start leaves last year's cycle alone.",
    ));

    let labels: Vec<&str> = stages.iter().map(|stage| stage.label.as_str()).collect();
    let selected = stages
        .iter()
        .position(|stage| stage.id == default_stage)
        .unwrap_or(0) as u32;
    let stage_row = combo_row("Stage that started", &labels, selected);
    stage_group.add(&stage_row);

    let (date_row, date_value) = date_row("When", default_day);
    stage_group.add(&date_row);

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
    note_group.set_description(Some(
        "Optional. For example what you saw when the stage changed.",
    ));
    note_group.add(&note_frame);

    let preferences = adw::PreferencesPage::new();
    preferences.add(&stage_group);
    preferences.add(&note_group);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    save.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let stages = stages.to_vec();
        let stage_row = stage_row.clone();
        let date_value = date_value.clone();
        let note_view = note_view.clone();
        move |_| {
            let Some(stage) = stages.get(stage_row.selected() as usize) else {
                overlay.add_toast(adw::Toast::new("Choose the stage that started"));
                return;
            };

            let buffer = note_view.buffer();
            let note = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);

            let input = ObservationInput {
                planting_id,
                kind: "stage".to_string(),
                occurred_on: date_value.get(),
                occurred_time: None,
                note: clean_text(&note),
                amount: None,
                unit: None,
                stage_id: Some(stage.id),
            };

            if let Err(error) = input.validate() {
                overlay.add_toast(adw::Toast::new(&error.to_string()));
                return;
            }

            match store.create_observation(&input) {
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

    let page = adw::NavigationPage::new(&toolbar, "Stage started");
    guard.attach(&page, &toolbar);
    page
}
