//! Change history.
//!
//! Every write OmaGrow keeps can leave one row per changed field (details) or per changed row
//! (a requirement, timing rule, attribute or stage). A row records the old value, the new value,
//! who did it and the source. That history is what makes direct agent writes safe: the review
//! moves from before the write to after it, with a revert per change.

use serde_json::{Map, Value, json};

use crate::plant_data::{Attribute, ProfileDetails, Requirement, Stage, TimingRule};
use crate::provenance;
use crate::store::{self, Store};
use crate::units::UnitPreference;

pub const ACTOR_USER: &str = "user";
pub const ACTOR_AGENT: &str = "agent";

/// The profile detail fields a change can name.
pub const DETAIL_FIELDS: &[&str] = &[
    "name",
    "species",
    "cultivar",
    "family",
    "genus",
    "lifecycle",
    "life_form",
    "foliage",
    "determinacy",
    "growth_habit",
    "sun_exposure",
    "photoperiod_type",
    "photoperiod_obligate",
    "critical_day_length_hours",
    "water_need",
    "container_suitable",
    "min_container_volume_l_min",
    "min_container_volume_l_max",
    "days_to_maturity_min",
    "days_to_maturity_max",
    "notes",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub id: i64,
    pub profile_id: i64,
    pub entity_type: String,
    pub entity_id: i64,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub source_type: Option<String>,
    pub source_ref: Option<String>,
    pub actor: String,
    pub created_at: String,
}

/// One change to write, before it has an id or a timestamp.
pub struct NewChange<'a> {
    pub profile_id: i64,
    pub entity_type: &'a str,
    pub entity_id: i64,
    pub field: &'a str,
    pub old: Option<Value>,
    pub new: Option<Value>,
    pub actor: &'a str,
    pub source_type: Option<&'a str>,
    pub source_ref: Option<&'a str>,
}

/// Records a change unless nothing changed.
pub fn record(store: &Store, change: &NewChange<'_>) -> store::Result<()> {
    if change.old == change.new {
        return Ok(());
    }

    store.record_change(&Change {
        id: 0,
        profile_id: change.profile_id,
        entity_type: change.entity_type.to_string(),
        entity_id: change.entity_id,
        field: change.field.to_string(),
        old_value: change.old.as_ref().map(Value::to_string),
        new_value: change.new.as_ref().map(Value::to_string),
        source_type: change.source_type.map(str::to_string),
        source_ref: change.source_ref.map(str::to_string),
        actor: change.actor.to_string(),
        created_at: String::new(),
    })?;

    Ok(())
}

/// A one-line title for the history list.
pub fn title(change: &Change) -> String {
    match (change.entity_type.as_str(), change.field.as_str()) {
        ("profile", field) => humanize(field),
        ("stage", _) => format!(
            "Stage {}",
            label_of(
                change,
                change.old_value.as_deref(),
                change.new_value.as_deref()
            )
        ),
        ("requirement", _) => row_title(change, "metric", "Requirement"),
        ("timing_rule", _) => timing_row_title(change),
        ("attribute", _) => row_title(change, "key", "Attribute"),
        (other, _) => humanize(other),
    }
}

fn label_of(change: &Change, old: Option<&str>, new: Option<&str>) -> String {
    for value in [new, old].into_iter().flatten() {
        if let Ok(value) = serde_json::from_str::<Value>(value)
            && let Some(label) = value.get("label").and_then(Value::as_str)
        {
            return label.to_string();
        }
    }

    change.entity_id.to_string()
}

fn timing_row_title(change: &Change) -> String {
    for value in [change.new_value.as_deref(), change.old_value.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Ok(value) = serde_json::from_str::<Value>(value)
            && let Some(kind) = value.get("kind").and_then(Value::as_str)
        {
            return match kind {
                "sow_indoor" => "Sow indoors".to_string(),
                "sow_outdoor" => "Sow outdoors".to_string(),
                "winter_protect" => "Winter protection".to_string(),
                "succession" => "Succession sowing".to_string(),
                other => humanize(other),
            };
        }
    }

    "Timing".to_string()
}

fn row_title(change: &Change, key: &str, fallback: &str) -> String {
    for value in [change.new_value.as_deref(), change.old_value.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Ok(value) = serde_json::from_str::<Value>(value)
            && let Some(name) = value.get(key).and_then(Value::as_str)
        {
            let label = if key == "metric" {
                crate::plant_data::metric_label(name).to_string()
            } else {
                humanize(name)
            };
            let day_part = value.get("day_part").and_then(Value::as_str);
            return match day_part {
                Some(day_part) => format!("{label} · {day_part}"),
                None => label,
            };
        }
    }

    fallback.to_string()
}

/// A short verb for a row change: added, removed or changed.
pub fn summary(change: &Change, pref: UnitPreference) -> String {
    if change.field != "*" {
        return detail_text(change, pref);
    }

    let same = change.old_value == change.new_value;
    let old = change
        .old_value
        .as_deref()
        .map(|value| row_value(change, value, pref));
    let new = change
        .new_value
        .as_deref()
        .map(|value| row_value(change, value, pref));

    match (old, new) {
        (None, Some(new)) => format!("added · {new}"),
        (Some(old), None) => format!("removed · {old}"),
        (Some(_), Some(new)) if same => new,
        (Some(old), Some(new)) => format!("{old} → {new}"),
        (None, None) => "changed".to_string(),
    }
}

