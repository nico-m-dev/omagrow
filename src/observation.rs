//! A diary entry for one planting: what happened, when, and with how much.
//!
//! An observation is a fact, not a requirement. It is append-only in spirit: the UI can delete a
//! mistaken entry, but it never overwrites a profile value. A `repot` entry updates the planting's
//! current pot volume and keeps the previous volume on the entry.

use crate::calendar::Date;
use crate::plant_data::ValidationError;
use crate::units::UnitPreference;

pub const KINDS: &[&str] = &[
    "water", "feed", "prune", "repot", "pest", "disease", "note", "photo", "harvest", "stage",
    "support", "other",
];

pub const FILTERS: &[&str] = &["All", "Care", "Problems", "Notes", "Stages", "Harvest"];

#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    pub id: i64,
    pub planting_id: i64,
    pub kind: String,
    pub occurred_on: Date,
    /// Local `HH:MM`; `None` when the user did not record a time.
    pub occurred_time: Option<String>,
    pub note: Option<String>,
    pub amount: Option<f64>,
    /// The canonical unit that belongs to the kind: L, ml or g.
    pub unit: Option<String>,
    pub stage_id: Option<i64>,
    /// For a repot: the pot volume before the change.
    pub previous_amount: Option<f64>,
    pub previous_unit: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservationInput {
    pub planting_id: i64,
    pub kind: String,
    pub occurred_on: Date,
    pub occurred_time: Option<String>,
    pub note: Option<String>,
    pub amount: Option<f64>,
    pub unit: Option<String>,
    pub stage_id: Option<i64>,
}

impl ObservationInput {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !KINDS.contains(&self.kind.as_str()) {
            return Err(ValidationError(format!(
                "unknown diary kind {:?}",
                self.kind
            )));
        }

        if let Some(time) = &self.occurred_time {
            parse_time(time).map_err(ValidationError)?;
        }

        if let Some(amount) = self.amount
            && (!amount.is_finite() || amount < 0.0)
        {
            return Err(ValidationError(
                "an amount must be a non-negative number".into(),
            ));
        }

        match unit_for_kind(&self.kind) {
            Some(expected) => {
                if self.amount.is_some() && self.unit.as_deref() != Some(expected) {
                    return Err(ValidationError(format!(
                        "{} is recorded in {expected}",
                        kind_label(&self.kind)
                    )));
                }
                if self.amount.is_none() && self.unit.is_some() {
                    return Err(ValidationError(
                        "a unit needs an amount; clear it or enter one".into(),
                    ));
                }
            }
            None => {
                if self.amount.is_some() || self.unit.is_some() {
                    return Err(ValidationError(format!(
                        "{} does not take an amount",
                        kind_label(&self.kind)
                    )));
                }
            }
        }

        if self.kind == "stage" {
            if self.stage_id.is_none() {
                return Err(ValidationError("choose the stage that started".into()));
            }
        } else if self.stage_id.is_some() {
            return Err(ValidationError(
                "only a stage entry can point at a stage".into(),
            ));
        }

        Ok(())
    }
}

/// The canonical unit a kind records, or `None` for a kind without an amount.
pub fn unit_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "water" | "repot" => Some("L"),
        "feed" => Some("ml"),
        "harvest" => Some("g"),
        _ => None,
    }
}

pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "water" => "Watered",
        "feed" => "Fed",
        "prune" => "Pruned",
        "repot" => "Repotted",
        "pest" => "Pest",
        "disease" => "Disease",
        "note" => "Note",
        "photo" => "Photo",
        "harvest" => "Harvested",
        "stage" => "Stage",
        "support" => "Supported",
        _ => "Other",
    }
}

/// Which diary filter an entry belongs to. `All` matches everything.
pub fn matches_filter(kind: &str, filter: &str) -> bool {
    match filter {
        "Care" => matches!(kind, "water" | "feed" | "prune" | "repot" | "support"),
        "Problems" => matches!(kind, "pest" | "disease"),
        "Notes" => matches!(kind, "note" | "photo"),
        "Stages" => kind == "stage",
        "Harvest" => kind == "harvest",
        _ => true,
    }
}

/// A one-line label for the diary and the calendar. The stage label comes from the profile.
pub fn label(
    observation: &Observation,
    stage_label: Option<&str>,
    units: UnitPreference,
) -> String {
    let base = match observation.kind.as_str() {
        "stage" => format!("{} started", stage_label.unwrap_or("Stage")),
        "repot" => format_repot(observation, units),
        _ => kind_label(&observation.kind).to_string(),
    };

    if observation.kind == "repot" || observation.kind == "stage" {
        return base;
    }

    match (observation.amount, observation.unit.as_deref()) {
        (Some(amount), Some(unit)) => {
            format!(
                "{base} · {} {}",
                units.number(unit, amount),
                units.label(unit)
            )
        }
        _ => base,
    }
}

fn format_repot(observation: &Observation, units: UnitPreference) -> String {
    let new = match (observation.amount, observation.unit.as_deref()) {
        (Some(amount), Some(unit)) => Some(format!(
            "{} {}",
            units.number(unit, amount),
            units.label(unit)
        )),
        _ => None,
    };
    let old = match (
        observation.previous_amount,
        observation.previous_unit.as_deref(),
    ) {
        (Some(amount), Some(unit)) => Some(format!(
            "{} {}",
            units.number(unit, amount),
            units.label(unit)
        )),
        _ => None,
    };

    match (old, new) {
        (Some(old), Some(new)) => format!("Repotted · {old} → {new}"),
        (None, Some(new)) => format!("Repotted · {new}"),
        (Some(old), None) => format!("Repotted · was {old}"),
        (None, None) => "Repotted".to_string(),
    }
}

