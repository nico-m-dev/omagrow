//! The planting timeline and month calendar. Both are drawn from the derived `Schedule`; nothing
//! here writes to the database.

use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::cairo;
use gtk::prelude::*;

use crate::calendar::{self, Date};
use crate::planting::{Planting, PlantingState};
use crate::schedule::{Milestone, Schedule};

use super::shared::graph_style::{Align, Colors, paint, rounded_rect, short_date, text_width};

const DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// The timeline card and the one action the page needs when its selected day changes.
pub(super) struct TimelineCard {
    pub widget: gtk::Widget,
    pub update: Rc<dyn Fn(Date)>,
}

/// How much of the grow the timeline shows. The selected day stays in the middle at every level,
/// so zooming only changes the number of days on either side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TimelineZoom {
    Year,
    HalfYear,
    Quarter,
}

impl TimelineZoom {
    /// Days before the selected day, and after it, at this level.
    fn half_span(self) -> i64 {
        match self {
            Self::Year => 182,
            Self::HalfYear => 91,
            Self::Quarter => 45,
        }
    }

    fn window(self, selected: Date) -> (Date, Date) {
        let span = self.half_span();
        (selected.add_days(-span), selected.add_days(span + 1))
    }

    fn zoom_out(self) -> Option<Self> {
        match self {
            Self::Year => None,
            Self::HalfYear => Some(Self::Year),
            Self::Quarter => Some(Self::HalfYear),
        }
    }

    fn zoom_in(self) -> Option<Self> {
        match self {
            Self::Year => Some(Self::HalfYear),
            Self::HalfYear => Some(Self::Quarter),
            Self::Quarter => None,
        }
    }

    /// The months that get a tick: one a quarter on a year, one a month closer in.
    fn tick_stride(self) -> u32 {
        match self {
            Self::Year => 3,
            Self::HalfYear | Self::Quarter => 1,
        }
    }

    /// The first of each ticked month, strictly inside the shown range.
    fn ticks(self, start: Date, end: Date) -> Vec<Date> {
        let stride = self.tick_stride();
        let (mut year, mut month) = (start.year, start.month);
        let mut ticks = Vec::new();

        while let Some(first) = Date::new(year, month, 1) {
            if first > end {
                break;
            }
            if first > start && (month - 1) % stride == 0 {
                ticks.push(first);
            }
            if month == 12 {
                year += 1;
                month = 1;
            } else {
                month += 1;
            }
        }

        ticks
    }
}

