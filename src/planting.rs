//! Plantings and the spaces they grow in.
//!
//! A profile says what a plant needs; a planting says what actually happened. A space groups
//! plantings that share an environment (light, climate, sensors), so those values live in one
//! place instead of being copied onto every planting. The design is in `docs/plantings.md`.

use crate::calendar::Date;
use crate::plant_data::{
    ValidationError, validate_choice, validate_non_negative, validate_optional_choice,
};

/// The physical kind of a space. It decides whether light settings are shown and stored.
pub const SPACE_KINDS: &[&str] = &["outdoor", "greenhouse", "indoor"];
pub const LIGHT_TYPES: &[&str] = &["natural", "led", "hps", "mixed"];
pub const CONTAINERS: &[&str] = &["ground", "pot", "bed", "hydroponics"];
pub const START_KINDS: &[&str] = &["planted", "sown_indoor", "sown_outdoor", "transplanted"];
pub const OUTCOMES: &[&str] = &["harvested", "removed", "died", "failed"];

pub const DEFAULT_SPACE_KIND: &str = "outdoor";
pub const DEFAULT_CONTAINER: &str = "ground";
pub const DEFAULT_START_KIND: &str = "planted";

/// A place whose environment is shared by the plantings inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Space {
    pub id: i64,
    /// `None` for a planting that stands on its own; such a space is not listed as a place.
    pub name: Option<String>,
    pub kind: String,
    pub light_type: Option<String>,
    pub light_hours: Option<f64>,
    /// When powered light comes on, in minutes after local midnight. `None` for natural light or
    /// when the schedule is incomplete.
    pub light_start: Option<i64>,
    pub notes: Option<String>,
}

impl Space {
    /// The name to show, or `None` for a private space.
    pub fn display_name(&self) -> Option<&str> {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }

    /// Light settings are meaningful indoors and in a greenhouse, not in the open garden.
    pub fn has_light(&self) -> bool {
        self.kind != "outdoor"
    }
}

/// One stretch of time a planting spent in a space.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantingPlacement {
    pub id: i64,
    pub planting_id: i64,
    pub space_id: i64,
    pub started_on: Date,
    pub ended_on: Option<Date>,
    pub space_kind: String,
    pub light_type: Option<String>,
    pub light_hours: Option<f64>,
    pub light_start: Option<i64>,
}

impl PlantingPlacement {
    /// Whether a date falls in this placement. Both ends are inclusive; the next placement starts
    /// on the day after a move.
    pub fn covers(&self, date: Date) -> bool {
        date >= self.started_on && self.ended_on.is_none_or(|ended| date <= ended)
    }
}

/// One variety, one group, one cycle, in one space.
#[derive(Clone, Debug, PartialEq)]
pub struct Planting {
    pub id: i64,
    pub profile_id: i64,
    pub space_id: i64,
    pub name: Option<String>,
    pub quantity: i64,
    pub container: String,
    pub container_volume_l: Option<f64>,
    pub substrate: Option<String>,
    pub started_on: Date,
    pub start_kind: String,
    pub ended_on: Option<Date>,
    pub outcome: Option<String>,
    pub notes: Option<String>,
}

impl Planting {
    /// The user's name for this planting, or the profile name when it has none.
    pub fn title<'a>(&'a self, profile_name: &'a str) -> &'a str {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(profile_name)
    }

    /// Derived from the dates, so the state can never drift away from them.
    pub fn state(&self, today: Date) -> PlantingState {
        if self.ended_on.is_some() {
            PlantingState::Finished
        } else if self.started_on > today {
            PlantingState::Planned
        } else {
            PlantingState::Growing
        }
    }

    /// Day 1 is the start date. For a planned planting this is zero or negative.
    pub fn day_number(&self, today: Date) -> i64 {
        self.started_on.days_until(today) + 1
    }
}

/// A planting with the profile and space names the list needs.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantingSummary {
    pub planting: Planting,
    pub profile_name: String,
    pub profile_species: Option<String>,
    pub profile_cultivar: Option<String>,
    /// `None` when the planting has a private space.
    pub space_name: Option<String>,
}

