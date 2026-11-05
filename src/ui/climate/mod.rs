//! One climate section for a planting or a space.
//!
//! The section owns the time range, the source switches and the day/night view; the plots below it
//! are small multiples on that one axis. A wide section shows every metric with data in a 2×2 grid;
//! a narrow one shows a single plot with a tab per metric, so each graph keeps the full width of
//! the page instead of shrinking to half of it.

mod data;
mod draw;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use gtk::prelude::*;

use crate::calendar::Date;
use crate::clock::today;
use crate::planting::{Planting, Space};
use crate::schedule::Schedule;
use crate::store::Store;
use crate::units::UnitPreference;

use super::shared::layout;
use data::{ClimateData, DayNightView, DayPart, Metric, Range, Sources, short_date};

/// The section switches to the 2×2 grid on its own width, not the window's: a one-column page has
/// to keep the tabbed single plot even on a wide screen. The section is about 1152 px on a wide
/// page and 672 px on a one-column page, so 900 px separates the two.
const GRID_FROM: i32 = 900;

fn shows_grid(width: i32) -> bool {
    width >= GRID_FROM
}

/// The date the page's calendar has selected. The section reads it on every draw.
pub(super) type SelectedDay = data::SelectedDay;

pub(super) fn selected_day(date: Date) -> SelectedDay {
    data::selected_day(date)
}

/// The state a page keeps while it is open: the section is rebuilt after a weather sync, but the
/// user's range and switches should survive that.
pub(super) struct SectionState {
    range: data::GraphRange,
    sources: data::SourceState,
    view: data::ViewState,
}

pub(super) struct ClimateSection {
    pub widget: gtk::Widget,
    pub redraw: Rc<dyn Fn()>,
}

type Updates = Rc<RefCell<Vec<Weak<dyn Fn()>>>>;

#[derive(Clone, Copy)]
struct SavedState {
    range: Range,
    sources: Sources,
    view: DayNightView,
    /// The graph the tabs showed last time, so a rebuilt section reopens on the same one.
    metric: Metric,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            range: Range::Day,
            sources: Sources::default(),
            view: DayNightView::Combined,
            metric: Metric::Temperature,
        }
    }
}

thread_local! {
    /// Kept for the life of the app, per planting or space. The page itself is rebuilt after a
    /// sync; the graph's view should not jump back to 24h every time.
    static SAVED: RefCell<HashMap<String, SavedState>> = RefCell::new(HashMap::new());
}

fn saved_state(key: &str) -> Option<SavedState> {
    SAVED.with(|saved| saved.borrow().get(key).copied())
}

fn remember(key: &str, state: SavedState) {
    SAVED.with(|saved| {
        saved.borrow_mut().insert(key.to_string(), state);
    });
}

fn section_state(saved: SavedState) -> SectionState {
    SectionState {
        range: Rc::new(Cell::new(saved.range)),
        sources: Rc::new(Cell::new(saved.sources)),
        view: Rc::new(Cell::new(saved.view)),
    }
}

/// The planting page's climate section. The selected calendar day anchors the 24h–90d windows.
pub(super) fn planting_section(
    store: &Store,
    planting: &Planting,
    units: UnitPreference,
    selected: SelectedDay,
    schedule: &Schedule,
) -> Option<ClimateSection> {
    let key = format!("planting-{}", planting.id);
    let saved = saved_state(&key).unwrap_or_default();
    let state = section_state(saved);
    let data = Rc::new(data::planting_data(
        store, planting, units, selected, schedule, state,
    )?);
    let kinds = data::kinds_with_data(&data);
    if kinds.is_empty() {
        return None;
    }
    let initial = initial_metric(&kinds, &saved);
    Some(build_section(data, kinds, initial, key))
}

