//! The user's frost dates.
//!
//! A timing rule is relative ("sow indoors 70–56 days before the last frost"); these two dates turn
//! the frost-anchored rules into calendar dates. They belong to the location, not to a plant, and
//! live in the settings table as `MM-DD`. The order the app reads them in follows the system locale
//! (`date_format`), so neither needs a migration.

use crate::calendar::Date;
use crate::plant_data::TimingRule;
use crate::store::{self, Store};

const LAST_KEY: &str = "last_frost";
const FIRST_KEY: &str = "first_frost";
const SOURCE_KEY: &str = "frost_source";

/// The last spring frost and the first autumn frost, as month and day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrostDates {
    pub last: Option<(u32, u32)>,
    pub first: Option<(u32, u32)>,
}

impl FrostDates {
    pub fn load(store: &Store) -> store::Result<Self> {
        Ok(Self {
            last: store.setting(LAST_KEY)?.as_deref().and_then(parse_stored),
            first: store.setting(FIRST_KEY)?.as_deref().and_then(parse_stored),
        })
    }

    pub fn save(&self, store: &Store) -> store::Result<()> {
        store.set_setting(LAST_KEY, &format_stored(self.last))?;
        store.set_setting(FIRST_KEY, &format_stored(self.first))?;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.last.is_none() && self.first.is_none()
    }
}

/// Where an estimated pair of dates came from. Empty means the user typed them.
pub fn load_source(store: &Store) -> Option<String> {
    store
        .setting(SOURCE_KEY)
        .ok()
        .flatten()
        .map(|source| source.trim().to_string())
        .filter(|source| !source.is_empty())
}

pub fn save_source(store: &Store, source: &str) -> store::Result<()> {
    store.set_setting(SOURCE_KEY, source.trim())
}

/// Saves an estimated pair together with the line that explains where it came from. A date the
/// archive could not find keeps its current value; a place without frost in one season should not
/// wipe a date the user already had. Returns the resulting pair.
pub fn apply_estimate(
    store: &Store,
    estimate: &crate::frost_lookup::Estimate,
) -> store::Result<FrostDates> {
    let mut dates = FrostDates::load(store)?;

    if estimate.last.is_some() {
        dates.last = estimate.last;
    }
    if estimate.first.is_some() {
        dates.first = estimate.first;
    }

    dates.save(store)?;
    save_source(store, &estimate.source())?;

    Ok(dates)
}

/// Saves dates typed by the user. Returns whether they changed; a change drops the estimate
/// source, because those dates are no longer the ones the archive produced.
pub fn save_typed(store: &Store, dates: FrostDates) -> store::Result<bool> {
    if FrostDates::load(store)? == dates {
        return Ok(false);
    }

    dates.save(store)?;
    if load_source(store).is_some() {
        save_source(store, "")?;
    }

    Ok(true)
}

/// Resolves a frost-anchored rule against these dates, for the current year.
///
/// `None` when the rule has another anchor or the needed frost date is unknown.
pub fn resolve(rule: &TimingRule, frost: &FrostDates, year: i32) -> Option<(Date, Date)> {
    let base = match rule.anchor.as_str() {
        "last_frost" => frost.last,
        "first_frost" => frost.first,
        _ => None,
    }?;
    let base = Date::in_year(year, base.0, base.1)?;

    let start = base.add_days(rule.offset_min_days.unwrap_or(0));
    let end = base.add_days(rule.offset_max_days.or(rule.offset_min_days).unwrap_or(0));

    Some((start.min(end), start.max(end)))
}

fn parse_stored(text: &str) -> Option<(u32, u32)> {
    let (month, day) = text.split_once('-')?;
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    Date::new(2028, month, day)?;
    Some((month, day))
}