/// What the change did, in the user's words.
pub fn detail_text(change: &Change, pref: UnitPreference) -> String {
    let old = change
        .old_value
        .as_deref()
        .map(|value| text_of_detail(&change.field, value, pref))
        .unwrap_or_else(|| "empty".to_string());
    let new = change
        .new_value
        .as_deref()
        .map(|value| text_of_detail(&change.field, value, pref))
        .unwrap_or_else(|| "empty".to_string());
    format!("{old} → {new}")
}

fn text_of_detail(field: &str, value: &str, pref: UnitPreference) -> String {
    let unit = crate::units::detail_unit(field);
    if !unit.is_empty()
        && let Ok(Value::Number(number)) = serde_json::from_str::<Value>(value)
    {
        return pref.text(unit, number.as_f64().unwrap_or_default());
    }

    let text = text_of(value);
    match field {
        "lifecycle" | "life_form" | "foliage" | "determinacy" | "growth_habit" | "sun_exposure"
        | "photoperiod_type" | "water_need" => humanize(&text),
        _ => text,
    }
}

fn row_value(change: &Change, value: &str, pref: UnitPreference) -> String {
    let Ok(value) = serde_json::from_str::<Value>(value) else {
        return text_of(value);
    };

    match change.entity_type.as_str() {
        "stage" => stage_value(&value, pref),
        "requirement" => requirement_value(&value, pref),
        "timing_rule" => timing_value(&value),
        "attribute" => attribute_value(&value),
        _ => text_of(value.to_string().as_str()),
    }
}

fn stage_value(value: &Value, pref: UnitPreference) -> String {
    let label = value
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or("stage");
    match range(
        value.get("duration_days_min").and_then(Value::as_f64),
        value.get("duration_days_max").and_then(Value::as_f64),
        "days",
        pref,
    ) {
        Some(range) => format!("{label} · {range}"),
        None => label.to_string(),
    }
}

fn requirement_value(value: &Value, pref: UnitPreference) -> String {
    let metric = value
        .get("metric")
        .and_then(Value::as_str)
        .unwrap_or("value");
    let unit = crate::plant_data::metric_unit(metric);

    let optimum = range(
        value.get("opt_min").and_then(Value::as_f64),
        value.get("opt_max").and_then(Value::as_f64),
        unit,
        pref,
    );
    let growth = range(
        value.get("min").and_then(Value::as_f64),
        value.get("max").and_then(Value::as_f64),
        unit,
        pref,
    );
    let survival = range(
        value.get("survival_min").and_then(Value::as_f64),
        value.get("survival_max").and_then(Value::as_f64),
        unit,
        pref,
    );

    match (optimum, growth, survival) {
        (Some(optimum), _, _) => format!("optimum {optimum}"),
        (None, Some(growth), _) => growth,
        (None, None, Some(survival)) => format!("survival {survival}"),
        (None, None, None) => "not set".to_string(),
    }
}

fn timing_value(value: &Value) -> String {
    if value.get("anchor").and_then(Value::as_str) == Some("gdd") {
        let target = value.get("gdd_target").and_then(Value::as_f64);
        let base = value.get("gdd_base_c").and_then(Value::as_f64);

        return match (target, base) {
            (Some(target), Some(base)) => format!(
                "{} GDD above {} °C",
                crate::plant_data::format_number(target),
                crate::plant_data::format_number(base)
            ),
            (Some(target), None) => {
                format!("{} GDD", crate::plant_data::format_number(target))
            }
            (None, Some(base)) => {
                format!("GDD base {} °C", crate::plant_data::format_number(base))
            }
            (None, None) => "growing degree days".to_string(),
        };
    }

    let anchor = value
        .get("anchor")
        .and_then(Value::as_str)
        .map(anchor_label)
        .unwrap_or_else(|| "an anchor".to_string());
    let minimum = value.get("offset_min_days").and_then(Value::as_i64);
    let maximum = value.get("offset_max_days").and_then(Value::as_i64);

    match (minimum, maximum) {
        (Some(minimum), Some(maximum)) if minimum == maximum => offset_text(minimum, &anchor),
        (Some(minimum), Some(maximum)) if minimum < 0 && maximum < 0 => {
            format!("{}–{} days before {anchor}", maximum.abs(), minimum.abs())
        }
        (Some(minimum), Some(maximum)) if minimum >= 0 && maximum >= 0 => {
            format!("{minimum}–{maximum} days after {anchor}")
        }
        (Some(minimum), Some(maximum)) => {
            format!("{minimum} to {maximum} days relative to {anchor}")
        }
        (Some(value), None) | (None, Some(value)) => offset_text(value, &anchor),
        (None, None) => {
            if let Some(month) = value.get("month_start").and_then(Value::as_i64) {
                format!("month {month}")
            } else if let Some(hours) = value.get("day_length_hours").and_then(Value::as_f64) {
                format!("{} h day length", crate::plant_data::format_number(hours))
            } else {
                "no offset".to_string()
            }
        }
    }
}

fn offset_text(offset: i64, anchor: &str) -> String {
    if offset < 0 {
        format!("{} days before {anchor}", offset.abs())
    } else {
        format!("{offset} days after {anchor}")
    }
}

fn anchor_label(anchor: &str) -> String {
    match anchor {
        "last_frost" => "the last frost".to_string(),
        "first_frost" => "the first frost".to_string(),
        "stage_start" => "the stage start".to_string(),
        "day_length" => "the day length".to_string(),
        other => humanize(other),
    }
}

