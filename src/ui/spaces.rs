//! The Spaces area: the named places, the shared setup they own, and the plantings inside them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::calendar::Date;
use crate::clock::today;
use crate::planting::{PlantingState, PlantingSummary, Space};
use crate::store::Store;
use crate::units::UnitPreference;

use super::page_header;
use super::planting_setup::space_editor;
use super::planting_view::{is_planting_page, planting_view};
use super::plantings::list_row;
use super::shared::avatar;
use super::shared::fields::light_start_text;
use super::shared::layout::{Columns, Slot};
use super::shell::Section;
use super::space_readings;
use super::{PLANTINGS, SPACES};

pub(super) fn section(store: &Rc<Store>, highlight: Rc<dyn Fn(&str)>) -> Section {
    let nav = adw::NavigationView::new();
    nav.set_pop_on_escape(true);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_valign(gtk::Align::Start);
    content.set_margin_top(24);
    content.set_margin_bottom(32);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&list);

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
    status.set_icon_name(Some("omagrow-space-symbolic"));
    status.set_title("No spaces yet");
    status.set_description(Some(
        "A space is a place where plantings share the light and the air. Name one while you set up \
         a planting, and it shows up here.",
    ));

    let pages = gtk::Stack::new();
    pages.add_named(&status, Some("empty"));
    pages.add_named(&scroller, Some("list"));

    let header = page_header("Spaces");

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&pages));

    let root_page = adw::NavigationPage::new(&toolbar, "Spaces");
    nav.push(&root_page);

    let open_space: Rc<dyn Fn(i64)> = {
        let nav = nav.clone();
        let store = store.clone();
        let root_page = root_page.clone();
        Rc::new(move |id| {
            nav.push(&space_page(&store, &nav, &root_page, id));
        })
    };

    let refresh: Rc<dyn Fn()> = {
        let list = list.clone();
        let pages = pages.clone();
        let status = status.clone();
        let store = store.clone();
        let open_space = open_space.clone();
        // Keep the columns (and their width watcher) alive with the list.
        let columns = columns;
        Rc::new(move || {
            let _keep_columns = &columns;
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }

            match store.spaces() {
                Ok(spaces) if spaces.is_empty() => {
                    status.set_title("No spaces yet");
                    status.set_description(Some(
                        "A space is a place where plantings share the light and the air. Name one \
                         while you set up a planting, and it shows up here.",
                    ));
                    pages.set_visible_child_name("empty");
                }
                Ok(spaces) => {
                    let today = today();
                    let summaries = store.plantings().unwrap_or_default();

                    for space in spaces {
                        list.append(&space_row(&store, &space, &summaries, &open_space, today));
                    }

                    pages.set_visible_child_name("list");
                }
                Err(error) => {
                    eprintln!("could not load spaces: {error}");
                    status.set_title("Could not load spaces");
                    status.set_description(Some(&error.to_string()));
                    pages.set_visible_child_name("empty");
                }
            }
        })
    };

    nav.connect_popped({
        let refresh = refresh.clone();
        move |_, _| refresh()
    });

    // A planting opened from a space is still a planting: the sidebar follows the page, not the
    // area it was reached through. Pages pushed from the planting (edit, diary) keep it, and going
    // back to the space restores Spaces.
    nav.connect_visible_page_notify({
        let highlight = highlight.clone();
        move |nav| {
            let stack = nav.navigation_stack();
            let planting = (0..stack.n_items())
                .filter_map(|position| stack.item(position))
                .filter_map(|item| item.downcast::<adw::NavigationPage>().ok())
                .any(|page| is_planting_page(&page));

            highlight(if planting { PLANTINGS } else { SPACES });
        }
    });

    refresh();

    Section::new(nav, root_page, refresh)
}

fn space_row(
    store: &Store,
    space: &Space,
    summaries: &[PlantingSummary],
    open: &Rc<dyn Fn(i64)>,
    today: crate::calendar::Date,
) -> gtk::ListBoxRow {
    let row = adw::ActionRow::builder()
        .title(space.display_name().unwrap_or("Space"))
        .subtitle(space_summary(space, summaries, today))
        .activatable(true)
        .build();
    row.add_prefix(&avatar::widget(store, "space", space.id, 40));

    let id = space.id;
    let open = open.clone();
    row.connect_activated(move |_| open(id));

    let arrow = gtk::Image::from_icon_name("go-next-symbolic");
    arrow.add_css_class("dim-label");
    row.add_suffix(&arrow);

    row.upcast()
}

