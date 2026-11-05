//! The Settings page: Open-Meteo, location, frost dates, display and agent sources.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::date_format::DateFormat;
use crate::frost;
use crate::frost_lookup::{self, FIRST_YEAR, LAST_YEAR};
use crate::geocode;
use crate::online;
use crate::proposal;
use crate::store::{Location, Store};
use crate::units::UnitPreference;
use crate::weather;

use super::{page_header, run_async};

pub(super) fn section(store: &Rc<Store>) -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    let date_format = DateFormat::system();

    // One group for every Open-Meteo setting: the switch allows all of it and the budget limits
    // the automatic weather sync. Credits live on the Help page.
    let online_group = adw::PreferencesGroup::new();
    online_group.set_title("Open-Meteo");
    online_group.set_description(Some(
        "Place search, the frost estimate and the weather for your location all come from \
         Open-Meteo. Off means no network requests; coordinates, frost dates and stored weather \
         stay.",
    ));

    let online_switch = adw::SwitchRow::builder()
        .title("Use Open-Meteo")
        .subtitle("Search places, estimate frost dates and refresh the weather while the app runs.")
        .active(online::enabled(store))
        .build();

    let weather_per_day = adw::EntryRow::new();
    weather_per_day.set_title("Weather updates per day");
    weather_per_day.set_input_purpose(gtk::InputPurpose::Number);
    weather_per_day.set_text(&weather::settings(store).per_day.to_string());
    weather_per_day.set_tooltip_text(Some(&format!(
        "Between 1 and {}; twelve a day is a calm default.",
        weather::MAX_PER_DAY
    )));

    let weather_status = adw::ActionRow::builder()
        .title("Last weather update")
        .build();
    weather_status.set_activatable(false);
    weather_status.set_focusable(false);

    online_group.add(&online_switch);
    online_group.add(&weather_per_day);
    online_group.add(&weather_status);
    page.add(&online_group);

    let location_group = adw::PreferencesGroup::new();
    location_group.set_title("Location");
    location_group.set_description(Some(
        "Used for the weather model, the sun times behind day and night, and local growing advice. \
         A greenhouse or indoor space with natural or mixed light needs it too.",
    ));

    let current_location = adw::ActionRow::new();
    current_location.set_title("No location set");
    current_location.set_subtitle("Search a city or enter coordinates below.");
    current_location.set_activatable(false);

    let remove_location = adw::ActionRow::builder()
        .title("Remove location…")
        .subtitle("Weather and sun times stop; the frost dates stay, without an estimate source.")
        .activatable(true)
        .build();
    remove_location.set_visible(false);

    let place = adw::EntryRow::new();
    place.set_title("City or place");
    place.set_tooltip_text(Some(
        "Add a country to narrow it down, for example Lisbon, Portugal",
    ));

    let search = gtk::Button::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text("Search Open-Meteo")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    place.add_suffix(&search);

    let search_spinner = gtk::Spinner::new();
    search_spinner.set_valign(gtk::Align::Center);
    search_spinner.set_visible(false);
    place.add_suffix(&search_spinner);

    let coordinates = adw::ExpanderRow::new();
    coordinates.set_title("Coordinates");
    coordinates.set_subtitle("Fine-tune the place or enter a point by hand.");

    let latitude = adw::EntryRow::new();
    latitude.set_title("Latitude");
    latitude.set_input_purpose(gtk::InputPurpose::Number);

    let longitude = adw::EntryRow::new();
    longitude.set_title("Longitude");
    longitude.set_input_purpose(gtk::InputPurpose::Number);

    coordinates.add_row(&latitude);
    coordinates.add_row(&longitude);

    let use_coordinates = adw::ActionRow::builder()
        .title("Use these coordinates")
        .subtitle("Nothing is saved while you type.")
        .activatable(true)
        .build();
    coordinates.add_row(&use_coordinates);

    location_group.add(&current_location);
    location_group.add(&remove_location);
    location_group.add(&place);
    location_group.add(&coordinates);
    page.add(&location_group);

    let results_group = adw::PreferencesGroup::new();
    results_group.set_title("Search results");
    results_group.set_description(Some("Pick a place to use its coordinates and timezone."));
    results_group.set_visible(false);
    page.add(&results_group);

    let frost_group = adw::PreferencesGroup::new();
    frost_group.set_title("Frost dates");
    frost_group.set_description(Some(&frost_description(date_format)));

    let (last_tooltip, first_tooltip) = frost_tooltips(date_format);

    let last_frost = adw::EntryRow::new();
    last_frost.set_title("Last spring frost");
    last_frost.set_tooltip_text(Some(last_tooltip));

    let first_frost = adw::EntryRow::new();
    first_frost.set_title("First autumn frost");
    first_frost.set_tooltip_text(Some(first_tooltip));

    let estimate = adw::ActionRow::builder()
        .title(format!(
            "Estimate from {} years of weather",
            LAST_YEAR - FIRST_YEAR + 1
        ))
        .subtitle(format!(
            "Open-Meteo ERA5-Land, {FIRST_YEAR}–{LAST_YEAR}. A grid estimate, not your garden."
        ))
        .activatable(true)
        .build();

    let estimate_spinner = gtk::Spinner::new();
    estimate_spinner.set_valign(gtk::Align::Center);
    estimate_spinner.set_visible(false);
    estimate.add_suffix(&estimate_spinner);

    let source_row = adw::ActionRow::new();
    source_row.set_title("Source");
    source_row.set_activatable(false);
    source_row.set_visible(false);

    frost_group.add(&last_frost);
    frost_group.add(&first_frost);
    frost_group.add(&estimate);
    frost_group.add(&source_row);
    page.add(&frost_group);

    let display_group = adw::PreferencesGroup::new();
    display_group.set_title("Display");
    display_group.set_description(Some(
        "Stored values stay metric; this only changes what you read and type.",
    ));

    let units_model =
        gtk::StringList::new(&["Metric (kg, cm, L, °C)", "Imperial (lb, in, US gal, °F)"]);
    let units_row = adw::ComboRow::builder()
        .title("Units")
        .model(&units_model)
        .build();
    units_row.set_selected(if UnitPreference::load(store).is_imperial() {
        1
    } else {
        0
    });
    display_group.add(&units_row);
    page.add(&display_group);

    let agent = adw::PreferencesGroup::new();
    agent.set_title("Agent sources");
    agent.set_description(Some(
        "What a coding agent may claim when it fills in a plant profile.",
    ));

    let allow_model_knowledge = adw::SwitchRow::builder()
        .title("Allow values from the model's own knowledge")
        .subtitle(
            "Without a source, such a value is marked as unreliable. Off means every value needs a \
             cited source.",
        )
        .build();
    allow_model_knowledge.set_active(
        proposal::SourcePolicy::load(store).unwrap_or(proposal::SourcePolicy::SourcesOnly)
            == proposal::SourcePolicy::ModelKnowledge,
    );
    allow_model_knowledge.connect_active_notify({
        let store = store.clone();
        move |row| {
            let policy = if row.is_active() {
                proposal::SourcePolicy::ModelKnowledge
            } else {
                proposal::SourcePolicy::SourcesOnly
            };

            if let Err(error) = policy.save(&store) {
                eprintln!("could not save the agent source policy: {error}");
            }
        }
    });
    agent.add(&allow_model_knowledge);
    page.add(&agent);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&page));

    if let Ok(Some(location)) = store.location() {
        show_location(&current_location, &location);
        latitude.set_text(&location.latitude.to_string());
        longitude.set_text(&location.longitude.to_string());
        remove_location.set_visible(true);
    }

    if let Ok(dates) = frost::FrostDates::load(store) {
        last_frost.set_text(&date_format.format(dates.last));
        first_frost.set_text(&date_format.format(dates.first));
    }

    if let Some(source) = frost::load_source(store) {
        source_row.set_subtitle(&source);
        source_row.set_visible(true);
    }

    let toast = {
        let overlay = overlay.clone();
        Rc::new(move |message: &str| {
            overlay.add_toast(adw::Toast::new(message));
        }) as Rc<dyn Fn(&str)>
    };

    // The rows are filled from the store; only a change by the user should be saved back.
    let weather_syncing = Rc::new(Cell::new(false));

    let weather_status_text = {
        let store = store.clone();
        Rc::new(move || {
            let settings = weather::settings(&store);
            let budget = weather::budget(&store, crate::clock::today());
            let today = crate::clock::today();

            let base = match budget.last_call {
                Some((date, minutes)) if date == today => format!(
                    "{:02}:{:02} · {} of {} used today",
                    minutes / 60,
                    minutes % 60,
                    budget.calls_today,
                    settings.per_day
                ),
                Some((date, minutes)) => format!(
                    "{} {:02}:{:02} · nothing yet today",
                    date.to_iso(),
                    minutes / 60,
                    minutes % 60
                ),
                None => "No call yet".to_string(),
            };

            if !online::enabled(&store) {
                return format!("Paused · {base}");
            }

            match weather::last_error(&store) {
                Some(error) => format!("{base} · last attempt failed: {error}"),
                None => base,
            }
        }) as Rc<dyn Fn() -> String>
    };

    let refresh_weather = {
        let store = store.clone();
        let weather_per_day = weather_per_day.clone();
        let weather_status = weather_status.clone();
        let weather_status_text = weather_status_text.clone();
        let weather_syncing = weather_syncing.clone();
        Rc::new(move || {
            weather_syncing.set(true);

            let settings = weather::settings(&store);
            let enabled = online::enabled(&store);
            weather_per_day.set_text(&settings.per_day.to_string());
            weather_per_day.set_sensitive(enabled);
            let status = weather_status_text();
            weather_status.set_subtitle(&status);
            weather_status.set_tooltip_text(Some(&status));

            weather_syncing.set(false);
        })
    };

    weather_per_day.connect_changed({
        let store = store.clone();
        let weather_syncing = weather_syncing.clone();
        let weather_status = weather_status.clone();
        let weather_status_text = weather_status_text.clone();
        move |row| {
            if weather_syncing.get() {
                return;
            }

            let Ok(per_day) = row.text().trim().parse::<i64>() else {
                return;
            };

            if !(1..=weather::MAX_PER_DAY).contains(&per_day) {
                return;
            }

            if let Err(error) = weather::set_per_day(&store, per_day) {
                eprintln!("could not save the weather budget: {error}");
            }

            let status = weather_status_text();
            weather_status.set_subtitle(&status);
            weather_status.set_tooltip_text(Some(&status));
        }
    });

    refresh_weather();

    units_row.connect_selected_notify({
        let store = store.clone();
        let toast = toast.clone();
        move |row| {
            let preference = if row.selected() == 1 {
                UnitPreference::Imperial
            } else {
                UnitPreference::Metric
            };

            if let Err(error) = preference.save(&store) {
                eprintln!("could not save the unit preference: {error}");
                return;
            }

            toast("Units changed. Values update when you open a profile.");
        }
    });

    let refresh_current = {
        let store = store.clone();
        let current_location = current_location.clone();
        Rc::new(move || match store.location() {
            Ok(Some(location)) => show_location(&current_location, &location),
            _ => {
                current_location.set_title("No location set");
                current_location.set_subtitle("Search a city or enter coordinates below.");
            }
        }) as Rc<dyn Fn()>
    };

    let busy = Rc::new(Cell::new(false));
    // A search that is still running when the page is left must not fill the list when the user
    // returns; every search and every unmap bumps this token.
    let search_generation = Rc::new(Cell::new(0u64));
    let result_rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));

    let refresh_actions = {
        let online_switch = online_switch.clone();
        let busy = busy.clone();
        let search = search.clone();
        let estimate = estimate.clone();
        Rc::new(move || {
            let ready = online_switch.is_active() && !busy.get();
            search.set_sensitive(ready);
            estimate.set_sensitive(ready);
        }) as Rc<dyn Fn()>
    };

    // The one write path for a location. A real move drops the frost claim but keeps the dates;
    // both writes share a transaction, so a failure leaves neither behind.
    let commit_location: Rc<dyn Fn(Location) -> bool> = {
        let store = store.clone();
        let latitude = latitude.clone();
        let longitude = longitude.clone();
        let remove_location = remove_location.clone();
        let source_row = source_row.clone();
        let refresh_current = refresh_current.clone();
        let toast = toast.clone();
        Rc::new(move |location| {
            let previous = store.location().ok().flatten();
            let moved = previous.as_ref().is_none_or(|old| {
                old.latitude != location.latitude || old.longitude != location.longitude
            });
            let had_source = moved && frost::load_source(&store).is_some();

            let written = store.transaction(|store| {
                store.set_location(location.clone())?;
                if had_source {
                    frost::save_source(store, "")?;
                }
                Ok(())
            });

            match written {
                Ok(()) => {
                    if had_source {
                        source_row.set_subtitle("");
                        source_row.set_visible(false);
                    }

                    latitude.set_text(&location.latitude.to_string());
                    longitude.set_text(&location.longitude.to_string());
                    remove_location.set_visible(true);
                    refresh_current();

                    let label = location_label(&location);
                    if had_source {
                        toast(&format!(
                            "Location set to {label}. Estimate the frost dates again."
                        ));
                    } else {
                        toast(&format!("Location set to {label}"));
                    }

                    // Do not make the new place wait for the old place's interval.
                    super::sync_weather_after_location_change(&store);
                    true
                }
                Err(error) => {
                    eprintln!("could not save the location: {error}");
                    toast("Could not save the location.");
                    false
                }
            }
        })
    };

    let apply_place: Rc<dyn Fn(geocode::Place)> = {
        let store = store.clone();
        let place_row = place.clone();
        let results_group = results_group.clone();
        let result_rows = result_rows.clone();
        let commit_location = commit_location.clone();
        Rc::new(move |place| {
            let label = place.label();
            let location = place.into_location();
            let stored = store.location().ok().flatten();
            let same = stored.as_ref().is_some_and(|old| {
                old.latitude == location.latitude && old.longitude == location.longitude
            });

            let finish = {
                let place_row = place_row.clone();
                let results_group = results_group.clone();
                let result_rows = result_rows.clone();
                let commit_location = commit_location.clone();
                Rc::new(move || {
                    if commit_location(location.clone()) {
                        place_row.set_text("");
                        clear_results(&results_group, &result_rows);
                        results_group.set_visible(false);
                    }
                }) as Rc<dyn Fn()>
            };

            let Some(current) = stored.filter(|_| !same) else {
                finish();
                return;
            };

            confirm(
                &place_row,
                "Change your location?",
                &format!(
                    "Weather and sun times will follow {label} instead of {}. Your frost dates \
                     stay; any estimate source is cleared.",
                    location_label(&current)
                ),
                "Change location",
                adw::ResponseAppearance::Suggested,
                finish,
            );
        })
    };

    let apply_coordinates: Rc<dyn Fn()> = {
        let store = store.clone();
        let latitude = latitude.clone();
        let longitude = longitude.clone();
        let commit_location = commit_location.clone();
        let toast = toast.clone();
        let anchor = use_coordinates.clone();
        Rc::new(move || {
            let latitude_text = latitude.text();
            let longitude_text = longitude.text();
            let parsed_latitude = parse_coordinate(&latitude_text, 90.0);
            let parsed_longitude = parse_coordinate(&longitude_text, 180.0);

            mark_invalid(
                &latitude,
                parsed_latitude.is_none() && !latitude_text.trim().is_empty(),
            );
            mark_invalid(
                &longitude,
                parsed_longitude.is_none() && !longitude_text.trim().is_empty(),
            );

            let (Some(latitude_value), Some(longitude_value)) = (parsed_latitude, parsed_longitude)
            else {
                toast("Enter a valid latitude and longitude first.");
                return;
            };

            let stored = store.location().ok().flatten();
            if stored.as_ref().is_some_and(|old| {
                old.latitude == latitude_value && old.longitude == longitude_value
            }) {
                toast("Those are already the current coordinates.");
                return;
            }

            // Keep the place name only when the point is a fine-tune of the same place; a point
            // far away is a different place, so the old name and timezone would lie.
            let place = stored.as_ref().filter(|old| {
                (old.latitude - latitude_value).abs() <= 0.1
                    && (old.longitude - longitude_value).abs() <= 0.1
            });
            let location = Location {
                latitude: latitude_value,
                longitude: longitude_value,
                name: place.and_then(|location| location.name.clone()),
                region: place.and_then(|location| location.region.clone()),
                country: place.and_then(|location| location.country.clone()),
                timezone: place.and_then(|location| location.timezone.clone()),
            };

            let Some(current) = stored else {
                let _ = commit_location(location);
                return;
            };

            let accept = {
                let commit_location = commit_location.clone();
                Rc::new(move || {
                    let _ = commit_location(location.clone());
                }) as Rc<dyn Fn()>
            };

            confirm(
                &anchor,
                "Change your location?",
                &format!(
                    "Weather and sun times will follow the new coordinates instead of {}. Your \
                     frost dates stay; any estimate source is cleared.",
                    location_label(&current)
                ),
                "Change location",
                adw::ResponseAppearance::Suggested,
                accept,
            );
        })
    };

    remove_location.connect_activated({
        let store = store.clone();
        let remove_location = remove_location.clone();
        let latitude = latitude.clone();
        let longitude = longitude.clone();
        let source_row = source_row.clone();
        let refresh_current = refresh_current.clone();
        let place_row = place.clone();
        let results_group = results_group.clone();
        let result_rows = result_rows.clone();
        let toast = toast.clone();
        move |row| {
            let store = store.clone();
            let remove_location = remove_location.clone();
            let latitude = latitude.clone();
            let longitude = longitude.clone();
            let source_row = source_row.clone();
            let refresh_current = refresh_current.clone();
            let place_row = place_row.clone();
            let results_group = results_group.clone();
            let result_rows = result_rows.clone();
            let toast = toast.clone();

            confirm(
                row,
                "Remove this location?",
                "Weather updates stop, and day and night that follow the sun need a place again. \
                 Your frost dates stay; any estimate source is cleared.",
                "Remove location",
                adw::ResponseAppearance::Destructive,
                Rc::new(move || {
                    let had_source = frost::load_source(&store).is_some();
                    let removed = store.transaction(|store| {
                        store.clear_location()?;
                        if had_source {
                            frost::save_source(store, "")?;
                        }
                        Ok(())
                    });

                    match removed {
                        Ok(()) => {
                            if had_source {
                                source_row.set_subtitle("");
                                source_row.set_visible(false);
                            }

                            latitude.set_text("");
                            longitude.set_text("");
                            mark_invalid(&latitude, false);
                            mark_invalid(&longitude, false);
                            remove_location.set_visible(false);
                            refresh_current();
                            place_row.set_text("");
                            clear_results(&results_group, &result_rows);
                            results_group.set_visible(false);
                            toast("Location removed.");
                        }
                        Err(error) => {
                            eprintln!("could not remove the location: {error}");
                            toast("Could not remove the location.");
                        }
                    }
                }),
            );
        }
    });

    use_coordinates.connect_activated({
        let apply_coordinates = apply_coordinates.clone();
        move |_| apply_coordinates()
    });
    for row in [&latitude, &longitude] {
        row.connect_entry_activated({
            let apply_coordinates = apply_coordinates.clone();
            move |_| apply_coordinates()
        });
    }

    let start_search: Rc<dyn Fn()> = {
        let store = store.clone();
        let place_row = place.clone();
        let results_group = results_group.clone();
        let result_rows = result_rows.clone();
        let search_spinner = search_spinner.clone();
        let busy = busy.clone();
        let search_generation = search_generation.clone();
        let refresh_actions = refresh_actions.clone();
        let apply_place = apply_place.clone();
        let toast = toast.clone();
        Rc::new(move || {
            let store = store.clone();
            let busy = busy.clone();
            let search_generation = search_generation.clone();
            let refresh_actions = refresh_actions.clone();
            let search_spinner = search_spinner.clone();
            let toast = toast.clone();
            let results_group = results_group.clone();
            let result_rows = result_rows.clone();
            let apply_place = apply_place.clone();

            if !online::enabled(&store) {
                toast("Open-Meteo is off in Settings. Turn it on to search a place.");
                return;
            }

            let query = place_row.text().to_string();
            if query.trim().chars().count() < 2 {
                toast("Type at least two characters, for example Lisbon.");
                return;
            }

            if busy.get() {
                toast("Wait for the current search to finish.");
                return;
            }

            let generation = search_generation.get().wrapping_add(1);
            search_generation.set(generation);
            busy.set(true);
            refresh_actions();
            search_spinner.start();
            search_spinner.set_visible(true);
            clear_results(&results_group, &result_rows);
            results_group.set_visible(false);

            let current_place_row = place_row.clone();
            let current_query = query.trim().to_string();

            run_async(
                move || geocode::search(&query),
                move |result| {
                    busy.set(false);
                    refresh_actions();
                    search_spinner.stop();
                    search_spinner.set_visible(false);

                    if search_generation.get() != generation {
                        return;
                    }
                    if !online::enabled(&store) {
                        return;
                    }
                    if current_place_row.text().trim() != current_query {
                        toast("The search box changed. Search again.");
                        return;
                    }

                    match result {
                        Some(Ok(places)) if places.is_empty() => {
                            toast("No place found for that search.");
                        }
                        Some(Ok(places)) => {
                            for place in places {
                                let row = adw::ActionRow::builder()
                                    .title(place.label())
                                    .subtitle(place_details(&place))
                                    .activatable(true)
                                    .build();

                                let apply = apply_place.clone();
                                row.connect_activated(move |_| apply(place.clone()));
                                results_group.add(&row);
                                result_rows.borrow_mut().push(row);
                            }

                            results_group.set_visible(true);
                        }
                        Some(Err(error)) => toast(&error),
                        None => toast("The search stopped unexpectedly."),
                    }
                },
            );
        }) as Rc<dyn Fn()>
    };

    let start_estimate: Rc<dyn Fn()> = {
        let store = store.clone();
        let last_frost = last_frost.clone();
        let first_frost = first_frost.clone();
        let source_row = source_row.clone();
        let estimate_spinner = estimate_spinner.clone();
        let busy = busy.clone();
        let refresh_actions = refresh_actions.clone();
        let toast = toast.clone();
        Rc::new(move || {
            let store = store.clone();
            let last_frost = last_frost.clone();
            let first_frost = first_frost.clone();
            let source_row = source_row.clone();
            let estimate_spinner = estimate_spinner.clone();
            let busy = busy.clone();
            let refresh_actions = refresh_actions.clone();
            let toast = toast.clone();

            if !online::enabled(&store) {
                toast("Open-Meteo is off in Settings. Turn it on to estimate frost dates.");
                return;
            }

            // The estimate belongs to the place that is saved, not to coordinates still being
            // typed; they are only a proposal until the user applies them.
            let Some(location) = store.location().ok().flatten() else {
                toast("Set a location first.");
                return;
            };
            let latitude_value = location.latitude;
            let longitude_value = location.longitude;

            if busy.get() {
                toast("Wait for the current frost estimate to finish.");
                return;
            }

            busy.set(true);
            refresh_actions();
            estimate_spinner.start();
            estimate_spinner.set_visible(true);

            let last_text = last_frost.text().to_string();
            let first_text = first_frost.text().to_string();

            run_async(
                move || frost_lookup::lookup(latitude_value, longitude_value),
                move |result| {
                    busy.set(false);
                    refresh_actions();
                    estimate_spinner.stop();
                    estimate_spinner.set_visible(false);

                    // The switch may have gone off, or the place or dates may have changed,
                    // while we waited. Do not overwrite what the user did in the meantime.
                    if !online::enabled(&store) {
                        return;
                    }

                    let moved = store.location().ok().flatten().is_none_or(|location| {
                        location.latitude != latitude_value || location.longitude != longitude_value
                    });
                    let dates_changed = last_frost.text() != last_text.as_str()
                        || first_frost.text() != first_text.as_str();

                    if moved || dates_changed {
                        toast("The place or frost dates changed. Estimate again.");
                        return;
                    }

                    let estimate = match result {
                        Some(Ok(estimate)) => estimate,
                        Some(Err(error)) => {
                            toast(&error);
                            return;
                        }
                        None => {
                            toast("The lookup stopped unexpectedly.");
                            return;
                        }
                    };

                    if estimate.last.is_none() && estimate.first.is_none() {
                        toast("No frost found in the archive for this place.");
                        return;
                    }

                    // Write the pair and its source before the entries fire their debounced save,
                    // so that save sees no change and keeps the source line.
                    let dates = match frost::apply_estimate(&store, &estimate) {
                        Ok(dates) => dates,
                        Err(error) => {
                            eprintln!("could not save the estimated frost dates: {error}");
                            toast("Could not save the estimated frost dates.");
                            return;
                        }
                    };

                    let source = estimate.source();

                    last_frost.set_text(&date_format.format(dates.last));
                    first_frost.set_text(&date_format.format(dates.first));
                    source_row.set_subtitle(&source);
                    source_row.set_visible(true);
                    toast("Frost dates estimated. Check them against your garden.");
                },
            );
        }) as Rc<dyn Fn()>
    };

    online_switch.connect_active_notify({
        let store = store.clone();
        let results_group = results_group.clone();
        let result_rows = result_rows.clone();
        let refresh_actions = refresh_actions.clone();
        let refresh_weather = refresh_weather.clone();
        move |row| {
            if let Err(error) = online::set_enabled(&store, row.is_active()) {
                eprintln!("could not save the Open-Meteo switch: {error}");
            }

            clear_results(&results_group, &result_rows);
            results_group.set_visible(false);
            refresh_actions();
            refresh_weather();
        }
    });

    // Set while a save is flushed on the way out, so it does not announce itself on a page that
    // is no longer visible.
    let quiet = Rc::new(Cell::new(false));

    let save = {
        let store = store.clone();
        let quiet = quiet.clone();
        let last_frost = last_frost.clone();
        let first_frost = first_frost.clone();
        let source_row = source_row.clone();
        let overlay = overlay.clone();
        Rc::new(move || {
            let parsed_last = date_format.parse(&last_frost.text());
            let parsed_first = date_format.parse(&first_frost.text());

            mark_invalid(
                &last_frost,
                parsed_last.is_err() && !last_frost.text().trim().is_empty(),
            );
            mark_invalid(
                &first_frost,
                parsed_first.is_err() && !first_frost.text().trim().is_empty(),
            );

            if let (Ok(last), Ok(first)) = (parsed_last, parsed_first) {
                match frost::save_typed(&store, frost::FrostDates { last, first }) {
                    Ok(true) => {
                        // Typed by hand: the estimate no longer describes these dates.
                        source_row.set_subtitle("");
                        source_row.set_visible(false);

                        if !quiet.get() {
                            overlay.add_toast(adw::Toast::new("Frost dates saved"));
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!("could not save the frost dates: {error}");
                    }
                }
            }
        })
    };

    // Save shortly after the last keystroke instead of on every character.
    let pending: Rc<RefCell<Option<gtk::glib::SourceId>>> = Rc::new(RefCell::new(None));
    let schedule = {
        let pending = pending.clone();
        let save = save.clone();
        Rc::new(move || {
            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
            }

            let save = save.clone();
            let slot = pending.clone();
            let source = gtk::glib::timeout_add_local(Duration::from_millis(600), move || {
                slot.borrow_mut().take();
                save();
                gtk::glib::ControlFlow::Break
            });

            *pending.borrow_mut() = Some(source);
        })
    };

    for row in [&last_frost, &first_frost] {
        row.connect_changed({
            let schedule = schedule.clone();
            move |_| schedule()
        });
    }

    search.connect_clicked({
        let start_search = start_search.clone();
        move |_| start_search()
    });
    place.connect_entry_activated({
        let start_search = start_search.clone();
        move |_| start_search()
    });
    estimate.connect_activated({
        let start_estimate = start_estimate.clone();
        move |_| start_estimate()
    });

    refresh_actions();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&page_header("Settings"));
    toolbar.set_content(Some(&overlay));

    // A weather call may have happened since the page was built.
    toolbar.connect_map({
        let refresh_weather = refresh_weather.clone();
        move |_| refresh_weather()
    });

    // A search list belongs to the visit; unsaved coordinate text is dropped, and a frost date
    // that is still waiting for the debounce must not be lost when Settings is left or the window
    // closes.
    toolbar.connect_unmap({
        let store = store.clone();
        let pending = pending.clone();
        let save = save.clone();
        let quiet = quiet.clone();
        let place = place.clone();
        let latitude = latitude.clone();
        let longitude = longitude.clone();
        let results_group = results_group.clone();
        let result_rows = result_rows.clone();
        let search_generation = search_generation.clone();
        move |_| {
            search_generation.set(search_generation.get().wrapping_add(1));

            place.set_text("");
            clear_results(&results_group, &result_rows);
            results_group.set_visible(false);

            match store.location() {
                Ok(Some(location)) => {
                    latitude.set_text(&location.latitude.to_string());
                    longitude.set_text(&location.longitude.to_string());
                }
                _ => {
                    latitude.set_text("");
                    longitude.set_text("");
                }
            }
            mark_invalid(&latitude, false);
            mark_invalid(&longitude, false);

            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
                quiet.set(true);
                save();
                quiet.set(false);
            }
        }
    });

    toolbar.upcast()
}