fn attribute_value(value: &Value) -> String {
    if let Some(number) = value.get("value_num").and_then(Value::as_f64) {
        return crate::plant_data::format_number(number);
    }
    if let Some(text) = value.get("value_text").and_then(Value::as_str) {
        return shorten(text, 90);
    }
    if let Some(flag) = value.get("value_bool").and_then(Value::as_bool) {
        return if flag { "yes" } else { "no" }.to_string();
    }

    "not set".to_string()
}

fn range(min: Option<f64>, max: Option<f64>, unit: &str, pref: UnitPreference) -> Option<String> {
    pref.range(min, max, unit)
}

fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut shortened: String = text.chars().take(limit.saturating_sub(1)).collect();
    shortened.push('…');
    shortened
}

fn text_of(value: &str) -> String {
    match serde_json::from_str::<Value>(value) {
        Ok(Value::String(text)) => text,
        Ok(Value::Null) => "empty".to_string(),
        Ok(Value::Bool(true)) => "yes".to_string(),
        Ok(Value::Bool(false)) => "no".to_string(),
        Ok(Value::Number(number)) => {
            crate::plant_data::format_number(number.as_f64().unwrap_or_default())
        }
        _ => value.to_string(),
    }
}

/// Snapshots, used for both recording and reverting.
pub fn detail(details: &ProfileDetails, field: &str) -> Value {
    match field {
        "name" => json!(details.name),
        "species" => json!(details.species),
        "cultivar" => json!(details.cultivar),
        "family" => json!(details.family),
        "genus" => json!(details.genus),
        "lifecycle" => json!(details.lifecycle),
        "life_form" => json!(details.life_form),
        "foliage" => json!(details.foliage),
        "determinacy" => json!(details.determinacy),
        "growth_habit" => json!(details.growth_habit),
        "sun_exposure" => json!(details.sun_exposure),
        "photoperiod_type" => json!(details.photoperiod_type),
        "photoperiod_obligate" => json!(details.photoperiod_obligate),
        "critical_day_length_hours" => json!(details.critical_day_length_hours),
        "water_need" => json!(details.water_need),
        "container_suitable" => json!(details.container_suitable),
        "min_container_volume_l_min" => json!(details.min_container_volume_l_min),
        "min_container_volume_l_max" => json!(details.min_container_volume_l_max),
        "days_to_maturity_min" => json!(details.days_to_maturity_min),
        "days_to_maturity_max" => json!(details.days_to_maturity_max),
        "notes" => json!(details.notes),
        _ => Value::Null,
    }
}

pub fn stage(stage: &Stage) -> Value {
    json!({
        "id": stage.id,
        "position": stage.position,
        "kind": stage.kind,
        "label": stage.label,
        "duration_days_min": stage.duration_days_min,
        "duration_days_max": stage.duration_days_max,
        "notes": stage.notes,
        "repeats": stage.repeats,
    })
}

pub fn requirement(requirement: &Requirement) -> Value {
    json!({
        "id": requirement.id,
        "stage_id": requirement.stage_id,
        "metric": requirement.metric,
        "day_part": requirement.day_part,
        "min": requirement.min,
        "max": requirement.max,
        "opt_min": requirement.opt_min,
        "opt_max": requirement.opt_max,
        "survival_min": requirement.survival_min,
        "survival_max": requirement.survival_max,
    })
}

pub fn timing(rule: &TimingRule) -> Value {
    json!({
        "id": rule.id,
        "stage_id": rule.stage_id,
        "kind": rule.kind,
        "anchor": rule.anchor,
        "offset_min_days": rule.offset_min_days,
        "offset_max_days": rule.offset_max_days,
        "month_start": rule.month_start,
        "month_end": rule.month_end,
        "day_length_hours": rule.day_length_hours,
        "gdd_base_c": rule.gdd_base_c,
        "gdd_target": rule.gdd_target,
        "repeat_days": rule.repeat_days,
        "notes": rule.notes,
    })
}

pub fn attribute(attribute: &Attribute) -> Value {
    json!({
        "id": attribute.id,
        "stage_id": attribute.stage_id,
        "key": attribute.key,
        "value_num": attribute.value_num,
        "value_text": attribute.value_text,
        "value_bool": attribute.value_bool,
    })
}

/// Puts a recorded old value back, through the same validated paths.
///
/// Refuses when the value changed again after the change being reverted.
pub fn revert(store: &Store, change: &Change) -> Result<String, String> {
    if change.field != "*" {
        return revert_detail(store, change);
    }

    match change.entity_type.as_str() {
        "stage" => revert_stage(store, change),
        "requirement" => revert_requirement(store, change),
        "timing_rule" => revert_timing(store, change),
        "attribute" => revert_attribute(store, change),
        other => Err(format!("cannot revert a {other} change yet")),
    }
}

fn revert_detail(store: &Store, change: &Change) -> Result<String, String> {
    let mut details = store
        .profile_details(change.profile_id)
        .map_err(|error| error.to_string())?
        .ok_or("this profile no longer exists")?;

    let current = detail(&details, &change.field);
    let recorded = parse(change.new_value.as_deref())?;
    if current != recorded {
        return Err("this value changed after that; revert the newer change instead".to_string());
    }

    let old = parse(change.old_value.as_deref())?;
    set_detail(&mut details, &change.field, &old)?;
    store
        .transaction(move |store| {
            store.update_profile_details(&details)?;
            store.update_profile(
                change.profile_id,
                &details.name,
                details.species.as_deref(),
                details.cultivar.as_deref(),
            )?;

            if !matches!(change.field.as_str(), "name" | "species" | "cultivar") {
                if matches!(old, Value::Null) {
                    store.delete_provenance("profile", change.profile_id, &change.field)?;
                } else {
                    provenance::mark_user(store, "profile", change.profile_id, &[&change.field])?;
                }
            }

            record(
                store,
                &NewChange {
                    profile_id: change.profile_id,
                    entity_type: "profile",
                    entity_id: change.profile_id,
                    field: &change.field,
                    old: Some(current),
                    new: Some(old),
                    actor: ACTOR_USER,
                    source_type: Some("user"),
                    source_ref: None,
                },
            )
        })
        .map_err(|error| error.to_string())?;

    Ok(format!("{} reverted", title(change)))
}

