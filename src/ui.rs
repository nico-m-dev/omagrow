mod climate;
mod help;
mod planting_diary;
mod planting_schedule;
mod planting_setup;
mod planting_stage;
mod planting_view;
mod plantings;
mod profile_agent;
mod profile_editors;
mod profile_history;
mod profile_overview;
mod settings;
mod shared;
mod shell;
mod space_readings;
mod spaces;
mod weather_view;
mod weather_watch;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::pango;
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::completeness;
use crate::media::{self, Media};
use crate::online;
use crate::proposal;
use crate::store::{Profile, Store};
use crate::theme;
use crate::weather;
use profile_agent::proposals_card;
use profile_editors::profile_edit;
use profile_history::recent_changes_card;
use profile_overview::{growing_data, stat_tiles, temperature_bands};
use shared::avatar;
use shared::layout::{Columns, Slot};

const PROFILES: &str = "profiles";
const PLANTINGS: &str = "plantings";
const SPACES: &str = "spaces";
const SETTINGS: &str = "settings";
const HELP: &str = "help";

use shell::{Section, confirm_discard};

type NavSelect = Rc<dyn Fn(&str)>;
type OpenArea = Rc<dyn Fn()>;

pub fn build(app: &adw::Application) {
    theme::install();

    // Agents drop proposal files here; an empty folder is friendlier than a missing one.
    if let Some(inbox) = proposal::inbox()
        && let Err(error) = std::fs::create_dir_all(&inbox)
    {
        eprintln!("could not create {}: {error}", inbox.display());
    }

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("OmaGrow")
        .default_width(1040)
        .default_height(640)
        .build();
    // The window may not be drawn narrower than this. The planting page needs about 502 px: the
    // calendar header carries "Calendar", both arrows, the month and "Today" on one line, and the
    // month label cannot shrink below its full text. 520 keeps a little room for another theme or
    // font, so the declared minimum stays honest and GTK has nothing to warn about.
    window.set_size_request(520, 420);

    let store = match Store::open_default() {
        Ok(store) => Rc::new(store),
        Err(error) => {
            window.set_content(Some(&error_page(&error.to_string())));
            window.present();
            return;
        }
    };

    start_weather(&store);

    let sections = gtk::Stack::new();
    sections.set_hexpand(true);
    sections.set_vexpand(true);

    // The grow comes first: a user starts with what is planted, not with the profiles behind it.
    let plantings_list = nav_list();
    let (plantings_row, plantings_label) =
        nav_row("Plantings", &nav_icon("omagrow-planting-symbolic"));
    plantings_list.append(&plantings_row);

    let spaces_list = nav_list();
    let (spaces_row, spaces_label) = nav_row("Spaces", &nav_icon("omagrow-space-symbolic"));
    spaces_list.append(&spaces_row);

    let profiles_list = nav_list();
    let profiles_icon = plant_icon(20);
    profiles_icon.add_css_class("omagrow-nav-icon");
    let (profiles_row, profiles_label) = nav_row("Plant Profiles", &profiles_icon);
    profiles_list.append(&profiles_row);

    let settings_list = nav_list();
    let settings_icon = gtk::Image::from_icon_name("preferences-system-symbolic");
    settings_icon.set_pixel_size(20);
    let (settings_row, settings_label) = nav_row("Settings", &settings_icon);
    settings_list.append(&settings_row);

    let help_list = nav_list();
    let help_icon = gtk::Image::from_icon_name("help-about-symbolic");
    help_icon.set_pixel_size(20);
    let (help_row, help_label) = nav_row("Help", &help_icon);
    help_list.append(&help_row);

    // The plantings section offers a way to the profile list, but that action needs the sidebar
    // selector, which is built after the sections. The slot breaks the cycle.
    let open_profiles_slot: Rc<RefCell<Option<OpenArea>>> = Rc::new(RefCell::new(None));
    let open_profiles: OpenArea = {
        let slot = open_profiles_slot.clone();
        Rc::new(move || {
            if let Some(open) = slot.borrow().as_ref() {
                open();
            }
        })
    };

    // The sidebar highlight follows the page the user is looking at. It is separate from the
    // active area (`current`): a planting opened from a space highlights Plantings, while leaving
    // still closes the pages that were pushed inside Spaces.
    let highlight: Rc<dyn Fn(&str)> = {
        let rows = [
            (PROFILES, profiles_row.clone()),
            (PLANTINGS, plantings_row.clone()),
            (SPACES, spaces_row.clone()),
            (HELP, help_row.clone()),
            (SETTINGS, settings_row.clone()),
        ];
        Rc::new(move |area| {
            for (name, row) in &rows {
                if *name == area {
                    row.add_css_class("omagrow-active");
                } else {
                    row.remove_css_class("omagrow-active");
                }
            }
        })
    };

    let profiles_section = profiles_section(&store);
    let plantings_section = plantings::section(&store, open_profiles);
    let spaces_section = spaces::section(&store, highlight.clone());
    let current = Rc::new(RefCell::new(String::from(PLANTINGS)));

    let select: NavSelect = {
        let window = window.clone();
        let sections = sections.clone();
        let profiles_section = profiles_section.clone();
        let plantings_section = plantings_section.clone();
        let spaces_section = spaces_section.clone();
        let current = current.clone();
        let highlight = highlight.clone();
        Rc::new(move |area| {
            // Leaving an area closes the pages pushed inside it, so the sidebar always lands on a
            // list. Clicking the area you are already in does the same, which makes it a way back
            // to the list from anywhere.
            let previous = current.borrow().clone();

            let switch = {
                let area = area.to_string();
                let previous = previous.clone();
                let current = current.clone();
                let sections = sections.clone();
                let profiles_section = profiles_section.clone();
                let plantings_section = plantings_section.clone();
                let spaces_section = spaces_section.clone();
                let highlight = highlight.clone();
                move || {
                    *current.borrow_mut() = area.clone();
                    sections.set_visible_child_name(&area);

                    // The area we leave closes its pages; the area we enter rebuilds its list,
                    // because the other area may have changed what it shows.
                    match previous.as_str() {
                        PROFILES => profiles_section.enter(),
                        PLANTINGS => plantings_section.enter(),
                        SPACES => spaces_section.enter(),
                        _ => {}
                    }

                    match area.as_str() {
                        PROFILES => profiles_section.enter(),
                        PLANTINGS => plantings_section.enter(),
                        SPACES => spaces_section.enter(),
                        _ => {}
                    }

                    // Closing those pages may report the area they belong to; the area we land
                    // in has the final say.
                    highlight(&area);
                }
            };

            let dirty = match previous.as_str() {
                PROFILES => profiles_section.is_dirty(),
                PLANTINGS => plantings_section.is_dirty(),
                SPACES => spaces_section.is_dirty(),
                _ => false,
            };

            if dirty {
                confirm_discard(window.upcast_ref::<gtk::Window>(), Rc::new(switch));
            } else {
                switch();
            }
        })
    };

    *open_profiles_slot.borrow_mut() = Some({
        let select = select.clone();
        Rc::new(move || select(PROFILES))
    });

    sections.add_named(profiles_section.widget(), Some(PROFILES));
    sections.add_named(plantings_section.widget(), Some(PLANTINGS));
    sections.add_named(spaces_section.widget(), Some(SPACES));
    sections.add_named(&help::page(), Some(HELP));
    sections.add_named(&settings::section(&store), Some(SETTINGS));

    profiles_list.connect_row_activated({
        let select = select.clone();
        move |_, _| select(PROFILES)
    });

    plantings_list.connect_row_activated({
        let select = select.clone();
        move |_, _| select(PLANTINGS)
    });

    spaces_list.connect_row_activated({
        let select = select.clone();
        move |_, _| select(SPACES)
    });

    help_list.connect_row_activated({
        let select = select.clone();
        move |_, _| select(HELP)
    });

    settings_list.connect_row_activated({
        let select = select.clone();
        move |_, _| select(SETTINGS)
    });

    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.add_css_class("omagrow-sidebar");
    sidebar.set_width_request(248);

    let brand = gtk::Label::new(Some("OmaGrow"));
    brand.add_css_class("omagrow-logo");
    brand.set_xalign(0.0);
    brand.set_margin_top(20);
    brand.set_margin_bottom(16);
    brand.set_margin_start(20);
    brand.set_margin_end(16);

    let brand_compact = gtk::Label::new(Some("OG"));
    brand_compact.add_css_class("omagrow-logo");
    brand_compact.add_css_class("omagrow-logo-compact");
    brand_compact.set_halign(gtk::Align::Center);
    brand_compact.set_margin_top(14);
    brand_compact.set_margin_bottom(12);
    brand_compact.set_visible(false);

    let nav = gtk::Box::new(gtk::Orientation::Vertical, 0);
    nav.set_margin_start(10);
    nav.set_margin_end(10);
    nav.append(&plantings_list);
    nav.append(&spaces_list);
    nav.append(&profiles_list);

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);

    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.set_margin_start(14);
    separator.set_margin_end(14);
    separator.set_margin_bottom(6);

    let bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bottom.set_margin_start(10);
    bottom.set_margin_end(10);
    bottom.set_margin_bottom(12);
    bottom.append(&help_list);
    bottom.append(&separator);
    bottom.append(&settings_list);

    sidebar.append(&brand);
    sidebar.append(&brand_compact);
    sidebar.append(&nav);
    sidebar.append(&spacer);
    sidebar.append(&bottom);

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.append(&sidebar);
    root.append(&sections);

    window.set_content(Some(&root));

    // Narrow window: collapse the sidebar to icons only.
    let compact = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 760px").expect("valid breakpoint condition"),
    );
    compact.add_setter(&sidebar, "width-request", Some(&56i32.to_value()));
    compact.add_setter(&brand, "visible", Some(&false.to_value()));
    compact.add_setter(&brand_compact, "visible", Some(&true.to_value()));
    compact.add_setter(&profiles_label, "visible", Some(&false.to_value()));
    compact.add_setter(&plantings_label, "visible", Some(&false.to_value()));
    compact.add_setter(&spaces_label, "visible", Some(&false.to_value()));
    compact.add_setter(&help_label, "visible", Some(&false.to_value()));
    compact.add_setter(&settings_label, "visible", Some(&false.to_value()));
    compact.add_setter(&nav, "margin-start", Some(&0i32.to_value()));
    compact.add_setter(&nav, "margin-end", Some(&0i32.to_value()));
    compact.add_setter(&nav, "margin-top", Some(&12i32.to_value()));
    compact.add_setter(&bottom, "margin-start", Some(&0i32.to_value()));
    compact.add_setter(&bottom, "margin-end", Some(&0i32.to_value()));
    window.add_breakpoint(compact);

    // Closing the window must not throw away a form either.
    window.connect_close_request({
        let current = current.clone();
        let profiles_section = profiles_section.clone();
        let plantings_section = plantings_section.clone();
        let spaces_section = spaces_section.clone();
        move |window| {
            let dirty = match current.borrow().as_str() {
                PROFILES => profiles_section.is_dirty(),
                PLANTINGS => plantings_section.is_dirty(),
                SPACES => spaces_section.is_dirty(),
                _ => false,
            };

            if dirty {
                let parent = window.clone();
                let window = window.clone();
                confirm_discard(
                    parent.upcast_ref::<gtk::Window>(),
                    Rc::new(move || {
                        window.destroy();
                    }),
                );
                return gtk::glib::Propagation::Stop;
            }

            gtk::glib::Propagation::Proceed
        }
    });

    select(PLANTINGS);

    window.present();
}