/// Reads `14:30`, `9:05` or an empty value. Empty means no time.
pub fn parse_time(text: &str) -> Result<Option<String>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }

    let Some((hours, minutes)) = text.split_once(':') else {
        return Err("use a time like 14:30".to_string());
    };
    let hours: u32 = hours
        .trim()
        .parse()
        .map_err(|_| "use a time like 14:30".to_string())?;
    let minutes: u32 = minutes
        .trim()
        .parse()
        .map_err(|_| "use a time like 14:30".to_string())?;

    if hours > 23 || minutes > 59 {
        return Err("use a time between 00:00 and 23:59".to_string());
    }

    Ok(Some(format!("{hours:02}:{minutes:02}")))
}

#[cfg(test)]
mod tests {
    use super::{Observation, ObservationInput, label, matches_filter, parse_time, unit_for_kind};
    use crate::calendar::Date;
    use crate::units::UnitPreference;

    fn input(kind: &str) -> ObservationInput {
        ObservationInput {
            planting_id: 1,
            kind: kind.into(),
            occurred_on: Date::new(2027, 5, 1).unwrap(),
            occurred_time: None,
            note: None,
            amount: None,
            unit: None,
            stage_id: None,
        }
    }

    fn observation(kind: &str, amount: Option<f64>, unit: Option<&str>) -> Observation {
        Observation {
            id: 1,
            planting_id: 1,
            kind: kind.into(),
            occurred_on: Date::new(2027, 5, 1).unwrap(),
            occurred_time: None,
            note: None,
            amount,
            unit: unit.map(str::to_string),
            stage_id: None,
            previous_amount: None,
            previous_unit: None,
        }
    }

    #[test]
    fn validates_the_kind_and_amount() {
        assert!(input("water").validate().is_ok());
        assert!(input("nonsense").validate().is_err());

        let mut water = input("water");
        water.amount = Some(2.0);
        water.unit = Some("L".into());
        assert!(water.validate().is_ok());

        water.unit = Some("g".into());
        assert!(water.validate().is_err());

        let mut harvest = input("harvest");
        harvest.amount = Some(500.0);
        harvest.unit = Some("g".into());
        assert!(harvest.validate().is_ok());

        let mut note = input("note");
        note.amount = Some(1.0);
        note.unit = Some("L".into());
        assert!(note.validate().is_err());
    }

    #[test]
    fn a_stage_entry_needs_a_stage() {
        let mut stage = input("stage");
        assert!(stage.validate().is_err());
        stage.stage_id = Some(2);
        assert!(stage.validate().is_ok());

        let mut water = input("water");
        water.stage_id = Some(2);
        assert!(water.validate().is_err());
    }

    #[test]
    fn reads_times_in_the_local_day() {
        assert_eq!(parse_time("").unwrap(), None);
        assert_eq!(parse_time(" 9:05 ").unwrap(), Some("09:05".into()));
        assert_eq!(parse_time("14:30").unwrap(), Some("14:30".into()));
        assert!(parse_time("24:00").is_err());
        assert!(parse_time("14:60").is_err());
        assert!(parse_time("afternoon").is_err());
    }

    #[test]
    fn knows_which_kinds_take_a_unit() {
        assert_eq!(unit_for_kind("water"), Some("L"));
        assert_eq!(unit_for_kind("repot"), Some("L"));
        assert_eq!(unit_for_kind("harvest"), Some("g"));
        assert_eq!(unit_for_kind("feed"), Some("ml"));
        assert_eq!(unit_for_kind("note"), None);
    }

    #[test]
    fn filters_by_category() {
        assert!(matches_filter("water", "Care"));
        assert!(matches_filter("pest", "Problems"));
        assert!(!matches_filter("pest", "Care"));
        assert!(matches_filter("photo", "Notes"));
        assert!(matches_filter("stage", "Stages"));
        assert!(matches_filter("harvest", "Harvest"));
        assert!(matches_filter("other", "All"));
    }

    #[test]
    fn labels_amounts_and_repots() {
        assert_eq!(
            label(
                &observation("water", Some(2.0), Some("L")),
                None,
                UnitPreference::Metric
            ),
            "Watered · 2 L"
        );

        let mut repot = observation("repot", Some(11.0), Some("L"));
        repot.previous_amount = Some(3.0);
        repot.previous_unit = Some("L".into());
        assert_eq!(
            label(&repot, None, UnitPreference::Metric),
            "Repotted · 3 L → 11 L"
        );

        assert_eq!(
            label(
                &observation("stage", None, None),
                Some("Flowering"),
                UnitPreference::Metric
            ),
            "Flowering started"
        );
    }

    #[test]
    fn labels_follow_the_unit_preference() {
        let water = observation("water", Some(3.785_411_784), Some("L"));
        assert_eq!(
            label(&water, None, UnitPreference::Imperial),
            "Watered · 1 US gal"
        );
    }
}