fn same_stage(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            // Compare the values a stage actually has, so a snapshot from before a new field
            // existed (missing key) matches a snapshot that writes the field as null. Position is
            // cycle order, not a value; identity is the same row either way.
            let (Ok(mut left), Ok(mut right)) = (stage_from(left, 0), stage_from(right, 0)) else {
                return false;
            };
            left.id = 0;
            right.id = 0;
            left.position = 0;
            right.position = 0;
            left == right
        }
        (None, None) => true,
        _ => false,
    }
}

fn revert_stage(store: &Store, change: &Change) -> Result<String, String> {
    let current = store
        .stage(change.entity_id)
        .map_err(|error| error.to_string())?;
    let current_value = current.as_ref().map(stage);
    let recorded = expected(change.new_value.as_deref())?;

    // Position is cycle order, not a value: reordering must not make an earlier stage edit
    // impossible to revert.
    if !same_stage(current_value.as_ref(), recorded.as_ref()) {
        return Err("this stage changed after that; revert the newer change instead".to_string());
    }

    store
        .transaction(move |store| {
            let restored_id = match parse(change.old_value.as_deref())? {
                Value::Null => {
                    // The store refuses a stage that still has its own values.
                    store.delete_stage(change.entity_id)?;
                    None
                }
                old => {
                    let mut stage = stage_from(&old, change.profile_id)?;
                    let id = if let Some(current_stage) = &current {
                        stage.id = change.entity_id;
                        stage.position = current_stage.position;
                        store.update_stage(&stage)?;
                        change.entity_id
                    } else {
                        let desired_position = stage.position.max(1);
                        stage.id = 0;
                        let stages = store.stages(change.profile_id)?;
                        stage.position =
                            stages.iter().map(|stage| stage.position).max().unwrap_or(0) + 1;
                        let id = store.create_stage(&stage)?;

                        let mut order: Vec<i64> = store
                            .stages(change.profile_id)?
                            .into_iter()
                            .filter(|stage| stage.id != id)
                            .map(|stage| stage.id)
                            .collect();
                        let target = ((desired_position - 1) as usize).min(order.len());
                        order.insert(target, id);
                        store.reorder_stages(change.profile_id, &order)?;
                        id
                    };
                    Some(id)
                }
            };

            if let Some(id) = restored_id {
                provenance::mark_user(store, "stage", id, &["*"])?;
            }

            record_revert(
                store,
                change,
                current_value,
                parse(change.old_value.as_deref())?,
            )
        })
        .map_err(|error| error.to_string())?;

    Ok(format!("{} reverted", title(change)))
}

fn revert_requirement(store: &Store, change: &Change) -> Result<String, String> {
    let profile_id = change.profile_id;
    let current = store
        .requirements(profile_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|requirement| requirement.id == change.entity_id);
    let current_value = current.as_ref().map(requirement);

    if current_value != expected(change.new_value.as_deref())? {
        return Err("this value changed after that; revert the newer change instead".to_string());
    }

    store
        .transaction(move |store| {
            let restored_id = match parse(change.old_value.as_deref())? {
                Value::Null => {
                    if current.is_some() {
                        store.delete_requirement(change.entity_id)?;
                    }
                    None
                }
                old => {
                    let requirement = requirement_from(&old, profile_id)?;
                    Some(store.set_requirement(&requirement)?)
                }
            };

            if let Some(id) = restored_id {
                provenance::mark_user(store, "requirement", id, &["*"])?;
            }

            record_revert(
                store,
                change,
                current_value,
                parse(change.old_value.as_deref())?,
            )
        })
        .map_err(|error| error.to_string())?;
    Ok(format!("{} reverted", title(change)))
}

fn revert_timing(store: &Store, change: &Change) -> Result<String, String> {
    let profile_id = change.profile_id;
    let current = store
        .timing_rules(profile_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|rule| rule.id == change.entity_id);
    let current_value = current.as_ref().map(timing);

    if current_value != expected(change.new_value.as_deref())? {
        return Err("this rule changed after that; revert the newer change instead".to_string());
    }

    store
        .transaction(move |store| {
            let restored_id = match parse(change.old_value.as_deref())? {
                Value::Null => {
                    if current.is_some() {
                        store.delete_timing_rule(change.entity_id)?;
                    }
                    None
                }
                old => {
                    let mut rule = timing_from(&old, profile_id)?;
                    if current.is_some() {
                        rule.id = change.entity_id;
                        store.update_timing_rule(&rule)?;
                        Some(change.entity_id)
                    } else {
                        rule.id = 0;
                        Some(store.create_timing_rule(&rule)?)
                    }
                }
            };

            if let Some(id) = restored_id {
                provenance::mark_user(store, "timing_rule", id, &["*"])?;
            }

            record_revert(
                store,
                change,
                current_value,
                parse(change.old_value.as_deref())?,
            )
        })
        .map_err(|error| error.to_string())?;
    Ok(format!("{} reverted", title(change)))
}