fn profiles_section(store: &Rc<Store>) -> Section {
    let nav = adw::NavigationView::new();
    nav.set_pop_on_escape(true);

    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(false)
        .max_children_per_line(4)
        .min_children_per_line(1)
        .row_spacing(12)
        .column_spacing(12)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .valign(gtk::Align::Start)
        .build();

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&flow)
        .build();

    let status = adw::StatusPage::new();
    status.add_css_class("omagrow-empty");
    status.set_icon_name(Some("omagrow-plant-symbolic"));
    status.set_title("No plant profiles yet");
    status.set_description(Some(
        "Without plants, there is no growing. Set up your first plant profile to get started.",
    ));

    let create = gtk::Button::with_label("Create plant profile");
    create.add_css_class("suggested-action");
    create.add_css_class("pill");
    create.set_halign(gtk::Align::Center);
    status.set_child(Some(&create));

    let pages = gtk::Stack::new();
    pages.add_named(&status, Some("empty"));
    pages.add_named(&scroller, Some("list"));

    let header = page_header("Plant Profiles");

    let add = gtk::Button::from_icon_name("list-add-symbolic");
    add.add_css_class("flat");
    add.set_tooltip_text(Some("New plant profile"));
    header.pack_end(&add);

    let root = adw::ToolbarView::new();
    root.add_top_bar(&header);
    root.set_content(Some(&pages));

    let root_page = adw::NavigationPage::new(&root, "Plant Profiles");
    nav.push(&root_page);

    let open_view: Rc<dyn Fn(i64)> = {
        let nav = nav.clone();
        let store = store.clone();
        let root_page = root_page.clone();
        Rc::new(move |id| {
            if let Some(page) = profile_view(&store, &nav, &root_page, id, None) {
                nav.push(&page);
            }
        })
    };

    let open_create: Rc<dyn Fn()> = {
        let nav = nav.clone();
        let store = store.clone();
        let root_page = root_page.clone();
        Rc::new(move || {
            nav.push(&profile_create(&store, &nav, &root_page));
        })
    };

    let refresh: Rc<dyn Fn()> = {
        let flow = flow.clone();
        let pages = pages.clone();
        let status = status.clone();
        let store = store.clone();
        let open_view = open_view.clone();
        Rc::new(move || {
            while let Some(child) = flow.first_child() {
                flow.remove(&child);
            }

            match store.profiles() {
                Ok(profiles) if profiles.is_empty() => {
                    status.set_title("No plant profiles yet");
                    status.set_description(Some(
                        "Without plants, there is no growing. Set up your first plant profile to get started.",
                    ));
                    pages.set_visible_child_name("empty");
                }
                Ok(profiles) => {
                    for profile in profiles {
                        flow.append(&card(&store, &profile, open_view.clone()));
                    }
                    pages.set_visible_child_name("list");
                }
                Err(error) => {
                    status.set_title("Could not load plant profiles");
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

    create.connect_clicked({
        let open_create = open_create.clone();
        move |_| open_create()
    });

    nav.connect_popped({
        let refresh = refresh.clone();
        move |_, _| refresh()
    });

    refresh();

    Section::new(nav, root_page, refresh)
}

fn card(store: &Store, profile: &Profile, on_open: Rc<dyn Fn(i64)>) -> gtk::Button {
    let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);

    match avatar::source(store, "profile", profile.id) {
        avatar::Source::Photo(stem) => {
            if let Some(path) = avatar::photo_path(&stem) {
                let picture = gtk::Picture::for_filename(&path);
                picture.set_content_fit(gtk::ContentFit::Cover);
                picture.set_height_request(120);
                picture.set_can_shrink(true);

                // The cover fills the top of the card, so its top corners follow the card's radius.
                let clip = gtk::Box::new(gtk::Orientation::Vertical, 0);
                clip.add_css_class("omagrow-card-cover");
                clip.set_overflow(gtk::Overflow::Hidden);
                clip.append(&picture);
                wrapper.append(&clip);
            }
        }
        avatar::Source::Icon(name) => {
            let band = gtk::Box::new(gtk::Orientation::Vertical, 0);
            band.add_css_class("omagrow-card-icon");
            band.set_height_request(120);

            let icon = gtk::Image::from_icon_name(name);
            icon.set_pixel_size(48);
            icon.add_css_class("omagrow-plant-icon");
            icon.set_halign(gtk::Align::Center);
            icon.set_valign(gtk::Align::Center);
            icon.set_vexpand(true);
            band.append(&icon);
            wrapper.append(&band);
        }
        avatar::Source::Picture(picture) | avatar::Source::Default(picture) => {
            let band = gtk::Box::new(gtk::Orientation::Vertical, 0);
            band.add_css_class("omagrow-card-icon");
            band.set_height_request(120);

            let picture = avatar::picture_widget(picture, 64);
            picture.set_halign(gtk::Align::Center);
            picture.set_valign(gtk::Align::Center);
            picture.set_vexpand(true);
            band.append(&picture);
            wrapper.append(&band);
        }
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let name = gtk::Label::new(Some(&profile.name));
    name.set_xalign(0.0);
    name.set_ellipsize(pango::EllipsizeMode::End);
    name.add_css_class("heading");
    content.append(&name);

    if let Some(species) = profile
        .species
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let label = gtk::Label::new(None);
        label.set_markup(&format!(
            "<i>{}</i>",
            gtk::glib::markup_escape_text(species)
        ));
        label.set_xalign(0.0);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.add_css_class("dim-label");
        content.append(&label);
    }

    if let Some(cultivar) = profile
        .cultivar
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        let label = gtk::Label::new(Some(cultivar));
        label.set_xalign(0.0);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.add_css_class("dim-label");
        content.append(&label);
    }

    wrapper.append(&content);

    let button = gtk::Button::new();
    button.add_css_class("card");
    button.set_child(Some(&wrapper));
    button.set_hexpand(false);
    button.set_width_request(240);

    let id = profile.id;
    button.connect_clicked(move |_| on_open(id));

    button
}

/// The read-only profile page: what you see after saving or when opening a card.
/// A status card above the plant data: how complete the profile is, how much of it the user has
/// confirmed, and the way in to the editor.
fn completeness_card(store: &Rc<Store>, profile_id: i64, on_edit: Rc<dyn Fn()>) -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("omagrow-completeness");

    let completeness = match completeness::load(store, profile_id) {
        Ok(completeness) => completeness,
        Err(error) => {
            eprintln!("could not score profile {profile_id}: {error}");
            return card.upcast();
        }
    };

    let ready = completeness.ready_done();

    if completeness.is_complete() {
        card.add_css_class("is-ready");
    }

    let heading = gtk::Label::new(Some(if completeness.is_complete() {
        "Profile data complete"
    } else if completeness.is_ready() {
        "Essentials present"
    } else if ready == 0 {
        "This profile is still empty"
    } else {
        "This profile is incomplete"
    }));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    card.append(&heading);

    let progress = gtk::ProgressBar::new();
    progress.set_fraction(completeness.fraction());
    progress.set_show_text(false);
    progress.set_hexpand(true);
    card.append(&progress);

    let summary = gtk::Label::new(Some(&completeness.summary()));
    summary.set_xalign(0.0);
    summary.set_wrap(true);
    card.append(&summary);

    let missing = if !completeness.is_ready() {
        completeness.missing_ready()
    } else {
        completeness.missing_useful()
    };

    if !missing.is_empty() {
        let label = gtk::Label::new(Some(&format!(
            "{}: {}.",
            if completeness.is_ready() {
                "Still useful"
            } else {
                "Missing"
            },
            missing.join(", ")
        )));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.add_css_class("dim-label");
        card.append(&label);
    }

    let trust = gtk::Label::new(Some(&trust_text(store, profile_id)));
    trust.set_xalign(0.0);
    trust.add_css_class("dim-label");
    trust.add_css_class("caption");
    card.append(&trust);
    gtk::glib::timeout_add_seconds_local(2, {
        let store = store.clone();
        let trust = trust.clone();
        move || {
            if trust.root().is_none() {
                return gtk::glib::ControlFlow::Break;
            }
            let text = trust_text(&store, profile_id);
            if trust.text() != text {
                trust.set_text(&text);
            }
            gtk::glib::ControlFlow::Continue
        }
    });

    if !completeness.is_complete() {
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.set_margin_top(4);

        let complete = gtk::Button::with_label("Complete this profile");
        complete.add_css_class("suggested-action");
        complete.connect_clicked(move |_| on_edit());
        actions.append(&complete);

        card.append(&actions);
    }

    card.upcast()
}

fn trust_text(store: &Store, profile_id: i64) -> String {
    let counts = match store.provenance_status_counts(profile_id) {
        Ok(counts) => counts,
        Err(error) => {
            eprintln!("could not count the value statuses for profile {profile_id}: {error}");
            return String::new();
        }
    };
    let count = |status: &str| {
        counts
            .iter()
            .find(|(kind, _)| kind == status)
            .map(|(_, value)| *value)
            .unwrap_or(0)
    };

    let not_applicable = count("not_applicable");
    if not_applicable > 0 {
        format!(
            "{} confirmed · {} estimated · {} not applicable",
            count("confirmed"),
            count("estimated"),
            not_applicable
        )
    } else {
        format!(
            "{} confirmed · {} estimated",
            count("confirmed"),
            count("estimated")
        )
    }
}

fn profile_view(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    id: i64,
    announce: Option<&str>,
) -> Option<adw::NavigationPage> {
    let profile = match store.profile(id) {
        Ok(Some(profile)) => profile,
        Ok(None) => {
            eprintln!("plant profile {id} no longer exists");
            return None;
        }
        Err(error) => {
            eprintln!("could not open plant profile {id}: {error}");
            return None;
        }
    };

    let header = page_header(&profile.name);

    let edit = gtk::Button::with_label("Edit");
    edit.add_css_class("flat");
    header.pack_end(&edit);

    let more = gtk::MenuButton::new();
    more.set_icon_name("view-more-symbolic");
    more.add_css_class("flat");
    more.set_tooltip_text(Some("More actions"));

    header.pack_end(&more);

    let overlay = adw::ToastOverlay::new();

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
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

    let page = adw::NavigationPage::new(&overlay, "Plant profile");

    // The profile cards push their own editors. Rebuild the body when the page is shown again,
    // so a new stage, range or confirmation is visible without leaving the profile and coming
    // back through the list.
    let rendered = Rc::new(Cell::new(false));
    let columns_slot: Rc<RefCell<Option<Columns>>> = Rc::new(RefCell::new(None));
    let render: Rc<dyn Fn()> = Rc::new({
        let body = body.clone();
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let overlay = overlay.clone();
        let columns_slot = columns_slot.clone();
        let clamp = clamp.clone();
        move || {
            fill_profile_body(
                &body,
                &store,
                &nav,
                &root_page,
                id,
                &overlay,
                &columns_slot,
                &clamp,
            );
        }
    });
    render();
    rendered.set(true);
    page.connect_showing({
        let render = render.clone();
        let rendered = rendered.clone();
        move |_| {
            // The first showing was already rendered above; rebuilding during the push
            // transition duplicates work and can upset the frame clock.
            if rendered.replace(false) {
                return;
            }
            render();
        }
    });

    edit.connect_clicked({
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        move |_| {
            nav.push(&profile_edit(&store, &nav, &root_page, id));
        }
    });

    let menu = gtk::gio::Menu::new();
    menu.append(Some("Duplicate plant profile"), Some("profile.duplicate"));
    menu.append(Some("Delete plant profile"), Some("profile.delete"));

    let actions = gtk::gio::SimpleActionGroup::new();

    let duplicate_action = gtk::gio::SimpleAction::new("duplicate", None);
    duplicate_action.connect_activate({
        let nav = nav.clone();
        let store = store.clone();
        let root_page = root_page.clone();
        let overlay = overlay.clone();
        move |_, _| {
            // Let the native menu finish closing before the page transition starts.
            let store = store.clone();
            let nav = nav.clone();
            let root_page = root_page.clone();
            let overlay = overlay.clone();
            gtk::glib::timeout_add_local_once(Duration::from_millis(250), move || {
                match store.copy_profile(id) {
                    Ok(new_id) => match profile_view(
                        &store,
                        &nav,
                        &root_page,
                        new_id,
                        Some("Profile copied. Photo, icon and history were not copied."),
                    ) {
                        Some(view) => nav.push(&view),
                        None => {
                            overlay.add_toast(adw::Toast::new(
                                "Copied, but the new profile could not be opened",
                            ));
                        }
                    },
                    Err(error) => {
                        eprintln!("could not copy plant profile {id}: {error}");
                        overlay.add_toast(adw::Toast::new("Could not duplicate the plant profile"));
                    }
                }
            });
        }
    });
    actions.add_action(&duplicate_action);

    let delete_action = gtk::gio::SimpleAction::new("delete", None);
    delete_action.connect_activate({
        let nav = nav.clone();
        let store = store.clone();
        let overlay = overlay.clone();
        let name = profile.name.clone();

        move |_, _| {
            let count = store.planting_count_for_profile(id).unwrap_or(0);
            if count > 0 {
                let plantings = if count == 1 {
                    "1 planting".to_string()
                } else {
                    format!("{count} plantings")
                };
                let dialog = adw::AlertDialog::builder()
                    .heading(format!("“{name}” is in use"))
                    .body(format!(
                        "This plant profile still has {plantings}. Finish or delete them first, \
                         then the profile can go."
                    ))
                    .build();
                dialog.add_response("close", "OK");
                dialog.set_default_response(Some("close"));
                dialog.set_close_response("close");

                let window = nav
                    .root()
                    .and_then(|root| root.downcast::<gtk::Window>().ok());
                dialog.present(window.as_ref());
                return;
            }

            let dialog = adw::AlertDialog::builder()
                .heading(format!("Delete “{name}”?"))
                .body("The plant profile is removed for good. This cannot be undone.")
                .build();
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("delete", "Delete");
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");

            let window = nav
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok());
            let nav = nav.clone();
            let store = store.clone();
            let overlay = overlay.clone();
            dialog.connect_response(None, move |_, response| {
                if response != "delete" {
                    return;
                }

                let stems: Vec<String> = store
                    .media_for("profile", id)
                    .map(|rows| rows.into_iter().map(|media| media.file_stem).collect())
                    .unwrap_or_default();

                if let Err(error) = store.delete_profile(id) {
                    eprintln!("could not delete plant profile {id}: {error}");
                    overlay.add_toast(adw::Toast::new("Could not delete the plant profile"));
                    return;
                }

                for stem in stems {
                    media::remove(&stem);
                }

                nav.pop();
            });

            dialog.present(window.as_ref());
        }
    });
    actions.add_action(&delete_action);

    more.insert_action_group("profile", Some(&actions));
    more.set_menu_model(Some(&menu));

    if let Some(message) = announce {
        let message = message.to_string();
        let overlay = overlay.clone();
        gtk::glib::idle_add_local_once(move || {
            overlay.add_toast(adw::Toast::new(&message));
        });
    }

    Some(page)
}