fn frost_description(format: DateFormat) -> String {
    format!(
        "The last spring frost and the first autumn frost, as {} (for example {}). They turn \
         rules like “sow 70 days before the last frost” into dates; leave them empty if you only \
         grow indoors.",
        format.label(),
        format.example()
    )
}

fn frost_tooltips(format: DateFormat) -> (&'static str, &'static str) {
    match format {
        DateFormat::DayMonth => (
            "Day/month, for example 15/04",
            "Day/month, for example 20/11",
        ),
        DateFormat::MonthDay => (
            "Month/day, for example 04/15",
            "Month/day, for example 11/20",
        ),
    }
}

fn location_label(location: &Location) -> String {
    location
        .label()
        .unwrap_or_else(|| format!("{:.5}, {:.5}", location.latitude, location.longitude))
}

/// Ask before the location changes or disappears, then run `accept`.
fn confirm(
    anchor: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    accept_label: &str,
    appearance: adw::ResponseAppearance,
    accept: Rc<dyn Fn()>,
) {
    let Some(window) = anchor
        .upcast_ref::<gtk::Widget>()
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };

    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("accept", accept_label);
    dialog.set_response_appearance("accept", appearance);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |_, response| {
        if response == "accept" {
            accept();
        }
    });

    dialog.present(Some(&window));
}