fn revert_attribute(store: &Store, change: &Change) -> Result<String, String> {
    let profile_id = change.profile_id;
    let current = store
        .attributes(profile_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|attribute| attribute.id == change.entity_id);
    let current_value = current.as_ref().map(attribute);

    if current_value != expected(change.new_value.as_deref())? {
        return Err("this value changed after that; revert the newer change instead".to_string());
    }

    store
        .transaction(move |store| {
            let restored_id = match parse(change.old_value.as_deref())? {
                Value::Null => {
                    if current.is_some() {
                        store.delete_attribute(change.entity_id)?;
                    }
                    None
                }
                old => {
                    let attribute = attribute_from(&old, profile_id)?;
                    Some(store.set_attribute(&attribute)?)
                }
            };

            if let Some(id) = restored_id {
                provenance::mark_user(store, "attribute", id, &["*"])?;
            }

            record_revert(
                store,
                change,
                current_value,
                parse(change.old_value.as_deref())?,
            )
        })
        .map_err(|error| error.to_string())?;
    Ok(format!("{} reverted", title(change)))
}

fn record_revert(
    store: &Store,
    change: &Change,
    current: Option<Value>,
    old: Value,
) -> crate::store::Result<()> {
    record(
        store,
        &NewChange {
            profile_id: change.profile_id,
            entity_type: &change.entity_type,
            entity_id: change.entity_id,
            field: "*",
            old: current,
            new: Some(old),
            actor: ACTOR_USER,
            source_type: Some("user"),
            source_ref: None,
        },
    )
}

/// The recorded "new" state as an optional row: `null` means the row was removed.
fn expected(value: Option<&str>) -> Result<Option<Value>, String> {
    Ok(match parse(value)? {
        Value::Null => None,
        value => Some(value),
    })
}

fn parse(value: Option<&str>) -> Result<Value, String> {
    match value {
        Some(text) => serde_json::from_str(text).map_err(|error| format!("bad history: {error}")),
        None => Ok(Value::Null),
    }
}

fn set_detail(details: &mut ProfileDetails, field: &str, value: &Value) -> Result<(), String> {
    let text = || match value {
        Value::String(text) => Ok(Some(text.clone())),
        Value::Null => Ok(None),
        _ => Err(format!("{field} does not take that value")),
    };
    let number = || match value {
        Value::Number(number) => Ok(number.as_f64()),
        Value::Null => Ok(None),
        _ => Err(format!("{field} does not take that value")),
    };
    let flag = || match value {
        Value::Bool(flag) => Ok(Some(*flag)),
        Value::Null => Ok(None),
        _ => Err(format!("{field} does not take that value")),
    };

    match field {
        "name" => details.name = text()?.unwrap_or_default(),
        "species" => details.species = text()?,
        "cultivar" => details.cultivar = text()?,
        "family" => details.family = text()?,
        "genus" => details.genus = text()?,
        "lifecycle" => details.lifecycle = text()?,
        "life_form" => details.life_form = text()?,
        "foliage" => details.foliage = text()?,
        "determinacy" => details.determinacy = text()?,
        "growth_habit" => details.growth_habit = text()?,
        "sun_exposure" => details.sun_exposure = text()?,
        "photoperiod_type" => details.photoperiod_type = text()?,
        "photoperiod_obligate" => details.photoperiod_obligate = flag()?,
        "critical_day_length_hours" => details.critical_day_length_hours = number()?,
        "water_need" => details.water_need = text()?,
        "container_suitable" => details.container_suitable = flag()?,
        "min_container_volume_l_min" => details.min_container_volume_l_min = number()?,
        "min_container_volume_l_max" => details.min_container_volume_l_max = number()?,
        "days_to_maturity_min" => details.days_to_maturity_min = number()?,
        "days_to_maturity_max" => details.days_to_maturity_max = number()?,
        "notes" => details.notes = text()?,
        other => return Err(format!("{other} is not a detail field")),
    }

    Ok(())
}

fn stage_from(value: &Value, profile_id: i64) -> Result<Stage, String> {
    let object = value.as_object().ok_or("bad stage history")?;
    Ok(Stage {
        id: int(object, "id").unwrap_or(0),
        profile_id,
        position: int(object, "position").unwrap_or(1),
        kind: string(object, "kind").ok_or("bad stage history")?,
        label: string(object, "label").ok_or("bad stage history")?,
        duration_days_min: number(object, "duration_days_min"),
        duration_days_max: number(object, "duration_days_max"),
        notes: string(object, "notes"),
        repeats: bool_of(object, "repeats"),
    })
}

fn requirement_from(value: &Value, profile_id: i64) -> Result<Requirement, String> {
    let object = value.as_object().ok_or("bad requirement history")?;
    Ok(Requirement {
        id: int(object, "id").unwrap_or(0),
        profile_id,
        stage_id: int(object, "stage_id"),
        metric: string(object, "metric").ok_or("bad requirement history")?,
        day_part: string(object, "day_part").ok_or("bad requirement history")?,
        min: number(object, "min"),
        max: number(object, "max"),
        opt_min: number(object, "opt_min"),
        opt_max: number(object, "opt_max"),
        survival_min: number(object, "survival_min"),
        survival_max: number(object, "survival_max"),
    })
}