#[allow(clippy::too_many_arguments)]
fn fill_profile_body(
    body: &gtk::Box,
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
    id: i64,
    overlay: &adw::ToastOverlay,
    columns_slot: &Rc<RefCell<Option<Columns>>>,
    clamp: &adw::Clamp,
) {
    while let Some(child) = body.first_child() {
        body.remove(&child);
    }

    let Some(profile) = store.profile(id).ok().flatten() else {
        return;
    };

    let open_editor: Rc<dyn Fn()> = Rc::new({
        let nav = nav.clone();
        let store = store.clone();
        let root_page = root_page.clone();
        move || {
            nav.push(&profile_edit(&store, &nav, &root_page, id));
        }
    });
    let columns = Columns::new(clamp);
    columns.push(&hero(store, id, overlay), Slot::Main);

    let names = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let name = gtk::Label::new(Some(&profile.name));
    name.set_xalign(0.0);
    name.set_wrap(true);
    name.add_css_class("omagrow-profile-name");
    names.append(&name);

    if let Some(markup) = botanical_markup(profile.species.as_deref(), profile.cultivar.as_deref())
    {
        let label = gtk::Label::new(None);
        label.set_markup(&markup);
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.add_css_class("dim-label");
        names.append(&label);
    }
    columns.push(&names, Slot::Main);

    if let Some(tiles) = stat_tiles(store, id) {
        columns.push(&tiles, Slot::Side);
    }

    columns.push(
        &completeness_card(store, id, open_editor.clone()),
        Slot::Side,
    );

    if let Some(proposals) = proposals_card(store, nav, root_page, id) {
        columns.push(&proposals, Slot::Side);
    }

    if let Some(bands) = temperature_bands(store, nav, id) {
        columns.push(&bands, Slot::Side);
    }

    columns.push(&growing_data(store, nav, root_page, id), Slot::Main);

    if let Some(history) = recent_changes_card(store, nav, root_page, id) {
        columns.push(&history, Slot::Side);
    }

    columns.finish(shared::layout::window_width(nav));
    body.append(columns.widget());
    *columns_slot.borrow_mut() = Some(columns);
}