/// The space page's climate section: the place's history, not only its current plantings.
pub(super) fn space_section(
    store: &Store,
    space: &Space,
    units: UnitPreference,
) -> Option<ClimateSection> {
    let key = format!("space-{}", space.id);
    let saved = saved_state(&key).unwrap_or(SavedState {
        range: Range::Days(7),
        ..SavedState::default()
    });
    let state = section_state(saved);
    let data = Rc::new(data::space_data(store, space, units, state)?);
    let kinds = data::kinds_with_data(&data);
    if kinds.is_empty() {
        return None;
    }
    let initial = initial_metric(&kinds, &saved);
    Some(build_section(data, kinds, initial, key))
}

/// The remembered graph if it still has data here, otherwise the first one.
fn initial_metric(kinds: &[Metric], saved: &SavedState) -> Metric {
    if kinds.contains(&saved.metric) {
        saved.metric
    } else {
        kinds[0]
    }
}

fn build_section(
    data: Rc<ClimateData>,
    kinds: Vec<Metric>,
    initial: Metric,
    key: String,
) -> ClimateSection {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.add_css_class("card");
    root.add_css_class("omagrow-climate");

    let updates: Updates = Rc::new(RefCell::new(Vec::new()));
    let redraw: Rc<dyn Fn()> = Rc::new({
        let updates = updates.clone();
        move || redraw_all(&updates)
    });

    let colors = draw::graph_colors(&root);
    let kind = Rc::new(Cell::new(initial));

    let save: Rc<dyn Fn()> = Rc::new({
        let data = data.clone();
        let kind = kind.clone();
        let key = key.clone();
        move || {
            remember(
                &key,
                SavedState {
                    range: data.range.get(),
                    sources: data.sources.get(),
                    view: data.view.get(),
                    metric: kind.get(),
                },
            );
        }
    });

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let title = gtk::Label::new(Some("Climate"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("heading");
    header.append(&title);

    let (view_button, view_update) = view_menu(data.clone(), updates.clone(), save.clone());
    header.append(&view_button);

    let (range_button, range_update) = range_menu(data.clone(), updates.clone(), save.clone());
    header.append(&range_button);
    root.append(&header);

    let meta = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let range_label = gtk::Label::new(None);
    range_label.set_xalign(0.0);
    range_label.set_hexpand(true);
    range_label.add_css_class("omagrow-axis");
    meta.append(&range_label);

    let source_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let (weather_chip, weather_sync) = source_chip(
        "Weather",
        "━",
        colors.weather,
        "weather",
        data.clone(),
        updates.clone(),
        save.clone(),
    );
    let (sensor_chip, sensor_sync) = source_chip(
        "Sensor",
        "━",
        colors.sensor,
        "sensor",
        data.clone(),
        updates.clone(),
        save.clone(),
    );
    let (manual_chip, manual_sync) = source_chip(
        "Manual",
        "●",
        colors.manual,
        "manual",
        data.clone(),
        updates.clone(),
        save.clone(),
    );
    source_row.append(&weather_chip);
    source_row.append(&sensor_chip);
    source_row.append(&manual_chip);

    let source_note = gtk::Label::new(None);
    source_note.add_css_class("omagrow-axis");
    source_row.append(&source_note);
    meta.append(&source_row);
    root.append(&meta);

    // Narrow: one plot with a tab per metric. Wide: every metric in a 2×2 grid. The section picks
    // on its own width, because a one-column page can sit in a wide window.
    let narrow = gtk::Box::new(gtk::Orientation::Vertical, 8);
    if kinds.len() > 1 {
        let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tabs.add_css_class("linked");
        let mut toggles: Vec<gtk::ToggleButton> = Vec::new();
        for metric in &kinds {
            let button = gtk::ToggleButton::with_label(metric.label());
            button.add_css_class("flat");
            if let Some(first) = toggles.first() {
                button.set_group(Some(first));
            }
            button.set_active(*metric == initial);
            button.connect_toggled({
                let kind = kind.clone();
                let save = save.clone();
                let updates = updates.clone();
                let metric = *metric;
                move |button| {
                    if !button.is_active() {
                        return;
                    }
                    kind.set(metric);
                    save();
                    redraw_all(&updates);
                }
            });
            tabs.append(&button);
            toggles.push(button);
        }
        narrow.append(&tabs);
    }
    narrow.append(&plot_panel(
        data.clone(),
        kind.clone(),
        260,
        updates.clone(),
    ));
    root.append(&narrow);

    let grid = gtk::Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(true);
    grid.set_visible(false);
    // A single card spans both columns; an odd last card spans them too, so the section never
    // leaves a half-empty row.
    for (index, metric) in kinds.iter().enumerate() {
        let panel = plot_panel(
            data.clone(),
            Rc::new(Cell::new(*metric)),
            230,
            updates.clone(),
        );
        let (x, y, width) =
            if kinds.len() == 1 || (index + 1 == kinds.len() && kinds.len() % 2 == 1) {
                (0, (index / 2) as i32, 2)
            } else {
                ((index % 2) as i32, (index / 2) as i32, 1)
            };
        grid.attach(&panel, x, y, width, 1);
    }
    root.append(&grid);

    let apply_width: Rc<dyn Fn(i32)> = Rc::new({
        let narrow = narrow.downgrade();
        let grid = grid.downgrade();
        move |width| {
            let (Some(narrow), Some(grid)) = (narrow.upgrade(), grid.upgrade()) else {
                return;
            };
            let grid_shown = shows_grid(width);
            narrow.set_visible(!grid_shown);
            grid.set_visible(grid_shown);
        }
    });
    layout::watch_widget_width(&root, apply_width);

    let header_update: Rc<dyn Fn()> = Rc::new({
        let data = data.clone();
        let range_label = range_label.clone();
        let range_update = range_update.clone();
        let view_update = view_update.clone();
        let view_button = view_button.clone();
        let weather_chip = weather_chip.clone();
        let sensor_chip = sensor_chip.clone();
        let manual_chip = manual_chip.clone();
        let weather_sync = weather_sync.clone();
        let sensor_sync = sensor_sync.clone();
        let manual_sync = manual_sync.clone();
        let source_note = source_note.clone();
        let source_row = source_row.clone();
        let kinds = kinds.clone();
        move || {
            let (start, end) = data.window();
            range_label.set_text(&if start == end {
                short_date(start)
            } else {
                format!("{} – {}", short_date(start), short_date(end))
            });
            range_update();
            view_update();

            let has_air = kinds.iter().any(|metric| metric.is_air());
            view_button.set_visible(!data.range.get().hourly() && has_air);

            let weather_here = source_here(&data, "weather", start, end);
            let sensor_here = source_here(&data, "sensor", start, end);
            let manual_here = source_here(&data, "manual", start, end);
            let count = [weather_here, sensor_here, manual_here]
                .into_iter()
                .filter(|value| *value)
                .count();
            let sources = data.sources.get();
            // Keep a chip on screen when its source is switched off, even if it is the only
            // source in this window: otherwise a hidden source leaves an empty graph with no way
            // back. A switch that is off but has no data here does not turn a weather-only graph
            // into a chip row, so the model label stays.
            let any_off = (weather_here && !sources.weather)
                || (sensor_here && !sources.sensor)
                || (manual_here && !sources.manual);
            let show_chips = count > 1 || any_off;

            weather_sync.set(true);
            sensor_sync.set(true);
            manual_sync.set(true);
            weather_chip.set_visible(show_chips && weather_here);
            sensor_chip.set_visible(show_chips && sensor_here);
            manual_chip.set_visible(show_chips && manual_here);
            weather_chip.set_active(sources.weather);
            sensor_chip.set_active(sources.sensor);
            manual_chip.set_active(sources.manual);
            weather_sync.set(false);
            sensor_sync.set(false);
            manual_sync.set(false);

            source_note.set_visible(!show_chips && count == 1);
            if !show_chips {
                source_note.set_text(match (weather_here, sensor_here, manual_here) {
                    (true, false, false) => "Open-Meteo model",
                    (false, true, false) => "Sensor readings",
                    (false, false, true) => "Manual readings",
                    _ => "",
                });
            }
            source_row.set_visible(count > 0);
        }
    });
    updates.borrow_mut().push(Rc::downgrade(&header_update));
    keep_alive(&root, header_update.clone());
    header_update();

    ClimateSection {
        widget: root.upcast(),
        redraw,
    }
}

fn plot_panel(
    data: Rc<ClimateData>,
    kind: Rc<Cell<Metric>>,
    height: i32,
    updates: Updates,
) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    card.add_css_class("omagrow-climate-plot");

    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.add_css_class("omagrow-climate-title");
    card.append(&title);

    let legend = gtk::Label::new(None);
    legend.set_xalign(0.0);
    legend.set_wrap(true);
    legend.add_css_class("omagrow-legend");
    card.append(&legend);

    let area = gtk::DrawingArea::new();
    area.set_height_request(height);
    area.set_hexpand(true);

    let empty = gtk::Label::new(None);
    empty.set_wrap(true);
    empty.set_xalign(0.0);
    empty.add_css_class("dim-label");
    empty.set_visible(false);

    let geometry = draw::geometry();
    let hover = Rc::new(Cell::new(None::<(Date, i64)>));
    let colors = draw::graph_colors(&card);

    let update: Rc<dyn Fn()> = Rc::new({
        let data = data.clone();
        let kind = kind.clone();
        let title = title.clone();
        let legend = legend.clone();
        let area = area.clone();
        let empty = empty.clone();
        move || {
            let metric = kind.get();
            title.set_text(&format!(
                "{} · {}",
                metric.label(),
                data.units.label(metric.unit())
            ));

            let (start, end) = data.window();
            let guides = data.has_guides(metric, start, end)
                && (data.range.get().hourly() || metric.is_air());
            let has_data = data.has_visible_data(metric, start, end);
            let show_plot = has_data || guides;

            let markup = legend_markup(&data, metric, start, end, colors);
            legend.set_markup(&markup);
            legend.set_visible(!markup.is_empty());

            area.set_visible(show_plot);
            empty.set_visible(!has_data);
            if show_plot {
                area.queue_draw();
            }
            if !has_data {
                let mut message = if data.has_data(metric, start, end) {
                    "All sources with data for this graph are hidden.".to_string()
                } else {
                    empty_message(&data, metric, start, end)
                };
                if guides && metric.is_air() {
                    message.push_str(" The band is the requirement, not a reading.");
                }
                empty.set_text(&message);
            }
        }
    });

    area.set_draw_func({
        let data = data.clone();
        let kind = kind.clone();
        let hover = hover.clone();
        let geometry = geometry.clone();
        move |area, cr, width, height| {
            draw::draw(
                area,
                cr,
                width,
                height,
                &data,
                kind.get(),
                hover.get(),
                &geometry,
            );
        }
    });

    area.set_has_tooltip(true);
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion({
        let data = data.clone();
        let kind = kind.clone();
        let area = area.downgrade();
        let hover = hover.clone();
        let geometry = geometry.clone();
        move |_, x, _| {
            let Some(area) = area.upgrade() else {
                return;
            };
            let point = draw::point_at(&data, &geometry, x);
            hover.set(point);
            area.set_tooltip_text(Some(&draw::tooltip(&data, kind.get(), point)));
            area.queue_draw();
        }
    });
    motion.connect_leave({
        let area = area.downgrade();
        let hover = hover.clone();
        move |_| {
            let Some(area) = area.upgrade() else {
                return;
            };
            hover.set(None);
            area.queue_draw();
        }
    });
    area.add_controller(motion);

    card.append(&area);
    card.append(&empty);

    updates.borrow_mut().push(Rc::downgrade(&update));
    keep_alive(&card, update.clone());
    update();

    card
}

/// Keeps the panel's update closure alive as long as its card exists. The shared list only holds
/// weak references; the card owns the strong one.
fn keep_alive(card: &gtk::Box, update: Rc<dyn Fn()>) {
    card.connect_destroy(move |_| {
        let _ = &update;
    });
}

fn redraw_all(updates: &Updates) {
    let callbacks: Vec<Rc<dyn Fn()>> = updates.borrow().iter().filter_map(Weak::upgrade).collect();
    for callback in callbacks {
        callback();
    }
}

fn source_chip(
    label: &str,
    symbol: &str,
    color: gtk::gdk::RGBA,
    source: &'static str,
    data: Rc<ClimateData>,
    updates: Updates,
    save: Rc<dyn Fn()>,
) -> (gtk::ToggleButton, Rc<Cell<bool>>) {
    let chip = gtk::ToggleButton::new();
    chip.add_css_class("flat");
    chip.add_css_class("omagrow-source-chip");
    chip.set_tooltip_text(Some(&format!("Show or hide {label}")));
    let text = gtk::Label::new(None);
    text.set_markup(&format!(
        "<span foreground=\"{}\">{symbol}</span> {}",
        rgba_hex(color),
        gtk::glib::markup_escape_text(label)
    ));
    chip.set_child(Some(&text));

    let syncing = Rc::new(Cell::new(false));
    chip.set_active(true);
    chip.connect_toggled({
        let data = data.clone();
        let updates = updates.clone();
        let syncing = syncing.clone();
        let save = save.clone();
        move |button| {
            if syncing.get() {
                return;
            }
            let mut sources = data.sources.get();
            match source {
                "weather" => sources.weather = button.is_active(),
                "sensor" => sources.sensor = button.is_active(),
                "manual" => sources.manual = button.is_active(),
                _ => {}
            }
            data.sources.set(sources);
            save();
            redraw_all(&updates);
        }
    });

    (chip, syncing)
}

fn range_menu(
    data: Rc<ClimateData>,
    updates: Updates,
    save: Rc<dyn Fn()>,
) -> (gtk::MenuButton, Rc<dyn Fn()>) {
    let menu = gtk::MenuButton::new();
    menu.set_label(data.range.get().label());
    menu.add_css_class("flat");
    menu.set_tooltip_text(Some("Choose a time range"));

    let popover = gtk::Popover::new();
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);

    for candidate in data::RANGES {
        let button = gtk::Button::with_label(candidate.label());
        button.add_css_class("flat");
        button.set_halign(gtk::Align::Start);
        button.connect_clicked({
            let data = data.clone();
            let updates = updates.clone();
            let save = save.clone();
            let popover = popover.clone();
            move |_| {
                data.range.set(candidate);
                save();
                popover.popdown();
                redraw_all(&updates);
            }
        });
        list.append(&button);
    }

    popover.set_child(Some(&list));
    menu.set_popover(Some(&popover));

    let update = Rc::new({
        let data = data.clone();
        let menu = menu.clone();
        move || menu.set_label(data.range.get().label())
    });

    (menu, update)
}