pub(super) fn timeline(
    schedule: &Schedule,
    planting: &Planting,
    today: Date,
    diary: &[CalendarEvent],
    zoom: &Rc<Cell<TimelineZoom>>,
    on_stage_start: Rc<dyn Fn(i64, Date)>,
) -> TimelineCard {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    card.add_css_class("omagrow-timeline");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("Grow timeline"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("heading");

    let status = gtk::Label::new(Some(&timeline_status(schedule, planting, today, today)));
    status.add_css_class("omagrow-axis");
    status.set_halign(gtk::Align::End);

    // The zoom is a pair of flat buttons, like the calendar's month arrows. A label for the
    // level would crowd the header; the dates under the timeline show the range.
    let zoom_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    zoom_buttons.add_css_class("linked");
    let zoom_out_button = gtk::Button::from_icon_name("zoom-out-symbolic");
    zoom_out_button.add_css_class("flat");
    zoom_out_button.set_tooltip_text(Some("Show more of the grow"));
    let zoom_in_button = gtk::Button::from_icon_name("zoom-in-symbolic");
    zoom_in_button.add_css_class("flat");
    zoom_in_button.set_tooltip_text(Some("Show less of the grow"));
    zoom_buttons.append(&zoom_out_button);
    zoom_buttons.append(&zoom_in_button);
    zoom_buttons.set_visible(!schedule.stages.is_empty());

    header.append(&title);
    header.append(&status);
    header.append(&zoom_buttons);
    card.append(&header);

    let selected = Rc::new(Cell::new(today));

    let area = gtk::DrawingArea::new();
    area.set_height_request(118);
    area.set_hexpand(true);
    area.set_draw_func({
        let schedule = schedule.clone();
        let planting = planting.clone();
        let selected = selected.clone();
        let zoom = zoom.clone();
        move |area, cr, width, height| {
            draw_timeline(
                area,
                cr,
                width,
                height,
                &schedule,
                &planting,
                today,
                selected.get(),
                zoom.get(),
            );
        }
    });

    if schedule.stages.is_empty() {
        let empty = gtk::Label::new(Some(if planting.state(today) == PlantingState::Finished {
            "No dated stages for this planting."
        } else {
            "Add stages and their durations to the plant profile to see a timeline here."
        }));
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        empty.add_css_class("dim-label");
        card.append(&empty);
    } else {
        card.append(&area);
    }

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let next = if planting.ended_on.is_none() {
        next_with_diary(schedule, diary, today)
    } else {
        None
    };
    let next = gtk::Label::new(Some(&next_text(next.as_ref(), today)));
    next.set_xalign(0.0);
    next.set_hexpand(true);
    next.set_wrap(true);
    next.add_css_class("omagrow-axis");

    footer.append(&next);
    card.append(&footer);

    // The action that changes the blocks lives with the blocks, not in the diary form. A stage
    // start is logged for the day selected in the calendar.
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let source = gtk::Label::new(Some("Estimated · logged starts marked"));
    source.set_xalign(0.0);
    source.set_hexpand(true);
    source.add_css_class("omagrow-axis");

    let stage_start = gtk::Button::with_label("Stage started…");
    stage_start.add_css_class("flat");
    stage_start.set_valign(gtk::Align::Center);
    stage_start.set_tooltip_text(Some("Log the start of a stage on the selected day"));
    stage_start.set_visible(!schedule.stages.is_empty());
    stage_start.connect_clicked({
        let schedule = schedule.clone();
        let selected = selected.clone();
        let on_stage_start = on_stage_start.clone();
        move |_| {
            if let Some(stage) = stage_on(&schedule, selected.get()) {
                on_stage_start(stage, selected.get());
            }
        }
    });

    actions.append(&source);
    actions.append(&stage_start);
    card.append(&actions);

    if !schedule.undated_stages.is_empty() {
        let note = gtk::Label::new(Some(&format!(
            "Not dated yet: {} has no duration",
            schedule.undated_stages.join(", ")
        )));
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.add_css_class("omagrow-axis");
        card.append(&note);
    }

    if !schedule.unknown.is_empty() {
        let note = gtk::Label::new(Some(&format!("Notes: {}", schedule.unknown.join("; "))));
        note.set_wrap(true);
        note.set_xalign(0.0);
        note.add_css_class("omagrow-axis");
        card.append(&note);
    }

    let update: Rc<dyn Fn(Date)> = Rc::new({
        let status = status.clone();
        let selected = selected.clone();
        let area = area.clone();
        let schedule = schedule.clone();
        let planting = planting.clone();
        let zoom = zoom.clone();
        move |date| {
            selected.set(date);
            status.set_text(&timeline_status(&schedule, &planting, today, date));
            area.set_tooltip_text(Some(&timeline_tooltip(&schedule, date, today, zoom.get())));
            area.queue_draw();
        }
    });

    // Zooming keeps the selected day in the middle: the buttons only change the span and the
    // tooltip, and turn themselves off at the two ends.
    let apply_zoom: Rc<dyn Fn()> = Rc::new({
        let zoom = zoom.clone();
        let area = area.clone();
        let schedule = schedule.clone();
        let selected = selected.clone();
        let zoom_out_button = zoom_out_button.clone();
        let zoom_in_button = zoom_in_button.clone();
        move || {
            let level = zoom.get();
            zoom_out_button.set_sensitive(level.zoom_out().is_some());
            zoom_in_button.set_sensitive(level.zoom_in().is_some());
            area.set_tooltip_text(Some(&timeline_tooltip(
                &schedule,
                selected.get(),
                today,
                level,
            )));
            area.queue_draw();
        }
    });

    zoom_out_button.connect_clicked({
        let zoom = zoom.clone();
        let apply_zoom = apply_zoom.clone();
        move |_| {
            if let Some(level) = zoom.get().zoom_out() {
                zoom.set(level);
                apply_zoom();
            }
        }
    });

    zoom_in_button.connect_clicked({
        let zoom = zoom.clone();
        let apply_zoom = apply_zoom.clone();
        move |_| {
            if let Some(level) = zoom.get().zoom_in() {
                zoom.set(level);
                apply_zoom();
            }
        }
    });

    apply_zoom();

    TimelineCard {
        widget: card.upcast(),
        update,
    }
}

/// The stage the selected day falls in, or the first one, as the form's default.
fn stage_on(schedule: &Schedule, date: Date) -> Option<i64> {
    schedule
        .stages
        .iter()
        .rev()
        .find(|window| window.likely_start <= date)
        .or_else(|| schedule.stages.first())
        .map(|window| window.stage.id)
}

fn timeline_status(
    schedule: &Schedule,
    planting: &Planting,
    today: Date,
    selected: Date,
) -> String {
    if selected != today {
        return selected_status(schedule, planting, today, selected);
    }

    match planting.state(today) {
        PlantingState::Planned => {
            let days = today.days_until(planting.started_on);
            match days {
                0 => "Starts today".to_string(),
                1 => "Starts tomorrow".to_string(),
                days => format!("Starts in {days} days"),
            }
        }
        PlantingState::Finished => planting
            .ended_on
            .map(|date| format!("Finished {}", short_date(date, today)))
            .unwrap_or_else(|| "Finished".to_string()),
        PlantingState::Growing => {
            let day = planting.day_number(today);
            match schedule
                .current_stage
                .and_then(|index| schedule.stages.get(index))
            {
                Some(window) => format!(
                    "Day {day} · {}{}",
                    window.stage.label,
                    if window.actual_start {
                        ""
                    } else {
                        " (estimated)"
                    }
                ),
                None => format!("Day {day} · between stages"),
            }
        }
    }
}

/// The day line for a date that is not today: the planting's own day number and the stage that
/// falls on that date.
fn selected_status(
    schedule: &Schedule,
    planting: &Planting,
    today: Date,
    selected: Date,
) -> String {
    if selected < planting.started_on {
        return format!("Not started on {}", short_date(selected, today));
    }
    if let Some(end) = planting.ended_on
        && selected > end
    {
        return format!("Finished {}", short_date(end, today));
    }

    let day = planting.day_number(selected);
    match schedule.stage_on(selected) {
        Some(window) => format!(
            "Day {day} · {}{}",
            window.stage.label,
            if window.actual_start {
                ""
            } else {
                " (estimated)"
            }
        ),
        None => format!("Day {day} · between stages"),
    }
}

/// The next thing to do: the earliest of the schedule's own milestone and a diary entry dated
/// today or later. A finished planting has no next.
fn next_with_diary(schedule: &Schedule, diary: &[CalendarEvent], today: Date) -> Option<Milestone> {
    let from_diary = diary
        .iter()
        .filter(|event| event.date >= today && event.kind != "stage")
        .min_by_key(|event| event.date)
        .map(|event| Milestone {
            date: event.date,
            end: event.end,
            kind: event.kind.clone(),
            label: event.label.clone(),
        });

    match (&schedule.next, from_diary) {
        (Some(next), Some(diary)) if diary.date < next.date => Some(diary),
        (Some(next), _) => Some(next.clone()),
        (None, diary) => diary,
    }
}

fn next_text(next: Option<&Milestone>, today: Date) -> String {
    let Some(next) = next else {
        return "No upcoming milestones".to_string();
    };

    if next.date <= today && next.last_day() >= today {
        return if next.is_window() {
            format!(
                "Now: {} · until {}",
                next.label,
                short_date(next.last_day(), today)
            )
        } else {
            format!("Today: {}", next.label)
        };
    }

    let days = today.days_until(next.date);
    match days {
        0 => format!("Next: {} · today", next.label),
        1 => format!("Next: {} · tomorrow", next.label),
        days => format!("Next: {} · in {days} days", next.label),
    }
}

/// The stages that fall in the shown range, for the card's tooltip.
fn timeline_tooltip(
    schedule: &Schedule,
    selected: Date,
    today: Date,
    zoom: TimelineZoom,
) -> String {
    let (start, end) = zoom.window(selected);
    let lines: Vec<String> = schedule
        .stages
        .iter()
        .filter(|window| window.latest_end >= start && window.earliest_start <= end)
        .map(|window| {
            format!(
                "{}: {} – {}{}",
                window.stage.label,
                short_date(window.likely_start, today),
                short_date(window.likely_end, today),
                if window.actual_start {
                    " (started)"
                } else {
                    ""
                }
            )
        })
        .collect();

    if lines.is_empty() {
        "No dated stages in this view".to_string()
    } else {
        lines.join("\n")
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_timeline(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
    schedule: &Schedule,
    planting: &Planting,
    today: Date,
    selected: Date,
    zoom: TimelineZoom,
) {
    let colors = Colors::load(area);
    let w = width as f64;
    let h = height as f64;
    let left = 8.0;
    let right = 8.0;
    let top = 26.0;
    // The selected day is the centre at every zoom level; the calendar moves it, the zoom buttons
    // change how many days are shown on either side.
    let (start, end) = zoom.window(selected);

    let total = start.days_until(end).max(1) as f64;
    let usable = (w - left - right).max(1.0);
    let x = |date: Date| left + (start.days_until(date) as f64 / total).clamp(0.0, 1.0) * usable;

    let bar_y = top + 26.0;
    let bar_h = 24.0;

    paint(cr, colors.fg, 0.10);
    cr.set_line_width(1.0);
    cr.move_to(left, bar_y + bar_h / 2.0);
    cr.line_to(left + usable, bar_y + bar_h / 2.0);
    let _ = cr.stroke();

    for date in zoom.ticks(start, end) {
        let tick = x(date);
        // The edges already carry the start and end dates.
        if tick - left < 44.0 || (left + usable) - tick < 44.0 {
            continue;
        }
        paint(cr, colors.fg, 0.06);
        cr.set_line_width(1.0);
        cr.move_to(tick, top - 4.0);
        cr.line_to(tick, bar_y + bar_h + 4.0);
        let _ = cr.stroke();

        draw_text(
            cr,
            &short_date(date, today),
            tick,
            h - 14.0,
            12.0,
            TextStyle {
                color: colors.fg,
                // The selected day already carries its date at the centre.
                alpha: if (tick - (left + usable / 2.0)).abs() < 46.0 {
                    0.0
                } else {
                    0.5
                },
                size: 8.0,
                max_width: 60.0,
                align: Align::Center,
            },
        );
    }

    for window in &schedule.stages {
        if window.latest_end < start || window.earliest_start > end {
            continue;
        }
        let color = stage_color(&window.stage.kind, &colors);
        let x1 = x(window.earliest_start.max(start));
        let x2 = x(window.latest_end.min(end)).max(x1 + 2.0);
        paint(cr, color, 0.22);
        rounded_rect(cr, x1, bar_y - 3.0, x2 - x1, bar_h + 6.0, 7.0);
        let _ = cr.fill();

        let core_x = x(window.likely_start.max(start));
        let core_end = x(window.likely_end.min(end)).max(core_x + 2.0);
        paint(cr, color, 0.92);
        rounded_rect(cr, core_x, bar_y, core_end - core_x, bar_h, 6.0);
        let _ = cr.fill();

        if window.actual_start && start <= window.likely_start && window.likely_start <= end {
            paint(cr, colors.bg, 0.9);
            cr.arc(core_x, bar_y + bar_h / 2.0, 2.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
        }

        let label = if core_end - core_x > 58.0 {
            window.stage.label.clone()
        } else if core_end - core_x > 30.0 {
            short_stage_label(&window.stage.kind).to_string()
        } else {
            String::new()
        };

        if !label.is_empty() {
            draw_text(
                cr,
                &label,
                (core_x + core_end) / 2.0,
                bar_y,
                bar_h,
                TextStyle {
                    color: colors.bg,
                    alpha: 0.95,
                    size: 9.0,
                    max_width: core_end - core_x - 6.0,
                    align: Align::Center,
                },
            );
        }
    }

    // The selected day is the centre and always gets the accent. Today stays visible but muted
    // when it falls in the same year.
    let mut markers: Vec<(Date, &str, gtk::gdk::RGBA, f64)> = Vec::new();
    if selected == today {
        markers.push((today, "TODAY", colors.accent, 0.9));
    } else {
        markers.push((selected, "SELECTED", colors.accent, 0.9));
        if planting.state(today) == PlantingState::Growing && start <= today && today <= end {
            markers.push((today, "TODAY", colors.fg, 0.5));
        }
    }
    if planting.state(today) == PlantingState::Finished
        && let Some(date) = planting.ended_on
        && date != selected
        && start <= date
        && date <= end
    {
        markers.push((date, "ENDED", colors.fg, 0.55));
    }

    // The selected day wins when two markers sit too close for their labels.
    markers.sort_by_key(|(_, label, ..)| if *label == "SELECTED" { 0 } else { 1 });

    let mut written: Vec<f64> = Vec::new();
    for (date, label, color, alpha) in markers {
        let mx = x(date);
        paint(cr, color, alpha);
        cr.set_line_width(1.0);
        cr.move_to(mx, top - 6.0);
        cr.line_to(mx, bar_y + bar_h + 4.0);
        let _ = cr.stroke();
        paint(cr, color, alpha);
        cr.arc(mx, top - 6.0, 3.0, 0.0, 2.0 * PI);
        let _ = cr.fill();

        if written.iter().any(|x| (x - mx).abs() < 70.0) {
            continue;
        }
        written.push(mx);

        let (label_align, label_x) = if mx < left + 36.0 {
            (Align::Left, mx)
        } else if mx > left + usable - 36.0 {
            (Align::Right, mx)
        } else {
            (Align::Center, mx)
        };

        draw_text(
            cr,
            label,
            label_x,
            top - 22.0,
            10.0,
            TextStyle {
                color,
                alpha: alpha.max(0.7),
                size: 8.0,
                max_width: 64.0,
                align: label_align,
            },
        );
        draw_text(
            cr,
            &short_date(date, today),
            label_x,
            top - 10.0,
            10.0,
            TextStyle {
                color,
                alpha: alpha.max(0.7),
                size: 8.0,
                max_width: 64.0,
                align: label_align,
            },
        );
    }

    draw_text(
        cr,
        &short_date(start, today),
        left,
        h - 14.0,
        12.0,
        TextStyle {
            color: colors.fg,
            alpha: 0.55,
            size: 8.0,
            max_width: 0.0,
            align: Align::Left,
        },
    );
    draw_text(
        cr,
        &short_date(end, today),
        left + usable,
        h - 14.0,
        12.0,
        TextStyle {
            color: colors.fg,
            alpha: 0.55,
            size: 8.0,
            max_width: 0.0,
            align: Align::Right,
        },
    );
}

fn draw_text(cr: &cairo::Context, text: &str, x: f64, top: f64, height: f64, style: TextStyle) {
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(style.size);

    let text = fit_text(cr, text, style.max_width);
    if text.is_empty() {
        return;
    }

    let Ok(extents) = cr.text_extents(&text) else {
        return;
    };
    let x = match style.align {
        Align::Left => x,
        Align::Center => x - extents.width() / 2.0,
        Align::Right => x - extents.width(),
    };
    let descent = cr.font_extents().map(|font| font.descent()).unwrap_or(0.0);
    let baseline = top + (height + extents.height()) / 2.0 - descent;

    paint(cr, style.color, style.alpha);
    cr.move_to(x, baseline);
    let _ = cr.show_text(&text);
}

fn fit_text(cr: &cairo::Context, text: &str, max_width: f64) -> String {
    if max_width <= 0.0 || text_width(cr, text) <= max_width {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();

    while keep > 1 {
        keep -= 1;
        let candidate: String = chars[..keep].iter().collect::<String>() + "…";
        if text_width(cr, &candidate) <= max_width {
            return candidate;
        }
    }

    String::new()
}

#[derive(Clone, Copy)]
struct TextStyle {
    color: gtk::gdk::RGBA,
    alpha: f64,
    size: f64,
    max_width: f64,
    align: Align,
}

fn stage_color(kind: &str, colors: &Colors) -> gtk::gdk::RGBA {
    match kind {
        "germination" | "seedling" | "establishment" | "juvenile" => colors.success,
        "flowering" | "bloom" => colors.warning,
        "fruiting" | "ripening" | "harvest" => colors.error,
        "dormancy" | "senescence" => colors.fg,
        _ => colors.accent,
    }
}

fn short_stage_label(kind: &str) -> &'static str {
    match kind {
        "germination" => "Germ.",
        "seedling" => "Seedl.",
        "establishment" => "Estab.",
        "vegetative" => "Veg.",
        "flowering" => "Flower",
        "fruiting" => "Fruit",
        "ripening" => "Ripen",
        "harvest" => "Harvest",
        "dormancy" => "Dormant",
        "senescence" => "Decline",
        "juvenile" => "Juven.",
        "mature" => "Mature",
        _ => "Stage",
    }
}

pub(super) struct CalendarState {
    pub month: Cell<(i32, u32)>,
    pub selected: Cell<Date>,
    initialized: Cell<bool>,
}

impl CalendarState {
    pub fn new(today: Date) -> Self {
        Self {
            month: Cell::new((today.year, today.month)),
            selected: Cell::new(today),
            initialized: Cell::new(false),
        }
    }

    /// Select a day and show its month. The page decides what follows.
    pub fn go_to(&self, date: Date) {
        self.month.set((date.year, date.month));
        self.selected.set(date);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CalendarEvent {
    pub date: Date,
    pub end: Option<Date>,
    pub kind: String,
    pub label: String,
}

/// The calendar card. `refresh` rebuilds the grid after the month changed from outside (the
/// page's "back to today" button); a day click updates the selection in place.
pub(super) struct CalendarCard {
    pub widget: gtk::Widget,
    pub refresh: Rc<dyn Fn()>,
}

pub(super) fn calendar(
    schedule: &Schedule,
    planting: &Planting,
    today: Date,
    diary: &[CalendarEvent],
    state: &Rc<CalendarState>,
    on_select: Rc<dyn Fn(Date)>,
) -> CalendarCard {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    card.add_css_class("omagrow-calendar-card");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let title = gtk::Label::new(Some("Calendar"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("heading");

    let previous = gtk::Button::from_icon_name("go-previous-symbolic");
    previous.add_css_class("flat");
    previous.set_tooltip_text(Some("Previous month"));

    let month = gtk::Label::new(None);
    month.add_css_class("omagrow-calendar-month");
    month.set_width_chars(16);

    let next = gtk::Button::from_icon_name("go-next-symbolic");
    next.add_css_class("flat");
    next.set_tooltip_text(Some("Next month"));

    let today_button = gtk::Button::with_label("Today");
    today_button.add_css_class("flat");
    today_button.add_css_class("omagrow-calendar-today");

    header.append(&title);
    header.append(&previous);
    header.append(&month);
    header.append(&next);
    header.append(&today_button);
    card.append(&header);

    let grid = gtk::Grid::builder()
        .column_homogeneous(true)
        .row_homogeneous(true)
        .column_spacing(2)
        .row_spacing(2)
        .build();
    card.append(&grid);

    let detail = gtk::Label::new(None);
    detail.set_wrap(true);
    detail.set_xalign(0.0);
    detail.add_css_class("omagrow-axis");
    card.append(&detail);

    if !state.initialized.replace(true) {
        let initial = match planting.state(today) {
            PlantingState::Finished => planting.ended_on.unwrap_or(today),
            PlantingState::Planned => planting.started_on,
            PlantingState::Growing => today,
        };
        state.month.set((initial.year, initial.month));
        state.selected.set(initial);
    }

    let mut events: Vec<CalendarEvent> = schedule
        .milestones
        .iter()
        .map(|milestone| CalendarEvent {
            date: milestone.date,
            end: milestone.end,
            kind: milestone.kind.clone(),
            label: milestone.label.clone(),
        })
        .collect();
    events.extend(diary.iter().cloned());
    events.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.kind.cmp(&b.kind)));
    let events = Rc::new(events);

    // A day outside the grow is not a day of this planting: it cannot be selected. A growing
    // planting has no end yet, so its future days stay available.
    let started_on = planting.started_on;
    let ended_on = planting.ended_on;
    let selectable =
        move |date: Date| date >= started_on && ended_on.is_none_or(|ended| date <= ended);
    today_button.set_sensitive(selectable(today));

    let buttons: Rc<RefCell<Vec<(Date, gtk::Button)>>> = Rc::new(RefCell::new(Vec::new()));

    // Selection only moves a style class and the detail line, so clicking never rebuilds the grid
    // and the clicked button keeps focus.
    let apply: Rc<dyn Fn()> = Rc::new({
        let buttons = buttons.clone();
        let detail = detail.clone();
        let state = state.clone();
        let schedule = schedule.clone();
        let events = events.clone();

        move || {
            let selected = state.selected.get();
            for (date, button) in buttons.borrow().iter() {
                if *date == selected {
                    button.add_css_class("selected");
                } else {
                    button.remove_css_class("selected");
                }
            }
            detail.set_text(&day_detail(&schedule, &events, selected, today));
        }
    });

    let render: Rc<dyn Fn()> = Rc::new({
        let grid = grid.clone();
        let month = month.clone();
        let state = state.clone();
        let events = events.clone();
        let buttons = buttons.clone();
        let apply = apply.clone();
        let on_select = on_select.clone();

        move || {
            let had_focus = grid
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
                .and_then(|window| gtk::prelude::GtkWindowExt::focus(&window))
                .is_some_and(|focus| focus.is_ancestor(&grid));

            while let Some(child) = grid.first_child() {
                grid.remove(&child);
            }
            buttons.borrow_mut().clear();

            let (year, month_number) = state.month.get();
            month.set_text(&format!("{} {year}", calendar::month_name(month_number)));

            for (index, name) in DAY_NAMES.iter().enumerate() {
                let label = gtk::Label::new(Some(name));
                label.add_css_class("omagrow-axis");
                label.set_halign(gtk::Align::Center);
                grid.attach(&label, index as i32, 0, 1, 1);
            }

            let Some(first) = Date::new(year, month_number, 1) else {
                return;
            };
            let offset = first.weekday_from_monday() as i64;

            for index in 0..42 {
                let date = first.add_days(index - offset);
                let weak = date.month != month_number;
                let events_on: Vec<&CalendarEvent> =
                    events.iter().filter(|event| event.date == date).collect();

                let button = gtk::Button::new();
                button.add_css_class("flat");
                button.add_css_class("omagrow-calendar-day");
                if weak {
                    button.add_css_class("outside");
                }
                if date == today {
                    button.add_css_class("today");
                }
                button.set_sensitive(selectable(date));

                let inner = gtk::Box::new(gtk::Orientation::Vertical, 2);
                inner.set_margin_top(3);
                inner.set_margin_bottom(3);
                inner.set_margin_start(5);
                inner.set_margin_end(5);

                let number = gtk::Label::new(Some(&date.day.to_string()));
                number.set_xalign(0.0);
                number.add_css_class("omagrow-calendar-number");
                inner.append(&number);

                let dots = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                dots.set_halign(gtk::Align::Start);
                for event in events_on.iter().take(3) {
                    let dot = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    dot.set_size_request(5, 5);
                    dot.add_css_class("omagrow-dot");
                    dot.add_css_class(dot_class(&event.kind));
                    dots.append(&dot);
                }
                inner.append(&dots);

                button.set_child(Some(&inner));
                button.set_size_request(-1, 40);

                if !events_on.is_empty() {
                    let labels: Vec<&str> =
                        events_on.iter().map(|event| event.label.as_str()).collect();
                    button.set_tooltip_text(Some(&labels.join(" · ")));
                }

                button.connect_clicked({
                    let state = state.clone();
                    let apply = apply.clone();
                    let on_select = on_select.clone();
                    move |_| {
                        state.selected.set(date);
                        apply();
                        on_select(date);
                    }
                });

                buttons.borrow_mut().push((date, button.clone()));
                grid.attach(&button, (index % 7) as i32, (index / 7) as i32 + 1, 1, 1);
            }

            apply();

            if had_focus
                && let Some((_, button)) = buttons
                    .borrow()
                    .iter()
                    .find(|(date, _)| *date == state.selected.get())
            {
                button.grab_focus();
            }
        }
    });

    render();

    previous.connect_clicked({
        let state = state.clone();
        let render = render.clone();
        move |_| {
            let (year, month) = state.month.get();
            let (year, month) = if month == 1 {
                (year - 1, 12)
            } else {
                (year, month - 1)
            };
            state.month.set((year, month));
            render();
        }
    });

    next.connect_clicked({
        let state = state.clone();
        let render = render.clone();
        move |_| {
            let (year, month) = state.month.get();
            let (year, month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            state.month.set((year, month));
            render();
        }
    });

    today_button.connect_clicked({
        let state = state.clone();
        let render = render.clone();
        let on_select = on_select.clone();
        move |_| {
            state.go_to(today);
            render();
            on_select(today);
        }
    });

    CalendarCard {
        widget: card.upcast(),
        refresh: render,
    }
}

/// The line under the calendar: the selected day, its milestones and its likely stage.
fn day_detail(
    schedule: &Schedule,
    events: &[CalendarEvent],
    selected: Date,
    today: Date,
) -> String {
    let events_on: Vec<&CalendarEvent> = events
        .iter()
        .filter(|event| event.date == selected)
        .collect();

    let mut text = short_date(selected, today);
    if events_on.is_empty() {
        text.push_str(" · No milestones");
    } else {
        let labels: Vec<String> = events_on
            .iter()
            .map(|event| {
                if let Some(end) = event.end {
                    format!("{} (until {})", event.label, short_date(end, today))
                } else {
                    event.label.clone()
                }
            })
            .collect();
        text.push_str(" · ");
        text.push_str(&labels.join(" · "));
    }

    if let Some(window) = schedule.stage_on(selected) {
        text.push_str(&format!(
            " · {}{}",
            window.stage.label,
            if window.actual_start {
                ""
            } else {
                " (estimated)"
            }
        ));
    }

    text
}

fn dot_class(kind: &str) -> &'static str {
    match kind {
        "stage" | "planted" => "stage",
        "sow" | "sow_indoor" | "sow_outdoor" | "sown_indoor" | "sown_outdoor" => "sow",
        "transplant" | "transplanted" => "transplant",
        "harvest" | "fruiting" | "ripening" => "harvest",
        "flowering" | "bloom" => "flower",
        "dormancy" | "senescence" => "dormant",
        "pest" | "disease" => "problem",
        "note" | "photo" | "other" => "note",
        _ => "care",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CalendarEvent, TimelineZoom, dot_class, next_text, next_with_diary, selected_status,
        short_date, short_stage_label, timeline_tooltip,
    };
    use crate::calendar::Date;
    use crate::plant_data::Stage;
    use crate::planting::Planting;
    use crate::schedule::{Milestone, Schedule, StageWindow};

    #[test]
    fn formats_short_dates_and_stage_names() {
        assert_eq!(
            short_date(
                Date::new(2027, 5, 1).unwrap(),
                Date::new(2027, 10, 30).unwrap()
            ),
            "1 May"
        );
        assert_eq!(
            short_date(
                Date::new(2028, 5, 1).unwrap(),
                Date::new(2027, 10, 30).unwrap()
            ),
            "1 May 2028"
        );
        assert_eq!(short_stage_label("germination"), "Germ.");
        assert_eq!(short_stage_label("vegetative"), "Veg.");
        assert_eq!(short_stage_label("unknown"), "Stage");
    }

    #[test]
    fn maps_milestones_to_dot_colours() {
        assert_eq!(dot_class("stage"), "stage");
        assert_eq!(dot_class("sown_indoor"), "sow");
        assert_eq!(dot_class("sow_indoor"), "sow");
        assert_eq!(dot_class("sow"), "sow");
        assert_eq!(dot_class("transplanted"), "transplant");
        assert_eq!(dot_class("transplant"), "transplant");
        assert_eq!(dot_class("harvest"), "harvest");
        assert_eq!(dot_class("flowering"), "flower");
        assert_eq!(dot_class("feed"), "care");
    }

    #[test]
    fn describes_the_next_milestone() {
        let today = Date::new(2027, 5, 1).unwrap();
        let mut schedule = Schedule {
            stages: Vec::new(),
            milestones: Vec::new(),
            current_stage: None,
            next: None,
            undated_stages: Vec::new(),
            unknown: Vec::new(),
            span_start: today,
            span_end: today,
        };

        assert_eq!(
            next_text(schedule.next.as_ref(), today),
            "No upcoming milestones"
        );

        schedule.next = Some(Milestone {
            date: Date::new(2027, 5, 12).unwrap(),
            end: None,
            kind: "harvest".into(),
            label: "Harvest".into(),
        });
        assert_eq!(
            next_text(schedule.next.as_ref(), today),
            "Next: Harvest · in 11 days"
        );

        schedule.next = Some(Milestone {
            date: Date::new(2027, 4, 28).unwrap(),
            end: Some(Date::new(2027, 5, 4).unwrap()),
            kind: "harvest".into(),
            label: "Harvest".into(),
        });
        assert_eq!(
            next_text(schedule.next.as_ref(), today),
            "Now: Harvest · until 4 May"
        );
    }

    #[test]
    fn a_diary_entry_can_be_the_next_thing() {
        let today = Date::new(2027, 5, 1).unwrap();
        let mut schedule = Schedule {
            stages: Vec::new(),
            milestones: Vec::new(),
            current_stage: None,
            next: None,
            undated_stages: Vec::new(),
            unknown: Vec::new(),
            span_start: today,
            span_end: today,
        };
        let event = |day: Date, label: &str| CalendarEvent {
            date: day,
            end: None,
            kind: "water".into(),
            label: label.into(),
        };

        let past = event(today.add_days(-1), "Watered");
        let soon = event(today.add_days(1), "Watered");
        assert_eq!(
            next_with_diary(&schedule, &[past.clone(), soon.clone()], today)
                .unwrap()
                .label,
            "Watered"
        );

        schedule.next = Some(Milestone {
            date: today.add_days(2),
            end: None,
            kind: "harvest".into(),
            label: "Harvest".into(),
        });
        assert_eq!(
            next_with_diary(&schedule, &[soon], today).unwrap().label,
            "Watered"
        );

        schedule.next = Some(Milestone {
            date: today,
            end: None,
            kind: "harvest".into(),
            label: "Harvest".into(),
        });
        assert_eq!(
            next_with_diary(&schedule, &[past], today).unwrap().label,
            "Harvest"
        );
    }

    #[test]
    fn describes_a_day_that_is_not_today() {
        let today = Date::new(2027, 5, 1).unwrap();
        let schedule = Schedule {
            stages: Vec::new(),
            milestones: Vec::new(),
            current_stage: None,
            next: None,
            undated_stages: Vec::new(),
            unknown: Vec::new(),
            span_start: Date::new(2027, 4, 20).unwrap(),
            span_end: today,
        };
        let planting = Planting {
            id: 1,
            profile_id: 1,
            space_id: 1,
            name: None,
            quantity: 1,
            container: "ground".into(),
            container_volume_l: None,
            substrate: None,
            started_on: Date::new(2027, 4, 20).unwrap(),
            start_kind: "planted".into(),
            ended_on: None,
            outcome: None,
            notes: None,
        };

        assert_eq!(
            selected_status(&schedule, &planting, today, Date::new(2027, 4, 18).unwrap()),
            "Not started on 18 Apr"
        );
        assert_eq!(
            selected_status(&schedule, &planting, today, Date::new(2027, 4, 21).unwrap()),
            "Day 2 · between stages"
        );

        let mut finished = planting.clone();
        finished.ended_on = Some(Date::new(2027, 4, 25).unwrap());
        assert_eq!(
            selected_status(&schedule, &finished, today, Date::new(2027, 4, 30).unwrap()),
            "Finished 25 Apr"
        );
    }

    fn window(label: &str, start: Date, end: Date) -> StageWindow {
        StageWindow {
            stage: Stage {
                id: 1,
                profile_id: 1,
                position: 1,
                kind: "vegetative".into(),
                label: label.into(),
                duration_days_min: Some(1.0),
                duration_days_max: Some(1.0),
                notes: None,
                repeats: None,
            },
            earliest_start: start,
            latest_start: start,
            earliest_end: end,
            latest_end: end,
            likely_start: start,
            likely_end: end,
            actual_start: false,
        }
    }

    #[test]
    fn the_timeline_shows_one_year_around_the_selected_day() {
        let selected = Date::new(2026, 5, 15).unwrap();
        let (start, end) = TimelineZoom::Year.window(selected);

        assert_eq!(start, Date::new(2025, 11, 14).unwrap());
        assert_eq!(end, Date::new(2026, 11, 14).unwrap());
        assert_eq!(start.days_until(end), 365);
        assert_eq!(start.days_until(selected), 182);
    }

    #[test]
    fn zooming_in_halves_and_thirds_the_window_around_the_same_day() {
        let selected = Date::new(2026, 5, 15).unwrap();
        let half = TimelineZoom::Year.zoom_in().unwrap();
        let quarter = half.zoom_in().unwrap();

        assert_eq!(half, TimelineZoom::HalfYear);
        assert_eq!(quarter, TimelineZoom::Quarter);
        assert_eq!(quarter.zoom_in(), None);
        assert_eq!(TimelineZoom::Year.zoom_out(), None);

        for level in [TimelineZoom::Year, half, quarter] {
            let (start, end) = level.window(selected);
            assert!(start < selected && selected < end);
            // Both sides stay equal, so the selected day keeps the centre of the bar.
            assert_eq!(start.days_until(selected), selected.days_until(end) - 1);
        }

        assert_eq!(half.window(selected).0.days_until(selected), 91);
        assert_eq!(quarter.window(selected).0.days_until(selected), 45);
    }

    #[test]
    fn the_timeline_ticks_fall_on_month_starts() {
        let selected = Date::new(2026, 5, 15).unwrap();

        let year = TimelineZoom::Year.ticks(selected.add_days(-182), selected.add_days(183));
        assert!(
            year.iter()
                .all(|date| date.day == 1 && (date.month - 1) % 3 == 0)
        );
        assert_eq!(year.len(), 4);

        let quarter = TimelineZoom::Quarter.ticks(selected.add_days(-45), selected.add_days(46));
        assert!(quarter.iter().all(|date| date.day == 1));
        assert_eq!(
            quarter,
            vec![
                Date::new(2026, 4, 1).unwrap(),
                Date::new(2026, 5, 1).unwrap(),
                Date::new(2026, 6, 1).unwrap()
            ]
        );
    }

    #[test]
    fn the_timeline_tooltip_lists_only_the_visible_range() {
        let today = Date::new(2026, 5, 15).unwrap();
        let schedule = Schedule {
            stages: vec![
                window(
                    "This year",
                    Date::new(2026, 1, 1).unwrap(),
                    Date::new(2026, 2, 1).unwrap(),
                ),
                window(
                    "Two years on",
                    Date::new(2028, 1, 1).unwrap(),
                    Date::new(2028, 2, 1).unwrap(),
                ),
            ],
            milestones: Vec::new(),
            current_stage: None,
            next: None,
            undated_stages: Vec::new(),
            unknown: Vec::new(),
            span_start: Date::new(2025, 1, 1).unwrap(),
            span_end: Date::new(2028, 2, 1).unwrap(),
        };

        let text = timeline_tooltip(&schedule, today, today, TimelineZoom::Year);
        assert!(text.contains("This year"));
        assert!(!text.contains("Two years on"));
        assert_eq!(text.lines().count(), 1);
    }
}