fn hero(store: &Rc<Store>, profile_id: i64, overlay: &adw::ToastOverlay) -> gtk::Widget {
    let canvas = gtk::Box::new(gtk::Orientation::Vertical, 0);
    canvas.set_hexpand(true);
    canvas.set_vexpand(false);

    let frame = gtk::Overlay::new();
    frame.set_child(Some(&canvas));
    frame.add_css_class("card");
    frame.add_css_class("omagrow-hero");
    frame.set_height_request(200);
    // A child that expands must not turn the hero into a full-height page block.
    frame.set_vexpand(false);
    frame.set_valign(gtk::Align::Start);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.set_halign(gtk::Align::End);
    actions.set_valign(gtk::Align::Start);
    actions.set_margin_top(10);
    actions.set_margin_end(10);

    let remove = gtk::Button::with_label("Remove");
    remove.add_css_class("flat");
    remove.add_css_class("omagrow-hero-button");

    let icon = gtk::Button::with_label("Choose illustration");
    icon.add_css_class("flat");
    icon.add_css_class("omagrow-hero-button");

    let change = gtk::Button::with_label("Add photo");
    change.add_css_class("flat");
    change.add_css_class("omagrow-hero-button");

    actions.append(&icon);
    actions.append(&remove);
    actions.append(&change);
    frame.add_overlay(&actions);

    render_hero(&canvas, &change, &remove, store, profile_id);

    icon.connect_clicked({
        let store = store.clone();
        let canvas = canvas.clone();
        let change = change.clone();
        let remove = remove.clone();
        move |trigger| {
            let store = store.clone();
            let canvas = canvas.clone();
            let change = change.clone();
            let remove = remove.clone();
            let refresh_store = store.clone();
            avatar::choose_illustration(
                trigger,
                &store,
                "profile",
                profile_id,
                Rc::new(move || {
                    render_hero(&canvas, &change, &remove, &refresh_store, profile_id);
                }),
            );
        }
    });

    change.connect_clicked({
        let store = store.clone();
        let canvas = canvas.clone();
        let remove_weak = remove.downgrade();
        let overlay = overlay.clone();
        move |trigger| {
            if let Some(remove) = remove_weak.upgrade() {
                choose_photo(trigger, &canvas, &remove, &store, profile_id, &overlay);
            }
        }
    });

    remove.connect_clicked({
        let store = store.clone();
        let canvas = canvas.clone();
        let change_weak = change.downgrade();
        let remove_weak = remove.downgrade();
        let overlay = overlay.clone();

        move |button| {
            let Some(window) = button
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            else {
                return;
            };

            let dialog = adw::AlertDialog::builder()
                .heading("Remove this photo?")
                .body("The photo is deleted from OmaGrow. This cannot be undone.")
                .build();
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("remove", "Remove");
            dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");

            let store = store.clone();
            let canvas = canvas.clone();
            let change_weak = change_weak.clone();
            let remove_weak = remove_weak.clone();
            let overlay = overlay.clone();

            dialog.connect_response(None, move |_, response| {
                if response != "remove" {
                    return;
                }

                let stem = store
                    .media("profile", profile_id, "cover")
                    .ok()
                    .flatten()
                    .map(|media| media.file_stem);

                if let Err(error) = store.delete_media("profile", profile_id, "cover") {
                    eprintln!("could not remove the photo for profile {profile_id}: {error}");
                    overlay.add_toast(adw::Toast::new("Could not remove the photo"));
                    return;
                }

                if let Some(stem) = stem {
                    media::remove(&stem);
                }

                if let (Some(change), Some(remove)) = (change_weak.upgrade(), remove_weak.upgrade())
                {
                    render_hero(&canvas, &change, &remove, &store, profile_id);
                }
                overlay.add_toast(adw::Toast::new("Photo removed"));
            });

            dialog.present(Some(&window));
        }
    });

    frame.upcast()
}