impl PlantingSummary {
    pub fn title(&self) -> &str {
        self.planting.title(&self.profile_name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlantingState {
    Planned,
    Growing,
    Finished,
}

/// The fields the setup form writes. Finish and reopen are separate actions.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantingInput {
    pub profile_id: i64,
    pub space_id: i64,
    pub name: Option<String>,
    pub quantity: i64,
    pub container: String,
    pub container_volume_l: Option<f64>,
    pub substrate: Option<String>,
    pub started_on: Date,
    pub start_kind: String,
    /// Set when an existing planting changes space. `None` on create or when the space stays.
    pub moved_on: Option<Date>,
    pub notes: Option<String>,
}

impl PlantingInput {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.quantity < 1 {
            return Err(ValidationError("quantity must be at least 1".into()));
        }

        validate_choice(&self.container, CONTAINERS, "container")?;
        validate_choice(&self.start_kind, START_KINDS, "start kind")?;
        validate_non_negative(self.container_volume_l, "container volume")?;

        if self.container != "pot" && self.container_volume_l.is_some() {
            return Err(ValidationError(
                "a pot volume only applies to a pot; clear it or choose pot".into(),
            ));
        }

        if let Some(moved_on) = self.moved_on
            && moved_on < self.started_on
        {
            return Err(ValidationError(
                "a move cannot be before the planting started".into(),
            ));
        }

        Ok(())
    }
}

/// The fields the space form writes.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceInput {
    /// `None` or empty makes it a private space for one planting.
    pub name: Option<String>,
    pub kind: String,
    pub light_type: Option<String>,
    pub light_hours: Option<f64>,
    /// Minutes after local midnight; only for a powered light with a schedule.
    pub light_start: Option<i64>,
    pub notes: Option<String>,
}

impl SpaceInput {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_choice(&self.kind, SPACE_KINDS, "space kind")?;
        validate_optional_choice(self.light_type.as_deref(), LIGHT_TYPES, "light type")?;

        if let Some(hours) = self.light_hours
            && (!hours.is_finite() || !(0.0..=24.0).contains(&hours))
        {
            return Err(ValidationError(
                "light hours must be between 0 and 24".into(),
            ));
        }

        if let Some(minutes) = self.light_start
            && !(0..24 * 60).contains(&minutes)
        {
            return Err(ValidationError(
                "the light start time must be between 00:00 and 23:59".into(),
            ));
        }

        if self.light_start.is_some() && self.light_hours.is_none() {
            return Err(ValidationError(
                "set light hours with the light start time".into(),
            ));
        }

        if self.light_start.is_some()
            && !matches!(self.light_type.as_deref(), Some("led" | "hps" | "mixed"))
        {
            return Err(ValidationError(
                "a light start time belongs to LED, HPS or mixed light".into(),
            ));
        }

        if self.kind == "outdoor"
            && (self.light_type.is_some()
                || self.light_hours.is_some()
                || self.light_start.is_some())
        {
            return Err(ValidationError(
                "an outdoor space has natural light; clear the light settings".into(),
            ));
        }

        Ok(())
    }
}

/// Validate an outcome before a planting is closed.
pub fn validate_outcome(outcome: &str) -> Result<(), ValidationError> {
    validate_choice(outcome, OUTCOMES, "outcome")
}

#[cfg(test)]
mod tests {
    use super::{
        CONTAINERS, Planting, PlantingInput, PlantingState, START_KINDS, SpaceInput,
        validate_outcome,
    };
    use crate::calendar::Date;

    fn planting(started: Date, ended: Option<Date>) -> Planting {
        Planting {
            id: 1,
            profile_id: 1,
            space_id: 1,
            name: None,
            quantity: 1,
            container: "ground".into(),
            container_volume_l: None,
            substrate: None,
            started_on: started,
            start_kind: "planted".into(),
            ended_on: ended,
            outcome: ended.map(|_| "harvested".into()),
            notes: None,
        }
    }

    #[test]
    fn derives_state_from_the_dates() {
        let today = Date::new(2026, 10, 30).unwrap();
        let tomorrow = Date::new(2026, 10, 31).unwrap();
        let yesterday = Date::new(2026, 10, 29).unwrap();

        assert_eq!(
            planting(tomorrow, None).state(today),
            PlantingState::Planned
        );
        assert_eq!(
            planting(yesterday, None).state(today),
            PlantingState::Growing
        );
        assert_eq!(
            planting(yesterday, Some(today)).state(today),
            PlantingState::Finished
        );
    }

