//! The planting setup form and the shared space editor.

use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::clock::today;
use crate::plant_data::{ValidationError, format_number};
use crate::planting::{
    CONTAINERS, DEFAULT_CONTAINER, DEFAULT_SPACE_KIND, DEFAULT_START_KIND, PlantingInput,
    SPACE_KINDS, START_KINDS, Space, SpaceInput,
};
use crate::store::Store;
use crate::units::UnitPreference;

use super::planting_view::replace_with_view;
use super::shared::fields::{
    choice_index, clean_text, combo_row, date_row, entry_row, light_start_text, light_type_index,
    light_type_value, number_entry_row, parse_light_start, parse_optional_number, profile_avatar,
    profile_subtitle, update_light_rows, update_volume_row,
};
use super::shell::Guard;

#[derive(Clone)]
enum SpaceChoice {
    Private,
    New,
    Existing(Space),
}

#[derive(Clone, Copy)]
enum SaveOutcome {
    Created(i64),
    Updated,
}

pub(super) fn setup_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    profile_id: i64,
    planting_id: Option<i64>,
) -> adw::NavigationPage {
    let Some(profile) = store.profile(profile_id).ok().flatten() else {
        return missing_profile_page(profile_id);
    };

    let existing = planting_id.and_then(|id| store.planting(id).ok().flatten());
    let existing_planting = existing.as_ref().map(|summary| &summary.planting);
    let existing_space =
        existing_planting.and_then(|planting| store.space(planting.space_id).ok().flatten());
    let spaces = store.spaces().unwrap_or_default();
    let today = today();

    let guard = Guard::new(nav);
    let header = guard.header(if planting_id.is_some() {
        "Edit planting"
    } else {
        "New planting"
    });

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let plant_group = adw::PreferencesGroup::new();
    plant_group.set_title("Plant");
    let plant_row = adw::ActionRow::builder()
        .title(profile.name.clone())
        .build();
    if let Some(subtitle) = profile_subtitle(&profile) {
        plant_row.set_subtitle(&subtitle);
    }
    plant_row.add_prefix(&profile_avatar(store, profile.id, 40));
    if planting_id.is_none() {
        let change = gtk::Button::with_label("Change");
        change.add_css_class("flat");
        change.connect_clicked({
            let guard = guard.clone();
            move |_| guard.close()
        });
        plant_row.add_suffix(&change);
    }
    plant_group.add(&plant_row);

    let setup_group = adw::PreferencesGroup::new();
    setup_group.set_title("Setup");
    setup_group.set_description(Some(
        "What belongs to this group of plants. Only the plant is required.",
    ));

    let name_row = entry_row("Name");
    name_row.set_tooltip_text(Some(
        "Optional. The plant profile name is used when this is empty.",
    ));
    if let Some(planting) = existing_planting {
        name_row.set_text(planting.name.as_deref().unwrap_or_default());
    }

    let quantity_adjustment = gtk::Adjustment::new(1.0, 1.0, 99_999.0, 1.0, 10.0, 0.0);
    let quantity_row = adw::SpinRow::new(Some(&quantity_adjustment), 1.0, 0);
    quantity_row.set_title("Plants");
    quantity_row.set_tooltip_text(Some("How many plants or pots this group holds"));
    quantity_row.set_value(
        existing_planting
            .map(|planting| planting.quantity as f64)
            .unwrap_or(1.0),
    );

    let container_row = combo_row(
        "Container",
        &["Ground", "Pot", "Raised bed", "Hydroponics"],
        existing_planting
            .map(|planting| choice_index(CONTAINERS, &planting.container))
            .unwrap_or_else(|| choice_index(CONTAINERS, DEFAULT_CONTAINER)),
    );

    let units = UnitPreference::load(store);
    let original_volume = existing_planting.and_then(|planting| planting.container_volume_l);
    let shown_volume = original_volume
        .map(|volume| units.number("L", volume))
        .unwrap_or_default();
    let volume_row = entry_row("Pot volume");
    volume_row.set_input_purpose(gtk::InputPurpose::Number);
    volume_row.set_text(&shown_volume);
    let volume_unit = gtk::Label::new(Some(&units.label("L")));
    volume_unit.add_css_class("dim-label");
    volume_row.add_suffix(&volume_unit);

    let substrate_row = entry_row("Substrate");
    if let Some(substrate) = existing_planting.and_then(|planting| planting.substrate.as_deref()) {
        substrate_row.set_text(substrate);
    }

    let notes_row = entry_row("Notes");
    if let Some(notes) = existing_planting.and_then(|planting| planting.notes.as_deref()) {
        notes_row.set_text(notes);
    }

    setup_group.add(&name_row);
    setup_group.add(&quantity_row);
    setup_group.add(&container_row);
    setup_group.add(&volume_row);
    setup_group.add(&substrate_row);
    setup_group.add(&notes_row);

    let start_group = adw::PreferencesGroup::new();
    start_group.set_title("Start");

    let started_on = existing_planting
        .map(|planting| planting.started_on)
        .unwrap_or(today);
    let (started_on_row, date_value) = date_row("Started on", started_on);

    let start_kind_row = combo_row(
        "How it started",
        &["Planted", "Sown indoors", "Sown outdoors", "Transplanted"],
        existing_planting
            .map(|planting| choice_index(START_KINDS, &planting.start_kind))
            .unwrap_or_else(|| choice_index(START_KINDS, DEFAULT_START_KIND)),
    );

    start_group.add(&started_on_row);
    start_group.add(&start_kind_row);

    let mut choices: Vec<SpaceChoice> = vec![SpaceChoice::Private, SpaceChoice::New];
    let mut space_labels = vec!["On its own".to_string(), "New space…".to_string()];
    for space in &spaces {
        if let Some(name) = space.display_name() {
            space_labels.push(name.to_string());
            choices.push(SpaceChoice::Existing(space.clone()));
        }
    }

    let default_space_index = existing_space
        .as_ref()
        .and_then(|space| {
            space.display_name()?;
            choices.iter().position(|choice| {
                matches!(choice, SpaceChoice::Existing(existing) if existing.id == space.id)
            })
        })
        .unwrap_or(0) as u32;

    let choices = Rc::new(choices);
    let space_label_refs: Vec<&str> = space_labels.iter().map(String::as_str).collect();
    let space_row = combo_row("Space", &space_label_refs, default_space_index);
    let (moved_on_row, moved_on_value) = date_row("Moved on", today);
    moved_on_row.set_visible(false);
    moved_on_row.set_tooltip_text(Some(
        "The date the planting moved to the selected space. Today by default.",
    ));

    let space_name_row = entry_row("New space name");
    space_name_row.set_visible(default_space_index == 1);
    space_name_row.set_tooltip_text(Some(
        "A garden, a greenhouse, a tent or a room. Plantings in one space share light and climate.",
    ));

    let space_kind = existing_space
        .as_ref()
        .map(|space| space.kind.as_str())
        .unwrap_or(DEFAULT_SPACE_KIND);
    let space_kind_row = combo_row(
        "Space type",
        &["Outdoor", "Greenhouse", "Indoor"],
        choice_index(SPACE_KINDS, space_kind),
    );
    if let Some(space) = existing_space.as_ref()
        && store.space_has_history(space.id).unwrap_or(false)
    {
        space_kind_row.set_sensitive(false);
        space_kind_row.set_tooltip_text(Some(
            "A space with plantings, placements or readings keeps its type; create a new space instead.",
        ));
    }

    let light_type_labels = ["Not set", "Natural", "LED", "HPS", "Mixed"];
    let light_type_row = combo_row(
        "Light",
        &light_type_labels,
        light_type_index(
            existing_space
                .as_ref()
                .and_then(|space| space.light_type.as_deref()),
        ),
    );

    let light_hours_row = number_entry_row(
        "Light hours per day",
        existing_space.as_ref().and_then(|space| space.light_hours),
    );

    let light_start_row = entry_row("Lights on at");
    light_start_row.set_text(&light_start_text(
        existing_space.as_ref().and_then(|space| space.light_start),
    ));
    light_start_row.set_tooltip_text(Some(
        "Local time the light comes on, for example 06:00. The graph uses it to mark day and night.",
    ));

    let space_group = adw::PreferencesGroup::new();
    space_group.set_title("Space");
    space_group.set_description(Some(
        "Light and climate belong to the place, so every planting in this space shares them. \
         Choose an existing space or create one here. A powered light needs the local time it \
         comes on for the graph to mark day and night.",
    ));
    space_group.add(&space_row);
    space_group.add(&moved_on_row);
    space_group.add(&space_name_row);
    space_group.add(&space_kind_row);
    space_group.add(&light_type_row);
    space_group.add(&light_hours_row);
    space_group.add(&light_start_row);

    update_light_rows(
        &space_kind_row,
        &light_type_row,
        &light_hours_row,
        &light_start_row,
    );
    update_volume_row(&container_row, &volume_row);

    space_row.connect_selected_notify({
        let choices = choices.clone();
        let store = store.clone();
        let existing_space = existing_space.clone();
        let current_space_id = existing_space.as_ref().map(|space| space.id);
        let current_private = existing_space
            .as_ref()
            .is_some_and(|space| space.display_name().is_none());
        let date_value = date_value.clone();
        let moved_on_row = moved_on_row.clone();
        let moved_on_value = moved_on_value.clone();
        let space_name_row = space_name_row.clone();
        let space_kind_row = space_kind_row.clone();
        let light_type_row = light_type_row.clone();
        let light_hours_row = light_hours_row.clone();
        let light_start_row = light_start_row.clone();
        move |row| {
            let index = row.selected() as usize;
            space_name_row.set_visible(index == 1);

            let selected_id = match choices.get(index) {
                Some(SpaceChoice::Existing(space)) => Some(space.id),
                Some(SpaceChoice::Private) if current_private => current_space_id,
                _ => None,
            };
            let moving = planting_id.is_some() && selected_id != current_space_id;
            moved_on_row.set_visible(moving && date_value.get() <= today);
            if moving {
                moved_on_value.set(today);
            }

            let has_history = match choices.get(index) {
                Some(SpaceChoice::Existing(space)) => {
                    store.space_has_history(space.id).unwrap_or(false)
                }
                Some(SpaceChoice::Private) if current_private => current_space_id
                    .and_then(|id| store.space_has_history(id).ok())
                    .unwrap_or(false),
                _ => false,
            };
            space_kind_row.set_sensitive(!has_history);
            space_kind_row.set_tooltip_text(if has_history {
                Some("A space with plantings, placements or readings keeps its type; create a new space instead.")
            } else {
                None
            });

            if let Some(SpaceChoice::Existing(space)) = choices.get(index) {
                space_kind_row.set_selected(choice_index(SPACE_KINDS, &space.kind));
                light_type_row.set_selected(light_type_index(space.light_type.as_deref()));
                light_hours_row.set_text(&space.light_hours.map(format_number).unwrap_or_default());
                light_start_row.set_text(&light_start_text(space.light_start));
            } else if index == 0
                && let Some(space) = existing_space
                    .as_ref()
                    .filter(|space| space.display_name().is_none())
            {
                space_kind_row.set_selected(choice_index(SPACE_KINDS, &space.kind));
                light_type_row.set_selected(light_type_index(space.light_type.as_deref()));
                light_hours_row.set_text(&space.light_hours.map(format_number).unwrap_or_default());
                light_start_row.set_text(&light_start_text(space.light_start));
            } else {
                space_kind_row.set_selected(choice_index(SPACE_KINDS, DEFAULT_SPACE_KIND));
                light_type_row.set_selected(0);
                light_hours_row.set_text("");
                light_start_row.set_text("");
            }

            update_light_rows(
                &space_kind_row,
                &light_type_row,
                &light_hours_row,
                &light_start_row,
            );
        }
    });

    space_kind_row.connect_selected_notify({
        let light_type_row = light_type_row.clone();
        let light_hours_row = light_hours_row.clone();
        let light_start_row = light_start_row.clone();
        move |row| {
            update_light_rows(row, &light_type_row, &light_hours_row, &light_start_row);
        }
    });

    light_type_row.connect_selected_notify({
        let space_kind_row = space_kind_row.clone();
        let light_hours_row = light_hours_row.clone();
        let light_start_row = light_start_row.clone();
        move |row| {
            update_light_rows(&space_kind_row, row, &light_hours_row, &light_start_row);
        }
    });

    container_row.connect_selected_notify({
        let volume_row = volume_row.clone();
        move |row| update_volume_row(row, &volume_row)
    });

    let preferences = adw::PreferencesPage::new();
    preferences.add(&plant_group);
    preferences.add(&setup_group);
    preferences.add(&start_group);
    preferences.add(&space_group);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    let do_save: Rc<dyn Fn()> = Rc::new({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let overlay = overlay.clone();
        let choices = choices.clone();
        let existing_space = existing_space.clone();
        let name_row = name_row.clone();
        let quantity_row = quantity_row.clone();
        let container_row = container_row.clone();
        let volume_row = volume_row.clone();
        let shown_volume = shown_volume.clone();
        let substrate_row = substrate_row.clone();
        let notes_row = notes_row.clone();
        let date_value = date_value.clone();
        let start_kind_row = start_kind_row.clone();
        let space_row = space_row.clone();
        let space_name_row = space_name_row.clone();
        let space_kind_row = space_kind_row.clone();
        let light_type_row = light_type_row.clone();
        let light_hours_row = light_hours_row.clone();
        let light_start_row = light_start_row.clone();

        move || {
            let kind = SPACE_KINDS
                .get(space_kind_row.selected() as usize)
                .copied()
                .unwrap_or(DEFAULT_SPACE_KIND);
            let light_type = if kind == "outdoor" {
                None
            } else {
                light_type_value(light_type_row.selected())
            };
            let powered = matches!(light_type.as_deref(), Some("led" | "hps" | "mixed"));
            let light_hours = if !powered {
                None
            } else {
                match parse_optional_number(&light_hours_row.text()) {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            };
            let light_hours = match light_hours {
                Some(hours) if (0.0..=24.0).contains(&hours) => Some(hours),
                Some(_) => {
                    overlay.add_toast(adw::Toast::new("Light hours must be between 0 and 24."));
                    return;
                }
                None => None,
            };
            let light_start = if !powered {
                None
            } else {
                match parse_light_start(&light_start_row.text()) {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            };
            let volume_text = volume_row.text();
            let volume = match parse_optional_number(&volume_text) {
                Ok(value) => {
                    if volume_text.trim() == shown_volume.trim() {
                        original_volume
                    } else {
                        value.map(|value| units.input_to_canonical("L", value))
                    }
                }
                Err(message) => {
                    overlay.add_toast(adw::Toast::new(&message));
                    return;
                }
            };
            let quantity = quantity_row.value().round().max(1.0) as i64;
            let container = CONTAINERS
                .get(container_row.selected() as usize)
                .copied()
                .unwrap_or(DEFAULT_CONTAINER);
            let start_kind = START_KINDS
                .get(start_kind_row.selected() as usize)
                .copied()
                .unwrap_or(DEFAULT_START_KIND);

            let moved_on = (moved_on_row.is_visible() && date_value.get() <= today)
                .then(|| moved_on_value.get());
            if moved_on.is_some_and(|date| date > today) {
                overlay.add_toast(adw::Toast::new("The move date cannot be in the future."));
                return;
            }

            let input = PlantingInput {
                profile_id,
                space_id: 0,
                name: clean_text(&name_row.text()),
                quantity,
                container: container.to_string(),
                container_volume_l: if container == "pot" { volume } else { None },
                substrate: clean_text(&substrate_row.text()),
                started_on: date_value.get(),
                start_kind: start_kind.to_string(),
                moved_on,
                notes: clean_text(&notes_row.text()),
            };

            if let Err(error) = input.validate() {
                overlay.add_toast(adw::Toast::new(&error.to_string()));
                return;
            }

            let space_choice = choices.get(space_row.selected() as usize).cloned();
            let current_space = existing_space.clone();
            let new_space_name = space_name_row.text().trim().to_string();
            let space_kind = kind.to_string();
            let space_light_type = light_type.clone();
            let space_light_hours = light_hours;
            let space_light_start = light_start;
            let mut input = input;

            let result: std::result::Result<SaveOutcome, String> = store
                .transaction(|s| {
                    let space_input = |name: Option<String>| SpaceInput {
                        name,
                        kind: space_kind.clone(),
                        light_type: space_light_type.clone(),
                        light_hours: space_light_hours,
                        light_start: space_light_start,
                        notes: None,
                    };

                    let space_id = match &space_choice {
                        Some(SpaceChoice::Existing(space)) => {
                            let mut update = space_input(space.name.clone());
                            update.notes = space.notes.clone();
                            s.update_space(space.id, &update)?;
                            space.id
                        }
                        Some(SpaceChoice::New) => {
                            if new_space_name.is_empty() {
                                return Err(
                                    ValidationError("give the new space a name".into()).into()
                                );
                            }
                            s.create_space(&space_input(Some(new_space_name.clone())))?
                        }
                        _ => match current_space.as_ref() {
                            Some(space) if space.display_name().is_none() => {
                                let mut update = space_input(None);
                                update.notes = space.notes.clone();
                                s.update_space(space.id, &update)?;
                                space.id
                            }
                            _ => s.create_space(&space_input(None))?,
                        },
                    };

                    input.space_id = space_id;
                    match planting_id {
                        Some(id) => {
                            s.update_planting(id, &input)?;
                            Ok(SaveOutcome::Updated)
                        }
                        None => {
                            let id = s.create_planting(&input)?;
                            Ok(SaveOutcome::Created(id))
                        }
                    }
                })
                .map_err(|error| error.to_string());

            match result {
                Ok(SaveOutcome::Created(id)) => {
                    replace_with_view(&store, &nav, &root_page, id);
                }
                Ok(SaveOutcome::Updated) => {
                    if let Some(id) = planting_id {
                        replace_with_view(&store, &nav, &root_page, id);
                    } else {
                        nav.pop();
                    }
                }
                Err(message) => {
                    eprintln!("could not save planting: {message}");
                    overlay.add_toast(adw::Toast::new(&message));
                }
            }
        }
    });

    save.connect_clicked({
        let do_save = do_save.clone();
        move |_| do_save()
    });
    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    let page = adw::NavigationPage::new(&toolbar, "Planting setup");
    guard.attach(&page, &toolbar);
    page
}

fn missing_profile_page(profile_id: i64) -> adw::NavigationPage {
    super::shared::form::missing_page(
        "New planting",
        "Planting setup",
        "This plant profile no longer exists",
        Some(&format!("Profile {profile_id} could not be opened.")),
    )
}

pub(super) fn space_editor(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    space_id: i64,
    on_saved: Rc<dyn Fn()>,
) -> adw::NavigationPage {
    let Some(space) = store.space(space_id).ok().flatten() else {
        return missing_space_page();
    };

    let guard = Guard::new(nav);
    let header = guard.header("Space");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let group = adw::PreferencesGroup::new();
    group.set_title("Space");
    group.set_description(Some(if space.display_name().is_some() {
        "Everything in this space shares the light and climate you set here. For LED, HPS or \
         mixed light, set when it comes on; the graph uses it for day and night."
    } else {
        "This space belongs to one planting. Give it a name to share it with another planting."
    }));

    let name_row = entry_row("Name");
    name_row.set_text(space.name.as_deref().unwrap_or_default());
    name_row.set_tooltip_text(Some(if space.display_name().is_some() {
        "The name shown in the picker. A space with more than one planting needs a name."
    } else {
        "Leave empty to keep this space private to this planting."
    }));

    let kind_row = combo_row(
        "Space type",
        &["Outdoor", "Greenhouse", "Indoor"],
        choice_index(SPACE_KINDS, &space.kind),
    );
    if store.space_has_history(space_id).unwrap_or(false) {
        kind_row.set_sensitive(false);
        kind_row.set_tooltip_text(Some(
            "A space with plantings, placements or readings keeps its type; create a new space instead.",
        ));
    }

    let light_type_row = combo_row(
        "Light",
        &["Not set", "Natural", "LED", "HPS", "Mixed"],
        light_type_index(space.light_type.as_deref()),
    );

    let light_hours_row = number_entry_row("Light hours per day", space.light_hours);
    let light_start_row = entry_row("Lights on at");
    light_start_row.set_text(&light_start_text(space.light_start));
    light_start_row.set_tooltip_text(Some(
        "Local time the light comes on, for example 06:00. The graph uses it to mark day and night.",
    ));
    let notes_row = entry_row("Notes");
    notes_row.set_text(space.notes.as_deref().unwrap_or_default());

    group.add(&name_row);
    group.add(&kind_row);
    group.add(&light_type_row);
    group.add(&light_hours_row);
    group.add(&light_start_row);
    group.add(&notes_row);

    update_light_rows(
        &kind_row,
        &light_type_row,
        &light_hours_row,
        &light_start_row,
    );
    kind_row.connect_selected_notify({
        let light_type_row = light_type_row.clone();
        let light_hours_row = light_hours_row.clone();
        let light_start_row = light_start_row.clone();
        move |row| update_light_rows(row, &light_type_row, &light_hours_row, &light_start_row)
    });
    light_type_row.connect_selected_notify({
        let kind_row = kind_row.clone();
        let light_hours_row = light_hours_row.clone();
        let light_start_row = light_start_row.clone();
        move |row| update_light_rows(&kind_row, row, &light_hours_row, &light_start_row)
    });

    let preferences = adw::PreferencesPage::new();
    preferences.add(&group);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    save.connect_clicked({
        let store = store.clone();
        let on_saved = on_saved.clone();
        let overlay = overlay.clone();
        let name_row = name_row.clone();
        let kind_row = kind_row.clone();
        let light_type_row = light_type_row.clone();
        let light_hours_row = light_hours_row.clone();
        let light_start_row = light_start_row.clone();
        let notes_row = notes_row.clone();

        move |_| {
            let kind = SPACE_KINDS
                .get(kind_row.selected() as usize)
                .copied()
                .unwrap_or(DEFAULT_SPACE_KIND);
            let light_type = if kind == "outdoor" {
                None
            } else {
                light_type_value(light_type_row.selected())
            };
            let powered = matches!(light_type.as_deref(), Some("led" | "hps" | "mixed"));
            let light_hours = if !powered {
                None
            } else {
                match parse_optional_number(&light_hours_row.text()) {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            };
            let light_hours = match light_hours {
                Some(hours) if (0.0..=24.0).contains(&hours) => Some(hours),
                Some(_) => {
                    overlay.add_toast(adw::Toast::new("Light hours must be between 0 and 24."));
                    return;
                }
                None => None,
            };
            let light_start = if !powered {
                None
            } else {
                match parse_light_start(&light_start_row.text()) {
                    Ok(value) => value,
                    Err(message) => {
                        overlay.add_toast(adw::Toast::new(&message));
                        return;
                    }
                }
            };

            let input = SpaceInput {
                name: clean_text(&name_row.text()),
                kind: kind.to_string(),
                light_type,
                light_hours,
                light_start,
                notes: clean_text(&notes_row.text()),
            };

            match store.update_space(space_id, &input) {
                Ok(()) => on_saved(),
                Err(error) => {
                    eprintln!("could not save space {space_id}: {error}");
                    overlay.add_toast(adw::Toast::new(&error.to_string()));
                }
            }
        }
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    let page = adw::NavigationPage::new(&toolbar, "Space");
    guard.attach(&page, &toolbar);
    page
}

fn missing_space_page() -> adw::NavigationPage {
    super::shared::form::missing_page(
        "Space",
        "Space",
        "This space no longer exists",
        Some("It may have been removed together with its last planting."),
    )
}