fn format_stored(value: Option<(u32, u32)>) -> String {
    match value {
        Some((month, day)) => format!("{month:02}-{day:02}"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{FrostDates, resolve};
    use crate::calendar::Date;
    use crate::plant_data::TimingRule;

    fn rule(anchor: &str, min: Option<i64>, max: Option<i64>) -> TimingRule {
        TimingRule {
            id: 1,
            profile_id: 1,
            stage_id: None,
            kind: "sow_indoor".to_string(),
            anchor: anchor.to_string(),
            offset_min_days: min,
            offset_max_days: max,
            month_start: None,
            month_end: None,
            day_length_hours: None,
            gdd_base_c: None,
            gdd_target: None,
            repeat_days: None,
            notes: None,
        }
    }

    #[test]
    fn keeps_the_source_of_an_estimate() {
        let store = crate::store::Store::open_in_memory().unwrap();
        assert_eq!(super::load_source(&store), None);

        super::save_source(&store, " Open-Meteo ERA5-Land ").unwrap();
        assert_eq!(
            super::load_source(&store).as_deref(),
            Some("Open-Meteo ERA5-Land")
        );

        super::save_source(&store, "").unwrap();
        assert_eq!(super::load_source(&store), None);
    }

    #[test]
    fn a_typed_date_drops_the_estimate_source() {
        let store = crate::store::Store::open_in_memory().unwrap();
        let estimate = crate::frost_lookup::Estimate {
            last: Some((4, 20)),
            first: Some((11, 7)),
            years: 30,
        };

        super::apply_estimate(&store, &estimate).unwrap();
        assert_eq!(
            FrostDates::load(&store).unwrap(),
            FrostDates {
                last: Some((4, 20)),
                first: Some((11, 7))
            }
        );
        assert!(super::load_source(&store).is_some_and(|source| source.contains("1991")));

        // The UI writes the fields after the estimate; that debounced save must keep the source.
        let same = FrostDates {
            last: Some((4, 20)),
            first: Some((11, 7)),
        };
        assert!(!super::save_typed(&store, same).unwrap());
        assert!(super::load_source(&store).is_some());

        let edited = FrostDates {
            last: Some((4, 25)),
            first: Some((11, 7)),
        };
        assert!(super::save_typed(&store, edited).unwrap());
        assert_eq!(FrostDates::load(&store).unwrap(), edited);
        assert_eq!(super::load_source(&store), None);
    }

    #[test]
    fn a_partial_estimate_keeps_the_other_date() {
        let store = crate::store::Store::open_in_memory().unwrap();
        FrostDates {
            last: None,
            first: Some((11, 20)),
        }
        .save(&store)
        .unwrap();

        let estimate = crate::frost_lookup::Estimate {
            last: Some((4, 15)),
            first: None,
            years: 30,
        };

        let dates = super::apply_estimate(&store, &estimate).unwrap();
        assert_eq!(
            dates,
            FrostDates {
                last: Some((4, 15)),
                first: Some((11, 20))
            }
        );
        assert!(
            super::load_source(&store)
                .is_some_and(|source| source.contains("last spring frost only"))
        );
    }

    #[test]
    fn resolves_before_the_last_frost() {
        let frost = FrostDates {
            last: Some((4, 15)),
            first: None,
        };

        let (start, end) =
            resolve(&rule("last_frost", Some(-70), Some(-56)), &frost, 2027).unwrap();
        assert_eq!(start, Date::new(2027, 2, 4).unwrap());
        assert_eq!(end, Date::new(2027, 2, 18).unwrap());
    }

    #[test]
    fn resolves_after_the_first_frost_and_into_next_year() {
        let frost = FrostDates {
            last: None,
            first: Some((11, 20)),
        };

        let (start, end) = resolve(&rule("first_frost", Some(30), Some(45)), &frost, 2027).unwrap();
        assert_eq!(start, Date::new(2027, 12, 20).unwrap());
        assert_eq!(end, Date::new(2028, 1, 4).unwrap());
    }

    #[test]
    fn needs_the_matching_frost_date_and_anchor() {
        let frost = FrostDates {
            last: Some((4, 15)),
            first: None,
        };

        assert!(resolve(&rule("first_frost", Some(0), Some(0)), &frost, 2027).is_none());
        assert!(resolve(&rule("transplant", Some(0), Some(0)), &frost, 2027).is_none());
        assert!(
            resolve(
                &rule("last_frost", Some(0), Some(0)),
                &FrostDates::default(),
                2027
            )
            .is_none()
        );
    }
}
