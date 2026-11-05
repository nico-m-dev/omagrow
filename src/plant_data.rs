//! Typed plant data.
//!
//! These types are shared by storage and the UI. String enums are
//! stored as text so the schema can grow without a migration for every new
//! value; the constants below are the controlled vocabulary used for validation.

use std::fmt;

/// A profile with the scalar traits from the plant data model.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileDetails {
    pub id: i64,
    pub name: String,
    pub species: Option<String>,
    pub cultivar: Option<String>,
    pub family: Option<String>,
    pub genus: Option<String>,
    pub lifecycle: Option<String>,
    pub life_form: Option<String>,
    pub foliage: Option<String>,
    pub determinacy: Option<String>,
    pub growth_habit: Option<String>,
    pub sun_exposure: Option<String>,
    pub photoperiod_type: Option<String>,
    pub photoperiod_obligate: Option<bool>,
    pub critical_day_length_hours: Option<f64>,
    pub water_need: Option<String>,
    pub container_suitable: Option<bool>,
    pub min_container_volume_l_min: Option<f64>,
    pub min_container_volume_l_max: Option<f64>,
    pub days_to_maturity_min: Option<f64>,
    pub days_to_maturity_max: Option<f64>,
    pub notes: Option<String>,
}

/// One stage in a profile's growing cycle.
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub id: i64,
    pub profile_id: i64,
    pub position: i64,
    pub kind: String,
    pub label: String,
    pub duration_days_min: Option<f64>,
    pub duration_days_max: Option<f64>,
    pub notes: Option<String>,
    /// `None` keeps the derived rule for the stage kind; `Some` is the user's explicit answer.
    pub repeats: Option<bool>,
}

impl Stage {
    /// Whether the stage returns every year when the profile says nothing.
    ///
    /// The establishment stages happen once at the start of a plant's life; the rest is the cycle
    /// that a perennial repeats. This is the rule the app used before the explicit switch existed.
    pub fn repeats_each_year(&self) -> bool {
        self.repeats.unwrap_or(!matches!(
            self.kind.as_str(),
            "germination" | "seedling" | "establishment" | "juvenile"
        ))
    }
}

/// A range for one metric, optionally specific to a stage and day part.
///
/// `stage_id == None` is the profile-level default. `id` is ignored when the
/// row is created or upserted; it is filled in when a row is read.
#[derive(Clone, Debug, PartialEq)]
pub struct Requirement {
    pub id: i64,
    pub profile_id: i64,
    pub stage_id: Option<i64>,
    pub metric: String,
    pub day_part: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub opt_min: Option<f64>,
    pub opt_max: Option<f64>,
    pub survival_min: Option<f64>,
    pub survival_max: Option<f64>,
}

/// What requirement inheritance found for a stage and day part.
#[derive(Clone, Debug, PartialEq)]
pub enum RequirementResolution {
    /// The row that applies: the stage row or the profile default.
    Resolved(Requirement),
    /// The row that applies is marked not applicable, so the requirement does not exist here.
    /// This is different from `None` in `Store::resolved_requirement`, which means nobody has
    /// said anything yet.
    NotApplicable,
}

/// A relative timing rule such as "sow 6–8 weeks before the last frost".
#[derive(Clone, Debug, PartialEq)]
pub struct TimingRule {
    pub id: i64,
    pub profile_id: i64,
    pub stage_id: Option<i64>,
    pub kind: String,
    pub anchor: String,
    pub offset_min_days: Option<i64>,
    pub offset_max_days: Option<i64>,
    pub month_start: Option<i64>,
    pub month_end: Option<i64>,
    pub day_length_hours: Option<f64>,
    pub gdd_base_c: Option<f64>,
    pub gdd_target: Option<f64>,
    pub repeat_days: Option<i64>,
    pub notes: Option<String>,
}

/// A registered key for the long tail of plant data.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeDefinition {
    pub key: String,
    pub label: String,
    pub description: Option<String>,
    pub value_type: String,
    pub unit: Option<String>,
    pub category: String,
    pub applies_to: String,
    pub allowed_stages: Option<String>,
    pub allowed_lifecycles: Option<String>,
    pub enum_values: Option<String>,
    pub is_custom: bool,
}