/// Fills the hero with the cover photo or the placeholder, and names the button accordingly.
fn render_hero(
    canvas: &gtk::Box,
    change: &gtk::Button,
    remove: &gtk::Button,
    store: &Store,
    profile_id: i64,
) {
    while let Some(child) = canvas.first_child() {
        canvas.remove(&child);
    }

    let has_photo = store
        .media("profile", profile_id, "cover")
        .ok()
        .flatten()
        .is_some_and(|media| media::detail_path(&media.file_stem).is_some());

    match avatar::source(store, "profile", profile_id) {
        avatar::Source::Photo(stem) => {
            let Some(path) = media::detail_path(&stem) else {
                hero_placeholder(
                    canvas,
                    change,
                    remove,
                    avatar::fallback(store, "profile", profile_id),
                );
                return;
            };

            let picture = gtk::Picture::for_filename(&path);
            picture.set_content_fit(gtk::ContentFit::Cover);
            picture.set_can_shrink(true);
            picture.set_hexpand(true);
            picture.set_vexpand(true);

            let open = gtk::Button::new();
            open.add_css_class("flat");
            open.set_child(Some(&picture));
            open.set_hexpand(true);
            open.set_vexpand(true);
            open.set_tooltip_text(Some("Show the photo"));
            open.connect_clicked(move |button| {
                if let Some(window) = button
                    .root()
                    .and_then(|root| root.downcast::<gtk::Window>().ok())
                {
                    show_photo(&window, &path);
                }
            });

            canvas.append(&open);
            change.set_label("Change photo");
            remove.set_visible(true);
        }
        avatar::Source::Icon(name) => {
            let icon = hero_icon(name);
            icon.set_opacity(0.8);
            icon.set_margin_top(52);
            canvas.append(&icon);
            change.set_label(if has_photo {
                "Change photo"
            } else {
                "Add photo"
            });
            remove.set_visible(has_photo);
        }
        avatar::Source::Picture(picture) => {
            let picture = avatar::picture_widget(picture, 128);
            picture.set_halign(gtk::Align::Center);
            picture.set_valign(gtk::Align::Center);
            picture.set_margin_top(36);
            canvas.append(&picture);
            change.set_label(if has_photo {
                "Change photo"
            } else {
                "Add photo"
            });
            remove.set_visible(has_photo);
        }
        avatar::Source::Default(picture) => {
            hero_placeholder(canvas, change, remove, picture);
        }
    }
}