fn view_menu(
    data: Rc<ClimateData>,
    updates: Updates,
    save: Rc<dyn Fn()>,
) -> (gtk::MenuButton, Rc<dyn Fn()>) {
    let menu = gtk::MenuButton::new();
    menu.set_label(data.view.get().label());
    menu.add_css_class("flat");
    menu.set_tooltip_text(Some("How each day is drawn above 7 days"));

    let popover = gtk::Popover::new();
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);

    for candidate in [
        DayNightView::Combined,
        DayNightView::Both,
        DayNightView::Day,
        DayNightView::Night,
    ] {
        let button = gtk::Button::with_label(candidate.label());
        button.add_css_class("flat");
        button.set_halign(gtk::Align::Start);
        button.connect_clicked({
            let data = data.clone();
            let updates = updates.clone();
            let save = save.clone();
            let popover = popover.clone();
            move |_| {
                data.view.set(candidate);
                save();
                popover.popdown();
                redraw_all(&updates);
            }
        });
        list.append(&button);
    }

    popover.set_child(Some(&list));
    menu.set_popover(Some(&popover));

    let update = Rc::new({
        let data = data.clone();
        let menu = menu.clone();
        move || menu.set_label(data.view.get().label())
    });

    (menu, update)
}

fn source_here(data: &ClimateData, source: &str, start: Date, end: Date) -> bool {
    match source {
        "weather" => data::METRICS
            .iter()
            .any(|metric| data.has_weather(*metric, start, end)),
        "sensor" | "manual" => [Metric::Temperature, Metric::Humidity]
            .iter()
            .any(|metric| data.has_readings(source, *metric, start, end)),
        _ => false,
    }
}

