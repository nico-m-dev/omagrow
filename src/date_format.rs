//! How the user types and reads a month and a day.
//!
//! Frost dates are stored as `MM-DD`. The order the app shows them in follows `LC_TIME`, the
//! system's date and time locale: day-first almost everywhere, month-first for countries that use
//! it. That is deliberately not the UI language (`LC_MESSAGES`), so the app can speak English while
//! reading dates the local way. Presentation only; stored data never changes.

use crate::calendar::Date;

/// Countries where a numeric date normally starts with the month.
const MONTH_FIRST: &[&str] = &[
    "US", "PR", "GU", "AS", "VI", "MP", "UM", "PH", "BZ", "FM", "MH", "PW",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DateFormat {
    #[default]
    DayMonth,
    MonthDay,
}

impl DateFormat {
    /// Follows `LC_TIME`, falling back to day-first when the locale names no country.
    pub fn system() -> Self {
        locale_tag(|key| std::env::var(key).ok())
            .and_then(|tag| Self::from_locale(&tag))
            .unwrap_or(Self::DayMonth)
    }

    fn from_locale(name: &str) -> Option<Self> {
        let country = name.split('_').nth(1)?;
        let country = country.split(['.', '@']).next()?.trim();
        if country.len() != 2 {
            return None;
        }

        Some(Self::for_country(country))
    }

    fn for_country(country: &str) -> Self {
        let country = country.to_ascii_uppercase();
        if MONTH_FIRST.contains(&country.as_str()) {
            Self::MonthDay
        } else {
            Self::DayMonth
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DayMonth => "day/month",
            Self::MonthDay => "month/day",
        }
    }

    pub fn example(self) -> &'static str {
        match self {
            Self::DayMonth => "15/04",
            Self::MonthDay => "04/15",
        }
    }

    /// "15/04" or "04/15"; empty for an unknown date.
    pub fn format(self, value: Option<(u32, u32)>) -> String {
        match (value, self) {
            (Some((month, day)), Self::DayMonth) => format!("{day:02}/{month:02}"),
            (Some((month, day)), Self::MonthDay) => format!("{month:02}/{day:02}"),
            (None, _) => String::new(),
        }
    }

    /// Reads `15/04`, `15-04` or `15.04` in the chosen order. Empty input means "unknown".
    pub fn parse(self, text: &str) -> Result<Option<(u32, u32)>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }

        let parts: Vec<&str> = text.split(['/', '-', '.']).collect();
        let hint = || format!("use {}, for example {}", self.label(), self.example());
        if parts.len() != 2 {
            return Err(hint());
        }

        let first: u32 = parts[0].trim().parse().map_err(|_| hint())?;
        let second: u32 = parts[1].trim().parse().map_err(|_| hint())?;
        let (day, month) = match self {
            Self::DayMonth => (first, second),
            Self::MonthDay => (second, first),
        };

        // A leap year so 29 February is accepted; only the month and day are stored.
        Date::new(2028, month, day).ok_or_else(|| format!("{text} is not a date"))?;

        Ok(Some((month, day)))
    }
}

/// The POSIX order: `LC_ALL` overrides everything, then `LC_TIME`, then `LANG`.
fn locale_tag(mut get: impl FnMut(&str) -> Option<String>) -> Option<String> {
    ["LC_ALL", "LC_TIME", "LANG"]
        .iter()
        .find_map(|key| get(key).filter(|value| !value.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::{DateFormat, locale_tag};

    fn tag(values: &[(&str, &str)]) -> Option<String> {
        locale_tag(|key| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        })
    }

    #[test]
    fn the_date_locale_wins_from_the_message_locale() {
        assert_eq!(
            tag(&[("LC_TIME", "nl_BE.UTF-8"), ("LANG", "en_US.UTF-8")]).as_deref(),
            Some("nl_BE.UTF-8")
        );
        assert_eq!(tag(&[]), None);
    }

    #[test]
    fn lc_all_overrides_the_date_locale() {
        assert_eq!(
            tag(&[("LC_ALL", "en_US.UTF-8"), ("LC_TIME", "nl_BE.UTF-8")]).as_deref(),
            Some("en_US.UTF-8")
        );
    }

    #[test]
    fn an_empty_value_falls_through() {
        assert_eq!(
            tag(&[("LC_TIME", " "), ("LANG", "nl_BE.UTF-8")]).as_deref(),
            Some("nl_BE.UTF-8")
        );
    }

    #[test]
    fn reads_day_first_by_default() {
        assert_eq!(DateFormat::DayMonth.parse("15/04").unwrap(), Some((4, 15)));
        assert_eq!(DateFormat::DayMonth.parse(" 1-5 ").unwrap(), Some((5, 1)));
        assert_eq!(DateFormat::DayMonth.parse("").unwrap(), None);
    }

    #[test]
    fn reads_month_first_when_chosen() {
        assert_eq!(DateFormat::MonthDay.parse("04/15").unwrap(), Some((4, 15)));
        assert_eq!(DateFormat::MonthDay.parse(" 4-1 ").unwrap(), Some((4, 1)));
    }

    #[test]
    fn the_two_orders_disagree_on_an_ambiguous_date() {
        assert_eq!(DateFormat::DayMonth.parse("04/05").unwrap(), Some((5, 4)));
        assert_eq!(DateFormat::MonthDay.parse("04/05").unwrap(), Some((4, 5)));
    }

    #[test]
    fn formats_in_the_chosen_order() {
        assert_eq!(DateFormat::DayMonth.format(Some((4, 15))), "15/04");
        assert_eq!(DateFormat::MonthDay.format(Some((4, 15))), "04/15");
        assert_eq!(DateFormat::DayMonth.format(None), "");
        assert_eq!(DateFormat::MonthDay.format(None), "");
    }

    #[test]
    fn rejects_nonsense() {
        assert!(DateFormat::DayMonth.parse("31/04").is_err());
        assert!(DateFormat::DayMonth.parse("april").is_err());
        assert!(DateFormat::DayMonth.parse("4").is_err());
        assert!(DateFormat::MonthDay.parse("13/13").is_err());
    }

    #[test]
    fn february_29_is_accepted() {
        assert_eq!(DateFormat::DayMonth.parse("29/02").unwrap(), Some((2, 29)));
        assert_eq!(DateFormat::MonthDay.parse("02/29").unwrap(), Some((2, 29)));
    }

    #[test]
    fn follows_the_country_in_the_locale() {
        assert_eq!(
            DateFormat::from_locale("en_US.UTF-8"),
            Some(DateFormat::MonthDay)
        );
        assert_eq!(DateFormat::from_locale("en_PH"), Some(DateFormat::MonthDay));
        assert_eq!(
            DateFormat::from_locale("nl_BE.UTF-8"),
            Some(DateFormat::DayMonth)
        );
        assert_eq!(
            DateFormat::from_locale("en_GB.UTF-8"),
            Some(DateFormat::DayMonth)
        );
        assert_eq!(
            DateFormat::from_locale("de_DE@euro"),
            Some(DateFormat::DayMonth)
        );
    }

    #[test]
    fn falls_back_when_the_locale_names_no_country() {
        assert_eq!(DateFormat::from_locale("en"), None);
        assert_eq!(DateFormat::from_locale("C"), None);
        assert_eq!(DateFormat::from_locale("POSIX"), None);
    }

    #[test]
    fn country_codes_are_case_insensitive() {
        assert_eq!(DateFormat::for_country("us"), DateFormat::MonthDay);
        assert_eq!(DateFormat::for_country("Nl"), DateFormat::DayMonth);
    }
}