fn hero_placeholder(
    canvas: &gtk::Box,
    change: &gtk::Button,
    remove: &gtk::Button,
    picture: avatar::Picture,
) {
    let placeholder = gtk::Box::new(gtk::Orientation::Vertical, 8);
    placeholder.set_halign(gtk::Align::Center);
    placeholder.set_valign(gtk::Align::Center);
    placeholder.set_margin_top(36);

    let picture = avatar::picture_widget(picture, 112);
    picture.set_opacity(0.6);
    let hint = gtk::Label::new(Some("No photo yet"));
    hint.add_css_class("dim-label");
    placeholder.append(&picture);
    placeholder.append(&hint);

    canvas.append(&placeholder);
    change.set_label("Add photo");
    remove.set_visible(false);
}

fn hero_icon(name: &str) -> gtk::Image {
    let icon = gtk::Image::from_icon_name(name);
    icon.set_pixel_size(96);
    icon.add_css_class("omagrow-plant-icon");
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
    icon
}

fn choose_photo(
    trigger: &gtk::Button,
    canvas: &gtk::Box,
    remove: &gtk::Button,
    store: &Rc<Store>,
    profile_id: i64,
    overlay: &adw::ToastOverlay,
) {
    let Some(window) = trigger
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

    let stem = store
        .media("profile", profile_id, "cover")
        .ok()
        .flatten()
        .map(|media| media.file_stem)
        .unwrap_or_else(|| media::stem_for("profile", profile_id, "cover"));

    let store = store.clone();
    let canvas = canvas.clone();
    let remove = remove.clone();
    let trigger = trigger.clone();
    let overlay = overlay.clone();

    dialog.open(Some(&window), gtk::gio::Cancellable::NONE, move |result| {
        let Ok(file) = result else {
            return;
        };
        let Some(path) = file.path() else {
            return;
        };

        let imported = match media::import(&path, &stem) {
            Ok(imported) => imported,
            Err(message) => {
                eprintln!("could not import the photo: {message}");
                overlay.add_toast(adw::Toast::new(&message));
                return;
            }
        };

        let media = Media {
            id: 0,
            owner_type: "profile".to_string(),
            owner_id: profile_id,
            role: "cover".to_string(),
            file_stem: stem,
            width: imported.width,
            height: imported.height,
            bytes: imported.bytes,
        };

        if let Err(error) = store.set_media(&media) {
            eprintln!("could not store the photo for profile {profile_id}: {error}");
            render_hero(&canvas, &trigger, &remove, &store, profile_id);
            overlay.add_toast(adw::Toast::new(
                "Photo saved, but its details could not be stored",
            ));
            return;
        }
        if let Err(error) = store.set_avatar_icon("profile", profile_id, None) {
            eprintln!("could not clear the avatar icon for profile {profile_id}: {error}");
        }

        render_hero(&canvas, &trigger, &remove, &store, profile_id);
        overlay.add_toast(adw::Toast::new("Photo saved"));
    });
}

