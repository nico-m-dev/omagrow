//! Civil dates without a date library.
//!
//! OmaGrow only needs "a month and a day, plus an offset in days" for now. These are the standard
//! civil-date conversions (Howard Hinnant's algorithm), so a February in a leap year behaves.

/// A calendar date, without a time or a timezone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// `None` for a day that does not exist in that month, such as 31 April.
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        if !(-9999..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
        {
            return None;
        }

        Some(Self { year, month, day })
    }

    /// A month and a day in `year`; 29 February falls back to the 28th in a common year.
    pub fn in_year(year: i32, month: u32, day: u32) -> Option<Self> {
        if month == 2 && day == 29 && !is_leap(year) {
            return Self::new(year, 2, 28);
        }

        Self::new(year, month, day)
    }

    pub fn add_days(self, days: i64) -> Self {
        // Keep the result inside the range the civil conversion can handle. A timing offset is
        // bounded at the store, but a stray huge value must not overflow the date math.
        let total = days_from_civil(self.year, self.month, self.day).saturating_add(days);
        let min = days_from_civil(-9999, 1, 1);
        let max = days_from_civil(9999, 12, 31);
        from_days(total.clamp(min, max))
    }

    /// Reads the ISO form the database uses: `2026-10-30`.
    pub fn parse_iso(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }

        Self::new(year, month, day)
    }

    /// The ISO form the database uses: `2026-10-30`. Sorts lexicographically.
    pub fn to_iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Days from this date to `other`: positive when `other` is later, zero on the same day.
    pub fn days_until(self, other: Self) -> i64 {
        days_from_civil(other.year, other.month, other.day)
            - days_from_civil(self.year, self.month, self.day)
    }

    /// The last day of this date's month.
    pub fn end_of_month(self) -> Self {
        Self {
            year: self.year,
            month: self.month,
            day: days_in_month(self.year, self.month),
        }
    }

    /// Monday is 0, Sunday is 6. 1970-01-01 was a Thursday.
    pub fn weekday_from_monday(self) -> u32 {
        (days_from_civil(self.year, self.month, self.day) + 3).rem_euclid(7) as u32
    }

    /// Today in UTC. The app does not need a timezone for a sowing window.
    pub fn today() -> Self {
        let days = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() / 86_400)
            .unwrap_or(0);

        from_days(days as i64)
    }

    /// "15 March" or "15 March 2027" when the year is not the current one.
    pub fn format_day_month(self, current_year: i32) -> String {
        let month = month_name(self.month);
        if self.year == current_year {
            format!("{} {month}", self.day)
        } else {
            format!("{} {month} {}", self.day, self.year)
        }
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

pub fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "?",
    }
}

pub fn month_short(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "?",
    }
}

/// Days since 1970-01-01.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = i64::from(year_of_era) * 365 + i64::from(year_of_era) / 4
        - i64::from(year_of_era) / 100
        + day_of_year;

    i64::from(era) * 146_097 + day_of_era - 719_468
}

fn from_days(days: i64) -> Date {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = month_index + if month_index < 10 { 3 } else { -9 };

    Date {
        year: (year + i64::from(month <= 2)) as i32,
        month: month as u32,
        day: day as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::Date;

    #[test]
    fn rejects_a_day_that_does_not_exist() {
        assert!(Date::new(2027, 4, 31).is_none());
        assert!(Date::new(2027, 2, 29).is_none());
        assert!(Date::new(2028, 2, 29).is_some());
        assert!(Date::new(2027, 13, 1).is_none());
    }

    #[test]
    fn adds_days_across_months_and_years() {
        let march = Date::new(2027, 3, 15).unwrap();
        assert_eq!(march.add_days(-40), Date::new(2027, 2, 3).unwrap());
        assert_eq!(march.add_days(16), Date::new(2027, 3, 31).unwrap());
        assert_eq!(march.add_days(17), Date::new(2027, 4, 1).unwrap());

        let december = Date::new(2027, 12, 20).unwrap();
        assert_eq!(december.add_days(20), Date::new(2028, 1, 9).unwrap());

        let january = Date::new(2028, 1, 5).unwrap();
        assert_eq!(january.add_days(-10), Date::new(2027, 12, 26).unwrap());
    }

    #[test]
    fn february_29_round_trips() {
        let leap = Date::new(2028, 2, 29).unwrap();
        assert_eq!(leap.add_days(0), leap);
        assert_eq!(leap.add_days(1), Date::new(2028, 3, 1).unwrap());
        assert_eq!(leap.add_days(-1), Date::new(2028, 2, 28).unwrap());
    }

    #[test]
    fn formats_with_the_year_only_when_it_differs() {
        let date = Date::new(2027, 3, 15).unwrap();
        assert_eq!(date.format_day_month(2027), "15 March");
        assert_eq!(date.format_day_month(2026), "15 March 2027");
    }

    #[test]
    fn reads_and_writes_the_iso_form() {
        let date = Date::new(2027, 3, 5).unwrap();
        assert_eq!(date.to_iso(), "2027-03-05");
        assert_eq!(Date::parse_iso("2027-03-05"), Some(date));
        assert_eq!(Date::parse_iso(" 2027-03-05 "), Some(date));
        assert_eq!(Date::parse_iso("2027-02-29"), None);
        assert_eq!(Date::parse_iso("2027-03"), None);
        assert_eq!(Date::parse_iso("2027-03-05-01"), None);
        assert_eq!(Date::parse_iso("tomorrow"), None);
    }

    #[test]
    fn measures_days_between_dates() {
        let start = Date::new(2027, 3, 15).unwrap();
        let today = Date::new(2027, 3, 20).unwrap();

        assert_eq!(start.days_until(today), 5);
        assert_eq!(today.days_until(start), -5);
        assert_eq!(start.days_until(start), 0);
        assert_eq!(start.days_until(Date::new(2028, 3, 15).unwrap()), 366);
    }

    #[test]
    fn knows_the_end_of_a_month_and_the_weekday() {
        assert_eq!(
            Date::new(2027, 2, 10).unwrap().end_of_month(),
            Date::new(2027, 2, 28).unwrap()
        );
        assert_eq!(
            Date::new(2028, 2, 10).unwrap().end_of_month(),
            Date::new(2028, 2, 29).unwrap()
        );

        // 2026-10-30 is a Friday; Monday is 0.
        assert_eq!(Date::new(2026, 10, 30).unwrap().weekday_from_monday(), 4);
        assert_eq!(Date::new(2026, 10, 26).unwrap().weekday_from_monday(), 0);
        assert_eq!(Date::new(2026, 11, 1).unwrap().weekday_from_monday(), 6);
    }

    #[test]
    fn an_extreme_offset_does_not_panic() {
        let date = Date::new(2026, 1, 1).unwrap();
        let _ = date.add_days(i64::MAX);
        let _ = date.add_days(i64::MIN);
        assert!(Date::new(10_000, 1, 1).is_none());
        assert!(Date::parse_iso("10000-01-01").is_none());
        assert_eq!(date.add_days(0), date);
    }
}