/// One long-tail value, optionally specific to a stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    pub id: i64,
    pub profile_id: i64,
    pub stage_id: Option<i64>,
    pub key: String,
    pub value_num: Option<f64>,
    pub value_text: Option<String>,
    pub value_bool: Option<bool>,
}

/// Provenance for one value: its status, source and confidence.
#[derive(Clone, Debug, PartialEq)]
pub struct Provenance {
    pub entity_type: String,
    pub entity_id: i64,
    pub field: String,
    pub status: String,
    pub source_type: Option<String>,
    pub source_ref: Option<String>,
    pub retrieved_at: Option<String>,
    pub confidence: Option<String>,
    pub notes: Option<String>,
}

/// A validation failure that is not a database or I/O error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError(pub String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ValidationError {}

pub const STAGE_KINDS: &[&str] = &[
    "germination",
    "establishment",
    "seedling",
    "juvenile",
    "vegetative",
    "flowering",
    "fruiting",
    "ripening",
    "harvest",
    "dormancy",
    "senescence",
    "mature",
];

pub const METRICS: &[&str] = &[
    "air_temp",
    "soil_temp",
    "rh",
    "vpd",
    "photoperiod",
    "direct_sun_hours",
    "light_dli",
    "light_ppfd",
    "soil_ph",
    "soil_ec",
    "water_mm_week",
    "water_l_week",
    "co2",
    "leaf_wetness_hours",
];

pub const DAY_PARTS: &[&str] = &["day", "night", "any"];

pub const TIMING_ANCHORS: &[&str] = &[
    "last_frost",
    "first_frost",
    "sowing",
    "transplant",
    "stage_start",
    "day_length",
    "gdd",
    "calendar",
];

pub const TIMING_KINDS: &[&str] = &[
    "sow_indoor",
    "sow_outdoor",
    "transplant",
    "harvest",
    "prune",
    "feed",
    "support",
    "repot",
    "winter_protect",
    "flowering",
    "fruiting",
    "dormancy",
    "succession",
    "thinning",
    "deadheading",
    "bloom",
];

pub const VALUE_TYPES: &[&str] = &["number", "text", "boolean", "enum", "json"];

pub const APPLIES_TO: &[&str] = &["profile", "stage", "planting", "observation"];

pub const LIFECYCLES: &[&str] = &["annual", "biennial", "perennial", "ephemeral"];

pub const LIFE_FORMS: &[&str] = &[
    "herbaceous",
    "woody",
    "vine",
    "grass",
    "succulent",
    "bulb",
    "tuber",
];

pub const FOLIAGE: &[&str] = &["evergreen", "deciduous", "semi_evergreen"];

pub const DETERMINACY: &[&str] = &["determinate", "indeterminate", "not_applicable"];

pub const GROWTH_HABITS: &[&str] = &[
    "upright",
    "bushy",
    "climbing",
    "creeping",
    "rosette",
    "spreading",
];

pub const SUN_EXPOSURES: &[&str] = &["full_sun", "partial_sun", "partial_shade", "full_shade"];

pub const PHOTOPERIOD_TYPES: &[&str] = &["short_day", "long_day", "day_neutral", "autoflowering"];

pub const WATER_NEEDS: &[&str] = &["low", "medium", "high"];

pub const PROVENANCE_STATUSES: &[&str] = &["estimated", "confirmed", "not_applicable"];

pub const SOURCE_TYPES: &[&str] = &["user", "agent", "import", "label", "book", "website"];

pub const CONFIDENCE_LEVELS: &[&str] = &["low", "medium", "high"];

/// A number without trailing zeros, for display and reports.
pub fn format_number(value: f64) -> String {
    if value == 0.0 {
        "0".to_string()
    } else {
        value.to_string()
    }
}

/// The label a metric shows in the UI and in reports.
pub fn metric_label(metric: &str) -> &'static str {
    match metric {
        "air_temp" => "Air temperature",
        "soil_temp" => "Soil temperature",
        "rh" => "Relative humidity",
        "vpd" => "VPD",
        "photoperiod" => "Photoperiod",
        "direct_sun_hours" => "Direct sun",
        "light_dli" => "DLI",
        "light_ppfd" => "PPFD",
        "soil_ph" => "Soil pH",
        "soil_ec" => "Soil EC",
        "water_mm_week" => "Water (ground)",
        "water_l_week" => "Water (container)",
        "co2" => "CO₂",
        "leaf_wetness_hours" => "Leaf wetness",
        _ => "Unknown metric",
    }
}