pub(in crate::ui) fn show_photo(window: &gtk::Window, path: &std::path::Path) {
    let picture = gtk::Picture::for_filename(path);
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_can_shrink(true);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&picture)
        .build();

    let dialog = adw::Dialog::builder()
        .title("Photo")
        .content_width(960)
        .content_height(720)
        .build();
    dialog.set_child(Some(&scroller));
    dialog.present(Some(window));
}

/// Minimal page for a new profile: just enough to make it available for later steps.
fn profile_create(
    store: &Rc<Store>,
    nav: &adw::NavigationView,
    root_page: &adw::NavigationPage,
) -> adw::NavigationPage {
    let (group, name_row, latin_row, cultivar_row) = profile_group(
        None,
        "Only the name is required. Latin name and cultivar can be added later.",
    );

    let preferences = adw::PreferencesPage::new();
    preferences.add(&group);

    let guard = shell::Guard::new(nav);
    let header = guard.header("New plant profile");

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.set_sensitive(false);
    header.pack_end(&save);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    header.pack_end(&cancel);

    let overlay = adw::ToastOverlay::new();
    overlay.set_child(Some(&preferences));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&overlay));

    name_row.connect_changed({
        let save = save.clone();
        move |row| save.set_sensitive(!row.text().trim().is_empty())
    });

    let do_save = {
        let store = store.clone();
        let nav = nav.clone();
        let root_page = root_page.clone();
        let name_row = name_row.clone();
        let latin_row = latin_row.clone();
        let cultivar_row = cultivar_row.clone();
        let overlay = overlay.clone();
        let save = save.clone();
        Rc::new(move || {
            let name = name_row.text().trim().to_string();
            if name.is_empty() {
                return;
            }

            let latin = latin_row.text().trim().to_string();
            let cultivar = cultivar_row.text().trim().to_string();
            save.set_sensitive(false);

            match store.create_profile(&name, Some(&latin), Some(&cultivar)) {
                Ok(id) => match profile_view(&store, &nav, &root_page, id, Some("Saved")) {
                    Some(view) => nav.replace(&[root_page.clone(), view]),
                    None => {
                        overlay.add_toast(adw::Toast::new(
                            "Saved, but the profile could not be opened",
                        ));
                        nav.pop();
                    }
                },
                Err(error) => {
                    eprintln!("could not save plant profile: {error}");
                    overlay.add_toast(adw::Toast::new("Could not save the plant profile"));
                    save.set_sensitive(true);
                }
            }
        })
    };

    save.connect_clicked({
        let do_save = do_save.clone();
        move |_| do_save()
    });

    for row in [&name_row, &latin_row, &cultivar_row] {
        row.connect_entry_activated({
            let do_save = do_save.clone();
            move |_| do_save()
        });
    }

    cancel.connect_clicked({
        let guard = guard.clone();
        move |_| guard.close()
    });

    toolbar.connect_map({
        let name_row = name_row.clone();
        move |_| {
            name_row.grab_focus();
        }
    });

    let page = adw::NavigationPage::new(&toolbar, "New plant profile");
    guard.attach(&page, &toolbar);
    page
}

fn profile_group(
    profile: Option<&Profile>,
    description: &str,
) -> (
    adw::PreferencesGroup,
    adw::EntryRow,
    adw::EntryRow,
    adw::EntryRow,
) {
    let group = adw::PreferencesGroup::new();
    group.set_title("Profile");
    group.set_description(Some(description));

    let name = adw::EntryRow::new();
    name.set_title("Name");

    name.set_tooltip_text(Some("What you call this plant"));

    let species = adw::EntryRow::new();
    species.set_title("Species");
    species.set_tooltip_text(Some("Genus and species, e.g. Capsicum chinense"));

    let cultivar = adw::EntryRow::new();
    cultivar.set_title("Cultivar or variety");
    cultivar.set_tooltip_text(Some("e.g. Bhut Jolokia, Rawit"));

    if let Some(profile) = profile {
        name.set_text(&profile.name);
        species.set_text(profile.species.as_deref().unwrap_or_default());
        cultivar.set_text(profile.cultivar.as_deref().unwrap_or_default());
    }

    group.add(&name);
    group.add(&species);
    group.add(&cultivar);

    (group, name, species, cultivar)
}

fn botanical_markup(species: Option<&str>, cultivar: Option<&str>) -> Option<String> {
    let species = species.map(str::trim).filter(|value| !value.is_empty());
    let cultivar = cultivar.map(str::trim).filter(|value| !value.is_empty());

    match (species, cultivar) {
        (Some(species), Some(cultivar)) => Some(format!(
            "<i>{}</i> '{}'",
            gtk::glib::markup_escape_text(species),
            gtk::glib::markup_escape_text(cultivar)
        )),
        (Some(species), None) => Some(format!("<i>{}</i>", gtk::glib::markup_escape_text(species))),
        (None, Some(cultivar)) => Some(format!("'{}'", gtk::glib::markup_escape_text(cultivar))),
        (None, None) => None,
    }
}