fn timing_from(value: &Value, profile_id: i64) -> Result<TimingRule, String> {
    let object = value.as_object().ok_or("bad timing history")?;
    Ok(TimingRule {
        id: int(object, "id").unwrap_or(0),
        profile_id,
        stage_id: int(object, "stage_id"),
        kind: string(object, "kind").ok_or("bad timing history")?,
        anchor: string(object, "anchor").ok_or("bad timing history")?,
        offset_min_days: int(object, "offset_min_days"),
        offset_max_days: int(object, "offset_max_days"),
        month_start: int(object, "month_start"),
        month_end: int(object, "month_end"),
        day_length_hours: number(object, "day_length_hours"),
        gdd_base_c: number(object, "gdd_base_c"),
        gdd_target: number(object, "gdd_target"),
        repeat_days: int(object, "repeat_days"),
        notes: string(object, "notes"),
    })
}

fn attribute_from(value: &Value, profile_id: i64) -> Result<Attribute, String> {
    let object = value.as_object().ok_or("bad attribute history")?;
    Ok(Attribute {
        id: int(object, "id").unwrap_or(0),
        profile_id,
        stage_id: int(object, "stage_id"),
        key: string(object, "key").ok_or("bad attribute history")?,
        value_num: number(object, "value_num"),
        value_text: string(object, "value_text"),
        value_bool: bool_of(object, "value_bool"),
    })
}

fn string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_string)
}

fn number(object: &Map<String, Value>, key: &str) -> Option<f64> {
    object.get(key).and_then(Value::as_f64)
}

fn int(object: &Map<String, Value>, key: &str) -> Option<i64> {
    object.get(key).and_then(Value::as_i64)
}

fn bool_of(object: &Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key).and_then(Value::as_bool)
}

