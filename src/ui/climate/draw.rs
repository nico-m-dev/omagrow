//! Drawing for one climate metric.
//!
//! The plot uses the card's whole width: the y labels sit inside a measured left margin and there
//! is nothing to the right of the plot. At 24 hours and 7 days the series is hourly, with a faint
//! day/night background. Above 7 days a day collapses; [`DayNightView`] decides whether that is
//! one mean with a min–max band, a day and a night average, or only one of the two.

use std::cell::Cell;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::rc::Rc;

use gtk::cairo;
use gtk::prelude::*;

use crate::calendar;
use crate::daylight::{self, day_intervals, night_intervals};
use crate::units::UnitPreference;

use super::data::{
    ClimateData, DailyWeather, DayNightView, DayPart, Metric, Range, Reference, dates_between,
    display_number, hourly_value, is_future_hour, reading_minutes, reading_value, short_date,
    source_label,
};
use crate::calendar::Date;
use crate::ui::shared::graph_style::{Align, Colors, paint};

/// The last plot geometry a draw used, so a pointer maps to the same x as the line it points at.
pub(super) type Geometry = Rc<Cell<(f64, f64)>>;

pub(super) fn geometry() -> Geometry {
    Rc::new(Cell::new((44.0, 300.0)))
}

#[derive(Clone, Copy)]
pub(super) struct GraphColors {
    pub fg: gtk::gdk::RGBA,
    pub weather: gtk::gdk::RGBA,
    pub sensor: gtk::gdk::RGBA,
    pub manual: gtk::gdk::RGBA,
    pub day_opt: gtk::gdk::RGBA,
    pub night_opt: gtk::gdk::RGBA,
    pub survival: gtk::gdk::RGBA,
    grid: gtk::gdk::RGBA,
    axis: gtk::gdk::RGBA,
    day_bg: gtk::gdk::RGBA,
    night_bg: gtk::gdk::RGBA,
    day_alpha: f64,
    night_alpha: f64,
    out: gtk::gdk::RGBA,
}

fn luminance(color: gtk::gdk::RGBA) -> f64 {
    0.2126 * color.red() as f64 + 0.7152 * color.green() as f64 + 0.0722 * color.blue() as f64
}

fn graph_color(value: &str) -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::parse(value).unwrap_or(gtk::gdk::RGBA::BLACK)
}

fn looks_green(color: gtk::gdk::RGBA) -> bool {
    color.green() > color.red() * 1.08 && color.green() > color.blue() * 1.08
}

fn looks_red(color: gtk::gdk::RGBA) -> bool {
    color.red() > color.green() * 1.08 && color.red() > color.blue() * 1.08
}

pub(super) fn graph_colors(widget: &impl IsA<gtk::Widget>) -> GraphColors {
    let colors = Colors::load(widget);
    let dark = luminance(colors.bg) < 0.5;
    let day_opt = if looks_green(colors.success) {
        colors.success
    } else {
        graph_color("#7FB77E")
    };
    let survival = if looks_red(colors.error) {
        colors.error
    } else {
        graph_color("#D96C6C")
    };

    GraphColors {
        fg: colors.fg,
        weather: colors.accent,
        sensor: graph_color("#6FA8DC"),
        manual: colors.fg,
        day_opt,
        night_opt: graph_color("#5FA8A0"),
        survival,
        grid: colors.fg,
        axis: colors.fg,
        day_bg: colors.fg,
        night_bg: if dark {
            gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)
        } else {
            colors.fg
        },
        day_alpha: if dark { 0.06 } else { 0.0 },
        night_alpha: if dark { 0.28 } else { 0.10 },
        out: survival,
    }
}

pub(super) fn x_text(minutes: i64) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
    data: &ClimateData,
    metric: Metric,
    hover: Option<(Date, i64)>,
    geometry: &Geometry,
) {
    let (start, end) = data.window();
    let total_days = data.total_days().max(1);
    let colors = graph_colors(area);
    let w = width as f64;
    let h = height as f64;
    let Some((min, max)) = scale(data, metric, start, end) else {
        return;
    };
    let ticks = nice_ticks(min, max);
    let left = axis_left(cr, data, metric, &ticks);
    let right = 8.0;
    let top = 10.0;
    let bottom = 24.0;
    let plot_w = (w - left - right).max(1.0);
    let plot_h = (h - top - bottom).max(1.0);
    geometry.set((left, plot_w));

    let x = |date: Date, minutes: i64| -> f64 {
        let offset = start.days_until(date) as f64 + minutes as f64 / 1440.0;
        left + (offset / total_days as f64).clamp(0.0, 1.0) * plot_w
    };
    let x_day = |date: Date| x(date, 12 * 60);
    let y = |value: f64| top + (1.0 - ((value - min) / (max - min)).clamp(0.0, 1.0)) * plot_h;

    draw_background(cr, data, start, end, &x, top, plot_h, colors);
    draw_outside_grow(cr, data, start, end, &x, left, top, plot_w, plot_h, colors);
    draw_placements(cr, data, start, end, &x, left, top, plot_w, plot_h, colors);
    draw_axes(
        cr, data, metric, start, end, &x, &ticks, left, top, plot_w, plot_h, &y, colors,
    );

    if metric.is_air() {
        if data.range.get().hourly() {
            draw_hourly_guides(cr, data, metric, start, end, &x, &y, min, max, colors);
        } else {
            draw_daily_guides(cr, data, metric, start, end, &x, &y, min, max, colors);
        }
    }

    if matches!(metric, Metric::Rain | Metric::Sunshine) {
        draw_bars(
            cr, data, metric, start, end, &x, &y, min, max, plot_w, colors,
        );
    } else if data.range.get().hourly() {
        draw_hourly_series(cr, data, metric, start, end, &x, &y, colors);
    } else {
        draw_daily_series(cr, data, metric, start, end, &x, &x_day, &y, colors);
    }

    draw_survival_edges(
        cr, data, metric, start, end, min, max, left, top, plot_h, colors,
    );

    if let Some(selected) = data
        .selected_date()
        .filter(|date| data.range.get() != Range::Day && start <= *date && *date <= end)
    {
        let marker = x_day(selected);
        cr.set_dash(&[3.0, 4.0], 0.0);
        paint(cr, colors.fg, 0.40);
        cr.set_line_width(1.0);
        cr.move_to(marker, top);
        cr.line_to(marker, top + plot_h);
        let _ = cr.stroke();
        cr.set_dash(&[], 0.0);
        paint(cr, colors.weather, 0.9);
        cr.arc(marker, top, 3.0, 0.0, 2.0 * PI);
        let _ = cr.fill();
    }

    // A 24-hour view of today stops at the last hour that passed. The marker says where that is,
    // so an empty right side reads as "not yet" instead of "missing".
    if data.range.get() == Range::Day
        && start == crate::clock::today()
        && let Some((_, minutes)) = crate::clock::now()
    {
        let now = x(start, minutes);
        paint(cr, colors.fg, 0.34);
        cr.set_line_width(1.0);
        cr.move_to(now, top);
        cr.line_to(now, top + plot_h);
        let _ = cr.stroke();
        crate::ui::shared::graph_style::text(
            cr,
            "now",
            now,
            top + 9.0,
            8.0,
            colors.fg,
            0.5,
            Align::Center,
        );
    }

    if let Some((date, minutes)) = hover {
        let line = x(date, minutes);
        paint(cr, colors.fg, 0.26);
        cr.set_line_width(1.0);
        cr.move_to(line, top);
        cr.line_to(line, top + plot_h);
        let _ = cr.stroke();
    }

    cr.set_dash(&[], 0.0);
}