fn legend_markup(
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    colors: draw::GraphColors,
) -> String {
    let mut parts = Vec::new();
    let sources = data.sources.get();

    if sources.weather && data.has_weather(metric, start, end) {
        parts.push(swatch(colors.weather, "━", "weather"));
    }
    if sources.sensor && data.has_readings("sensor", metric, start, end) {
        parts.push(swatch(colors.sensor, "━", "sensor"));
    }
    if sources.manual && data.has_readings("manual", metric, start, end) {
        parts.push(swatch(colors.manual, "●", "manual"));
    }

    if metric.is_air() && data.has_guides(metric, start, end) {
        let view = data.view.get();
        let hourly = data.range.get().hourly();
        if (hourly || view.draws_day()) && has_part(data, metric, DayPart::Day, start, end) {
            parts.push(swatch(colors.day_opt, "▬", "day optimum"));
        }
        if (hourly || view.draws_night()) && has_part(data, metric, DayPart::Night, start, end) {
            parts.push(swatch(colors.night_opt, "▬", "night optimum"));
        }
        if has_survival(data, metric, start, end, view, hourly) {
            parts.push(swatch(colors.survival, "┄", "survival"));
        }
    }

    parts.join("   ")
}

fn has_part(data: &ClimateData, metric: Metric, part: DayPart, start: Date, end: Date) -> bool {
    data.guides.for_metric(metric).is_some_and(|guides| {
        let spans = match part {
            DayPart::Day => &guides.day,
            DayPart::Night => &guides.night,
        };
        spans
            .iter()
            .any(|span| span.end >= start && span.start <= end && !span.reference.is_empty())
    })
}