fn humanize(value: &str) -> String {
    let text = value.replace('_', " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ACTOR_AGENT, ACTOR_USER, Change, NewChange, detail, record, requirement, revert, stage,
        summary, title,
    };
    use crate::plant_data::{Requirement, Stage};
    use crate::store::Store;
    use crate::units::UnitPreference;
    use serde_json::json;

    fn store_with_profile() -> (Store, i64) {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();
        (store, id)
    }

    #[test]
    fn records_and_reverts_a_detail_field() {
        let (store, id) = store_with_profile();
        let mut details = store.profile_details(id).unwrap().unwrap();
        details.lifecycle = Some("perennial".to_string());
        store.update_profile_details(&details).unwrap();

        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "profile",
                entity_id: id,
                field: "lifecycle",
                old: Some(json!(null)),
                new: Some(json!("perennial")),
                actor: ACTOR_AGENT,
                source_type: Some("website"),
                source_ref: Some("https://example.org"),
            },
        )
        .unwrap();

        let changes = store.changes(id, 10).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].new_value.as_deref(), Some("\"perennial\""));
        assert_eq!(title(&changes[0]), "Lifecycle");

        revert(&store, &changes[0]).unwrap();
        assert_eq!(store.profile_details(id).unwrap().unwrap().lifecycle, None);
        assert!(
            store
                .provenance("profile", id, "lifecycle")
                .unwrap()
                .is_none(),
            "reverting to empty clears the status"
        );

        let history = store.changes(id, 10).unwrap();
        assert_eq!(history.len(), 2, "the revert is recorded too");
        assert_eq!(history[0].actor, ACTOR_USER);
    }

    #[test]
    fn records_and_reverts_an_identity_change() {
        let (store, id) = store_with_profile();
        let before = store.profile_details(id).unwrap().unwrap();
        store
            .update_profile(id, "New name", Some("Capsicum annuum"), Some("Rawit"))
            .unwrap();
        let after = store.profile_details(id).unwrap().unwrap();

        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "profile",
                entity_id: id,
                field: "name",
                old: Some(detail(&before, "name")),
                new: Some(detail(&after, "name")),
                actor: ACTOR_USER,
                source_type: Some("user"),
                source_ref: None,
            },
        )
        .unwrap();

        let change = store.changes(id, 1).unwrap().remove(0);
        revert(&store, &change).unwrap();
        assert_eq!(store.profile(id).unwrap().unwrap().name, "Chilli");
    }

    #[test]
    fn a_revert_to_a_non_empty_value_is_confirmed_for_the_user() {
        let (store, id) = store_with_profile();
        let mut details = store.profile_details(id).unwrap().unwrap();
        details.lifecycle = Some("perennial".to_string());
        store.update_profile_details(&details).unwrap();

        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "profile",
                entity_id: id,
                field: "lifecycle",
                old: Some(json!("annual")),
                new: Some(json!("perennial")),
                actor: ACTOR_AGENT,
                source_type: Some("website"),
                source_ref: Some("https://example.org"),
            },
        )
        .unwrap();

        let change = store.changes(id, 1).unwrap().remove(0);
        revert(&store, &change).unwrap();

        assert_eq!(
            store
                .profile_details(id)
                .unwrap()
                .unwrap()
                .lifecycle
                .as_deref(),
            Some("annual")
        );
        let provenance = store
            .provenance("profile", id, "lifecycle")
            .unwrap()
            .unwrap();
        assert_eq!(provenance.status, "confirmed");
        assert_eq!(provenance.source_type.as_deref(), Some("user"));
    }

    #[test]
    fn gdd_timing_history_shows_the_base_and_target() {
        let change = Change {
            id: 3,
            profile_id: 1,
            entity_type: "timing_rule".to_string(),
            entity_id: 1,
            field: "*".to_string(),
            old_value: None,
            new_value: Some(
                json!({
                    "kind": "harvest",
                    "anchor": "gdd",
                    "gdd_base_c": 20,
                    "gdd_target": 1000
                })
                .to_string(),
            ),
            source_type: Some("book".to_string()),
            source_ref: None,
            actor: ACTOR_AGENT.to_string(),
            created_at: String::new(),
        };

        assert_eq!(
            summary(&change, UnitPreference::Metric),
            "added · 1000 GDD above 20 °C"
        );

        let edited = Change {
            old_value: Some(
                json!({
                    "kind": "harvest",
                    "anchor": "gdd",
                    "gdd_base_c": 20,
                    "gdd_target": 900
                })
                .to_string(),
            ),
            new_value: Some(
                json!({
                    "kind": "harvest",
                    "anchor": "gdd",
                    "gdd_base_c": 20,
                    "gdd_target": 1000
                })
                .to_string(),
            ),
            ..change
        };
        assert_eq!(
            summary(&edited, UnitPreference::Metric),
            "900 GDD above 20 °C → 1000 GDD above 20 °C"
        );
    }

    #[test]
    fn requirement_and_volume_history_follow_the_preference() {
        let requirement = Change {
            id: 1,
            profile_id: 1,
            entity_type: "requirement".to_string(),
            entity_id: 1,
            field: "*".to_string(),
            old_value: None,
            new_value: Some(
                json!({
                    "metric": "air_temp",
                    "min": 10,
                    "max": 30,
                    "opt_min": 20,
                    "opt_max": 26,
                    "survival_min": 0,
                    "survival_max": 35
                })
                .to_string(),
            ),
            source_type: Some("book".to_string()),
            source_ref: None,
            actor: ACTOR_AGENT.to_string(),
            created_at: String::new(),
        };

        assert!(summary(&requirement, UnitPreference::Imperial).contains("optimum 68–78.8 °F"));

        let volume = Change {
            id: 2,
            profile_id: 1,
            entity_type: "profile".to_string(),
            entity_id: 1,
            field: "min_container_volume_l_min".to_string(),
            old_value: Some(json!(10.0).to_string()),
            new_value: Some(json!(20.0).to_string()),
            source_type: Some("user".to_string()),
            source_ref: None,
            actor: ACTOR_USER.to_string(),
            created_at: String::new(),
        };

        assert_eq!(
            summary(&volume, UnitPreference::Imperial),
            "2.64 US gal → 5.28 US gal"
        );
        assert_eq!(summary(&volume, UnitPreference::Metric), "10 L → 20 L");
    }

    #[test]
    fn timing_ranges_keep_the_earliest_offset_first() {
        let change = Change {
            id: 1,
            profile_id: 1,
            entity_type: "timing_rule".to_string(),
            entity_id: 1,
            field: "*".to_string(),
            old_value: None,
            new_value: Some(
                json!({
                    "kind": "sow_indoor",
                    "anchor": "last_frost",
                    "offset_min_days": -56,
                    "offset_max_days": -42
                })
                .to_string(),
            ),
            source_type: Some("book".to_string()),
            source_ref: None,
            actor: ACTOR_AGENT.to_string(),
            created_at: String::new(),
        };

        assert_eq!(
            summary(&change, UnitPreference::Metric),
            "added · 42–56 days before the last frost"
        );
    }

    #[test]
    fn refuses_to_revert_over_a_newer_value() {
        let (store, id) = store_with_profile();
        let mut details = store.profile_details(id).unwrap().unwrap();
        details.lifecycle = Some("perennial".to_string());
        store.update_profile_details(&details).unwrap();

        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "profile",
                entity_id: id,
                field: "lifecycle",
                old: Some(json!(null)),
                new: Some(json!("perennial")),
                actor: ACTOR_AGENT,
                source_type: Some("website"),
                source_ref: Some("https://example.org"),
            },
        )
        .unwrap();

        details.lifecycle = Some("annual".to_string());
        store.update_profile_details(&details).unwrap();

        let changes = store.changes(id, 10).unwrap();
        let error = revert(&store, &changes[0]).unwrap_err();
        assert!(error.contains("changed after"), "{error}");
        assert_eq!(
            store
                .profile_details(id)
                .unwrap()
                .unwrap()
                .lifecycle
                .as_deref(),
            Some("annual")
        );
    }

    #[test]
    fn records_and_reverts_a_requirement_row() {
        let (store, id) = store_with_profile();
        let requirement_id = store
            .set_requirement(&Requirement {
                id: 0,
                profile_id: id,
                stage_id: None,
                metric: "air_temp".to_string(),
                day_part: "day".to_string(),
                min: Some(10.0),
                max: Some(30.0),
                opt_min: Some(18.0),
                opt_max: Some(26.0),
                survival_min: None,
                survival_max: None,
            })
            .unwrap();

        let current = store
            .requirements(id)
            .unwrap()
            .into_iter()
            .find(|item| item.id == requirement_id)
            .unwrap();

        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "requirement",
                entity_id: requirement_id,
                field: "*",
                old: Some(json!(null)),
                new: Some(requirement(&current)),
                actor: ACTOR_AGENT,
                source_type: Some("book"),
                source_ref: Some("RHS"),
            },
        )
        .unwrap();

        let change = store.changes(id, 1).unwrap().remove(0);
        assert_eq!(title(&change), "Air temperature · day");
        revert(&store, &change).unwrap();
        assert!(store.requirements(id).unwrap().is_empty());
    }

    #[test]
    fn a_reorder_does_not_block_reverting_a_stage_value() {
        let (store, id) = store_with_profile();
        let first = store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "seedling".to_string(),
                label: "Seedling".to_string(),
                duration_days_min: Some(10.0),
                duration_days_max: None,
                notes: None,
                repeats: None,
            })
            .unwrap();
        store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 2,
                kind: "harvest".to_string(),
                label: "Harvest".to_string(),
                duration_days_min: None,
                duration_days_max: None,
                notes: None,
                repeats: None,
            })
            .unwrap();

        let before = store.stage(first).unwrap().unwrap();
        let mut changed = before.clone();
        changed.duration_days_max = Some(20.0);
        changed.repeats = Some(true);
        store.update_stage(&changed).unwrap();
        let after = store.stage(first).unwrap().unwrap();
        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "stage",
                entity_id: first,
                field: "*",
                old: Some(stage(&before)),
                new: Some(stage(&after)),
                actor: ACTOR_USER,
                source_type: Some("user"),
                source_ref: None,
            },
        )
        .unwrap();

        store.move_stage(first, 1).unwrap();
        let change = store.changes(id, 1).unwrap().remove(0);
        revert(&store, &change).unwrap();

        let restored = store.stage(first).unwrap().unwrap();
        assert_eq!(restored.duration_days_max, None);
        assert_eq!(restored.repeats, None, "the repeat marker is reverted too");
        assert_eq!(restored.position, 2, "the reorder is kept");
    }

    #[test]
    fn a_stage_snapshot_from_before_the_repeat_marker_still_reverts() {
        let (store, id) = store_with_profile();
        let stage_id = store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "flowering".to_string(),
                label: "Flowering".to_string(),
                duration_days_min: Some(30.0),
                duration_days_max: None,
                notes: None,
                repeats: None,
            })
            .unwrap();

        let before = store.stage(stage_id).unwrap().unwrap();
        let mut changed = before.clone();
        changed.label = "Bloom".to_string();
        store.update_stage(&changed).unwrap();

        // A schema-14 history row had no `repeats` key at all.
        let old_snapshot = json!({
            "id": stage_id,
            "position": 1,
            "kind": "flowering",
            "label": "Bloom",
            "duration_days_min": 30.0,
            "duration_days_max": null,
            "notes": null,
        });
        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "stage",
                entity_id: stage_id,
                field: "*",
                old: Some(stage(&before)),
                new: Some(old_snapshot),
                actor: ACTOR_USER,
                source_type: Some("user"),
                source_ref: None,
            },
        )
        .unwrap();

        let change = store.changes(id, 1).unwrap().remove(0);
        revert(&store, &change).unwrap();
        assert_eq!(store.stage(stage_id).unwrap().unwrap().label, "Flowering");
    }

    #[test]
    fn reverting_a_stage_with_children_is_refused() {
        let (store, id) = store_with_profile();
        let stage_id = store
            .create_stage(&Stage {
                id: 0,
                profile_id: id,
                position: 1,
                kind: "flowering".to_string(),
                label: "Flowering".to_string(),
                duration_days_min: Some(30.0),
                duration_days_max: None,
                notes: None,
                repeats: None,
            })
            .unwrap();

        let created = store.stage(stage_id).unwrap().unwrap();
        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "stage",
                entity_id: stage_id,
                field: "*",
                old: Some(json!(null)),
                new: Some(stage(&created)),
                actor: ACTOR_AGENT,
                source_type: None,
                source_ref: None,
            },
        )
        .unwrap();

        store
            .set_requirement(&Requirement {
                id: 0,
                profile_id: id,
                stage_id: Some(stage_id),
                metric: "rh".to_string(),
                day_part: "any".to_string(),
                min: Some(40.0),
                max: None,
                opt_min: None,
                opt_max: None,
                survival_min: None,
                survival_max: None,
            })
            .unwrap();

        let change = store.changes(id, 1).unwrap().remove(0);
        let error = revert(&store, &change).unwrap_err();
        assert!(error.contains("own values"), "{error}");
        assert!(store.stage(stage_id).unwrap().is_some());
    }

    #[test]
    fn a_deleted_requirement_comes_back() {
        let (store, id) = store_with_profile();
        let requirement_id = store
            .set_requirement(&Requirement {
                id: 0,
                profile_id: id,
                stage_id: None,
                metric: "rh".to_string(),
                day_part: "any".to_string(),
                min: Some(40.0),
                max: Some(70.0),
                opt_min: None,
                opt_max: None,
                survival_min: None,
                survival_max: None,
            })
            .unwrap();
        let before = store
            .requirements(id)
            .unwrap()
            .into_iter()
            .find(|item| item.id == requirement_id)
            .unwrap();

        store.delete_requirement(requirement_id).unwrap();

        record(
            &store,
            &NewChange {
                profile_id: id,
                entity_type: "requirement",
                entity_id: requirement_id,
                field: "*",
                old: Some(requirement(&before)),
                new: Some(json!(null)),
                actor: ACTOR_USER,
                source_type: Some("user"),
                source_ref: None,
            },
        )
        .unwrap();

        let change = store.changes(id, 1).unwrap().remove(0);
        revert(&store, &change).unwrap();

        let restored = store.requirements(id).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].max, Some(70.0));
    }

    #[test]
    fn the_detail_snapshot_round_trips() {
        let (store, id) = store_with_profile();
        let details = store.profile_details(id).unwrap().unwrap();
        assert_eq!(detail(&details, "lifecycle"), json!(null));
        assert_eq!(detail(&details, "unknown"), json!(null));
    }
}
