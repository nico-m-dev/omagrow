//! The civil day where the user is.
//!
//! `Date::today` stays the UTC fallback, but a move, a picker default and a "planned" state
//! should agree with the calendar on the wall. One function keeps them from drifting apart.

use crate::calendar::Date;

/// The local civil date and the minutes after midnight. `None` when GLib cannot read the clock.
pub fn now() -> Option<(Date, i64)> {
    let now = gtk::glib::DateTime::now_local().ok()?;
    let date = Date::new(now.year(), now.month() as u32, now.day_of_month() as u32)?;
    let minutes = i64::from(now.hour()) * 60 + i64::from(now.minute());
    Some((date, minutes))
}

pub fn today() -> Date {
    now().map(|(date, _)| date).unwrap_or_else(Date::today)
}

#[cfg(test)]
mod tests {
    #[test]
    fn today_is_within_a_day_of_utc() {
        let local = super::today();
        let utc = crate::calendar::Date::today();
        assert!(local.days_until(utc).abs() <= 1);
    }

    #[test]
    fn now_gives_a_time_of_day() {
        let (_, minutes) = super::now().expect("a local clock");
        assert!((0..24 * 60).contains(&minutes));
    }
}