/// Weather updates while the app runs: one check a minute, a call only when the budget allows it.
fn start_weather(store: &Rc<Store>) {
    maybe_fetch_weather(store, WeatherTrigger::Open);

    gtk::glib::timeout_add_seconds_local(60, {
        let store = store.clone();
        move || {
            maybe_fetch_weather(&store, WeatherTrigger::Poll);
            gtk::glib::ControlFlow::Continue
        }
    });
}

#[derive(Clone, Copy)]
enum WeatherTrigger {
    Open,
    Poll,
    Location,
}

/// A location change asks for the weather of the new place without waiting for the budget
/// interval; the daily budget still applies.
fn sync_weather_after_location_change(store: &Rc<Store>) {
    maybe_fetch_weather(store, WeatherTrigger::Location);
}

fn maybe_fetch_weather(store: &Rc<Store>, trigger: WeatherTrigger) {
    let settings = weather::settings(store);

    let Some((today, minutes)) = crate::clock::now() else {
        return;
    };

    let budget = weather::budget(store, today);
    let online = online::enabled(store);
    let Some(location) = store.location().ok().flatten() else {
        return;
    };

    let due = match trigger {
        WeatherTrigger::Open => {
            weather::due_on_open(&settings, &budget, today, minutes, online, true)
        }
        WeatherTrigger::Poll => weather::due_poll(&settings, &budget, today, minutes, online, true),
        WeatherTrigger::Location => weather::due_location_change(&settings, &budget, online, true),
    };
    if !due {
        return;
    }

    // Count the call first: a failure or a crash must not turn into a burst of requests.
    if let Err(error) = weather::record_call(store, today, minutes) {
        eprintln!("could not record the weather call: {error}");
        return;
    }

    let store = store.clone();
    run_async(
        move || weather::fetch(&location),
        move |result| match result {
            Some(Ok(data)) => match weather::apply(&store, &data) {
                Ok(report) => {
                    weather::set_last_error(&store, None);
                    weather_watch::notify();
                    eprintln!(
                        "weather: {} current, {} daily row(s), {} hourly row(s)",
                        if report.current_written {
                            "wrote"
                        } else {
                            "kept"
                        },
                        report.days_written,
                        report.hours_written
                    );
                }
                Err(error) => {
                    weather::set_last_error(&store, Some(&error));
                    eprintln!("could not store the weather: {error}");
                }
            },
            Some(Err(error)) => {
                weather::set_last_error(&store, Some(&error));
                eprintln!("weather update failed: {error}");
            }
            None => {
                weather::set_last_error(&store, Some("the request was dropped"));
                eprintln!("weather update was dropped");
            }
        },
    );
}

/// Runs a blocking network call off the main thread and hands the result back on it.
fn run_async<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(Option<T>) + 'static,
) {
    type Done<T> = Rc<RefCell<Option<Box<dyn FnOnce(Option<T>)>>>>;

    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });

    let receiver = Rc::new(RefCell::new(Some(receiver)));
    let done: Done<T> = Rc::new(RefCell::new(Some(Box::new(done))));

    gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
        let received = {
            let receiver = receiver.borrow();
            let Some(receiver) = receiver.as_ref() else {
                return gtk::glib::ControlFlow::Break;
            };

            match receiver.try_recv() {
                Ok(value) => Some(Some(value)),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                // The worker stopped without an answer, for example after a panic.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
            }
        };

        let Some(received) = received else {
            return gtk::glib::ControlFlow::Continue;
        };

        *receiver.borrow_mut() = None;
        let done = done.borrow_mut().take();
        if let Some(done) = done {
            done(received);
        }

        gtk::glib::ControlFlow::Break
    });
}

fn error_page(message: &str) -> gtk::Widget {
    adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title("OmaGrow could not open its database")
        .description(message)
        .build()
        .upcast()
}

fn page_header(title: &str) -> adw::HeaderBar {
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(title, "")));
    header
}

fn nav_list() -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("navigation-sidebar");
    list
}

/// A sidebar icon in the plant green, shared by the three grow areas.
fn nav_icon(name: &str) -> gtk::Image {
    let icon = gtk::Image::from_icon_name(name);
    icon.set_pixel_size(20);
    icon.add_css_class("omagrow-nav-icon");
    icon
}

fn plant_icon(size: i32) -> gtk::Image {
    let icon = gtk::Image::from_icon_name("omagrow-plant-symbolic");
    icon.set_pixel_size(size);
    icon.add_css_class("omagrow-plant-icon");
    icon
}

fn nav_row(label: &str, icon: &gtk::Image) -> (gtk::ListBoxRow, gtk::Label) {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(10);
    content.set_margin_end(10);

    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);

    content.append(icon);
    content.append(&text);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&content));
    row.set_tooltip_text(Some(label));
    row.set_activatable(true);

    (row, text)
}

#[cfg(test)]
mod tests {
    use super::botanical_markup;

    #[test]
    fn renders_the_botanical_name() {
        assert_eq!(
            botanical_markup(Some("Capsicum chinense"), Some("Bhut Jolokia Red")).as_deref(),
            Some("<i>Capsicum chinense</i> 'Bhut Jolokia Red'")
        );
        assert_eq!(
            botanical_markup(Some("Ocimum basilicum"), None).as_deref(),
            Some("<i>Ocimum basilicum</i>")
        );
        assert_eq!(
            botanical_markup(None, Some("Rawit")).as_deref(),
            Some("'Rawit'")
        );
        assert_eq!(botanical_markup(None, None), None);
        assert_eq!(botanical_markup(Some("  "), Some(" ")), None);
    }

    #[test]
    fn escapes_markup_in_names() {
        assert_eq!(
            botanical_markup(Some("Capsicum <b>"), Some("Red & Hot")).as_deref(),
            Some("<i>Capsicum &lt;b&gt;</i> 'Red &amp; Hot'")
        );
    }
}
