//! The planting page: overview, diary and weather cards, graph, finish and space page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::calendar::Date;
use crate::clock::today;
use crate::frost::FrostDates;
use crate::media;
use crate::plant_data::{Stage, format_number};
use crate::planting::{OUTCOMES, Planting, PlantingState, PlantingSummary, Space};
use crate::schedule;
use crate::store::Store;
use crate::units::UnitPreference;

use super::planting_setup::{setup_page, space_editor};
use super::plantings::{SelectSlot, quantity_text, state_chip, status_detail};
use super::shared::avatar;
use super::shared::fields::{
    combo_row, date_row, light_start_text, light_type_text, outcome_label, short_date,
    space_kind_text, species_subtitle, start_kind_text,
};
use super::shared::layout::{Columns, Slot};
use super::shell::Guard;
use super::{page_header, planting_diary, planting_schedule};

/// Every planting page carries this tag prefix, so the Spaces section can tell a planting apart
/// from a space and move the sidebar highlight.
const PAGE_TAG: &str = "planting-";

pub(super) fn is_planting_page(page: &adw::NavigationPage) -> bool {
    page.tag().is_some_and(|tag| tag.starts_with(PAGE_TAG))
}

pub(super) fn planting_view(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    id: i64,
) -> Option<adw::NavigationPage> {
    let summary = store.planting(id).ok().flatten()?;

    let header = page_header(summary.title());
    let edit = gtk::Button::with_label("Edit");
    edit.add_css_class("flat");
    header.pack_end(&edit);

    let more = gtk::MenuButton::new();
    more.set_icon_name("view-more-symbolic");
    more.add_css_class("flat");
    more.set_tooltip_text(Some("More actions"));
    header.pack_end(&more);

    // A second top bar stays under the header whatever the body does, so a selected day that is
    // not today keeps its banner while the page scrolls.
    let banner = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    banner.add_css_class("omagrow-banner");
    let banner_text = gtk::Label::new(None);
    banner_text.set_xalign(0.0);
    banner_text.set_hexpand(true);
    banner_text.set_wrap(true);
    let banner_button = gtk::Button::with_label("Back to today");
    banner_button.add_css_class("flat");
    banner_button.add_css_class("omagrow-banner-action");
    banner.append(&banner_text);
    banner.append(&banner_button);

    let banner_revealer = gtk::Revealer::new();
    banner_revealer.set_child(Some(&banner));
    banner_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    banner_revealer.set_reveal_child(false);

    let overlay = adw::ToastOverlay::new();

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.set_valign(gtk::Align::Start);
    content.set_margin_top(24);
    content.set_margin_bottom(32);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&body);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(1200);
    clamp.set_tightening_threshold(1200);
    clamp.set_child(Some(&content));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    scroller.set_focusable(true);
    scroller.connect_map(|scroller| {
        scroller.grab_focus();
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&banner_revealer);
    toolbar.set_content(Some(&scroller));
    overlay.set_child(Some(&toolbar));

    let page = adw::NavigationPage::new(&overlay, "Planting");
    let tag = format!("{PAGE_TAG}{id}");
    page.set_tag(Some(&tag));

    let calendar_state = Rc::new(planting_schedule::CalendarState::new(today()));
    // The timeline zoom lives as long as the page: a calendar click or a layout switch keeps it.
    let timeline_zoom = Rc::new(Cell::new(planting_schedule::TimelineZoom::Year));
    let select_slot: SelectSlot = Rc::new(RefCell::new(None));
    // The body keeps its columns alive while the page is on screen; the width watcher holds a
    // weak handle to them.
    let columns_slot: Rc<RefCell<Option<Columns>>> = Rc::new(RefCell::new(None));

    // "Not today" only makes sense while today is a day of this grow. A planned or finished
    // planting opens on its own anchor date without a banner.
    let today_in_grow = summary.planting.started_on <= today()
        && summary
            .planting
            .ended_on
            .is_none_or(|ended| today() <= ended);

    let show_banner: Rc<dyn Fn(Date)> = Rc::new({
        let revealer = banner_revealer.clone();
        let text = banner_text.clone();
        move |date| {
            let today = today();
            if !today_in_grow || date == today {
                revealer.set_reveal_child(false);
            } else {
                text.set_text(&format!(
                    "Viewing {} {} {} — not today",
                    date.day,
                    crate::calendar::month_name(date.month),
                    date.year
                ));
                revealer.set_reveal_child(true);
            }
        }
    });

    let select: Rc<dyn Fn(Date)> = Rc::new({
        let slot = select_slot.clone();
        let show_banner = show_banner.clone();
        move |date| {
            show_banner(date);
            let update = slot.borrow().as_ref().cloned();
            if let Some(update) = update {
                update(date);
            }
        }
    });

    banner_button.connect_clicked({
        let state = calendar_state.clone();
        let select = select.clone();
        move |_| {
            let today = today();
            state.go_to(today);
            select(today);
        }
    });

    let rendered = Rc::new(Cell::new(false));
    let render: Rc<dyn Fn()> = Rc::new({
        let body = body.clone();
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let scroller = scroller.clone();
        let calendar_state = calendar_state.clone();
        let select_slot = select_slot.clone();
        let columns_slot = columns_slot.clone();
        let clamp = clamp.clone();
        let overlay = overlay.clone();
        move || {
            let position = scroller.vadjustment().value();
            fill_planting_body(
                &body,
                &store,
                &nav,
                &root_page,
                id,
                &calendar_state,
                &timeline_zoom,
                &show_banner,
                &select_slot,
                &columns_slot,
                &clamp,
                &overlay,
            );

            let scroller = scroller.clone();
            gtk::glib::idle_add_local_once(move || {
                let adjustment = scroller.vadjustment();
                let maximum = (adjustment.upper() - adjustment.page_size()).max(0.0);
                adjustment.set_value(position.min(maximum));
            });
        }
    });
    render();
    rendered.set(true);
    page.connect_showing({
        let render = render.clone();
        let rendered = rendered.clone();
        move |_| {
            if rendered.replace(false) {
                return;
            }
            render();
        }
    });

    // A background sync must reach a page that is already open. Rebuilding the body keeps the
    // selected day (the calendar state lives outside it) and the scroll position.
    let weather_listener: Rc<dyn Fn()> = Rc::new({
        let render = render.clone();
        let page = page.downgrade();
        move || {
            if let Some(page) = page.upgrade()
                && page.is_mapped()
            {
                render();
            }
        }
    });
    super::weather_watch::register(weather_listener.clone());
    page.connect_destroy(move |_| {
        let _ = &weather_listener;
    });

    edit.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        move |_| {
            nav.push(&setup_page(
                &store,
                &nav,
                &root_page,
                summary.planting.profile_id,
                Some(id),
            ));
        }
    });

    let menu = gtk::gio::Menu::new();
    if summary.planting.state(today()) == PlantingState::Finished {
        menu.append(Some("Reopen planting"), Some("planting.reopen"));
        menu.append(Some("Repeat planting"), Some("planting.repeat"));
    } else {
        menu.append(Some("Finish planting"), Some("planting.finish"));
    }
    menu.append(Some("Delete planting"), Some("planting.delete"));

    let actions = gtk::gio::SimpleActionGroup::new();

    let finish_action = gtk::gio::SimpleAction::new("finish", None);
    finish_action.connect_activate({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        move |_, _| {
            nav.push(&finish_page(&store, &nav, &root_page, id));
        }
    });
    actions.add_action(&finish_action);

    let reopen_action = gtk::gio::SimpleAction::new("reopen", None);
    reopen_action.connect_activate({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let overlay = overlay.clone();
        move |_, _| match store.reopen_planting(id) {
            Ok(()) => replace_with_view(&store, &nav, &root_page, id),
            Err(error) => {
                eprintln!("could not reopen planting {id}: {error}");
                overlay.add_toast(adw::Toast::new("Could not reopen the planting"));
            }
        }
    });
    actions.add_action(&reopen_action);

    let repeat_action = gtk::gio::SimpleAction::new("repeat", None);
    repeat_action.connect_activate({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let overlay = overlay.clone();
        move |_, _| match store.repeat_planting(id, today()) {
            Ok(new_id) => match planting_view(&store, &nav, &root_page, new_id) {
                Some(view) => nav.push(&view),
                None => {
                    overlay.add_toast(adw::Toast::new(
                        "Repeated, but the new planting could not be opened",
                    ));
                }
            },
            Err(error) => {
                eprintln!("could not repeat planting {id}: {error}");
                overlay.add_toast(adw::Toast::new("Could not repeat the planting"));
            }
        }
    });
    actions.add_action(&repeat_action);

    let delete_action = gtk::gio::SimpleAction::new("delete", None);
    delete_action.connect_activate({
        let store = store.clone();
        let nav = nav.clone();
        let overlay = overlay.clone();
        let title = summary.title().to_string();

        move |_, _| {
            let nav = nav.clone();
            let store = store.clone();
            let overlay_c = overlay.clone();

            super::shared::form::confirm_delete(
                &overlay,
                &format!("Delete “{title}”?"),
                "The planting, its diary and photos are removed for good. This cannot be undone.",
                Rc::new(move || {
                    let stems = match store.delete_planting(id) {
                        Ok(stems) => stems,
                        Err(error) => {
                            eprintln!("could not delete planting {id}: {error}");
                            overlay_c.add_toast(adw::Toast::new("Could not delete the planting"));
                            return;
                        }
                    };

                    for stem in stems {
                        media::remove(&stem);
                    }

                    nav.pop();
                }),
            );
        }
    });
    actions.add_action(&delete_action);

    more.insert_action_group("planting", Some(&actions));
    more.set_menu_model(Some(&menu));

    Some(page)
}

