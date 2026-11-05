//! Drawing primitives for the timeline and the graphs.
//!
//! Both used to carry their own copy of the colours, the alpha rule and the short date, which is
//! exactly how the two charts drifted apart. One copy keeps them honest.

use gtk::cairo;
use gtk::prelude::*;

use crate::calendar::{self, Date};

#[derive(Clone, Copy)]
pub(in crate::ui) enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy)]
pub(in crate::ui) struct Colors {
    pub fg: gtk::gdk::RGBA,
    pub bg: gtk::gdk::RGBA,
    pub accent: gtk::gdk::RGBA,
    pub success: gtk::gdk::RGBA,
    pub warning: gtk::gdk::RGBA,
    pub error: gtk::gdk::RGBA,
}

impl Colors {
    #[allow(deprecated)]
    pub(in crate::ui) fn load(widget: &impl IsA<gtk::Widget>) -> Self {
        let fg = widget.color();
        let context = widget.style_context();
        let lookup =
            |name: &str, fallback: gtk::gdk::RGBA| context.lookup_color(name).unwrap_or(fallback);

        Self {
            fg,
            bg: lookup("window_bg_color", fg),
            accent: lookup("accent_color", fg),
            success: lookup("omagrow_green", lookup("success_color", fg)),
            warning: lookup("warning_color", fg),
            error: lookup("error_color", fg),
        }
    }
}

/// Sets a colour as the source, scaling its own alpha by `alpha`.
pub(in crate::ui) fn paint(cr: &cairo::Context, color: gtk::gdk::RGBA, alpha: f64) {
    cr.set_source_rgba(
        color.red() as f64,
        color.green() as f64,
        color.blue() as f64,
        color.alpha() as f64 * alpha,
    );
}

pub(in crate::ui) fn text_width(cr: &cairo::Context, text: &str) -> f64 {
    cr.text_extents(text)
        .map(|extents| extents.width())
        .unwrap_or(0.0)
}

pub(in crate::ui) fn rounded_rect(
    cr: &cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    use std::f64::consts::{FRAC_PI_2, PI};

    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + width - radius, y + radius, radius, -FRAC_PI_2, 0.0);
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        FRAC_PI_2,
    );
    cr.arc(x + radius, y + height - radius, radius, FRAC_PI_2, PI);
    cr.arc(x + radius, y + radius, radius, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn text(
    cr: &cairo::Context,
    value: &str,
    x: f64,
    y: f64,
    size: f64,
    color: gtk::gdk::RGBA,
    alpha: f64,
    align: Align,
) {
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(size);
    let width = text_width(cr, value);
    let x = match align {
        Align::Left => x,
        Align::Center => x - width / 2.0,
        Align::Right => x - width,
    };
    paint(cr, color, alpha);
    cr.move_to(x, y);
    let _ = cr.show_text(value);
}

/// "30 Oct" in the current year, "30 Oct 2026" outside it.
pub(in crate::ui) fn short_date(date: Date, today: Date) -> String {
    let month = calendar::month_short(date.month);
    if date.year == today.year {
        format!("{} {month}", date.day)
    } else {
        format!("{} {month} {}", date.day, date.year)
    }
}

#[cfg(test)]
mod tests {
    use super::short_date;
    use crate::calendar::Date;

    #[test]
    fn a_short_date_keeps_the_year_only_when_it_differs() {
        let date = Date::new(2026, 10, 30).unwrap();
        assert_eq!(short_date(date, Date::new(2026, 1, 1).unwrap()), "30 Oct");
        assert_eq!(
            short_date(date, Date::new(2027, 1, 1).unwrap()),
            "30 Oct 2026"
        );
    }
}