/// "Indoor · LED · 16 h/day · 2 plantings"
fn space_summary(space: &Space, summaries: &[PlantingSummary], today: Date) -> String {
    let mut parts = vec![kind_label(&space.kind).to_string()];

    if let Some(light) = light_summary(space) {
        parts.push(light);
    }

    let plantings = summaries
        .iter()
        .filter(|summary| summary.planting.space_id == space.id);
    let mut active = 0;
    let mut total = 0;
    for summary in plantings {
        total += 1;
        if summary.planting.state(today) != PlantingState::Finished {
            active += 1;
        }
    }

    parts.push(count_text(active, total));
    parts.join(" · ")
}

fn count_text(active: usize, total: usize) -> String {
    let noun = |count: usize| {
        if count == 1 {
            "planting".to_string()
        } else {
            "plantings".to_string()
        }
    };

    if active == total {
        format!("{total} {}", noun(total))
    } else {
        format!("{active} active of {total} {}", noun(total))
    }
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "greenhouse" => "Greenhouse",
        "indoor" => "Indoor",
        _ => "Outdoor",
    }
}

fn light_label(light: &str) -> Option<&'static str> {
    match light {
        "natural" => Some("Natural light"),
        "led" => Some("LED"),
        "hps" => Some("HPS"),
        "mixed" => Some("Mixed light"),
        _ => None,
    }
}

fn light_summary(space: &Space) -> Option<String> {
    if !space.has_light() {
        return None;
    }

    let light = space.light_type.as_deref().and_then(light_label);
    let hours = space
        .light_hours
        .map(|hours| format!("{} h/day", crate::plant_data::format_number(hours)));
    let start = (!light_start_text(space.light_start).is_empty())
        .then(|| format!("on {}", light_start_text(space.light_start)));

    let mut parts = Vec::new();
    parts.extend(light.map(str::to_string));
    parts.extend(hours);
    parts.extend(start);
    if parts.is_empty() {
        Some("Light not set".to_string())
    } else {
        Some(parts.join(" · "))
    }
}

fn space_page(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    space_id: i64,
) -> adw::NavigationPage {
    let Some(space) = store.space(space_id).ok().flatten() else {
        return missing_page();
    };

    let overlay = adw::ToastOverlay::new();
    let title = adw::WindowTitle::new(
        space.display_name().unwrap_or("Space"),
        "Shared light and air",
    );

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));

    let edit = gtk::Button::with_label("Edit");
    edit.add_css_class("flat");
    header.pack_end(&edit);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.set_valign(gtk::Align::Start);
    // The body keeps its columns alive while the page is on screen; the width watcher holds a
    // weak handle to them.
    let columns_slot: Rc<RefCell<Option<Columns>>> = Rc::new(RefCell::new(None));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
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
    toolbar.set_content(Some(&scroller));
    overlay.set_child(Some(&toolbar));

    let page = adw::NavigationPage::new(&overlay, "Space");

    let render: Rc<dyn Fn()> = Rc::new({
        let body = body.clone();
        let title = title.clone();
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let columns_slot = columns_slot.clone();
        let clamp = clamp.clone();
        let overlay = overlay.clone();
        move || {
            let Some(space) = store.space(space_id).ok().flatten() else {
                return;
            };

            title.set_title(space.display_name().unwrap_or("Space"));

            while let Some(child) = body.first_child() {
                body.remove(&child);
            }

            let columns = Columns::new(&clamp);
            columns.push(&setup_with_avatar(&store, &space, &overlay), Slot::Main);
            columns.push(
                &plantings_section(&store, &nav, &root_page, &space, today()),
                Slot::Main,
            );
            if let Some(weather) =
                super::weather_view::section(&store, &space, UnitPreference::load(&store))
            {
                columns.push(&weather, Slot::Side);
            }
            // The rail holds every source the graphs draw from: the weather model, the local
            // readings and, later, the sensors. The card shows the newest few; the page has all.
            columns.push(
                &space_readings::card(&store, &nav, &space, UnitPreference::load(&store)),
                Slot::Side,
            );
            // Every space has a rail, even indoors: this placeholder keeps the same two-column
            // layout and marks where sensors will join the readings and the graphs.
            columns.push(&sensors_section(), Slot::Side);
            if let Some(graph) =
                super::climate::space_section(&store, &space, UnitPreference::load(&store))
            {
                columns.push(&graph.widget, Slot::Full);
            }

            columns.finish(super::shared::layout::window_width(&nav));
            body.append(columns.widget());
            *columns_slot.borrow_mut() = Some(columns);
        }
    });

    render();

    let rendered = Rc::new(Cell::new(true));
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

    // A sync that lands while this page is open reloads the weather and the graph in place.
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
        move |_| {
            let close = {
                let nav = nav.clone();
                Rc::new(move || {
                    nav.pop();
                })
            };
            nav.push(&space_editor(&store, &nav, space_id, close));
        }
    });

    page
}