fn has_survival(
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    view: DayNightView,
    hourly: bool,
) -> bool {
    data.guides.for_metric(metric).is_some_and(|guides| {
        let mut parts = Vec::new();
        if hourly || view.draws_day() {
            parts.push(&guides.day);
        }
        if hourly || view.draws_night() {
            parts.push(&guides.night);
        }
        parts.into_iter().flatten().any(|span| {
            span.end >= start
                && span.start <= end
                && (span.reference.survival_min.is_some() || span.reference.survival_max.is_some())
        })
    })
}

fn swatch(color: gtk::gdk::RGBA, symbol: &str, label: &str) -> String {
    format!(
        "<span foreground=\"{}\">{symbol}</span> {}",
        rgba_hex(color),
        gtk::glib::markup_escape_text(label)
    )
}

fn rgba_hex(color: gtk::gdk::RGBA) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        (color.red() * 255.0).round() as u8,
        (color.green() * 255.0).round() as u8,
        (color.blue() * 255.0).round() as u8
    )
}

fn empty_message(data: &ClimateData, metric: Metric, start: Date, end: Date) -> String {
    let mut message = if start == end && start == today() && data.weather_visible(start) {
        "No weather for today yet.".to_string()
    } else if start == end
        && data.space.is_none()
        && data
            .placement_at(start)
            .is_some_and(|placement| placement.space_kind != "outdoor")
    {
        "This planting is indoors on this day; weather is not shown here.".to_string()
    } else {
        format!("No {} data in this range.", metric.label().to_lowercase())
    };

    if let Some(note) = data.empty_note(start, end) {
        message.push(' ');
        message.push_str(&note);
    }

    message
}

#[cfg(test)]
mod tests {
    use super::{GRID_FROM, shows_grid};

    #[test]
    fn a_narrow_section_keeps_one_plot_with_tabs() {
        // About 672 px on a one-column page, about 1152 px on a wide one.
        assert!(!shows_grid(672));
        assert!(!shows_grid(GRID_FROM - 1));
        assert!(shows_grid(GRID_FROM));
        assert!(shows_grid(1152));
    }

    #[test]
    fn an_unmeasured_section_starts_with_the_tabs() {
        assert!(!shows_grid(0));
    }
}