fn show_location(row: &adw::ActionRow, location: &Location) {
    let title = location
        .label()
        .unwrap_or_else(|| "Coordinates entered by hand".to_string());
    row.set_title(&title);

    let mut subtitle = format!("{:.5}, {:.5}", location.latitude, location.longitude);
    if let Some(timezone) = location
        .timezone
        .as_deref()
        .map(str::trim)
        .filter(|timezone| !timezone.is_empty())
    {
        subtitle.push_str(" · ");
        subtitle.push_str(timezone);
    }

    row.set_subtitle(&subtitle);
}

fn place_details(place: &geocode::Place) -> String {
    let mut details = format!("{:.4}, {:.4}", place.latitude, place.longitude);
    if let Some(timezone) = place.timezone.as_deref() {
        details.push_str(" · ");
        details.push_str(timezone);
    }

    details
}

fn clear_results(group: &adw::PreferencesGroup, rows: &Rc<RefCell<Vec<adw::ActionRow>>>) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }
}

fn mark_invalid(row: &adw::EntryRow, invalid: bool) {
    if invalid {
        row.add_css_class("error");
    } else {
        row.remove_css_class("error");
    }
}

fn parse_coordinate(text: &str, limit: f64) -> Option<f64> {
    let value: f64 = text.trim().replace(',', ".").parse().ok()?;
    (value.is_finite() && value.abs() <= limit).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::parse_coordinate;

    #[test]
    fn accepts_dots_and_commas() {
        assert_eq!(parse_coordinate("51.05", 90.0), Some(51.05));
        assert_eq!(parse_coordinate(" 51,05 ", 90.0), Some(51.05));
        assert_eq!(parse_coordinate("-33.87", 90.0), Some(-33.87));
    }

    #[test]
    fn rejects_out_of_range_and_garbage() {
        assert_eq!(parse_coordinate("91", 90.0), None);
        assert_eq!(parse_coordinate("181", 180.0), None);
        assert_eq!(parse_coordinate("", 90.0), None);
        assert_eq!(parse_coordinate("north", 90.0), None);
        assert_eq!(parse_coordinate("NaN", 90.0), None);
    }
}