/// The space's avatar sits above the shared setup, not in the top bar and not beside the text.
fn setup_with_avatar(store: &Rc<Store>, space: &Space, overlay: &adw::ToastOverlay) -> gtk::Widget {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);
    column.set_valign(gtk::Align::Start);

    let avatar = avatar::editor(store, "space", space.id, 96, overlay);
    avatar.set_halign(gtk::Align::Start);
    column.append(&avatar);
    column.append(&setup_group(space));

    column.upcast()
}

fn setup_group(space: &Space) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Shared setup");
    group.set_description(Some(
        "Everything in this space shares this light and air. Changing it here changes it for \
         every planting inside.",
    ));

    group.add(&read_only_row("Type", kind_label(&space.kind)));

    if space.has_light() {
        let light = light_summary(space).unwrap_or_else(|| "Not set".to_string());
        group.add(&read_only_row("Light", &light));
    }

    if let Some(notes) = space
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|notes| !notes.is_empty())
    {
        group.add(&note_row(notes));
    }

    group
}

/// The rail needs a card on every space, indoors too. Sensors are the next source of climate data;
/// until they exist this card explains where they will land and keeps the wide layout stable.
fn sensors_section() -> gtk::Widget {
    let group = adw::PreferencesGroup::new();
    group.set_title("Sensors");
    group.set_description(Some(
        "A sensor joins this space's readings and graphs later.",
    ));

    let row = adw::ActionRow::builder()
        .title("No sensor connected")
        .build();
    row.set_activatable(false);
    row.set_focusable(false);
    group.add(&row);

    group.upcast()
}

fn note_row(notes: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title("Notes")
        .subtitle(notes)
        .build();
    row.set_activatable(false);
    row.set_selectable(false);
    row.set_focusable(false);
    row
}

fn read_only_row(title: &str, value: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.set_activatable(false);
    row.set_selectable(false);
    row.set_focusable(false);

    let label = gtk::Label::new(Some(value));
    label.set_wrap(true);
    label.set_xalign(1.0);
    label.set_max_width_chars(36);
    label.add_css_class("dim-label");
    row.add_suffix(&label);

    row
}

fn plantings_section(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    space: &Space,
    today: Date,
) -> gtk::Widget {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 10);

    let mut summaries: Vec<PlantingSummary> = store
        .plantings()
        .unwrap_or_default()
        .into_iter()
        .filter(|summary| summary.planting.space_id == space.id)
        .collect();

    summaries.sort_by_key(|summary| {
        (
            summary.planting.state(today) == PlantingState::Finished,
            summary.title().to_lowercase(),
        )
    });

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some("Plantings"));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.add_css_class("omagrow-section-title");

    let count = gtk::Label::new(Some(&summaries.len().to_string()));
    count.add_css_class("omagrow-axis");

    head.append(&label);
    head.append(&count);
    section.append(&head);

    if summaries.is_empty() {
        let empty = gtk::Label::new(Some(
            "No plantings in this space yet. Add one from Plantings and pick this space.",
        ));
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        empty.add_css_class("dim-label");
        section.append(&empty);
        return section.upcast();
    }

    let open: Rc<dyn Fn(i64)> = {
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        Rc::new(move |id| {
            if let Some(page) = planting_view(&store, &nav, &root_page, id) {
                nav.push(&page);
            }
        })
    };

    for summary in &summaries {
        section.append(&list_row(store, summary, open.clone(), today, false));
    }

    section.upcast()
}

fn missing_page() -> adw::NavigationPage {
    super::shared::form::missing_page(
        "Space",
        "Space",
        "This space no longer exists",
        Some("It may have been removed together with its last planting."),
    )
}