pub(super) fn replace_with_view(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    id: i64,
) {
    let Some(view) = planting_view(store, nav, root_page, id) else {
        nav.pop();
        return;
    };

    // Keep the pages below the planting page: a planting opened from a space returns to it. The
    // tag finds the stale page whether the save came from its own form or from Finish/Reopen.
    let tag = format!("{PAGE_TAG}{id}");
    let pages = nav.navigation_stack();
    let mut stack: Vec<adw::NavigationPage> = Vec::new();
    for position in 0..pages.n_items() {
        if let Some(page) = pages
            .item(position)
            .and_then(|item| item.downcast::<adw::NavigationPage>().ok())
        {
            stack.push(page);
        }
    }

    let target = (0..stack.len())
        .rev()
        .find(|index| stack[*index].tag().as_deref() == Some(tag.as_str()));

    match target {
        Some(index) => {
            stack.truncate(index);
            stack.push(view);
            nav.replace(&stack);
        }
        None => {
            nav.replace(&[root_page.clone(), view]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_planting_body(
    body: &gtk::Box,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    id: i64,
    calendar_state: &Rc<planting_schedule::CalendarState>,
    timeline_zoom: &Rc<Cell<planting_schedule::TimelineZoom>>,
    show_banner: &Rc<dyn Fn(Date)>,
    select_slot: &SelectSlot,
    columns_slot: &Rc<RefCell<Option<Columns>>>,
    clamp: &adw::Clamp,
    overlay: &adw::ToastOverlay,
) {
    // A rebuild replaces the update handle; clear it first so an early return cannot leave a
    // handler pointing at widgets that are about to be detached.
    *select_slot.borrow_mut() = None;

    while let Some(child) = body.first_child() {
        body.remove(&child);
    }

    let Some(summary) = store.planting(id).ok().flatten() else {
        return;
    };
    let today = today();
    let planting = &summary.planting;
    let space = store.space(planting.space_id).ok().flatten();
    let units = UnitPreference::load(store);

    let columns = Columns::new(clamp);

    let hero = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    top.append(&avatar::editor(store, "planting", planting.id, 96, overlay));

    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text.set_valign(gtk::Align::Center);

    let name = gtk::Label::new(Some(summary.title()));
    name.set_xalign(0.0);
    name.set_wrap(true);
    name.add_css_class("omagrow-profile-name");

    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    chips.append(&state_chip(&summary, today));

    let day = gtk::Label::new(Some(&status_detail(&summary, today)));
    day.add_css_class("omagrow-chip");
    chips.append(&day);

    text.append(&name);
    text.append(&chips);

    if summary.title() != summary.profile_name {
        let profile = gtk::Label::new(Some(&summary.profile_name));
        profile.set_xalign(0.0);
        profile.add_css_class("dim-label");
        text.append(&profile);
    }

    if let Some(subtitle) = species_subtitle(
        summary.profile_species.as_deref(),
        summary.profile_cultivar.as_deref(),
    ) {
        let label = gtk::Label::new(Some(&subtitle));
        label.set_xalign(0.0);
        label.add_css_class("dim-label");
        text.append(&label);
    }

    top.append(&text);
    hero.append(&top);
    hero.set_margin_bottom(18);
    columns.push(
        &setup_tiles(store, nav, root_page, &summary, space.as_ref(), units),
        Slot::Side,
    );

    let stages = store.stages(planting.profile_id).unwrap_or_default();
    let rules = store.timing_rules(planting.profile_id).unwrap_or_default();
    let stage_starts = store.stage_starts(planting.id).unwrap_or_default();
    let frost = FrostDates::load(store).unwrap_or_default();
    let lifecycle = store
        .profile_details(planting.profile_id)
        .ok()
        .flatten()
        .and_then(|profile| profile.lifecycle);
    let schedule = schedule::build(
        planting,
        &stages,
        &rules,
        &stage_starts,
        &frost,
        today,
        lifecycle.as_deref(),
    );
    let observations = store.observations(planting.id).unwrap_or_default();
    let diary_events = planting_diary::calendar_events(&observations, &stages, units, &schedule);

    // The timeline owns the stage start: it opens a small form for the selected day, and the page
    // rebuilds when it returns, so the blocks follow at once.
    let on_stage_start: Rc<dyn Fn(i64, Date)> = Rc::new({
        let store = store.clone();
        let nav = nav.clone();
        let stages = stages.clone();
        move |stage_id, day| {
            nav.push(&super::planting_stage::page(
                &store, &nav, id, &stages, stage_id, day,
            ));
        }
    });

    let timeline = planting_schedule::timeline(
        &schedule,
        planting,
        today,
        &diary_events,
        timeline_zoom,
        on_stage_start,
    );

    // The cards that follow the selected day keep their own containers, so a calendar click
    // replaces only their content and the page keeps its scroll position.
    let diary_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    // Weather follows the placement of the selected day, not the space the plant is in now.
    let placements = store.planting_placements(planting.id).unwrap_or_default();
    let weather_slot = placements
        .iter()
        .any(|placement| placement.space_kind == "outdoor")
        .then(|| gtk::Box::new(gtk::Orientation::Vertical, 0));

    let selected = super::climate::selected_day(calendar_state.selected.get());
    let climate =
        super::climate::planting_section(store, planting, units, selected.clone(), &schedule);

    let planting_id = planting.id;
    let update: Rc<dyn Fn(Date)> = Rc::new({
        let store = store.clone();
        let nav = nav.clone();
        let summary = summary.clone();
        let observations = observations.clone();
        let stages = stages.clone();
        let placements = placements.clone();
        let show_banner = show_banner.clone();
        let timeline_update = timeline.update.clone();
        let diary_slot = diary_slot.clone();
        let weather_slot = weather_slot.clone();
        let selected = selected.clone();
        let climate = climate.as_ref().map(|section| section.redraw.clone());

        move |date| {
            show_banner(date);
            selected.set(Some(date));
            timeline_update(date);

            let card =
                planting_diary::card(&store, &nav, &summary, &observations, &stages, units, date);
            replace_children(&diary_slot, Some(&card));

            if let Some(slot) = &weather_slot {
                let placement = placements.iter().find(|placement| placement.covers(date));
                let card = match placement.filter(|placement| placement.space_kind == "outdoor") {
                    Some(placement) => {
                        let day = store
                            .planting_weather_days(planting_id)
                            .unwrap_or_default()
                            .into_iter()
                            .find(|day| day.date == date);
                        let readings: Vec<_> = store
                            .planting_climate_readings(planting_id)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|reading| reading.measured_on == date)
                            .collect();
                        store
                            .space(placement.space_id)
                            .ok()
                            .flatten()
                            .and_then(|space| {
                                super::weather_view::day_card(
                                    &store,
                                    &space,
                                    units,
                                    date,
                                    day.as_ref(),
                                    &readings,
                                )
                            })
                    }
                    None => None,
                };
                replace_children(slot, card.as_ref());
                slot.queue_resize();
            }

            if let Some(redraw) = &climate {
                redraw();
            }
        }
    });

    // The calendar updates its own grid on a click; the page listens through `update`.
    let calendar = planting_schedule::calendar(
        &schedule,
        planting,
        today,
        &diary_events,
        calendar_state,
        update.clone(),
    );

    columns.push(&timeline.widget, Slot::Main);
    columns.push(&calendar.widget, Slot::Main);
    if let Some(climate) = &climate {
        columns.push(&climate.widget, Slot::Full);
    }
    if let Some(slot) = &weather_slot {
        columns.push(slot, Slot::Side);
    }
    // The diary and notes fill the rail under the weather card on a wide page.
    columns.push(&diary_slot, Slot::Side);

    if !schedule.undated_stages.is_empty() || (schedule.stages.is_empty() && !stages.is_empty()) {
        let cycle = adw::PreferencesGroup::new();
        cycle.set_title("Growing cycle");
        cycle.set_description(Some(
            "The full cycle in the profile. The timeline only places the stages that can be \
             dated for this planting.",
        ));

        let row = adw::ActionRow::new();
        row.set_title("Stages");
        row.set_subtitle(&cycle_text(&stages));
        row.set_subtitle_lines(0);
        cycle.add(&row);

        columns.push(&cycle, Slot::Main);
    }

    if let Some(notes) = planting
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|notes| !notes.is_empty())
    {
        let notes_group = adw::PreferencesGroup::new();
        notes_group.set_title("Notes");

        let row = adw::ActionRow::new();
        row.set_title("Notes");
        row.set_subtitle(notes);
        row.set_subtitle_lines(0);
        notes_group.add(&row);

        columns.push(&notes_group, Slot::Side);
    }

    columns.finish(super::shared::layout::window_width(nav));
    body.append(&hero);
    body.append(columns.widget());
    *columns_slot.borrow_mut() = Some(columns);

    // "Back to today" moves the month, so it needs the calendar grid rebuilt as well as the cards.
    let apply: Rc<dyn Fn(Date)> = Rc::new({
        let refresh = calendar.refresh.clone();
        let update = update.clone();
        move |date| {
            refresh();
            update(date);
        }
    });

    *select_slot.borrow_mut() = Some(apply);
    update(calendar_state.selected.get());
}

/// Swap the contents of a card container without touching the widgets around it.
fn replace_children(container: &gtk::Box, child: Option<&gtk::Widget>) {
    while let Some(existing) = container.first_child() {
        container.remove(&existing);
    }
    if let Some(child) = child {
        container.append(child);
    }
}

fn setup_tiles(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    summary: &PlantingSummary,
    space: Option<&Space>,
    units: UnitPreference,
) -> gtk::Widget {
    let planting = &summary.planting;

    let grid = gtk::Grid::new();
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(true);

    let mut tiles: Vec<gtk::Widget> = Vec::new();

    if let Some(space) = space {
        let chevron = gtk::Image::from_icon_name("go-next-symbolic");
        chevron.add_css_class("dim-label");
        let name = space.display_name().unwrap_or("This planting only");
        let contents = tile_contents("Space", name, Some(chevron.upcast_ref()));

        let tile = gtk::Button::new();
        tile.add_css_class("flat");
        tile.add_css_class("omagrow-tile");
        tile.set_child(Some(&contents));
        tile.set_hexpand(true);
        tile.set_halign(gtk::Align::Fill);
        tile.set_valign(gtk::Align::Fill);
        tile.set_tooltip_text(Some(&space_summary(Some(space))));
        tile.connect_clicked({
            let store = store.clone();
            let nav = nav.clone();
            let root_page = root_page.clone();
            let space_id = space.id;
            let planting_id = planting.id;
            move |_| {
                nav.push(&space_page(&store, &nav, &root_page, space_id, planting_id));
            }
        });
        tiles.push(tile.upcast());
    } else {
        tiles.push(tile("Space", &space_summary(None)).upcast());
    }

    tiles.push(tile("Plants", &quantity_text(planting.quantity)).upcast());
    tiles.push(tile("Container", &container_text(planting, units)).upcast());
    tiles.push(
        tile(
            "Started",
            &format!(
                "{} · {}",
                short_date(planting.started_on),
                start_kind_text(&planting.start_kind)
            ),
        )
        .upcast(),
    );

    if let Some(ended_on) = planting.ended_on {
        let outcome = planting
            .outcome
            .as_deref()
            .map(outcome_label)
            .unwrap_or("Finished");
        tiles.push(tile("Finished", &format!("{} · {outcome}", short_date(ended_on))).upcast());
    }

    if let Some(substrate) = planting
        .substrate
        .as_deref()
        .map(str::trim)
        .filter(|substrate| !substrate.is_empty())
    {
        tiles.push(tile("Substrate", substrate).upcast());
    }

    for (index, tile) in tiles.iter().enumerate() {
        let last_odd = tiles.len() % 2 == 1 && index + 1 == tiles.len();
        let width = if last_odd { 2 } else { 1 };
        grid.attach(tile, (index % 2) as i32, (index / 2) as i32, width, 1);
    }

    grid.upcast()
}

fn tile(caption: &str, value: &str) -> gtk::Box {
    let tile = tile_contents(caption, value, None);
    tile.add_css_class("omagrow-tile");
    tile
}

fn tile_contents(caption: &str, value: &str, suffix: Option<&gtk::Widget>) -> gtk::Box {
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 2);
    tile.set_hexpand(true);
    tile.set_halign(gtk::Align::Fill);

    let caption_label = gtk::Label::new(Some(caption));
    caption_label.set_xalign(0.0);
    caption_label.add_css_class("omagrow-tile-caption");
    tile.append(&caption_label);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_hexpand(true);
    row.set_halign(gtk::Align::Fill);
    let value_label = gtk::Label::new(Some(value));
    value_label.set_xalign(0.0);
    value_label.set_wrap(true);
    value_label.set_max_width_chars(20);
    value_label.set_hexpand(true);
    value_label.add_css_class("omagrow-tile-value");
    row.append(&value_label);

    if let Some(suffix) = suffix {
        row.append(suffix);
    }

    tile.append(&row);
    tile
}

fn space_summary(space: Option<&Space>) -> String {
    let Some(space) = space else {
        return "This planting only".to_string();
    };

    let kind = space_kind_text(&space.kind);
    let light = match (space.light_type.as_deref(), space.light_hours) {
        (Some(light), Some(hours)) => {
            let start = light_start_text(space.light_start);
            if start.is_empty() {
                format!(
                    "{} · {} h/day",
                    light_type_text(light),
                    format_number(hours)
                )
            } else {
                format!(
                    "{} · {} h/day · on {start}",
                    light_type_text(light),
                    format_number(hours)
                )
            }
        }
        (Some(light), None) => light_type_text(light).to_string(),
        (None, Some(hours)) => format!("{} h/day", format_number(hours)),
        (None, None) => String::new(),
    };

    match space.display_name() {
        Some(name) if light.is_empty() => format!("{name} · {kind}"),
        Some(name) => format!("{name} · {kind} · {light}"),
        None if light.is_empty() => format!("This planting only · {kind}"),
        None => format!("This planting only · {kind} · {light}"),
    }
}

fn container_text(planting: &Planting, units: UnitPreference) -> String {
    let label = match planting.container.as_str() {
        "ground" => "Ground",
        "pot" => "Pot",
        "bed" => "Raised bed",
        "hydroponics" => "Hydroponics",
        other => other,
    };

    match (planting.container.as_str(), planting.container_volume_l) {
        ("pot", Some(volume)) => format!(
            "{label} · {} {}",
            units.number("L", volume),
            units.label("L")
        ),
        _ => label.to_string(),
    }
}

fn cycle_text(stages: &[Stage]) -> String {
    stages
        .iter()
        .map(
            |stage| match (stage.duration_days_min, stage.duration_days_max) {
                (Some(min), Some(max)) if min != max => format!(
                    "{} ({}–{} d)",
                    stage.label,
                    format_number(min),
                    format_number(max)
                ),
                (Some(days), _) | (_, Some(days)) => {
                    format!("{} ({} d)", stage.label, format_number(days))
                }
                _ => stage.label.clone(),
            },
        )
        .collect::<Vec<_>>()
        .join(" → ")
}

fn finish_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    id: i64,
) -> adw::NavigationPage {
    let Some(summary) = store.planting(id).ok().flatten() else {
        return missing_planting_page(id);
    };

    let guard = Guard::new(nav);
    let header = guard.header("Finish planting");
    let save = gtk::Button::with_label("Finish");
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let group = adw::PreferencesGroup::new();
    group.set_title("Finish");
    group.set_description(Some(
        "The planting stays in your history. Nothing is deleted, so you can look back at it later.",
    ));

    let now = today();
    let last_day = if summary.planting.started_on > now {
        summary.planting.started_on
    } else {
        now
    };
    let (date_row, date_value) = date_row("Last day", last_day);
    let outcome_row = combo_row("Outcome", &["Harvested", "Removed", "Died", "Failed"], 0);

    group.add(&date_row);
    group.add(&outcome_row);

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
        let root_page = root_page.clone();
        let overlay = overlay.clone();
        let date_value = date_value.clone();
        let outcome_row = outcome_row.clone();
        move |_| {
            let outcome = OUTCOMES
                .get(outcome_row.selected() as usize)
                .copied()
                .unwrap_or("harvested");

            if date_value.get() > today() {
                overlay.add_toast(adw::Toast::new("The last day cannot be in the future"));
                return;
            }

            match store.finish_planting(id, date_value.get(), outcome) {
                Ok(()) => replace_with_view(&store, &nav, &root_page, id),
                Err(error) => {
                    eprintln!("could not finish planting {id}: {error}");
                    overlay.add_toast(adw::Toast::new(&error.to_string()));
                }
            }
        }
    });

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    let page = adw::NavigationPage::new(&toolbar, "Finish planting");
    page.set_tag(Some(&format!("{PAGE_TAG}finish-{id}")));
    guard.attach(&page, &toolbar);
    page
}

fn space_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    space_id: i64,
    planting_id: i64,
) -> adw::NavigationPage {
    let store_for_save = store.clone();
    let nav_for_save = nav.clone();
    let root_for_save = root_page.clone();
    let on_saved: Rc<dyn Fn()> = Rc::new(move || {
        replace_with_view(&store_for_save, &nav_for_save, &root_for_save, planting_id)
    });

    space_editor(store, nav, space_id, on_saved)
}

/// The shared space form. Saving runs `on_saved`, because the page is reached from a planting and
/// from the Spaces area with a different place to return to.
fn missing_planting_page(id: i64) -> adw::NavigationPage {
    super::shared::form::missing_page(
        "Planting",
        "Planting",
        "This planting no longer exists",
        Some(&format!("Planting {id} could not be opened.")),
    )
}