/// The canonical unit for a metric. Values are stored in this unit everywhere; the UI prints it
/// and the agent JSON carries it, so there is one place to change.
pub fn metric_unit(metric: &str) -> &'static str {
    match metric {
        "air_temp" | "soil_temp" => "°C",
        "rh" => "%",
        "vpd" => "kPa",
        "photoperiod" | "direct_sun_hours" => "hours/day",
        "light_dli" => "mol/m²/day",
        "light_ppfd" => "µmol/m²/s",
        "soil_ph" => "pH",
        "soil_ec" => "mS/cm",
        "water_mm_week" => "mm/week",
        "water_l_week" => "L/plant/week",
        "co2" => "ppm",
        "leaf_wetness_hours" => "hours",
        _ => "",
    }
}

/// Check a stored string against a controlled vocabulary.
pub fn validate_choice(value: &str, allowed: &[&str], field: &str) -> Result<(), ValidationError> {
    if allowed.contains(&value) {
        return Ok(());
    }

    Err(ValidationError(format!(
        "invalid {field}: {value:?}; expected one of {}",
        allowed.join(", ")
    )))
}

/// Check an optional stored string against a controlled vocabulary.
pub fn validate_optional_choice(
    value: Option<&str>,
    allowed: &[&str],
    field: &str,
) -> Result<(), ValidationError> {
    match value {
        Some(value) => validate_choice(value, allowed, field),
        None => Ok(()),
    }
}

/// Validate a min/max pair: both may be absent, but not inverted.
pub fn validate_optional_range(
    min: Option<f64>,
    max: Option<f64>,
    label: &str,
) -> Result<(), ValidationError> {
    for value in [min, max].into_iter().flatten() {
        if !value.is_finite() {
            return Err(ValidationError(format!("{label} must be a finite number")));
        }
    }

    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(ValidationError(format!(
            "{label}: minimum must not exceed maximum"
        )));
    }

    Ok(())
}

/// Validate a non-negative number.
pub fn validate_non_negative(value: Option<f64>, label: &str) -> Result<(), ValidationError> {
    if let Some(value) = value
        && (!value.is_finite() || value < 0.0)
    {
        return Err(ValidationError(format!(
            "{label} must be a non-negative finite number"
        )));
    }

    Ok(())
}

/// The sanity range for air and soil temperature, in the one canonical unit.
pub const TEMPERATURE_MIN_C: f64 = -60.0;
pub const TEMPERATURE_MAX_C: f64 = 80.0;

fn bound_label(name: &str) -> &'static str {
    match name {
        "survival_min" => "the survival minimum",
        "survival_max" => "the survival maximum",
        "min" => "the growth minimum",
        "max" => "the growth maximum",
        "opt_min" => "the optimum minimum",
        "opt_max" => "the optimum maximum",
        _ => "a bound",
    }
}

/// Validate the six bounds of a requirement range.
///
/// The order is survival, growth, optimum: each present bound must not exceed
/// a later present bound. Missing bounds are allowed.
pub fn validate_requirement_range(requirement: &Requirement) -> Result<(), ValidationError> {
    let bounds = [
        ("survival_min", requirement.survival_min),
        ("min", requirement.min),
        ("opt_min", requirement.opt_min),
        ("opt_max", requirement.opt_max),
        ("max", requirement.max),
        ("survival_max", requirement.survival_max),
    ];

    for (name, value) in bounds {
        if let Some(value) = value
            && !value.is_finite()
        {
            return Err(ValidationError(format!(
                "{} must be a finite number",
                bound_label(name)
            )));
        }
    }

    for (index, (name, value)) in bounds.iter().enumerate() {
        let Some(value) = value else {
            continue;
        };

        for (later_name, later_value) in bounds.iter().skip(index + 1) {
            let Some(later_value) = later_value else {
                continue;
            };

            if value > later_value {
                return Err(ValidationError(format!(
                    "{} must not exceed {}",
                    bound_label(name),
                    bound_label(later_name)
                )));
            }
        }
    }

    Ok(())
}