/// The y labels without a unit: the card title carries the unit, and the plot keeps the width.
fn axis_left(cr: &cairo::Context, data: &ClimateData, metric: Metric, ticks: &[f64]) -> f64 {
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(9.0);
    let widest = ticks
        .iter()
        .map(|value| {
            cr.text_extents(&display_number(metric, data.units, *value))
                .map(|extents| extents.width())
                .unwrap_or(0.0)
        })
        .fold(0.0_f64, f64::max);
    (widest + 14.0).clamp(30.0, 64.0)
}

#[allow(clippy::too_many_arguments)]
fn draw_background(
    cr: &cairo::Context,
    data: &ClimateData,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    top: f64,
    plot_h: f64,
    colors: GraphColors,
) {
    if !data.range.get().hourly() {
        return;
    }

    for date in dates_between(start, end) {
        let rule = data.rule_at(date);
        let sun = data.sun(date);

        if let Some(intervals) = night_intervals(rule, sun) {
            paint(cr, colors.night_bg, colors.night_alpha);
            for (from, to) in intervals {
                cr.rectangle(
                    x(date, from),
                    top,
                    (x(date, to) - x(date, from)).max(1.0),
                    plot_h,
                );
                let _ = cr.fill();
            }
        }

        if colors.day_alpha > 0.0
            && let Some(intervals) = day_intervals(rule, sun)
        {
            paint(cr, colors.day_bg, colors.day_alpha);
            for (from, to) in intervals {
                cr.rectangle(
                    x(date, from),
                    top,
                    (x(date, to) - x(date, from)).max(1.0),
                    plot_h,
                );
                let _ = cr.fill();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_outside_grow(
    cr: &cairo::Context,
    data: &ClimateData,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    left: f64,
    top: f64,
    plot_w: f64,
    plot_h: f64,
    colors: GraphColors,
) {
    let Some((grow_start, grow_end)) = data.grow else {
        return;
    };
    let grow_end = grow_end.unwrap_or(end);

    if start < grow_start {
        let x1 = left;
        let x2 = x(grow_start, 0).max(x1);
        paint(cr, colors.out, 0.05);
        cr.rectangle(x1, top, x2 - x1, plot_h);
        let _ = cr.fill();
        if x2 - x1 > 70.0 {
            crate::ui::shared::graph_style::text(
                cr,
                "before planting",
                (x1 + x2) / 2.0,
                top + 14.0,
                8.0,
                colors.survival,
                0.55,
                Align::Center,
            );
        }
    }

    if end > grow_end {
        let x1 = x(grow_end.add_days(1), 0).min(left + plot_w);
        let x2 = left + plot_w;
        paint(cr, colors.out, 0.05);
        cr.rectangle(x1, top, x2 - x1, plot_h);
        let _ = cr.fill();
        if x2 - x1 > 60.0 {
            crate::ui::shared::graph_style::text(
                cr,
                "after",
                (x1 + x2) / 2.0,
                top + 14.0,
                8.0,
                colors.survival,
                0.55,
                Align::Center,
            );
        }
    }
}

/// A move is a thin divider and a faint tint, not a label: the space's name is in the header and
/// the setup tiles, and the timeline above already has the context.
#[allow(clippy::too_many_arguments)]
fn draw_placements(
    cr: &cairo::Context,
    data: &ClimateData,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    left: f64,
    top: f64,
    plot_w: f64,
    plot_h: f64,
    colors: GraphColors,
) {
    for (index, placement) in data.placements.iter().enumerate() {
        let from = placement.started_on.max(start);
        let to = placement.ended_on.unwrap_or(end).min(end);
        if from > to {
            continue;
        }
        let x1 = x(from, 0).max(left);
        let x2 = x(to.add_days(1), 0).min(left + plot_w);
        if x2 <= x1 {
            continue;
        }
        paint(cr, colors.fg, if index % 2 == 0 { 0.022 } else { 0.010 });
        cr.rectangle(x1, top, x2 - x1, plot_h);
        let _ = cr.fill();

        if placement.started_on > start {
            paint(cr, colors.fg, 0.18);
            cr.set_line_width(1.0);
            cr.move_to(x1, top);
            cr.line_to(x1, top + plot_h);
            let _ = cr.stroke();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_axes(
    cr: &cairo::Context,
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    ticks: &[f64],
    left: f64,
    top: f64,
    plot_w: f64,
    plot_h: f64,
    y: &impl Fn(f64) -> f64,
    colors: GraphColors,
) {
    for value in ticks {
        let line_y = y(*value);
        paint(cr, colors.grid, 0.08);
        cr.set_line_width(1.0);
        cr.move_to(left, line_y);
        cr.line_to(left + plot_w, line_y);
        let _ = cr.stroke();
        crate::ui::shared::graph_style::text(
            cr,
            &display_number(metric, data.units, *value),
            left - 8.0,
            line_y,
            9.0,
            colors.axis,
            0.55,
            Align::Right,
        );
    }

    if data.range.get() == Range::Day {
        for hour in 0..=24 {
            let tick = x(start, hour * 60);
            if hour > 0 && hour < 24 {
                paint(cr, colors.grid, 0.05);
                cr.set_line_width(1.0);
                cr.move_to(tick, top);
                cr.line_to(tick, top + plot_h);
                let _ = cr.stroke();
            }
            if hour % 6 == 0 {
                crate::ui::shared::graph_style::text(
                    cr,
                    &format!("{hour:02}:00"),
                    tick,
                    top + plot_h + 18.0,
                    9.0,
                    colors.axis,
                    0.55,
                    if hour == 0 {
                        Align::Left
                    } else if hour == 24 {
                        Align::Right
                    } else {
                        Align::Center
                    },
                );
            }
        }
        return;
    }

    let dates = dates_between(start, end);
    let step = tick_step(dates.len() as i64);
    let mut last_label = f64::NEG_INFINITY;
    for (index, date) in dates.iter().enumerate() {
        if index > 0 && index % step as usize == 0 {
            let tick = x(*date, 0);
            paint(cr, colors.grid, 0.06);
            cr.set_line_width(1.0);
            cr.move_to(tick, top);
            cr.line_to(tick, top + plot_h);
            let _ = cr.stroke();
        }

        let label_x = x(*date, 12 * 60);
        let due = index == 0 || index % step as usize == 0 || index + 1 == dates.len();
        if due && (index == 0 || label_x - last_label >= 48.0) {
            crate::ui::shared::graph_style::text(
                cr,
                &short_date(*date),
                label_x,
                top + plot_h + 18.0,
                9.0,
                colors.axis,
                0.55,
                Align::Center,
            );
            last_label = label_x;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_hourly_guides(
    cr: &cairo::Context,
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    y: &impl Fn(f64) -> f64,
    min: f64,
    max: f64,
    colors: GraphColors,
) {
    for date in dates_between(start, end) {
        let rule = data.rule_at(date);
        let sun = data.sun(date);

        if let Some(intervals) = day_intervals(rule, sun)
            && let Some(reference) = data.guide(metric, DayPart::Day, date)
        {
            draw_reference(
                cr,
                &intervals,
                reference,
                date,
                x,
                y,
                colors.day_opt,
                colors,
                0.16,
                min,
                max,
            );
        }
        if let Some(intervals) = night_intervals(rule, sun)
            && let Some(reference) = data.guide(metric, DayPart::Night, date)
        {
            draw_reference(
                cr,
                &intervals,
                reference,
                date,
                x,
                y,
                colors.night_opt,
                colors,
                0.13,
                min,
                max,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_reference(
    cr: &cairo::Context,
    intervals: &[(i64, i64)],
    reference: Reference,
    date: Date,
    x: &impl Fn(Date, i64) -> f64,
    y: &impl Fn(f64) -> f64,
    color: gtk::gdk::RGBA,
    colors: GraphColors,
    alpha: f64,
    min: f64,
    max: f64,
) {
    for (from, to) in intervals {
        let x1 = x(date, *from);
        let x2 = x(date, *to);
        if x2 <= x1 {
            continue;
        }

        if let (Some(opt_min), Some(opt_max)) = (reference.opt_min, reference.opt_max) {
            paint(cr, color, alpha);
            cr.rectangle(x1, y(opt_max), x2 - x1, (y(opt_min) - y(opt_max)).max(1.0));
            let _ = cr.fill();
        }

        for value in [reference.survival_min, reference.survival_max]
            .into_iter()
            .flatten()
        {
            if value < min || value > max {
                continue;
            }
            cr.set_dash(&[4.0, 4.0], 0.0);
            paint(cr, colors.survival, 0.4);
            cr.set_line_width(0.9);
            cr.move_to(x1, y(value));
            cr.line_to(x2, y(value));
            let _ = cr.stroke();
        }
    }
    cr.set_dash(&[], 0.0);
}

#[allow(clippy::too_many_arguments)]
fn draw_daily_guides(
    cr: &cairo::Context,
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    y: &impl Fn(f64) -> f64,
    min: f64,
    max: f64,
    colors: GraphColors,
) {
    let view = data.view.get();
    let Some(guides) = data.guides.for_metric(metric) else {
        return;
    };

    let mut parts = Vec::new();
    if view.draws_day() {
        parts.push((&guides.day, colors.day_opt, 0.12));
    }
    if view.draws_night() {
        parts.push((&guides.night, colors.night_opt, 0.10));
    }

    for (spans, color, alpha) in parts {
        for span in spans {
            let from = span.start.max(start);
            let to = span.end.min(end);
            if from > to {
                continue;
            }
            let x1 = x(from, 0);
            let x2 = x(to.add_days(1), 0);
            if let (Some(opt_min), Some(opt_max)) = (span.reference.opt_min, span.reference.opt_max)
            {
                paint(cr, color, alpha);
                cr.rectangle(x1, y(opt_max), x2 - x1, (y(opt_min) - y(opt_max)).max(1.0));
                let _ = cr.fill();
            }
            for value in [span.reference.survival_min, span.reference.survival_max]
                .into_iter()
                .flatten()
            {
                if value < min || value > max {
                    continue;
                }
                cr.set_dash(&[4.0, 4.0], 0.0);
                paint(cr, colors.survival, 0.38);
                cr.set_line_width(0.9);
                cr.move_to(x1, y(value));
                cr.line_to(x2, y(value));
                let _ = cr.stroke();
            }
        }
    }
    cr.set_dash(&[], 0.0);
}

#[allow(clippy::too_many_arguments)]
fn draw_hourly_series(
    cr: &cairo::Context,
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    y: &impl Fn(f64) -> f64,
    colors: GraphColors,
) {
    let sources = data.sources.get();

    if sources.weather {
        let mut previous: Option<(Date, i64)> = None;
        let mut started = false;
        paint(cr, colors.weather, 0.9);
        cr.set_line_width(1.8);
        for date in dates_between(start, end) {
            if !data.weather_visible(date) {
                if started {
                    let _ = cr.stroke();
                    started = false;
                }
                previous = None;
                continue;
            }
            let mut hours = data.hours(date);
            hours.sort_by_key(|hour| hour.minutes);
            if hours.is_empty() {
                // A day before the hourly history still has a daily summary. Today must not fall
                // back to it: the daily row is a forecast for the rest of the day.
                if date != crate::clock::today()
                    && let Some(aggregate) = data.aggregate(date)
                {
                    let values = aggregate.values(metric);
                    if let Some(mean) = values.mean.or(values.day).or(values.night) {
                        let point_x = x(date, 12 * 60);
                        let point_y = y(mean);
                        if started && previous.is_some() {
                            cr.line_to(point_x, point_y);
                        } else {
                            cr.move_to(point_x, point_y);
                            started = true;
                        }
                        paint(cr, colors.weather, 0.9);
                        cr.set_line_width(1.8);
                        cr.arc(point_x, point_y, 2.2, 0.0, 2.0 * PI);
                        let _ = cr.fill();
                    }
                }
                continue;
            }

            for hour in hours {
                if is_future_hour(date, hour.minutes) {
                    continue;
                }
                let Some(value) = hourly_value(metric, hour) else {
                    continue;
                };
                let point_x = x(date, hour.minutes);
                let point_y = y(value);
                if let Some((last_date, last_minutes)) = previous {
                    let gap = last_date.days_until(date) * 1440 + hour.minutes - last_minutes;
                    if gap > 120 {
                        if started {
                            let _ = cr.stroke();
                        }
                        paint(cr, colors.weather, 0.9);
                        cr.set_line_width(1.8);
                        cr.move_to(point_x, point_y);
                        started = true;
                    } else {
                        cr.line_to(point_x, point_y);
                    }
                } else {
                    cr.move_to(point_x, point_y);
                    started = true;
                }
                previous = Some((date, hour.minutes));
            }
        }
        if started {
            paint(cr, colors.weather, 0.9);
            cr.set_line_width(1.8);
            let _ = cr.stroke();
        }
    }

    if sources.sensor {
        let mut readings: Vec<&crate::climate::ClimateReading> = data
            .readings
            .iter()
            .filter(|reading| reading.source == "sensor")
            .filter(|reading| start <= reading.measured_on && reading.measured_on <= end)
            .filter(|reading| reading_value(metric, reading).is_some())
            .collect();
        readings.sort_by_key(|reading| {
            (
                reading.measured_on,
                reading.measured_time.clone().unwrap_or_default(),
            )
        });
        let mut previous: Option<(Date, i64, i64)> = None;
        let mut started = false;
        cr.set_line_width(1.5);
        paint(cr, colors.sensor, 0.86);
        for reading in readings {
            let value = reading_value(metric, reading).unwrap_or(0.0);
            let point_x = x(reading.measured_on, reading_minutes(reading));
            let point_y = y(value);
            let space = data
                .placement_at(reading.measured_on)
                .map(|placement| placement.space_id)
                .unwrap_or(0);
            let same = previous.is_some_and(|(date, minutes, previous_space)| {
                previous_space == space
                    && date.days_until(reading.measured_on) * 1440 + reading_minutes(reading)
                        - minutes
                        <= 360
            });
            if same {
                cr.line_to(point_x, point_y);
            } else {
                if started {
                    let _ = cr.stroke();
                }
                paint(cr, colors.sensor, 0.86);
                cr.set_line_width(1.5);
                cr.move_to(point_x, point_y);
                started = true;
            }
            previous = Some((reading.measured_on, reading_minutes(reading), space));
            cr.arc(point_x, point_y, 1.8, 0.0, 2.0 * PI);
            let _ = cr.fill();
        }
        if started {
            let _ = cr.stroke();
        }
    }

    if sources.manual {
        for reading in &data.readings {
            if reading.source != "manual"
                || reading.measured_on < start
                || reading.measured_on > end
            {
                continue;
            }
            let Some(value) = reading_value(metric, reading) else {
                continue;
            };
            paint(cr, colors.manual, 0.95);
            cr.arc(
                x(reading.measured_on, reading_minutes(reading)),
                y(value),
                2.8,
                0.0,
                2.0 * PI,
            );
            let _ = cr.fill();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_survival_edges(
    cr: &cairo::Context,
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    min: f64,
    max: f64,
    left: f64,
    top: f64,
    plot_h: f64,
    colors: GraphColors,
) {
    if !metric.is_air() {
        return;
    }
    let Some(guides) = data.guides.for_metric(metric) else {
        return;
    };
    let view = data.view.get();
    let hourly = data.range.get().hourly();
    let mut parts: Vec<&[super::data::GuideSpan]> = Vec::new();
    if hourly || view.draws_day() {
        parts.push(&guides.day);
    }
    if hourly || view.draws_night() {
        parts.push(&guides.night);
    }

    let unit = data.units.label(metric.unit());
    let mut below: Vec<String> = Vec::new();
    let mut above: Vec<String> = Vec::new();
    for spans in parts {
        for span in spans {
            if span.end < start || span.start > end {
                continue;
            }
            if let Some(value) = span.reference.survival_min {
                let label = format!(
                    "survival ≥ {} {unit}",
                    display_number(metric, data.units, value)
                );
                if value < min {
                    below.push(label);
                } else if value > max {
                    above.push(label);
                }
            }
            if let Some(value) = span.reference.survival_max {
                let label = format!(
                    "survival ≤ {} {unit}",
                    display_number(metric, data.units, value)
                );
                if value < min {
                    below.push(label);
                } else if value > max {
                    above.push(label);
                }
            }
        }
    }
    below.sort();
    below.dedup();
    above.sort();
    above.dedup();

    if !below.is_empty() {
        crate::ui::shared::graph_style::text(
            cr,
            &below.join(" · "),
            left + 6.0,
            top + plot_h - 6.0,
            8.5,
            colors.survival,
            0.85,
            Align::Left,
        );
    }
    if !above.is_empty() {
        crate::ui::shared::graph_style::text(
            cr,
            &above.join(" · "),
            left + 6.0,
            top + 11.0,
            8.5,
            colors.survival,
            0.85,
            Align::Left,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_daily_series(
    cr: &cairo::Context,
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    x_day: &impl Fn(Date) -> f64,
    y: &impl Fn(f64) -> f64,
    colors: GraphColors,
) {
    let sources = data.sources.get();
    let view = data.view.get();

    if sources.weather {
        if view == DayNightView::Combined {
            // One continuous min–max band, not a rectangle per day: at 30 days a per-day
            // rectangle reads as a piano of vertical blocks.
            let mut run: Vec<(f64, f64, f64)> = Vec::new();
            for date in dates_between(start, end) {
                let point = if data.weather_visible(date) {
                    data.aggregate(date).and_then(|aggregate| {
                        let values = aggregate.values(metric);
                        values
                            .min
                            .zip(values.max)
                            .map(|(min, max)| (x_day(date), y(max), y(min)))
                    })
                } else {
                    None
                };
                match point {
                    Some(point) => run.push(point),
                    None => {
                        draw_min_max_ribbon(cr, &run, colors);
                        run.clear();
                    }
                }
            }
            draw_min_max_ribbon(cr, &run, colors);

            draw_daily_line(
                cr,
                data,
                metric,
                start,
                end,
                x_day,
                y,
                colors.weather,
                1.8,
                0.9,
                Part::Mean,
            );
        } else {
            if view.draws_day() {
                draw_daily_line(
                    cr,
                    data,
                    metric,
                    start,
                    end,
                    x_day,
                    y,
                    colors.weather,
                    1.8,
                    0.9,
                    Part::Day,
                );
            }
            if view.draws_night() {
                draw_daily_line(
                    cr,
                    data,
                    metric,
                    start,
                    end,
                    x_day,
                    y,
                    colors.weather,
                    1.4,
                    0.75,
                    Part::Night,
                );
            }
        }
    }

    if sources.sensor {
        let readings: Vec<&crate::climate::ClimateReading> = data
            .readings
            .iter()
            .filter(|reading| reading.source == "sensor")
            .filter(|reading| start <= reading.measured_on && reading.measured_on <= end)
            .filter(|reading| reading_value(metric, reading).is_some())
            .collect();

        let mut all: HashMap<Date, (f64, usize)> = HashMap::new();
        let mut day: HashMap<Date, (f64, usize)> = HashMap::new();
        let mut night: HashMap<Date, (f64, usize)> = HashMap::new();
        let mut unknown: HashMap<Date, (f64, usize)> = HashMap::new();
        for reading in readings {
            let Some(value) = reading_value(metric, reading) else {
                continue;
            };
            let entry = all.entry(reading.measured_on).or_insert((0.0, 0));
            entry.0 += value;
            entry.1 += 1;
            let part = if reading.measured_time.is_none() {
                None
            } else {
                daylight::is_day(
                    data.rule_at(reading.measured_on),
                    data.sun(reading.measured_on),
                    reading_minutes(reading),
                )
            };
            let bucket = match part {
                Some(true) => &mut day,
                Some(false) => &mut night,
                None => &mut unknown,
            };
            let entry = bucket.entry(reading.measured_on).or_insert((0.0, 0));
            entry.0 += value;
            entry.1 += 1;
        }

        if view == DayNightView::Combined {
            draw_average_map(cr, &all, x_day, y, colors.sensor, 1.5, 0.86, false);
        } else {
            if view.draws_day() {
                draw_average_map(cr, &day, x_day, y, colors.sensor, 1.5, 0.86, false);
            }
            if view.draws_night() {
                draw_average_map(cr, &night, x_day, y, colors.sensor, 1.2, 0.72, true);
            }
            draw_average_map(cr, &unknown, x_day, y, colors.sensor, 1.2, 0.58, false);
        }
    }

    if sources.manual {
        for reading in &data.readings {
            if reading.source != "manual"
                || reading.measured_on < start
                || reading.measured_on > end
            {
                continue;
            }
            let Some(value) = reading_value(metric, reading) else {
                continue;
            };
            paint(cr, colors.manual, 0.95);
            cr.arc(
                x(reading.measured_on, reading_minutes(reading)),
                y(value),
                2.8,
                0.0,
                2.0 * PI,
            );
            let _ = cr.fill();
        }
    }
}

#[derive(Clone, Copy)]
enum Part {
    Mean,
    Day,
    Night,
}

fn part_value(values: DailyWeather, part: Part) -> Option<f64> {
    match part {
        Part::Mean => values.mean.or(values.day).or(values.night),
        Part::Day => values.day,
        Part::Night => values.night,
    }
}

fn draw_min_max_ribbon(cr: &cairo::Context, run: &[(f64, f64, f64)], colors: GraphColors) {
    if run.is_empty() {
        return;
    }

    if run.len() == 1 {
        let (x, top, bottom) = run[0];
        paint(cr, colors.fg, 0.06);
        cr.rectangle(x - 1.0, top, 2.0, (bottom - top).max(1.0));
        let _ = cr.fill();
        return;
    }

    paint(cr, colors.fg, 0.06);
    cr.move_to(run[0].0, run[0].1);
    for (x, top, _) in &run[1..] {
        cr.line_to(*x, *top);
    }
    for (x, _, bottom) in run.iter().rev() {
        cr.line_to(*x, *bottom);
    }
    cr.close_path();
    let _ = cr.fill();
}

#[allow(clippy::too_many_arguments)]
fn draw_daily_line(
    cr: &cairo::Context,
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    x_day: &impl Fn(Date) -> f64,
    y: &impl Fn(f64) -> f64,
    color: gtk::gdk::RGBA,
    width: f64,
    alpha: f64,
    part: Part,
) {
    let mut previous: Option<Date> = None;
    let mut started = false;
    cr.set_line_width(width);
    if matches!(part, Part::Night) {
        cr.set_dash(&[2.0, 4.0], 0.0);
    }

    for date in dates_between(start, end) {
        if !data.weather_visible(date) {
            continue;
        }
        let Some(aggregate) = data.aggregate(date) else {
            continue;
        };
        let Some(value) = part_value(aggregate.values(metric), part) else {
            continue;
        };
        let point_x = x_day(date);
        let point_y = y(value);
        if started && previous.is_some_and(|previous| previous.add_days(1) == date) {
            cr.line_to(point_x, point_y);
        } else {
            if started {
                let _ = cr.stroke();
            }
            paint(cr, color, alpha);
            cr.set_line_width(width);
            if matches!(part, Part::Night) {
                cr.set_dash(&[2.0, 4.0], 0.0);
            }
            cr.move_to(point_x, point_y);
            started = true;
        }
        previous = Some(date);
    }
    if started {
        paint(cr, color, alpha);
        cr.set_line_width(width);
        let _ = cr.stroke();
    }
    cr.set_dash(&[], 0.0);
}

#[allow(clippy::too_many_arguments)]
fn draw_average_map(
    cr: &cairo::Context,
    values: &HashMap<Date, (f64, usize)>,
    x_day: &impl Fn(Date) -> f64,
    y: &impl Fn(f64) -> f64,
    color: gtk::gdk::RGBA,
    width: f64,
    alpha: f64,
    night: bool,
) {
    let mut dates: Vec<&Date> = values.keys().collect();
    dates.sort();
    let mut previous: Option<Date> = None;
    let mut started = false;
    cr.set_line_width(width);
    if night {
        cr.set_dash(&[2.0, 4.0], 0.0);
    }

    for date in dates {
        let (sum, count) = values[date];
        if count == 0 {
            continue;
        }
        let point_x = x_day(*date);
        let point_y = y(sum / count as f64);
        if started && previous.is_some_and(|previous| previous.add_days(1) == *date) {
            cr.line_to(point_x, point_y);
        } else {
            if started {
                let _ = cr.stroke();
            }
            paint(cr, color, alpha);
            cr.set_line_width(width);
            if night {
                cr.set_dash(&[2.0, 4.0], 0.0);
            }
            cr.move_to(point_x, point_y);
            started = true;
        }
        previous = Some(*date);
    }
    if started {
        let _ = cr.stroke();
    }
    cr.set_dash(&[], 0.0);
}

#[allow(clippy::too_many_arguments)]
fn draw_bars(
    cr: &cairo::Context,
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
    x: &impl Fn(Date, i64) -> f64,
    y: &impl Fn(f64) -> f64,
    min: f64,
    max: f64,
    plot_w: f64,
    colors: GraphColors,
) {
    if !data.sources.get().weather {
        return;
    }
    let total_days = data.total_days().max(1);
    let daily = data.range.get() != Range::Day;
    let bar_width = if daily {
        (plot_w / total_days as f64).clamp(2.0, 20.0) * 0.65
    } else {
        (plot_w / 24.0).clamp(2.0, 20.0) * 0.7
    };

    for date in dates_between(start, end) {
        if !data.weather_visible(date) {
            continue;
        }

        if !daily {
            let mut drew = false;
            for hour in data.hours(date) {
                if is_future_hour(date, hour.minutes) {
                    continue;
                }
                let Some(value) = hourly_value(metric, hour) else {
                    continue;
                };
                drew = true;
                let xx = x(date, hour.minutes);
                let base = y(min);
                let top_y = y(value.clamp(min, max));
                paint(cr, colors.weather, 0.75);
                cr.rectangle(
                    xx - bar_width / 2.0,
                    top_y,
                    bar_width,
                    (base - top_y).max(1.0),
                );
                let _ = cr.fill();
            }
            // A past day older than the hourly history still has its daily row; today must not
            // use it, because that row is a forecast for the rest of the day.
            if drew || date == crate::clock::today() {
                continue;
            }
        }

        let Some(value) = data.day_value(metric, date) else {
            continue;
        };
        let xx = x(date, 12 * 60);
        let base = y(min);
        let top_y = y(value.clamp(min, max));
        paint(cr, colors.weather, 0.75);
        cr.rectangle(
            xx - bar_width / 2.0,
            top_y,
            bar_width,
            (base - top_y).max(1.0),
        );
        let _ = cr.fill();
    }
}

pub(super) fn scale(
    data: &ClimateData,
    metric: Metric,
    start: Date,
    end: Date,
) -> Option<(f64, f64)> {
    let sources = data.sources.get();
    let view = data.view.get();

    match metric {
        Metric::Humidity => Some((0.0, 100.0)),
        Metric::Rain | Metric::Sunshine => {
            let mut max = 0.0_f64;
            if sources.weather {
                if data.range.get() == Range::Day {
                    let mut found = false;
                    for date in dates_between(start, end) {
                        if !data.weather_visible(date) {
                            continue;
                        }
                        for hour in data.hours(date) {
                            if is_future_hour(date, hour.minutes) {
                                continue;
                            }
                            if let Some(value) = hourly_value(metric, hour) {
                                found = true;
                                max = max.max(value);
                            }
                        }
                    }
                    // A past day older than the hourly history still has its daily row. Today
                    // must not use it: the daily row is a forecast for the rest of the day.
                    if !found {
                        for date in dates_between(start, end) {
                            if date != crate::clock::today()
                                && data.weather_visible(date)
                                && let Some(value) = data.day_value(metric, date)
                            {
                                max = max.max(value);
                            }
                        }
                    }
                } else {
                    for date in dates_between(start, end) {
                        if data.weather_visible(date)
                            && let Some(value) = data.day_value(metric, date)
                        {
                            max = max.max(value);
                        }
                    }
                }
            }

            let floor = if metric == Metric::Rain || data.range.get() == Range::Day {
                1.0
            } else {
                6.0
            };
            let max = if metric == Metric::Rain {
                (max * 1.15).max(floor)
            } else {
                // A sunshine bar at the day's maximum must not touch the top edge; it reads as
                // clipped even when the value is correct.
                (max.max(floor) * 1.12).max(floor)
            };
            Some((0.0, max))
        }
        Metric::Temperature => {
            let mut data_values: Vec<f64> = Vec::new();
            let mut optimum_values: Vec<f64> = Vec::new();
            let mut survival_values: Vec<f64> = Vec::new();

            if sources.weather {
                for date in dates_between(start, end) {
                    if !data.weather_visible(date) {
                        continue;
                    }
                    if data.range.get().hourly() {
                        let mut found = false;
                        for hour in data.hours(date) {
                            if is_future_hour(date, hour.minutes) {
                                continue;
                            }
                            if let Some(value) = hourly_value(metric, hour) {
                                data_values.push(value);
                                found = true;
                            }
                        }
                        // The hourly line falls back to the daily mean for a past day older than
                        // the hourly history; the scale must keep that point visible.
                        if !found
                            && date != crate::clock::today()
                            && let Some(aggregate) = data.aggregate(date)
                        {
                            let values = aggregate.temperature;
                            data_values.extend(
                                [values.min, values.max, values.mean].into_iter().flatten(),
                            );
                        }
                    } else if let Some(aggregate) = data.aggregate(date) {
                        let values = aggregate.temperature;
                        if view == DayNightView::Combined {
                            data_values.extend(
                                [values.min, values.max, values.mean].into_iter().flatten(),
                            );
                        } else {
                            if view.draws_day() {
                                data_values.extend(
                                    [values.day, values.min, values.max].into_iter().flatten(),
                                );
                            }
                            if view.draws_night() {
                                data_values.extend(
                                    [values.night, values.min, values.max].into_iter().flatten(),
                                );
                            }
                        }
                    }
                }
            }
            for source in ["manual", "sensor"] {
                if !source_on(sources, source) {
                    continue;
                }
                for reading in &data.readings {
                    if reading.source == source
                        && start <= reading.measured_on
                        && reading.measured_on <= end
                        && let Some(value) = reading.temperature_c
                    {
                        data_values.push(value);
                    }
                }
            }

            let hourly = data.range.get().hourly();
            let parts: Vec<DayPart> = if hourly {
                vec![DayPart::Day, DayPart::Night]
            } else {
                match view {
                    DayNightView::Both => vec![DayPart::Day, DayPart::Night],
                    DayNightView::Day => vec![DayPart::Day],
                    DayNightView::Night => vec![DayPart::Night],
                    DayNightView::Combined => vec![DayPart::Day],
                }
            };
            for part in &parts {
                if let Some(guides) = data.guides.for_metric(metric) {
                    let spans = match part {
                        DayPart::Day => &guides.day,
                        DayPart::Night => &guides.night,
                    };
                    for span in spans {
                        if span.end >= start && span.start <= end {
                            optimum_values.extend(
                                [span.reference.opt_min, span.reference.opt_max]
                                    .into_iter()
                                    .flatten(),
                            );
                            survival_values.extend(
                                [span.reference.survival_min, span.reference.survival_max]
                                    .into_iter()
                                    .flatten(),
                            );
                        }
                    }
                }
            }

            let mut values = data_values.clone();
            values.extend(optimum_values);
            if values.is_empty() {
                values = survival_values;
            } else {
                let base_min = values.iter().copied().reduce(f64::min).unwrap_or(0.0);
                let base_max = values.iter().copied().reduce(f64::max).unwrap_or(0.0);
                // A survival limit is part of the plant's story. Keep it when it is within one
                // extra data-span of the chart; a limit much farther out would flatten the data,
                // so the plot marks it at the edge instead.
                let margin = ((base_max - base_min) * 1.0).max(2.0);
                values.extend(
                    survival_values
                        .into_iter()
                        .filter(|value| *value >= base_min - margin && *value <= base_max + margin),
                );
            }

            let mut min = values.iter().copied().reduce(f64::min)?;
            let mut max = values.iter().copied().reduce(f64::max)?;
            if (max - min).abs() < 1.0 {
                min -= 0.5;
                max += 0.5;
            } else {
                let pad = (max - min) * 0.06;
                min -= pad;
                max += pad;
            }
            Some((min, max))
        }
    }
}

fn source_on(sources: super::data::Sources, source: &str) -> bool {
    match source {
        "weather" => sources.weather,
        "sensor" => sources.sensor,
        "manual" => sources.manual,
        _ => false,
    }
}

/// Three to five round labels inside the scale, so an axis never reads 18.9, 20.2, 21.5.
pub(super) fn nice_ticks(min: f64, max: f64) -> Vec<f64> {
    let range = max - min;
    if !range.is_finite() || range <= 0.0 {
        return vec![min];
    }

    let raw = range / 4.0;
    let magnitude = 10f64.powf(raw.log10().floor());
    let residual = raw / magnitude;
    let step = if residual <= 1.0 {
        magnitude
    } else if residual <= 2.0 {
        2.0 * magnitude
    } else if residual <= 5.0 {
        5.0 * magnitude
    } else {
        10.0 * magnitude
    };

    let mut ticks = Vec::new();
    let mut value = (min / step).ceil() * step;
    while value <= max + step * 0.001 {
        let rounded = (value * 1_000_000.0).round() / 1_000_000.0;
        ticks.push(if rounded.abs() < 1e-9 { 0.0 } else { rounded });
        value += step;
    }
    if ticks.is_empty() {
        ticks.push(min);
    }
    ticks
}

fn tick_step(total_days: i64) -> i64 {
    match total_days.max(1) {
        0..=8 => 1,
        9..=15 => 2,
        16..=45 => 5,
        46..=120 => 14,
        121..=240 => 28,
        _ => 56,
    }
}

/// A pointer position. Above 7 days a day is one point, so the pointer snaps to the day.
pub(super) fn point_at(data: &ClimateData, geometry: &Geometry, x: f64) -> Option<(Date, i64)> {
    let (start, end) = data.window();
    let total = data.total_days().max(1) as f64;
    let (left, plot_w) = geometry.get();
    let ratio = ((x - left) / plot_w).clamp(0.0, 0.999);
    let offset = ratio * total;

    if data.range.get() == Range::Day {
        return Some((start, (offset * 1440.0).round() as i64));
    }

    let day_offset = offset.floor() as i64;
    let date = start.add_days(day_offset).min(end);
    Some((date, 12 * 60))
}

pub(super) fn tooltip(data: &ClimateData, metric: Metric, point: Option<(Date, i64)>) -> String {
    let Some((date, minutes)) = point else {
        return String::new();
    };

    let sources = data.sources.get();
    let weather_visible = sources.weather && data.weather_visible(date);
    let mut parts = Vec::new();
    let day = format!(
        "{} {} {}",
        date.day,
        calendar::month_name(date.month),
        date.year
    );

    if weather_visible && data.range.get().hourly() {
        if matches!(metric, Metric::Rain | Metric::Sunshine) && data.range.get() != Range::Day {
            if let Some(value) = data.day_value(metric, date) {
                parts.push(format!(
                    "{} {}",
                    super::data::display_value(metric, data.units, value),
                    if metric == Metric::Rain {
                        "rain"
                    } else {
                        "sun"
                    }
                ));
            }
        } else {
            let hour = data
                .hours(date)
                .into_iter()
                .filter(|hour| !is_future_hour(date, hour.minutes))
                .min_by_key(|hour| (hour.minutes - minutes).abs());
            let mut found = false;
            if let Some(hour) = hour
                && let Some(value) = hourly_value(metric, hour)
            {
                parts.push(format!(
                    "{} {}",
                    x_text(hour.minutes),
                    super::data::display_value(metric, data.units, value)
                ));
                found = true;
            }
            // The line falls back to the daily row for a past day older than the hourly history;
            // the readout must name the same value.
            if !found && date != crate::clock::today() {
                if matches!(metric, Metric::Rain | Metric::Sunshine) {
                    if let Some(value) = data.day_value(metric, date) {
                        parts.push(format!(
                            "{} {}",
                            super::data::display_value(metric, data.units, value),
                            if metric == Metric::Rain {
                                "rain"
                            } else {
                                "sun"
                            }
                        ));
                    }
                } else if let Some(aggregate) = data.aggregate(date) {
                    let values = aggregate.values(metric);
                    if let Some(mean) = values.mean.or(values.day).or(values.night) {
                        parts.push(format!(
                            "mean {}",
                            super::data::display_value(metric, data.units, mean)
                        ));
                    }
                }
            }
        }
    } else if weather_visible && let Some(aggregate) = data.aggregate(date) {
        let values = aggregate.values(metric);
        let view = data.view.get();
        match metric {
            Metric::Temperature | Metric::Humidity => {
                if view == DayNightView::Combined {
                    if let Some(mean) = values.mean.or(values.day) {
                        parts.push(format!(
                            "mean {}",
                            super::data::display_value(metric, data.units, mean)
                        ));
                    }
                } else {
                    if view.draws_day()
                        && let Some(day_value) = values.day
                    {
                        parts.push(format!(
                            "day {}",
                            super::data::display_value(metric, data.units, day_value)
                        ));
                    }
                    if view.draws_night()
                        && let Some(night) = values.night
                    {
                        parts.push(format!(
                            "night {}",
                            super::data::display_value(metric, data.units, night)
                        ));
                    }
                }
                if let (Some(min), Some(max)) = (values.min, values.max) {
                    parts.push(format!(
                        "{}–{}",
                        display_number(metric, data.units, min),
                        display_number(metric, data.units, max)
                    ));
                }
            }
            Metric::Rain => {
                if let Some(value) = data.day_value(metric, date) {
                    parts.push(format!(
                        "{} rain",
                        super::data::display_value(metric, data.units, value)
                    ));
                }
            }
            Metric::Sunshine => {
                if let Some(value) = data.day_value(metric, date) {
                    parts.push(format!(
                        "{} sun",
                        super::data::display_value(metric, data.units, value)
                    ));
                }
            }
        }
    }

    for reading in data.readings.iter().filter(|reading| {
        reading.measured_on == date
            && reading_value(metric, reading).is_some()
            && source_on(sources, &reading.source)
            && (data.range.get() != Range::Day || (reading_minutes(reading) - minutes).abs() <= 60)
    }) {
        if let Some(value) = reading_value(metric, reading) {
            let time = reading
                .measured_time
                .as_deref()
                .map(|time| format!(" {time}"))
                .unwrap_or_default();
            parts.push(format!(
                "{} {}{} · {}",
                display_number(metric, data.units, value),
                data.units.label(metric.unit()),
                time,
                source_label(&reading.source).to_lowercase()
            ));
        }
    }

    if let Some(stage) = data.stage_label(date) {
        parts.push(format!("stage {stage}"));
    }
    if let Some(space) = data.placement_label(date) {
        parts.push(format!("in {space}"));
    }

    if metric.is_air() {
        let view = data.view.get();
        let hourly = data.range.get().hourly();
        let mut parts_to_show = Vec::new();
        if hourly || view.draws_day() {
            parts_to_show.push((DayPart::Day, "day opt"));
        }
        if hourly || view.draws_night() {
            parts_to_show.push((DayPart::Night, "night opt"));
        }
        let multiple = parts_to_show.len() > 1;
        let mut survival: Vec<(DayPart, String)> = Vec::new();
        for (part, label) in parts_to_show {
            let Some(reference) = data.guide(metric, part, date) else {
                continue;
            };
            if let (Some(min), Some(max)) = (reference.opt_min, reference.opt_max) {
                parts.push(format!(
                    "{label} {}–{} {}",
                    display_number(metric, data.units, min),
                    display_number(metric, data.units, max),
                    data.units.label(metric.unit())
                ));
            }
            if let Some(text) = survival_text(metric, data.units, reference) {
                survival.push((part, text));
            }
        }

        if survival.len() == 2 && survival[0].1 == survival[1].1 {
            parts.push(format!("survives {}", survival[0].1));
        } else {
            for (part, text) in survival {
                parts.push(if multiple {
                    format!(
                        "survives {} {text}",
                        match part {
                            DayPart::Day => "day",
                            DayPart::Night => "night",
                        }
                    )
                } else {
                    format!("survives {text}")
                });
            }
        }
    }

    if parts.is_empty() {
        format!("{day} · no data")
    } else {
        format!("{day} · {}", parts.join(" · "))
    }
}

fn survival_text(metric: Metric, units: UnitPreference, reference: Reference) -> Option<String> {
    let unit = units.label(metric.unit());
    match (reference.survival_min, reference.survival_max) {
        (Some(min), Some(max)) => Some(format!(
            "{}–{} {unit}",
            display_number(metric, units, min),
            display_number(metric, units, max)
        )),
        (Some(min), None) => Some(format!("≥ {} {unit}", display_number(metric, units, min))),
        (None, Some(max)) => Some(format!("≤ {} {unit}", display_number(metric, units, max))),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::ClimateReading;
    use crate::planting::PlantingPlacement;
    use crate::ui::climate::data::{
        ClimateData, Guides, SourceState, default_sources, default_view,
    };
    use crate::units::UnitPreference;
    use std::cell::Cell;

    #[test]
    fn the_axis_uses_round_ticks() {
        assert_eq!(nice_ticks(18.9, 21.5), vec![19.0, 20.0, 21.0]);
        assert_eq!(nice_ticks(2.9, 16.9), vec![5.0, 10.0, 15.0]);
        assert_eq!(nice_ticks(0.0, 1.0), vec![0.0, 0.5, 1.0]);
    }

    fn graph_data(
        range: Range,
        selected: Option<Date>,
        weather_days: Vec<crate::weather::WeatherDay>,
        readings: Vec<ClimateReading>,
        anchor_selected: bool,
    ) -> ClimateData {
        let placements = if anchor_selected {
            vec![PlantingPlacement {
                id: 1,
                planting_id: 1,
                space_id: 1,
                started_on: Date::new(2000, 1, 1).unwrap(),
                ended_on: None,
                space_kind: "outdoor".into(),
                light_type: None,
                light_hours: None,
                light_start: None,
            }]
        } else {
            Vec::new()
        };
        let sources: SourceState = default_sources();
        ClimateData::new(
            weather_days,
            Vec::new(),
            readings,
            placements,
            Vec::new(),
            UnitPreference::Metric,
            Rc::new(Cell::new(selected)),
            Rc::new(Cell::new(range)),
            sources,
            default_view(),
            Guides::default(),
            Vec::new(),
            None,
            None,
            false,
            anchor_selected,
            None,
        )
    }

    fn weather_day(date: Date, min: f64, max: f64, mean: f64) -> crate::weather::WeatherDay {
        crate::weather::WeatherDay {
            latitude: 52.0,
            longitude: 6.0,
            date,
            temp_min_c: Some(min),
            temp_max_c: Some(max),
            temp_mean_c: Some(mean),
            humidity_mean_pct: Some(70.0),
            humidity_min_pct: Some(50.0),
            humidity_max_pct: Some(90.0),
            precipitation_mm: Some(0.0),
            precipitation_hours: None,
            weather_code: None,
            sunshine_hours: None,
            daylight_hours: None,
            radiation_sum_mj_m2: None,
            wind_max_kmh: None,
            wind_gust_max_kmh: None,
            et0_mm: None,
            sunrise: Some("08:00".into()),
            sunset: Some("18:00".into()),
            night_start: Some(format!("{}T18:00", date.add_days(-1).to_iso())),
            night_end: Some(format!("{}T08:00", date.to_iso())),
            night_temp_avg_c: Some(6.0),
            night_temp_min_c: Some(4.0),
            night_humidity_avg_pct: Some(85.0),
            night_hours: Some(14.0),
            night_complete: true,
        }
    }

    #[test]
    fn a_24h_bar_falls_back_to_the_daily_value_for_a_past_day() {
        let date = Date::new(2026, 5, 12).unwrap();
        let mut day = weather_day(date, 8.0, 16.0, 12.0);
        day.precipitation_mm = Some(3.0);
        day.sunshine_hours = Some(4.0);
        let data = graph_data(Range::Day, Some(date), vec![day], Vec::new(), true);

        assert!(data.has_weather(Metric::Rain, date, date));
        assert_eq!(data.day_value(Metric::Rain, date), Some(3.0));
        assert!(data.has_weather(Metric::Temperature, date, date));
        let (min, max) = scale(&data, Metric::Temperature, date, date).unwrap();
        assert!(min <= 12.0 && max >= 12.0, "{min}–{max}");
        let (_, rain_max) = scale(&data, Metric::Rain, date, date).unwrap();
        assert!(rain_max >= 3.0, "{rain_max}");
        let (_, sun_max) = scale(&data, Metric::Sunshine, date, date).unwrap();
        assert!(sun_max >= 4.0, "{sun_max}");
    }

    #[test]
    fn today_rain_does_not_fall_back_to_the_daily_forecast() {
        let date = crate::clock::today();
        let mut day = weather_day(date, 8.0, 16.0, 12.0);
        day.precipitation_mm = Some(3.0);
        let data = graph_data(Range::Day, Some(date), vec![day], Vec::new(), true);

        assert!(!data.has_weather(Metric::Rain, date, date));
        assert_eq!(data.day_value(Metric::Rain, date), None);
        let (_, rain_max) = scale(&data, Metric::Rain, date, date).unwrap();
        assert_eq!(rain_max, 1.0);
    }

    #[test]
    fn the_survival_text_uses_the_units() {
        let reference = Reference {
            opt_min: None,
            opt_max: None,
            survival_min: Some(0.0),
            survival_max: Some(30.0),
        };
        assert_eq!(
            survival_text(Metric::Temperature, UnitPreference::Metric, reference).as_deref(),
            Some("0–30 °C")
        );
    }

    #[test]
    fn a_weekly_rain_tooltip_uses_the_daily_bar() {
        let date = Date::new(2026, 5, 12).unwrap();
        let mut day = weather_day(date, 8.0, 16.0, 12.0);
        day.precipitation_mm = Some(3.0);
        let data = graph_data(Range::Days(7), Some(date), vec![day], Vec::new(), true);
        let text = tooltip(&data, Metric::Rain, Some((date, 12 * 60)));
        assert!(text.contains("3 mm rain"), "{text}");
    }

    #[test]
    fn a_pointer_position_maps_to_the_selected_day() {
        let date = Date::new(2026, 5, 12).unwrap();
        let data = graph_data(Range::Day, Some(date), Vec::new(), Vec::new(), true);
        let geometry = geometry();
        let mut point = point_at(&data, &geometry, 54.0).unwrap();
        assert_eq!(point.0, date);
        point = point_at(&data, &geometry, 800.0).unwrap();
        assert_eq!(point.0, date);
    }

    #[test]
    fn a_collapsed_pointer_snaps_to_the_day() {
        let date = Date::new(2026, 5, 12).unwrap();
        let data = graph_data(Range::Days(30), Some(date), Vec::new(), Vec::new(), true);
        let geometry = geometry();
        let point = point_at(&data, &geometry, 200.0).unwrap();
        assert_eq!(point.1, 12 * 60);
    }
}