    #[test]
    fn counts_the_start_day_as_day_one() {
        let start = Date::new(2026, 5, 1).unwrap();
        assert_eq!(planting(start, None).day_number(start), 1);
        assert_eq!(
            planting(start, None).day_number(Date::new(2026, 5, 11).unwrap()),
            11
        );
        assert_eq!(
            planting(Date::new(2026, 5, 5).unwrap(), None).day_number(start),
            -3
        );
    }

    #[test]
    fn falls_back_to_the_profile_name() {
        let start = Date::new(2026, 5, 1).unwrap();
        let mut row = planting(start, None);
        assert_eq!(row.title("Ghost Pepper"), "Ghost Pepper");

        row.name = Some("  ".into());
        assert_eq!(row.title("Ghost Pepper"), "Ghost Pepper");

        row.name = Some("Windowsill chilli".into());
        assert_eq!(row.title("Ghost Pepper"), "Windowsill chilli");
    }

    fn input() -> PlantingInput {
        PlantingInput {
            profile_id: 1,
            space_id: 1,
            name: None,
            quantity: 1,
            container: "ground".into(),
            container_volume_l: None,
            substrate: None,
            started_on: Date::new(2026, 5, 1).unwrap(),
            start_kind: "planted".into(),
            moved_on: None,
            notes: None,
        }
    }

    #[test]
    fn rejects_an_impossible_planting() {
        let mut row = input();
        row.quantity = 0;
        assert!(row.validate().is_err());

        let mut row = input();
        row.container = "bucket".into();
        assert!(row.validate().is_err());

        let mut row = input();
        row.container_volume_l = Some(-1.0);
        assert!(row.validate().is_err());

        let mut row = input();
        row.start_kind = "thrown".into();
        assert!(row.validate().is_err());

        assert!(input().validate().is_ok());
        assert!(CONTAINERS.contains(&"pot"));
        assert!(START_KINDS.contains(&"transplanted"));
    }

    #[test]
    fn a_volume_belongs_to_a_pot() {
        let mut row = input();
        row.container_volume_l = Some(3.0);
        assert!(row.validate().is_err());

        row.container = "pot".into();
        assert!(row.validate().is_ok());
    }

    #[test]
    fn validates_the_shared_light() {
        let space = |kind: &str, light_type: Option<&str>, hours: Option<f64>| SpaceInput {
            name: Some("Grow tent".into()),
            kind: kind.into(),
            light_type: light_type.map(str::to_string),
            light_hours: hours,
            light_start: None,
            notes: None,
        };

        assert!(space("indoor", Some("led"), Some(12.0)).validate().is_ok());
        assert!(space("outdoor", None, None).validate().is_ok());
        assert!(space("indoor", Some("led"), Some(25.0)).validate().is_err());
        assert!(space("indoor", Some("candle"), None).validate().is_err());
        assert!(space("outdoor", Some("led"), None).validate().is_err());
        assert!(space("rooftop", None, None).validate().is_err());
    }

    #[test]
    fn a_light_start_belongs_to_a_powered_schedule() {
        let base = SpaceInput {
            name: Some("Tent".into()),
            kind: "indoor".into(),
            light_type: Some("led".into()),
            light_hours: Some(16.0),
            light_start: Some(6 * 60),
            notes: None,
        };
        assert!(base.validate().is_ok());

        let mut missing_hours = base.clone();
        missing_hours.light_hours = None;
        assert!(missing_hours.validate().is_err());

        let mut natural = base.clone();
        natural.light_type = Some("natural".into());
        assert!(natural.validate().is_err());

        let mut outdoor = base;
        outdoor.kind = "outdoor".into();
        assert!(outdoor.validate().is_err());
    }

    #[test]
    fn accepts_only_known_outcomes() {
        for outcome in ["harvested", "removed", "died", "failed"] {
            assert!(validate_outcome(outcome).is_ok());
        }
        assert!(validate_outcome("eaten").is_err());
    }
}
